//! Keyboard shortcuts: the [`Chord`] type (a key plus modifiers), the [`Keymap`] that binds chords
//! to commands, conflict detection and a small text format for saving customisations.
//!
//! Key names are independent of any GUI library. The GUI maps its own key type to these names.

use crate::commands::{CommandId, COMMANDS};
use std::collections::BTreeMap;
use std::fmt;

/// A key together with the modifiers that must be held. `key` is a canonical name such as
/// `"Z"`, `"5"`, `"F1"`, `"Space"`, `"Delete"`, `"ArrowLeft"`, `"Comma"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

/// Names accepted for keys that are not a single letter or digit.
const NAMED_KEYS: &[&str] = &[
    "Space", "Enter", "Escape", "Tab", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown", "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Comma", "Period", "Slash", "Backslash", "Minus", "Equals", "Semicolon", "Quote", "OpenBracket", "CloseBracket", "Backtick",
];

fn canonical_key(raw: &str) -> Option<String> {
    let r = raw.trim();
    if r.chars().count() == 1 {
        let c = r.chars().next()?;
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase().to_string());
        }
        if c.is_ascii_digit() {
            return Some(c.to_string());
        }
        return None;
    }
    if let Some(n) = NAMED_KEYS.iter().find(|n| n.eq_ignore_ascii_case(r)) {
        return Some((*n).to_string());
    }
    // F1..F24
    let up = r.to_ascii_uppercase();
    if let Some(num) = up.strip_prefix('F') {
        if let Ok(n) = num.parse::<u32>() {
            if (1..=24).contains(&n) {
                return Some(format!("F{n}"));
            }
        }
    }
    None
}

impl Chord {
    pub fn new(ctrl: bool, shift: bool, alt: bool, key: &str) -> Option<Chord> {
        Some(Chord { ctrl, shift, alt, key: canonical_key(key)? })
    }

    /// Parses `"Ctrl+Shift+Z"`, `"alt+f4"`, `"Space"` (case-insensitive, any modifier order).
    pub fn parse(text: &str) -> Result<Chord, String> {
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        let mut key = None;
        for part in text.split('+') {
            let p = part.trim();
            match p.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                "" => return Err(format!("empty part in shortcut \"{text}\"")),
                _ => {
                    if key.is_some() {
                        return Err(format!("shortcut \"{text}\" has more than one key"));
                    }
                    key = Some(canonical_key(p).ok_or_else(|| format!("unknown key \"{p}\" in shortcut \"{text}\""))?);
                }
            }
        }
        Ok(Chord { ctrl, shift, alt, key: key.ok_or_else(|| format!("shortcut \"{text}\" has no key"))? })
    }

    /// True when this chord uses no modifier at all (a bare key). Such shortcuts must not fire
    /// while the user types into a text field; the GUI checks this.
    pub fn is_bare(&self) -> bool {
        !self.ctrl && !self.shift && !self.alt
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        f.write_str(&self.key)
    }
}

/// Binding table. Every command always has an entry (possibly empty = unbound).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<CommandId, Vec<Chord>>,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut bindings = BTreeMap::new();
        for c in COMMANDS {
            let chords = c.default_chords.iter().map(|s| Chord::parse(s).expect("default shortcut in the catalogue must parse")).collect();
            bindings.insert(c.id, chords);
        }
        Keymap { bindings }
    }
}

impl Keymap {
    pub fn chords(&self, id: CommandId) -> &[Chord] {
        self.bindings.get(&id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// First chord of a command as text for menus and tooltips, or an empty string.
    pub fn label(&self, id: CommandId) -> String {
        self.chords(id).first().map(|c| c.to_string()).unwrap_or_default()
    }

    /// Replaces all chords of a command.
    pub fn set(&mut self, id: CommandId, chords: Vec<Chord>) {
        self.bindings.insert(id, chords);
    }

    /// Restores one command to its default shortcuts.
    pub fn reset(&mut self, id: CommandId) {
        let fresh = Keymap::default();
        self.bindings.insert(id, fresh.chords(id).to_vec());
    }

    pub fn reset_all(&mut self) {
        *self = Keymap::default();
    }

    pub fn is_default(&self, id: CommandId) -> bool {
        Keymap::default().chords(id) == self.chords(id)
    }

    /// All commands bound to this chord (more than one means a conflict).
    pub fn commands_for(&self, chord: &Chord) -> Vec<CommandId> {
        self.bindings.iter().filter(|(_, v)| v.contains(chord)).map(|(k, _)| *k).collect()
    }

    /// Chords that trigger more than one command, with the commands involved.
    pub fn conflicts(&self) -> Vec<(Chord, Vec<CommandId>)> {
        let mut by_chord: BTreeMap<&Chord, Vec<CommandId>> = BTreeMap::new();
        for (id, chords) in &self.bindings {
            for c in chords {
                by_chord.entry(c).or_default().push(*id);
            }
        }
        by_chord.into_iter().filter(|(_, v)| v.len() > 1).map(|(c, v)| (c.clone(), v)).collect()
    }

    /// Binds `chord` to `id` as its only shortcut and removes the chord from every other command,
    /// returning the commands that lost it so the GUI can tell the user.
    pub fn assign_exclusive(&mut self, id: CommandId, chord: Chord) -> Vec<CommandId> {
        let mut stolen = vec![];
        for (other, chords) in self.bindings.iter_mut() {
            if *other != id && chords.contains(&chord) {
                chords.retain(|c| c != &chord);
                stolen.push(*other);
            }
        }
        self.bindings.insert(id, vec![chord]);
        stolen
    }

    /// Serialises only the differences from the defaults, one `name = chord, chord` line each.
    /// A command with no shortcut is written as `name =`.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for c in COMMANDS {
            if !self.is_default(c.id) {
                let list: Vec<String> = self.chords(c.id).iter().map(|x| x.to_string()).collect();
                s.push_str(&format!("{} = {}\n", c.name, list.join(", ")));
            }
        }
        s
    }

    /// Applies customisations produced by [`Keymap::to_text`]. Unknown commands and unreadable
    /// shortcuts are skipped and reported in the returned list instead of failing the whole load.
    pub fn apply_text(&mut self, text: &str) -> Vec<String> {
        let mut problems = vec![];
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, rest)) = line.split_once('=') else {
                problems.push(format!("line {}: missing '='", n + 1));
                continue;
            };
            let Some(id) = CommandId::from_stable_name(name.trim()) else {
                problems.push(format!("line {}: unknown command \"{}\"", n + 1, name.trim()));
                continue;
            };
            let mut chords = vec![];
            let mut ok = true;
            for part in rest.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                match Chord::parse(part) {
                    Ok(c) => chords.push(c),
                    Err(e) => {
                        problems.push(format!("line {}: {}", n + 1, e));
                        ok = false;
                    }
                }
            }
            if ok {
                self.set(id, chords);
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_canonically() {
        let c = Chord::parse("shift+ctrl+z").unwrap();
        assert_eq!(c.to_string(), "Ctrl+Shift+Z");
        assert_eq!(Chord::parse("f1").unwrap().key, "F1");
        assert_eq!(Chord::parse("space").unwrap().to_string(), "Space");
        assert!(Chord::parse("Ctrl+").is_err());
        assert!(Chord::parse("Ctrl+A+B").is_err());
        assert!(Chord::parse("Ctrl+Banana").is_err());
        assert!(Chord::parse("Ctrl").is_err());
    }

    #[test]
    fn default_keymap_has_no_conflicts() {
        let k = Keymap::default();
        assert!(k.conflicts().is_empty(), "default shortcuts collide: {:?}", k.conflicts());
    }

    #[test]
    fn redo_has_two_shortcuts_and_lookup_works() {
        let k = Keymap::default();
        assert_eq!(k.chords(CommandId::Redo).len(), 2);
        let c = Chord::parse("Ctrl+Shift+Z").unwrap();
        assert_eq!(k.commands_for(&c), vec![CommandId::Redo]);
        assert_eq!(k.label(CommandId::SaveProject), "Ctrl+S");
        assert_eq!(k.label(CommandId::Subdivide), "");
    }

    #[test]
    fn conflicts_are_detected_and_exclusive_assignment_resolves_them() {
        let mut k = Keymap::default();
        k.set(CommandId::Subdivide, vec![Chord::parse("Ctrl+S").unwrap()]);
        let conflicts = k.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].1.len(), 2);
        let stolen = k.assign_exclusive(CommandId::Subdivide, Chord::parse("Ctrl+S").unwrap());
        assert_eq!(stolen, vec![CommandId::SaveProject]);
        assert!(k.conflicts().is_empty());
        assert!(k.chords(CommandId::SaveProject).is_empty());
    }

    #[test]
    fn text_round_trip_stores_only_differences() {
        let mut k = Keymap::default();
        assert_eq!(k.to_text(), "");
        k.set(CommandId::MirrorX, vec![Chord::parse("Alt+X").unwrap()]);
        k.set(CommandId::SaveProject, vec![]);
        let text = k.to_text();
        assert!(text.contains("model.mirror_x = Alt+X"));
        assert!(text.contains("file.save ="));
        let mut k2 = Keymap::default();
        let problems = k2.apply_text(&text);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(k, k2);
    }

    #[test]
    fn bad_lines_are_reported_but_do_not_break_good_ones() {
        let mut k = Keymap::default();
        let p = k.apply_text("nope.nothing = Ctrl+Q\nmodel.mirror_x = Alt+X\nfile.new = Ctrl+Banana\ngarbage\n# comment\n");
        assert_eq!(p.len(), 3, "{p:?}");
        assert_eq!(k.label(CommandId::MirrorX), "Alt+X");
        assert_eq!(k.label(CommandId::NewProject), "Ctrl+N", "a bad line leaves the old binding alone");
    }

    #[test]
    fn reset_restores_defaults() {
        let mut k = Keymap::default();
        k.set(CommandId::Undo, vec![]);
        assert!(!k.is_default(CommandId::Undo));
        k.reset(CommandId::Undo);
        assert!(k.is_default(CommandId::Undo));
    }
}
