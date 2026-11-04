//! Shadows by a shadow map.
//!
//! The objects are drawn once as seen from the light (a flat view along the light's direction) into a small depth picture: for every
//! texel it keeps how far from the light the nearest surface is. After the normal drawing, every pixel of the picture is turned back
//! into a point in the world (from its depth), and that point is looked up in the depth picture: if something is nearer to the light, the
//! point is in shadow. This is why shadows fall on any surface - the floor, a wall, another object - and not on one fixed height.
//!
//! * Objects with a cut-out texture (wings, leaves) cast the shape of the cut-out.
//! * `blob` mode replaces every object by a soft ellipsoid, which gives a round soft spot (very PS1, cheap).
//! * The edge is softened by looking at a few neighbouring texels (`softness`).
//! * Flat objects (floors) do not cast shadows: they are only receivers.
use crate::lighting::ShadowQuality;
use crate::math::*;
use crate::mesh::*;
use crate::texture::Texture;
use std::borrow::Cow;

/// One object that may cast a shadow: its model, the posed vertex positions (model space) and where it stands.
pub struct Caster<'a> {
    pub model: &'a Model,
    pub pos: &'a [Vec<Vec3>],
    pub world: Mat4,
}

/// The shadow map plus how strongly it darkens; handed to the renderer for one frame.
pub struct ShadowInput {
    pub map: ShadowMap,
    /// 0 = invisible, 1 = black.
    pub strength: f32,
}

pub struct ShadowMap {
    res: usize,
    /// Distance from the light of the nearest surface per texel (INFINITY = nothing there).
    depth: Vec<f32>,
    /// Centre of the map and the light's axes: `l` is the direction the light travels, `u` and `v` span the map.
    c: Vec3,
    u: Vec3,
    v: Vec3,
    l: Vec3,
    half: f32,
    texel: f32,
    bias: f32,
    /// Neighbour lookups: `taps` on each side, `spread` texels apart at most.
    taps: i32,
    spread: f32,
}

/// A piece of geometry drawn into the map.
struct Piece<'a> {
    pts: Vec<Vec3>,
    tris: Cow<'a, [[u32; 3]]>,
    /// Texture coordinates and picture, only for pieces whose picture has see-through parts.
    cutout: Option<(&'a [Vertex], &'a Texture)>,
}

fn has_cutout(t: &Texture) -> bool {
    t.px.iter().any(|p| p[3] < 128)
}

/// A low-poly unit sphere (radius 1), used as the stand-in of an object in blob mode.
fn unit_sphere() -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let (seg, rings) = (10u32, 6u32);
    let mut pts = Vec::new();
    for r in 0..=rings {
        let phi = std::f32::consts::PI * r as f32 / rings as f32;
        for s in 0..seg {
            let th = std::f32::consts::TAU * s as f32 / seg as f32;
            pts.push(Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin()));
        }
    }
    let mut tris = Vec::new();
    for r in 0..rings {
        for s in 0..seg {
            let (a, b) = (r * seg + s, r * seg + (s + 1) % seg);
            let (c, d) = (a + seg, b + seg);
            tris.push([a, b, c]);
            tris.push([b, d, c]);
        }
    }
    (pts, tris)
}

impl ShadowMap {
    /// Draws the casters into a new shadow map. `light_dir` is the direction the light travels. `None` when nothing casts a shadow.
    pub fn build(casters: &[Caster], light_dir: Vec3, blob: bool, quality: ShadowQuality, softness: f32) -> Option<ShadowMap> {
        let l = light_dir.normalized();
        if !(l.x.is_finite() && l.y.is_finite() && l.z.is_finite()) || light_dir.length() < 1e-6 {
            return None;
        }
        let mut pieces: Vec<Piece> = Vec::new();
        let sphere = unit_sphere();
        for c in casters {
            let world: Vec<Vec<Vec3>> = c.pos.iter().map(|m| m.iter().map(|p| c.world.transform_point(*p)).collect()).collect();
            let mut it = world.iter().flatten();
            let Some(&first) = it.next() else { continue };
            let (lo, hi) = it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            if hi.y - lo.y < 0.05 {
                continue; // flat: a floor receives shadows but casts none
            }
            if blob {
                let (ctr, half) = ((lo + hi) * 0.5, (hi - lo) * 0.5 * 0.95);
                let pts = sphere.0.iter().map(|p| ctr + Vec3::new(p.x * half.x.max(0.05), p.y * half.y.max(0.05), p.z * half.z.max(0.05))).collect();
                pieces.push(Piece { pts, tris: Cow::Owned(sphere.1.clone()), cutout: None });
                continue;
            }
            for (mi, pts) in world.into_iter().enumerate() {
                let Some(mesh) = c.model.meshes.get(mi) else { continue };
                let tex = c.model.materials.get(mesh.material).and_then(|m| m.texture).and_then(|t| c.model.textures.get(t));
                let cutout = tex.filter(|t| has_cutout(t)).map(|t| (&mesh.verts[..], t));
                pieces.push(Piece { pts, tris: Cow::Borrowed(&mesh.tris[..]), cutout });
            }
        }
        if pieces.is_empty() {
            return None;
        }
        // the light's axes
        let up = if l.y.abs() > 0.99 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::Y };
        let u = up.cross(l).normalized();
        let v = l.cross(u).normalized();
        // extent of everything that casts, as seen from the light
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in pieces.iter().flat_map(|pc| pc.pts.iter()) {
            for (k, ax) in [u, v, l].iter().enumerate() {
                let d = p.dot(*ax);
                lo[k] = lo[k].min(d);
                hi[k] = hi[k].max(d);
            }
        }
        if !lo.iter().chain(hi.iter()).all(|x| x.is_finite()) {
            return None;
        }
        let res = if blob { 128 } else { quality.resolution() };
        let size = ((hi[0] - lo[0]).max(hi[1] - lo[1]) * (1.0 + 4.0 / res as f32)).max(0.01);
        let texel = size / res as f32;
        let (cu, cv) = ((lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5);
        let c = u * cu + v * cv; // the map's origin lies on the axis through the middle; depth is measured along `l` from the world origin
        let mut depth = vec![f32::INFINITY; res * res];
        let half = size * 0.5;
        for pc in &pieces {
            let proj: Vec<(f32, f32, f32)> = pc.pts.iter().map(|p| (((p.dot(u) - cu) + half) / texel, ((p.dot(v) - cv) + half) / texel, p.dot(l))).collect();
            for t in pc.tris.iter() {
                let (a, b, cc) = (t[0] as usize, t[1] as usize, t[2] as usize);
                let (Some(&pa), Some(&pb), Some(&pc3)) = (proj.get(a), proj.get(b), proj.get(cc)) else { continue };
                let cut = pc.cutout.and_then(|(verts, tex)| Some((verts.get(a)?.uv, verts.get(b)?.uv, verts.get(cc)?.uv, tex)));
                draw_depth(&mut depth, res, [pa, pb, pc3], cut);
            }
        }
        let softness = softness.clamp(0.0, 1.0);
        let (taps, spread) = if blob { (2, 2.5 + softness * 2.0) } else if softness > 0.3 { (2, 0.8 + softness * 3.0) } else { (1, 0.6 + softness * 2.0) };
        Some(ShadowMap { res, depth, c, u, v, l, half, texel, bias: texel * 2.5 + 0.01, taps, spread })
    }

    /// How much of the light is blocked at the world point `p`: 0 = fully lit, 1 = fully in shadow.
    pub fn occlusion(&self, p: Vec3) -> f32 {
        let (x, y, z) = (p.dot(self.u) - self.c.dot(self.u), p.dot(self.v) - self.c.dot(self.v), p.dot(self.l));
        let (tx, ty) = ((x + self.half) / self.texel, (y + self.half) / self.texel);
        let (k, s) = (self.taps, self.spread);
        let reach = s + 1.0;
        if tx < -reach || ty < -reach || tx > self.res as f32 + reach || ty > self.res as f32 + reach {
            return 0.0; // outside the map nothing casts a shadow
        }
        let (mut hit, mut total) = (0u32, 0u32);
        let lim = self.res as i32;
        for j in -k..=k {
            for i in -k..=k {
                total += 1;
                let (ix, iy) = ((tx + i as f32 * s / k as f32).floor() as i32, (ty + j as f32 * s / k as f32).floor() as i32);
                if ix < 0 || iy < 0 || ix >= lim || iy >= lim {
                    continue;
                }
                if z - self.bias > self.depth[iy as usize * self.res + ix as usize] {
                    hit += 1;
                }
            }
        }
        hit as f32 / total as f32
    }
}

/// Writes the nearest depth of one triangle (x and y in texels, z = distance from the light) into the map.
fn draw_depth(depth: &mut [f32], res: usize, t: [(f32, f32, f32); 3], cutout: Option<(Vec2, Vec2, Vec2, &Texture)>) {
    let [(x0, y0, z0), (x1, y1, z1), (x2, y2, z2)] = t;
    let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
    if area.abs() < 1e-9 || !area.is_finite() {
        return;
    }
    let lim = res as i32 - 1;
    let (minx, maxx) = (((x0.min(x1).min(x2) - 0.5).floor() as i32).max(0), ((x0.max(x1).max(x2) + 0.5).ceil() as i32).min(lim));
    let (miny, maxy) = (((y0.min(y1).min(y2) - 0.5).floor() as i32).max(0), ((y0.max(y1).max(y2) + 0.5).ceil() as i32).min(lim));
    let inv = 1.0 / area;
    let eps = -1e-4;
    for y in miny..=maxy {
        let py = y as f32 + 0.5;
        for x in minx..=maxx {
            let px = x as f32 + 0.5;
            let w0 = ((x1 - px) * (y2 - py) - (y1 - py) * (x2 - px)) * inv;
            let w1 = ((x2 - px) * (y0 - py) - (y2 - py) * (x0 - px)) * inv;
            let w2 = 1.0 - w0 - w1;
            if w0 < eps || w1 < eps || w2 < eps {
                continue;
            }
            if let Some((a, b, c, tex)) = &cutout {
                let (u, v) = (w0 * a.x + w1 * b.x + w2 * c.x, w0 * a.y + w1 * b.y + w2 * c.y);
                let (tx, ty) = (((u * tex.w as f32).floor() as i32).rem_euclid(tex.w as i32), ((v * tex.h as f32).floor() as i32).rem_euclid(tex.h as i32));
                if tex.get(tx, ty).is_some_and(|p| p[3] < 128) {
                    continue;
                }
            }
            let z = w0 * z0 + w1 * z1 + w2 * z2;
            let d = &mut depth[y as usize * res + x as usize];
            if z < *d {
                *d = z;
            }
        }
    }
}

/// Everything the renderer needs to apply a shadow map to a finished picture.
pub struct ShadowPass<'a> {
    map: &'a ShadowMap,
    strength: f32,
    inv_vp: Mat4,
    view: Mat4,
    fog: Option<(f32, f32)>,
}

impl<'a> ShadowPass<'a> {
    pub fn new(map: &'a ShadowMap, strength: f32, vp: &Mat4, view: &Mat4, fog: Option<(f32, f32)>, _w: u32, _h: u32) -> Option<ShadowPass<'a>> {
        let strength = strength.clamp(0.0, 1.0);
        if strength <= 0.0 {
            return None;
        }
        Some(ShadowPass { map, strength, inv_vp: vp.inverse()?, view: *view, fog })
    }

    /// Darkens the pixels of some rows (starting at row `y0`) that lie in shadow. Only pixels the model drew are touched.
    pub fn apply(&self, color: &mut [[u8; 4]], depth: &[f32], tri_at: &[u32], y0: i32, w: u32, h: u32, qlut: &[u8; 256], lit_of: &dyn Fn(u32) -> f32) {
        let wu = w as usize;
        for (i, px) in color.iter_mut().enumerate() {
            let z = depth[i];
            if tri_at[i] == crate::raster::NO_TRI || !z.is_finite() {
                continue;
            }
            let (x, y) = ((i % wu) as f32, y0 as f32 + (i / wu) as f32);
            let ndc = [(x + 0.5) / w as f32 * 2.0 - 1.0, 1.0 - (y + 0.5) / h as f32 * 2.0, z, 1.0];
            let o = self.inv_vp.mul_vec4(ndc);
            if o[3].abs() < 1e-12 {
                continue;
            }
            let p = Vec3::new(o[0] / o[3], o[1] / o[3], o[2] / o[3]);
            // a surface turned away from the light is already dark: its shadow fades out instead of darkening it twice
            let t = ((lit_of(tri_at[i]) - 0.02) / 0.28).clamp(0.0, 1.0);
            let facing = t * t * (3.0 - 2.0 * t);
            if facing <= 0.0 {
                continue;
            }
            let occ = self.map.occlusion(p) * facing;
            if occ <= 0.0 {
                continue;
            }
            // far away things melt into the fog; their shadows go with them
            let fade = match self.fog {
                Some((near, far)) => 1.0 - ((-self.view.transform_point(p).z - near) / (far - near).max(1e-3)).clamp(0.0, 1.0),
                None => 1.0,
            };
            let keep = 1.0 - self.strength * occ * fade;
            for ch in 0..3 {
                px[ch] = qlut[((px[ch] as f32 * keep) + 0.5).clamp(0.0, 255.0) as usize];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modeling::{cube, model_from_mesh, plane};

    fn caster<'a>(m: &'a Model, pos: &'a Vec<Vec<Vec3>>, at: Vec3) -> Caster<'a> {
        Caster { model: m, pos: &pos[..], world: Mat4::translation(at) }
    }
    fn positions(m: &Model) -> Vec<Vec<Vec3>> {
        m.meshes.iter().map(|me| me.verts.iter().map(|v| v.pos).collect()).collect()
    }

    #[test]
    fn a_box_shadows_the_points_behind_it_at_any_height_and_nothing_else() {
        let m = model_from_mesh("box", cube(Vec3::new(2.0, 2.0, 2.0)), 8);
        let pos = positions(&m);
        // light travels to +x and down; the box floats at y = 3
        let l = Vec3::new(1.0, -1.0, 0.0);
        let map = ShadowMap::build(&[caster(&m, &pos, Vec3::new(0.0, 3.0, 0.0))], l, false, ShadowQuality::Medium, 0.3).expect("a map");
        // the shadow is the box's footprint moved along the light: centred at x = 3, wherever the ground is
        for ground in [0.0f32, -2.0] {
            let c = Vec3::new(3.0 - ground, ground, 0.0) + Vec3::new(0.0, 0.0, 0.0);
            let inside = map.occlusion(Vec3::new(3.0 + (0.0 - ground), ground, 0.0));
            assert!(inside > 0.95, "ground {ground}: centre of the shadow is dark, got {inside} at {c:?}");
        }
        assert!(map.occlusion(Vec3::new(-4.0, 0.0, 0.0)) < 0.01, "left of the box is lit");
        assert!(map.occlusion(Vec3::new(3.0, 0.0, 6.0)) < 0.01, "beside the shadow is lit");
        assert!(map.occlusion(Vec3::new(0.0, 8.0, 0.0)) < 0.01, "above the box (nearer to the light) is lit");
    }

    #[test]
    fn a_cut_out_texture_casts_only_its_solid_part() {
        let mut m = model_from_mesh("card", plane(2.0, 2.0, 1, 1), 16);
        for y in 0..16 {
            for x in 0..16 {
                m.textures[0].set(x, y, if x < 8 { [0, 0, 0, 0] } else { [200, 40, 40, 255] });
            }
        }
        let pos = positions(&m);
        let map = ShadowMap::build(&[Caster { model: &m, pos: &pos[..], world: Mat4::translation(Vec3::new(0.0, 0.0, 0.0)) }], Vec3::new(0.0, -1.0, 0.0), false, ShadowQuality::High, 0.0);
        // a single flat card is "flat": it is a floor, not a caster - tilt it so it counts as an object
        assert!(map.is_none(), "flat objects cast nothing");
        let tilt = Mat4::from_quat(Quat::from_euler(Vec3::new(0.9, 0.0, 0.0)));
        let map = ShadowMap::build(&[Caster { model: &m, pos: &pos[..], world: tilt }], Vec3::new(0.0, -1.0, 0.0), false, ShadowQuality::High, 0.0).expect("a map");
        let (a, b) = (map.occlusion(Vec3::new(-0.5, -3.0, 0.0)), map.occlusion(Vec3::new(0.5, -3.0, 0.0)));
        assert!((a > 0.9 && b < 0.1) || (b > 0.9 && a < 0.1), "exactly one half casts a shadow: {a} {b}");
    }
}
