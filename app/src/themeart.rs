//! Pictures of the active theme (background, title bar, panels…): loaded into textures, painted into their places, and
//! picked up again when the theme file or a picture changes on disk.
use crate::theme::*;
use eframe::egui::{self, pos2, Color32, Context, Painter, Rect, TextureHandle, TextureOptions};
use psxcore::themes::{ImageFit, Theme, ThemeImage, IMAGE_SLOTS};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// A place a theme picture can be shown (index into `IMAGE_SLOTS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Background = 0,
    Titlebar = 1,
    Ribbon = 2,
    Panel = 3,
    Statusbar = 4,
    Viewport = 5,
    Welcome = 6,
    Timeline = 7,
    Logo = 8,
}

struct Loaded {
    tex: TextureHandle,
    size: [f32; 2],
    img: ThemeImage,
}

/// Largest picture side that is loaded (bigger pictures are refused with a message).
const MAX_SIDE: u32 = 8192;

#[derive(Default)]
pub struct ThemeArt {
    /// Theme id and file times the textures were made from.
    stamp: Option<(String, u64)>,
    tex: [Option<Loaded>; 9],
    /// Problems with the pictures of the current theme (missing or unreadable files).
    pub problems: Vec<String>,
    last_check: Option<Instant>,
}

/// File times of the theme file and its pictures, mixed into one number.
fn file_stamp(t: &Theme) -> u64 {
    let mut k = 0xcbf29ce484222325u64;
    let mut mix = |p: Option<PathBuf>| {
        let t = p.and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(1, |d| d.as_nanos() as u64);
        k = (k ^ t).wrapping_mul(0x100000001b3);
    };
    mix(t.path.clone());
    for (slot, _) in IMAGE_SLOTS {
        mix(t.image_path(slot));
    }
    k
}

impl ThemeArt {
    /// True when the theme file or one of its pictures changed on disk since the last look (checked about once a second).
    pub fn changed_on_disk(&mut self, t: &Theme) -> bool {
        if t.path.is_none() {
            return false;
        }
        if self.last_check.is_some_and(|c| c.elapsed() < Duration::from_millis(900)) {
            return false;
        }
        self.last_check = Some(Instant::now());
        self.stamp.as_ref().is_some_and(|(id, s)| *id == t.id() && *s != file_stamp(t))
    }
    /// Makes sure the textures match the theme (loads, reloads or drops pictures).
    pub fn sync(&mut self, ctx: &Context, t: &Theme) {
        let has_any = t.images.iter().any(|i| i.is_some());
        if !has_any {
            if self.stamp.is_some() || self.tex.iter().any(|x| x.is_some()) {
                *self = ThemeArt::default();
            }
            return;
        }
        let stamp = (t.id(), file_stamp(t));
        if self.stamp.as_ref() == Some(&stamp) {
            // same files: only the way the pictures are shown may have changed
            for (i, l) in self.tex.iter_mut().enumerate() {
                if let (Some(l), Some(img)) = (l.as_mut(), t.images[i].as_ref()) {
                    l.img = img.clone();
                }
            }
            return;
        }
        self.problems.clear();
        for (i, (slot, _)) in IMAGE_SLOTS.iter().enumerate() {
            self.tex[i] = None;
            let Some(img) = t.images[i].clone() else { continue };
            let Some(path) = t.image_path(slot) else {
                self.problems.push(format!("{slot}: picture \"{}\" not found next to the theme file or in its images folder", img.file));
                continue;
            };
            let loaded = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| psxcore::decode::image::decode_image(&img.file, &b));
            match loaded {
                Ok(im) if im.w > 0 && im.h > 0 && im.w <= MAX_SIDE && im.h <= MAX_SIDE => {
                    let opts = if img.fit == ImageFit::Tile { TextureOptions::LINEAR_REPEAT } else { TextureOptions::LINEAR };
                    let ci = egui::ColorImage::from_rgba_unmultiplied([im.w as usize, im.h as usize], &im.rgba);
                    let tex = ctx.load_texture(format!("theme-{slot}"), ci, opts);
                    self.tex[i] = Some(Loaded { tex, size: [im.w as f32, im.h as f32], img });
                }
                Ok(im) => self.problems.push(format!("{slot}: picture \"{}\" is too large ({}×{}, at most {MAX_SIDE})", img.file, im.w, im.h)),
                Err(e) => self.problems.push(format!("{slot}: could not read \"{}\": {e}", img.file)),
            }
        }
        self.stamp = Some(stamp);
    }
    pub fn has(&self, slot: Slot) -> bool {
        self.tex[slot as usize].is_some()
    }
    /// Size of a slot's picture in pixels.
    pub fn size(&self, slot: Slot) -> Option<[f32; 2]> {
        self.tex[slot as usize].as_ref().map(|l| l.size)
    }
    /// Paints a slot's picture into `rect` (nothing when the theme has no picture there).
    pub fn paint(&self, p: &Painter, slot: Slot, rect: Rect) {
        let Some(l) = &self.tex[slot as usize] else { return };
        let (uv, r) = l.img.fit.layout(l.size, [rect.width(), rect.height()]);
        let place = Rect::from_min_max(pos2(rect.min.x + r[0], rect.min.y + r[1]), pos2(rect.min.x + r[2], rect.min.y + r[3]));
        let t = l.img.tint.unwrap_or([255, 255, 255]);
        let tint = Color32::from_rgba_unmultiplied(t[0], t[1], t[2], (l.img.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
        p.image(l.tex.id(), place, Rect::from_min_max(pos2(uv[0], uv[1]), pos2(uv[2], uv[3])), tint);
    }
    /// Paints a slot's picture over the whole panel the `ui` belongs to (`pad` = the panel's inner margin).
    pub fn paint_panel(&self, ui: &egui::Ui, slot: Slot, pad: egui::Vec2) {
        if self.has(slot) {
            let r = ui.max_rect().expand2(pad);
            self.paint(&ui.painter().with_clip_rect(r), slot, r);
        }
    }
}

/// The window background behind every panel (seen through panels when the theme's panel opacity is below 1).
pub fn paint_background(ctx: &Context, art: &ThemeArt) {
    if art.has(Slot::Background) {
        let p = ctx.layer_painter(egui::LayerId::background());
        let r = ctx.screen_rect();
        p.rect_filled(r, 0.0, BG0());
        art.paint(&p, Slot::Background, r);
    }
}
