//! Central 3D viewport: shows the render, camera navigation, per-mode mouse tools and overlays.
use crate::theme::*;
use crate::{compat, dialogs, widgets, widgets::Btn, App, AnimTool, ModelTool, SURF_GHOST_NEXT, SURF_GHOST_PREV, SURF_OVERLAY, SURF_VIEW};
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
    /// Moving a reference picture in the screen plane.
    Ref(usize),
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
    /// Lazy redraw: fingerprint of what the last viewport picture was drawn with.
    pub drawn_key: u64,
    /// Frames still to redraw after the last input, so results applied a frame later are picked up.
    pub settle: u8,
    /// Counts viewport redraws (the overlay is rebuilt when the picture changed).
    pub gen: u64,
    pub overlay_key: u64,
    /// Object outlined because the mouse is over it.
    pub hover_obj: Option<u32>,
    /// Ignore the pointer until every button is up (after the radial menu closed with a click).
    pub block_until_release: bool,
    /// Stabilizer: smoothed brush position and the last painted pixel.
    pub stab: (f32, f32),
    pub stab_last: (i32, i32),
}

/// True when this frame's input may have changed the scene or the view: anything except plain
/// pointer movement with no button held (hovering the panels must not re-render the 3D view).
pub fn scene_input(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.any_down() || i.events.iter().any(|e| !matches!(e, egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_) | egui::Event::PointerGone)))
}

/// Fingerprint of the view state that changes the picture without any input event (playback,
/// resizing, quality scale changes and the camera).
fn view_key(ed: &psxcore::editor::Editor, w: u32, h: u32, cap: Option<psxcore::lighting::ShadowCap>) -> u64 {
    let mut k = 0xcbf29ce484222325u64;
    let mut mix = |x: u64| k = (k ^ x).wrapping_mul(0x100000001b3);
    mix(((w as u64) << 32) | h as u64);
    mix(ed.mode as u64);
    mix(((ed.current_anim as u64) << 32) | ed.current_frame as u64);
    mix(ed.tick as u64);
    mix(ed.playing as u64 | (ed.through_camera as u64) << 1 | (ed.dirty as u64) << 2 | (ed.onion as u64) << 3);
    mix(ed.selected_object.map_or(u64::MAX, |v| v as u64));
    mix(ed.project.scene.objects.len() as u64);
    mix(cap.map_or(0, |c| 1 + c.mode as u64 * 16 + c.quality as u64));
    let cam = ed.active_camera();
    for v in [cam.eye.x, cam.eye.y, cam.eye.z, cam.target.x, cam.target.y, cam.target.z, cam.fov_y] {
        mix(v.to_bits() as u64);
    }
    k
}

fn camera_axes(cam: &Camera) -> (Vec3, Vec3, Vec3) {
    let fwd = (cam.target - cam.eye).normalized();
    let right = fwd.cross(Vec3::Y).normalized();
    let up = right.cross(fwd);
    (fwd, right, up)
}

/// Onion-skin ghost of a neighbouring frame as premultiplied RGBA: a crisp outline plus a faint fill, in one colour per direction,
/// so the previous and the next frame can be told apart from each other and from the live picture.
fn ghost_pixels(f: &Frame, tint: Color32, opacity: f32, out: &mut Vec<u8>) {
    let (w, h) = (f.w as usize, f.h as usize);
    out.clear();
    out.resize(w * h * 4, 0);
    let drawn = |x: usize, y: usize| f.depth[y * w + x].is_finite();
    let premul = |a: u32| [(tint.r() as u32 * a / 255) as u8, (tint.g() as u32 * a / 255) as u8, (tint.b() as u32 * a / 255) as u8, a as u8];
    let o = opacity.clamp(0.0, 1.0) / 0.6; // 0.6 is the default strength the base values were tuned for
    let (fill, edge) = (premul((46.0 * o).round().min(255.0) as u32), premul((235.0 * o.min(255.0 / 235.0)).round() as u32));
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
fn ghost_key(idx: usize, cam: &Camera, w: u32, h: u32, anim: usize, frames: usize, opacity: f32) -> u64 {
    let mut k = 0xcbf29ce484222325u64;
    let mut mix = |x: u64| k = (k ^ x).wrapping_mul(0x100000001b3);
    mix(idx as u64);
    mix(((w as u64) << 32) | h as u64);
    mix(anim as u64);
    mix(frames as u64);
    mix(opacity.to_bits() as u64);
    for v in [cam.eye.x, cam.eye.y, cam.eye.z, cam.target.x, cam.target.y, cam.target.z, cam.fov_y] {
        mix(v.to_bits() as u64);
    }
    k
}

/// Shadow limit of the editing viewport for the "Viewport shadows" setting (`None`: like the render).
pub fn viewport_shadow_cap(v: psxcore::settings::ViewportShadows) -> Option<psxcore::lighting::ShadowCap> {
    use psxcore::lighting::{ShadowCap, ShadowMode, ShadowQuality};
    use psxcore::settings::ViewportShadows as V;
    match v {
        V::Render => None,
        V::Medium => Some(ShadowCap { mode: ShadowMode::Projected, quality: ShadowQuality::Medium }),
        V::Blob => Some(ShadowCap { mode: ShadowMode::Blob, quality: ShadowQuality::Ultra }),
        V::Off => Some(ShadowCap { mode: ShadowMode::Off, quality: ShadowQuality::Low }),
    }
}

/// Composition guides over the part of the view that the final picture covers (`aspect` = width / height).
fn draw_guides(p: &egui::Painter, img: Rect, aspect: f32, g: psxcore::settings::Guides) {
    use psxcore::settings::Guides;
    if g == Guides::Off {
        return;
    }
    let (w, h) = if img.width() / img.height() > aspect { (img.height() * aspect, img.height()) } else { (img.width(), img.width() / aspect) };
    let r = Rect::from_center_size(img.center(), vec2(w, h));
    let line = crate::compat::stroke(1.0, alpha(Color32::WHITE, 120));
    let soft = crate::compat::stroke(1.0, alpha(Color32::BLACK, 90));
    let seg = |a: Pos2, b: Pos2| {
        p.line_segment([a + vec2(1.0, 1.0), b + vec2(1.0, 1.0)], soft);
        p.line_segment([a, b], line);
    };
    let at = |fx: f32, fy: f32| pos2(r.min.x + fx * r.width(), r.min.y + fy * r.height());
    crate::compat::stroke_rect(p, r, 0.0, crate::compat::stroke(1.0, alpha(accent(), 160)));
    match g {
        Guides::Off => {}
        Guides::Thirds | Guides::Golden => {
            let t = if g == Guides::Thirds { 1.0 / 3.0 } else { 0.381_966 };
            for f in [t, 1.0 - t] {
                seg(at(f, 0.0), at(f, 1.0));
                seg(at(0.0, f), at(1.0, f));
            }
        }
        Guides::Centre => {
            let c = r.center();
            let k = r.height().min(r.width()) * 0.06;
            seg(c - vec2(k, 0.0), c + vec2(k, 0.0));
            seg(c - vec2(0.0, k), c + vec2(0.0, k));
            seg(at(0.5, 0.0), at(0.5, 0.04));
            seg(at(0.5, 0.96), at(0.5, 1.0));
            seg(at(0.0, 0.5), at(0.03, 0.5));
            seg(at(0.97, 0.5), at(1.0, 0.5));
        }
        Guides::SafeArea => {
            for (f, a) in [(0.9, 120u8), (0.8, 80)] {
                let s = Rect::from_center_size(r.center(), r.size() * f);
                crate::compat::stroke_rect(p, s, 0.0, crate::compat::stroke(1.0, alpha(Color32::WHITE, a)));
            }
        }
    }
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
        self.rend.threads = if self.settings.render_threads == 0 { psxcore::raster::default_threads() } else { self.settings.render_threads as usize };
        // the editing view may draw cheaper shadows than the final render (Settings → Viewport)
        self.rend.shadow_cap = viewport_shadow_cap(self.settings.viewport_shadows);
        // lazy redraw: skip the software render when neither the input nor the view changed
        // (hovering panels, tooltips and UI animations then cost almost nothing)
        let key = view_key(&self.ed, w, h, self.rend.shadow_cap);
        if scene_input(&ctx) {
            self.vp.settle = 2;
        }
        let redraw = !self.settings.lazy_redraw || key != self.vp.drawn_key || self.vp.settle > 0 || self.surf[SURF_VIEW].id().is_none();
        if redraw {
            let t0 = std::time::Instant::now();
            self.ed.render_viewport_sized(&mut self.rend, w, h);
            if let Some(gl) = self.gl.clone() {
                self.surf[SURF_VIEW].upload(&gl, w, h, self.rend.frame.rgba());
            }
            self.perf.note_draw(t0.elapsed().as_secs_f32() * 1000.0);
            self.vp.drawn_key = key;
            self.vp.gen = self.vp.gen.wrapping_add(1);
            if self.vp.settle > 0 {
                self.vp.settle -= 1;
                ctx.request_repaint();
            }
        }

        // ---- onion-skin ghosts (redrawn only when their frame, the camera or the size changes)
        let ghosts = self.ed.mode == Mode::Animate && self.ed.onion && !self.ed.playing;
        if ghosts {
            let (prev, next) = self.ed.onion_indices();
            let cam = self.ed.active_camera();
            let (anim, frames) = (self.ed.current_anim, self.ed.animation().map_or(0, |a| a.frames.len()));
            for (slot, idx, tint) in [(0usize, prev, ONION_PREV), (1, next, ONION_NEXT)] {
                let key = idx.map_or(0, |i| ghost_key(i, &cam, w, h, anim, frames, self.settings.onion_opacity));
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
                            ghost_pixels(&r.frame, tint, self.settings.onion_opacity, &mut cache.bytes);
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
        painter.rect_filled(rect, compat::cr(0.0), VIEWPORT());
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
        // hover outline and Edit-mode face tint
        if self.update_overlay(&ctx, img, sc, w, h) {
            if let Some(id) = self.surf[SURF_OVERLAY].id() {
                painter.image(id, img, full, Color32::WHITE);
            }
        }

        self.vp_width = img.width();
        self.edui.view = (w, h);
        self.vp_input(&ctx, &resp, img, sc, w, h);
        self.vp_overlays(&painter, img, sc, w, h);
        self.edit_overlays(&painter, img, sc);
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
        p.rect_filled(bg, compat::cr(2.0), alpha(BG0(), 215));
        for (i, l) in lines.iter().enumerate() {
            compat::text(p, pos + vec2(0.0, i as f32 * 14.0), Align2::RIGHT_TOP, l, 12.0, TEXT());
        }
    }

    fn vp_input(&mut self, ctx: &egui::Context, resp: &egui::Response, img: Rect, sc: f32, w: u32, h: u32) {
        let (pos, pressed, down, mods) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
        let hover = resp.hovered();
        let to_px = |p: Pos2| (((p.x - img.min.x) / sc).floor() as i32, ((p.y - img.min.y) / sc).floor() as i32);
        let render_mode = self.ed.mode == Mode::Render;
        if self.vp.block_until_release {
            if !ctx.input(|i| i.pointer.any_down()) {
                self.vp.block_until_release = false;
            }
            return;
        }
        if self.radial.is_some() {
            return;
        }
        // a running Edit-mode tool uses the right button (cancel) and the wheel (segments)
        let modal = self.ed.edit_drag_active().is_some();

        // navigation: middle/right button or Alt+left to orbit; Shift pans; the wheel zooms
        let nav = resp.dragged_by(PointerButton::Middle) || (!modal && resp.dragged_by(PointerButton::Secondary)) || (resp.dragged_by(PointerButton::Primary) && mods.alt);
        if nav {
            let d = resp.drag_delta();
            if mods.shift {
                self.ed.view.pan(d.x, d.y, img.height());
            } else {
                let k = self.settings.orbit_speed;
                self.ed.view.orbit(d.x * k, d.y * k * if self.settings.invert_orbit_y { -1.0 } else { 1.0 });
            }
            self.ed.through_camera = false;
        }
        let wheel_taken = (modal && (self.ed.edit.as_ref().is_some_and(|s| s.proportional) || self.ed.edit_drag_active().is_some_and(|d| d.kind == psxcore::editor::MeshDragKind::Bevel)))
            || (self.ed.edit_active() && self.edui.tool == crate::editui::EditTool::LoopCut && (mods.ctrl || mods.command));
        if hover && !wheel_taken {
            let s = compat::scroll_y(ctx);
            if s != 0.0 {
                self.ed.view.zoom(s * 0.05 * self.settings.zoom_speed);
                self.ed.through_camera = false;
            }
        }
        if !render_mode && self.edit_input(ctx, resp, img, sc, w, h) {
            return;
        }
        if render_mode || mods.alt {
            return;
        }
        if self.vp.drag.is_none() && !self.ed.edit_active() && self.gizmo_input(ctx, img, sc, w, h, hover) {
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
                ModelTool::Select => {
                    // empty background: a reference picture may be under the mouse
                    if self.ed.pick_info(self.rend.frame.hit_at(x, y)).is_none() {
                        if let Some(i) = self.ed.ref_pick(x as f32 + 0.5, y as f32 + 0.5, w, h) {
                            self.ed.selected_ref = Some(i);
                            self.ed.begin_group();
                            self.vp.drag = Some(Drag::Ref(i));
                            return;
                        }
                    }
                    self.ed.click_select(&self.rend.frame, x, y, shift)
                }
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
        self.vp.stab = (px.0 as f32, px.1 as f32);
        self.vp.stab_last = px;
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
                let s = self.settings.stabilizer;
                if s <= 0.0 || !self.ed.brush.tool.is_continuous() {
                    self.ed.paint_drag(&self.rend.frame, last, cur);
                } else {
                    // Krita-style stabilizer: the brush trails behind the mouse on a soft string
                    let k = 1.0 - s * 0.92;
                    self.vp.stab.0 += (cur.0 as f32 - self.vp.stab.0) * k;
                    self.vp.stab.1 += (cur.1 as f32 - self.vp.stab.1) * k;
                    let np = (self.vp.stab.0.round() as i32, self.vp.stab.1.round() as i32);
                    if np != self.vp.stab_last {
                        self.ed.paint_drag(&self.rend.frame, self.vp.stab_last, np);
                        self.vp.stab_last = np;
                    }
                }
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
            Drag::Bone(b) if self.ed.ik_chain > 0 => {
                // inverse kinematics: the joint follows the mouse, the parent bones bend
                self.ed.ik_drag(b, cur.0 as f32 + 0.5, cur.1 as f32 + 0.5, _w, h);
            }
            Drag::Ref(i) => {
                let d = right * (dx * k) + up * (-dy * k);
                self.ed.ref_edit(i, |r| r.transform.pos = r.transform.pos + d);
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
            Some(Drag::Paint) => {
                // with the stabilizer the stroke still ends where the mouse was let go
                if self.settings.stabilizer > 0.0 && self.ed.brush.tool.is_continuous() && self.vp.stab_last != self.vp.last_px {
                    self.ed.paint_drag(&self.rend.frame, self.vp.stab_last, self.vp.last_px);
                }
                self.ed.paint_release()
            }
            Some(Drag::Sculpt) => self.ed.sculpt_end(),
            Some(Drag::Hand(c)) => {
                let p = self.vp.hand_last;
                self.ed.canvas_release(c, p);
            }
            Some(Drag::Bone(_)) | Some(Drag::MoveObject(_)) | Some(Drag::Ref(_)) => self.ed.end_group(),
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
                p.add(Shape::convex_polygon(pts, alpha(accent(), 70), crate::compat::stroke(2.0, accent())));
            }
        }
        // skeleton
        let show_bones = self.ed.mode == Mode::Pose || (self.ed.mode == Mode::Animate && self.anim_tool == AnimTool::Pose);
        if show_bones {
            for b in self.ed.screen_bones(w, h) {
                let sel = self.ed.selected_bone == Some(b.bone);
                let col = if sel { accent() } else { accent() };
                if b.head != b.tail {
                    p.line_segment([at(b.head.0, b.head.1), at(b.tail.0, b.tail.1)], crate::compat::stroke(if sel { 4.0 } else { 2.5 }, col));
                }
                p.circle_filled(at(b.tail.0, b.tail.1), if sel { 7.0 } else { 5.0 }, col);
            }
        }
        // corner caption
        let tris = self.ed.selected_model().map_or(0, |m| m.triangle_count());
        let mut info = format!("{}   {} tris", crate::mode_label(self.ed.mode), tris);
        if let Some(s) = self.edit_summary() {
            info = s;
        }
        if matches!(self.ed.mode, Mode::Pose | Mode::Animate) && self.ed.ik_chain > 0 {
            info.push_str(&format!("   IK ({} bones)", self.ed.ik_chain));
        }
        compat::text(p, img.min + vec2(10.0, 10.0), Align2::LEFT_TOP, &info, 13.0, alpha(Color32::WHITE, 200));
        if self.ed.playing {
            compat::text(p, img.max - vec2(10.0, 10.0), Align2::RIGHT_BOTTOM, "▶", 16.0, accent());
        }
        if let Some(f) = self.ed.frame().filter(|_| matches!(self.ed.mode, Mode::Pose | Mode::Animate | Mode::Render)) {
            if !self.ed.through_camera {
                compat::text(p, img.min + vec2(10.0, 28.0), Align2::LEFT_TOP, "free view: the frame camera is not changed", 12.0, alpha(WARN(), 220));
            }
            if !f.note.is_empty() {
                compat::text(p, pos2(img.center().x, img.max.y - 12.0), Align2::CENTER_BOTTOM, &f.note, 14.0, alpha(Color32::WHITE, 220));
            }
        }
        if self.ed.mode == Mode::Render {
            let (rw, rh) = self.ed.render_size();
            draw_guides(p, img, rw as f32 / rh.max(1) as f32, self.settings.guides);
        }
    }

    /// Welcome window shown for an empty project.
    fn welcome(&mut self, ctx: &egui::Context) {
        egui::Window::new("Welcome to Jank!").collapsible(false).resizable(false).default_pos(compat::centered_pos(ctx, vec2(380.0, 160.0))).show(ctx, |ui| {
            ui.set_min_width(360.0);
            widgets::dim(ui, "Import a model or start from a primitive, then paint right on it. Or look around the starter scene first.");
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Import model").icon(crate::icons::Icon::Open).accent(accent()).show(ui).clicked() {
                    self.dialog = Some(dialogs::browser(dialogs::Purpose::ImportModel));
                }
                if Btn::new("Cube").icon(crate::icons::Icon::Cube).accent(accent()).show(ui).clicked() {
                    self.ed.add_primitive(psxcore::editor::Primitive::Cube);
                }
                if Btn::new("Open project").icon(crate::icons::Icon::Save).accent(accent()).show(ui).clicked() {
                    self.dialog = Some(dialogs::browser(dialogs::Purpose::OpenProject));
                }
                if Btn::new("Bunny scene").icon(crate::icons::Icon::Sparkle).accent(accent()).tip("Two plush bunnies on a rug: textures from photos, bones, lighting, an idle animation").show(ui).clicked() {
                    self.load_starter();
                }
            });
        });
    }
}
