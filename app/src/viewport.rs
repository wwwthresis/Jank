//! Central 3D viewport: shows the render, camera navigation, per-mode mouse tools and overlays.
use crate::theme::*;
use crate::{compat, dialogs, widgets, widgets::Btn, App, AnimTool, ModelTool, SURF_GHOST_NEXT, SURF_GHOST_PREV, SURF_VIEW};
use eframe::egui::{self, pos2, vec2, Align2, Color32, PointerButton, Pos2, Rect, Sense, Shape, Ui};
use psxcore::editor::{Canvas, Mode};
use psxcore::math::Vec3;
use psxcore::raster::{Camera, Frame, Renderer};

#[derive(Clone, Copy, PartialEq)]
pub enum Drag {
    Paint,
    Sculpt,
    Bone(usize),
    Hand(Canvas),
    MoveObject(u32),
}

#[derive(Default)]
pub struct VpState {
    pub drag: Option<Drag>,
    pub last_px: (i32, i32),
    /// Gizmo drag in progress.
    pub gizmo: Option<psxcore::gizmo::GizmoDrag>,
    pub gizmo_hover: Option<psxcore::gizmo::Handle>,
    /// Rotation ring drag on the selected bone: (drag, bone, axis, angle already applied in degrees).
    pub bone_drag: Option<(psxcore::gizmo::GizmoDrag, usize, psxcore::gizmo::Axis, f32)>,
    /// Readout drawn at the top of the viewport while dragging.
    pub gizmo_text: String,
    /// Last point of a freehand drawing stroke, in render pixels.
    pub hand_last: (f32, f32),
}

fn camera_axes(cam: &Camera) -> (Vec3, Vec3, Vec3) {
    let fwd = (cam.target - cam.eye).normalized();
    let right = fwd.cross(Vec3::Y).normalized();
    let up = right.cross(fwd);
    (fwd, right, up)
}

/// Onion-skin ghost of a neighbouring frame as premultiplied RGBA: a crisp outline plus a faint fill, in one colour per direction,
/// so the previous and the next frame can be told apart from each other and from the live picture.
fn ghost_pixels(f: &Frame, tint: Color32, out: &mut Vec<u8>) {
    let (w, h) = (f.w as usize, f.h as usize);
    out.clear();
    out.resize(w * h * 4, 0);
    let drawn = |x: usize, y: usize| f.depth[y * w + x].is_finite();
    let premul = |a: u32| [(tint.r() as u32 * a / 255) as u8, (tint.g() as u32 * a / 255) as u8, (tint.b() as u32 * a / 255) as u8, a as u8];
    let (fill, edge) = (premul(46), premul(235));
    for y in 0..h {
        for x in 0..w {
            if !drawn(x, y) {
                continue;
            }
            let border = x == 0 || y == 0 || x + 1 == w || y + 1 == h || !drawn(x - 1, y) || !drawn(x + 1, y) || !drawn(x, y - 1) || !drawn(x, y + 1);
            out[(y * w + x) * 4..][..4].copy_from_slice(if border { &edge } else { &fill });
        }
    }
}

/// Cheap key that changes whenever the ghost of a frame has to be drawn again.
fn ghost_key(idx: usize, cam: &Camera, w: u32, h: u32, anim: usize, frames: usize) -> u64 {
    let mut k = 0xcbf29ce484222325u64;
    let mut mix = |x: u64| k = (k ^ x).wrapping_mul(0x100000001b3);
    mix(idx as u64);
    mix(((w as u64) << 32) | h as u64);
    mix(anim as u64);
    mix(frames as u64);
    for v in [cam.eye.x, cam.eye.y, cam.eye.z, cam.target.x, cam.target.y, cam.target.z, cam.fov_y] {
        mix(v.to_bits() as u64);
    }
    k
}

/// Pixel size the main viewport is drawn at for a widget of `rect` points.
fn draw_size(rect: Rect, ppp: f32, scale: f32) -> (u32, u32) {
    let f = |v: f32| ((v * ppp * scale).round() as u32).clamp(8, 4096);
    (f(rect.width()), f(rect.height()))
}

#[derive(Default)]
pub struct GhostCache {
    prev_key: u64,
    next_key: u64,
    rend: Option<Renderer>,
    bytes: Vec<u8>,
}

impl App {
    pub fn viewport_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
        let ppp = ctx.pixels_per_point();

        // ---- draw the main viewport at the size of the widget (times the quality scale), independent of the render resolution
        let scale = self.perf.scale(self.settings.viewport_quality);
        let (w, h) = draw_size(rect, ppp, scale);
        let t0 = std::time::Instant::now();
        self.rend.threads = if self.settings.render_threads == 0 { psxcore::raster::default_threads() } else { self.settings.render_threads as usize };
        self.ed.render_viewport_sized(&mut self.rend, w, h);
        if let Some(gl) = self.gl.clone() {
            self.surf[SURF_VIEW].upload(&gl, w, h, self.rend.frame.rgba());
        }
        self.perf.note_draw(t0.elapsed().as_secs_f32() * 1000.0);

        // ---- onion-skin ghosts (redrawn only when their frame, the camera or the size changes)
        let ghosts = self.ed.mode == Mode::Animate && self.ed.onion && !self.ed.playing;
        if ghosts {
            let (prev, next) = self.ed.onion_indices();
            let cam = self.ed.active_camera();
            let (anim, frames) = (self.ed.current_anim, self.ed.animation().map_or(0, |a| a.frames.len()));
            for (slot, idx, tint) in [(0usize, prev, ONION_PREV), (1, next, ONION_NEXT)] {
                let key = idx.map_or(0, |i| ghost_key(i, &cam, w, h, anim, frames));
                let cache = &mut self.ghost;
                let old = if slot == 0 { &mut cache.prev_key } else { &mut cache.next_key };
                if *old == key {
                    continue;
                }
                *old = key;
                let surf = &mut self.surf[SURF_GHOST_PREV + slot];
                match idx {
                    Some(i) => {
                        let r = cache.rend.get_or_insert_with(|| Renderer::new(1, 1));
                        r.threads = self.rend.threads;
                        // draw the neighbour at the same size as the viewport
                        if self.ed.render_frame_sized(i, r, w, h) {
                            ghost_pixels(&r.frame, tint, &mut cache.bytes);
                            if let Some(gl) = &self.gl {
                                surf.upload(gl, w, h, &cache.bytes);
                            }
                        }
                    }
                    None => {
                        if let Some(gl) = &self.gl {
                            cache.bytes.clear();
                            cache.bytes.resize((w * h * 4) as usize, 0);
                            surf.upload(gl, w, h, &cache.bytes);
                        }
                    }
                }
            }
        }

        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, compat::cr(0.0), BG0);
        // the picture fills the whole widget: `sc` is how many screen points one drawn pixel covers
        let img = rect;
        let sc = rect.width() / w as f32;
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        if let Some(id) = self.surf[SURF_VIEW].id() {
            painter.image(id, img, full, Color32::WHITE);
        }
        if ghosts {
            for s in [SURF_GHOST_PREV, SURF_GHOST_NEXT] {
                if let Some(id) = self.surf[s].id() {
                    painter.image(id, img, full, Color32::WHITE);
                }
            }
        }

        self.vp_input(&ctx, &resp, img, sc, w, h);
        self.vp_overlays(&painter, img, sc, w, h);
        self.gizmo_draw(&painter, img, sc, w, h);
        self.gizmo_bar(&ctx, img);
        self.preview_ui(&ctx, img);
        if self.settings.show_stats {
            self.stats_overlay(&painter, img, w, h, scale);
        }
        if self.ed.project.scene.objects.is_empty() {
            self.welcome(&ctx);
        }
    }

    fn stats_overlay(&self, p: &egui::Painter, img: Rect, w: u32, h: u32, scale: f32) {
        let tris = self.ed.project.scene.objects.iter().filter(|o| o.visible).filter_map(|o| self.ed.project.assets.get(o.asset)).map(|a| a.model.triangle_count()).sum::<usize>();
        let lines = [
            format!("{:.0} fps   {:.1} ms frame", self.perf.fps(), self.perf.frame_ms),
            format!("viewport {:.1} ms   {}x{} ({:.0}%)", self.perf.draw_ms, w, h, scale * 100.0),
            format!("preview {:.1} ms   {} tris   {} threads", self.perf.preview_ms, tris, self.rend.threads),
        ];
        let pos = pos2(img.max.x - 10.0, img.min.y + 10.0);
        let bg = Rect::from_min_size(pos2(pos.x - 262.0, pos.y - 4.0), vec2(266.0, 14.0 * 3.0 + 10.0));
        p.rect_filled(bg, compat::cr(2.0), alpha(BG0, 215));
        for (i, l) in lines.iter().enumerate() {
            compat::text(p, pos + vec2(0.0, i as f32 * 14.0), Align2::RIGHT_TOP, l, 12.0, TEXT);
        }
    }

    fn vp_input(&mut self, ctx: &egui::Context, resp: &egui::Response, img: Rect, sc: f32, w: u32, h: u32) {
        let (pos, pressed, down, mods) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
        let hover = resp.hovered();
        let to_px = |p: Pos2| (((p.x - img.min.x) / sc).floor() as i32, ((p.y - img.min.y) / sc).floor() as i32);
        let render_mode = self.ed.mode == Mode::Render;

        // navigation: middle/right button or Alt+left to orbit; Shift pans; the wheel zooms
        let nav = resp.dragged_by(PointerButton::Middle) || resp.dragged_by(PointerButton::Secondary) || (resp.dragged_by(PointerButton::Primary) && mods.alt);
        if nav {
            let d = resp.drag_delta();
            if mods.shift {
                self.ed.view.pan(d.x, d.y, img.height());
            } else {
                self.ed.view.orbit(d.x, d.y);
            }
            self.ed.through_camera = false;
        }
        if hover {
            let s = compat::scroll_y(ctx);
            if s != 0.0 {
                self.ed.view.zoom(s * 0.05);
                self.ed.through_camera = false;
            }
        }
        if render_mode || mods.alt {
            return;
        }
        if self.vp.drag.is_none() && self.gizmo_input(ctx, img, sc, w, h, hover) {
            return;
        }

        if pressed && hover && self.vp.drag.is_none() {
            if let Some(p) = pos {
                let px = to_px(p);
                self.vp.last_px = px;
                self.begin_tool(px, mods.shift, sc, w, h);
            }
        }
        if let (Some(drag), true) = (self.vp.drag, down) {
            if let Some(p) = pos {
                let cur = to_px(p);
                if cur != self.vp.last_px {
                    self.drag_tool(drag, cur, w, h);
                    self.vp.last_px = cur;
                }
            }
            ctx.request_repaint();
        }
        if self.vp.drag.is_some() && !down {
            self.end_tool();
        }
        if hover && self.vp.drag.is_none() {
            let icon = match (self.ed.mode, self.anim_tool, self.model_tool) {
                (Mode::Texture, _, _) | (Mode::Animate, AnimTool::Hand, _) | (Mode::Animate, AnimTool::PaintModel, _) => egui::CursorIcon::Crosshair,
                _ => egui::CursorIcon::Default,
            };
            ctx.set_cursor_icon(icon);
        }
    }

    fn begin_tool(&mut self, px: (i32, i32), shift: bool, sc: f32, w: u32, h: u32) {
        let (x, y) = px;
        match self.ed.mode {
            Mode::Model => match self.model_tool {
                ModelTool::Select => self.ed.click_select(&self.rend.frame, x, y, shift),
                ModelTool::Sculpt => {
                    if self.ed.sculpt_begin(&self.rend.frame, x, y) {
                        self.vp.drag = Some(Drag::Sculpt);
                    }
                }
            },
            Mode::Texture => self.begin_paint(px),
            Mode::Pose => self.begin_pose(px, sc, w, h),
            Mode::Animate => match self.anim_tool {
                AnimTool::Pose => self.begin_pose(px, sc, w, h),
                AnimTool::PaintModel => self.begin_paint(px),
                AnimTool::Hand => {
                    if let Some(c) = self.current_canvas() {
                        let p = self.to_render_px(px, w, h);
                        self.vp.hand_last = p;
                        self.ed.canvas_press(c, p);
                        self.vp.drag = Some(Drag::Hand(c));
                    }
                }
                AnimTool::MoveObject => {
                    self.select_under(px);
                    if let Some(id) = self.ed.selected_object {
                        self.ed.begin_group();
                        self.vp.drag = Some(Drag::MoveObject(id));
                    }
                }
            },
            Mode::Render => {}
        }
    }
    /// Viewport pixel -> pixel of the final picture. The picture is the middle of the viewport: its height spans the whole
    /// view height (the vertical field of view is shared), so the horizontal offset is centred.
    fn to_render_px(&self, px: (i32, i32), w: u32, h: u32) -> (f32, f32) {
        let (rw, rh) = self.ed.render_size();
        let k = rh as f32 / h.max(1) as f32;
        ((px.0 as f32 + 0.5 - w as f32 * 0.5) * k + rw as f32 * 0.5, (px.1 as f32 + 0.5) * k)
    }
    fn select_under(&mut self, px: (i32, i32)) {
        let hit = self.rend.frame.hit_at(px.0, px.1);
        if let Some(info) = self.ed.pick_info(hit) {
            if self.ed.selected_object != Some(info.object) {
                self.ed.select_object(Some(info.object));
            }
        }
    }
    fn begin_paint(&mut self, px: (i32, i32)) {
        if self.ed.paint_press(&self.rend.frame, px.0, px.1) {
            if !matches!(self.ed.brush.tool, psxcore::paint::Tool::Picker) {
                self.remember_color();
            }
            self.vp.drag = Some(Drag::Paint);
        } else {
            self.select_under(px);
        }
    }
    fn begin_pose(&mut self, px: (i32, i32), sc: f32, w: u32, h: u32) {
        self.select_under(px);
        let radius = (14.0 / sc).max(2.0);
        if let Some(b) = self.ed.pick_bone(px.0 as f32 + 0.5, px.1 as f32 + 0.5, w, h, radius) {
            self.ed.begin_group();
            self.vp.drag = Some(Drag::Bone(b));
        }
    }

    fn drag_tool(&mut self, drag: Drag, cur: (i32, i32), _w: u32, h: u32) {
        let last = self.vp.last_px;
        let (dx, dy) = ((cur.0 - last.0) as f32, (cur.1 - last.1) as f32);
        let cam = self.ed.active_camera();
        let (fwd, right, up) = camera_axes(&cam);
        // world-space distance of one render pixel at the target distance
        let k = 2.0 * (cam.target - cam.eye).length() * (cam.fov_y * 0.5).tan() / h.max(1) as f32;
        match drag {
            Drag::Paint => {
                self.ed.paint_drag(&self.rend.frame, last, cur);
            }
            Drag::Hand(c) => {
                let p = self.to_render_px(cur, _w, h);
                self.vp.hand_last = p;
                self.ed.canvas_drag(c, p);
            }
            Drag::Sculpt => {
                if let Some(id) = self.ed.selected_object {
                    let world = right * (dx * k) + up * (-dy * k);
                    let model = self.ed.world_to_model_dir(id, world);
                    self.ed.sculpt_drag(model, self.sculpt_radius);
                }
            }
            Drag::Bone(b) => {
                // horizontal drag tilts within the screen plane, vertical drag tilts forward/back
                if dx != 0.0 {
                    self.ed.rotate_bone_world(b, fwd, dx * 0.6);
                }
                if dy != 0.0 {
                    self.ed.rotate_bone_world(b, right, dy * 0.6);
                }
            }
            Drag::MoveObject(id) => {
                if let Some(mut t) = self.ed.object_transform(id) {
                    let d = right * (dx * k) + up * (-dy * k);
                    t.pos = t.pos + d;
                    self.ed.set_object_transform(id, t);
                }
            }
        }
    }
    fn end_tool(&mut self) {
        match self.vp.drag.take() {
            Some(Drag::Paint) => self.ed.paint_release(),
            Some(Drag::Sculpt) => self.ed.sculpt_end(),
            Some(Drag::Hand(c)) => {
                let p = self.vp.hand_last;
                self.ed.canvas_release(c, p);
            }
            Some(Drag::Bone(_)) | Some(Drag::MoveObject(_)) => self.ed.end_group(),
            None => {}
        }
    }

    fn vp_overlays(&mut self, p: &egui::Painter, img: Rect, sc: f32, w: u32, h: u32) {
        let at = |x: f32, y: f32| pos2(img.min.x + x * sc, img.min.y + y * sc);
        let cam = self.ed.active_camera();
        // selected faces
        for tri in self.ed.selected_face_world_tris() {
            let pts: Vec<Pos2> = tri.iter().filter_map(|v| cam.project(*v, w, h)).map(|(x, y, _)| at(x, y)).collect();
            if pts.len() == 3 {
                p.add(Shape::convex_polygon(pts, alpha(ACCENT, 70), crate::compat::stroke(2.0, ACCENT)));
            }
        }
        // skeleton
        let show_bones = self.ed.mode == Mode::Pose || (self.ed.mode == Mode::Animate && self.anim_tool == AnimTool::Pose);
        if show_bones {
            for b in self.ed.screen_bones(w, h) {
                let sel = self.ed.selected_bone == Some(b.bone);
                let col = if sel { ACCENT } else { ACCENT };
                if b.head != b.tail {
                    p.line_segment([at(b.head.0, b.head.1), at(b.tail.0, b.tail.1)], crate::compat::stroke(if sel { 4.0 } else { 2.5 }, col));
                }
                p.circle_filled(at(b.tail.0, b.tail.1), if sel { 7.0 } else { 5.0 }, col);
            }
        }
        // corner caption
        let tris = self.ed.selected_model().map_or(0, |m| m.triangle_count());
        let info = format!("{}   {} tris", crate::mode_label(self.ed.mode), tris);
        compat::text(p, img.min + vec2(10.0, 10.0), Align2::LEFT_TOP, &info, 13.0, alpha(Color32::WHITE, 200));
        if self.ed.playing {
            compat::text(p, img.max - vec2(10.0, 10.0), Align2::RIGHT_BOTTOM, "▶", 16.0, ACCENT);
        }
        if let Some(f) = self.ed.frame().filter(|_| matches!(self.ed.mode, Mode::Pose | Mode::Animate | Mode::Render)) {
            if !self.ed.through_camera {
                compat::text(p, img.min + vec2(10.0, 28.0), Align2::LEFT_TOP, "free view: the frame camera is not changed", 12.0, alpha(WARN, 220));
            }
            if !f.note.is_empty() {
                compat::text(p, pos2(img.center().x, img.max.y - 12.0), Align2::CENTER_BOTTOM, &f.note, 14.0, alpha(Color32::WHITE, 220));
            }
        }
    }

    /// Welcome window shown for an empty project.
    fn welcome(&mut self, ctx: &egui::Context) {
        egui::Window::new("Welcome to Jank!").collapsible(false).resizable(false).default_pos(compat::centered_pos(ctx, vec2(380.0, 160.0))).show(ctx, |ui| {
            ui.set_min_width(360.0);
            widgets::dim(ui, "Import a model or start from a primitive, then paint right on it.");
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Import model").icon(crate::icons::Icon::Open).accent(ACCENT).show(ui).clicked() {
                    self.dialog = Some(dialogs::browser(dialogs::Purpose::ImportModel));
                }
                if Btn::new("Cube").icon(crate::icons::Icon::Cube).accent(ACCENT).show(ui).clicked() {
                    self.ed.add_primitive(psxcore::editor::Primitive::Cube);
                }
                if Btn::new("Open project").icon(crate::icons::Icon::Save).accent(ACCENT).show(ui).clicked() {
                    self.dialog = Some(dialogs::browser(dialogs::Purpose::OpenProject));
                }
            });
        });
    }
}
