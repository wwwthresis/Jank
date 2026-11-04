//! Viewport side of the transform gizmos: the tool strip, drawing of the handles and the
//! pointer handling that starts, updates and ends a drag. The geometry lives in `psxcore::gizmo`.

use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::Btn;
use crate::{compat, App, AnimTool, ModelTool};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Pos2, Rect, Shape};
use psxcore::editor::Mode;
use psxcore::gizmo::{self, Axis, GizmoDrag, GizmoMode, Handle, Snap, View};
use psxcore::math::Vec3;

/// User-facing gizmo preferences.
pub struct GizmoPrefs {
    /// `None` = selection only (no handles).
    pub mode: Option<GizmoMode>,
    pub local: bool,
    pub snap: bool,
    pub steps: Snap,
}

impl Default for GizmoPrefs {
    fn default() -> Self {
        GizmoPrefs { mode: Some(GizmoMode::Translate), local: false, snap: false, steps: Snap::default() }
    }
}

fn axis_color(a: Axis) -> Color32 {
    match a {
        Axis::X => AXIS_X,
        Axis::Y => AXIS_Y,
        Axis::Z => AXIS_Z,
    }
}

impl App {
    /// True when handles should be shown and can be grabbed.
    pub fn gizmo_visible(&self) -> bool {
        if self.gizmo.mode.is_none() || self.ed.selected_object.is_none() {
            return false;
        }
        match self.ed.mode {
            Mode::Model => self.model_tool == ModelTool::Select,
            Mode::Animate => self.anim_tool == AnimTool::MoveObject,
            _ => false,
        }
    }

    /// The selected bone and its joint position when the rotation rings of a bone should be shown.
    pub fn bone_gizmo_target(&self) -> Option<(usize, Vec3)> {
        let posing = self.ed.mode == Mode::Pose || (self.ed.mode == Mode::Animate && self.anim_tool == AnimTool::Pose);
        if !posing || self.gizmo.mode.is_none() {
            return None;
        }
        let b = self.ed.selected_bone?;
        Some((b, self.ed.bone_joint_world(b)?))
    }

    /// Bone rings are a little smaller than the object handles; picking and drawing must use the same size.
    fn bone_gizmo_view(&self, sc: f32, w: u32, h: u32) -> View {
        let v = self.gizmo_view(sc, w, h);
        View { size_px: v.size_px * 0.8, ..v }
    }

    fn bone_transform(joint: Vec3) -> psxcore::scene::Transform {
        psxcore::scene::Transform { pos: joint, rot_deg: Vec3::ZERO, scale: Vec3::ONE }
    }

    /// Rotation rings on the selected bone: drag a ring to turn the bone around that world axis.
    fn bone_gizmo_input(&mut self, ctx: &egui::Context, img: Rect, sc: f32, w: u32, h: u32, hover: bool, bone: usize, joint: Vec3) -> bool {
        let (pos, pressed, down, mods) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
        let view = self.bone_gizmo_view(sc, w, h);
        let to_px = |p: Pos2| ((p.x - img.min.x) / sc, (p.y - img.min.y) / sc);
        let cam = self.ed.active_camera();
        if let Some((drag, b, axis, applied)) = self.vp.bone_drag.clone() {
            if down {
                if let Some(p) = pos {
                    let t = drag.update(&cam, to_px(p), view, self.gizmo_snap(mods.ctrl));
                    let total = match axis {
                        Axis::X => t.rot_deg.x,
                        Axis::Y => t.rot_deg.y,
                        Axis::Z => t.rot_deg.z,
                    };
                    let delta = total - applied;
                    if delta.abs() > 1e-3 {
                        self.ed.rotate_bone_world(b, axis.unit(), delta);
                    }
                    self.vp.gizmo_text = format!("Rotate bone  {total:+.1} deg");
                    self.vp.bone_drag = Some((drag, b, axis, total));
                }
                ctx.request_repaint();
            } else {
                self.ed.end_group();
                self.vp.bone_drag = None;
            }
            return true;
        }
        self.vp.gizmo_hover = None;
        self.vp.gizmo_text.clear();
        if mods.alt {
            return false;
        }
        let t = Self::bone_transform(joint);
        if hover {
            if let Some(p) = pos {
                self.vp.gizmo_hover = gizmo::pick_handle(GizmoMode::Rotate, &cam, &t, false, to_px(p), view, (9.0 / sc).max(3.0));
            }
        }
        if pressed && hover {
            if let (Some(handle @ Handle::Axis(axis)), Some(p)) = (self.vp.gizmo_hover, pos) {
                if let Some(d) = GizmoDrag::begin(GizmoMode::Rotate, handle, &t, false, &cam, to_px(p), view) {
                    self.ed.begin_group();
                    self.vp.bone_drag = Some((d, bone, axis, 0.0));
                    return true;
                }
            }
        }
        if self.vp.gizmo_hover.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        false
    }

    fn bone_gizmo_draw(&self, p: &egui::Painter, img: Rect, sc: f32, w: u32, h: u32, joint: Vec3) {
        let cam = self.ed.active_camera();
        let view = self.bone_gizmo_view(sc, w, h);
        let at = |v: Vec3| cam.project(v, w, h).map(|(x, y, _)| pos2(img.min.x + x * sc, img.min.y + y * sc));
        let t = Self::bone_transform(joint);
        let len = gizmo::world_size(&cam, joint, h, view.size_px);
        let dirs = gizmo::axes(&t, false);
        let hot = self.vp.bone_drag.as_ref().map(|d| Handle::Axis(d.2)).or(self.vp.gizmo_hover);
        for ax in Axis::ALL {
            let hdl = Handle::Axis(ax);
            let col = if hot == Some(hdl) { mix(axis_color(ax), Color32::WHITE, 0.55) } else { axis_color(ax) };
            let pts: Vec<Pos2> = gizmo::ring_points(joint, &dirs, ax, len, 64).iter().filter_map(|v| at(*v)).collect();
            if pts.len() > 2 {
                p.add(Shape::line(pts, compat::stroke(if hot == Some(hdl) { 4.5 } else { 3.0 }, col)));
            }
        }
        if let Some(o) = at(joint) {
            p.circle_filled(o, 3.5, Color32::WHITE);
        }
        if !self.vp.gizmo_text.is_empty() {
            compat::text(p, pos2(img.center().x, img.min.y + 14.0), Align2::CENTER_TOP, &self.vp.gizmo_text, 17.0, TEXT);
        }
    }

    /// Handle length in render pixels. About 110 screen pixels whatever the zoom of the image.
    pub fn gizmo_view(&self, sc: f32, w: u32, h: u32) -> View {
        View { w, h, size_px: (110.0 / sc).clamp(22.0, 90.0) }
    }

    pub fn gizmo_snap(&self, ctrl: bool) -> Snap {
        Snap { enabled: self.gizmo.snap != ctrl, ..self.gizmo.steps }
    }

    /// Handles pointer input for the gizmo. Returns true when the gizmo consumed the event, so
    /// the normal tool of the mode must not start.
    pub fn gizmo_input(&mut self, ctx: &egui::Context, img: Rect, sc: f32, w: u32, h: u32, hover: bool) -> bool {
        if let Some((b, joint)) = self.bone_gizmo_target() {
            return self.bone_gizmo_input(ctx, img, sc, w, h, hover, b, joint);
        }
        let (pos, pressed, down, mods) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
        let view = self.gizmo_view(sc, w, h);
        let to_px = |p: Pos2| ((p.x - img.min.x) / sc, (p.y - img.min.y) / sc);
        // a drag in progress
        if let Some(drag) = self.vp.gizmo.clone() {
            let Some(id) = self.ed.selected_object else {
                self.vp.gizmo = None;
                return false;
            };
            if down {
                if let Some(p) = pos {
                    let cam = self.ed.active_camera();
                    let t = drag.update(&cam, to_px(p), view, self.gizmo_snap(mods.ctrl));
                    self.vp.gizmo_text = describe(&drag, &t);
                    self.ed.set_object_transform(id, t);
                }
                ctx.request_repaint();
            } else {
                self.ed.end_group();
                self.vp.gizmo = None;
            }
            return true;
        }
        self.vp.gizmo_hover = None;
        self.vp.gizmo_text.clear();
        if !self.gizmo_visible() || mods.alt {
            return false;
        }
        let (Some(id), Some(mode)) = (self.ed.selected_object, self.gizmo.mode) else { return false };
        let Some(t) = self.ed.object_transform(id) else { return false };
        let cam = self.ed.active_camera();
        if hover {
            if let Some(p) = pos {
                self.vp.gizmo_hover = gizmo::pick_handle(mode, &cam, &t, self.gizmo.local, to_px(p), view, (9.0 / sc).max(3.0));
            }
        }
        if pressed && hover {
            if let (Some(handle), Some(p)) = (self.vp.gizmo_hover, pos) {
                if let Some(d) = GizmoDrag::begin(mode, handle, &t, self.gizmo.local, &cam, to_px(p), view) {
                    self.ed.begin_group();
                    self.vp.gizmo = Some(d);
                    return true;
                }
            }
        }
        if self.vp.gizmo_hover.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        false
    }

    /// Draws the handles over the viewport image.
    pub fn gizmo_draw(&self, p: &egui::Painter, img: Rect, sc: f32, w: u32, h: u32) {
        if let Some((_, joint)) = self.bone_gizmo_target() {
            self.bone_gizmo_draw(p, img, sc, w, h, joint);
            return;
        }
        if !self.gizmo_visible() {
            return;
        }
        let (Some(id), Some(mode)) = (self.ed.selected_object, self.gizmo.mode) else { return };
        let Some(t) = self.ed.object_transform(id) else { return };
        let cam = self.ed.active_camera();
        let view = self.gizmo_view(sc, w, h);
        let at = |v: Vec3| cam.project(v, w, h).map(|(x, y, _)| pos2(img.min.x + x * sc, img.min.y + y * sc));
        let Some(o) = at(t.pos) else { return };
        let len = gizmo::world_size(&cam, t.pos, h, view.size_px);
        let dirs = gizmo::axes(&t, self.gizmo.local || mode == GizmoMode::Scale);
        let active = self.vp.gizmo.as_ref().map(|d| d.handle);
        let hot = active.or(self.vp.gizmo_hover);
        let lit = |h: Handle, base: Color32| if hot == Some(h) { mix(base, Color32::WHITE, 0.55) } else { base };
        let width = |h: Handle| if hot == Some(h) { 4.5 } else { 3.0 };
        match mode {
            GizmoMode::Translate => {
                for ax in Axis::ALL {
                    let c = gizmo::plane_handle_corners(t.pos, &dirs, ax, len);
                    let pts: Vec<Pos2> = c.iter().filter_map(|v| at(*v)).collect();
                    if pts.len() == 4 {
                        let hdl = Handle::Plane(ax);
                        p.add(Shape::convex_polygon(pts, alpha(lit(hdl, axis_color(ax)), if hot == Some(hdl) { 150 } else { 70 }), compat::stroke(1.5, lit(hdl, axis_color(ax)))));
                    }
                }
                for ax in Axis::ALL {
                    let Some(e) = at(t.pos + dirs[ax.index()] * len) else { continue };
                    let hdl = Handle::Axis(ax);
                    let col = lit(hdl, axis_color(ax));
                    p.line_segment([o, e], compat::stroke(width(hdl), col));
                    let d = (e - o).normalized();
                    let n = vec2(-d.y, d.x);
                    p.add(Shape::convex_polygon(vec![e + d * 12.0, e - d * 4.0 + n * 6.0, e - d * 4.0 - n * 6.0], col, compat::stroke(1.0, col)));
                }
            }
            GizmoMode::Scale => {
                for ax in Axis::ALL {
                    let Some(e) = at(t.pos + dirs[ax.index()] * len) else { continue };
                    let hdl = Handle::Axis(ax);
                    let col = lit(hdl, axis_color(ax));
                    p.line_segment([o, e], compat::stroke(width(hdl), col));
                    p.rect_filled(Rect::from_center_size(e, vec2(11.0, 11.0)), compat::cr(2.0), col);
                }
                let col = lit(Handle::Uniform, TEXT);
                p.rect_filled(Rect::from_center_size(o, vec2(15.0, 15.0)), compat::cr(3.0), alpha(col, 200));
                compat::stroke_rect(p, Rect::from_center_size(o, vec2(15.0, 15.0)), 3.0, compat::stroke(1.5, col));
            }
            GizmoMode::Rotate => {
                for ax in Axis::ALL {
                    let hdl = Handle::Axis(ax);
                    let pts: Vec<Pos2> = gizmo::ring_points(t.pos, &dirs, ax, len, 64).iter().filter_map(|v| at(*v)).collect();
                    if pts.len() > 2 {
                        p.add(Shape::line(pts, compat::stroke(width(hdl), lit(hdl, axis_color(ax)))));
                    }
                }
            }
        }
        p.circle_filled(o, 3.5, Color32::WHITE);
        if !self.vp.gizmo_text.is_empty() {
            let r = compat::text(p, pos2(img.center().x, img.min.y + 14.0), Align2::CENTER_TOP, &self.vp.gizmo_text, 17.0, TEXT);
            let _ = r;
        }
    }

    /// Small strip with the gizmo tools, shown at the top left of the viewport.
    pub fn gizmo_bar(&mut self, ctx: &egui::Context, img: Rect) {
        let relevant = match self.ed.mode {
            Mode::Model => self.model_tool == ModelTool::Select,
            Mode::Animate => matches!(self.anim_tool, AnimTool::MoveObject | AnimTool::Pose),
            Mode::Pose => true,
            _ => false,
        };
        if !relevant {
            return;
        }
        egui::Area::new(egui::Id::new("gizmo_bar")).fixed_pos(img.min + vec2(10.0, 54.0)).order(egui::Order::Foreground).constrain_to(img).show(ctx, |ui| {
            ui.horizontal(|ui| {
                use psxcore::commands::CommandId as C;
                for (m, icon, id, label) in [
                    (None, Icon::Select, C::GizmoSelect, "Select only"),
                    (Some(GizmoMode::Translate), Icon::Move, C::GizmoMove, "Move"),
                    (Some(GizmoMode::Rotate), Icon::Rotate, C::GizmoRotate, "Rotate"),
                    (Some(GizmoMode::Scale), Icon::Scale, C::GizmoScale, "Scale"),
                ] {
                    let tip = self.tip_for(id, label);
                    if Btn::new("").icon(icon).accent(ACCENT).on(self.gizmo.mode == m).size(vec2(34.0, 30.0)).tip(&tip).show(ui).clicked() {
                        self.gizmo.mode = m;
                    }
                }
                let tip = self.tip_for(C::GizmoToggleSpace, "World or local axes");
                if Btn::new(if self.gizmo.local { "Local" } else { "World" }).accent(ACCENT).size(vec2(70.0, 30.0)).tip(&tip).show(ui).clicked() {
                    self.gizmo.local = !self.gizmo.local;
                }
                let tip = self.tip_for(C::GizmoToggleSnap, "Snap to steps (hold Ctrl for a one-off snap)");
                if Btn::new("Snap").accent(ACCENT).on(self.gizmo.snap).size(vec2(60.0, 30.0)).tip(&tip).show(ui).clicked() {
                    self.gizmo.snap = !self.gizmo.snap;
                }
            });
        });
    }
}

/// Readout shown while dragging, comparing the new transform to the one at the start.
fn describe(d: &GizmoDrag, t: &psxcore::scene::Transform) -> String {
    let s = d.start_transform();
    match (d.mode, d.handle) {
        (GizmoMode::Translate, _) => {
            let dv = t.pos - s.pos;
            format!("Move  x {:+.2}   y {:+.2}   z {:+.2}", dv.x, dv.y, dv.z)
        }
        (GizmoMode::Rotate, Handle::Axis(a)) => {
            let v = match a {
                Axis::X => t.rot_deg.x - s.rot_deg.x,
                Axis::Y => t.rot_deg.y - s.rot_deg.y,
                Axis::Z => t.rot_deg.z - s.rot_deg.z,
            };
            format!("Rotate  {:+.1} deg", v)
        }
        (GizmoMode::Scale, _) => format!("Scale  {:.2} {:.2} {:.2}", t.scale.x, t.scale.y, t.scale.z),
        _ => String::new(),
    }
}
