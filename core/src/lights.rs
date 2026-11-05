//! Lamps of the scene: point lights and spot lights with their own colour, strength, reach, flicker and glow.
//!
//! The sun and the moon (`lighting.rs`) light the whole world from one direction. Lamps light what is near them:
//! a candle on a table, a lantern by a path, a neon sign. Every lamp has
//! * a colour and a strength (`intensity`, 1 = about as strong as a plain sun on a surface right next to the lamp),
//! * a reach (`range`): nothing farther away is lit, and the light fades out smoothly before it,
//! * a falloff: 0 = soft and even, 1 = bright near the lamp and quickly darker (closer to a real bulb),
//! * for spot lights a cone (`cone_deg`, the full opening angle) with a soft edge,
//! * flicker (candles and fire) and a halo: a soft glow drawn around the lamp itself,
//! * optionally an object it is attached to: then `pos` / `rot_deg` are in that object's space and the lamp moves with it.
//!
//! Lamps are light only: they do not cast shadows (that keeps them cheap). Rendering asks [`resolve`] for the lamps of one
//! frame in world space ([`LiveLight`]) and adds their light per vertex (PS1 style) or per pixel (`PsxStyle::pixel_lights`).
use crate::math::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    /// Shines in every direction.
    Point,
    /// Shines in a cone along its direction.
    Spot,
}
impl LightKind {
    pub const ALL: [LightKind; 2] = [LightKind::Point, LightKind::Spot];
    pub fn label(self) -> &'static str {
        match self {
            LightKind::Point => "Point",
            LightKind::Spot => "Spot",
        }
    }
    pub fn id(self) -> u8 {
        match self {
            LightKind::Point => 0,
            LightKind::Spot => 1,
        }
    }
    pub fn from_id(v: u8) -> LightKind {
        if v == 1 {
            LightKind::Spot
        } else {
            LightKind::Point
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    /// Unique within the scene; never reused while the scene is open.
    pub id: u32,
    pub name: String,
    pub kind: LightKind,
    /// Position (in the space of `attach` when set, otherwise in the world).
    pub pos: Vec3,
    /// Orientation of a spot light in Euler degrees; with no rotation it shines straight down (-Y).
    pub rot_deg: Vec3,
    /// Colour 0..1.
    pub color: [f32; 3],
    /// Strength, 0..8 (1 = like a plain sun close to the lamp).
    pub intensity: f32,
    /// How far the light reaches, in world units.
    pub range: f32,
    /// 0 = soft and even, 1 = bright near the lamp and quickly darker.
    pub falloff: f32,
    /// Full opening angle of a spot light, in degrees.
    pub cone_deg: f32,
    /// Softness of the spot's edge, 0 = hard, 1 = very soft.
    pub blend: f32,
    /// 0 = steady, 1 = a nervous fire.
    pub flicker: f32,
    /// How fast it flickers (1 = candle speed).
    pub flicker_speed: f32,
    /// Size and strength of the glow around the lamp (0 = none).
    pub halo: f32,
    /// The object this lamp moves with.
    pub attach: Option<u32>,
    pub enabled: bool,
}

/// Ready-made lamps for the Add menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightPreset {
    Bulb,
    Candle,
    Lantern,
    Campfire,
    Torch,
    Neon,
    Moonbeam,
    Spotlight,
    Firefly,
}
impl LightPreset {
    pub const ALL: [LightPreset; 9] = [LightPreset::Bulb, LightPreset::Candle, LightPreset::Lantern, LightPreset::Campfire, LightPreset::Torch, LightPreset::Neon, LightPreset::Moonbeam, LightPreset::Spotlight, LightPreset::Firefly];
    pub fn label(self) -> &'static str {
        match self {
            LightPreset::Bulb => "Bulb",
            LightPreset::Candle => "Candle",
            LightPreset::Lantern => "Lantern",
            LightPreset::Campfire => "Campfire",
            LightPreset::Torch => "Torch",
            LightPreset::Neon => "Neon",
            LightPreset::Moonbeam => "Moonbeam",
            LightPreset::Spotlight => "Spotlight",
            LightPreset::Firefly => "Firefly",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            LightPreset::Bulb => "A plain warm-white lamp",
            LightPreset::Candle => "Small, warm and flickering, with a soft glow",
            LightPreset::Lantern => "An orange paper or stone lantern",
            LightPreset::Campfire => "Strong orange light that dances",
            LightPreset::Torch => "A hand torch: orange, flickering, medium reach",
            LightPreset::Neon => "A pink neon sign: steady, with a bright glow",
            LightPreset::Moonbeam => "A cold blue spot from above",
            LightPreset::Spotlight => "A white theatre spot pointing down",
            LightPreset::Firefly => "A tiny green-yellow point that pulses",
        }
    }
}

impl Light {
    pub fn new(id: u32, preset: LightPreset) -> Light {
        let mut l = Light {
            id,
            name: preset.label().into(),
            kind: LightKind::Point,
            pos: Vec3::new(0.0, 1.5, 0.0),
            rot_deg: Vec3::ZERO,
            color: [1.0, 0.86, 0.66],
            intensity: 1.2,
            range: 5.0,
            falloff: 0.5,
            cone_deg: 50.0,
            blend: 0.35,
            flicker: 0.0,
            flicker_speed: 1.0,
            halo: 0.3,
            attach: None,
            enabled: true,
        };
        match preset {
            LightPreset::Bulb => {}
            LightPreset::Candle => {
                l.color = [1.0, 0.66, 0.32];
                l.intensity = 1.0;
                l.range = 2.5;
                l.falloff = 0.6;
                l.flicker = 0.35;
                l.halo = 0.45;
                l.pos = Vec3::new(0.0, 0.6, 0.0);
            }
            LightPreset::Lantern => {
                l.color = [1.0, 0.62, 0.30];
                l.intensity = 1.4;
                l.range = 4.5;
                l.falloff = 0.45;
                l.flicker = 0.12;
                l.halo = 0.55;
            }
            LightPreset::Campfire => {
                l.color = [1.0, 0.50, 0.18];
                l.intensity = 2.4;
                l.range = 7.0;
                l.falloff = 0.55;
                l.flicker = 0.55;
                l.flicker_speed = 1.4;
                l.halo = 0.8;
                l.pos = Vec3::new(0.0, 0.5, 0.0);
            }
            LightPreset::Torch => {
                l.color = [1.0, 0.58, 0.24];
                l.intensity = 1.6;
                l.range = 5.0;
                l.falloff = 0.5;
                l.flicker = 0.4;
                l.halo = 0.5;
            }
            LightPreset::Neon => {
                l.color = [1.0, 0.28, 0.72];
                l.intensity = 1.5;
                l.range = 4.0;
                l.falloff = 0.35;
                l.halo = 0.9;
            }
            LightPreset::Moonbeam => {
                l.kind = LightKind::Spot;
                l.color = [0.55, 0.68, 1.0];
                l.intensity = 1.2;
                l.range = 12.0;
                l.falloff = 0.15;
                l.cone_deg = 40.0;
                l.blend = 0.6;
                l.halo = 0.0;
                l.pos = Vec3::new(0.0, 6.0, 0.0);
            }
            LightPreset::Spotlight => {
                l.kind = LightKind::Spot;
                l.color = [1.0, 0.97, 0.9];
                l.intensity = 2.2;
                l.range = 9.0;
                l.falloff = 0.3;
                l.cone_deg = 34.0;
                l.blend = 0.25;
                l.halo = 0.35;
                l.pos = Vec3::new(0.0, 4.0, 0.0);
            }
            LightPreset::Firefly => {
                l.color = [0.75, 1.0, 0.35];
                l.intensity = 0.7;
                l.range = 1.2;
                l.falloff = 0.6;
                l.flicker = 0.7;
                l.flicker_speed = 0.35;
                l.halo = 0.35;
                l.pos = Vec3::new(0.0, 1.0, 0.0);
            }
        }
        l
    }
    /// Direction a spot light shines in (in its own space: attached lamps turn with their object).
    pub fn direction(&self) -> Vec3 {
        let r = Vec3::new(self.rot_deg.x.to_radians(), self.rot_deg.y.to_radians(), self.rot_deg.z.to_radians());
        Quat::from_euler(r).rotate(Vec3::new(0.0, -1.0, 0.0)).normalized()
    }
    /// Brightness multiplier at time `t` seconds: 1 for a steady lamp, wandering below 1 for a flickering one.
    /// Smooth, deterministic (the same time always gives the same value) and different for every lamp.
    pub fn flicker_at(&self, t: f32) -> f32 {
        if self.flicker <= 0.0 {
            return 1.0;
        }
        let ph = (self.id as f32 * 2.399_963).rem_euclid(std::f32::consts::TAU);
        let s = t * self.flicker_speed.clamp(0.05, 8.0) * 6.0;
        let n = 0.5 * (s * 1.7 + ph).sin() + 0.3 * (s * 3.9 + ph * 1.3).sin() + 0.2 * (s * 9.7 + ph * 2.1).sin();
        let k = 0.5 + 0.5 * n; // 0..1
        (1.0 - self.flicker.clamp(0.0, 1.0) * 0.55 * k).max(0.0)
    }
    /// Sanity limits for values that come from files or typed fields.
    pub fn clamp(&mut self) {
        let f = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.intensity = f(self.intensity, 0.0, 8.0, 1.0);
        self.range = f(self.range, 0.05, 500.0, 5.0);
        self.falloff = f(self.falloff, 0.0, 1.0, 0.5);
        self.cone_deg = f(self.cone_deg, 1.0, 170.0, 50.0);
        self.blend = f(self.blend, 0.0, 1.0, 0.3);
        self.flicker = f(self.flicker, 0.0, 1.0, 0.0);
        self.flicker_speed = f(self.flicker_speed, 0.05, 8.0, 1.0);
        self.halo = f(self.halo, 0.0, 2.0, 0.0);
        for c in &mut self.color {
            *c = f(*c, 0.0, 1.0, 1.0);
        }
        let ok = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        if !ok(self.pos) {
            self.pos = Vec3::ZERO;
        }
        if !ok(self.rot_deg) {
            self.rot_deg = Vec3::ZERO;
        }
    }
}

/// A lamp of one frame, in world space, ready for the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveLight {
    pub pos: Vec3,
    /// Unit direction of a spot light (unused for points).
    pub dir: Vec3,
    /// Colour times strength times flicker.
    pub rgb: [f32; 3],
    pub range: f32,
    /// 1 / range².
    pub inv_r2: f32,
    /// Extra darkening with distance (from `falloff`).
    pub k: f32,
    pub spot: bool,
    /// Cosines of the edge of the cone: fully lit inside `cos_in`, dark outside `cos_out`.
    pub cos_in: f32,
    pub cos_out: f32,
    pub halo: f32,
}

impl LiveLight {
    /// Light arriving at point `p` on a surface with unit normal `n` (`two_sided`: both sides of the surface are lit).
    #[inline]
    pub fn at(&self, p: Vec3, n: Vec3, two_sided: bool) -> Option<[f32; 3]> {
        let l = self.pos - p;
        let d2 = l.dot(l);
        let x = d2 * self.inv_r2;
        if x >= 1.0 {
            return None;
        }
        let d = d2.sqrt().max(1e-5);
        let mut ndl = n.dot(l) / d;
        if two_sided {
            ndl = ndl.abs();
        }
        // a little wrap makes low-poly surfaces look rounder near the lamp
        let ndl = ((ndl + 0.15) / 1.15).max(0.0);
        if ndl <= 0.0 {
            return None;
        }
        let win = (1.0 - x) * (1.0 - x);
        let mut a = win * ndl / (1.0 + self.k * x);
        if self.spot {
            let c = -(l.dot(self.dir)) / d;
            if c <= self.cos_out {
                return None;
            }
            let t = ((c - self.cos_out) / (self.cos_in - self.cos_out).max(1e-4)).clamp(0.0, 1.0);
            a *= t * t * (3.0 - 2.0 * t);
        }
        Some([self.rgb[0] * a, self.rgb[1] * a, self.rgb[2] * a])
    }
    /// True if this lamp can reach anything in the box `lo..hi`.
    pub fn touches_box(&self, lo: Vec3, hi: Vec3) -> bool {
        let c = self.pos.max(lo).min(hi);
        let d = c - self.pos;
        d.dot(d) < self.range * self.range
    }
    /// Rough brightness of this lamp (for sorting the strongest first).
    pub fn power(&self) -> f32 {
        (self.rgb[0] + self.rgb[1] + self.rgb[2]) * self.range
    }
}

/// Most lamps that light one mesh; the strongest ones nearest to it win.
pub const MAX_PER_MESH: usize = 8;

/// The lamps of a frame in world space. `world_of(object)` returns the frame's transform of an object (lamps attached to a hidden
/// or missing object are skipped); `t` is the time in seconds for the flicker.
pub fn resolve(lights: &[Light], world_of: &dyn Fn(u32) -> Option<Mat4>, t: f32) -> Vec<LiveLight> {
    let mut out = Vec::new();
    for l in lights.iter().filter(|l| l.enabled && l.intensity > 0.0) {
        let (pos, dir) = match l.attach {
            Some(o) => {
                let Some(m) = world_of(o) else { continue };
                (m.transform_point(l.pos), m.transform_dir(l.direction()).normalized())
            }
            None => (l.pos, l.direction()),
        };
        let f = l.intensity * l.flicker_at(t);
        let half = (l.cone_deg.clamp(1.0, 170.0) * 0.5).to_radians();
        let inner = half * (1.0 - l.blend.clamp(0.0, 1.0) * 0.9);
        let range = l.range.max(0.05);
        out.push(LiveLight {
            pos,
            dir,
            rgb: [l.color[0] * f, l.color[1] * f, l.color[2] * f],
            range,
            inv_r2: 1.0 / (range * range),
            k: l.falloff.clamp(0.0, 1.0) * 24.0,
            spot: l.kind == LightKind::Spot,
            cos_in: inner.cos(),
            cos_out: half.cos(),
            halo: l.halo.max(0.0),
        });
    }
    out
}

/// Indices of the lamps (at most [`MAX_PER_MESH`]) that reach the box `lo..hi`, strongest and nearest first.
pub fn affecting(lights: &[LiveLight], lo: Vec3, hi: Vec3) -> Vec<u8> {
    let c = (lo + hi) * 0.5;
    let mut v: Vec<(f32, u8)> = lights
        .iter()
        .enumerate()
        .take(255)
        .filter(|(_, l)| l.touches_box(lo, hi))
        .map(|(i, l)| {
            let d = (l.pos - c).length();
            (l.power() / (1.0 + d * d), i as u8)
        })
        .collect();
    v.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    v.truncate(MAX_PER_MESH);
    let mut ids: Vec<u8> = v.into_iter().map(|x| x.1).collect();
    ids.sort_unstable();
    ids
}

/// The sum of the listed lamps at a point (without the sun or the ambient light).
#[inline]
pub fn sum_at(lights: &[LiveLight], ids: &[u8], p: Vec3, n: Vec3, two_sided: bool) -> [f32; 3] {
    let mut s = [0.0f32; 3];
    for &i in ids {
        if let Some(c) = lights[i as usize].at(p, n, two_sided) {
            s[0] += c[0];
            s[1] += c[1];
            s[2] += c[2];
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(l: &Light) -> LiveLight {
        resolve(std::slice::from_ref(l), &|_| None, 0.0)[0]
    }

    #[test]
    fn a_point_light_fades_with_distance_and_stops_at_its_range() {
        let mut l = Light::new(1, LightPreset::Bulb);
        l.pos = Vec3::new(0.0, 1.0, 0.0);
        l.range = 4.0;
        let ll = live(&l);
        let up = Vec3::Y;
        let near = ll.at(Vec3::new(0.0, 0.0, 0.0), up, false).unwrap()[0];
        let far = ll.at(Vec3::new(2.0, 0.0, 0.0), up, false).unwrap()[0];
        assert!(near > far && far > 0.0, "{near} {far}");
        assert!(ll.at(Vec3::new(5.0, 0.0, 0.0), up, false).is_none(), "beyond the range nothing is lit");
        assert!(ll.at(Vec3::new(0.0, 0.0, 0.0), -up, false).is_none(), "a surface facing away gets nothing");
        assert!(ll.at(Vec3::new(0.0, 0.0, 0.0), -up, true).is_some(), "unless it is two-sided");
    }

    #[test]
    fn a_spot_light_lights_only_its_cone() {
        let mut l = Light::new(1, LightPreset::Spotlight);
        l.pos = Vec3::new(0.0, 4.0, 0.0);
        l.cone_deg = 30.0;
        let ll = live(&l);
        assert!(ll.at(Vec3::ZERO, Vec3::Y, false).is_some(), "straight below is lit");
        assert!(ll.at(Vec3::new(3.0, 0.0, 0.0), Vec3::Y, false).is_none(), "outside the cone is dark");
        l.rot_deg = Vec3::new(0.0, 0.0, 90.0);
        let d = l.direction();
        assert!(d.y.abs() < 1e-4 && d.x.abs() > 0.99, "rotation turns the cone: {d:?}");
    }

    #[test]
    fn flicker_is_deterministic_and_stays_in_range() {
        let mut l = Light::new(7, LightPreset::Campfire);
        let a: Vec<f32> = (0..50).map(|i| l.flicker_at(i as f32 * 0.07)).collect();
        let b: Vec<f32> = (0..50).map(|i| l.flicker_at(i as f32 * 0.07)).collect();
        assert_eq!(a, b);
        assert!(a.iter().all(|v| (0.4..=1.0).contains(v)), "{a:?}");
        assert!(a.iter().any(|v| *v < 0.95), "it really flickers");
        l.flicker = 0.0;
        assert_eq!(l.flicker_at(3.3), 1.0);
    }

    #[test]
    fn attached_lights_follow_their_object_and_culling_picks_the_near_ones() {
        let mut l = Light::new(1, LightPreset::Lantern);
        l.pos = Vec3::new(0.0, 1.0, 0.0);
        l.attach = Some(5);
        let m = Mat4::translation(Vec3::new(10.0, 0.0, 0.0));
        let ll = resolve(std::slice::from_ref(&l), &|id| (id == 5).then_some(m), 0.0);
        assert!((ll[0].pos - Vec3::new(10.0, 1.0, 0.0)).length() < 1e-5);
        assert!(resolve(std::slice::from_ref(&l), &|_| None, 0.0).is_empty(), "a missing object takes its lamp along");
        let ids = affecting(&ll, Vec3::new(9.0, 0.0, -1.0), Vec3::new(11.0, 2.0, 1.0));
        assert_eq!(ids, vec![0]);
        assert!(affecting(&ll, Vec3::new(-50.0, 0.0, 0.0), Vec3::new(-49.0, 1.0, 1.0)).is_empty());
    }
}
