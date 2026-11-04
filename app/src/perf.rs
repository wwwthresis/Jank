//! Frame-time bookkeeping for the viewport: smoothed timings for the statistics overlay and the
//! controller that picks the viewport's drawing scale in "Auto" quality.
use psxcore::settings::ViewportQuality;

/// Scales the Auto mode moves between.
const STEPS: [f32; 4] = [1.0, 0.75, 0.5, 0.35];
/// Target: drawing + upload of the main viewport should stay below this many milliseconds.
const SLOW_MS: f32 = 20.0;
const FAST_MS: f32 = 7.0;

pub struct Perf {
    pub frame_ms: f32,
    pub draw_ms: f32,
    pub preview_ms: f32,
    step: usize,
    cooldown: u32,
}

impl Default for Perf {
    fn default() -> Self {
        Self { frame_ms: 16.0, draw_ms: 0.0, preview_ms: 0.0, step: 0, cooldown: 0 }
    }
}

fn ema(old: f32, new: f32) -> f32 {
    old + (new - old) * 0.15
}

impl Perf {
    pub fn note_frame(&mut self, dt_seconds: f32) {
        self.frame_ms = ema(self.frame_ms, (dt_seconds * 1000.0).clamp(0.1, 500.0));
    }
    pub fn note_draw(&mut self, ms: f32) {
        self.draw_ms = ema(self.draw_ms, ms);
    }
    pub fn fps(&self) -> f32 {
        1000.0 / self.frame_ms.max(0.1)
    }
    /// The drawing scale to use for the next viewport frame.
    pub fn scale(&mut self, q: ViewportQuality) -> f32 {
        if q != ViewportQuality::Auto {
            self.step = 0;
            return q.scale();
        }
        if self.cooldown > 0 {
            self.cooldown -= 1;
        } else if self.draw_ms > SLOW_MS && self.step + 1 < STEPS.len() {
            self.step += 1;
            self.cooldown = 24;
        } else if self.draw_ms < FAST_MS && self.step > 0 {
            self.step -= 1;
            self.cooldown = 90; // going back up is slower, so the scale does not flicker
        }
        STEPS[self.step]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_scale_drops_when_slow_and_recovers_when_fast() {
        let mut p = Perf::default();
        assert_eq!(p.scale(ViewportQuality::Auto), 1.0);
        for _ in 0..60 {
            p.note_draw(40.0);
        }
        let mut s = 1.0;
        for _ in 0..200 {
            s = p.scale(ViewportQuality::Auto);
        }
        assert!(s <= 0.5, "got {s}");
        for _ in 0..200 {
            p.note_draw(2.0);
        }
        for _ in 0..1000 {
            s = p.scale(ViewportQuality::Auto);
        }
        assert_eq!(s, 1.0);
    }
    #[test]
    fn manual_quality_is_fixed() {
        let mut p = Perf::default();
        p.note_draw(100.0);
        assert_eq!(p.scale(ViewportQuality::Half), 0.5);
    }
}
