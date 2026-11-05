//! Edit mode of Model mode (Tab): the vertices, edges and polygons of the selected object are
//! edited in the viewport the way Blender does it. All the geometry work lives in
//! `psxcore::editor` (`edit_*`) and `psxcore::editmesh`; this file turns mouse and keys into those
//! calls and draws the overlay (edges, vertices, face dots, hover, loop-cut preview, knife line).
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{dim, section, slider_f, tool_cell, toggle_tip, Btn};
use crate::{compat, App};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Key, PointerButton, Pos2, Rect, Shape, Ui};
use psxcore::commands::CommandId;
use psxcore::editmesh::{BevelOptions, BoolOp, DeleteKind, MergeKind, SelectMode};
use psxcore::editor::{AxisLock, ClickMod, DragSnap, MeshDragKind, Primitive, EDGE_LOOSE, EDGE_SEAM, EDGE_SEL, EDGE_SHARP, EDGE_VISIBLE};

/// The mouse tool of Edit mode.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum EditTool {
    #[default]
    Select,
    LoopCut,
    Knife,
}

/// Viewport state of Edit mode that only the UI needs.
pub struct EditUi {
    pub tool: EditTool,
    /// Ctrl+R / K started the tool for one cut; it returns to Select afterwards.
    pub oneshot: bool,
    /// Press position of a possible box selection (screen points).
    pub press: Option<Pos2>,
    /// Box selection in progress (start, current) in screen points.
    pub boxing: Option<(Pos2, Pos2)>,
    /// Number of loop cuts placed at once.
    pub cuts: usize,
    /// Last mouse position over the viewport, in frame pixels.
    pub mouse: Option<(f32, f32)>,
    /// Size of the last drawn viewport picture.
    pub view: (u32, u32),
    /// What the last modal update was computed from (mouse, Ctrl, Shift).
    last_update: Option<((f32, f32), bool, bool)>,
    pub merge_dist: f32,
    pub bevel_width: f32,
    pub smooth: f32,
    /// The press that confirmed a modal tool: its release must not select anything.
    pub suppress_release: bool,
}
impl Default for EditUi {
    fn default() -> Self {
        EditUi { tool: EditTool::Select, oneshot: false, press: None, boxing: None, cuts: 1, mouse: None, view: (1, 1), last_update: None, merge_dist: 0.001, bevel_width: 0.1, smooth: 0.5, suppress_release: false }
    }
}

/// Above this many edges, points and dots the Edit-mode overlay is drawn as two batched meshes.
const HEAVY_OVERLAY: usize = 6000;

fn edge_colour(fl: u8, acc: Color32, wire: Color32) -> Color32 {
    if fl & EDGE_SEL != 0 {
        acc
    } else if fl & EDGE_SEAM != 0 {
        Color32::from_rgb(230, 70, 60)
    } else if fl & EDGE_SHARP != 0 {
        Color32::from_rgb(60, 200, 230)
    } else if fl & EDGE_LOOSE != 0 {
        Color32::from_rgb(200, 200, 200)
    } else {
        wire
    }
}

fn drag_name(k: MeshDragKind) -> &'static str {
    match k {
        MeshDragKind::Move => "Move",
        MeshDragKind::Rotate => "Rotate",
        MeshDragKind::Scale => "Scale",
        MeshDragKind::ShrinkFatten => "Shrink / fatten",
        MeshDragKind::Inset => "Inset",
        MeshDragKind::Bevel => "Bevel",
        MeshDragKind::BevelVerts => "Vertex bevel",
        MeshDragKind::LoopSlide => "Slide loop",
    }
}
fn axis_name(a: AxisLock) -> &'static str {
    match a {
        AxisLock::Free => "",
        AxisLock::Axis(0) => "along X",
        AxisLock::Axis(1) => "along Y",
        AxisLock::Axis(_) => "along Z",
        AxisLock::Plane(0) => "in the YZ plane",
        AxisLock::Plane(1) => "in the XZ plane",
        AxisLock::Plane(_) => "in the XY plane",
        AxisLock::Normal => "along the normal",
    }
}

impl App {
    /// Mouse position for a command that starts a modal tool (the middle of the view when the mouse is elsewhere).
    fn edit_mouse(&self) -> ((f32, f32), u32, u32) {
        let (w, h) = self.edui.view;
        (self.edui.mouse.unwrap_or((w as f32 * 0.5, h as f32 * 0.5)), w, h)
    }
    fn pick_radius(&self) -> f32 {
        // about 10 screen points, in frame pixels
        (10.0 * self.edui.view.0 as f32 / self.vp_width.max(1.0)).max(3.0)
    }

    /// Runs an Edit-mode command. Returns false when `id` is not one.
    pub fn run_mesh_command(&mut self, id: CommandId) -> bool {
        use CommandId::*;
        let is_mesh = matches!(id.info().category, psxcore::commands::Category::MeshEdit);
        if !is_mesh {
            return false;
        }
        if !self.ed.edit_active() {
            self.info("Mesh commands work in Edit mode: select an object and press Tab");
            return true;
        }
        let (m, w, h) = self.edit_mouse();
        let drag = |app: &mut App, k: MeshDragKind| {
            app.edui.last_update = None;
            app.ed.edit_drag_begin(k, m, w, h);
        };
        match id {
            MeshSelectVertex => self.ed.edit_set_select_mode(SelectMode::Vertex),
            MeshSelectEdge => self.ed.edit_set_select_mode(SelectMode::Edge),
            MeshSelectFace => self.ed.edit_set_select_mode(SelectMode::Face),
            MeshGrab => drag(self, MeshDragKind::Move),
            MeshRotate => drag(self, MeshDragKind::Rotate),
            MeshScale => drag(self, MeshDragKind::Scale),
            MeshShrinkFatten => drag(self, MeshDragKind::ShrinkFatten),
            MeshInset => drag(self, MeshDragKind::Inset),
            MeshBevel => drag(self, MeshDragKind::Bevel),
            MeshBevelVertices => drag(self, MeshDragKind::BevelVerts),
            MeshLoopSlide => drag(self, MeshDragKind::LoopSlide),
            MeshExtrude => {
                self.edui.last_update = None;
                if !self.ed.edit_extrude_begin(m, w, h) {
                    self.info("Select something to extrude");
                }
            }
            MeshDuplicate => {
                self.edui.last_update = None;
                self.ed.edit_duplicate_begin(m, w, h);
            }
            MeshLoopCut => {
                self.edui.tool = EditTool::LoopCut;
                self.edui.oneshot = true;
            }
            MeshKnife => {
                self.edui.tool = EditTool::Knife;
                self.edui.oneshot = true;
            }
            MeshMerge => {
                let n = self.ed.edit_merge(MergeKind::Center);
                self.info(format!("Merged: {n} vertices removed"));
            }
            MeshDelete => {
                self.ed.edit_delete_selected();
            }
            MeshDissolve => {
                self.ed.edit_dissolve();
            }
            MeshFill => {
                self.ed.edit_op(|m| m.fill());
            }
            MeshConnect => {
                self.ed.edit_op(|m| m.connect_verts());
            }
            MeshSelectAll => self.ed.edit_select_toggle(),
            MeshSelectNone => self.ed.edit_select_all(false),
            MeshSelectInvert => self.ed.edit_select_invert(),
            MeshSelectLinked => self.ed.edit_select_linked(),
            MeshSelectMore => self.ed.edit_grow(true),
            MeshSelectLess => self.ed.edit_grow(false),
            MeshHide => {
                self.ed.edit_op(|m| m.hide(false));
            }
            MeshHideOthers => {
                self.ed.edit_op(|m| m.hide(true));
            }
            MeshReveal => {
                self.ed.edit_op(|m| m.reveal());
            }
            MeshSplit => {
                self.ed.edit_op(|m| m.split());
            }
            MeshSeparate => {
                if self.ed.edit_separate().is_some() {
                    self.info("The selection is now a separate object");
                }
            }
            MeshRecalcNormals => {
                self.ed.edit_op(|m| m.recalc_normals(false));
            }
            MeshFlipNormals => {
                self.ed.edit_op(|m| m.flip_normals());
            }
            MeshTriangulate => {
                self.ed.edit_op(|m| m.triangulate_faces());
            }
            MeshTrisToQuads => {
                self.ed.edit_op(|m| {
                    let faces = m.selected_faces();
                    m.tris_to_quads(&faces, 40.0)
                });
            }
            MeshSubdivide => {
                self.ed.edit_op(|m| m.subdivide());
            }
            MeshSmooth => {
                let f = self.edui.smooth;
                self.ed.edit_op(|m| m.smooth_verts(f, 1));
            }
            MeshBridge => {
                if self.ed.edit_op(|m| m.bridge()) != Some(true) {
                    self.info("Bridge needs two edge loops (or two faces) selected");
                }
            }
            MeshSymmetrize => {
                self.ed.edit_op(|m| m.symmetrize(0, true));
            }
            MeshMarkSeam => {
                self.ed.edit_op(|m| m.mark_seam(true));
            }
            MeshClearSeam => {
                self.ed.edit_op(|m| m.mark_seam(false));
            }
            MeshUnwrap => {
                let n = self.ed.edit_unwrap(0, w, h);
                self.info(format!("Unwrapped: {n} UV island(s)"));
            }
            MeshXray => {
                if let Some(s) = self.ed.edit.as_mut() {
                    s.xray = !s.xray;
                }
            }
            MeshProportional => {
                if let Some(s) = self.ed.edit.as_mut() {
                    s.proportional = !s.proportional;
                }
            }
            MeshShadeSmooth => {
                self.ed.edit_op(|m| m.set_smooth(true));
            }
            MeshShadeFlat => {
                self.ed.edit_op(|m| m.set_smooth(false));
            }
            _ => return false,
        }
        true
    }

    /// Keys of a running modal tool (axis locks, typed values, confirm and cancel) and of the knife.
    /// Returns true when the keyboard belongs to Edit mode this frame (normal shortcuts are skipped).
    pub fn edit_modal_keys(&mut self, ctx: &egui::Context) -> bool {
        if !self.ed.edit_active() {
            return false;
        }
        let events = ctx.input(|i| i.events.clone());
        if self.ed.edit_drag_active().is_some() {
            for e in events {
                match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                        Key::X => self.ed.edit_drag_axis(0, modifiers.shift),
                        Key::Y => self.ed.edit_drag_axis(1, modifiers.shift),
                        Key::Z => self.ed.edit_drag_axis(2, modifiers.shift),
                        Key::Enter | Key::Space => self.ed.edit_drag_end(true),
                        Key::Escape => self.ed.edit_drag_end(false),
                        Key::Backspace => self.ed.edit_drag_char('\u{8}'),
                        Key::Minus => self.ed.edit_drag_char('-'),
                        _ => {}
                    },
                    egui::Event::Text(t) => {
                        for c in t.chars() {
                            if c.is_ascii_digit() || c == '.' {
                                self.ed.edit_drag_char(c);
                            }
                        }
                    }
                    _ => {}
                }
                if self.ed.edit_drag_active().is_none() {
                    break;
                }
            }
            if self.ed.edit_drag_active().is_none() && self.edui.oneshot {
                self.edui.tool = EditTool::Select;
                self.edui.oneshot = false;
            }
            return true;
        }
        let enter = ctx.input(|i| i.key_pressed(Key::Enter));
        let esc = ctx.input(|i| i.key_pressed(Key::Escape));
        match self.edui.tool {
            EditTool::Knife => {
                if enter {
                    let pts = self.ed.edit.as_ref().map(|s| s.knife.clone()).unwrap_or_default();
                    let n = self.ed.edit_knife(&self.rend.frame, &pts);
                    self.info(if n > 0 { format!("Knife: {n} cut(s)") } else { "Knife: the line crossed no visible edge".to_string() });
                    if self.edui.oneshot {
                        self.edui.tool = EditTool::Select;
                    }
                    return true;
                }
                if esc {
                    let had = self.ed.edit.as_ref().is_some_and(|s| !s.knife.is_empty());
                    if let Some(s) = self.ed.edit.as_mut() {
                        s.knife.clear();
                    }
                    if !had || self.edui.oneshot {
                        self.edui.tool = EditTool::Select;
                    }
                    return true;
                }
            }
            EditTool::LoopCut => {
                if esc {
                    self.edui.tool = EditTool::Select;
                    if let Some(s) = self.ed.edit.as_mut() {
                        s.loop_preview = None;
                    }
                    return true;
                }
            }
            EditTool::Select => {
                // L: select what is connected to the element under the cursor (Shift+L deselects)
                let l = ctx.input(|i| i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key: Key::L, pressed: true, modifiers, .. } if !modifiers.ctrl && !modifiers.command && !modifiers.alt => Some(modifiers.shift),
                    _ => None,
                }));
                if let (Some(shift), Some(m)) = (l, self.edui.mouse) {
                    let r = self.pick_radius();
                    self.ed.edit_select_linked_at(&self.rend.frame, m.0, m.1, r, shift);
                    return true;
                }
            }
        }
        false
    }

    /// Mouse input of Edit mode. Returns true when the viewport input was used here.
    pub fn edit_input(&mut self, ctx: &egui::Context, resp: &egui::Response, img: Rect, sc: f32, w: u32, h: u32) -> bool {
        if !self.ed.edit_active() {
            return false;
        }
        let (pos, pressed, released, down, sec, mods, scroll) =
            ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.primary_down(), i.pointer.button_pressed(PointerButton::Secondary), i.modifiers, i.raw_scroll_delta.y));
        let hover = resp.hovered() || resp.dragged();
        let to_f = |p: Pos2| ((p.x - img.min.x) / sc, (p.y - img.min.y) / sc);
        self.edui.view = (w, h);
        self.vp_width = img.width();
        let moved = if let Some(p) = pos.filter(|_| hover) {
            let f = to_f(p);
            let m = self.edui.mouse != Some(f);
            self.edui.mouse = Some(f);
            m
        } else {
            false
        };
        let r = self.pick_radius();
        let ctrl = mods.ctrl || mods.command;

        // ---- a modal tool follows the mouse until a click confirms it (right click cancels)
        if let Some(kind) = self.ed.edit_drag_active().map(|d| d.kind) {
            if let Some(m) = self.edui.mouse {
                let key = (m, ctrl, mods.shift);
                if self.edui.last_update != Some(key) {
                    self.edui.last_update = Some(key);
                    let snap = DragSnap { increment: ctrl, step: if mods.shift { 0.01 } else { 0.1 }, vertex: false, precise: mods.shift && !ctrl };
                    self.ed.edit_drag_update(m, snap);
                }
            }
            if scroll != 0.0 && hover && matches!(kind, MeshDragKind::Bevel) || scroll != 0.0 && self.ed.edit.as_ref().is_some_and(|s| s.proportional) {
                self.ed.edit_drag_wheel(if scroll > 0.0 { 1 } else { -1 });
            }
            if pressed {
                self.ed.edit_drag_end(true);
                self.edui.press = None;
                self.edui.suppress_release = true;
            } else if sec {
                self.ed.edit_drag_end(false);
            }
            if self.ed.edit_drag_active().is_none() && self.edui.oneshot {
                self.edui.tool = EditTool::Select;
                self.edui.oneshot = false;
            }
            ctx.set_cursor_icon(egui::CursorIcon::Move);
            ctx.request_repaint();
            return true;
        }
        if self.edui.suppress_release {
            if !down {
                self.edui.suppress_release = false;
            }
            return true;
        }
        if !hover {
            return false;
        }
        let Some(p) = pos else { return true };
        let m = to_f(p);
        match self.edui.tool {
            EditTool::Knife => {
                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                if pressed && !mods.alt {
                    if let Some(s) = self.ed.edit.as_mut() {
                        s.knife.push(m);
                    }
                }
                if resp.double_clicked() {
                    let pts = self.ed.edit.as_ref().map(|s| s.knife.clone()).unwrap_or_default();
                    self.ed.edit_knife(&self.rend.frame, &pts);
                    if self.edui.oneshot {
                        self.edui.tool = EditTool::Select;
                    }
                }
                true
            }
            EditTool::LoopCut => {
                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                if ctrl && scroll != 0.0 {
                    self.edui.cuts = (self.edui.cuts as i32 + if scroll > 0.0 { 1 } else { -1 }).clamp(1, 32) as usize;
                }
                if moved || ctrl && scroll != 0.0 {
                    self.ed.edit_loopcut_preview(&self.rend.frame, m.0, m.1, r, self.edui.cuts);
                }
                if pressed && !mods.alt {
                    self.edui.last_update = None;
                    if self.ed.edit_loopcut_apply(m, w, h) {
                        self.edui.suppress_release = true;
                    }
                }
                true
            }
            EditTool::Select => {
                if moved && !down {
                    self.ed.edit_hover(Some(&self.rend.frame), m.0, m.1, r);
                }
                if pressed && !mods.alt {
                    self.edui.press = Some(p);
                }
                if let (Some(a), true) = (self.edui.press, down) {
                    if self.edui.boxing.is_some() || (p - a).length() > 5.0 {
                        self.edui.boxing = Some((a, p));
                    }
                }
                if released {
                    if let Some((a, b)) = self.edui.boxing.take() {
                        let op = if ctrl { 2 } else if mods.shift { 1 } else { 0 };
                        self.ed.edit_select_region(&self.rend.frame, &[to_f(a), to_f(b)], op);
                    } else if self.edui.press.is_some() {
                        let how = if mods.alt && ctrl {
                            ClickMod::Ring
                        } else if mods.alt {
                            ClickMod::Loop
                        } else if ctrl {
                            ClickMod::Path
                        } else if mods.shift {
                            ClickMod::Extend
                        } else {
                            ClickMod::Replace
                        };
                        self.ed.edit_click(&self.rend.frame, m.0, m.1, r, how);
                    }
                    self.edui.press = None;
                }
                // Alt+click (no drag): loops; the press itself belongs to navigation
                if mods.alt && resp.clicked_by(PointerButton::Primary) {
                    let how = if ctrl { ClickMod::Ring } else { ClickMod::Loop };
                    let how = if mods.shift && how == ClickMod::Loop { ClickMod::Loop } else { how };
                    self.ed.edit_click(&self.rend.frame, m.0, m.1, r, how);
                }
                true
            }
        }
    }

    /// Edges, vertices, face dots, hover, previews and the box of Edit mode.
    pub fn edit_overlays(&self, p: &egui::Painter, img: Rect, sc: f32) {
        if !self.ed.edit_active() {
            return;
        }
        let at = |q: (f32, f32)| pos2(img.min.x + q.0 * sc, img.min.y + q.1 * sc);
        let o = self.ed.edit_overlay(&self.rend.frame);
        let s = self.ed.edit.as_ref().unwrap();
        let xray = s.xray;
        let mode = self.ed.edit_mesh().map_or(SelectMode::Vertex, |m| m.mode);
        let acc = accent();
        let wire = Color32::from_rgb(10, 10, 12);
        let hidden_alpha = if xray { 150 } else { 45 };
        // Heavy meshes: everything in two meshes (wires, then selection on top), without the halos.
        let heavy = o.edges.len() + o.verts.len() + o.dots.len() > HEAVY_OVERLAY;
        if heavy {
            let (mut under, mut over) = (compat::Batch::default(), compat::Batch::default());
            for &(a, b, fl) in &o.edges {
                let sel = fl & EDGE_SEL != 0;
                let vis = fl & EDGE_VISIBLE != 0;
                if !vis && !xray && !sel {
                    continue;
                }
                let c = edge_colour(fl, acc, wire);
                let c = if vis { c } else { alpha(c, hidden_alpha) };
                if sel {
                    over.line(at(a), at(b), 2.0, c);
                } else {
                    under.line(at(a), at(b), 1.2, c);
                }
            }
            let dots = if mode == SelectMode::Vertex { &o.verts } else if mode == SelectMode::Face { &o.dots } else { &Vec::new() };
            let (size, plain) = if mode == SelectMode::Vertex { (4.0, Color32::BLACK) } else { (3.0, Color32::from_gray(20)) };
            for &(x, y, sel, vis) in dots {
                if !vis && !xray && !sel {
                    continue;
                }
                let c = if sel { acc } else { plain };
                (if sel { &mut over } else { &mut under }).square(at((x, y)), size, if vis { c } else { alpha(c, hidden_alpha) });
            }
            under.paint(p);
            over.paint(p);
        }
        // unselected first, selected on top
        for pass in 0..2 {
            if heavy {
                break;
            }
            for &(a, b, fl) in &o.edges {
                let sel = fl & EDGE_SEL != 0;
                if sel != (pass == 1) {
                    continue;
                }
                let vis = fl & EDGE_VISIBLE != 0;
                let mut c = edge_colour(fl, acc, wire);
                if !vis {
                    c = alpha(c, hidden_alpha);
                    if !xray && !sel {
                        continue;
                    }
                }
                let wdt = if sel { 2.0 } else { 1.2 };
                if !sel && vis {
                    // light halo so dark wires read on dark textures
                    p.line_segment([at(a), at(b)], compat::stroke(wdt + 1.5, alpha(Color32::WHITE, 40)));
                }
                p.line_segment([at(a), at(b)], compat::stroke(wdt, c));
            }
        }
        for &(a, b) in &o.hover {
            p.line_segment([at(a), at(b)], compat::stroke(2.5, alpha(Color32::WHITE, 200)));
        }
        if mode == SelectMode::Vertex && !heavy {
            for &(x, y, sel, vis) in &o.verts {
                if !vis && !xray && !sel {
                    continue;
                }
                let c = if sel { acc } else { Color32::BLACK };
                let r = Rect::from_center_size(at((x, y)), vec2(5.0, 5.0));
                p.rect_filled(r, compat::cr(0.0), if vis { c } else { alpha(c, hidden_alpha) });
                if !sel && vis {
                    compat::stroke_rect(p, r.expand(0.5), 0.0, compat::stroke(1.0, alpha(Color32::WHITE, 90)));
                }
            }
        }
        if mode == SelectMode::Face && !heavy {
            for &(x, y, sel, vis) in &o.dots {
                if !vis && !xray && !sel {
                    continue;
                }
                let c = if sel { acc } else { Color32::from_gray(20) };
                p.rect_filled(Rect::from_center_size(at((x, y)), vec2(4.0, 4.0)), compat::cr(0.0), if vis { c } else { alpha(c, hidden_alpha) });
            }
        }
        if let Some(q) = o.hover_dot {
            p.circle_stroke(at(q), 6.0, compat::stroke(2.0, Color32::WHITE));
        }
        if let Some(q) = o.active {
            p.circle_stroke(at(q), 5.0, compat::stroke(1.5, Color32::WHITE));
        }
        for &(a, b) in &o.loop_preview {
            p.line_segment([at(a), at(b)], compat::stroke(2.0, Color32::from_rgb(255, 220, 60)));
        }
        // knife line
        if !s.knife.is_empty() {
            let mut pts: Vec<Pos2> = s.knife.iter().map(|&q| at(q)).collect();
            if let Some(m) = self.edui.mouse.filter(|_| self.edui.tool == EditTool::Knife) {
                pts.push(at(m));
            }
            p.add(Shape::line(pts.clone(), compat::stroke(2.0, Color32::from_rgb(255, 220, 60))));
            for q in &s.knife {
                p.circle_filled(at(*q), 3.5, Color32::from_rgb(255, 220, 60));
            }
        }
        if let Some((a, b)) = self.edui.boxing {
            let r = Rect::from_two_pos(a, b);
            p.rect_filled(r, compat::cr(0.0), alpha(acc, 30));
            compat::stroke_rect(p, r, 0.0, compat::stroke(1.0, alpha(Color32::WHITE, 200)));
        }
        // readout of the running tool
        let text = if let Some(d) = self.ed.edit_drag_active() {
            let mut t = format!("{}  {}", drag_name(d.kind), axis_name(d.axis));
            if !d.numeric.is_empty() {
                t.push_str(&format!("   typed: {}", d.numeric));
            } else {
                t.push_str(&format!("   {:.3}", d.value));
            }
            if matches!(d.kind, MeshDragKind::Bevel) {
                t.push_str(&format!("   segments {} (wheel)", d.segments));
            }
            t.push_str("   ·   click / Enter confirm, right click / Esc cancel, X Y Z lock, Ctrl snap, Shift precise");
            Some(t)
        } else {
            match self.edui.tool {
                EditTool::LoopCut => Some(format!("Loop cut: {} cut(s) (Ctrl+wheel) · click to cut, then slide · Esc leaves the tool", self.edui.cuts)),
                EditTool::Knife => Some("Knife: click points along the cut · Enter or double click cuts · Esc cancels".to_string()),
                EditTool::Select => None,
            }
        };
        if let Some(t) = text {
            let r = compat::text(p, pos2(img.center().x, img.min.y + 14.0), Align2::CENTER_TOP, &t, 12.5, Color32::WHITE);
            p.rect_filled(r.expand2(vec2(8.0, 4.0)), compat::cr(3.0), alpha(BG0(), 200));
            compat::text(p, pos2(img.center().x, img.min.y + 14.0), Align2::CENTER_TOP, &t, 12.5, Color32::WHITE);
        }
    }

    /// Short Edit-mode summary for the status bar and the viewport caption.
    pub fn edit_summary(&self) -> Option<String> {
        let (v, e, t, q, n) = self.ed.edit_stats()?;
        let m = self.ed.edit_mesh()?;
        let sel = match m.mode {
            SelectMode::Vertex => format!("{} {} selected", m.sel_v.len(), if m.sel_v.len() == 1 { "vertex" } else { "vertices" }),
            SelectMode::Edge => format!("{} {} selected", m.sel_e.len(), if m.sel_e.len() == 1 { "edge" } else { "edges" }),
            SelectMode::Face => format!("{} {} selected", m.sel_f.len(), if m.sel_f.len() == 1 { "face" } else { "faces" }),
        };
        Some(format!("Edit mode   {v} verts  {e} edges  {} faces ({t} tris, {q} quads, {n} n-gons)   {sel}", t + q + n))
    }

    /// Tool-strip cells of Edit mode.
    pub fn edit_cells(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        for (t, icon, tip) in [
            (EditTool::Select, Icon::Select, "Select: click, Shift adds, Ctrl picks a path, Alt picks a loop; drag a box (Shift adds, Ctrl removes)"),
            (EditTool::LoopCut, Icon::LoopCut, "Loop cut (Ctrl+R): hover an edge, Ctrl+wheel for more cuts, click to cut and slide"),
            (EditTool::Knife, Icon::Knife, "Knife (K): click points, Enter cuts"),
        ] {
            if tool_cell(ui, icon, self.edui.tool == t, tip).clicked() {
                self.edui.tool = t;
                self.edui.oneshot = false;
            }
        }
        crate::chrome::hairline(ui);
        let mode = self.ed.edit_mesh().map_or(SelectMode::Vertex, |m| m.mode);
        for (m, icon, id, tip) in [(SelectMode::Vertex, Icon::Vertex, CommandId::MeshSelectVertex, "Vertex select"), (SelectMode::Edge, Icon::Edge, CommandId::MeshSelectEdge, "Edge select"), (SelectMode::Face, Icon::Face, CommandId::MeshSelectFace, "Face select")] {
            let tip = self.tip_for(id, tip);
            if tool_cell(ui, icon, mode == m, &tip).clicked() {
                self.ed.edit_set_select_mode(m);
            }
        }
        crate::chrome::hairline(ui);
        for (id, icon, tip) in [(CommandId::MeshExtrude, Icon::Extrude, "Extrude"), (CommandId::MeshInset, Icon::Inset, "Inset faces"), (CommandId::MeshBevel, Icon::Bevel, "Bevel")] {
            let tip = self.tip_for(id, tip);
            if tool_cell(ui, icon, false, &tip).clicked() {
                self.run_command(ctx, id);
            }
        }
        crate::chrome::hairline(ui);
        let (xray, prop) = self.ed.edit.as_ref().map_or((false, false), |s| (s.xray, s.proportional));
        let tip = self.tip_for(CommandId::MeshXray, "X-ray: see and select through the mesh");
        if tool_cell(ui, Icon::Eye, xray, &tip).clicked() {
            self.run_command(ctx, CommandId::MeshXray);
        }
        let tip = self.tip_for(CommandId::MeshProportional, "Proportional editing: nearby vertices follow (wheel changes the radius while moving)");
        if tool_cell(ui, Icon::Sculpt, prop, &tip).clicked() {
            self.run_command(ctx, CommandId::MeshProportional);
        }
        let tip = self.tip_for(CommandId::EditModeToggle, "Leave Edit mode");
        if tool_cell(ui, Icon::Cube, false, &tip).clicked() {
            self.run_command(ctx, CommandId::EditModeToggle);
        }
    }

    /// Left panel of Edit mode.
    pub fn edit_tools(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let acc = accent();
        let b = |ui: &mut Ui, app: &App, id: CommandId, label: &str, icon: Option<Icon>| -> bool {
            let tip = app.tip_for(id, id.info().description);
            let mut btn = Btn::new(label).tip(&tip);
            if let Some(i) = icon {
                btn = btn.icon(i);
            }
            btn.show(ui).clicked()
        };
        let mut run: Option<CommandId> = None;
        section(ui, "Edit mode", acc, |ui| {
            if let Some(s) = self.edit_summary() {
                dim(ui, &s.replacen("Edit mode   ", "", 1));
            }
            let mode = self.ed.edit_mesh().map_or(SelectMode::Vertex, |m| m.mode);
            ui.horizontal(|ui| {
                for (m, icon, label) in [(SelectMode::Vertex, Icon::Vertex, "Vertex"), (SelectMode::Edge, Icon::Edge, "Edge"), (SelectMode::Face, Icon::Face, "Face")] {
                    if Btn::new(label).icon(icon).accent(acc).on(mode == m).show(ui).clicked() {
                        self.ed.edit_set_select_mode(m);
                    }
                }
            });
            if !self.settings.show_toolbar {
                ui.horizontal(|ui| {
                    for (t, icon, label) in [(EditTool::Select, Icon::Select, "Select"), (EditTool::LoopCut, Icon::LoopCut, "Loop cut"), (EditTool::Knife, Icon::Knife, "Knife")] {
                        if Btn::new(label).icon(icon).accent(acc).on(self.edui.tool == t).show(ui).clicked() {
                            self.edui.tool = t;
                            self.edui.oneshot = false;
                        }
                    }
                });
            }
            if Btn::new("Back to object mode (Tab)").icon(Icon::Cube).size(vec2(ui.available_width(), 30.0)).show(ui).clicked() {
                run = Some(CommandId::EditModeToggle);
            }
        });
        section(ui, "Select", acc, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (id, l) in [
                    (CommandId::MeshSelectAll, "All"),
                    (CommandId::MeshSelectNone, "None"),
                    (CommandId::MeshSelectInvert, "Invert"),
                    (CommandId::MeshSelectLinked, "Linked"),
                    (CommandId::MeshSelectMore, "More"),
                    (CommandId::MeshSelectLess, "Less"),
                    (CommandId::MeshHide, "Hide"),
                    (CommandId::MeshReveal, "Reveal"),
                ] {
                    if b(ui, self, id, l, None) {
                        run = Some(id);
                    }
                }
            });
        });
        section(ui, "Transform", acc, |ui| {
            dim(ui, "G move · R rotate · S scale, then X/Y/Z to lock an axis and type a number.");
            ui.horizontal_wrapped(|ui| {
                for (id, l, i) in [(CommandId::MeshGrab, "Move", Icon::Move), (CommandId::MeshRotate, "Rotate", Icon::Rotate), (CommandId::MeshScale, "Scale", Icon::Scale), (CommandId::MeshShrinkFatten, "Shrink/fatten", Icon::Sculpt)] {
                    if b(ui, self, id, l, Some(i)) {
                        run = Some(id);
                    }
                }
            });
        });
        section(ui, "Mesh tools", acc, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (id, l, i) in [
                    (CommandId::MeshExtrude, "Extrude", Some(Icon::Extrude)),
                    (CommandId::MeshInset, "Inset", Some(Icon::Inset)),
                    (CommandId::MeshBevel, "Bevel", Some(Icon::Bevel)),
                    (CommandId::MeshLoopCut, "Loop cut", Some(Icon::LoopCut)),
                    (CommandId::MeshKnife, "Knife", Some(Icon::Knife)),
                    (CommandId::MeshSubdivide, "Subdivide", None),
                    (CommandId::MeshFill, "Fill", None),
                    (CommandId::MeshBridge, "Bridge", None),
                    (CommandId::MeshConnect, "Connect", None),
                    (CommandId::MeshDuplicate, "Duplicate", None),
                    (CommandId::MeshSplit, "Split", None),
                    (CommandId::MeshSeparate, "Separate", None),
                    (CommandId::MeshBevelVertices, "Vertex bevel", None),
                ] {
                    if b(ui, self, id, l, i) {
                        run = Some(id);
                    }
                }
            });
            ui.horizontal(|ui| {
                let segs = self.ed.edit.as_ref().map_or(1, |s| s.bevel_segments) as f32;
                let mut sg = segs;
                ui.label(egui::RichText::new("bevel segments").color(DIM()));
                if ui.add(egui::DragValue::new(&mut sg).speed(0.1).range(1.0..=32.0)).changed() {
                    if let Some(s) = self.ed.edit.as_mut() {
                        s.bevel_segments = sg.round().max(1.0) as usize;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.edui.bevel_width).speed(0.005).range(0.001..=10.0).max_decimals(3));
                if Btn::new("Bevel by width").tip("Bevels the selected edges by exactly this width").show(ui).clicked() {
                    let segments = self.ed.edit.as_ref().map_or(1, |s| s.bevel_segments);
                    if !self.ed.edit_bevel_apply(BevelOptions { width: self.edui.bevel_width, segments, clamp: true }) {
                        self.info("Bevel: select edges first");
                    }
                }
            });
            if let Some(s) = self.ed.edit.as_mut() {
                toggle_tip(ui, &mut s.inset_individual, "Inset each face on its own", acc, "Off: the selected faces are inset as one region");
            }
        });
        section(ui, "Remove and merge", acc, |ui| {
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Vertices").icon(Icon::Trash).accent(DANGER()).tip("Delete the selected vertices and what uses them").show(ui).clicked() {
                    self.ed.edit_delete(DeleteKind::Verts);
                }
                if Btn::new("Edges").accent(DANGER()).show(ui).clicked() {
                    self.ed.edit_delete(DeleteKind::Edges);
                }
                if Btn::new("Faces").accent(DANGER()).show(ui).clicked() {
                    self.ed.edit_delete(DeleteKind::Faces);
                }
                if Btn::new("Only faces").accent(DANGER()).tip("Delete the faces but keep their edges and vertices").show(ui).clicked() {
                    self.ed.edit_delete(DeleteKind::OnlyFaces);
                }
                if b(ui, self, CommandId::MeshDissolve, "Dissolve", None) {
                    run = Some(CommandId::MeshDissolve);
                }
            });
            ui.horizontal_wrapped(|ui| {
                for (k, l) in [(MergeKind::Center, "Merge at centre"), (MergeKind::AtActive, "At last clicked"), (MergeKind::Collapse, "Collapse")] {
                    if Btn::new(l).show(ui).clicked() {
                        let n = self.ed.edit_merge(k);
                        self.info(format!("Merged: {n} vertices removed"));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.edui.merge_dist).speed(0.0005).range(0.0..=1.0).max_decimals(4));
                if Btn::new("Merge by distance").tip("Merge selected vertices closer than this distance (all when nothing is selected)").show(ui).clicked() {
                    let n = self.ed.edit_merge_by_distance(self.edui.merge_dist);
                    self.info(format!("Merged by distance: {n} vertices removed"));
                }
            });
        });
        section(ui, "Faces and normals", acc, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (id, l) in [
                    (CommandId::MeshTriangulate, "Triangulate"),
                    (CommandId::MeshTrisToQuads, "Tris to quads"),
                    (CommandId::MeshFlipNormals, "Flip normals"),
                    (CommandId::MeshRecalcNormals, "Recalculate normals"),
                    (CommandId::MeshShadeSmooth, "Shade smooth"),
                    (CommandId::MeshShadeFlat, "Shade flat"),
                    (CommandId::MeshSymmetrize, "Symmetrize (−X to +X)"),
                ] {
                    if b(ui, self, id, l, None) {
                        run = Some(id);
                    }
                }
            });
            slider_f(ui, "smooth amount", &mut self.edui.smooth, 0.0..=1.0);
            if b(ui, self, CommandId::MeshSmooth, "Smooth vertices", None) {
                run = Some(CommandId::MeshSmooth);
            }
        });
        section(ui, "UV", acc, |ui| {
            ui.horizontal_wrapped(|ui| {
                if b(ui, self, CommandId::MeshMarkSeam, "Mark seam", None) {
                    run = Some(CommandId::MeshMarkSeam);
                }
                if b(ui, self, CommandId::MeshClearSeam, "Clear seam", None) {
                    run = Some(CommandId::MeshClearSeam);
                }
            });
            ui.horizontal_wrapped(|ui| {
                let (_, w, h) = self.edit_mouse();
                for (how, l, tip) in [(0u8, "Unwrap", "Flattens the selected faces along their seams (angle based)"), (1, "Cube projection", "Projects from the six box sides"), (2, "From view", "Projects the faces as they are seen in the viewport")] {
                    if Btn::new(l).tip(tip).show(ui).clicked() {
                        let n = self.ed.edit_unwrap(how, w, h);
                        self.info(format!("UV: {n} island(s)"));
                    }
                }
            });
        });
        section(ui, "Boolean inside the mesh", acc, |ui| {
            dim(ui, "The selected polygons cut or join the rest of this mesh.");
            ui.horizontal_wrapped(|ui| {
                for (op, l) in [(BoolOp::Union, "Union"), (BoolOp::Difference, "Difference"), (BoolOp::Intersect, "Intersect")] {
                    if Btn::new(l).icon(Icon::Boolean).show(ui).clicked() && !self.ed.edit_boolean_selected(op) {
                        self.info("Boolean: select closed polygons that overlap the rest of the mesh");
                    }
                }
            });
        });
        section(ui, "Add to this mesh", acc, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (pr, icon, tip) in [(Primitive::Cube, Icon::Cube, "Cube"), (Primitive::Plane, Icon::Plane, "Plane"), (Primitive::Cylinder, Icon::Cylinder, "Cylinder"), (Primitive::Cone, Icon::Cone, "Cone"), (Primitive::Sphere, Icon::Sphere, "Sphere"), (Primitive::Torus, Icon::Torus, "Torus")] {
                    if Btn::new("").icon(icon).tip(tip).show(ui).clicked() {
                        self.ed.edit_add_primitive(pr);
                    }
                }
            });
        });
        section(ui, "Options", acc, |ui| {
            if let Some(s) = self.ed.edit.as_mut() {
                toggle_tip(ui, &mut s.xray, "X-ray", acc, "See and select through the mesh (Alt+Z)");
                toggle_tip(ui, &mut s.proportional, "Proportional editing", acc, "Nearby vertices follow with a soft falloff (O)");
                if s.proportional {
                    slider_f(ui, "radius", &mut s.prop_radius, 0.01..=10.0);
                }
                toggle_tip(ui, &mut s.mirror_x, "Mirror X", acc, "Moves the matching vertex on the other side of X too");
            }
            dim(ui, "Click selects · Shift adds · Ctrl shortest path · Alt edge loop · Ctrl+Alt ring · L linked · box: drag");
        });
        if let Some(id) = run {
            self.run_command(ctx, id);
        }
    }
}
