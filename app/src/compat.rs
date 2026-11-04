//! Every call that changes between egui versions lives here.
//! When egui/eframe is upgraded, only this file (and possibly `theme::apply`) needs to change.
use eframe::egui::{self, Align2, Color32, ColorImage, Context, CornerRadius, FontId, Frame, Margin, Painter, Pos2, Rect, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2};

pub type AppResult = eframe::Result;

/// Stroke with a typed width, so float literals at call sites do not depend on type inference.
pub fn stroke(w: f32, c: impl Into<Color32>) -> Stroke {
    Stroke::new(w, c.into())
}
/// Uniform corner radius in pixels.
pub fn cr(r: f32) -> CornerRadius {
    CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}
/// Uniform frame margin in pixels.
pub fn margin(v: f32) -> Margin {
    Margin::same(v.round().clamp(-128.0, 127.0) as i8)
}
/// Filled frame with a uniform inner margin.
pub fn frame(fill: Color32, pad: f32) -> Frame {
    Frame::new().fill(fill).inner_margin(margin(pad))
}
/// Draws an inside-aligned rectangle outline with rounded corners.
pub fn stroke_rect(p: &Painter, r: Rect, radius: f32, s: Stroke) {
    p.rect_stroke(r, cr(radius), s, StrokeKind::Inside);
}
/// Uploads or updates an RGBA texture without smoothing (pixels stay square).
pub fn upload(slot: &mut Option<TextureHandle>, ctx: &Context, name: &str, w: usize, h: usize, rgba: &[u8]) {
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return;
    }
    let img = ColorImage::from_rgba_unmultiplied([w, h], &rgba[..w * h * 4]);
    match slot {
        Some(t) => t.set(img, TextureOptions::NEAREST),
        None => *slot = Some(ctx.load_texture(name, img, TextureOptions::NEAREST)),
    }
}
/// Smoothed vertical scroll delta of this frame.
pub fn scroll_y(ctx: &Context) -> f32 {
    ctx.input(|i| i.smooth_scroll_delta.y)
}
/// Frame time in seconds, clamped so a stall does not cause a huge animation step.
pub fn dt(ctx: &Context) -> f32 {
    ctx.input(|i| i.stable_dt).clamp(0.0, 0.25)
}
/// Draws single-line proportional text and returns its rectangle.
pub fn text(p: &Painter, pos: Pos2, a: Align2, s: &str, size: f32, c: Color32) -> Rect {
    p.text(pos, a, s, FontId::proportional(size), c)
}
/// Sets the native window title.
pub fn set_title(ctx: &Context, title: &str) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_string()));
}
/// True when the user asked to close the window this frame.
pub fn close_requested(ctx: &Context) -> bool {
    ctx.input(|i| i.viewport().close_requested())
}
/// Cancels a pending close request.
pub fn cancel_close(ctx: &Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
}
/// Closes the window.
pub fn close(ctx: &Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
}
/// Sets the interface scale (1.0 = normal). Cheap when the value did not change.
pub fn set_scale(ctx: &Context, scale: f32) {
    if (ctx.zoom_factor() - scale).abs() > 0.001 {
        ctx.set_zoom_factor(scale);
    }
}
/// Name of a key in the GUI-independent spelling used by `psxcore::keymap`.
pub fn key_name(k: egui::Key) -> &'static str {
    match k {
        egui::Key::ArrowDown => "ArrowDown",
        egui::Key::ArrowLeft => "ArrowLeft",
        egui::Key::ArrowRight => "ArrowRight",
        egui::Key::ArrowUp => "ArrowUp",
        other => other.name(),
    }
}
/// Key presses of this frame as chords (keys with modifiers), in order. Repeats are included.
pub fn pressed_chords(ctx: &Context) -> Vec<psxcore::keymap::Chord> {
    ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => psxcore::keymap::Chord::new(modifiers.ctrl || modifiers.command, modifiers.shift, modifiers.alt, key_name(*key)),
                _ => None,
            })
            .collect()
    })
}
/// Smoothed texture (for photos and splash images; pixel textures go through `upload`).
pub fn load_smooth(ctx: &Context, name: &str, w: usize, h: usize, rgba: &[u8]) -> TextureHandle {
    ctx.load_texture(name, ColorImage::from_rgba_unmultiplied([w, h], &rgba[..w * h * 4]), TextureOptions::LINEAR)
}
/// Centers the window on the monitor (if the system reports the monitor size).
pub fn center_window(ctx: &Context, size: [f32; 2]) {
    if let Some(m) = ctx.input(|i| i.viewport().monitor_size) {
        let pos = Pos2::new(((m.x - size[0]) * 0.5).max(0.0), ((m.y - size[1]) * 0.5).max(0.0));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    }
}
/// Turns the splash window into the regular main window: decorations, resizable, normal size.
pub fn enter_main(ctx: &Context, title: &str, size: [f32; 2], min: [f32; 2]) {
    // at most 95% of the monitor so the window never extends past the screen
    let fit = ctx.input(|i| i.viewport().monitor_size).map_or(size, |m| [size[0].min(m.x * 0.95), size[1].min(m.y * 0.92)]);
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::new(min[0].min(fit[0]), min[1].min(fit[1]))));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(fit[0], fit[1])));
    ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.to_string()));
    center_window(ctx, fit);
}
/// Starts the app. `splash` is the splash window size: the window then starts borderless and non-resizable.
pub fn run(title: &str, size: [f32; 2], min: [f32; 2], splash: Option<[f32; 2]>, make: impl FnOnce(&Context) -> Box<dyn eframe::App> + 'static) -> AppResult {
    let vb = egui::ViewportBuilder::default().with_title(title);
    let vb = match splash {
        // The splash stays resizable on purpose: a non-resizable window makes Wayland compositors pin
        // min = max = splash size, and the later switch to the larger main window then violates
        // "minimum width can't be bigger than the maximum width" (xdg_toplevel error 2).
        // Without decorations nobody can resize it anyway.
        Some(s) => vb.with_inner_size(s).with_decorations(false).with_resizable(true),
        None => vb.with_inner_size(size).with_min_inner_size(min),
    };
    let opts = eframe::NativeOptions { viewport: vb, ..Default::default() };
    eframe::run_native(title, opts, Box::new(move |cc| Ok(make(&cc.egui_ctx))))
}

/// A texture owned by us on the GPU, updated straight from a pixel buffer.
///
/// egui's own texture path converts every pixel to its internal colour type and re-uploads the whole
/// image through several copies. For a full-size viewport that is the biggest per-frame cost after the
/// drawing itself, so the viewport pictures go through this type: one `glTexSubImage2D` of the RGBA
/// bytes, no conversion, no allocation.
pub struct GlSurface {
    tex: Option<eframe::glow::Texture>,
    id: Option<egui::TextureId>,
    w: u32,
    h: u32,
}
impl Default for GlSurface {
    fn default() -> Self {
        Self::new()
    }
}
impl GlSurface {
    pub fn new() -> Self {
        Self { tex: None, id: None, w: 0, h: 0 }
    }
    /// Creates and registers the GL texture the first time it is needed.
    pub fn ensure(&mut self, frame: &mut eframe::Frame) {
        use eframe::glow::{self, HasContext};
        if self.tex.is_some() {
            return;
        }
        let Some(gl) = frame.gl().cloned() else { return };
        // SAFETY: plain GL calls on the context eframe created; the texture is handed over to egui_glow, which frees it.
        let tex = unsafe {
            let Ok(t) = gl.create_texture() else { return };
            gl.bind_texture(glow::TEXTURE_2D, Some(t));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            t
        };
        self.id = Some(frame.register_native_glow_texture(tex));
        self.tex = Some(tex);
    }
    pub fn id(&self) -> Option<egui::TextureId> {
        self.id.filter(|_| self.w > 0)
    }
    /// Smooth (linear) magnification, for pictures shown larger or smaller than their pixel size.
    pub fn set_linear(&self, gl: &eframe::glow::Context, linear: bool) {
        use eframe::glow::{self, HasContext};
        let Some(t) = self.tex else { return };
        let f = if linear { glow::LINEAR } else { glow::NEAREST } as i32;
        // SAFETY: see `ensure`.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(t));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, f);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, f);
        }
    }
    /// Uploads `rgba` (premultiplied alpha, `w*h*4` bytes). Re-allocates GPU storage only when the size changes.
    pub fn upload(&mut self, gl: &eframe::glow::Context, w: u32, h: u32, rgba: &[u8]) {
        use eframe::glow::{self, HasContext};
        let Some(t) = self.tex else { return };
        if w == 0 || h == 0 || rgba.len() < (w as usize) * (h as usize) * 4 {
            return;
        }
        let data = glow::PixelUnpackData::Slice(Some(&rgba[..(w as usize) * (h as usize) * 4]));
        // SAFETY: the slice length was checked above; format RGBA/UNSIGNED_BYTE matches the data.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(t));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            if self.w != w || self.h != h {
                gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w as i32, h as i32, 0, glow::RGBA, glow::UNSIGNED_BYTE, data);
                self.w = w;
                self.h = h;
            } else {
                gl.tex_sub_image_2d(glow::TEXTURE_2D, 0, 0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, data);
            }
        }
    }
}

/// The area of the window that egui lays out in (version-sensitive name).
pub fn screen_rect(ctx: &eframe::egui::Context) -> eframe::egui::Rect {
    ctx.screen_rect()
}
/// Where a window of about `size` starts so that it first appears in the middle of the screen. Unlike `Window::anchor`, the
/// window can then be dragged anywhere and resizing it does not make it jump.
pub fn centered_pos(ctx: &eframe::egui::Context, size: eframe::egui::Vec2) -> eframe::egui::Pos2 {
    let r = screen_rect(ctx);
    eframe::egui::pos2((r.center().x - size.x * 0.5).max(r.min.x), (r.center().y - size.y * 0.5).max(r.min.y))
}
