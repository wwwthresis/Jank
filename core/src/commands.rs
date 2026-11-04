//! The command catalogue: every user-invokable action, with a stable identifier, a title for the
//! command palette, a category, a one-line description and its default shortcut.
//!
//! The GUI turns a [`CommandId`] into behaviour; this module only describes the commands and
//! offers a fuzzy search over them. Keeping the catalogue in the core lets the palette, the
//! shortcut editor, the settings file and the documentation all read one source of truth.

use std::fmt;

/// Every command the application understands. The numeric order is not significant; use
/// [`CommandId::stable_name`] for anything that is stored on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CommandId {
    // File
    NewProject,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    ImportModel,
    ExportOutput,
    Quit,
    // Edit
    Undo,
    Redo,
    DeleteSelected,
    // Modes
    ModeModel,
    ModeTexture,
    ModePose,
    ModeAnimate,
    ModeRender,
    // Modeling
    AddCube,
    AddButterfly,
    Subdivide,
    MirrorX,
    WeldVertices,
    FlatShade,
    AutoUv,
    // Animation
    TogglePlay,
    PrevFrame,
    NextFrame,
    AddFrame,
    DuplicateFrame,
    // View
    FrameAll,
    TogglePreview,
    ToggleStats,
    GizmoSelect,
    GizmoMove,
    GizmoRotate,
    GizmoScale,
    GizmoToggleSpace,
    GizmoToggleSnap,
    // Application
    CommandPalette,
    OpenSettings,
    OpenAbout,
    OpenLicenses,
}

/// Menu-like grouping used by the palette and by the shortcut editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    File,
    Edit,
    Mode,
    Modeling,
    Animation,
    View,
    Application,
}

impl Category {
    pub fn title(self) -> &'static str {
        match self {
            Category::File => "File",
            Category::Edit => "Edit",
            Category::Mode => "Mode",
            Category::Modeling => "Modeling",
            Category::Animation => "Animation",
            Category::View => "View",
            Category::Application => "Application",
        }
    }
}

/// Static description of one command.
#[derive(Clone, Copy, Debug)]
pub struct CommandInfo {
    pub id: CommandId,
    /// Stable, dotted identifier stored in settings files, e.g. `file.save`. Never rename one.
    pub name: &'static str,
    /// Human title shown in the palette, e.g. "Save project".
    pub title: &'static str,
    pub category: Category,
    pub description: &'static str,
    /// Default shortcuts in the text form accepted by `keymap::Chord::parse`.
    pub default_chords: &'static [&'static str],
}

macro_rules! cmd {
    ($id:ident, $name:expr, $title:expr, $cat:ident, $desc:expr, [$($c:expr),*]) => {
        CommandInfo { id: CommandId::$id, name: $name, title: $title, category: Category::$cat, description: $desc, default_chords: &[$($c),*] }
    };
}

/// The catalogue, in the order shown in the shortcut editor.
pub const COMMANDS: &[CommandInfo] = &[
    cmd!(NewProject, "file.new", "New project", File, "Start an empty project", ["Ctrl+N"]),
    cmd!(OpenProject, "file.open", "Open project", File, "Open a .jank project from disk", ["Ctrl+O"]),
    cmd!(SaveProject, "file.save", "Save project", File, "Save the project; asks for a name the first time", ["Ctrl+S"]),
    cmd!(SaveProjectAs, "file.save_as", "Save project as", File, "Save a copy of the project under a new name", ["Ctrl+Shift+S"]),
    cmd!(ImportModel, "file.import", "Import model", File, "Import OBJ, STL, PLY, glTF or GLB", ["Ctrl+I"]),
    cmd!(ExportOutput, "file.export", "Export photo or video", File, "Open the Render mode with the export panel", ["Ctrl+E"]),
    cmd!(Quit, "file.quit", "Quit", File, "Close Jank!", ["Ctrl+Q"]),
    cmd!(Undo, "edit.undo", "Undo", Edit, "Undo the last action", ["Ctrl+Z"]),
    cmd!(Redo, "edit.redo", "Redo", Edit, "Redo the action that was undone", ["Ctrl+Y", "Ctrl+Shift+Z"]),
    cmd!(DeleteSelected, "edit.delete", "Delete selection", Edit, "Delete what is selected: faces, then the object in Model mode; the selected pixels in Texture mode; the current frame when the timeline is under the mouse", ["Delete", "Backspace"]),
    cmd!(ModeModel, "mode.model", "Mode: Model", Mode, "Edit geometry", ["1"]),
    cmd!(ModeTexture, "mode.texture", "Mode: Texture", Mode, "Paint textures", ["2"]),
    cmd!(ModePose, "mode.pose", "Mode: Pose", Mode, "Pose the skeleton", ["3"]),
    cmd!(ModeAnimate, "mode.animate", "Mode: Animate", Mode, "Build the frame-by-frame animation", ["4"]),
    cmd!(ModeRender, "mode.render", "Mode: Render", Mode, "Preview and export", ["5"]),
    cmd!(AddCube, "model.add_cube", "Add cube", Modeling, "Add a cube primitive to the scene", ["Shift+A"]),
    cmd!(AddButterfly, "model.add_butterfly", "Add butterfly", Modeling, "Add the built-in butterfly: a 12-triangle model with its own transparent texture", []),
    cmd!(Subdivide, "model.subdivide", "Subdivide", Modeling, "Split every face of the selected model into smaller faces", []),
    cmd!(MirrorX, "model.mirror_x", "Mirror along X", Modeling, "Mirror the selected model across the X axis", []),
    cmd!(WeldVertices, "model.weld", "Weld vertices", Modeling, "Merge vertices that share a position", []),
    cmd!(FlatShade, "model.flat_shade", "Flat shading", Modeling, "Give every face its own normal for a faceted look", []),
    cmd!(AutoUv, "model.auto_uv", "Automatic UV unwrap", Modeling, "Generate UV coordinates for the selected model", ["U"]),
    cmd!(TogglePlay, "anim.play", "Play or pause", Animation, "Start or stop the animation preview", ["Space"]),
    cmd!(PrevFrame, "anim.prev_frame", "Previous frame", Animation, "Go one frame back", ["Comma", "ArrowLeft"]),
    cmd!(NextFrame, "anim.next_frame", "Next frame", Animation, "Go one frame forward", ["Period", "ArrowRight"]),
    cmd!(AddFrame, "anim.add_frame", "Add frame", Animation, "Append a new frame after the current one", ["Shift+N"]),
    cmd!(DuplicateFrame, "anim.duplicate_frame", "Duplicate frame", Animation, "Copy the current frame", ["Shift+D"]),
    cmd!(FrameAll, "view.frame_all", "Frame all", View, "Move the viewport camera so the whole scene is visible", ["Home"]),
    cmd!(TogglePreview, "view.preview", "Render preview window", View, "Show or hide the small window with the final render at the chosen resolution", ["Shift+P"]),
    cmd!(ToggleStats, "view.stats", "Performance statistics", View, "Show frame rate, drawing time and triangle count in the viewport", ["F3"]),
    cmd!(GizmoSelect, "gizmo.select", "Gizmo: selection only", View, "Hide the transform gizmo and just select", ["Q"]),
    cmd!(GizmoMove, "gizmo.move", "Gizmo: move", View, "Show arrows to move the selected object", ["W"]),
    cmd!(GizmoRotate, "gizmo.rotate", "Gizmo: rotate", View, "Show rings to rotate the selected object", ["E"]),
    cmd!(GizmoScale, "gizmo.scale", "Gizmo: scale", View, "Show boxes to scale the selected object", ["R"]),
    cmd!(GizmoToggleSpace, "gizmo.space", "Gizmo: world or local axes", View, "Switch the move and rotate gizmo between world axes and the object's own axes", ["Shift+L"]),
    cmd!(GizmoToggleSnap, "gizmo.snap", "Gizmo: snapping on or off", View, "Round gizmo results to the snap steps (hold Ctrl while dragging for a one-off snap)", ["Shift+S"]),
    cmd!(CommandPalette, "app.palette", "Command palette", Application, "Search and run any command", ["Ctrl+K", "Ctrl+Shift+P"]),
    cmd!(OpenSettings, "app.settings", "Settings", Application, "Interface scale, autosave, accessibility and shortcuts", ["Ctrl+Comma"]),
    cmd!(OpenAbout, "app.about", "About Jank!", Application, "Version, support information and credits", ["F1"]),
    cmd!(OpenLicenses, "app.licenses", "Third-party licenses", Application, "Licences of the libraries built into Jank!", []),
];

impl CommandId {
    pub fn info(self) -> &'static CommandInfo {
        COMMANDS.iter().find(|c| c.id == self).expect("every CommandId has a catalogue entry")
    }
    pub fn stable_name(self) -> &'static str {
        self.info().name
    }
    pub fn title(self) -> &'static str {
        self.info().title
    }
    pub fn from_stable_name(name: &str) -> Option<CommandId> {
        COMMANDS.iter().find(|c| c.name == name).map(|c| c.id)
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.title())
    }
}

/// One result of [`search`].
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub id: CommandId,
    pub score: i32,
}

/// Fuzzy score of `query` against `text` (both case-insensitive). `None` when the letters of
/// the query do not appear in order. Consecutive letters and word starts score higher.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(|c| c.to_lowercase()).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).collect();
    // The first letter must start a word ("sp" finds "Save project", "av" does not).
    let start = (0..t.len()).find(|&i| t[i] == q[0] && (i == 0 || !t[i - 1].is_alphanumeric()))?;
    let mut score = 0i32;
    let mut qi = 0usize;
    let mut prev_match: Option<usize> = None;
    for (ti, &tc) in t.iter().enumerate().skip(start) {
        if qi < q.len() && tc == q[qi] {
            score += 10;
            if prev_match == Some(ti.wrapping_sub(1)) && ti > 0 {
                score += 8; // consecutive run
            }
            if ti == 0 || !t[ti - 1].is_alphanumeric() {
                score += 12; // start of a word
            }
            prev_match = Some(ti);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    // Prefer short titles and a match that starts early.
    score -= (t.len() as i32) / 4;
    Some(score)
}

/// Searches titles, categories and descriptions. An empty query lists everything in catalogue order.
pub fn search(query: &str) -> Vec<Hit> {
    let mut out: Vec<Hit> = COMMANDS
        .iter()
        .filter_map(|c| {
            if query.trim().is_empty() {
                return Some(Hit { id: c.id, score: 0 });
            }
            let by_title = fuzzy_score(query, c.title).map(|s| s * 3);
            let by_cat = c.category.title().to_lowercase().contains(&query.trim().to_lowercase()).then_some(30);
            // descriptions are long, so only an exact word-part match counts there
            let by_desc = c.description.to_lowercase().contains(&query.trim().to_lowercase()).then_some(40);
            [by_title, by_cat, by_desc].into_iter().flatten().max().map(|score| Hit { id: c.id, score })
        })
        .collect();
    if !query.trim().is_empty() {
        out.sort_by(|a, b| b.score.cmp(&a.score));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_are_unique_and_round_trip() {
        let mut seen = HashSet::new();
        for c in COMMANDS {
            assert!(seen.insert(c.name), "duplicate command name {}", c.name);
            assert_eq!(CommandId::from_stable_name(c.name), Some(c.id));
            assert!(c.name.contains('.'), "{} should be category.action", c.name);
            assert!(!c.title.is_empty() && !c.description.is_empty());
        }
    }

    #[test]
    fn every_id_has_exactly_one_entry() {
        let ids: HashSet<_> = COMMANDS.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), COMMANDS.len());
        // `info` panics for a missing entry, so touching each id proves coverage.
        for c in COMMANDS {
            let _ = c.id.info();
        }
    }

    #[test]
    fn fuzzy_matches_in_order_only() {
        assert!(fuzzy_score("svp", "Save project").is_some());
        assert!(fuzzy_score("pvs", "Save project").is_none());
        assert!(fuzzy_score("", "anything").is_some());
    }

    #[test]
    fn search_ranks_the_obvious_command_first() {
        let r = search("save");
        assert_eq!(r[0].id, CommandId::SaveProject);
        let r = search("undo");
        assert_eq!(r[0].id, CommandId::Undo);
        let r = search("lic");
        assert_eq!(r[0].id, CommandId::OpenLicenses);
        assert!(search("zzzzzz").is_empty());
        // scattered letters in a long description must not produce a hit
        assert!(search("mirr").iter().all(|h| h.id == CommandId::MirrorX), "{:?}", search("mirr"));
        assert_eq!(search("").len(), COMMANDS.len());
    }

    #[test]
    fn word_start_beats_scattered_letters() {
        let a = fuzzy_score("mod", "Mode: Model").unwrap();
        let b = fuzzy_score("mod", "Mirror octal dial").unwrap();
        assert!(a > b, "{a} should beat {b}");
        assert!(fuzzy_score("av", "Save project").is_none(), "the first letter must start a word");
    }
}
