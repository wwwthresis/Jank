//! The Jank! project and its native `.jank` file format.
//!
//! Format (version 1, little-endian):
//! ```text
//! "JANK" | u16 version | u16 reserved
//! chunk*:  tag[4] | u8 compression(0 raw, 1 deflate) | u32 raw_len | u32 stored_len | u32 crc32(stored) | data
//! last chunk: tag "END\0", empty
//! ```
//! Chunks: `META` (project name), `ASST` (models, textures, skeletons, emotions), `SCEN` (scene, camera, style), `LAYR` (scene layers and texture layers; optional), `LITE` (time of day and shadows; optional),
//! `ANIM` (frame-by-frame animations). Unknown chunks are skipped, so older builds can still read newer files that only add chunks.
//! Only Jank! understands this format; the `export` layer exists for exchanging data with other programs.
use crate::anim::*;
use crate::encode::png::{crc32, deflate, inflate};
use crate::math::*;
use crate::mesh::*;
use crate::pose::{BonePose, Pose};
use crate::raster::{Camera, PsxStyle};
use crate::layers::{Layer, LayerStack};
use crate::scene::*;
use crate::texture::Texture;
use std::fmt;
use std::io;
use std::path::Path;

pub const MAGIC: &[u8; 4] = b"JANK";
pub const VERSION: u16 = 1;
const MAX_CHUNK: usize = 1 << 30;

#[derive(Debug)]
pub enum JankError {
    BadMagic,
    UnsupportedVersion(u16),
    Truncated,
    Corrupt(String),
    Crc(String),
    Io(io::Error),
}
impl fmt::Display for JankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JankError::BadMagic => write!(f, "This is not a Jank! project file (.jank)"),
            JankError::UnsupportedVersion(v) => write!(f, "This file was created by a newer version of Jank! (file format v{v}; this build supports up to v{VERSION})"),
            JankError::Truncated => write!(f, "The file is cut off: it contains less data than it declares"),
            JankError::Corrupt(s) => write!(f, "The file is damaged: {s}"),
            JankError::Crc(s) => write!(f, "Checksum mismatch in block {s}"),
            JankError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}
impl std::error::Error for JankError {}
impl From<io::Error> for JankError {
    fn from(e: io::Error) -> Self {
        JankError::Io(e)
    }
}
type Res<T> = Result<T, JankError>;

// ------------------------------------------------------------------ project

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub model: Model,
    pub emotions: Vec<Emotion>,
    /// Layer stacks of textures (texture index, stack). The texture itself always holds the flattened picture.
    pub layers: Vec<(usize, LayerStack)>,
}
impl Asset {
    pub fn stack(&self, tex: usize) -> Option<&LayerStack> {
        self.layers.iter().find(|(t, _)| *t == tex).map(|(_, s)| s)
    }
    pub fn stack_mut(&mut self, tex: usize) -> Option<&mut LayerStack> {
        self.layers.iter_mut().find(|(t, _)| *t == tex).map(|(_, s)| s)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub name: String,
    pub assets: Vec<Asset>,
    pub scene: Scene,
    pub animations: Vec<Animation>,
}
impl Project {
    pub fn new(name: &str) -> Self {
        Self { name: name.into(), assets: vec![], scene: Scene::default(), animations: vec![] }
    }
    pub fn add_asset(&mut self, model: Model) -> usize {
        self.assets.push(Asset { model, emotions: vec![], layers: vec![] });
        self.assets.len() - 1
    }
    pub fn bones_of(&self, asset: usize) -> usize {
        self.assets.get(asset).and_then(|a| a.model.skeleton.as_ref()).map_or(0, |s| s.bones.len())
    }
    /// Adds a model to the project's assets and places it in the scene. Returns the new object id.
    pub fn place_model(&mut self, model: Model) -> u32 {
        let name = model.name.clone();
        let a = self.add_asset(model);
        self.scene.add_object(&name, a)
    }
}

// ------------------------------------------------------------------ reading and writing primitives

#[derive(Default)]
struct W {
    b: Vec<u8>,
}
impl W {
    fn u8(&mut self, v: u8) {
        self.b.push(v)
    }
    fn bool(&mut self, v: bool) {
        self.b.push(v as u8)
    }
    fn u16(&mut self, v: u16) {
        self.b.extend(v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) {
        self.b.extend(v.to_le_bytes())
    }
    fn f32(&mut self, v: f32) {
        self.b.extend(v.to_le_bytes())
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.b.extend(s.as_bytes())
    }
    fn v3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z)
    }
    fn f3(&mut self, v: [f32; 3]) {
        v.iter().for_each(|x| self.f32(*x))
    }
    fn quat(&mut self, q: Quat) {
        self.f32(q.x);
        self.f32(q.y);
        self.f32(q.z);
        self.f32(q.w)
    }
    fn opt_u32(&mut self, v: Option<usize>) {
        self.u32(v.map_or(u32::MAX, |x| x as u32))
    }
}

struct R<'a> {
    d: &'a [u8],
    p: usize,
}
impl<'a> R<'a> {
    fn new(d: &'a [u8]) -> Self {
        Self { d, p: 0 }
    }
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self.p.checked_add(n).ok_or(JankError::Truncated)?;
        let s = self.d.get(self.p..end).ok_or(JankError::Truncated)?;
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Res<u8> {
        Ok(self.take(1)?[0])
    }
    fn bool(&mut self) -> Res<bool> {
        Ok(self.u8()? != 0)
    }
    fn u16(&mut self) -> Res<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Res<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&mut self) -> Res<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn str(&mut self) -> Res<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| JankError::Corrupt("string is not valid UTF-8".into()))
    }
    /// Reads an element count with a safety check: the count cannot exceed what the remaining bytes could hold (every element takes at least `min` bytes).
    fn count(&mut self, min: usize) -> Res<usize> {
        let n = self.u32()? as usize;
        if n.checked_mul(min.max(1)).map_or(true, |t| t > self.d.len() - self.p) {
            return Err(JankError::Corrupt(format!("count {n} exceeds the size of the data")));
        }
        Ok(n)
    }
    fn v3(&mut self) -> Res<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn f3(&mut self) -> Res<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    fn quat(&mut self) -> Res<Quat> {
        Ok(Quat { x: self.f32()?, y: self.f32()?, z: self.f32()?, w: self.f32()? })
    }
    fn opt_u32(&mut self) -> Res<Option<usize>> {
        let v = self.u32()?;
        Ok((v != u32::MAX).then_some(v as usize))
    }
}

// ------------------------------------------------------------------ types

fn w_texture(w: &mut W, t: &Texture) {
    w.str(&t.name);
    w.u32(t.w);
    w.u32(t.h);
    // Palette mode: most low-resolution textures have at most 256 colours, which makes the data 4 times smaller before compression
    let mut pal: Vec<[u8; 4]> = Vec::new();
    let mut idx: Vec<u8> = Vec::with_capacity(t.px.len());
    let mut ok = true;
    let mut lookup = std::collections::HashMap::new();
    for p in &t.px {
        let i = *lookup.entry(*p).or_insert_with(|| {
            pal.push(*p);
            pal.len() - 1
        });
        if i > 255 {
            ok = false;
            break;
        }
        idx.push(i as u8);
    }
    if ok {
        w.u8(1);
        w.u16(pal.len() as u16);
        pal.iter().for_each(|c| w.b.extend(c));
        w.b.extend(idx);
    } else {
        w.u8(0);
        w.b.extend(t.px.iter().flat_map(|p| *p));
    }
}
fn r_texture(r: &mut R) -> Res<Texture> {
    let name = r.str()?;
    let (tw, th) = (r.u32()?, r.u32()?);
    let n = (tw as usize).checked_mul(th as usize).filter(|&n| n > 0 && n <= (1 << 28)).ok_or_else(|| JankError::Corrupt("texture size".into()))?;
    let mode = r.u8()?;
    let px: Vec<[u8; 4]> = match mode {
        1 => {
            let pn = r.u16()? as usize;
            let pal: Vec<[u8; 4]> = r.take(pn * 4)?.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
            let idx = r.take(n)?;
            let mut out = Vec::with_capacity(n);
            for &i in idx {
                out.push(*pal.get(i as usize).ok_or_else(|| JankError::Corrupt("palette index".into()))?);
            }
            out
        }
        0 => r.take(n * 4)?.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect(),
        m => return Err(JankError::Corrupt(format!("texture mode {m}"))),
    };
    Ok(Texture { name, w: tw, h: th, px })
}

fn w_model(w: &mut W, m: &Model) {
    w.str(&m.name);
    w.u32(m.meshes.len() as u32);
    for me in &m.meshes {
        w.str(&me.name);
        w.u32(me.material as u32);
        w.u32(me.verts.len() as u32);
        for v in &me.verts {
            w.v3(v.pos);
            w.v3(v.nrm);
            w.f32(v.uv.x);
            w.f32(v.uv.y);
            w.f3(v.color);
            v.joints.iter().for_each(|j| w.u16(*j));
            v.weights.iter().for_each(|x| w.f32(*x));
        }
        w.u32(me.tris.len() as u32);
        for t in &me.tris {
            t.iter().for_each(|i| w.u32(*i));
        }
    }
    w.u32(m.materials.len() as u32);
    for ma in &m.materials {
        w.str(&ma.name);
        w.opt_u32(ma.texture);
        w.f3(ma.color);
        w.u8(ma.unlit as u8 | (ma.double_sided as u8) << 1 | (ma.color_key as u8) << 2);
    }
    w.u32(m.textures.len() as u32);
    m.textures.iter().for_each(|t| w_texture(w, t));
    w.bool(m.skeleton.is_some());
    if let Some(sk) = &m.skeleton {
        w.u32(sk.bones.len() as u32);
        for b in &sk.bones {
            w.str(&b.name);
            w.opt_u32(b.parent);
            w.v3(b.rest_t);
            w.quat(b.rest_r);
            w.v3(b.rest_s);
        }
    }
}
fn r_model(r: &mut R) -> Res<Model> {
    let name = r.str()?;
    let mut meshes = vec![];
    for _ in 0..r.count(16)? {
        let mname = r.str()?;
        let material = r.u32()? as usize;
        let vn = r.count(80)?;
        let mut verts = Vec::with_capacity(vn);
        for _ in 0..vn {
            let pos = r.v3()?;
            let nrm = r.v3()?;
            let uv = Vec2::new(r.f32()?, r.f32()?);
            let color = r.f3()?;
            let joints = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
            let weights = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
            verts.push(Vertex { pos, nrm, uv, color, joints, weights });
        }
        let tn = r.count(12)?;
        let mut tris = Vec::with_capacity(tn);
        for _ in 0..tn {
            let t = [r.u32()?, r.u32()?, r.u32()?];
            if t.iter().any(|&i| i as usize >= verts.len()) {
                return Err(JankError::Corrupt("vertex index out of range".into()));
            }
            tris.push(t);
        }
        meshes.push(Mesh { name: mname, verts, tris, material });
    }
    let mut materials = vec![];
    for _ in 0..r.count(21)? {
        let name = r.str()?;
        let texture = r.opt_u32()?;
        let color = r.f3()?;
        let fl = r.u8()?;
        materials.push(Material { name, texture, color, unlit: fl & 1 != 0, double_sided: fl & 2 != 0, color_key: fl & 4 != 0 });
    }
    let mut textures = vec![];
    for _ in 0..r.count(13)? {
        textures.push(r_texture(r)?);
    }
    let skeleton = if r.bool()? {
        let mut bones = vec![];
        for _ in 0..r.count(44)? {
            let name = r.str()?;
            let parent = r.opt_u32()?;
            let (rest_t, rest_r, rest_s) = (r.v3()?, r.quat()?, r.v3()?);
            bones.push(Bone { name, parent, rest_t, rest_r, rest_s });
        }
        // the "parent before child" invariant protects skinning from cycles
        if bones.iter().enumerate().any(|(i, b)| b.parent.map_or(false, |p| p >= i)) {
            return Err(JankError::Corrupt("bones are not ordered: a parent must come before its children".into()));
        }
        Some(Skeleton { bones })
    } else {
        None
    };
    let mut m = Model { name, meshes, materials, textures, skeleton };
    m.ensure_material();
    Ok(m)
}

fn w_transform(w: &mut W, t: &Transform) {
    w.v3(t.pos);
    w.v3(t.rot_deg);
    w.v3(t.scale);
}
fn r_transform(r: &mut R) -> Res<Transform> {
    Ok(Transform { pos: r.v3()?, rot_deg: r.v3()?, scale: r.v3()? })
}
fn w_camera(w: &mut W, c: &Camera) {
    w.v3(c.eye);
    w.v3(c.target);
    w.v3(c.up);
    w.f32(c.fov_y);
    w.f32(c.near);
    w.f32(c.far);
}
fn r_camera(r: &mut R) -> Res<Camera> {
    Ok(Camera { eye: r.v3()?, target: r.v3()?, up: r.v3()?, fov_y: r.f32()?, near: r.f32()?, far: r.f32()? })
}
fn w_style(w: &mut W, s: &PsxStyle) {
    w.str(&s.name);
    w.u32(s.width);
    w.u32(s.height);
    w.f32(s.snap);
    w.u8(s.affine as u8 | (s.z_buffer as u8) << 1 | (s.dither as u8) << 2 | (s.fog_on as u8) << 3);
    w.u32(s.color_bits);
    w.f32(s.fog_near);
    w.f32(s.fog_far);
    w.f3(s.fog_color);
    w.f3(s.ambient);
    w.v3(s.light_dir);
    w.f3(s.light_color);
    w.f3(s.bg_top);
    w.f3(s.bg_bottom);
    w.f3(s.tint);
    for v in [s.saturation, s.contrast, s.brightness, s.vignette, s.scanlines, s.grain] {
        w.f32(v);
    }
}
fn r_style(r: &mut R) -> Res<PsxStyle> {
    let name = r.str()?;
    let (width, height) = (r.u32()?, r.u32()?);
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(JankError::Corrupt("render resolution".into()));
    }
    let snap = r.f32()?;
    let fl = r.u8()?;
    Ok(PsxStyle {
        name,
        width,
        height,
        snap,
        affine: fl & 1 != 0,
        z_buffer: fl & 2 != 0,
        dither: fl & 4 != 0,
        fog_on: fl & 8 != 0,
        color_bits: r.u32()?.clamp(1, 8),
        fog_near: r.f32()?,
        fog_far: r.f32()?,
        fog_color: r.f3()?,
        ambient: r.f3()?,
        light_dir: r.v3()?,
        light_color: r.f3()?,
        bg_top: r.f3()?,
        bg_bottom: r.f3()?,
        tint: r.f3()?,
        saturation: r.f32()?,
        contrast: r.f32()?,
        brightness: r.f32()?,
        vignette: r.f32()?,
        scanlines: r.f32()?,
        grain: r.f32()?,
    })
}
fn w_opt_tex(w: &mut W, t: &Option<Texture>) {
    w.bool(t.is_some());
    if let Some(t) = t {
        w_texture(w, t);
    }
}
fn r_opt_tex(r: &mut R) -> Res<Option<Texture>> {
    Ok(if r.bool()? { Some(r_texture(r)?) } else { None })
}

fn w_assets(p: &Project) -> Vec<u8> {
    let mut w = W::default();
    w.u32(p.assets.len() as u32);
    for a in &p.assets {
        w_model(&mut w, &a.model);
        w.u32(a.emotions.len() as u32);
        for e in &a.emotions {
            w.str(&e.name);
            w.u32(e.texture as u32);
            w_texture(&mut w, &e.image);
        }
    }
    w.b
}
fn r_assets(d: &[u8]) -> Res<Vec<Asset>> {
    let mut r = R::new(d);
    let mut out = vec![];
    for _ in 0..r.count(8)? {
        let model = r_model(&mut r)?;
        let mut emotions = vec![];
        for _ in 0..r.count(16)? {
            let name = r.str()?;
            let texture = r.u32()? as usize;
            emotions.push(Emotion { name, texture, image: r_texture(&mut r)? });
        }
        out.push(Asset { model, emotions, layers: vec![] });
    }
    Ok(out)
}
fn w_scene(s: &Scene) -> Vec<u8> {
    let mut w = W::default();
    w.u32(s.next_id());
    w.u32(s.objects.len() as u32);
    for o in &s.objects {
        w.u32(o.id);
        w.str(&o.name);
        w.u32(o.asset as u32);
        w_transform(&mut w, &o.transform);
        w.f3(o.tint);
        w.bool(o.visible);
    }
    w_camera(&mut w, &s.camera);
    w_style(&mut w, &s.style);
    w.b
}
fn r_scene(d: &[u8]) -> Res<Scene> {
    let mut r = R::new(d);
    let next = r.u32()?;
    let mut sc = Scene::default();
    for _ in 0..r.count(50)? {
        let id = r.u32()?;
        let name = r.str()?;
        let asset = r.u32()? as usize;
        let transform = r_transform(&mut r)?;
        let (tint, visible) = (r.f3()?, r.bool()?);
        sc.objects.push(SceneObject { id, name, asset, transform, tint, visible });
    }
    sc.camera = r_camera(&mut r)?;
    sc.style = r_style(&mut r)?;
    sc.set_next_id(next);
    Ok(sc)
}
fn w_layer(w: &mut W, l: &Layer) {
    w.str(&l.name);
    w.u8(l.visible as u8 | (l.locked as u8) << 1);
    w.u8(l.opacity);
    w_texture(w, &l.img);
}
fn r_layer(r: &mut R) -> Res<Layer> {
    let name = r.str()?;
    let fl = r.u8()?;
    let opacity = r.u8()?;
    Ok(Layer { name, visible: fl & 1 != 0, locked: fl & 2 != 0, opacity, img: r_texture(r)? })
}
/// Scene layers and texture layer stacks. Empty when the project uses no layers (then no block is written at all).
fn w_layers(p: &Project) -> Option<Vec<u8>> {
    let stacks: usize = p.assets.iter().map(|a| a.layers.len()).sum();
    if p.scene.layers.is_empty() && stacks == 0 {
        return None;
    }
    let mut w = W::default();
    w.u32(p.scene.layers.len() as u32);
    w.u32(p.scene.layers_back as u32);
    for l in &p.scene.layers {
        w_layer(&mut w, l);
    }
    w.u32(stacks as u32);
    for (ai, a) in p.assets.iter().enumerate() {
        for (tex, s) in &a.layers {
            w.u32(ai as u32);
            w.u32(*tex as u32);
            w.u32(s.active as u32);
            w.u32(s.layers.len() as u32);
            for l in &s.layers {
                w_layer(&mut w, l);
            }
        }
    }
    Some(w.b)
}
/// Time of day and shadows. Not written at all while they are at their defaults.
fn w_lighting(l: &crate::lighting::Lighting) -> Option<Vec<u8>> {
    if l.is_default() {
        return None;
    }
    let mut w = W::default();
    w.u8(l.tod_on as u8);
    w.f32(l.time);
    w.f32(l.azimuth);
    w.u8(l.shadows.id());
    w.f32(l.shadow_strength);
    w.f32(0.0); // formerly the ground height; shadows now fall on any surface
    w.u8(l.quality.id());
    w.f32(l.softness);
    Some(w.b)
}
fn r_lighting(d: &[u8]) -> Res<crate::lighting::Lighting> {
    let mut r = R::new(d);
    let tod_on = r.u8()? != 0;
    let time = r.f32()?;
    let azimuth = r.f32()?;
    let shadows = crate::lighting::ShadowMode::from_id(r.u8()?);
    let shadow_strength = r.f32()?;
    let _ground_y = r.f32()?;
    // quality and softness came later: older files end here and get the defaults
    let defaults = crate::lighting::Lighting::default();
    let quality = r.u8().map_or(defaults.quality, crate::lighting::ShadowQuality::from_id);
    let softness = r.f32().unwrap_or(defaults.softness);
    if ![time, azimuth, shadow_strength, softness].iter().all(|v| v.is_finite()) {
        return Err(JankError::Corrupt("LITE: not a number".into()));
    }
    Ok(crate::lighting::Lighting { tod_on, time: time.rem_euclid(24.0), azimuth, shadows, shadow_strength: shadow_strength.clamp(0.0, 1.0), quality, softness: softness.clamp(0.0, 1.0) })
}
fn r_layers(d: &[u8], p: &mut Project) -> Res<()> {
    let mut r = R::new(d);
    let n = r.count(16)?;
    let back = r.u32()? as usize;
    for _ in 0..n {
        p.scene.layers.push(r_layer(&mut r)?);
    }
    p.scene.layers_back = back.min(p.scene.layers.len());
    for _ in 0..r.count(16)? {
        let (ai, tex, active) = (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize);
        let mut layers = Vec::new();
        for _ in 0..r.count(16)? {
            layers.push(r_layer(&mut r)?);
        }
        let Some(a) = p.assets.get_mut(ai) else { continue };
        if tex >= a.model.textures.len() || layers.is_empty() {
            continue;
        }
        let (w, h) = (a.model.textures[tex].w, a.model.textures[tex].h);
        if layers.iter().any(|l| l.img.w != w || l.img.h != h) {
            continue; // inconsistent stack: keep the flattened texture only
        }
        let active = active.min(layers.len() - 1);
        a.layers.push((tex, LayerStack { layers, active }));
    }
    Ok(())
}
fn w_anims(p: &Project) -> Vec<u8> {
    let mut w = W::default();
    w.u32(p.animations.len() as u32);
    for a in &p.animations {
        w.str(&a.name);
        w.u32(a.fps_num);
        w.u32(a.fps_den);
        w.u32(a.frames.len() as u32);
        for f in &a.frames {
            w.u32(f.hold);
            w.u8(f.tween as u8 | (f.hand_only as u8) << 1);
            w.str(&f.note);
            w_camera(&mut w, &f.camera);
            w.bool(f.style.is_some());
            if let Some(s) = &f.style {
                w_style(&mut w, s);
            }
            w_opt_tex(&mut w, &f.hand_layer);
            w.u32(f.states.len() as u32);
            for s in &f.states {
                w.u32(s.object);
                w_transform(&mut w, &s.transform);
                w.bool(s.visible);
                w.opt_u32(s.emotion);
                w.u32(s.pose.bones.len() as u32);
                for b in &s.pose.bones {
                    w.v3(b.pos);
                    w.quat(b.rot);
                    w.v3(b.scale);
                }
                w.u32(s.tex_patch.len() as u32);
                for (i, t) in &s.tex_patch {
                    w.u32(*i as u32);
                    w_texture(&mut w, t);
                }
            }
        }
    }
    w.b
}
fn r_anims(d: &[u8]) -> Res<Vec<Animation>> {
    let mut r = R::new(d);
    let mut out = vec![];
    for _ in 0..r.count(16)? {
        let name = r.str()?;
        let (fps_num, fps_den) = (r.u32()?.max(1), r.u32()?.max(1));
        let mut frames = vec![];
        for _ in 0..r.count(40)? {
            let hold = r.u32()?.max(1);
            let fl = r.u8()?;
            let note = r.str()?;
            let camera = r_camera(&mut r)?;
            let style = if r.bool()? { Some(r_style(&mut r)?) } else { None };
            let hand_layer = r_opt_tex(&mut r)?;
            let mut states = vec![];
            for _ in 0..r.count(40)? {
                let object = r.u32()?;
                let transform = r_transform(&mut r)?;
                let visible = r.bool()?;
                let emotion = r.opt_u32()?;
                let mut bones = vec![];
                for _ in 0..r.count(40)? {
                    bones.push(BonePose { pos: r.v3()?, rot: r.quat()?, scale: r.v3()? });
                }
                let mut tex_patch = vec![];
                for _ in 0..r.count(13)? {
                    let i = r.u32()? as usize;
                    tex_patch.push((i, r_texture(&mut r)?));
                }
                states.push(ObjectState { object, transform, pose: Pose { bones }, visible, emotion, tex_patch });
            }
            frames.push(AnimFrame { states, camera, style, hold, tween: fl & 1 != 0, hand_layer, hand_only: fl & 2 != 0, note });
        }
        out.push(Animation { name, fps_num, fps_den, frames });
    }
    Ok(out)
}

// ------------------------------------------------------------------ container

fn put_chunk(out: &mut Vec<u8>, tag: &[u8; 4], raw: &[u8]) {
    let packed = deflate(raw);
    let (comp, data): (u8, &[u8]) = if packed.len() < raw.len() { (1, &packed) } else { (0, raw) };
    out.extend(tag);
    out.push(comp);
    out.extend((raw.len() as u32).to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(crc32(data).to_le_bytes());
    out.extend(data);
}

impl Project {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(MAGIC);
        out.extend(VERSION.to_le_bytes());
        out.extend([0u8, 0]);
        let mut meta = W::default();
        meta.str(&self.name);
        put_chunk(&mut out, b"META", &meta.b);
        put_chunk(&mut out, b"ASST", &w_assets(self));
        put_chunk(&mut out, b"SCEN", &w_scene(&self.scene));
        put_chunk(&mut out, b"ANIM", &w_anims(self));
        if let Some(l) = w_layers(self) {
            put_chunk(&mut out, b"LAYR", &l);
        }
        if let Some(l) = w_lighting(&self.scene.lighting) {
            put_chunk(&mut out, b"LITE", &l);
        }
        put_chunk(&mut out, b"END\0", &[]);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Res<Project> {
        if bytes.len() < 8 || &bytes[..4] != MAGIC {
            return Err(JankError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version == 0 || version > VERSION {
            return Err(JankError::UnsupportedVersion(version));
        }
        let mut r = R::new(&bytes[8..]);
        let (mut name, mut assets, mut scene, mut anims) = (None, None, None, None);
        let mut layer_data: Option<Vec<u8>> = None;
        let mut lite: Option<crate::lighting::Lighting> = None;
        loop {
            let tag: [u8; 4] = r.take(4)?.try_into().unwrap();
            let comp = r.u8()?;
            let raw_len = r.u32()? as usize;
            let stored_len = r.u32()? as usize;
            let crc = r.u32()?;
            if raw_len > MAX_CHUNK || stored_len > MAX_CHUNK {
                return Err(JankError::Corrupt("block size".into()));
            }
            let stored = r.take(stored_len)?;
            let tag_s = String::from_utf8_lossy(&tag).trim_end_matches('\0').to_string();
            if crc32(stored) != crc {
                return Err(JankError::Crc(tag_s));
            }
            if &tag == b"END\0" {
                break;
            }
            let data: Vec<u8> = match comp {
                0 => stored.to_vec(),
                1 => inflate(stored, raw_len).map_err(|e| JankError::Corrupt(format!("{tag_s}: {e}")))?,
                c => return Err(JankError::Corrupt(format!("unknown compression {c}"))),
            };
            if data.len() != raw_len {
                return Err(JankError::Corrupt(format!("{tag_s}: size after decompression")));
            }
            match &tag {
                b"META" => name = Some(R::new(&data).str()?),
                b"ASST" => assets = Some(r_assets(&data)?),
                b"SCEN" => scene = Some(r_scene(&data)?),
                b"ANIM" => anims = Some(r_anims(&data)?),
                b"LAYR" => layer_data = Some(data),
                b"LITE" => lite = Some(r_lighting(&data)?),
                _ => {} // a chunk from a newer version that this build does not know: skip it
            }
        }
        let mut p = Project {
            name: name.ok_or_else(|| JankError::Corrupt("missing META block".into()))?,
            assets: assets.ok_or_else(|| JankError::Corrupt("missing ASST block".into()))?,
            scene: scene.ok_or_else(|| JankError::Corrupt("missing SCEN block".into()))?,
            animations: anims.ok_or_else(|| JankError::Corrupt("missing ANIM block".into()))?,
        };
        if let Some(d) = layer_data {
            r_layers(&d, &mut p)?;
        }
        if let Some(l) = lite {
            p.scene.lighting = l;
        }
        p.validate()?;
        Ok(p)
    }

    /// References between parts of the project must be intact; otherwise rendering or editing would fail later and far from the cause.
    fn validate(&self) -> Res<()> {
        let bad = |s: String| Err(JankError::Corrupt(s));
        for o in &self.scene.objects {
            if o.asset >= self.assets.len() {
                return bad(format!("object \"{}\" refers to a model {} that does not exist", o.name, o.asset));
            }
        }
        for (ai, a) in self.assets.iter().enumerate() {
            for m in &a.model.meshes {
                if m.material >= a.model.materials.len() {
                    return bad(format!("model {ai}: material out of range"));
                }
            }
            for m in &a.model.materials {
                if m.texture.map_or(false, |t| t >= a.model.textures.len()) {
                    return bad(format!("model {ai}: material texture out of range"));
                }
            }
            for e in &a.emotions {
                if e.texture >= a.model.textures.len() {
                    return bad(format!("emotion \"{}\" refers to a texture that does not exist", e.name));
                }
            }
        }
        Ok(())
    }

    /// Atomic save: writes to a temporary file next to the target and renames it,
    /// so a failure in the middle of writing can never damage the previous project.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        use std::io::Write;
        let tmp = path.with_extension("jank.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&self.to_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)
    }
    pub fn load(path: &Path) -> Res<Project> {
        Project::from_bytes(&std::fs::read(path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modeling::*;

    fn sample() -> Project {
        let mut p = Project::new("Test ✦ project");
        let mut model = model_from_mesh("Hero", cube(Vec3::new(1.0, 2.0, 1.0)), 32);
        model.skeleton = Some(Skeleton {
            bones: vec![
                Bone { name: "root".into(), parent: None, rest_t: Vec3::ZERO, rest_r: Quat::IDENTITY, rest_s: Vec3::ONE },
                Bone { name: "head".into(), parent: Some(0), rest_t: Vec3::new(0.0, 1.0, 0.0), rest_r: Quat::IDENTITY, rest_s: Vec3::ONE },
            ],
        });
        model.meshes[0].verts[0].weights = [0.5, 0.5, 0.0, 0.0];
        model.meshes[0].verts[0].joints = [0, 1, 0, 0];
        let id = p.place_model(model);
        let mut happy = p.assets[0].model.textures[0].clone();
        happy.name = "happy".into();
        happy.set(3, 3, [255, 0, 255, 255]);
        p.assets[0].emotions.push(Emotion { name: "joy".into(), texture: 0, image: happy });
        let mut anim = Animation::from_scene("walk", &p.scene, &|a| p.bones_of(a));
        anim.frames[0].hand_layer = Some(Texture::new("hand", 8, 8, [10, 20, 30, 128]));
        anim.frames[0].states[0].emotion = Some(0);
        anim.frames[0].states[0].pose.bones[1].rot = Quat::from_axis_angle(Vec3::Y, 0.4);
        anim.frames[0].states[0].tex_patch.push((0, Texture::new("patch", 4, 4, [1, 2, 3, 255])));
        anim.frames[0].style = Some(PsxStyle::lofi_calm());
        anim.duplicate_frame(0);
        anim.frames[1].tween = true;
        anim.frames[1].hold = 4;
        anim.frames[1].note = "turn head".into();
        p.animations.push(anim);
        assert_eq!(p.scene.objects[0].id, id);
        p
    }

    #[test]
    fn full_roundtrip_is_lossless() {
        let p = sample();
        let bytes = p.to_bytes();
        let q = Project::from_bytes(&bytes).expect("read back");
        assert_eq!(p, q);
        assert_eq!(bytes, q.to_bytes(), "saving again gives the same file (deterministic output)");
    }
    #[test]
    fn layers_roundtrip_and_old_files_have_no_layer_block() {
        use crate::layers::{Layer, LayerStack};
        let plain = sample();
        assert!(!plain.to_bytes().windows(4).any(|w| w == b"LAYR"), "a project without layers is written exactly as before");
        let mut p = sample();
        let mut sky = Layer::new("Sky", 16, 9);
        sky.img.set(3, 3, [10, 20, 30, 255]);
        sky.opacity = 200;
        let mut fg = Layer::new("Fog", 16, 9);
        fg.visible = false;
        fg.locked = true;
        p.scene.layers = vec![sky, fg];
        p.scene.layers_back = 1;
        let tex = p.assets[0].model.textures[0].clone();
        let mut st = LayerStack::single(tex);
        st.add("Detail");
        st.layers[1].img.set(0, 0, [1, 2, 3, 255]);
        p.assets[0].layers.push((0, st));
        let bytes = p.to_bytes();
        let q = Project::from_bytes(&bytes).expect("read back");
        assert_eq!(p, q);
        assert_eq!(bytes, q.to_bytes());
    }
    #[test]
    fn lighting_roundtrip_and_default_is_not_written() {
        use crate::lighting::ShadowMode;
        let plain = sample();
        assert!(!plain.to_bytes().windows(4).any(|w| w == b"LITE"));
        let mut p = sample();
        p.scene.lighting.tod_on = true;
        p.scene.lighting.time = 17.5;
        p.scene.lighting.azimuth = 120.0;
        p.scene.lighting.shadows = ShadowMode::Projected;
        p.scene.lighting.shadow_strength = 0.7;
        p.scene.lighting.quality = crate::lighting::ShadowQuality::High;
        p.scene.lighting.softness = 0.8;
        let bytes = p.to_bytes();
        let q = Project::from_bytes(&bytes).expect("read back");
        assert_eq!(p, q);
        assert_eq!(bytes, q.to_bytes());
    }
    #[test]
    fn file_is_compact() {
        let p = sample();
        // a 32x32 checker in palette mode plus deflate is an order of magnitude smaller than raw RGBA
        assert!(p.to_bytes().len() < 20_000, "{}", p.to_bytes().len());
    }
    #[test]
    fn rejects_garbage_and_truncation_without_panic() {
        assert!(matches!(Project::from_bytes(b"nope"), Err(JankError::BadMagic)));
        assert!(matches!(Project::from_bytes(b"PK\x03\x04aaaaaaaa"), Err(JankError::BadMagic)));
        let good = sample().to_bytes();
        for cut in [8, 20, good.len() / 2, good.len() - 1] {
            assert!(Project::from_bytes(&good[..cut]).is_err(), "truncation at {cut} must be an error");
        }
    }
    #[test]
    fn crc_detects_bit_flip() {
        let mut b = sample().to_bytes();
        let mid = b.len() / 2;
        b[mid] ^= 0x55;
        assert!(Project::from_bytes(&b).is_err());
    }
    #[test]
    fn newer_version_gives_clear_error_and_unknown_chunks_are_skipped() {
        let mut b = sample().to_bytes();
        b[4] = 9;
        match Project::from_bytes(&b) {
            Err(JankError::UnsupportedVersion(9)) => {}
            other => panic!("{other:?}"),
        }
        // insert an unknown chunk "FUTR" before END: the reader must skip it
        let good = sample().to_bytes();
        let end_at = good.len() - (4 + 1 + 4 + 4 + 4);
        let mut with = good[..end_at].to_vec();
        put_chunk(&mut with, b"FUTR", b"future data from jank 2");
        with.extend(&good[end_at..]);
        assert_eq!(Project::from_bytes(&with).unwrap(), sample());
    }
    #[test]
    fn hostile_counts_do_not_allocate_or_panic() {
        // META is valid, ASST claims 4 billion models
        let mut out = Vec::new();
        out.extend(MAGIC);
        out.extend(VERSION.to_le_bytes());
        out.extend([0, 0]);
        let mut meta = W::default();
        meta.str("x");
        put_chunk(&mut out, b"META", &meta.b);
        put_chunk(&mut out, b"ASST", &u32::MAX.to_le_bytes());
        put_chunk(&mut out, b"END\0", &[]);
        assert!(matches!(Project::from_bytes(&out), Err(JankError::Corrupt(_))));
    }
    #[test]
    fn dangling_references_are_rejected() {
        let mut p = sample();
        p.scene.objects[0].asset = 7;
        assert!(Project::from_bytes(&p.to_bytes()).is_err());
    }
    #[test]
    fn atomic_save_and_load() {
        let dir = std::env::temp_dir().join(format!("jank_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.jank");
        let p = sample();
        p.save(&path).unwrap();
        assert!(!dir.join("a.jank.tmp").exists(), "the temporary file is removed");
        assert_eq!(Project::load(&path).unwrap(), p);
        std::fs::remove_dir_all(&dir).ok();
    }
}
