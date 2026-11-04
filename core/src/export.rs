//! Output layer: still images in any format and resolution, video through sinks, and model export to OBJ.
//! The working project lives in a `.jank` file; everything here produces one-off "releases" for the outside world.
use crate::anim::Animation;
use crate::compose::*;
use crate::encode::{jpeg::encode_jpeg, png::encode_png, simple::*, sinks::VideoSink};
use crate::mesh::Model;
use crate::pose::Pose;
use crate::project::Project;
use crate::raster::{skin_model, Renderer};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageFormat {
    Png,
    /// PNG without an alpha channel (smaller file).
    PngRgb,
    Jpeg(u8),
    Bmp,
    Tga,
    Ppm,
}
impl ImageFormat {
    /// File extension (without the dot) for this format; both PNG variants give `"png"`.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png | ImageFormat::PngRgb => "png",
            ImageFormat::Jpeg(_) => "jpg",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tga => "tga",
            ImageFormat::Ppm => "ppm",
        }
    }
    /// Maps a file extension (case-insensitive; `png`, `jpg`/`jpeg`, `bmp`, `tga`, `ppm`) to a format,
    /// or `None` if unknown. `jpeg_quality` is used for JPEG. `png` yields `Png` (with alpha).
    pub fn from_extension(ext: &str, jpeg_quality: u8) -> Option<Self> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "png" => ImageFormat::Png,
            "jpg" | "jpeg" => ImageFormat::Jpeg(jpeg_quality),
            "bmp" => ImageFormat::Bmp,
            "tga" => ImageFormat::Tga,
            "ppm" => ImageFormat::Ppm,
            _ => return None,
        })
    }
}

/// Encodes tightly packed RGBA8 pixels (`w * h * 4` bytes) into the given image format.
pub fn encode_image(fmt: ImageFormat, w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    match fmt {
        ImageFormat::Png => encode_png(w, h, rgba, true),
        ImageFormat::PngRgb => encode_png(w, h, rgba, false),
        ImageFormat::Jpeg(q) => encode_jpeg(w, h, rgba, q),
        ImageFormat::Bmp => encode_bmp(w, h, rgba),
        ImageFormat::Tga => encode_tga(w, h, rgba),
        ImageFormat::Ppm => encode_ppm(w, h, rgba),
    }
}

/// Maximum output width/height in pixels, so a typo (e.g. "1920000") cannot eat all the memory.
pub const MAX_OUTPUT_SIDE: u32 = 16384;

/// Validates an output resolution. Fails with `InvalidInput` if either side is zero or exceeds
/// `MAX_OUTPUT_SIDE`, or if the total pixel count is above 200 million.
pub fn check_size(w: u32, h: u32) -> io::Result<()> {
    if w == 0 || h == 0 || w > MAX_OUTPUT_SIDE || h > MAX_OUTPUT_SIDE || (w as u64 * h as u64) > 200_000_000 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid resolution {w}×{h} (maximum {MAX_OUTPUT_SIDE} per side)")));
    }
    Ok(())
}

/// Renders a still of one animation tick at any resolution: the frame is rendered at the scene's
/// "native" PSX resolution and then scaled with nearest-neighbour filtering (pixels stay square and crisp).
/// Returns the encoded image bytes. Fails with `InvalidInput` if the resolution is invalid
/// (see `check_size`) or the animation has no frames.
pub fn render_photo(project: &Project, anim: &Animation, tick: u32, out_w: u32, out_h: u32, fmt: ImageFormat) -> io::Result<Vec<u8>> {
    check_size(out_w, out_h)?;
    let mut r = Renderer::new(1, 1);
    if !render_tick(project, anim, tick, &mut r) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "The animation has no frames"));
    }
    let px = r.frame.resized_nearest(out_w, out_h);
    let flat: Vec<u8> = px.iter().flat_map(|p| *p).collect();
    Ok(encode_image(fmt, out_w, out_h, &flat))
}

/// Bakes the whole animation into a video sink, one rendered frame per tick, scaled to
/// `out_w` x `out_h` with nearest-neighbour filtering. `progress(done, total)` is called after each
/// frame and returns `false` to cancel. Returns the number of frames written (fewer than the total
/// if cancelled). Fails with `InvalidInput` for an invalid resolution or an animation with no
/// frames, and propagates sink I/O errors.
pub fn bake_video(project: &Project, anim: &Animation, out_w: u32, out_h: u32, sink: &mut dyn VideoSink, progress: &mut dyn FnMut(u32, u32) -> bool) -> io::Result<u32> {
    check_size(out_w, out_h)?;
    let total = anim.total_ticks();
    let mut r = Renderer::new(1, 1);
    for tick in 0..total {
        if !render_tick(project, anim, tick, &mut r) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "The animation has no frames"));
        }
        let px = r.frame.resized_nearest(out_w, out_h);
        let flat: Vec<u8> = px.iter().flat_map(|p| *p).collect();
        sink.add_frame(&flat)?;
        if !progress(tick + 1, total) {
            return Ok(tick + 1);
        }
    }
    Ok(total)
}

// ------------------------------------------------------------------ OBJ

pub struct ObjExport {
    pub obj: String,
    pub mtl: String,
    /// (file name, PNG bytes): the textures referenced by the MTL.
    pub textures: Vec<(String, Vec<u8>)>,
}

fn safe_name(s: &str) -> String {
    let n: String = s.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    if n.is_empty() { "unnamed".into() } else { n }
}

/// Exports a model to Wavefront OBJ + MTL + PNG textures. With a `pose`, vertices are taken already
/// posed (the pose is baked in). `base_name` is sanitised and used for the `mtllib` and texture
/// file names. OBJ indices are 1-based and global across meshes; the V coordinate is flipped (1 - v).
pub fn export_obj(model: &Model, pose: Option<&Pose>, base_name: &str) -> ObjExport {
    let base = safe_name(base_name);
    let (pos, nrm) = skin_model(model, pose);
    let mut obj = format!("# Jank! export\nmtllib {base}.mtl\n");
    let mut mtl = String::from("# Jank! export\n");
    let mut textures = Vec::new();
    for (i, m) in model.materials.iter().enumerate() {
        let name = format!("{}_{}", safe_name(&m.name), i);
        mtl += &format!("newmtl {name}\nKd {:.4} {:.4} {:.4}\n", m.color[0], m.color[1], m.color[2]);
        if let Some(t) = m.texture.and_then(|t| model.textures.get(t)) {
            let file = format!("{}_{}.png", base, safe_name(&t.name));
            mtl += &format!("map_Kd {file}\n");
            if !textures.iter().any(|(f, _): &(String, Vec<u8>)| *f == file) {
                textures.push((file, encode_png(t.w, t.h, &t.to_rgba_bytes(), true)));
            }
        }
        mtl += "\n";
    }
    let (mut vo, mut to, mut no) = (1usize, 1usize, 1usize); // OBJ indices start at 1 and are global
    for (mi, mesh) in model.meshes.iter().enumerate() {
        obj += &format!("o {}\n", safe_name(&mesh.name));
        for p in &pos[mi] {
            obj += &format!("v {:.6} {:.6} {:.6}\n", p.x, p.y, p.z);
        }
        for v in &mesh.verts {
            obj += &format!("vt {:.6} {:.6}\n", v.uv.x, 1.0 - v.uv.y);
        }
        for n in &nrm[mi] {
            obj += &format!("vn {:.6} {:.6} {:.6}\n", n.x, n.y, n.z);
        }
        obj += &format!("usemtl {}_{}\n", safe_name(&model.materials.get(mesh.material).map_or("Material", |m| m.name.as_str())), mesh.material);
        for t in &mesh.tris {
            let f = |k: usize| format!("{}/{}/{}", vo + t[k] as usize, to + t[k] as usize, no + t[k] as usize);
            obj += &format!("f {} {} {}\n", f(0), f(1), f(2));
        }
        vo += mesh.verts.len();
        to += mesh.verts.len();
        no += mesh.verts.len();
    }
    ObjExport { obj, mtl, textures }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::*;
    use crate::anim::Animation;
    use crate::encode::sinks::MemorySink;
    use crate::import::parse_obj;
    use crate::modeling::*;
    use crate::raster::{Camera, PsxStyle};

    fn project() -> Project {
        let mut p = Project::new("t");
        p.place_model(model_from_mesh("cube", cube(Vec3::ONE), 16));
        p.scene.style = PsxStyle { width: 48, height: 32, ..PsxStyle::ps1_classic() };
        p.scene.camera = Camera { eye: Vec3::new(2.0, 1.5, 3.0), target: Vec3::ZERO, ..Default::default() };
        p
    }
    #[test]
    fn photo_any_resolution_and_format() {
        let p = project();
        let a = Animation::from_scene("a", &p.scene, &|x| p.bones_of(x));
        for (fmt, magic) in [(ImageFormat::Png, &b"\x89PNG"[..]), (ImageFormat::Jpeg(90), &b"\xFF\xD8"[..]), (ImageFormat::Bmp, &b"BM"[..]), (ImageFormat::Ppm, &b"P6"[..])] {
            let b = render_photo(&p, &a, 0, 200, 150, fmt).unwrap();
            assert!(b.starts_with(magic), "{fmt:?}");
        }
        assert!(render_photo(&p, &a, 0, 0, 10, ImageFormat::Png).is_err());
        assert!(render_photo(&p, &a, 0, 100_000, 10, ImageFormat::Png).is_err());
    }
    #[test]
    fn bake_counts_ticks_and_cancels() {
        let p = project();
        let mut a = Animation::from_scene("a", &p.scene, &|x| p.bones_of(x));
        a.frames[0].hold = 5;
        let mut sink = MemorySink::default();
        let n = bake_video(&p, &a, 96, 64, &mut sink, &mut |_, _| true).unwrap();
        assert_eq!((n, sink.frames.len()), (5, 5));
        assert_eq!(sink.frames[0].len(), 96 * 64 * 4);
        let mut s2 = MemorySink::default();
        let n2 = bake_video(&p, &a, 96, 64, &mut s2, &mut |done, _| done < 2).unwrap();
        assert_eq!(n2, 2, "cancelling stops the bake");
    }
    #[test]
    fn obj_roundtrip_through_importer() {
        let mut m = model_from_mesh("Hero cube!", cube(Vec3::new(1.0, 2.0, 3.0)), 16);
        m.meshes[0].name = "Body".into();
        let e = export_obj(&m, None, "my model");
        assert_eq!(e.textures.len(), 1);
        assert!(e.textures[0].1.starts_with(b"\x89PNG"));
        let back = parse_obj(&e.obj, Some(&e.mtl)).unwrap();
        assert_eq!(back.model.triangle_count(), m.triangle_count());
        assert_eq!(back.texture_files.len(), 1);
        let (lo, hi) = back.model.bounds().unwrap();
        assert!((hi.y - lo.y - 2.0).abs() < 1e-4 && (hi.z - lo.z - 3.0).abs() < 1e-4);
    }
}
