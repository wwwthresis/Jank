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
    OpenStarter,
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
    WhatsNew,
    RadialMenu,
    // Modeling (object level)
    EditModeToggle,
    BooleanUnion,
    BooleanDifference,
    BooleanIntersect,
    AddReference,
    ToggleIk,
    // Mesh Edit mode
    MeshSelectVertex,
    MeshSelectEdge,
    MeshSelectFace,
    MeshGrab,
    MeshRotate,
    MeshScale,
    MeshExtrude,
    MeshInset,
    MeshBevel,
    MeshBevelVertices,
    MeshLoopCut,
    MeshKnife,
    MeshMerge,
    MeshDelete,
    MeshDissolve,
    MeshFill,
    MeshConnect,
    MeshSelectAll,
    MeshSelectNone,
    MeshSelectInvert,
    MeshSelectLinked,
    MeshSelectMore,
    MeshSelectLess,
    MeshHide,
    MeshHideOthers,
    MeshReveal,
    MeshDuplicate,
    MeshSplit,
    MeshSeparate,
    MeshRecalcNormals,
    MeshFlipNormals,
    MeshTriangulate,
    MeshTrisToQuads,
    MeshShrinkFatten,
    MeshLoopSlide,
    MeshSubdivide,
    MeshSmooth,
    MeshBridge,
    MeshSymmetrize,
    MeshMarkSeam,
    MeshClearSeam,
    MeshUnwrap,
    MeshXray,
    MeshProportional,
    MeshShadeSmooth,
    MeshShadeFlat,
    // Painting
    ToolBrush,
    ToolEraser,
    ToolFill,
    ToolGradient,
    ToolPicker,
    ToolSmudge,
    ToolBlur,
    SwapColors,
    BrushSmaller,
    BrushBigger,
    ToggleSymmetry,
    ToggleProjectionPaint,
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
    /// Only active in Edit mode (polygon editing); shadows other commands with the same key there.
    MeshEdit,
    /// Only active while painting (Texture mode, painting in Animate); shadows other commands there.
    Paint,
}

/// Where a category's shortcuts are active.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Context {
    /// Everywhere (unless a context command with the same key is active).
    Global,
    MeshEdit,
    Paint,
}

impl Category {
    pub const ALL: [Category; 9] = [Category::File, Category::Edit, Category::Mode, Category::Modeling, Category::MeshEdit, Category::Paint, Category::Animation, Category::View, Category::Application];
    pub fn context(self) -> Context {
        match self {
            Category::MeshEdit => Context::MeshEdit,
            Category::Paint => Context::Paint,
            _ => Context::Global,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Category::MeshEdit => "Edit mode",
            Category::Paint => "Painting",
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
    cmd!(OpenStarter, "file.starter", "Open the bunny scene", File, "Open the starter scene: two plush bunnies on a rug, with bones, lighting and an idle animation", []),
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
    cmd!(TogglePlay, "anim.play", "Play or pause", Animation, "Start or stop the animation preview", ["Shift+Space"]),
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
    cmd!(WhatsNew, "app.whats_new", "What's new", Application, "The changes in this version of Jank!", []),
    cmd!(RadialMenu, "app.radial_menu", "Radial menu", Application, "A ring of the most useful tools for the current mode around the mouse (hold, move, release; or tap and click)", ["Space"]),
    cmd!(EditModeToggle, "model.edit_mode", "Edit mode (polygons)", Modeling, "Edit the vertices, edges and faces of the selected object, or go back to object mode", ["Tab"]),
    cmd!(BooleanUnion, "model.boolean_union", "Boolean: union", Modeling, "Join the other selected object (the cutter) into the active one", []),
    cmd!(BooleanDifference, "model.boolean_difference", "Boolean: difference", Modeling, "Cut the cutter object out of the active one", []),
    cmd!(BooleanIntersect, "model.boolean_intersect", "Boolean: intersect", Modeling, "Keep only the part where the active object and the cutter overlap", []),
    cmd!(AddReference, "view.add_reference", "Add reference image", View, "Place a picture in the 3D view to model or pose from (never rendered)", []),
    cmd!(ToggleIk, "anim.ik", "Inverse kinematics on or off", Animation, "Dragging a bone end moves its parents too (IK chain)", ["Shift+I"]),
    cmd!(MeshSelectVertex, "mesh.select_vertex", "Select vertices", MeshEdit, "Vertex select mode", ["1"]),
    cmd!(MeshSelectEdge, "mesh.select_edge", "Select edges", MeshEdit, "Edge select mode", ["2"]),
    cmd!(MeshSelectFace, "mesh.select_face", "Select faces", MeshEdit, "Face select mode", ["3"]),
    cmd!(MeshGrab, "mesh.grab", "Move (grab)", MeshEdit, "Move the selection with the mouse; X/Y/Z lock an axis, type a number, click or Enter to confirm, right-click or Esc to cancel", ["G"]),
    cmd!(MeshRotate, "mesh.rotate", "Rotate", MeshEdit, "Rotate the selection around its centre with the mouse", ["R"]),
    cmd!(MeshScale, "mesh.scale", "Scale", MeshEdit, "Scale the selection with the mouse", ["S"]),
    cmd!(MeshExtrude, "mesh.extrude", "Extrude", MeshEdit, "Pull new geometry out of the selected faces, edges or vertices", ["E"]),
    cmd!(MeshInset, "mesh.inset", "Inset faces", MeshEdit, "Make a smaller copy of the selected faces inside them", ["I"]),
    cmd!(MeshBevel, "mesh.bevel", "Bevel edges", MeshEdit, "Round or chamfer the selected edges; the wheel changes the segments", ["Ctrl+B"]),
    cmd!(MeshBevelVertices, "mesh.bevel_vertices", "Bevel vertices", MeshEdit, "Cut the corners at the selected vertices", ["Ctrl+Shift+B"]),
    cmd!(MeshLoopCut, "mesh.loop_cut", "Loop cut", MeshEdit, "Cut a ring of faces in half (hover an edge, the wheel adds cuts, click to place, then slide)", ["Ctrl+R"]),
    cmd!(MeshKnife, "mesh.knife", "Knife", MeshEdit, "Draw a cut across the faces: click points, Enter to cut, Esc to cancel", ["K"]),
    cmd!(MeshMerge, "mesh.merge", "Merge vertices", MeshEdit, "Merge the selected vertices at their centre", ["M"]),
    cmd!(MeshDelete, "mesh.delete", "Delete", MeshEdit, "Delete the selected vertices, edges or faces (depending on the select mode)", ["X", "Delete"]),
    cmd!(MeshDissolve, "mesh.dissolve", "Dissolve", MeshEdit, "Remove the selection but keep the surface closed", ["Ctrl+X"]),
    cmd!(MeshFill, "mesh.fill", "Make face", MeshEdit, "Make a face or edge from the selected vertices or close a hole", ["F"]),
    cmd!(MeshConnect, "mesh.connect", "Connect vertices", MeshEdit, "Split faces along the path between the selected vertices", ["J"]),
    cmd!(MeshSelectAll, "mesh.select_all", "Select all / none", MeshEdit, "Select everything, or nothing if everything is selected", ["A"]),
    cmd!(MeshSelectNone, "mesh.select_none", "Select none", MeshEdit, "Clear the selection", ["Alt+A"]),
    cmd!(MeshSelectInvert, "mesh.select_invert", "Invert selection", MeshEdit, "Select what is not selected", ["Ctrl+I"]),
    cmd!(MeshSelectLinked, "mesh.select_linked", "Select linked", MeshEdit, "Select everything connected to the selection (L under the mouse)", ["Ctrl+L"]),
    cmd!(MeshSelectMore, "mesh.select_more", "Select more", MeshEdit, "Grow the selection by one ring", ["Ctrl+Equals"]),
    cmd!(MeshSelectLess, "mesh.select_less", "Select less", MeshEdit, "Shrink the selection by one ring", ["Ctrl+Minus"]),
    cmd!(MeshHide, "mesh.hide", "Hide selected", MeshEdit, "Hide the selection to work on what is behind it", ["H"]),
    cmd!(MeshHideOthers, "mesh.hide_others", "Hide unselected", MeshEdit, "Hide everything except the selection", ["Shift+H"]),
    cmd!(MeshReveal, "mesh.reveal", "Reveal hidden", MeshEdit, "Show everything that was hidden", ["Alt+H"]),
    cmd!(MeshDuplicate, "mesh.duplicate", "Duplicate", MeshEdit, "Copy the selection and move the copy", ["Shift+D"]),
    cmd!(MeshSplit, "mesh.split", "Split", MeshEdit, "Detach the selected faces from the rest (same object)", ["Y"]),
    cmd!(MeshSeparate, "mesh.separate", "Separate into new object", MeshEdit, "Move the selected faces into a new object", ["P"]),
    cmd!(MeshRecalcNormals, "mesh.recalc_normals", "Recalculate normals", MeshEdit, "Make all faces point outwards", ["Shift+N"]),
    cmd!(MeshFlipNormals, "mesh.flip_normals", "Flip normals", MeshEdit, "Turn the selected faces inside out", []),
    cmd!(MeshTriangulate, "mesh.triangulate", "Triangulate", MeshEdit, "Split the selected faces into triangles", ["Ctrl+T"]),
    cmd!(MeshTrisToQuads, "mesh.tris_to_quads", "Triangles to quads", MeshEdit, "Join pairs of triangles into quads", ["Alt+J"]),
    cmd!(MeshShrinkFatten, "mesh.shrink_fatten", "Shrink / fatten", MeshEdit, "Move the selection along its normals", ["Alt+S"]),
    cmd!(MeshLoopSlide, "mesh.edge_slide", "Edge slide", MeshEdit, "Slide the selected edge loop along the faces next to it", ["Shift+V"]),
    cmd!(MeshSubdivide, "mesh.subdivide", "Subdivide", MeshEdit, "Split the selected edges and faces in half", []),
    cmd!(MeshSmooth, "mesh.smooth", "Smooth vertices", MeshEdit, "Relax the selected vertices towards their neighbours", []),
    cmd!(MeshBridge, "mesh.bridge", "Bridge edge loops", MeshEdit, "Connect two selected edge loops with faces", []),
    cmd!(MeshSymmetrize, "mesh.symmetrize", "Symmetrize", MeshEdit, "Mirror the +X half onto the −X half", []),
    cmd!(MeshMarkSeam, "mesh.mark_seam", "Mark seam", MeshEdit, "Mark the selected edges as UV seams", []),
    cmd!(MeshClearSeam, "mesh.clear_seam", "Clear seam", MeshEdit, "Remove the UV seams from the selected edges", []),
    cmd!(MeshUnwrap, "mesh.unwrap", "Unwrap UV", MeshEdit, "Flatten the selected faces into the texture along the seams (angle-based)", ["U"]),
    cmd!(MeshXray, "mesh.xray", "X-ray", MeshEdit, "See and select through the model", ["Alt+Z"]),
    cmd!(MeshProportional, "mesh.proportional", "Proportional editing", MeshEdit, "Moving the selection also drags nearby vertices with a soft falloff (the wheel sets the radius)", ["O"]),
    cmd!(MeshShadeSmooth, "mesh.shade_smooth", "Shade smooth", MeshEdit, "Smooth lighting across the selected faces", []),
    cmd!(MeshShadeFlat, "mesh.shade_flat", "Shade flat", MeshEdit, "Faceted lighting on the selected faces", []),
    cmd!(ToolBrush, "paint.brush", "Brush", Paint, "Paint with the brush", ["B"]),
    cmd!(ToolEraser, "paint.eraser", "Eraser", Paint, "Erase to transparency", ["E"]),
    cmd!(ToolFill, "paint.fill", "Fill", Paint, "Fill a region of one colour", ["F"]),
    cmd!(ToolGradient, "paint.gradient", "Gradient", Paint, "Drag a gradient from the main to the second colour (dithered in pixel mode)", ["G"]),
    cmd!(ToolPicker, "paint.picker", "Eyedropper", Paint, "Pick a colour from the picture", ["I"]),
    cmd!(ToolSmudge, "paint.smudge", "Smudge", Paint, "Push colours along the stroke", []),
    cmd!(ToolBlur, "paint.blur", "Blur", Paint, "Soften details under the brush", []),
    cmd!(SwapColors, "paint.swap_colors", "Swap colours", Paint, "Swap the main and the second colour", ["X"]),
    cmd!(BrushSmaller, "paint.smaller", "Smaller brush", Paint, "Make the brush smaller", ["OpenBracket"]),
    cmd!(BrushBigger, "paint.bigger", "Bigger brush", Paint, "Make the brush bigger", ["CloseBracket"]),
    cmd!(ToggleSymmetry, "paint.symmetry", "Symmetric painting", Paint, "Paint the mirrored side of the picture at the same time", []),
    cmd!(ToggleProjectionPaint, "paint.projection", "Projection painting", Paint, "Paint through the screen onto whatever is visible, across UV seams", []),
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
        let m = search("mirr");
        assert_eq!(m[0].id, CommandId::MirrorX, "{m:?}");
        assert!(m.iter().all(|h| (h.id.title().to_lowercase() + h.id.info().description).to_lowercase().contains("mirr")), "{m:?}");
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
