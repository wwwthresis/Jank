//! Software PSX-style rasterizer.
//! Reproduces the characteristic limitations of PS1-era rendering: vertex snapping to whole pixels (jitter), affine (not perspective-correct)
//! texture mapping so that textures "swim", painter's-algorithm ordering by mean triangle depth instead of a z-buffer, 4x4 ordered (Bayer) dithering,
//! 15-bit colour, per-vertex Gouraud lighting and distance fog. Also provides mood post-processing.
use crate::math::*;
use crate::mesh::*;
use crate::pose::*;
use crate::texture::Texture;

// ------------------------------------------------------------------ camera

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    /// Vertical field of view in radians.
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self { eye: Vec3::new(0.0, 1.0, 4.0), target: Vec3::ZERO, up: Vec3::Y, fov_y: 0.9, near: 0.05, far: 200.0 }
    }
}
impl Camera {
    /// Orbit camera: yaw/pitch in radians, `dist` is the distance to the target.
    pub fn orbit(target: Vec3, yaw: f32, pitch: f32, dist: f32) -> Camera {
        let p = pitch.clamp(-1.553, 1.553);
        let eye = target + Vec3::new(yaw.sin() * p.cos(), p.sin(), yaw.cos() * p.cos()) * dist.max(0.01);
        Camera { eye, target, ..Default::default() }
    }
    pub fn view(&self) -> Mat4 {
        Mat4::look_at(self.eye, self.target, self.up)
    }
    pub fn proj(&self, aspect: f32) -> Mat4 {
        Mat4::perspective(self.fov_y, aspect.max(1e-3), self.near, self.far)
    }
    /// World point -> (screen x, screen y, depth along the view direction). `None` if the point is behind the camera.
    pub fn project(&self, p: Vec3, w: u32, h: u32) -> Option<(f32, f32, f32)> {
        self.project_vp(&self.view_proj(w, h), p, w, h)
    }
    /// The matrix [`Camera::project`] uses for a `w` x `h` picture. Build it once to project many points.
    pub fn view_proj(&self, w: u32, h: u32) -> Mat4 {
        self.proj(w as f32 / h as f32) * self.view()
    }
    /// [`Camera::project`] with a matrix from [`Camera::view_proj`] (same result, no matrix work per point).
    #[inline]
    pub fn project_vp(&self, vp: &Mat4, p: Vec3, w: u32, h: u32) -> Option<(f32, f32, f32)> {
        let c = vp.mul_vec4([p.x, p.y, p.z, 1.0]);
        if c[3] <= self.near * 0.5 {
            return None;
        }
        Some(((c[0] / c[3] * 0.5 + 0.5) * w as f32, (0.5 - c[1] / c[3] * 0.5) * h as f32, c[3]))
    }
    /// Ray through a screen pixel, returned as (origin, direction).
    pub fn ray(&self, sx: f32, sy: f32, w: u32, h: u32) -> (Vec3, Vec3) {
        let inv = (self.proj(w as f32 / h as f32) * self.view()).inverse().unwrap_or(Mat4::IDENTITY);
        let (nx, ny) = (sx / w as f32 * 2.0 - 1.0, 1.0 - sy / h as f32 * 2.0);
        let a = inv.mul_vec4([nx, ny, -1.0, 1.0]);
        let b = inv.mul_vec4([nx, ny, 1.0, 1.0]);
        let (pa, pb) = (Vec3::new(a[0] / a[3], a[1] / a[3], a[2] / a[3]), Vec3::new(b[0] / b[3], b[1] / b[3], b[2] / b[3]));
        (self.eye, (pb - pa).normalized())
    }
}

// ------------------------------------------------------------------ style

#[derive(Clone, Debug, PartialEq)]
pub struct PsxStyle {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Vertex snapping step in screen pixels. 0 = off, 1 = PS1-like.
    pub snap: f32,
    pub affine: bool,
    /// false = PS1-like: triangles are ordered by mean depth and no z-buffer is used.
    pub z_buffer: bool,
    pub dither: bool,
    /// Bits per channel (5 = PS1 15-bit colour). 8 = no quantization.
    pub color_bits: u32,
    pub fog_on: bool,
    pub fog_near: f32,
    pub fog_far: f32,
    pub fog_color: [f32; 3],
    pub ambient: [f32; 3],
    pub light_dir: Vec3,
    pub light_color: [f32; 3],
    pub bg_top: [f32; 3],
    pub bg_bottom: [f32; 3],
    // mood / post-processing
    pub tint: [f32; 3],
    pub saturation: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub vignette: f32,
    pub scanlines: f32,
    pub grain: f32,
}
impl Default for PsxStyle {
    fn default() -> Self {
        Self::ps1_classic()
    }
}
impl PsxStyle {
    fn base(name: &str) -> Self {
        Self {
            name: name.into(),
            width: 320,
            height: 240,
            snap: 1.0,
            affine: true,
            z_buffer: false,
            dither: true,
            color_bits: 5,
            fog_on: true,
            fog_near: 6.0,
            fog_far: 22.0,
            fog_color: [0.30, 0.32, 0.40],
            ambient: [0.45, 0.45, 0.50],
            light_dir: Vec3::new(-0.4, -0.8, -0.5).normalized(),
            light_color: [0.75, 0.72, 0.65],
            bg_top: [0.30, 0.32, 0.40],
            bg_bottom: [0.30, 0.32, 0.40],
            tint: [1.0; 3],
            saturation: 1.0,
            contrast: 1.0,
            brightness: 1.0,
            vignette: 0.0,
            scanlines: 0.0,
            grain: 0.0,
        }
    }
    pub fn ps1_classic() -> Self {
        Self::base("PS1 Classic")
    }
    /// Calm Lo-Fi: warm haze, soft evening gradient, film grain.
    pub fn lofi_calm() -> Self {
        Self {
            width: 384,
            height: 216,
            fog_near: 4.0,
            fog_far: 16.0,
            fog_color: [0.55, 0.42, 0.55],
            ambient: [0.58, 0.50, 0.60],
            light_color: [0.80, 0.62, 0.50],
            bg_top: [0.22, 0.20, 0.38],
            bg_bottom: [0.85, 0.52, 0.52],
            tint: [1.04, 0.97, 1.02],
            saturation: 0.85,
            contrast: 0.95,
            vignette: 0.45,
            scanlines: 0.12,
            grain: 0.05,
            ..Self::base("Lo-Fi Calm")
        }
    }
    /// Crisp low-poly without PSX artifacts: sharp facets, full colour.
    pub fn clean_lowpoly() -> Self {
        Self {
            width: 640,
            height: 360,
            snap: 0.0,
            affine: false,
            z_buffer: true,
            dither: false,
            color_bits: 8,
            fog_on: false,
            bg_top: [0.55, 0.75, 0.95],
            bg_bottom: [0.90, 0.95, 1.0],
            ..Self::base("Clean Low-Poly")
        }
    }
    pub fn horror_fog() -> Self {
        Self {
            fog_near: 2.0,
            fog_far: 9.0,
            fog_color: [0.05, 0.07, 0.06],
            ambient: [0.25, 0.30, 0.28],
            light_color: [0.50, 0.55, 0.45],
            bg_top: [0.02, 0.03, 0.03],
            bg_bottom: [0.05, 0.07, 0.06],
            saturation: 0.6,
            vignette: 0.6,
            grain: 0.08,
            ..Self::base("Horror Fog")
        }
    }
    pub fn presets() -> Vec<PsxStyle> {
        vec![Self::ps1_classic(), Self::lofi_calm(), Self::clean_lowpoly(), Self::horror_fog()]
    }
}

// ------------------------------------------------------------------ frame

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// 0xFFFF = empty.
    pub obj: u16,
    pub mesh: u16,
    pub tri: u32,
    /// Perspective-correct UV (v=0 at the top), used for painting directly on the model.
    pub u: f32,
    pub v: f32,
}
impl Hit {
    pub const NONE: Hit = Hit { obj: 0xFFFF, mesh: 0, tri: 0, u: 0.0, v: 0.0 };
    pub fn is_some(&self) -> bool {
        self.obj != 0xFFFF
    }
}

/// Marks "no triangle" in the per-pixel triangle buffer.
pub const NO_TRI: u32 = u32::MAX;

pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub color: Vec<[u8; 4]>,
    pub depth: Vec<f32>,
    /// Per pixel: index into `tris` of the triangle that won the pixel (or `NO_TRI`).
    /// Only 4 bytes per pixel are written while drawing; picking data is derived on demand by `hit_at`.
    pub tri_at: Vec<u32>,
    tris: Vec<Tri>,
}
impl Frame {
    pub fn new(w: u32, h: u32) -> Self {
        let n = (w * h) as usize;
        Self { w, h, color: vec![[0, 0, 0, 255]; n], depth: vec![f32::INFINITY; n], tri_at: vec![NO_TRI; n], tris: Vec::new() }
    }
    /// What is under pixel (x, y): object, mesh, triangle and the perspective-correct UV (computed here, not per pixel while drawing).
    pub fn hit_at(&self, x: i32, y: i32) -> Hit {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return Hit::NONE;
        }
        let ti = self.tri_at[(y as u32 * self.w + x as u32) as usize];
        let Some(t) = self.tris.get(ti as usize) else { return Hit::NONE };
        let [v0, v1, v2] = t.v;
        let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
        let area = (v1.x - v0.x) * (v2.y - v0.y) - (v1.y - v0.y) * (v2.x - v0.x);
        let e = |a: &SV, b: &SV| (b.x - a.x) * (cy - a.y) - (b.y - a.y) * (cx - a.x);
        let (l0, l1, l2) = ((e(&v1, &v2) / area) as f32, (e(&v2, &v0) / area) as f32, (e(&v0, &v1) / area) as f32);
        let (p0, p1, p2) = (l0 * v0.iw, l1 * v1.iw, l2 * v2.iw);
        let den = (p0 + p1 + p2).max(1e-20);
        let u = (p0 * v0.uv[0] + p1 * v1.uv[0] + p2 * v2.uv[0]) / den;
        let v = (p0 * v0.uv[1] + p1 * v1.uv[1] + p2 * v2.uv[1]) / den;
        Hit { obj: t.obj, mesh: t.mesh, tri: t.tri, u, v }
    }
    /// (object, mesh, triangle) of an entry of `tri_at`, without the UV work of `hit_at`.
    pub fn tri_key(&self, ti: u32) -> Option<(u16, u16, u32)> {
        self.tris.get(ti as usize).map(|t| (t.obj, t.mesh, t.tri))
    }
    /// Linear view depth at a pixel (`None` for the background).
    pub fn view_depth(&self, x: i32, y: i32, near: f32, far: f32) -> Option<f32> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return None;
        }
        let z = self.depth[(y as u32 * self.w + x as u32) as usize];
        if !z.is_finite() || self.tri_at[(y as u32 * self.w + x as u32) as usize] == NO_TRI {
            return None;
        }
        Some(2.0 * far * near / ((far + near) - z * (far - near)))
    }
    /// An RGBA picture (same size as the frame) that highlights pixels: `tint(obj, mesh, tri)`
    /// returns a colour (alpha 0 = nothing) per triangle; `outline` draws a border of `width`
    /// pixels around everything of the listed objects in the given colours.
    pub fn overlay(&self, tint: &dyn Fn(u16, u16, u32) -> [u8; 4], outline: &[(u16, [u8; 4])], width: i32) -> Vec<u8> {
        let (w, h) = (self.w as usize, self.h as usize);
        let mut out = vec![0u8; w * h * 4];
        // the tint of each triangle, asked once (rebuilt for every frame while editing)
        let mut cache: Vec<Option<[u8; 4]>> = vec![None; self.tris.len()];
        let obj_at = |i: usize| self.tri_at.get(i).and_then(|&t| self.tris.get(t as usize)).map(|t| t.obj);
        for (i, &ti) in self.tri_at.iter().enumerate().take(w * h) {
            let Some(slot) = cache.get_mut(ti as usize) else { continue };
            let c = *slot.get_or_insert_with(|| {
                let t = &self.tris[ti as usize];
                tint(t.obj, t.mesh, t.tri)
            });
            if c[3] > 0 {
                out[i * 4..i * 4 + 4].copy_from_slice(&c);
            }
        }
        for (id, col) in outline {
            // bounding box of the object's pixels, grown by the width
            let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
            let mut mask = vec![false; w * h];
            for i in 0..w * h {
                if obj_at(i) == Some(*id) {
                    mask[i] = true;
                    let (x, y) = (i % w, i / w);
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
            if x0 == usize::MAX {
                continue;
            }
            let wd = width.max(1) as usize;
            let (x0, y0, x1, y1) = (x0.saturating_sub(wd), y0.saturating_sub(wd), (x1 + wd).min(w - 1), (y1 + wd).min(h - 1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let i = y * w + x;
                    if mask[i] {
                        continue;
                    }
                    let mut near = false;
                    'n: for dy in -width..=width {
                        for dx in -width..=width {
                            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                            if nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32 && mask[ny as usize * w + nx as usize] {
                                near = true;
                                break 'n;
                            }
                        }
                    }
                    if near {
                        out[i * 4..i * 4 + 4].copy_from_slice(col);
                    }
                }
            }
        }
        out
    }
    /// True when at least one pixel shows geometry.
    pub fn any_hit(&self) -> bool {
        self.tri_at.iter().any(|&t| t != NO_TRI)
    }
    /// The colour buffer as plain RGBA bytes without copying.
    pub fn rgba(&self) -> &[u8] {
        self.color.as_flattened()
    }
    pub fn to_rgba_bytes(&self) -> Vec<u8> {
        self.rgba().to_vec()
    }
    pub fn to_rgb_bytes(&self) -> Vec<u8> {
        self.color.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
    }
    /// Nearest-neighbour upscale to any resolution (keeps the pixel look).
    pub fn resized_nearest(&self, nw: u32, nh: u32) -> Vec<[u8; 4]> {
        let (nw, nh) = (nw.max(1), nh.max(1));
        let mut out = Vec::with_capacity((nw * nh) as usize);
        for y in 0..nh {
            let sy = (y as u64 * self.h as u64 / nh as u64) as u32;
            for x in 0..nw {
                let sx = (x as u64 * self.w as u64 / nw as u64) as u32;
                out.push(self.color[(sy * self.w + sx) as usize]);
            }
        }
        out
    }
}

// ------------------------------------------------------------------ renderer

pub struct Instance<'a> {
    pub model: &'a Model,
    /// Per-mesh vertex positions/normals (after skinning). See `skin_model`.
    pub pos: &'a [Vec<Vec3>],
    pub nrm: &'a [Vec<Vec3>],
    pub world: Mat4,
    /// Texture set used for this frame (allows swapping textures without changing the model).
    pub textures: &'a [Texture],
    pub id: u16,
    pub tint: [f32; 3],
}

/// Skins every mesh of the model (or returns the source positions when there is no skeleton/pose).
pub fn skin_model(model: &Model, pose: Option<&Pose>) -> (Vec<Vec<Vec3>>, Vec<Vec<Vec3>>) {
    match (&model.skeleton, pose) {
        (Some(sk), Some(p)) if !sk.bones.is_empty() => {
            let sm = skin_matrices(sk, p);
            model.meshes.iter().map(|m| skin_mesh(m, &sm)).unzip()
        }
        _ => (
            model.meshes.iter().map(|m| m.verts.iter().map(|v| v.pos).collect()).collect(),
            model.meshes.iter().map(|m| m.verts.iter().map(|v| v.nrm).collect()).collect(),
        ),
    }
}

#[derive(Clone, Copy)]
struct CV {
    clip: [f32; 4],
    uv: [f32; 2],
    shade: [f32; 3],
    fog: f32,
}
fn lerp_cv(a: &CV, b: &CV, t: f32) -> CV {
    let l = |x: f32, y: f32| x + (y - x) * t;
    CV {
        clip: [l(a.clip[0], b.clip[0]), l(a.clip[1], b.clip[1]), l(a.clip[2], b.clip[2]), l(a.clip[3], b.clip[3])],
        uv: [l(a.uv[0], b.uv[0]), l(a.uv[1], b.uv[1])],
        shade: [l(a.shade[0], b.shade[0]), l(a.shade[1], b.shade[1]), l(a.shade[2], b.shade[2])],
        fog: l(a.fog, b.fog),
    }
}
/// Near-plane clipping (z + w >= 0 in clip space). A clipped triangle has at most 4 vertices.
fn clip_near(p: [CV; 3]) -> ([CV; 4], usize) {
    let d = |c: &CV| c.clip[2] + c.clip[3];
    let mut out = [p[0]; 4];
    let mut n = 0;
    for i in 0..3 {
        let (a, b) = (&p[i], &p[(i + 1) % 3]);
        let (da, db) = (d(a), d(b));
        if da >= 0.0 {
            out[n] = *a;
            n += 1;
        }
        if (da >= 0.0) != (db >= 0.0) {
            out[n] = lerp_cv(a, b, da / (da - db));
            n += 1;
        }
    }
    (out, n)
}

#[derive(Clone, Copy)]
struct SV {
    x: f64,
    y: f64,
    z: f32,
    iw: f32,
    uv: [f32; 2],
    shade: [f32; 3],
    fog: f32,
}
#[derive(Clone)]
struct Tri {
    v: [SV; 3],
    obj: u16,
    inst: u16,
    mesh: u16,
    tri: u32,
    tex: Option<usize>,
    key_black: bool,
    /// How much the light falls on this triangle (0 = turned away from it, 1 = facing it); shadows fade out on unlit surfaces.
    lit: f32,
    sort: f32,
}

const DITHER: [[i32; 4]; 4] = [[-4, 0, -3, 1], [2, -2, 3, -1], [-3, 1, -4, 0], [3, -1, 2, -2]];
/// Sub-pixel precision of the rasterizer: coordinates are rounded to 1/256 pixel and edge tests are exact integer arithmetic.
const FP: f64 = 256.0;

/// Vertex-stage data of one mesh of one instance, shared by the triangle builders.
struct MeshJob<'a> {
    mesh: &'a Mesh,
    wpos: Vec<Vec3>,
    /// Per vertex: colour * material * tint, cos(normal, light) and fog factor, computed once per vertex (not per triangle corner).
    vert: Vec<VertOut>,
    clips: Vec<[f32; 4]>,
    mat: Material,
    tex_idx: Option<usize>,
    inst: u16,
    mesh_i: u16,
    obj: u16,
}

#[derive(Clone, Copy)]
struct VertOut {
    base: [f32; 3],
    ndl: f32,
    fog: f32,
}

struct Setup<'a> {
    style: &'a PsxStyle,
    eye: Vec3,
    ldir: Vec3,
    w: u32,
    h: u32,
}

/// A picture (any size) blended over or under the 3D scene. It is sampled nearest-neighbour at the render size.
pub struct Plane<'a> {
    pub w: u32,
    pub h: u32,
    pub px: &'a [[u8; 4]],
    pub opacity: u8,
}

/// A plane together with the column lookup for the current render width.
struct PlaneJob<'a> {
    px: &'a [[u8; 4]],
    pw: u32,
    ph: u32,
    opacity: u8,
    xmap: Vec<u32>,
}
impl<'a> PlaneJob<'a> {
    /// `aspect`: when the picture is drawn into a frame of another shape (the editor viewport), the plane covers the centred
    /// region of that aspect ratio, as the final render would be framed; columns outside it stay empty.
    fn new(p: &Plane<'a>, w: u32, h: u32, aspect: Option<f32>) -> Option<Self> {
        if p.w == 0 || p.h == 0 || p.opacity == 0 || p.px.len() < (p.w as usize) * (p.h as usize) {
            return None;
        }
        let (x0, x1) = match aspect {
            Some(a) if a > 0.0 => {
                let rw = (h as f32 * a).round().max(1.0) as i64;
                let x0 = (w as i64 - rw) / 2;
                (x0, x0 + rw)
            }
            _ => (0, w as i64),
        };
        let xmap = (0..w as i64).map(|x| if x < x0 || x >= x1 { u32::MAX } else { ((x - x0) as u64 * p.w as u64 / (x1 - x0) as u64) as u32 }).collect();
        Some(PlaneJob { px: p.px, pw: p.w, ph: p.h, opacity: p.opacity, xmap })
    }
}

/// Blends planes (bottom first) into the rows of a band that starts at row `y0`.
fn blend_planes(color: &mut [[u8; 4]], y0: i32, w: u32, h: u32, planes: &[PlaneJob]) {
    let wu = w as usize;
    let rows = color.len() / wu;
    for p in planes {
        for r in 0..rows {
            let y = y0 as u64 + r as u64;
            let sy = (y * p.ph as u64 / h as u64) as usize;
            let src = &p.px[sy * p.pw as usize..(sy + 1) * p.pw as usize];
            let dst = &mut color[r * wu..(r + 1) * wu];
            for (d, &sx) in dst.iter_mut().zip(&p.xmap) {
                if sx == u32::MAX {
                    continue;
                }
                let s = src[sx as usize];
                if s[3] != 0 {
                    crate::layers::blend_over(d, s, p.opacity);
                }
            }
        }
    }
}

pub struct Renderer {
    pub frame: Frame,
    /// Worker threads used for drawing (1 = single-threaded). The picture is identical for any value.
    pub threads: usize,
    /// Aspect ratio (width / height) of the final picture when this renderer draws something of another shape (the viewport).
    pub layer_aspect: Option<f32>,
    /// Shadows for the next `render_layered` call (taken by it, so they never carry over to another frame).
    pub shadow: Option<crate::shadow::ShadowInput>,
    /// Upper limit for the scene's shadows in pictures drawn by this renderer (the editing viewport); `None` = exactly as the scene says.
    pub shadow_cap: Option<crate::lighting::ShadowCap>,
    // buffers reused between frames
    bands: Vec<Vec<u32>>,
}

/// How many worker threads this machine can use sensibly (all logical cores, at most 16).
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 16)
}

impl Renderer {
    pub fn new(w: u32, h: u32) -> Self {
        Self { frame: Frame::new(w, h), threads: default_threads(), layer_aspect: None, shadow: None, shadow_cap: None, bands: Vec::new() }
    }

    pub fn render(&mut self, style: &PsxStyle, cam: &Camera, insts: &[Instance]) {
        self.render_layered(style, cam, insts, &[], &[]);
    }

    /// Like [`Renderer::render`], with pictures behind (`back`, drawn over the sky) and in front of (`front`) the 3D objects.
    /// Both are blended before the post effects (colour depth, grain, scanlines), so they share the look of the scene.
    pub fn render_layered(&mut self, style: &PsxStyle, cam: &Camera, insts: &[Instance], back: &[Plane], front: &[Plane]) {
        let (w, h) = (style.width.max(1), style.height.max(1));
        if self.frame.w != w || self.frame.h != h {
            self.frame = Frame::new(w, h);
        }
        let view = cam.view();
        let vp = cam.proj(w as f32 / h as f32) * view;
        let setup = Setup { style, eye: cam.eye, ldir: -style.light_dir.normalized(), w, h };
        let threads = self.threads.max(1);

        // ---- vertex stage + triangle setup (parallel over triangle chunks, deterministic order)
        let mut tris = std::mem::take(&mut self.frame.tris);
        tris.clear();
        for (ii, inst) in insts.iter().enumerate() {
            for (mi, mesh) in inst.model.meshes.iter().enumerate() {
                let (Some(pos), Some(nrm)) = (inst.pos.get(mi), inst.nrm.get(mi)) else { continue };
                if pos.len() != mesh.verts.len() || nrm.len() != mesh.verts.len() {
                    continue;
                }
                let mat = inst.model.materials.get(mesh.material).cloned().unwrap_or_default();
                let tex_idx = mat.texture.filter(|&t| t < inst.textures.len());
                let wpos: Vec<Vec3> = pos.iter().map(|p| inst.world.transform_point(*p)).collect();
                let clips: Vec<[f32; 4]> = wpos.iter().map(|p| vp.mul_vec4([p.x, p.y, p.z, 1.0])).collect();
                let vert_stage = |range: std::ops::Range<usize>| -> Vec<VertOut> {
                    range
                        .map(|vi| {
                            let v = &mesh.verts[vi];
                            let n = inst.world.transform_dir(nrm[vi]).normalized();
                            let fog = if style.fog_on {
                                let depth = -view.transform_point(wpos[vi]).z;
                                ((depth - style.fog_near) / (style.fog_far - style.fog_near).max(1e-3)).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            VertOut { base: [v.color[0] * mat.color[0] * inst.tint[0], v.color[1] * mat.color[1] * inst.tint[1], v.color[2] * mat.color[2] * inst.tint[2]], ndl: n.dot(setup.ldir), fog }
                        })
                        .collect()
                };
                let nv = mesh.verts.len();
                let vert: Vec<VertOut> = if threads > 1 && nv >= 8192 {
                    let chunk = nv.div_ceil(threads);
                    let parts: Vec<Vec<VertOut>> = std::thread::scope(|sc| {
                        let hs: Vec<_> = (0..nv).step_by(chunk).map(|s0| { let vs = &vert_stage; sc.spawn(move || vs(s0..(s0 + chunk).min(nv))) }).collect();
                        hs.into_iter().map(|h| h.join().expect("vertex stage thread")).collect()
                    });
                    parts.into_iter().flatten().collect()
                } else {
                    vert_stage(0..nv)
                };
                let job = MeshJob { mesh, wpos, vert, clips, mat, tex_idx, inst: ii as u16, mesh_i: mi as u16, obj: inst.id };
                let n = mesh.tris.len();
                if threads > 1 && n >= 4096 {
                    let chunk = n.div_ceil(threads);
                    let parts: Vec<Vec<Tri>> = std::thread::scope(|sc| {
                        let handles: Vec<_> = (0..n).step_by(chunk).map(|s| {
                            let (job, setup) = (&job, &setup);
                            sc.spawn(move || build_tris(job, setup, s..(s + chunk).min(n)))
                        }).collect();
                        handles.into_iter().map(|h| h.join().expect("triangle setup thread")).collect()
                    });
                    for p in parts {
                        tris.extend(p);
                    }
                } else {
                    tris.extend(build_tris(&job, &setup, 0..n));
                }
            }
        }
        // drawing order: generation order with a z-buffer; far triangles first (PS1 ordering-table style) without.
        // Only a compact list of (key, index) is sorted - the stable sort keeps the original order of equal keys.
        let mut order: Vec<(u64, u32)> = Vec::new();
        if !style.z_buffer {
            order.extend(tris.iter().enumerate().map(|(i, t)| (!sortable(t.sort) as u64, i as u32)));
            order.sort_by_key(|k| k.0);
        }

        // ---- bin triangles into horizontal bands
        let rows_per_band = ((h as usize).div_ceil(threads * 4)).max(8);
        let nb = (h as usize).div_ceil(rows_per_band);
        self.bands.resize_with(nb, Vec::new);
        self.bands.iter_mut().for_each(|b| b.clear());
        let mut bin = |i: u32| {
            let (y0, y1) = tri_rows(&tris[i as usize], h as i32);
            if y0 <= y1 {
                for b in (y0 as usize / rows_per_band)..=(y1 as usize / rows_per_band) {
                    self.bands[b].push(i);
                }
            }
        };
        if order.is_empty() {
            (0..tris.len() as u32).for_each(&mut bin);
        } else {
            order.iter().for_each(|&(_, i)| bin(i));
        }

        // ---- draw bands (each band owns its rows of every buffer; no sharing, no locks while drawing)
        let mut rows = RowTables::new(style, h);
        rows.back = back.iter().filter_map(|p| PlaneJob::new(p, w, h, self.layer_aspect)).collect();
        rows.front = front.iter().filter_map(|p| PlaneJob::new(p, w, h, self.layer_aspect)).collect();
        let shadow_in = self.shadow.take();
        if let Some(s) = &shadow_in {
            let fog = style.fog_on.then_some((style.fog_near, style.fog_far));
            rows.shadow = crate::shadow::ShadowPass::new(&s.map, s.strength, &vp, &view, fog, w, h);
        }
        let f = &mut self.frame;
        let wu = w as usize;
        let jobs: Vec<BandJob> = f
            .color
            .chunks_mut(rows_per_band * wu)
            .zip(f.depth.chunks_mut(rows_per_band * wu))
            .zip(f.tri_at.chunks_mut(rows_per_band * wu))
            .enumerate()
            .map(|(bi, ((color, depth), tri_at))| BandJob { y0: (bi * rows_per_band) as i32, color, depth, tri_at, list: &self.bands[bi] })
            .collect();
        let work = (tris.len() as u64) * 64 + (w as u64) * (h as u64);
        if threads == 1 || work < 40_000 {
            for j in jobs {
                draw_band(j, &tris, insts, style, &rows, w, h);
            }
        } else {
            let queue = std::sync::Mutex::new(jobs);
            std::thread::scope(|sc| {
                for _ in 0..threads {
                    sc.spawn(|| loop {
                        let job = queue.lock().expect("band queue").pop();
                        match job {
                            Some(j) => draw_band(j, &tris, insts, style, &rows, w, h),
                            None => break,
                        }
                    });
                }
            });
        }
        self.frame.tris = tris;
    }
}

/// Monotonic integer key of a float (larger float = larger key), so triangles can be sorted by plain integers.
fn sortable(f: f32) -> u32 {
    let b = f.to_bits();
    if b & 0x8000_0000 != 0 { !b } else { b | 0x8000_0000 }
}

/// Inclusive range of screen rows a triangle touches (empty when y0 > y1).
fn tri_rows(t: &Tri, h: i32) -> (i32, i32) {
    let [a, b, c] = t.v;
    let y0 = (a.y.min(b.y).min(c.y).floor() as i32).max(0);
    let y1 = (a.y.max(b.y).max(c.y).ceil() as i32).min(h - 1);
    (y0, y1)
}

/// Vertex shading, near-plane clipping, projection and snapping for one range of a mesh's triangles.
fn build_tris(job: &MeshJob, st: &Setup, range: std::ops::Range<usize>) -> Vec<Tri> {
    let (style, mat) = (st.style, &job.mat);
    let (w, h) = (st.w as f32, st.h as f32);
    let mut out = Vec::with_capacity(range.len());
    for ti in range {
        let t = job.mesh.tris[ti];
        let ids = [t[0] as usize, t[1] as usize, t[2] as usize];
        if ids.iter().any(|&i| i >= job.wpos.len()) {
            continue;
        }
        let (a, b, c) = (job.wpos[ids[0]], job.wpos[ids[1]], job.wpos[ids[2]]);
        let front = (b - a).cross(c - a).dot(st.eye - a) > 0.0;
        if !front && !mat.double_sided {
            continue;
        }
        // trivially outside one side of the view volume: nothing of this triangle can be visible
        let cl = [job.clips[ids[0]], job.clips[ids[1]], job.clips[ids[2]]];
        if cl.iter().all(|p| p[0] > p[3]) || cl.iter().all(|p| p[0] < -p[3]) || cl.iter().all(|p| p[1] > p[3]) || cl.iter().all(|p| p[1] < -p[3]) {
            continue;
        }
        let sign = if front { 1.0 } else { -1.0 };
        let mut cvs = [CV { clip: [0.0; 4], uv: [0.0; 2], shade: [0.0; 3], fog: 0.0 }; 3];
        let mut lit_sum = 0.0f32;
        for k in 0..3 {
            let vi = ids[k];
            let (v, o) = (&job.mesh.verts[vi], &job.vert[vi]);
            let shade = if mat.unlit {
                o.base
            } else {
                let d = (o.ndl * sign).max(0.0);
                [
                    o.base[0] * (style.ambient[0] + style.light_color[0] * d).min(2.0),
                    o.base[1] * (style.ambient[1] + style.light_color[1] * d).min(2.0),
                    o.base[2] * (style.ambient[2] + style.light_color[2] * d).min(2.0),
                ]
            };
            lit_sum += if mat.unlit { 1.0 } else { (o.ndl * sign).max(0.0) };
            cvs[k] = CV { clip: job.clips[vi], uv: [v.uv.x, v.uv.y], shade, fog: o.fog };
        }
        let lit = lit_sum / 3.0;
        let (poly, n) = clip_near(cvs);
        if n < 3 {
            continue;
        }
        let mut svs = [SV { x: 0.0, y: 0.0, z: 0.0, iw: 0.0, uv: [0.0; 2], shade: [0.0; 3], fog: 0.0 }; 4];
        for (sv, c) in svs.iter_mut().zip(&poly[..n]) {
            let iw = 1.0 / c.clip[3];
            let (mut sx, mut sy) = (((c.clip[0] * iw * 0.5 + 0.5) * w) as f64, ((0.5 - c.clip[1] * iw * 0.5) * h) as f64);
            if style.snap > 0.0 {
                let s = style.snap as f64;
                sx = (sx / s).round() * s;
                sy = (sy / s).round() * s;
            }
            *sv = SV { x: sx, y: sy, z: c.clip[2] * iw, iw, uv: c.uv, shade: c.shade, fog: c.fog };
        }
        for k in 1..n - 1 {
            let (v0, mut v1, mut v2) = (svs[0], svs[k], svs[k + 1]);
            let area = (v1.x - v0.x) * (v2.y - v0.y) - (v1.y - v0.y) * (v2.x - v0.x);
            if area == 0.0 {
                continue;
            }
            if area < 0.0 {
                std::mem::swap(&mut v1, &mut v2);
            }
            // completely outside the screen after projection
            let (minx, maxx) = (v0.x.min(v1.x).min(v2.x), v0.x.max(v1.x).max(v2.x));
            let (miny, maxy) = (v0.y.min(v1.y).min(v2.y), v0.y.max(v1.y).max(v2.y));
            if maxx < 0.0 || maxy < 0.0 || minx > w as f64 || miny > h as f64 {
                continue;
            }
            let sort = (1.0 / v0.iw + 1.0 / v1.iw + 1.0 / v2.iw) / 3.0;
            out.push(Tri { v: [v0, v1, v2], obj: job.obj, inst: job.inst, mesh: job.mesh_i, tri: ti as u32, tex: job.tex_idx, key_black: mat.color_key, lit, sort });
        }
    }
    out
}

/// Per-frame lookup tables shared by all bands.
struct RowTables<'a> {
    back: Vec<PlaneJob<'a>>,
    front: Vec<PlaneJob<'a>>,
    /// Background colour of each row.
    bg: Vec<[u8; 4]>,
    /// Output value for every 8-bit channel value after quantization to `color_bits`.
    qlut: [u8; 256],
    /// Shadows to apply to what the model drew (before the layers in front and the post effects).
    shadow: Option<crate::shadow::ShadowPass<'a>>,
}
impl<'a> RowTables<'a> {
    fn new(style: &PsxStyle, h: u32) -> Self {
        let bg = (0..h)
            .map(|y| {
                let t = y as f32 / (h.max(2) - 1) as f32;
                let ch = |i: usize| ((style.bg_top[i] + (style.bg_bottom[i] - style.bg_top[i]) * t).clamp(0.0, 1.0) * 255.0).round() as u8;
                [ch(0), ch(1), ch(2), 255]
            })
            .collect();
        let bits = style.color_bits.clamp(1, 8);
        let mut qlut = [0u8; 256];
        for (v, o) in qlut.iter_mut().enumerate() {
            *o = if bits >= 8 { v as u8 } else { (((v as u32) >> (8 - bits)) * 255 / ((1u32 << bits) - 1)) as u8 };
        }
        Self { back: Vec::new(), front: Vec::new(), bg, qlut, shadow: None }
    }
}

struct BandJob<'a> {
    y0: i32,
    color: &'a mut [[u8; 4]],
    depth: &'a mut [f32],
    tri_at: &'a mut [u32],
    list: &'a [u32],
}

fn draw_band(j: BandJob, tris: &[Tri], insts: &[Instance], style: &PsxStyle, rows: &RowTables, w: u32, h: u32) {
    let wu = w as usize;
    let nrows = j.color.len() / wu;
    for r in 0..nrows {
        let bg = rows.bg[j.y0 as usize + r];
        j.color[r * wu..(r + 1) * wu].fill(bg);
    }
    if !rows.back.is_empty() {
        blend_planes(j.color, j.y0, w, h, &rows.back);
    }
    j.depth.fill(f32::INFINITY);
    j.tri_at.fill(NO_TRI);
    let mut band = Band { y0: j.y0, y1: j.y0 + nrows as i32, color: j.color, depth: j.depth, tri_at: j.tri_at };
    for &ti in j.list {
        let t = &tris[ti as usize];
        let tex = t.tex.map(|i| &insts[t.inst as usize].textures[i]);
        band.raster_tri(ti, t, tex, style, rows, w);
    }
    if let Some(sp) = &rows.shadow {
        sp.apply(j.color, j.depth, j.tri_at, j.y0, w, h, &rows.qlut, &|ti| tris[ti as usize].lit);
    }
    if !rows.front.is_empty() {
        blend_planes(j.color, j.y0, w, h, &rows.front);
    }
    post_rows(style, j.color, w, h, j.y0);
}

struct Band<'a> {
    y0: i32,
    y1: i32,
    color: &'a mut [[u8; 4]],
    depth: &'a mut [f32],
    tri_at: &'a mut [u32],
}

#[inline]
fn floor_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b)
}
#[inline]
fn ceil_div(a: i64, b: i64) -> i64 {
    -((-a).div_euclid(b))
}

impl Band<'_> {
    fn raster_tri(&mut self, ti: u32, t: &Tri, tex: Option<&Texture>, style: &PsxStyle, rows: &RowTables, w: u32) {
        let [v0, v1, v2] = t.v;
        let wi = w as i32;
        let minx = (v0.x.min(v1.x).min(v2.x).floor() as i32).max(0);
        let maxx = (v0.x.max(v1.x).max(v2.x).ceil() as i32).min(wi - 1);
        let miny = (v0.y.min(v1.y).min(v2.y).floor() as i32).max(self.y0);
        let maxy = (v0.y.max(v1.y).max(v2.y).ceil() as i32).min(self.y1 - 1);
        if minx > maxx || miny > maxy {
            return;
        }
        // exact fixed-point edge functions
        let fx = |v: &SV| ((v.x * FP).round() as i64, (v.y * FP).round() as i64);
        let (p0, p1, p2) = (fx(&v0), fx(&v1), fx(&v2));
        let area = (p1.0 - p0.0) * (p2.1 - p0.1) - (p1.1 - p0.1) * (p2.0 - p0.0);
        if area <= 0 {
            return;
        }
        // edge i is opposite vertex i: e0 = (v1->v2), e1 = (v2->v0), e2 = (v0->v1)
        let edges = [(p1, p2), (p2, p0), (p0, p1)];
        let sample0 = (minx as i64 * 256 + 128, miny as i64 * 256 + 128);
        let mut a_step = [0i64; 3]; // d e / d pixel x
        let mut b_step = [0i64; 3]; // d e / d pixel y
        let mut e_start = [0i64; 3];
        let mut thr = [0i64; 3]; // pixel is inside when e >= thr (shared edges belong to exactly one triangle)
        for (i, (a, b)) in edges.iter().enumerate() {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            a_step[i] = -dy * 256;
            b_step[i] = dx * 256;
            e_start[i] = dx * (sample0.1 - a.1) - dy * (sample0.0 - a.0);
            let owns = dy > 0 || (dy == 0 && dx > 0);
            thr[i] = if owns { 0 } else { 1 };
        }
        let inv_area = 1.0 / area as f64;
        let wu = w as usize;
        // vertex attribute table: z, u, v, r, g, b, fog [, iw, u*iw, v*iw]
        let persp = !style.affine;
        let na = if persp { 10 } else { 7 };
        let mut q = [[0f64; 10]; 3];
        for (k, v) in [v0, v1, v2].iter().enumerate() {
            q[k][..7].copy_from_slice(&[v.z as f64, v.uv[0] as f64, v.uv[1] as f64, v.shade[0] as f64, v.shade[1] as f64, v.shade[2] as f64, v.fog as f64]);
            q[k][7..].copy_from_slice(&[v.iw as f64, (v.uv[0] * v.iw) as f64, (v.uv[1] * v.iw) as f64]);
        }
        let (tw, th, tpx): (i32, i32, &[[u8; 4]]) = match tex {
            Some(t) => (t.w as i32, t.h as i32, &t.px[..]),
            None => (1, 1, &[]),
        };
        let pow2 = tw & (tw - 1) == 0 && th & (th - 1) == 0;
        let (fw, fh) = (tw as f32, th as f32);
        let fog_c = [style.fog_color[0] * 255.0, style.fog_color[1] * 255.0, style.fog_color[2] * 255.0];
        let fog_on = style.fog_on;
        let dither = style.dither;
        let zbuf = style.z_buffer;

        for py in miny..=maxy {
            let dy = (py - miny) as i64;
            let mut e_row = [0i64; 3];
            let (mut kmin, mut kmax) = (0i64, (maxx - minx) as i64);
            let mut empty = false;
            for i in 0..3 {
                e_row[i] = e_start[i] + b_step[i] * dy;
                let a = a_step[i];
                if a > 0 {
                    kmin = kmin.max(ceil_div(thr[i] - e_row[i], a));
                } else if a < 0 {
                    kmax = kmax.min(floor_div(e_row[i] - thr[i], -a));
                } else if e_row[i] < thr[i] {
                    empty = true;
                }
            }
            if empty || kmin > kmax {
                continue;
            }
            // attribute value at pixel k of this row: base + step * k
            let mut base = [0f32; 10];
            let mut step = [0f32; 10];
            for n in 0..na {
                let (mut b, mut s) = (0.0f64, 0.0f64);
                for i in 0..3 {
                    b += (e_row[i] + a_step[i] * kmin) as f64 * q[i][n];
                    s += a_step[i] as f64 * q[i][n];
                }
                base[n] = (b * inv_area) as f32;
                step[n] = (s * inv_area) as f32;
            }
            let row_i = (py - self.y0) as usize * wu;
            for k in kmin..=kmax {
                let kk = (k - kmin) as f32;
                let px = minx + k as i32;
                let idx = row_i + px as usize;
                let z = base[0] + step[0] * kk;
                if zbuf && z >= self.depth[idx] {
                    continue;
                }
                let (u, v) = if persp {
                    let iw = (base[7] + step[7] * kk).max(1e-20);
                    ((base[8] + step[8] * kk) / iw, (base[9] + step[9] * kk) / iw)
                } else {
                    (base[1] + step[1] * kk, base[2] + step[2] * kk)
                };
                let mut rgb = [255.0f32; 3];
                if tex.is_some() {
                    let (ix, iy) = (ifloor(u * fw), ifloor(v * fh));
                    let (xx, yy) = if pow2 { ((ix & (tw - 1)) as usize, (iy & (th - 1)) as usize) } else { (ix.rem_euclid(tw) as usize, iy.rem_euclid(th) as usize) };
                    let texel = tpx[yy * tw as usize + xx];
                    // transparency: alpha 0, or black as a colour key (material `color_key`)
                    if texel[3] < 128 || (t.key_black && texel[0] == 0 && texel[1] == 0 && texel[2] == 0) {
                        continue;
                    }
                    rgb = [texel[0] as f32, texel[1] as f32, texel[2] as f32];
                }
                let fog = if fog_on { base[6] + step[6] * kk } else { 0.0 };
                let d = if dither { DITHER[(py & 3) as usize][(px & 3) as usize] as f32 } else { 0.0 };
                let mut out = [0u8; 4];
                out[3] = 255;
                for c in 0..3 {
                    let lit = rgb[c] * (base[3 + c] + step[3 + c] * kk);
                    let col = if fog_on { lit + (fog_c[c] - lit) * fog } else { lit };
                    out[c] = rows.qlut[(col + d + 0.5).clamp(0.0, 255.0) as usize];
                }
                self.color[idx] = out;
                self.depth[idx] = z;
                self.tri_at[idx] = ti;
            }
        }
    }
}

/// Mood post-processing for the rows of one band: tint, saturation, contrast, brightness, vignette, scanlines, grain.
fn post_rows(s: &PsxStyle, color: &mut [[u8; 4]], w: u32, h: u32, y0: i32) {
    let neutral = s.tint == [1.0; 3] && s.saturation == 1.0 && s.contrast == 1.0 && s.brightness == 1.0 && s.vignette == 0.0 && s.scanlines == 0.0 && s.grain == 0.0;
    if neutral {
        return;
    }
    let nrows = color.len() / w as usize;
    // the column part of the vignette, with the same arithmetic as per pixel
    let dx2: Vec<f32> = if s.vignette > 0.0 { (0..w).map(|x| { let dx = x as f32 / w as f32 - 0.5; dx * dx }).collect() } else { vec![] };
    for r in 0..nrows {
        let y = y0 as u32 + r as u32;
        let dy = y as f32 / h as f32 - 0.5;
        let dy2 = dy * dy;
        for x in 0..w {
            let p = &mut color[r * w as usize + x as usize];
            let mut c = [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0];
            let luma = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            for k in 0..3 {
                c[k] = luma + (c[k] - luma) * s.saturation;
                c[k] = ((c[k] * s.tint[k] - 0.5) * s.contrast + 0.5) * s.brightness;
            }
            if s.vignette > 0.0 {
                let v = 1.0 - s.vignette * ((dx2[x as usize] + dy2) * 2.2).min(1.0);
                c.iter_mut().for_each(|k| *k *= v);
            }
            if s.scanlines > 0.0 && y % 2 == 1 {
                c.iter_mut().for_each(|k| *k *= 1.0 - s.scanlines);
            }
            if s.grain > 0.0 {
                // deterministic hash: the same frame always produces the same grain
                let mut n = (x.wrapping_mul(73856093)) ^ (y.wrapping_mul(19349663));
                n = (n ^ (n >> 13)).wrapping_mul(1274126177);
                let g = ((n >> 8) & 0xFF) as f32 / 255.0 - 0.5;
                c.iter_mut().for_each(|k| *k += g * s.grain);
            }
            *p = [(c[0].clamp(0.0, 1.0) * 255.0).round() as u8, (c[1].clamp(0.0, 1.0) * 255.0).round() as u8, (c[2].clamp(0.0, 1.0) * 255.0).round() as u8, 255];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad_model(double: bool) -> Model {
        let mut m = Mesh::new("q");
        // quad in the XY plane, normal +Z (counter-clockwise when viewed from +Z)
        for (p, uv) in [((-1., -1.), (0., 1.)), ((1., -1.), (1., 1.)), ((1., 1.), (1., 0.)), ((-1., 1.), (0., 0.))] {
            m.verts.push(Vertex::new(Vec3::new(p.0, p.1, 0.0), Vec2::new(uv.0, uv.1)));
        }
        m.tris = vec![[0, 1, 2], [0, 2, 3]];
        m.recompute_normals();
        let mut model = Model { meshes: vec![m], ..Default::default() };
        model.materials.push(Material { color: [1.0, 0.0, 0.0], double_sided: double, unlit: true, ..Default::default() });
        model
    }
    fn style() -> PsxStyle {
        PsxStyle { width: 64, height: 64, fog_on: false, dither: false, color_bits: 8, ..PsxStyle::ps1_classic() }
    }
    fn render_with(model: &Model, cam: &Camera, st: &PsxStyle) -> Renderer {
        let (p, n) = skin_model(model, None);
        let inst = Instance { model, pos: &p, nrm: &n, world: Mat4::IDENTITY, textures: &[], id: 3, tint: [1.0; 3] };
        let mut r = Renderer::new(st.width, st.height);
        r.render(st, cam, &[inst]);
        r
    }
    fn cam_front(z: f32) -> Camera {
        Camera { eye: Vec3::new(0.0, 0.0, z), target: Vec3::ZERO, ..Default::default() }
    }

    #[test]
    fn front_face_visible_back_face_culled() {
        let m = quad_model(false);
        let r = render_with(&m, &cam_front(4.0), &style());
        assert_eq!(r.frame.color[32 * 64 + 32][..3], [255, 0, 0]);
        assert_eq!(r.frame.hit_at(32, 32).obj, 3);
        let r2 = render_with(&m, &cam_front(-4.0), &style());
        assert!(!r2.frame.hit_at(32, 32).is_some(), "back face must be culled");
        let md = quad_model(true);
        let r3 = render_with(&md, &cam_front(-4.0), &style());
        assert!(r3.frame.hit_at(32, 32).is_some(), "a double-sided material is visible from behind");
    }

    #[test]
    fn no_gap_on_shared_diagonal() {
        let m = quad_model(false);
        let r = render_with(&m, &cam_front(2.5), &PsxStyle { snap: 0.0, ..style() });
        // there must be no holes inside the quad (the shared diagonal is covered)
        let (mut inside, mut holes) = (0, 0);
        for y in 0..64 {
            for x in 0..64 {
                let c = r.frame.color[y * 64 + x];
                let covered = r.frame.hit_at(x as i32, y as i32).is_some();
                if x > 16 && x < 48 && y > 16 && y < 48 {
                    inside += 1;
                    if !covered || c[..3] != [255, 0, 0] {
                        holes += 1;
                    }
                }
            }
        }
        assert!(inside > 0);
        assert_eq!(holes, 0);
    }

    #[test]
    fn picking_returns_perspective_correct_uv() {
        let m = quad_model(false);
        let r = render_with(&m, &cam_front(4.0), &PsxStyle { snap: 0.0, ..style() });
        let h = r.frame.hit_at(32, 32);
        assert!((h.u - 0.5).abs() < 0.08 && (h.v - 0.5).abs() < 0.08, "quad centre -> UV~(0.5,0.5), got ({}, {})", h.u, h.v);
        let left = r.frame.hit_at(20, 32);
        assert!(left.u < h.u);
    }

    #[test]
    fn snap_quantizes_vertices_and_clipping_survives() {
        // camera lies in the plane: the triangle crosses the near plane and must not panic
        let m = quad_model(true);
        let cam = Camera { eye: Vec3::new(0.0, 0.0, 0.01), target: Vec3::new(0.0, 0.0, -1.0), ..Default::default() };
        let _ = render_with(&m, &cam, &style());
        let cam2 = Camera { eye: Vec3::new(0.3, 0.2, 1.0), target: Vec3::new(1.0, 0.0, 0.0), ..Default::default() };
        let _ = render_with(&m, &cam2, &style());
    }

    #[test]
    fn zbuffer_vs_painter_order() {
        // two quads: red is farther, green is nearer. Drawn in an unfavourable order.
        let mut a = quad_model(false);
        let mut b = quad_model(false);
        a.materials[0].color = [1.0, 0.0, 0.0];
        b.materials[0].color = [0.0, 1.0, 0.0];
        for m in [&mut a, &mut b] {
            m.meshes[0].verts.iter_mut().for_each(|v| v.color = [1.0; 3]);
        }
        let (pa, na) = skin_model(&a, None);
        let (pb, nb) = skin_model(&b, None);
        for zbuf in [true, false] {
            let st = PsxStyle { z_buffer: zbuf, ..style() };
            let far = Instance { model: &a, pos: &pa, nrm: &na, world: Mat4::translation(Vec3::new(0.0, 0.0, -1.0)), textures: &[], id: 1, tint: [1.0; 3] };
            let near = Instance { model: &b, pos: &pb, nrm: &nb, world: Mat4::IDENTITY, textures: &[], id: 2, tint: [1.0; 3] };
            let mut r = Renderer::new(64, 64);
            r.render(&st, &cam_front(4.0), &[near, far]);
            assert_eq!(r.frame.hit_at(32, 32).obj, 2, "the nearer object must be on top (z_buffer={zbuf})");
        }
    }

    #[test]
    fn texture_and_color_key() {
        let mut m = quad_model(false);
        m.materials[0].texture = Some(0);
        m.materials[0].color = [1.0; 3];
        m.materials[0].color_key = true;
        let mut tex = Texture::new("t", 2, 2, [0, 0, 255, 255]);
        tex.set(0, 0, [0, 0, 0, 255]); // black = transparent via colour key
        let (p, n) = skin_model(&m, None);
        let texs = [tex];
        let inst = Instance { model: &m, pos: &p, nrm: &n, world: Mat4::IDENTITY, textures: &texs, id: 0, tint: [1.0; 3] };
        let st = PsxStyle { snap: 0.0, ..style() };
        let mut r = Renderer::new(64, 64);
        r.render(&st, &cam_front(2.5), &[inst]);
        // top-left of the quad is the black key -> background; bottom-right is blue
        assert!(!r.frame.hit_at(22, 22).is_some());
        assert_eq!(r.frame.color[42 * 64 + 42][..3], [0, 0, 255]);
    }

    #[test]
    fn quantize_to_5_bits_and_post_is_deterministic() {
        let m = quad_model(false);
        let st = PsxStyle { color_bits: 5, grain: 0.1, vignette: 0.3, ..style() };
        let a = render_with(&m, &cam_front(4.0), &st);
        let b = render_with(&m, &cam_front(4.0), &st);
        assert_eq!(a.frame.color, b.frame.color);
        let st2 = PsxStyle { grain: 0.0, vignette: 0.0, ..st };
        let c = render_with(&m, &cam_front(4.0), &st2);
        let px = c.frame.color[32 * 64 + 32];
        assert_eq!(px[0] % 8, 7 % 8, "5 bits -> values of the form (q*255/31)");
        assert!([0u8, 8, 16, 24, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173, 181, 189, 197, 206, 214, 222, 230, 239, 247, 255].contains(&px[0]));
    }

    #[test]
    fn camera_project_roundtrip() {
        let cam = Camera::orbit(Vec3::ZERO, 0.6, 0.3, 5.0);
        let (sx, sy, _) = cam.project(Vec3::ZERO, 200, 100).unwrap();
        assert!((sx - 100.0).abs() < 0.5 && (sy - 50.0).abs() < 0.5);
        let (o, d) = cam.ray(100.0, 50.0, 200, 100);
        // the ray through the screen centre passes through the target
        let to = (Vec3::ZERO - o).normalized();
        assert!(d.dot(to) > 0.999);
    }

    #[test]
    fn presets_exist() {
        assert_eq!(PsxStyle::presets().len(), 4);
    }
}
