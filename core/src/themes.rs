//! Interface themes as plain text files. A theme names the colours of the workspace surfaces,
//! text and the functional colours; the GUI reads them at paint time, so switching is instant.
//!
//! File format (`*.theme` in `<config dir>/themes/`), one `key = #rrggbb` per line:
//!
//! ```text
//! # My theme
//! name = Night Shift
//! bg0 = #101014
//! text = #e0e0e6
//! accent = #ff8800      # optional: overrides the accent chosen in Settings
//! ```
//!
//! Keys that are missing are taken from the built-in Graphite theme, so a theme file can be tiny.

use std::path::{Path, PathBuf};

/// The colour slots of a theme, in file order.
pub const KEYS: [&str; 16] = ["bg0", "bg1", "bg2", "bg3", "line", "line_soft", "text", "dim", "hilite", "shade", "warn", "danger", "viewport", "axis_x", "axis_y", "axis_z"];

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    /// Colours in the order of [`KEYS`].
    pub colors: [[u8; 3]; 16],
    /// An accent that replaces the one chosen in Settings (optional).
    pub accent: Option<[u8; 3]>,
    /// Light themes get dark text on accent-coloured buttons and lighter shadows.
    pub light: bool,
    /// Where the theme was loaded from (`None` = built in).
    pub path: Option<PathBuf>,
}

const fn hex(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

const GRAPHITE: [[u8; 3]; 16] = [
    hex(0x131316),
    hex(0x1b1b1f),
    hex(0x26262b),
    hex(0x313137),
    hex(0x3a3a41),
    hex(0x27272c),
    hex(0xdcdce0),
    hex(0x8b8b94),
    hex(0x3d3d45),
    hex(0x0d0d10),
    hex(0xd6a74a),
    hex(0xd65f6b),
    hex(0x131316),
    hex(0xd06464),
    hex(0x84b06a),
    hex(0x6493d2),
];

impl Theme {
    /// A stable id for the settings file: the name in lower case with dashes.
    pub fn id(&self) -> String {
        self.name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect()
    }
    pub fn color(&self, key: &str) -> [u8; 3] {
        KEYS.iter().position(|k| *k == key).map_or([255, 0, 255], |i| self.colors[i])
    }
    /// Parses a theme file. Unknown keys and bad values are reported, the rest is used.
    pub fn parse(text: &str) -> (Theme, Vec<String>) {
        let mut t = builtin().remove(0);
        t.name = "Untitled theme".into();
        let mut problems = vec![];
        let mut light = None;
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                problems.push(format!("line {}: missing '='", n + 1));
                continue;
            };
            let k = k.trim().to_lowercase();
            // strip a trailing comment after the value
            let v = v.trim();
            let v = match v.strip_prefix('#') {
                Some(rest) => format!("#{}", rest.split_whitespace().next().unwrap_or("")),
                None => v.split(" #").next().unwrap_or("").trim().to_string(),
            };
            match k.as_str() {
                "name" => t.name = v.chars().take(40).collect(),
                "light" => light = Some(matches!(v.to_lowercase().as_str(), "true" | "yes" | "1" | "on")),
                "accent" => match parse_hex(&v) {
                    Some(c) => t.accent = Some(c),
                    None => problems.push(format!("line {}: \"{v}\" is not a colour like #12ab34", n + 1)),
                },
                _ => match KEYS.iter().position(|x| *x == k) {
                    Some(i) => match parse_hex(&v) {
                        Some(c) => t.colors[i] = c,
                        None => problems.push(format!("line {}: \"{v}\" is not a colour like #12ab34", n + 1)),
                    },
                    None => problems.push(format!("line {}: unknown key \"{k}\"", n + 1)),
                },
            }
        }
        // light or dark is guessed from the panel colour unless the file says so
        let bg = t.colors[1];
        t.light = light.unwrap_or((bg[0] as u32 * 3 + bg[1] as u32 * 6 + bg[2] as u32) / 10 > 128);
        (t, problems)
    }
    /// The theme as a file that [`Theme::parse`] reads back.
    pub fn to_text(&self) -> String {
        let mut s = format!("# Jank! interface theme. Edit the colours and save; missing keys use the Graphite theme.\nname = {}\nlight = {}\n", self.name, self.light);
        if let Some(a) = self.accent {
            s.push_str(&format!("accent = #{:02x}{:02x}{:02x}\n", a[0], a[1], a[2]));
        }
        for (k, c) in KEYS.iter().zip(self.colors.iter()) {
            s.push_str(&format!("{k} = #{:02x}{:02x}{:02x}\n", c[0], c[1], c[2]));
        }
        s
    }
}

fn parse_hex(v: &str) -> Option<[u8; 3]> {
    let h = v.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(hex(n))
}

fn theme(name: &str, colors: [[u8; 3]; 16], accent: Option<[u8; 3]>, light: bool) -> Theme {
    Theme { name: name.into(), colors, accent, light, path: None }
}

/// The themes built into the program. The first one is the default.
pub fn builtin() -> Vec<Theme> {
    let mut midnight = GRAPHITE;
    for (i, c) in [0x0e1018, 0x151824, 0x1f2333, 0x2a2f44, 0x343a52, 0x1e2130, 0xd8dcf0, 0x8389a6, 0x353b55, 0x08090f, 0xe0b050, 0xe0607a, 0x0e1018].iter().enumerate() {
        midnight[i] = hex(*c);
    }
    let mut paper = GRAPHITE;
    for (i, c) in [0xdcdad4, 0xeceae4, 0xf8f7f3, 0xffffff, 0xb8b5ac, 0xd6d3cb, 0x26252a, 0x6b6964, 0xffffff, 0xa9a69e, 0xa86f10, 0xb8343f, 0xc9c7c0].iter().enumerate() {
        paper[i] = hex(*c);
    }
    paper[13] = hex(0xc0392b);
    paper[14] = hex(0x4c8a2f);
    paper[15] = hex(0x2f63b0);
    let mut psx = GRAPHITE;
    for (i, c) in [0x1c1f26, 0x2a2e38, 0x3a404d, 0x4a5161, 0x596172, 0x323744, 0xe6e8ee, 0x9aa1b2, 0x5d6578, 0x111318, 0xe8c050, 0xe86a6a, 0x101216].iter().enumerate() {
        psx[i] = hex(*c);
    }
    let mut crt = GRAPHITE;
    for (i, c) in [0x0c0a06, 0x14110a, 0x201b10, 0x2c2516, 0x3d331c, 0x1d180e, 0xffc56b, 0xb08443, 0x3a301a, 0x050402, 0xffd27a, 0xff6a4a, 0x0c0a06].iter().enumerate() {
        crt[i] = hex(*c);
    }
    let mut krita = GRAPHITE;
    for (i, c) in [0x232323, 0x2f2f2f, 0x3b3b3b, 0x474747, 0x505050, 0x383838, 0xe8e8e8, 0x9a9a9a, 0x4a4a4a, 0x181818, 0xe0b040, 0xe05a5a, 0x2a2a2a].iter().enumerate() {
        krita[i] = hex(*c);
    }
    vec![
        theme("Graphite", GRAPHITE, None, false),
        theme("Midnight", midnight, None, false),
        theme("Studio Grey", krita, None, false),
        theme("PlayStation", psx, Some([0x5c, 0x9c, 0xf0]), false),
        theme("Amber CRT", crt, Some([0xff, 0xa0, 0x30]), false),
        theme("Paper", paper, Some([0x3d, 0x6f, 0xd8]), true),
    ]
}

/// Folder with the user's theme files.
pub fn themes_dir() -> Option<PathBuf> {
    crate::settings::config_dir().map(|d| d.join("themes"))
}

/// Built-in themes followed by the ones found in `dir` (`*.theme`). A file whose name matches a
/// built-in theme replaces it. Problems are returned per file.
pub fn load_all(dir: Option<&Path>) -> (Vec<Theme>, Vec<String>) {
    let mut out = builtin();
    let mut problems = vec![];
    let Some(dir) = dir else { return (out, problems) };
    let Ok(rd) = std::fs::read_dir(dir) else { return (out, problems) };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("theme"))).collect();
    files.sort();
    for p in files.into_iter().take(64) {
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        if text.len() > 64 * 1024 {
            continue;
        }
        let (mut t, pr) = Theme::parse(&text);
        let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
        problems.extend(pr.into_iter().map(|m| format!("{fname}: {m}")));
        t.path = Some(p);
        match out.iter().position(|o| o.id() == t.id()) {
            Some(i) => out[i] = t,
            None => out.push(t),
        }
    }
    (out, problems)
}

/// Writes a theme into the themes folder (as `<id>.theme`) and returns the file path.
pub fn save(t: &Theme, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let p = dir.join(format!("{}.theme", t.id()));
    std::fs::write(&p, t.to_text()).map_err(|e| format!("Could not write {}: {e}", p.display()))?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_unique_and_round_trip() {
        let b = builtin();
        let mut ids: Vec<String> = b.iter().map(|t| t.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), b.len());
        for t in &b {
            let (back, p) = Theme::parse(&t.to_text());
            assert!(p.is_empty(), "{p:?}");
            assert_eq!(back.colors, t.colors);
            assert_eq!(back.accent, t.accent);
            assert_eq!(back.name, t.name);
            assert_eq!(back.light, t.light);
        }
        assert!(b.iter().any(|t| t.light));
    }

    #[test]
    fn partial_files_fall_back_and_report_problems() {
        let (t, p) = Theme::parse("name = Tiny\nbg0 = #010203 # deep\ntext = banana\nwhat = #ffffff\n");
        assert_eq!(t.name, "Tiny");
        assert_eq!(t.color("bg0"), [1, 2, 3]);
        assert_eq!(t.color("text"), builtin()[0].color("text"));
        assert_eq!(p.len(), 2, "{p:?}");
    }

    #[test]
    fn files_in_the_folder_are_loaded_and_override_builtins() {
        let dir = std::env::temp_dir().join(format!("jank_themes_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = builtin()[1].clone();
        t.colors[0] = [9, 9, 9];
        save(&t, &dir).unwrap();
        std::fs::write(dir.join("mine.theme"), "name = Mine\nbg1 = #f0f0f0\n").unwrap();
        let (all, p) = load_all(Some(&dir));
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(all.len(), builtin().len() + 1);
        assert_eq!(all.iter().find(|x| x.id() == "midnight").unwrap().colors[0], [9, 9, 9]);
        let mine = all.iter().find(|x| x.name == "Mine").unwrap();
        assert!(mine.light, "a light panel colour makes a light theme");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
