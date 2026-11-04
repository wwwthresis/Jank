//! 2D texture editor: pixel-level zoom, grid, UV layout overlay and painting.
use crate::icons::Icon;
use crate::theme::*;
use crate::{compat, dialogs, widgets, widgets::Btn, App};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Context, PointerButton, Pos2, Rect, Sense, Shape, SidePanel, Vec2};
use psxcore::editor::{Canvas, Mode, PaintLayer, TexOp};
use psxcore::paint::Tool;

pub struct TexView {
    pub zoom: f32,
    pub pan: Vec2,
    pub show_uv: bool,
    pub tex: Option<egui::TextureHandle>,
    pub drag: Option<Canvas>,
    pub last_canvas: Option<Canvas>,
    pub last_size: (u32, u32),
    /// Stabilizer: the smoothed brush position of the running stroke.
    pub stab: (f32, f32),
}
impl Default for TexView {
    fn default() -> Self {
        Self { zoom: 4.0, pan: Vec2::ZERO, show_uv: true, tex: None, drag: None, last_canvas: None, last_size: (0, 0), stab: (0.0, 0.0) }
    }
}

impl App {
    pub fn texture_panel(&mut self, ctx: &Context) {
        let mut panel = SidePanel::left("tex2d").resizable(true).default_width(self.settings.layout.texture_width).width_range(300.0..=900.0).frame(compat::frame(BG1(), 10.0));
        if self.layout_reset {
            panel = panel.exact_width(self.settings.layout.texture_width);
        }
        let shown = panel.show(ctx, |ui| {
            let Some(canvas) = self.current_canvas() else {
                widgets::dim(ui, "Select an object to edit its texture, or make a picture layer to draw a plain image or a backdrop.");
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if Btn::new("Layer behind").icon(Icon::Plus).accent(accent()).tip("A picture behind the 3D scene").show(ui).clicked() {
                        self.ed.add_scene_layer(true);
                        self.paint.scene_target = true;
                    }
                    if Btn::new("Layer in front").icon(Icon::Plus).accent(accent()).tip("A picture in front of the 3D scene").show(ui).clicked() {
                        self.ed.add_scene_layer(false);
                        self.paint.scene_target = true;
                    }
                });
                return;
            };
            let accent = if self.ed.mode == Mode::Animate { WARN() } else { accent() };
            if matches!(canvas, Canvas::Base { .. } | Canvas::Patch { .. } | Canvas::Emotion { .. }) {
                self.texture_selector(ui, accent);
            }
            let Some((tw, th)) = self.ed.canvas_texture(canvas).map(|t| (t.w, t.h)) else { return };
            ui.horizontal(|ui| {
                widgets::dim(ui, &format!("{tw}×{th} px"));
                widgets::toggle(ui, &mut self.tex.show_uv, "UV", accent);
                if widgets::toggle(ui, &mut self.settings.pixel_grid, "Grid", accent).clicked() {
                    self.touch_settings();
                }
                if Btn::new("Fit").tip("Zoom so the whole texture fits the window").show(ui).clicked() {
                    self.tex.last_canvas = None;
                }
            });

            let avail = ui.available_size();
            let (rect, resp) = ui.allocate_exact_size(vec2(avail.x, (avail.y - 150.0).max(160.0)), Sense::click_and_drag());
            if let Some(t) = self.ed.canvas_texture(canvas) {
                // upload only when the picture changed (a cheap hash instead of copying every frame)
                let hsh = px_hash(&t.px, t.w, t.h);
                if hsh != self.paint.tex_hash || self.tex.tex.is_none() {
                    compat::upload(&mut self.tex.tex, ctx, "texture2d", t.w as usize, t.h as usize, &t.to_rgba_bytes());
                    self.paint.tex_hash = hsh;
                }
            }
            self.update_reference(ctx, canvas);
            // fit when the canvas or its size changes
            if self.tex.last_canvas != Some(canvas) || self.tex.last_size != (tw, th) {
                let z = (rect.width() / tw as f32).min(rect.height() / th as f32) * 0.92;
                self.tex.zoom = if z >= 1.0 { z.floor().clamp(1.0, 64.0) } else { z.max(0.1) };
                self.tex.pan = Vec2::ZERO;
                self.tex.last_canvas = Some(canvas);
                self.tex.last_size = (tw, th);
            }
            let painter = ui.painter().with_clip_rect(rect);
            self.texture_canvas(ctx, &painter, rect, &resp, canvas, (tw, th));
            self.texture_ops(ui, canvas);
        });
        self.track_width(crate::Panel::Texture, shown.response.rect.width());
    }

    fn texture_selector(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let names: Vec<String> = self.ed.selected_model().map(|m| m.textures.iter().map(|t| t.name.clone()).collect()).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            for (i, n) in names.iter().enumerate() {
                if Btn::new(n).accent(accent).on(self.ed.active_texture == i).size(vec2(20.0 + n.chars().count().min(14) as f32 * 8.0, 28.0)).show(ui).clicked() {
                    self.ed.active_texture = i;
                }
            }
            if Btn::new("").icon(Icon::Plus).size(vec2(30.0, 28.0)).tip("New 64×64 texture").show(ui).clicked() {
                self.ed.new_texture(64, 64);
            }
        });
        if self.ed.mode == Mode::Animate {
            ui.horizontal_wrapped(|ui| {
                for (l, name, tip) in [(PaintLayer::FramePatch, "This frame only", "One-off repaint of this frame (blink, blush)"), (PaintLayer::Emotion, "Emotion", "Paint into the image of the selected emotion"), (PaintLayer::Base, "Whole model", "Edit the base texture in every frame")] {
                    if Btn::new(name).tip(tip).accent(WARN()).on(self.ed.paint_layer == l).size(vec2(24.0 + name.chars().count() as f32 * 7.5, 28.0)).show(ui).clicked() {
                        self.ed.paint_layer = l;
                    }
                }
            });
        }
    }

    fn texture_canvas(&mut self, ctx: &Context, painter: &egui::Painter, rect: Rect, resp: &egui::Response, canvas: Canvas, size: (u32, u32)) {
        let (tw, th) = (size.0 as f32, size.1 as f32);
        let (pos, pressed, down, mods) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
        let hover = resp.hovered();
        // wheel zoom around the cursor
        if hover {
            let s = compat::scroll_y(ctx);
            if s != 0.0 {
                let old = self.tex.zoom;
                let new = (old * (s * 0.01).exp()).clamp(0.25, 96.0);
                if let Some(p) = pos {
                    let center = rect.center() + self.tex.pan;
                    let off = p - center;
                    self.tex.pan = self.tex.pan - off * (new / old - 1.0);
                }
                self.tex.zoom = new;
            }
        }
        if resp.dragged_by(PointerButton::Middle) || (resp.dragged_by(PointerButton::Primary) && mods.alt) {
            self.tex.pan += resp.drag_delta();
        }
        let z = self.tex.zoom;
        let img = Rect::from_center_size(rect.center() + self.tex.pan, vec2(tw * z, th * z));
        painter.rect_filled(rect, compat::cr(3.0), BG0());
        // checkerboard under transparent pixels
        let vis = img.intersect(rect);
        let cell = 12.0;
        let (nx, ny) = (((vis.width() / cell).ceil() as i32).min(120), ((vis.height() / cell).ceil() as i32).min(120));
        for iy in 0..ny {
            for ix in 0..nx {
                if (ix + iy) % 2 == 0 {
                    let r = Rect::from_min_size(vis.min + vec2(ix as f32 * cell, iy as f32 * cell), vec2(cell, cell)).intersect(vis);
                    painter.rect_filled(r, compat::cr(0.0), Color32::from_gray(38));
                } else {
                    let r = Rect::from_min_size(vis.min + vec2(ix as f32 * cell, iy as f32 * cell), vec2(cell, cell)).intersect(vis);
                    painter.rect_filled(r, compat::cr(0.0), Color32::from_gray(52));
                }
            }
        }
        if let (true, Some(r)) = (self.paint.ref_on && matches!(canvas, Canvas::Scene { .. } | Canvas::Hand { .. }), &self.paint.ref_tex) {
            painter.image(r.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_white_alpha(150));
        }
        if let Some(t) = &self.tex.tex {
            painter.image(t.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        compat::stroke_rect(painter, img.expand(1.0), 0.0, crate::compat::stroke(1.0, LINE()));
        if self.settings.pixel_grid && z >= 8.0 {
            let st = crate::compat::stroke(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 26));
            for i in 1..size.0 {
                let x = img.min.x + i as f32 * z;
                if x > rect.min.x && x < rect.max.x {
                    painter.line_segment([pos2(x, img.min.y.max(rect.min.y)), pos2(x, img.max.y.min(rect.max.y))], st);
                }
            }
            for j in 1..size.1 {
                let y = img.min.y + j as f32 * z;
                if y > rect.min.y && y < rect.max.y {
                    painter.line_segment([pos2(img.min.x.max(rect.min.x), y), pos2(img.max.x.min(rect.max.x), y)], st);
                }
            }
        }
        if self.tex.show_uv {
            self.draw_uv(painter, img, canvas);
        }

        // painting and selecting
        let to_tex = |p: Pos2| ((p.x - img.min.x) / z, (p.y - img.min.y) / z);
        let sel_tool = self.paint.sel_tool;
        if let Some(tool) = sel_tool {
            let busy = self.paint.sel_start.is_some();
            let (press, held) = (pressed && hover && !mods.alt, down);
            if press || busy {
                self.selection_pointer(canvas, tool, pos.map(to_tex), press, held, mods.shift, mods.command);
            }
            self.draw_selection(painter, img, canvas, z);
            if hover {
                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                self.selection_keys(ctx, canvas);
            }
            return;
        }
        self.draw_selection(painter, img, canvas, z);
        if hover {
            self.selection_keys(ctx, canvas);
        }
        if pressed && hover && !mods.alt && self.tex.drag.is_none() {
            if let Some(p) = pos {
                if !matches!(self.ed.brush.tool, Tool::Picker | Tool::Eraser) {
                    self.remember_color();
                }
                self.ed.canvas_press(canvas, to_tex(p));
                self.tex.stab = to_tex(p);
                self.tex.drag = Some(canvas);
            }
        }
        if let (Some(c), true) = (self.tex.drag, down) {
            // every pointer position of this frame, so fast strokes leave no gaps
            let pts = ctx.input(|i| {
                let mut v: Vec<Pos2> = i.events.iter().filter_map(|e| if let egui::Event::PointerMoved(p) = e { Some(*p) } else { None }).collect();
                if let Some(p) = i.pointer.latest_pos() {
                    if v.last() != Some(&p) {
                        v.push(p);
                    }
                }
                v
            });
            let s = self.settings.stabilizer;
            let smooth = s > 0.0 && self.ed.brush.tool.is_continuous();
            for p in pts {
                let t = to_tex(p);
                if smooth {
                    // the brush trails behind the mouse (Krita's stabilizer)
                    let k = 1.0 - s * 0.92;
                    self.tex.stab = (self.tex.stab.0 + (t.0 - self.tex.stab.0) * k, self.tex.stab.1 + (t.1 - self.tex.stab.1) * k);
                    self.ed.canvas_drag(c, self.tex.stab);
                } else {
                    self.ed.canvas_drag(c, t);
                }
            }
        }
        if self.tex.drag.is_some() && !down {
            let c = self.tex.drag.take().unwrap();
            if let Some(p) = pos {
                self.ed.canvas_release(c, to_tex(p));
            } else {
                self.ed.end_group();
            }
        }
        // brush-shaped cursor
        if hover {
            if let Some(p) = pos {
                let t = to_tex(p);
                let s = self.ed.brush.size.max(1.0);
                let (cx, cy) = (t.0.floor(), t.1.floor());
                let half = ((s - 1.0) / 2.0).floor();
                let r = Rect::from_min_size(pos2(img.min.x + (cx - half) * z, img.min.y + (cy - half) * z), vec2(s * z, s * z));
                if matches!(self.ed.brush.tool, Tool::Brush | Tool::Eraser | Tool::Line | Tool::Rect | Tool::RectFilled) {
                    compat::stroke_rect(painter, r, 0.0, crate::compat::stroke(1.0, Color32::WHITE));
                }
                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
    }

    /// The 3D scene without its picture layers, drawn at the render size and shown under the picture being edited.
    fn update_reference(&mut self, ctx: &Context, canvas: Canvas) {
        if !(self.paint.ref_on && matches!(canvas, Canvas::Scene { .. } | Canvas::Hand { .. })) {
            return;
        }
        let (rw, rh) = self.ed.render_size();
        let key = ((rw as u64) << 40) ^ ((rh as u64) << 20) ^ (self.ed.current_frame as u64) << 8 ^ self.ed.current_anim as u64 ^ (self.ed.project.scene.objects.len() as u64) << 50 ^ (self.ed.mode as u64) << 60;
        let events = ctx.input(|i| !i.events.is_empty());
        if key != self.paint.ref_key || events || self.paint.ref_tex.is_none() {
            let r = self.paint.ref_rend.get_or_insert_with(|| psxcore::raster::Renderer::new(1, 1));
            r.threads = self.rend.threads;
            self.ed.render_reference(r);
            compat::upload(&mut self.paint.ref_tex, ctx, "reference3d", r.frame.w as usize, r.frame.h as usize, r.frame.rgba());
            self.paint.ref_key = key;
        }
    }

    /// Outline of the pixel selection (cached) and the rubber band of the rectangle tool.
    fn draw_selection(&mut self, painter: &egui::Painter, img: Rect, canvas: Canvas, z: f32) {
        if let Some((a, b)) = self.paint.sel_start.map(|s| (s, self.paint.sel_cur)) {
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0) + 1);
            let (y0, y1) = (a.1.min(b.1), a.1.max(b.1) + 1);
            let r = Rect::from_min_max(pos2(img.min.x + x0 as f32 * z, img.min.y + y0 as f32 * z), pos2(img.min.x + x1 as f32 * z, img.min.y + y1 as f32 * z));
            painter.rect_filled(r, compat::cr(0.0), alpha(accent(), 40));
            compat::stroke_rect(painter, r, 0.0, crate::compat::stroke(1.0, Color32::WHITE));
        }
        let Some(sel) = self.ed.selection_for_public(canvas) else { return };
        let key = (sel.count() as u64 ^ ((sel.w as u64) << 32), sel.count(), sel.bounds().unwrap_or((0, 0, 0, 0)));
        if key != self.paint.sel_key {
            self.paint.sel_segs = sel.outline();
            self.paint.sel_key = key;
        }
        let t = painter.ctx().input(|i| i.time) as f32;
        let n = self.paint.sel_segs.len();
        if n > 8000 {
            if let Some((x, y, w, h)) = self.ed.selection_for_public(canvas).and_then(|s| s.bounds()) {
                let r = Rect::from_min_max(pos2(img.min.x + x as f32 * z, img.min.y + y as f32 * z), pos2(img.min.x + (x + w) as f32 * z, img.min.y + (y + h) as f32 * z));
                compat::stroke_rect(painter, r, 0.0, crate::compat::stroke(1.5, Color32::WHITE));
            }
            return;
        }
        for (i, &(x0, y0, x1, y1)) in self.paint.sel_segs.iter().enumerate() {
            // marching ants: the colour alternates along the outline and moves with time
            let on = ((i as f32 * 0.5 + t * 6.0) as i32) % 2 == 0;
            let c = if on { Color32::WHITE } else { Color32::BLACK };
            painter.line_segment([pos2(img.min.x + x0 as f32 * z, img.min.y + y0 as f32 * z), pos2(img.min.x + x1 as f32 * z, img.min.y + y1 as f32 * z)], crate::compat::stroke(1.5, c));
        }
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(120));
    }

    /// Keyboard shortcuts of the selection while the pointer is over the canvas.
    fn selection_keys(&mut self, ctx: &Context, canvas: Canvas) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let (a, d, i) = ctx.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::A), i.modifiers.command && i.key_pressed(egui::Key::D), i.modifiers.command && i.key_pressed(egui::Key::I)));
        if a {
            self.ed.select_all(canvas);
        }
        if d {
            self.ed.select_none();
        }
        if i {
            self.ed.select_invert(canvas);
        }
    }

    fn draw_uv(&self, painter: &egui::Painter, img: Rect, canvas: Canvas) {
        let Some(model) = self.ed.selected_model() else { return };
        let tex = match canvas {
            Canvas::Base { tex, .. } | Canvas::Patch { tex, .. } => tex,
            Canvas::Emotion { .. } => 0,
            Canvas::Hand { .. } | Canvas::Scene { .. } => return,
        };
        let z = img.width();
        let st = crate::compat::stroke(1.0, alpha(accent(), 110));
        let mut budget = 6000usize;
        for (mi, mesh) in model.meshes.iter().enumerate() {
            let uses = model.materials.get(mesh.material).and_then(|m| m.texture) == Some(self.tex_index(canvas, tex));
            if !uses {
                continue;
            }
            for (ti, t) in mesh.tris.iter().enumerate() {
                if budget == 0 {
                    return;
                }
                budget -= 1;
                let pts: Vec<Pos2> = t.iter().map(|&i| mesh.verts[i as usize].uv).map(|uv| pos2(img.min.x + uv.x * z, img.min.y + uv.y * img.height())).collect();
                if self.ed.selected_faces.contains(&(mi, ti)) {
                    painter.add(Shape::convex_polygon(pts, alpha(accent(), 90), crate::compat::stroke(1.5, accent())));
                } else {
                    painter.add(Shape::closed_line(pts, st));
                }
            }
        }
    }
    fn tex_index(&self, canvas: Canvas, tex: usize) -> usize {
        match canvas {
            Canvas::Emotion { asset, idx } => self.ed.project.assets.get(asset).and_then(|a| a.emotions.get(idx)).map_or(tex, |e| e.texture),
            _ => tex,
        }
    }

    fn texture_ops(&mut self, ui: &mut egui::Ui, c: Canvas) {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            fn b(l: &str) -> Btn<'_> {
                Btn::new(l).size(vec2(28.0 + l.chars().count() as f32 * 7.5, 30.0))
            }
            if b("↔").tip("Flip horizontally").show(ui).clicked() {
                self.ed.texture_op(c, TexOp::FlipH);
            }
            if b("↕").tip("Flip vertically").show(ui).clicked() {
                self.ed.texture_op(c, TexOp::FlipV);
            }
            for n in [16u32, 32, 64, 128, 256] {
                if b(&n.to_string()).tip("Resize the texture (no smoothing)").show(ui).clicked() {
                    self.ed.texture_op(c, TexOp::Resize(n, n));
                }
            }
            if b("15-bit").tip("Reduces colours to 15-bit (5 bits per channel), like the PS1").show(ui).clicked() {
                self.ed.texture_op(c, TexOp::Quantize15);
            }
            if b("Dither").tip("Reduces to 5 bits per channel with ordered dithering").show(ui).clicked() {
                self.ed.texture_op(c, TexOp::Dither(5));
            }
            if b("16 colours").tip("Reduces the texture to a 16-colour palette").show(ui).clicked() {
                self.ed.texture_op(c, TexOp::ReducePalette(16));
            }
            if b("Fill all").tip("Fills the whole canvas with the current colour").show(ui).clicked() {
                let col = self.ed.brush.color;
                self.ed.texture_op(c, TexOp::Fill(col));
            }
            if Btn::new("Load").icon(Icon::Image).accent(accent()).tip("Replace the texture with an image (PNG, JPEG, BMP, TGA, PPM)").show(ui).clicked() {
                self.dialog = Some(dialogs::browser(dialogs::Purpose::ImportTexture(c)));
            }
            if Btn::new("Save PNG").icon(Icon::Export).accent(accent()).tip("Exports the texture as a PNG file").show(ui).clicked() {
                self.dialog = Some(dialogs::browser(dialogs::Purpose::SaveTexture(c)));
            }
        });
    }
}

#[allow(dead_code)]
fn _unused(_: Align2) {}

/// Fast hash of a picture, used to notice changes without comparing or copying it.
fn px_hash(px: &[[u8; 4]], w: u32, h: u32) -> u64 {
    let mut a: u64 = 0xcbf29ce484222325 ^ ((w as u64) << 32 | h as u64);
    for c in px {
        a = (a ^ u32::from_le_bytes(*c) as u64).wrapping_mul(0x100000001b3);
    }
    a
}
