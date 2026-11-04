//! Lighting of the scene: sun and moon by time of day, and cheap shadows.
//!
//! Time of day replaces the light direction, light colour, ambient light, sky and fog colours of the render style by values
//! computed from the hour (0..24). The sun crosses the sky from 06:00 to 18:00, the moon from 18:00 to 06:00. Nothing here
//! costs render time: the colours are worked out once per frame.
//!
//! Shadows (see `shadow.rs`) are worked out with a shadow map: the scene is drawn once as seen from the light, and every pixel of the
//! picture is then asked whether something stands between it and the light. So they fall on any surface (floor, walls, other objects):
//! * `Blob`: every object is replaced by a soft ellipsoid, which gives a soft dark spot (fast, very PS1);
//! * `Projected`: the real shape of the objects (cut-out textures such as wings and leaves included), sharpness by `ShadowQuality`.
use crate::math::*;
use crate::raster::PsxStyle;
use std::borrow::Cow;

/// An upper limit for shadows: at most `mode` (Off < Blob < Cast) and, for cast shadows, at most `quality`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowCap {
    pub mode: ShadowMode,
    pub quality: ShadowQuality,
}
impl ShadowCap {
    /// The mode and quality to draw with when the scene asks for `mode` / `quality`.
    pub fn limit(self, mode: ShadowMode, quality: ShadowQuality) -> (ShadowMode, ShadowQuality) {
        let rank = |m: ShadowMode| match m {
            ShadowMode::Off => 0,
            ShadowMode::Blob => 1,
            ShadowMode::Projected => 2,
        };
        let m = if rank(mode) <= rank(self.mode) { mode } else { self.mode };
        let q = if quality.id() <= self.quality.id() { quality } else { self.quality };
        (m, q)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowMode {
    Off,
    Blob,
    Projected,
}
impl ShadowMode {
    pub const ALL: [ShadowMode; 3] = [ShadowMode::Off, ShadowMode::Blob, ShadowMode::Projected];
    pub fn label(self) -> &'static str {
        match self {
            ShadowMode::Off => "Off",
            ShadowMode::Blob => "Blob",
            ShadowMode::Projected => "Cast",
        }
    }
    pub fn id(self) -> u8 {
        match self {
            ShadowMode::Off => 0,
            ShadowMode::Blob => 1,
            ShadowMode::Projected => 2,
        }
    }
    pub fn from_id(v: u8) -> ShadowMode {
        match v {
            1 => ShadowMode::Blob,
            2 => ShadowMode::Projected,
            _ => ShadowMode::Off,
        }
    }
}

/// Resolution of the shadow map: sharper edges cost more time and memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowQuality {
    Low,
    Medium,
    High,
    Ultra,
}
impl ShadowQuality {
    pub const ALL: [ShadowQuality; 4] = [ShadowQuality::Low, ShadowQuality::Medium, ShadowQuality::High, ShadowQuality::Ultra];
    pub fn label(self) -> &'static str {
        match self {
            ShadowQuality::Low => "Low",
            ShadowQuality::Medium => "Medium",
            ShadowQuality::High => "High",
            ShadowQuality::Ultra => "Ultra",
        }
    }
    /// Side of the square shadow map in texels.
    pub fn resolution(self) -> usize {
        match self {
            ShadowQuality::Low => 256,
            ShadowQuality::Medium => 512,
            ShadowQuality::High => 1024,
            ShadowQuality::Ultra => 2048,
        }
    }
    pub fn id(self) -> u8 {
        self as u8
    }
    pub fn from_id(v: u8) -> ShadowQuality {
        match v {
            0 => ShadowQuality::Low,
            2 => ShadowQuality::High,
            3 => ShadowQuality::Ultra,
            _ => ShadowQuality::Medium,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Derive the light and the sky from `time` instead of using the style's own values.
    pub tod_on: bool,
    /// Hour of the day, 0..24.
    pub time: f32,
    /// Compass direction of the sun's path in degrees.
    pub azimuth: f32,
    pub shadows: ShadowMode,
    /// 0 = invisible, 1 = black.
    pub shadow_strength: f32,
    pub quality: ShadowQuality,
    /// 0 = crisp edge, 1 = very soft edge.
    pub softness: f32,
}
impl Default for Lighting {
    fn default() -> Self {
        Lighting { tod_on: false, time: 12.0, azimuth: 35.0, shadows: ShadowMode::Off, shadow_strength: 0.45, quality: ShadowQuality::Medium, softness: 0.5 }
    }
}
impl Lighting {
    /// True when the project still has the default (nothing to save).
    pub fn is_default(&self) -> bool {
        *self == Lighting::default()
    }
    /// The style with time-of-day values applied (borrowed unchanged when time of day is off).
    pub fn apply<'a>(&self, style: &'a PsxStyle) -> Cow<'a, PsxStyle> {
        if !self.tod_on {
            return Cow::Borrowed(style);
        }
        let d = daylight(self.time, self.azimuth);
        let mut s = style.clone();
        s.light_dir = d.light_dir;
        s.light_color = d.light_color;
        s.ambient = d.ambient;
        s.bg_top = d.sky_top;
        s.bg_bottom = d.sky_bottom;
        s.fog_color = d.sky_bottom;
        Cow::Owned(s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Daylight {
    /// Direction the light travels (from the sun or moon towards the scene).
    pub light_dir: Vec3,
    pub light_color: [f32; 3],
    pub ambient: [f32; 3],
    pub sky_top: [f32; 3],
    pub sky_bottom: [f32; 3],
    /// True while the sun is the light source.
    pub sun: bool,
    /// Height of the light source above the horizon in degrees.
    pub elevation_deg: f32,
}

type Key = (f32, [f32; 3], [f32; 3], [f32; 3], [f32; 3]); // hour, light, ambient, sky top, sky bottom
const KEYS: [Key; 10] = [
    (0.0, [0.20, 0.25, 0.42], [0.10, 0.12, 0.20], [0.02, 0.03, 0.09], [0.07, 0.09, 0.18]),
    (4.5, [0.18, 0.22, 0.38], [0.11, 0.13, 0.22], [0.04, 0.05, 0.14], [0.12, 0.12, 0.26]),
    (6.0, [0.85, 0.45, 0.28], [0.28, 0.24, 0.34], [0.22, 0.28, 0.50], [0.95, 0.55, 0.38]),
    (7.5, [1.00, 0.80, 0.60], [0.40, 0.40, 0.48], [0.30, 0.52, 0.88], [0.90, 0.80, 0.70]),
    (12.0, [1.00, 0.96, 0.88], [0.46, 0.50, 0.58], [0.25, 0.52, 0.95], [0.75, 0.88, 1.00]),
    (16.5, [1.00, 0.88, 0.70], [0.44, 0.46, 0.52], [0.28, 0.50, 0.90], [0.86, 0.86, 0.86]),
    (18.0, [0.95, 0.50, 0.25], [0.32, 0.26, 0.32], [0.24, 0.28, 0.55], [0.98, 0.52, 0.30]),
    (19.5, [0.35, 0.30, 0.50], [0.18, 0.16, 0.28], [0.08, 0.09, 0.25], [0.40, 0.25, 0.38]),
    (21.0, [0.22, 0.27, 0.45], [0.11, 0.13, 0.22], [0.03, 0.04, 0.12], [0.09, 0.10, 0.20]),
    (24.0, [0.20, 0.25, 0.42], [0.10, 0.12, 0.20], [0.02, 0.03, 0.09], [0.07, 0.09, 0.18]),
];

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Light and sky for an hour of the day (any real number; it wraps around 24).
pub fn daylight(time: f32, azimuth_deg: f32) -> Daylight {
    let t = time.rem_euclid(24.0);
    let i = KEYS.iter().rposition(|k| k.0 <= t).unwrap_or(0).min(KEYS.len() - 2);
    let (a, b) = (&KEYS[i], &KEYS[i + 1]);
    let f = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    // source of light: the sun by day, the moon by night; both cross the sky along a half circle
    let sun = (6.0..18.0).contains(&t);
    let phase = if sun { (t - 6.0) / 12.0 } else { ((t - 18.0).rem_euclid(24.0)) / 12.0 };
    let ang = phase * std::f32::consts::PI;
    let elev = ang.sin() * if sun { 1.0 } else { 0.8 };
    let az = azimuth_deg.to_radians();
    let horiz = Vec3::new(az.cos(), 0.0, az.sin());
    // keep the light from skimming the ground: at least a few degrees above the horizon
    let y = elev.max(0.0).max(0.08);
    let source = (horiz * ang.cos() * 0.9 + Vec3::Y * y).normalized();
    Daylight {
        light_dir: -source,
        light_color: lerp3(a.1, b.1, f),
        ambient: lerp3(a.2, b.2, f),
        sky_top: lerp3(a.3, b.3, f),
        sky_bottom: lerp3(a.4, b.4, f),
        sun,
        elevation_deg: elev.asin().to_degrees(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_is_bright_and_high_midnight_is_dim_and_blue() {
        let n = daylight(12.0, 35.0);
        let m = daylight(0.0, 35.0);
        assert!(n.sun && !m.sun);
        assert!(n.light_dir.y < -0.9, "the sun is overhead at noon: {:?}", n.light_dir);
        assert!(n.elevation_deg > 80.0);
        let lum = |c: [f32; 3]| c[0] + c[1] + c[2];
        assert!(lum(n.light_color) > 2.5 * lum(m.light_color));
        assert!(m.light_color[2] > m.light_color[0], "moonlight is bluish");
        assert!(lum(n.sky_top) > 2.0 * lum(m.sky_top));
    }
    #[test]
    fn sunrise_is_warm_and_low() {
        let r = daylight(6.0, 0.0);
        assert!(r.light_color[0] > r.light_color[2], "warm light at sunrise");
        assert!(r.elevation_deg < 5.0);
        let s = daylight(18.0, 0.0);
        assert!(s.light_color[0] > s.light_color[2]);
    }
    #[test]
    fn colours_change_smoothly_and_wrap_around() {
        let mut prev = daylight(0.0, 10.0);
        for k in 1..=240 {
            let d = daylight(k as f32 * 0.1, 10.0);
            for c in 0..3 {
                assert!((d.sky_top[c] - prev.sky_top[c]).abs() < 0.08, "sky jumps at {}h", k as f32 * 0.1);
                assert!((d.light_color[c] - prev.light_color[c]).abs() < 0.2);
            }
            prev = d;
        }
        assert_eq!(daylight(25.0, 0.0).light_color, daylight(1.0, 0.0).light_color);
        assert_eq!(daylight(-1.0, 0.0).sky_top, daylight(23.0, 0.0).sky_top);
    }
    #[test]
    fn apply_only_changes_the_style_when_on() {
        let st = PsxStyle::ps1_classic();
        let mut l = Lighting::default();
        assert!(matches!(l.apply(&st), Cow::Borrowed(_)));
        l.tod_on = true;
        l.time = 21.0;
        let out = l.apply(&st);
        assert_ne!(out.bg_top, st.bg_top);
        assert_eq!(out.width, st.width);
        assert_eq!(ShadowMode::from_id(ShadowMode::Projected.id()), ShadowMode::Projected);
    }

    #[test]
    fn a_shadow_cap_only_ever_lowers_the_shadows() {
        let cap = ShadowCap { mode: ShadowMode::Projected, quality: ShadowQuality::Medium };
        assert_eq!(cap.limit(ShadowMode::Projected, ShadowQuality::Ultra), (ShadowMode::Projected, ShadowQuality::Medium));
        assert_eq!(cap.limit(ShadowMode::Projected, ShadowQuality::Low), (ShadowMode::Projected, ShadowQuality::Low));
        assert_eq!(cap.limit(ShadowMode::Off, ShadowQuality::Ultra).0, ShadowMode::Off);
        let blob = ShadowCap { mode: ShadowMode::Blob, quality: ShadowQuality::Ultra };
        assert_eq!(blob.limit(ShadowMode::Projected, ShadowQuality::High).0, ShadowMode::Blob);
        assert_eq!(blob.limit(ShadowMode::Off, ShadowQuality::High).0, ShadowMode::Off);
    }
}
