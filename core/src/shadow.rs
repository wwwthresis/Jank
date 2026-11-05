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
//!
//! Speed (the results are bit-for-bit the same as the straightforward version):
//! * Maps are cached by a fingerprint of everything that goes into them ([`ShadowMap::build_cached`]). Orbiting the camera,
//!   painting or drawing the preview reuses the map; it is redrawn only when an object, the pose or the light changes.
//! * The depth picture is drawn by several threads, each owning a band of rows; the buffer of an evicted map is reused.
//! * Every 8x8 block of texels keeps its nearest and farthest depth. A pixel in front of every block its soft-edge taps touch
//!   is lit, one behind all of them is fully shadowed: one to four lookups instead of up to 25. Only real penumbra pixels
//!   read the taps, and their rows and columns are computed once instead of per tap.
use crate::lighting::ShadowQuality;
use crate::math::*;
use crate::mesh::*;
use crate::texture::Texture;
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

/// Side of a min/max block in texels (as a shift).
const TILE_SHIFT: usize = 3;
/// Most taps per side (`taps` is at most 2, so 2 * 2 + 1).
const MAX_TAPS: usize = 5;
/// How many recent maps are kept: the viewport, the render preview and an onion-skin frame each may need their own.
const CACHE_SLOTS: usize = 3;

/// One object that may cast a shadow: its model, the posed vertex positions (model space) and where it stands.
pub struct Caster<'a> {
    pub model: &'a Model,
    pub pos: &'a [Vec<Vec3>],
    pub world: Mat4,
}

/// The shadow map plus how strongly it darkens; handed to the renderer for one frame.
pub struct ShadowInput {
    pub map: Arc<ShadowMap>,
    /// 0 = invisible, 1 = black.
    pub strength: f32,
}

pub struct ShadowMap {
    res: usize,
    /// Distance from the light of the nearest surface per texel (INFINITY = nothing there).
    depth: Vec<f32>,
    /// The light's axes: `l` is the direction the light travels, `u` and `v` span the map.
    u: Vec3,
    v: Vec3,
    l: Vec3,
    half: f32,
    texel: f32,
    bias: f32,
    /// Neighbour lookups: `taps` on each side, `spread` texels apart at most.
    taps: i32,
    spread: f32,
    /// Centre of the map measured along `u` and `v`.
    cu: f32,
    cv: f32,
    /// Blocks per side, and per block the nearest / farthest depth.
    tiles: usize,
    tmin: Vec<f32>,
    tmax: Vec<f32>,
    /// The same, widened to the block's 8 neighbours: a first one-lookup test around the centre texel.
    nmin: Vec<f32>,
    nmax: Vec<f32>,
    /// Texels the taps may reach away from the centre texel (smaller than a block, so the neighbours cover them).
    reach: i32,
}

/// A small fast hasher (the FxHash mix) for the cache fingerprint; not for anything security related.
struct Fp(u64);
impl Fp {
    #[inline]
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn f(&mut self, v: f32) {
        self.add(v.to_bits() as u64);
    }
    fn v3(&mut self, v: Vec3) {
        self.add(((v.x.to_bits() as u64) << 32) | v.y.to_bits() as u64);
        self.f(v.z);
    }
}

static CACHE: Mutex<Vec<(u64, Arc<ShadowMap>)>> = Mutex::new(Vec::new());

/// Fingerprint of everything [`ShadowMap::build`] reads.
fn fingerprint(casters: &[Caster], light_dir: Vec3, blob: bool, quality: ShadowQuality, softness: f32) -> u64 {
    let mut h = Fp(0x9e37_79b9_7f4a_7c15);
    h.v3(light_dir);
    h.add(blob as u64);
    h.add(quality.id() as u64);
    h.f(softness);
    h.add(casters.len() as u64);
    for c in casters {
        for row in c.world.m {
            for v in row {
                h.f(v);
            }
        }
        h.add(c.pos.len() as u64);
        for m in c.pos {
            h.add(m.len() as u64);
            for p in m {
                h.v3(*p);
            }
        }
        if blob {
            continue; // blobs depend only on the bounds of the positions
        }
        for mesh in &c.model.meshes {
            h.add(mesh.tris.len() as u64);
            for t in &mesh.tris {
                h.add(((t[0] as u64) << 32) | t[1] as u64);
                h.add(t[2] as u64);
            }
            let tex = c.model.materials.get(mesh.material).and_then(|m| m.texture).and_then(|t| c.model.textures.get(t));
            match tex {
                Some(t) => {
                    h.add(((t.w as u64) << 32) | t.h as u64);
                    // only "see-through or not" matters to the map; pack 64 texels per word
                    let mut word = 0u64;
                    for (i, p) in t.px.iter().enumerate() {
                        word = (word << 1) | (p[3] < 128) as u64;
                        if i % 64 == 63 {
                            h.add(word);
                            word = 0;
                        }
                    }
                    h.add(word);
                    for v in &mesh.verts {
                        h.add(((v.uv.x.to_bits() as u64) << 32) | v.uv.y.to_bits() as u64);
                    }
                }
                None => h.add(0),
            }
        }
    }
    h.0
}

fn worker_threads() -> usize {
    crate::raster::default_threads()
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
    /// Like [`ShadowMap::build`], but returns a recent identical map when nothing that affects it has changed.
    pub fn build_cached(casters: &[Caster], light_dir: Vec3, blob: bool, quality: ShadowQuality, softness: f32) -> Option<Arc<ShadowMap>> {
        let key = fingerprint(casters, light_dir, blob, quality, softness);
        let mut spare = Vec::new();
        if let Ok(mut cache) = CACHE.lock() {
            if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
                let hit = cache.remove(i);
                let map = hit.1.clone();
                cache.insert(0, hit);
                return Some(map);
            }
            if cache.len() >= CACHE_SLOTS {
                // reuse the oldest map's memory when no frame holds it any more
                if let Some((_, old)) = cache.pop() {
                    if let Ok(m) = Arc::try_unwrap(old) {
                        spare = m.depth;
                    }
                }
            }
        }
        let map = Arc::new(Self::build_with(casters, light_dir, blob, quality, softness, spare)?);
        if let Ok(mut cache) = CACHE.lock() {
            cache.insert(0, (key, map.clone()));
            cache.truncate(CACHE_SLOTS);
        }
        Some(map)
    }

    /// Draws the casters into a new shadow map. `light_dir` is the direction the light travels. `None` when nothing casts a shadow.
    pub fn build(casters: &[Caster], light_dir: Vec3, blob: bool, quality: ShadowQuality, softness: f32) -> Option<ShadowMap> {
        Self::build_with(casters, light_dir, blob, quality, softness, Vec::new())
    }

    fn build_with(casters: &[Caster], light_dir: Vec3, blob: bool, quality: ShadowQuality, softness: f32, mut depth: Vec<f32>) -> Option<ShadowMap> {
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
        depth.resize(res * res, f32::INFINITY); // every band clears its own rows below
        let half = size * 0.5;
        // project every triangle once; the bands below only clip them to their rows
        let mut tris: Vec<DepthTri> = Vec::new();
        for pc in &pieces {
            let proj: Vec<(f32, f32, f32)> = pc.pts.iter().map(|p| (((p.dot(u) - cu) + half) / texel, ((p.dot(v) - cv) + half) / texel, p.dot(l))).collect();
            for t in pc.tris.iter() {
                let (a, b, cc) = (t[0] as usize, t[1] as usize, t[2] as usize);
                let (Some(&pa), Some(&pb), Some(&pc3)) = (proj.get(a), proj.get(b), proj.get(cc)) else { continue };
                let cut = pc.cutout.and_then(|(verts, tex)| Some((verts.get(a)?.uv, verts.get(b)?.uv, verts.get(cc)?.uv, tex)));
                tris.push(([pa, pb, pc3], cut));
            }
        }
        let tsize = 1usize << TILE_SHIFT;
        let tiles = res.div_ceil(tsize);
        let (mut tmin, mut tmax) = (vec![f32::INFINITY; tiles * tiles], vec![f32::NEG_INFINITY; tiles * tiles]);
        let threads = if res >= 512 { worker_threads() } else { 1 };
        // bands of whole block rows, so every band also owns its rows of the block tables
        let rows = res.div_ceil(threads).div_ceil(tsize) * tsize;
        let job = |bi: usize, band: &mut [f32], lo: &mut [f32], hi: &mut [f32]| {
            band.fill(f32::INFINITY);
            for (t, cut) in &tris {
                draw_depth(band, res, bi * rows, *t, *cut);
            }
            block_min_max(band, res, lo, hi);
        };
        let per_band = rows / tsize * tiles;
        if threads <= 1 {
            job(0, &mut depth, &mut tmin, &mut tmax);
        } else {
            std::thread::scope(|sc| {
                for (bi, ((band, lo), hi)) in depth.chunks_mut(rows * res).zip(tmin.chunks_mut(per_band)).zip(tmax.chunks_mut(per_band)).enumerate() {
                    let job = &job;
                    sc.spawn(move || job(bi, band, lo, hi));
                }
            });
        }
        let softness = softness.clamp(0.0, 1.0);
        let (taps, spread) = if blob { (2, 2.5 + softness * 2.0) } else if softness > 0.3 { (2, 0.8 + softness * 3.0) } else { (1, 0.6 + softness * 2.0) };
        let (nmin, nmax) = widen(&tmin, &tmax, tiles);
        let reach = spread.ceil() as i32 + 1;
        debug_assert!(reach < 1 << TILE_SHIFT);
        Some(ShadowMap { res, depth, u, v, l, half, texel, bias: texel * 2.5 + 0.01, taps, spread, cu: c.dot(u), cv: c.dot(v), tiles, tmin, tmax, nmin, nmax, reach })
    }

    /// How much of the light is blocked at the world point `p`: 0 = fully lit, 1 = fully in shadow.
    pub fn occlusion(&self, p: Vec3) -> f32 {
        let (x, y, z) = (p.dot(self.u) - self.cu, p.dot(self.v) - self.cv, p.dot(self.l));
        let (tx, ty) = ((x + self.half) / self.texel, (y + self.half) / self.texel);
        let (k, s) = (self.taps, self.spread);
        let reach = s + 1.0;
        if tx < -reach || ty < -reach || tx > self.res as f32 + reach || ty > self.res as f32 + reach {
            return 0.0; // outside the map nothing casts a shadow
        }
        let lim = self.res as i32;
        let zb = z - self.bias;
        // quick test around the centre texel: every tap lies within `reach` texels of it, so inside the block's neighbourhood
        let (cx, cy) = (ifloor(tx), ifloor(ty));
        if cx >= 0 && cy >= 0 && cx < lim && cy < lim {
            let ti = (cy as usize >> TILE_SHIFT) * self.tiles + (cx as usize >> TILE_SHIFT);
            if !(zb > self.nmin[ti]) {
                return 0.0;
            }
            let r = self.reach;
            if zb > self.nmax[ti] && cx >= r && cy >= r && cx + r < lim && cy + r < lim {
                return 1.0;
            }
        }
        // the tap columns and rows (the same arithmetic as one tap at a time, so the same texels)
        let n = (2 * k + 1) as usize;
        let (mut xs, mut ys) = ([0i32; MAX_TAPS], [0i32; MAX_TAPS]);
        for (idx, i) in (-k..=k).enumerate().take(MAX_TAPS) {
            let o = i as f32 * s / k as f32;
            xs[idx] = ifloor(tx + o);
            ys[idx] = ifloor(ty + o);
        }
        let total = (n * n) as u32;
        // taps outside the map count but never hit; the offsets only grow, so the inside ones are a range
        let (x0, x1) = (xs[..n].iter().position(|&x| x >= 0), xs[..n].iter().rposition(|&x| x < lim));
        let (y0, y1) = (ys[..n].iter().position(|&y| y >= 0), ys[..n].iter().rposition(|&y| y < lim));
        let (Some(x0), Some(x1), Some(y0), Some(y1)) = (x0, x1, y0, y1) else { return 0.0 };
        if x0 > x1 || y0 > y1 {
            return 0.0;
        }
        // block test over exactly the blocks the taps touch
        let (bx0, bx1) = (xs[x0] as usize >> TILE_SHIFT, xs[x1] as usize >> TILE_SHIFT);
        let (by0, by1) = (ys[y0] as usize >> TILE_SHIFT, ys[y1] as usize >> TILE_SHIFT);
        let (mut near, mut far) = (f32::INFINITY, f32::NEG_INFINITY);
        for by in by0..=by1 {
            for bx in bx0..=bx1 {
                near = near.min(self.tmin[by * self.tiles + bx]);
                far = far.max(self.tmax[by * self.tiles + bx]);
            }
        }
        if !(zb > near) {
            return 0.0; // in front of everything the taps can see
        }
        let all_inside = x0 == 0 && y0 == 0 && x1 == n - 1 && y1 == n - 1;
        if all_inside && zb > far {
            return 1.0; // behind everything, and no tap falls off the map
        }
        let mut hit = 0u32;
        for &iy in &ys[y0..=y1] {
            let row = &self.depth[iy as usize * self.res..(iy as usize + 1) * self.res];
            for &ix in &xs[x0..=x1] {
                hit += (zb > row[ix as usize]) as u32;
            }
        }
        hit as f32 / total as f32
    }
}

/// A projected triangle (x and y in texels, z = distance from the light) and, for cut-out pictures, its UVs and picture.
type DepthTri<'a> = ([(f32, f32, f32); 3], Option<(Vec2, Vec2, Vec2, &'a Texture)>);

/// Block tables widened to each block's 8 neighbours.
fn widen(lo: &[f32], hi: &[f32], n: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut wlo, mut whi) = (vec![f32::INFINITY; n * n], vec![f32::NEG_INFINITY; n * n]);
    for y in 0..n {
        for x in 0..n {
            for yy in y.saturating_sub(1)..=(y + 1).min(n - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(n - 1) {
                    wlo[y * n + x] = wlo[y * n + x].min(lo[yy * n + xx]);
                    whi[y * n + x] = whi[y * n + x].max(hi[yy * n + xx]);
                }
            }
        }
    }
    (wlo, whi)
}

/// Nearest and farthest depth of every block in some rows of the map (`band` starts on a block row; `lo` / `hi` are its block rows).
fn block_min_max(band: &[f32], res: usize, lo: &mut [f32], hi: &mut [f32]) {
    let t = 1usize << TILE_SHIFT;
    let n = res.div_ceil(t);
    for (y, row) in band.chunks(res).enumerate() {
        let base = (y >> TILE_SHIFT) * n;
        for (bx, block) in row.chunks(t).enumerate() {
            let (a, b) = block.iter().fold((lo[base + bx], hi[base + bx]), |(a, b), &d| (a.min(d), b.max(d)));
            lo[base + bx] = a;
            hi[base + bx] = b;
        }
    }
}

/// Writes the nearest depth of one triangle into the rows of the map held by `depth`, which start at row `row0`.
fn draw_depth(depth: &mut [f32], res: usize, row0: usize, t: [(f32, f32, f32); 3], cutout: Option<(Vec2, Vec2, Vec2, &Texture)>) {
    let [(x0, y0, z0), (x1, y1, z1), (x2, y2, z2)] = t;
    let area = (x1 - x0) * (y2 - y0) - (y1 - y0) * (x2 - x0);
    if area.abs() < 1e-9 || !area.is_finite() {
        return;
    }
    let lim = res as i32 - 1;
    let (band_lo, band_hi) = (row0 as i32, (row0 + depth.len() / res) as i32 - 1);
    let (minx, maxx) = (((x0.min(x1).min(x2) - 0.5).floor() as i32).max(0), ((x0.max(x1).max(x2) + 0.5).ceil() as i32).min(lim));
    let (miny, maxy) = (((y0.min(y1).min(y2) - 0.5).floor() as i32).max(band_lo), ((y0.max(y1).max(y2) + 0.5).ceil() as i32).min(lim).min(band_hi));
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
                let (tx, ty) = ((ifloor(u * tex.w as f32)).rem_euclid(tex.w as i32), ((v * tex.h as f32).floor() as i32).rem_euclid(tex.h as i32));
                if tex.get(tx, ty).is_some_and(|p| p[3] < 128) {
                    continue;
                }
            }
            let z = w0 * z0 + w1 * z1 + w2 * z2;
            let d = &mut depth[(y as usize - row0) * res + x as usize];
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
    pub fn apply(&self, color: &mut [[u8; 4]], depth: &[f32], tri_at: &[u32], y0: i32, w: u32, h: u32, qlut: &[u8; 256], lit_of: impl Fn(u32) -> f32) {
        let wu = w as usize;
        if wu == 0 {
            return;
        }
        // screen x in NDC per column, worked out once per band with the same arithmetic as per pixel
        let ndc_x: Vec<f32> = (0..wu).map(|x| (x as f32 + 0.5) / w as f32 * 2.0 - 1.0).collect();
        for (r, ((crow, drow), trow)) in color.chunks_mut(wu).zip(depth.chunks(wu)).zip(tri_at.chunks(wu)).enumerate() {
            let y = y0 as f32 + r as f32;
            let ndc_y = 1.0 - (y + 0.5) / h as f32 * 2.0;
            for (x, px) in crow.iter_mut().enumerate() {
                let (z, ti) = (drow[x], trow[x]);
                if ti == crate::raster::NO_TRI || !z.is_finite() {
                    continue;
                }
                // a surface turned away from the light is already dark: its shadow fades out instead of darkening it twice
                let t = ((lit_of(ti) - 0.02) / 0.28).clamp(0.0, 1.0);
                let facing = t * t * (3.0 - 2.0 * t);
                if facing <= 0.0 {
                    continue;
                }
                let o = self.inv_vp.mul_vec4([ndc_x[x], ndc_y, z, 1.0]);
                if o[3].abs() < 1e-12 {
                    continue;
                }
                let p = Vec3::new(o[0] / o[3], o[1] / o[3], o[2] / o[3]);
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

    /// The original lookup, one tap at a time, without the block tables.
    fn reference(m: &ShadowMap, p: Vec3) -> f32 {
        let c = m.u * m.cu + m.v * m.cv;
        let (x, y, z) = (p.dot(m.u) - c.dot(m.u), p.dot(m.v) - c.dot(m.v), p.dot(m.l));
        let (tx, ty) = ((x + m.half) / m.texel, (y + m.half) / m.texel);
        let (k, s) = (m.taps, m.spread);
        let reach = s + 1.0;
        if tx < -reach || ty < -reach || tx > m.res as f32 + reach || ty > m.res as f32 + reach {
            return 0.0;
        }
        let (mut hit, mut total) = (0u32, 0u32);
        let lim = m.res as i32;
        for j in -k..=k {
            for i in -k..=k {
                total += 1;
                let (ix, iy) = ((tx + i as f32 * s / k as f32).floor() as i32, (ty + j as f32 * s / k as f32).floor() as i32);
                if ix < 0 || iy < 0 || ix >= lim || iy >= lim {
                    continue;
                }
                if z - m.bias > m.depth[iy as usize * m.res + ix as usize] {
                    hit += 1;
                }
            }
        }
        hit as f32 / total as f32
    }

    #[test]
    fn fast_lookups_and_the_cache_match_the_tap_by_tap_lookup_exactly() {
        let m = model_from_mesh("box", cube(Vec3::new(1.5, 2.0, 1.0)), 8);
        let pos = positions(&m);
        let tilt = Mat4::translation(Vec3::new(0.3, 2.0, -0.2)) * (Mat4::from_quat(Quat::from_euler(Vec3::new(0.4, 0.7, 0.1))));
        let casters = [Caster { model: &m, pos: &pos[..], world: tilt }];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / (1u64 << 24) as f32
        };
        for blob in [false, true] {
            for q in ShadowQuality::ALL {
                for soft in [0.0f32, 0.25, 0.6, 1.0] {
                    let map = ShadowMap::build(&casters, Vec3::new(0.6, -1.0, 0.3), blob, q, soft).expect("a map");
                    for _ in 0..4000 {
                        let p = Vec3::new(rnd() * 8.0 - 3.0, rnd() * 5.0 - 1.5, rnd() * 8.0 - 4.0);
                        assert_eq!(map.occlusion(p).to_bits(), reference(&map, p).to_bits(), "blob {blob} {q:?} softness {soft} at {p:?}");
                    }
                    let get = || ShadowMap::build_cached(&casters, Vec3::new(0.6, -1.0, 0.3), blob, q, soft).expect("a map");
                    let a = get();
                    assert_eq!(a.depth, map.depth, "the cached map is the same picture");
                    // other tests share the cache and may evict between two calls, so allow a retry
                    assert!((0..3).any(|_| Arc::ptr_eq(&get(), &get())), "an unchanged scene reuses its map");
                }
            }
        }
        let moved = [Caster { model: &m, pos: &pos[..], world: Mat4::translation(Vec3::new(0.0, 3.0, 0.0)) }];
        let a = ShadowMap::build_cached(&casters, Vec3::new(0.6, -1.0, 0.3), false, ShadowQuality::Low, 0.5).unwrap();
        let b = ShadowMap::build_cached(&moved, Vec3::new(0.6, -1.0, 0.3), false, ShadowQuality::Low, 0.5).unwrap();
        assert!(!Arc::ptr_eq(&a, &b), "a moved object gets a new map");
    }
}
