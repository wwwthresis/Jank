//! Dependency-free model importers: OBJ (+MTL), STL (ASCII and binary) and PLY (ASCII and binary
//! little-endian). glTF/GLB lives in the `gltf` module. `load_from_path` ties everything together
//! (MTL files, texture images and external glTF buffers).
use crate::math::*;
use crate::mesh::*;
use std::collections::HashMap;

/// Result of parsing an in-memory model file.
pub struct ObjResult {
    /// The parsed model.
    pub model: Model,
    /// (material index, diffuse texture file name from the MTL); the caller loads the images itself.
    pub texture_files: Vec<(usize, String)>,
}

/// Parses a model from memory, choosing the format from the file extension `ext` (without the dot,
/// case-insensitive): `obj`, `stl`, `ply`, `glb` or `gltf`.
///
/// `mtl` is the optional text of the OBJ's material library (used only for OBJ). Texture images and
/// external glTF buffers are NOT loaded here (for glTF every external URI resolves to nothing); use
/// [`load_from_path`] for that.
/// Fails with the parser's message, or with "Unsupported format" for any other extension.
pub fn load_from_bytes(ext: &str, bytes: &[u8], mtl: Option<&str>) -> Result<ObjResult, String> {
    match ext.to_ascii_lowercase().as_str() {
        "obj" => parse_obj(&String::from_utf8_lossy(bytes), mtl),
        "stl" => parse_stl(bytes).map(|m| ObjResult { model: m, texture_files: vec![] }),
        "ply" => parse_ply(bytes).map(|m| ObjResult { model: m, texture_files: vec![] }),
        "glb" | "gltf" => crate::gltf::load_gltf(bytes, &|_| None).map(|r| ObjResult { model: r.model, texture_files: vec![] }),
        other => Err(format!("Unsupported format .{other} (supported: OBJ, STL, PLY, glTF/GLB)")),
    }
}

/// Result of loading a file from disk.
pub struct Loaded {
    /// The loaded model.
    pub model: Model,
    /// Non-fatal problems (for example textures that could not be loaded), to show to the user.
    pub warnings: Vec<String>,
}

/// Loads a model from disk, also reading the `.mtl` file, texture images and external glTF files
/// that sit next to it.
///
/// Only files in the model's own folder (or its subfolders) are read: absolute paths and paths
/// containing `..` are refused. Missing or undecodable textures become warnings in
/// [`Loaded::warnings`]. If the model comes out unnamed (or with the default glTF name), the file name without its
/// extension is used as the model name.
/// Fails if the file cannot be read, or with the parser's error message.
pub fn load_from_path(path: &std::path::Path) -> Result<Loaded, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let bytes = std::fs::read(path).map_err(|e| format!("couldn't read \"{}\": {e}", path.display()))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model").to_string();
    // Files next to the model. Absolute paths and paths with ".." are refused, so a model file can't make the app read unrelated files.
    let read_sibling = |rel: &str| -> Option<Vec<u8>> {
        let rel = rel.replace('\\', "/");
        let p = std::path::Path::new(&rel);
        if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return None;
        }
        std::fs::read(dir.join(p)).ok()
    };
    let mut warnings = vec![];
    let mut model = match ext.as_str() {
        "glb" | "gltf" => {
            let r = crate::gltf::load_gltf(&bytes, &read_sibling)?;
            warnings = r.warnings;
            r.model
        }
        "obj" => {
            let mtl_name = String::from_utf8_lossy(&bytes).lines().find_map(|l| l.trim().strip_prefix("mtllib ").map(|s| s.trim().to_string()));
            let mtl = mtl_name.and_then(|n| read_sibling(&n)).map(|b| String::from_utf8_lossy(&b).into_owned());
            let mut r = parse_obj(&String::from_utf8_lossy(&bytes), mtl.as_deref())?;
            for (mat, file) in std::mem::take(&mut r.texture_files) {
                match read_sibling(&file).ok_or_else(|| "file not found next to the model".to_string()).and_then(|b| crate::decode::image::decode_image(&file, &b)) {
                    Ok(img) => {
                        let tex = crate::texture::Texture { name: file.clone(), w: img.w, h: img.h, px: img.rgba.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect() };
                        r.model.textures.push(tex);
                        let ti = r.model.textures.len() - 1;
                        if let Some(m) = r.model.materials.get_mut(mat) {
                            m.texture = Some(ti);
                        }
                    }
                    Err(e) => warnings.push(format!("texture \"{file}\": {e}")),
                }
            }
            r.model
        }
        _ => load_from_bytes(&ext, &bytes, None)?.model,
    };
    if model.name.is_empty() || model.name == "glTF" {
        model.name = stem;
    }
    Ok(Loaded { model, warnings })
}

// ---------------------------------------------------------------- OBJ

fn parse_mtl(text: &str) -> Vec<(String, [f32; 3], Option<String>)> {
    let mut out: Vec<(String, [f32; 3], Option<String>)> = vec![];
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("newmtl") => out.push((it.collect::<Vec<_>>().join(" "), [1.0; 3], None)),
            Some("Kd") => {
                if let Some(m) = out.last_mut() {
                    let v: Vec<f32> = it.filter_map(|s| s.parse().ok()).collect();
                    if v.len() >= 3 {
                        m.1 = [v[0], v[1], v[2]];
                    }
                }
            }
            Some("map_Kd") => {
                if let Some(m) = out.last_mut() {
                    // the last token is the file name (options such as -s before it are ignored)
                    m.2 = it.last().map(|s| s.replace('\\', "/"));
                }
            }
            _ => {}
        }
    }
    out
}

/// Parses OBJ text (with optional MTL text) into a model.
///
/// Supports `v`, `vt`, `vn`, `f` (any polygon, fan-triangulated; negative indices allowed), `o`/`g`
/// and `usemtl`. One mesh is created per (material, object name) pair. The V texture coordinate is
/// flipped (V=0 is the top of the image). Normals missing from the file are recomputed. Kd and
/// map_Kd from the MTL give the material colour and the texture file name.
/// Fails (with the line number) on a vertex with fewer than 3 coordinates, a bad or out-of-range
/// face index, or when the file has no faces.
pub fn parse_obj(text: &str, mtl: Option<&str>) -> Result<ObjResult, String> {
    let mats = mtl.map(parse_mtl).unwrap_or_default();
    let mut model = Model { name: "OBJ".into(), ..Default::default() };
    let mut texture_files = vec![];
    for (i, (name, col, tex)) in mats.iter().enumerate() {
        model.materials.push(Material { name: name.clone(), color: *col, ..Default::default() });
        if let Some(t) = tex {
            texture_files.push((i, t.clone()));
        }
    }
    let (mut vs, mut vts, mut vns): (Vec<Vec3>, Vec<Vec2>, Vec<Vec3>) = (vec![], vec![], vec![]);
    // one mesh per material
    let mut cur_mat: Option<usize> = None;
    let mut cur_name = String::from("mesh");
    let mut meshes: HashMap<(Option<usize>, String), usize> = HashMap::new();
    let mut dedupe: Vec<HashMap<(i64, i64, i64), u32>> = vec![];

    let resolve = |s: i64, len: usize| -> Option<usize> {
        let i = if s < 0 { len as i64 + s } else { s - 1 };
        (i >= 0 && (i as usize) < len).then_some(i as usize)
    };
    let f32s = |it: std::str::SplitWhitespace| -> Vec<f32> { it.filter_map(|s| s.parse().ok()).collect() };

    for (ln, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        match it.next().unwrap() {
            "v" => {
                let v = f32s(it);
                if v.len() < 3 {
                    return Err(format!("OBJ line {}: vertex needs 3 coordinates", ln + 1));
                }
                vs.push(Vec3::new(v[0], v[1], v[2]));
            }
            "vt" => {
                let v = f32s(it);
                vts.push(Vec2::new(*v.first().unwrap_or(&0.0), 1.0 - *v.get(1).unwrap_or(&0.0)));
            }
            "vn" => {
                let v = f32s(it);
                if v.len() >= 3 {
                    vns.push(Vec3::new(v[0], v[1], v[2]));
                }
            }
            "o" | "g" => cur_name = it.collect::<Vec<_>>().join(" "),
            "usemtl" => {
                let n = it.collect::<Vec<_>>().join(" ");
                cur_mat = model.materials.iter().position(|m| m.name == n);
                if cur_mat.is_none() {
                    model.materials.push(Material { name: n, ..Default::default() });
                    cur_mat = Some(model.materials.len() - 1);
                }
            }
            "f" => {
                let mesh_idx = *meshes.entry((cur_mat, cur_name.clone())).or_insert_with(|| {
                    let mut m = Mesh::new(&cur_name);
                    m.material = cur_mat.unwrap_or(0);
                    model.meshes.push(m);
                    dedupe.push(HashMap::new());
                    model.meshes.len() - 1
                });
                let mut face: Vec<u32> = vec![];
                for tok in it {
                    let mut p = tok.split('/');
                    let vi: i64 = p.next().and_then(|s| s.parse().ok()).ok_or_else(|| format!("OBJ line {}: bad index", ln + 1))?;
                    let ti: i64 = p.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let ni: i64 = p.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let key = (vi, ti, ni);
                    let idx = if let Some(&i) = dedupe[mesh_idx].get(&key) {
                        i
                    } else {
                        let pos = resolve(vi, vs.len()).map(|i| vs[i]).ok_or_else(|| format!("OBJ line {}: vertex index out of range", ln + 1))?;
                        let mut v = Vertex::new(pos, Vec2::default());
                        if ti != 0 {
                            if let Some(i) = resolve(ti, vts.len()) {
                                v.uv = vts[i];
                            }
                        }
                        v.nrm = if ni != 0 { resolve(ni, vns.len()).map(|i| vns[i]).unwrap_or(Vec3::ZERO) } else { Vec3::ZERO };
                        let m = &mut model.meshes[mesh_idx];
                        m.verts.push(v);
                        let i = (m.verts.len() - 1) as u32;
                        dedupe[mesh_idx].insert(key, i);
                        i
                    };
                    face.push(idx);
                }
                if face.len() >= 3 {
                    for k in 1..face.len() - 1 {
                        model.meshes[mesh_idx].tris.push([face[0], face[k], face[k + 1]]);
                    }
                }
            }
            _ => {}
        }
    }
    if model.meshes.is_empty() {
        return Err("OBJ has no faces".into());
    }
    for m in &mut model.meshes {
        if m.verts.iter().any(|v| v.nrm == Vec3::ZERO) {
            m.recompute_normals();
        }
    }
    model.ensure_material();
    Ok(ObjResult { model, texture_files })
}

// ---------------------------------------------------------------- STL

/// Parses an STL file, either binary (80-byte header, u32 triangle count, then 50 bytes per
/// triangle) or ASCII. The file is treated as binary only when its length matches the stored
/// triangle count exactly. Stored normals are ignored; identical vertices are welded and normals
/// are recomputed. Fails if the file contains no triangles.
pub fn parse_stl(bytes: &[u8]) -> Result<Model, String> {
    let mut tris_pos: Vec<[Vec3; 3]> = vec![];
    let is_binary = bytes.len() >= 84 && {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        bytes.len() == 84 + n * 50
    };
    if is_binary {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        let f = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        for i in 0..n {
            let b = 84 + i * 50 + 12; // skip the normal
            let v = |k: usize| Vec3::new(f(b + k * 12), f(b + k * 12 + 4), f(b + k * 12 + 8));
            tris_pos.push([v(0), v(1), v(2)]);
        }
    } else {
        let text = String::from_utf8_lossy(bytes);
        let mut cur: Vec<Vec3> = vec![];
        for line in text.lines() {
            let mut it = line.split_whitespace();
            if it.next() == Some("vertex") {
                let v: Vec<f32> = it.filter_map(|s| s.parse().ok()).collect();
                if v.len() == 3 {
                    cur.push(Vec3::new(v[0], v[1], v[2]));
                    if cur.len() == 3 {
                        tris_pos.push([cur[0], cur[1], cur[2]]);
                        cur.clear();
                    }
                }
            }
        }
    }
    if tris_pos.is_empty() {
        return Err("STL has no triangles".into());
    }
    let mut mesh = Mesh::new("stl");
    let mut weld: HashMap<[u32; 3], u32> = HashMap::new();
    for t in tris_pos {
        let mut idx = [0u32; 3];
        for (k, p) in t.iter().enumerate() {
            let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
            idx[k] = *weld.entry(key).or_insert_with(|| {
                mesh.verts.push(Vertex::new(*p, Vec2::default()));
                (mesh.verts.len() - 1) as u32
            });
        }
        mesh.tris.push(idx);
    }
    mesh.cleanup();
    mesh.recompute_normals();
    let mut m = Model { name: "STL".into(), meshes: vec![mesh], ..Default::default() };
    m.ensure_material();
    Ok(m)
}

// ---------------------------------------------------------------- PLY

#[derive(Clone, Copy, PartialEq)]
enum Ty {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}
impl Ty {
    fn parse(s: &str) -> Option<Ty> {
        Some(match s {
            "char" | "int8" => Ty::I8,
            "uchar" | "uint8" => Ty::U8,
            "short" | "int16" => Ty::I16,
            "ushort" | "uint16" => Ty::U16,
            "int" | "int32" => Ty::I32,
            "uint" | "uint32" => Ty::U32,
            "float" | "float32" => Ty::F32,
            "double" | "float64" => Ty::F64,
            _ => return None,
        })
    }
    fn size(self) -> usize {
        match self {
            Ty::I8 | Ty::U8 => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 => 4,
            Ty::F64 => 8,
        }
    }
}
struct Prop {
    name: String,
    ty: Ty,
    list: Option<Ty>, // type of the count field for list properties
}
struct Elem {
    name: String,
    count: usize,
    props: Vec<Prop>,
}

/// Parses a PLY file in `ascii` or `binary_little_endian` format.
///
/// Reads the `vertex` element (x, y, z, nx, ny, nz, s/u/texture_u, t/v/texture_v, red, green, blue;
/// 8-bit colours are scaled to 0..1, V is flipped) and the `face` element's `vertex_indices` (or
/// `vertex_index`) list, fan-triangulated. Normals are recomputed when absent.
/// Fails on a missing `end_header`, a bad element count, an unsupported format, truncated or
/// malformed data, no faces, or a face index out of range.
pub fn parse_ply(bytes: &[u8]) -> Result<Model, String> {
    let hdr_end = bytes.windows(10).position(|w| w == b"end_header").ok_or("PLY: missing end_header")?;
    let mut body = hdr_end + 10;
    while body < bytes.len() && bytes[body] != b'\n' {
        body += 1;
    }
    body += 1;
    let header = String::from_utf8_lossy(&bytes[..hdr_end]);
    let (mut fmt, mut elems) = (String::new(), Vec::<Elem>::new());
    for line in header.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        match t.as_slice() {
            ["format", f, _] => fmt = f.to_string(),
            ["element", n, c] => elems.push(Elem { name: n.to_string(), count: c.parse().map_err(|_| "PLY: bad element count")?, props: vec![] }),
            ["property", "list", ct, it, n] => {
                let (c, i) = (Ty::parse(ct), Ty::parse(it));
                if let (Some(c), Some(i), Some(e)) = (c, i, elems.last_mut()) {
                    e.props.push(Prop { name: n.to_string(), ty: i, list: Some(c) });
                }
            }
            ["property", ty, n] => {
                if let (Some(t), Some(e)) = (Ty::parse(ty), elems.last_mut()) {
                    e.props.push(Prop { name: n.to_string(), ty: t, list: None });
                }
            }
            _ => {}
        }
    }
    let ascii = fmt == "ascii";
    if !ascii && fmt != "binary_little_endian" {
        return Err(format!("PLY format '{fmt}' is not supported (use ascii or binary_little_endian)"));
    }
    let data = &bytes[body.min(bytes.len())..];
    let text = if ascii { String::from_utf8_lossy(data).into_owned() } else { String::new() };
    let mut toks = text.split_whitespace();
    let mut pos = 0usize;

    let mut read = |ty: Ty| -> Result<f64, String> {
        if ascii {
            toks.next().ok_or("PLY: unexpected end of data")?.parse::<f64>().map_err(|_| "PLY: bad number".to_string())
        } else {
            let s = ty.size();
            let b = data.get(pos..pos + s).ok_or("PLY: unexpected end of data")?;
            pos += s;
            Ok(match ty {
                Ty::I8 => b[0] as i8 as f64,
                Ty::U8 => b[0] as f64,
                Ty::I16 => i16::from_le_bytes([b[0], b[1]]) as f64,
                Ty::U16 => u16::from_le_bytes([b[0], b[1]]) as f64,
                Ty::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                Ty::U32 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                Ty::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                Ty::F64 => f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
            })
        }
    };

    let mut mesh = Mesh::new("ply");
    for e in &elems {
        for _ in 0..e.count {
            let mut v = Vertex::new(Vec3::ZERO, Vec2::default());
            let mut face: Vec<u32> = vec![];
            let mut has_n = false;
            for p in &e.props {
                if let Some(ct) = p.list {
                    let n = read(ct)? as usize;
                    let mut ids = Vec::with_capacity(n);
                    for _ in 0..n {
                        ids.push(read(p.ty)? as u32);
                    }
                    if e.name == "face" && (p.name == "vertex_indices" || p.name == "vertex_index") {
                        face = ids;
                    }
                } else {
                    let val = read(p.ty)? as f32;
                    if e.name == "vertex" {
                        let c8 = |x: f32| if p.ty == Ty::U8 { x / 255.0 } else { x };
                        match p.name.as_str() {
                            "x" => v.pos.x = val,
                            "y" => v.pos.y = val,
                            "z" => v.pos.z = val,
                            "nx" => {
                                v.nrm.x = val;
                                has_n = true
                            }
                            "ny" => v.nrm.y = val,
                            "nz" => v.nrm.z = val,
                            "s" | "u" | "texture_u" => v.uv.x = val,
                            "t" | "v" | "texture_v" => v.uv.y = 1.0 - val,
                            "red" => v.color[0] = c8(val),
                            "green" => v.color[1] = c8(val),
                            "blue" => v.color[2] = c8(val),
                            _ => {}
                        }
                    }
                }
            }
            if e.name == "vertex" {
                if !has_n {
                    v.nrm = Vec3::ZERO;
                }
                mesh.verts.push(v);
            } else if e.name == "face" && face.len() >= 3 {
                for k in 1..face.len() - 1 {
                    mesh.tris.push([face[0], face[k], face[k + 1]]);
                }
            }
        }
    }
    if mesh.tris.is_empty() {
        return Err("PLY has no faces".into());
    }
    if mesh.tris.iter().flatten().any(|&i| i as usize >= mesh.verts.len()) {
        return Err("PLY: vertex index out of range".into());
    }
    if mesh.verts.iter().any(|v| v.nrm == Vec3::ZERO) {
        mesh.recompute_normals();
    }
    let mut m = Model { name: "PLY".into(), meshes: vec![mesh], ..Default::default() };
    m.ensure_material();
    Ok(m)
}

#[cfg(test)]
mod tests {
    #[test]
    fn load_from_path_reads_mtl_texture_and_blocks_path_escape() {
        let dir = std::env::temp_dir().join(format!("jank_imp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = crate::encode::png::encode_png(2, 2, &[255, 0, 0, 255].repeat(4), true);
        std::fs::write(dir.join("t.png"), png).unwrap();
        std::fs::write(dir.join("m.mtl"), "newmtl a\nKd 1 1 1\nmap_Kd t.png\nnewmtl b\nmap_Kd ../secret.png\n").unwrap();
        std::fs::write(dir.join("m.obj"), "mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nusemtl a\nf 1/1 2/2 3/3\nusemtl b\nf 1/1 3/3 2/2\n").unwrap();
        let l = load_from_path(&dir.join("m.obj")).unwrap();
        assert_eq!(l.model.textures.len(), 1);
        assert_eq!(l.model.textures[0].px[0], [255, 0, 0, 255]);
        assert!(l.warnings.iter().any(|w| w.contains("secret.png")), "{:?}", l.warnings);
        assert!(load_from_path(&dir.join("nope.obj")).is_err());
        assert!(load_from_path(&dir.join("t.png")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;
    #[test]
    fn obj_quad_with_uv_and_negative_idx() {
        let src = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nf 1/1 2/2 3/3 4/4\nf -4 -3 -2\n";
        let r = parse_obj(src, None).unwrap();
        assert_eq!(r.model.triangle_count(), 3);
        // UV v is flipped: vt (0,0) -> v=1
        assert!((r.model.meshes[0].verts[0].uv.y - 1.0).abs() < 1e-6);
    }
    #[test]
    fn obj_mtl_materials() {
        let obj = "usemtl red\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let mtl = "newmtl red\nKd 1 0 0\nmap_Kd tex/a.png\n";
        let r = parse_obj(obj, Some(mtl)).unwrap();
        assert_eq!(r.model.materials[0].color, [1.0, 0.0, 0.0]);
        assert_eq!(r.texture_files, vec![(0, "tex/a.png".to_string())]);
    }
    #[test]
    fn obj_bad_index_errors() {
        assert!(parse_obj("v 0 0 0\nf 1 2 3\n", None).is_err());
    }
    #[test]
    fn stl_ascii_and_binary_weld() {
        let a = "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nfacet normal 0 0 1\nouter loop\nvertex 1 0 0\nvertex 1 1 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n";
        let m = parse_stl(a.as_bytes()).unwrap();
        assert_eq!((m.triangle_count(), m.vertex_count()), (2, 4));
        let mut b = vec![0u8; 80];
        b.extend(1u32.to_le_bytes());
        b.extend([0u8; 12]);
        for f in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
            b.extend(f.to_le_bytes());
        }
        b.extend([0u8; 2]);
        assert_eq!(parse_stl(&b).unwrap().triangle_count(), 1);
    }
    #[test]
    fn ply_ascii() {
        let p = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";
        assert_eq!(parse_ply(p.as_bytes()).unwrap().triangle_count(), 2);
    }
    #[test]
    fn ply_binary_le() {
        let mut p = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n".to_vec();
        for f in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
            p.extend(f.to_le_bytes());
        }
        p.push(3);
        for i in [0i32, 1, 2] {
            p.extend(i.to_le_bytes());
        }
        assert_eq!(parse_ply(&p).unwrap().triangle_count(), 1);
    }
    #[test]
    fn unsupported_ext() {
        assert!(load_from_bytes("xyz", b"", None).is_err());
    }
}
