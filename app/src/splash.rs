//! Startup splash screen: a borderless window with the seasonal picture and a progress bar.
//! The bar first fills to about 40% on its own (an introduction, so the picture is on screen long enough to be seen), then follows
//! the real loading steps (see `startup.rs`). The splash stays at least `MIN_SECS` and closes once loading is finished.
//! The pictures are embedded in the binary; `psxcore::splash` decides which one is shown.
use crate::compat;
use crate::startup::Boot;
use crate::theme::*;
use eframe::egui::{self, vec2, Color32, Context, Rect, TextureHandle};

pub const SPLASH_SIZE: [f32; 2] = [960.0, 768.0]; // 1280x1024 * 0.75
const FADE_IN: f32 = 0.20;
const FADE_OUT: f32 = 0.18;
/// The introduction: the bar fills to `INTRO_TO` during `INTRO_SECS`, whatever the loader is doing.
const INTRO_SECS: f32 = 1.0;
const INTRO_TO: f32 = 0.4;
/// The splash is never shown for less than this, however fast the loading is.
const MIN_SECS: f32 = 2.0;

/// Embedded picture by file name (names are defined by `psxcore::splash::RULES`; the test checks every one).
pub fn bytes(file: &str) -> Option<&'static [u8]> {
    Some(match file {
        "default_a.jpg" => include_bytes!("../assets/splash/default_a.jpg"),
        "default_b.jpg" => include_bytes!("../assets/splash/default_b.jpg"),
        "halloween_1030-1031.jpg" => include_bytes!("../assets/splash/halloween_1030-1031.jpg"),
        "christmas_1224-1225.jpg" => include_bytes!("../assets/splash/christmas_1224-1225.jpg"),
        "newyear_1231-0101.jpg" => include_bytes!("../assets/splash/newyear_1231-0101.jpg"),
        "valentine_0213-0214.jpg" => include_bytes!("../assets/splash/valentine_0213-0214.jpg"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_picture_is_embedded_and_decodes() {
        for r in psxcore::splash::RULES {
            for f in r.files {
                let on_disk = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/splash").join(f);
                let disk = std::fs::read(&on_disk).unwrap_or_else(|_| panic!("missing {}", on_disk.display()));
                assert_eq!(disk, bytes(f).expect("embedded"), "embedded bytes of {f}");
                let img = psxcore::decode::image::decode_image(f, &disk).expect("valid JPEG");
                assert_eq!((img.w, img.h), (1280, 1024), "{f}");
            }
        }
    }
}

pub struct Splash {
    tex: Option<TextureHandle>,
    t: f32,
    leaving: Option<f32>,
    positioned: bool,
    /// The fraction drawn on the bar: it only grows and eases towards the target.
    shown: f32,
}

pub enum State {
    Running,
    Done,
}

impl Splash {
    pub fn new() -> Self {
        Self { tex: None, t: 0.0, leaving: None, positioned: false, shown: 0.0 }
    }

    /// Draws one frame. `finished` tells that the loader is done: the splash then fades out at once.
    pub fn ui(&mut self, ctx: &Context, boot: &mut Boot, finished: bool) -> State {
        if !self.positioned {
            compat::center_window(ctx, SPLASH_SIZE);
            self.positioned = true;
        }
        if self.tex.is_none() {
            if let Some(img) = boot.image.take() {
                self.tex = Some(compat::load_smooth(ctx, "splash", img.w, img.h, &img.rgba));
            }
        }
        let dt = compat::dt(ctx);
        self.t += dt;
        // the bar: introduction first, then the real fraction of the loading work
        let intro = (self.t / INTRO_SECS).clamp(0.0, 1.0);
        let intro = 1.0 - (1.0 - intro) * (1.0 - intro); // eases out
        let target = if self.t < INTRO_SECS { INTRO_TO * intro } else { INTRO_TO + (1.0 - INTRO_TO) * boot.progress.clamp(0.0, 1.0) };
        let target = if finished && self.t >= INTRO_SECS { 1.0 } else { target.min(if finished { 1.0 } else { 0.99 }) };
        self.shown = self.shown.max(self.shown + (target - self.shown) * (dt * 9.0).min(1.0));
        if (target - self.shown).abs() < 0.004 {
            self.shown = target;
        }
        if finished && self.t >= MIN_SECS && self.shown >= 0.999 && self.leaving.is_none() {
            self.leaving = Some(0.0);
        }
        if let Some(l) = &mut self.leaving {
            *l += dt;
            if *l >= FADE_OUT {
                return State::Done;
            }
        }
        let alpha = (self.t / FADE_IN).clamp(0.0, 1.0) * self.leaving.map_or(1.0, |l| 1.0 - (l / FADE_OUT).clamp(0.0, 1.0));
        let a = (alpha * 255.0) as u8;
        let frac = self.shown;
        let label = if self.t < INTRO_SECS { "Starting Jank!" } else { boot.label };

        egui::CentralPanel::default().frame(compat::frame(BG0, 0.0)).show(ctx, |ui| {
            let r = ui.max_rect();
            let p = ui.painter();
            if let Some(tex) = &self.tex {
                p.image(tex.id(), r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::from_white_alpha(a));
            }
            // the status strip: what is being loaded right now, and the real fraction done
            let strip = Rect::from_min_max(egui::pos2(r.min.x, r.max.y - 26.0), r.max);
            p.rect_filled(strip, 0.0, alpha_c(BG0, (a as f32 * 0.82) as u8));
            compat::text(p, egui::pos2(strip.min.x + 10.0, strip.center().y - 1.0), egui::Align2::LEFT_CENTER, label, 12.0, alpha_c(TEXT, a));
            compat::text(p, egui::pos2(strip.max.x - 10.0, strip.center().y - 1.0), egui::Align2::RIGHT_CENTER, &format!("{:.0}%", frac * 100.0), 12.0, alpha_c(DIM, a));
            let bar = Rect::from_min_size(egui::pos2(r.min.x, r.max.y - 3.0), vec2(r.width() * frac, 3.0));
            p.rect_filled(bar, 0.0, alpha_c(ACCENT, a));
            compat::stroke_rect(p, r, 0.0, compat::stroke(1.0, alpha_c(LINE, a)));
        });
        ctx.request_repaint();
        State::Running
    }
}

fn alpha_c(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), ((c.a() as u16 * a as u16) / 255) as u8)
}
