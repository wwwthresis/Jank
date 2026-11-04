//! The render preview window: a small, movable window inside the viewport that shows the REAL final
//! picture - the render style at the chosen resolution, seen through the final camera - not a scaled
//! copy of the viewport. It has its own renderer and GPU texture, so resizing the window or changing
//! the render resolution never changes the main viewport's resolution or speed.
use crate::theme::*;
use crate::{compat, App, SURF_PREVIEW};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Context, Id, Pos2, Rect, Sense};
use psxcore::raster::Renderer;
use psxcore::settings::{Corner, PREVIEW_WIDTH_MAX, PREVIEW_WIDTH_MIN};
use std::time::{Duration, Instant};

/// At most one redraw per this many milliseconds while the scene changes.
const MIN_INTERVAL_MS: u64 = 80;
const HEADER: f32 = 22.0;
const MARGIN: f32 = 10.0;

pub struct PreviewState {
    rend: Renderer,
    last: Option<Instant>,
    stale: bool,
    size: (u32, u32),
    /// Position of the window while the user drags it by the header.
    drag_pos: Option<Pos2>,
    linear: Option<bool>,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self { rend: Renderer::new(1, 1), last: None, stale: true, size: (0, 0), drag_pos: None, linear: None }
    }
}

fn corner_pos(c: Corner, vp: Rect, total: egui::Vec2) -> Pos2 {
    let (left, top) = match c {
        Corner::BottomLeft => (true, false),
        Corner::BottomRight => (false, false),
        Corner::TopLeft => (true, true),
        Corner::TopRight => (false, true),
    };
    pos2(if left { vp.min.x + MARGIN } else { vp.max.x - MARGIN - total.x }, if top { vp.min.y + MARGIN } else { vp.max.y - MARGIN - total.y })
}

fn nearest_corner(center: Pos2, vp: Rect) -> Corner {
    match (center.x < vp.center().x, center.y < vp.center().y) {
        (true, false) => Corner::BottomLeft,
        (false, false) => Corner::BottomRight,
        (true, true) => Corner::TopLeft,
        (false, true) => Corner::TopRight,
    }
}

impl App {
    /// Draws (and, when needed, refreshes) the preview window inside the viewport rectangle `vp`.
    pub fn preview_ui(&mut self, ctx: &Context, vp: Rect) {
        let before = self.settings.preview;
        self.preview_inner(ctx, vp);
        if self.settings.preview != before {
            self.touch_settings();
        }
    }

    fn preview_inner(&mut self, ctx: &Context, vp: Rect) {
        if !self.settings.preview.visible {
            return;
        }
        let (rw, rh) = self.ed.render_size();
        let aspect = rw as f32 / rh as f32;

        // ---- refresh the picture when something may have changed (input, playback, resolution), at most ~12 times a second
        let busy = (if self.settings.lazy_redraw { crate::viewport::scene_input(ctx) } else { ctx.input(|i| !i.events.is_empty() || i.pointer.any_down()) }) || self.ed.playing;
        let st = &mut self.pv;
        if busy || st.size != (rw, rh) {
            st.stale = true;
        }
        if st.stale {
            let wait = st.last.map_or(Duration::ZERO, |t| Duration::from_millis(MIN_INTERVAL_MS).saturating_sub(t.elapsed()));
            if wait.is_zero() {
                let t0 = Instant::now();
                st.rend.threads = self.rend.threads;
                self.ed.render_preview(&mut st.rend);
                if let Some(gl) = self.gl.clone() {
                    self.surf[SURF_PREVIEW].upload(&gl, st.rend.frame.w, st.rend.frame.h, st.rend.frame.rgba());
                }
                self.perf.preview_ms = t0.elapsed().as_secs_f32() * 1000.0;
                st.size = (rw, rh);
                st.last = Some(Instant::now());
                st.stale = false;
            } else {
                ctx.request_repaint_after(wait);
            }
        }

        // ---- geometry: width from the settings, height follows the render aspect ratio
        let max_w = (vp.width() * 0.7).min(vp.height() * 0.7 * aspect).max(PREVIEW_WIDTH_MIN);
        let w = self.settings.preview.width.clamp(PREVIEW_WIDTH_MIN, PREVIEW_WIDTH_MAX).min(max_w);
        let total = vec2(w, w / aspect + HEADER);
        let corner = self.settings.preview.corner;
        let pos = self.pv.drag_pos.unwrap_or_else(|| corner_pos(corner, vp, total));
        let rect = Rect::from_min_size(pos, total);
        let header = Rect::from_min_size(rect.min, vec2(w, HEADER));
        let body = Rect::from_min_max(pos2(rect.min.x, rect.min.y + HEADER), rect.max);
        // the grip sits at the corner that faces the middle of the viewport
        let (left, top) = (matches!(corner, Corner::BottomLeft | Corner::TopLeft), matches!(corner, Corner::TopLeft | Corner::TopRight));
        let grip = Rect::from_center_size(pos2(if left { body.max.x } else { body.min.x }, if top { body.max.y } else { body.min.y + 0.0 }), vec2(16.0, 16.0));
        let (sx, sy) = (if left { 1.0 } else { -1.0 }, if top { 1.0 } else { -1.0 });

        let mut visible = true;
        let mut size_factor = 1.0f32;
        egui::Area::new(Id::new("render_preview")).fixed_pos(rect.min).order(egui::Order::Middle).constrain(false).show(ctx, |ui| {
            ui.set_clip_rect(rect.expand(2.0));
            let (_, _) = ui.allocate_exact_size(total, Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, compat::cr(0.0), BG1);
            p.rect_filled(header, compat::cr(0.0), BG2);
            if let Some(id) = self.surf[SURF_PREVIEW].id() {
                p.image(id, body, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            } else {
                p.rect_filled(body, compat::cr(0.0), BG0);
            }
            compat::stroke_rect(p, rect, 0.0, compat::stroke(1.0, LINE));
            compat::text(p, header.min + vec2(8.0, HEADER * 0.5), Align2::LEFT_CENTER, &format!("Render  {rw}x{rh}"), 11.5, TEXT);

            // header buttons: smaller, larger, hide
            let mut bx = header.max.x - 4.0;
            for (label, tip, act) in [("x", "Hide the render preview (Shift+P shows it again)", 0), ("+", "Larger preview", 1), ("-", "Smaller preview", 2)] {
                let r = Rect::from_min_size(pos2(bx - 18.0, header.min.y + 2.0), vec2(18.0, HEADER - 4.0));
                bx -= 20.0;
                let resp = ui.interact(r, Id::new(("pv_btn", act)), Sense::click()).on_hover_text(tip);
                if resp.hovered() {
                    ui.painter().rect_filled(r, compat::cr(2.0), BG3);
                }
                compat::text(ui.painter(), r.center(), Align2::CENTER_CENTER, label, 13.0, if resp.hovered() { TEXT } else { DIM });
                if resp.clicked() {
                    match act {
                        0 => visible = false,
                        1 => size_factor = 1.25,
                        _ => size_factor = 0.8,
                    }
                }
            }
            // drag the header to move the window; it settles into the nearest corner on release
            let drag_rect = Rect::from_min_max(header.min, pos2(bx, header.max.y));
            let dr = ui.interact(drag_rect, Id::new("pv_drag"), Sense::drag()).on_hover_text("Drag to another corner of the viewport");
            if dr.dragged() {
                let cur = self.pv.drag_pos.unwrap_or(pos);
                let np = cur + dr.drag_delta();
                self.pv.drag_pos = Some(pos2(np.x.clamp(vp.min.x, (vp.max.x - total.x).max(vp.min.x)), np.y.clamp(vp.min.y, (vp.max.y - total.y).max(vp.min.y))));
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            if dr.drag_stopped() {
                let center = self.pv.drag_pos.map_or(rect.center(), |p| p + total * 0.5);
                self.settings.preview.corner = nearest_corner(center, vp);
                self.pv.drag_pos = None;
            }
            // resize by the grip (keeps the aspect ratio) or with the wheel over the picture
            let gr = ui.interact(grip, Id::new("pv_grip"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeNeSw).on_hover_text("Drag to resize");
            if gr.dragged() {
                let d = gr.drag_delta();
                let dw = (d.x * sx + d.y * sy * aspect) * 0.5 * 2.0 * 0.5 + (d.x * sx) * 0.5;
                self.settings.preview.width = (w + dw).clamp(PREVIEW_WIDTH_MIN, PREVIEW_WIDTH_MAX);
            }
            ui.painter().rect_filled(Rect::from_center_size(grip.center(), vec2(8.0, 8.0)), compat::cr(1.0), alpha(accent(), if gr.hovered() || gr.dragged() { 255 } else { 140 }));
            let over = ui.interact(body, Id::new("pv_body"), Sense::hover());
            if over.hovered() {
                let s = compat::scroll_y(ctx);
                if s != 0.0 {
                    size_factor = (1.0 + s * 0.002).clamp(0.8, 1.25);
                }
            }
        });
        if !visible {
            self.settings.preview.visible = false;
        }
        if size_factor != 1.0 {
            self.settings.preview.width = (w * size_factor).clamp(PREVIEW_WIDTH_MIN, PREVIEW_WIDTH_MAX);
        }
        // smooth filtering when the picture is shown smaller than it was drawn (avoids shimmering), crisp pixels otherwise
        let want_linear = w < rw as f32;
        if self.pv.linear != Some(want_linear) {
            if let Some(gl) = &self.gl {
                self.surf[SURF_PREVIEW].set_linear(gl, want_linear);
                self.pv.linear = Some(want_linear);
            }
        }
    }
}
