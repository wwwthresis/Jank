//! glTF 2.0 / GLB importer with no external dependencies. It reads meshes, materials, textures
//! (PNG/JPEG/BMP/TGA), the node hierarchy, and the skeleton with skin weights (first used skin only).
//! glTF animations are not read; poses and frames are authored in Jank! itself.
use crate::decode::image::decode_image;
use crate::json::Json;
use crate::math::*;
use crate::mesh::*;
use crate::texture::Texture;
use std::collections::HashMap;

/// Result of a successful glTF/GLB import.
pub struct GltfResult {
    /// The imported model (meshes, materials, textures and optional skeleton).
    pub model: Model,
    /// Things that could not be carried over as-is (to show to the user).
    pub warnings: Vec<String>,
}

const MAX_VERTS: usize = 20_000_000;

// ------------------------------------------------------------------ base64 / URI

/// Decodes base64 text into bytes. Both the standard (`+` `/`) and URL-safe (`-` `_`) alphabets are
/// accepted; padding (`=`) and whitespace (space, CR, LF, tab) are skipped.
/// Fails with an error message on any other character.
pub fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return Err("base64: invalid character".into()),
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Decodes `%XX` escapes in a URI (as UTF-8 bytes); malformed escapes are left unchanged.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 + 1 {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn load_uri(uri: &str, resolve: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (_, data) = rest.split_once(',').ok_or("data URI has no data")?;
        return base64_decode(data);
    }
    let path = percent_decode(uri);
    resolve(&path).ok_or_else(|| format!("couldn't read the external file \"{path}\" (put it next to the model)"))
}

// ------------------------------------------------------------------ matrices

fn mat_from_array(a: &[f32]) -> Option<Mat4> {
    if a.len() != 16 {
        return None;
    }
    let mut m = Mat4::IDENTITY;
    for c in 0..4 {
        for r in 0..4 {
            m.m[c][r] = a[c * 4 + r];
        }
    }
    Some(m)
}

fn quat_from_basis(c0: Vec3, c1: Vec3, c2: Vec3) -> Quat {
    let (m00, m10, m20) = (c0.x, c0.y, c0.z);
    let (m01, m11, m21) = (c1.x, c1.y, c1.z);
    let (m02, m12, m22) = (c2.x, c2.y, c2.z);
    let tr = m00 + m11 + m22;
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        Quat { w: 0.25 * s, x: (m21 - m12) / s, y: (m02 - m20) / s, z: (m10 - m01) / s }
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        Quat { w: (m21 - m12) / s, x: 0.25 * s, y: (m01 + m10) / s, z: (m02 + m20) / s }
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        Quat { w: (m02 - m20) / s, x: (m01 + m10) / s, y: 0.25 * s, z: (m12 + m21) / s }
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        Quat { w: (m10 - m01) / s, x: (m02 + m20) / s, y: (m12 + m21) / s, z: 0.25 * s }
    };
    q.normalized()
}

/// Decomposes a matrix into (translation, rotation, scale). Shear is discarded.
fn decompose(m: &Mat4) -> (Vec3, Quat, Vec3) {
    let col = |i: usize| Vec3::new(m.m[i][0], m.m[i][1], m.m[i][2]);
    let t = col(3);
    let (mut sx, sy, sz) = (col(0).length(), col(1).length(), col(2).length());
    if col(0).cross(col(1)).dot(col(2)) < 0.0 {
        sx = -sx; // mirrored matrix
    }
    let safe = |v: f32| if v.abs() < 1e-12 { 1.0 } else { v };
    let (c0, c1, c2) = (col(0) / safe(sx), col(1) / safe(sy), col(2) / safe(sz));
    (t, quat_from_basis(c0, c1, c2), Vec3::new(sx, sy, sz))
}

fn det3(m: &Mat4) -> f32 {
    let c = |i: usize| Vec3::new(m.m[i][0], m.m[i][1], m.m[i][2]);
    c(0).cross(c(1)).dot(c(2))
}

// ------------------------------------------------------------------ accessors

struct Gltf<'a> {
    root: &'a Json,
    buffers: Vec<Vec<u8>>,
}
impl Gltf<'_> {
    fn view(&self, i: usize) -> Result<(&[u8], Option<usize>), String> {
        let v = self.root.get("bufferViews").and_then(|a| a.idx(i)).ok_or("bufferView not found")?;
        let buf = self.buffers.get(v.get("buffer").and_then(|x| x.as_usize()).ok_or("bufferView has no buffer")?).ok_or("buffer not found")?;
        let off = v.get("byteOffset").and_then(|x| x.as_usize()).unwrap_or(0);
        let len = v.get("byteLength").and_then(|x| x.as_usize()).ok_or("bufferView has no byteLength")?;
        let data = buf.get(off..off.checked_add(len).ok_or("integer overflow")?).ok_or("bufferView extends past its buffer")?;
        Ok((data, v.get("byteStride").and_then(|x| x.as_usize())))
    }
    /// Reads an accessor and converts it to f32. Returns (data as f32, components per element,
    /// element count). Sparse accessors are not supported: base values are used and a warning is
    /// pushed to `warn`. Fails if the accessor, its bufferView or its type is invalid, or if it
    /// reaches outside the bufferView.
    fn read(&self, acc: usize, warn: &mut Vec<String>) -> Result<(Vec<f32>, usize, usize), String> {
        let a = self.root.get("accessors").and_then(|x| x.idx(acc)).ok_or("accessor not found")?;
        let count = a.get("count").and_then(|x| x.as_usize()).ok_or("accessor has no count")?;
        let ncomp = match a.get("type").and_then(|x| x.as_str()) {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") | Some("MAT2") => 4,
            Some("MAT3") => 9,
            Some("MAT4") => 16,
            _ => return Err("unknown accessor type".into()),
        };
        let ct = a.get("componentType").and_then(|x| x.as_usize()).ok_or("accessor has no componentType")?;
        let (csize, conv): (usize, fn(&[u8], bool) -> f32) = match ct {
            5120 => (1, |b, n| if n { (b[0] as i8 as f32 / 127.0).max(-1.0) } else { b[0] as i8 as f32 }),
            5121 => (1, |b, n| if n { b[0] as f32 / 255.0 } else { b[0] as f32 }),
            5122 => (2, |b, n| { let v = i16::from_le_bytes([b[0], b[1]]) as f32; if n { (v / 32767.0).max(-1.0) } else { v } }),
            5123 => (2, |b, n| { let v = u16::from_le_bytes([b[0], b[1]]) as f32; if n { v / 65535.0 } else { v } }),
            5125 => (4, |b, _| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32),
            5126 => (4, |b, _| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            _ => return Err(format!("componentType {ct} is not supported")),
        };
        let normalized = a.get("normalized").and_then(|x| x.as_bool()).unwrap_or(false);
        if count.checked_mul(ncomp).map_or(true, |n| n > MAX_VERTS * 4) {
            return Err("accessor is too large".into());
        }
        if a.get("sparse").is_some() {
            warn.push("sparse accessors are not supported; base values were used".into());
        }
        let Some(view_idx) = a.get("bufferView").and_then(|x| x.as_usize()) else {
            return Ok((vec![0.0; count * ncomp], ncomp, count)); // per the spec, an accessor without a bufferView is all zeros
        };
        let (data, stride) = self.view(view_idx)?;
        let off = a.get("byteOffset").and_then(|x| x.as_usize()).unwrap_or(0);
        let elem = csize * ncomp;
        let stride = stride.filter(|&s| s > 0).unwrap_or(elem);
        if count > 0 {
            let end = off.checked_add((count - 1).checked_mul(stride).ok_or("integer overflow")?).and_then(|v| v.checked_add(elem)).ok_or("integer overflow")?;
            if end > data.len() {
                return Err("accessor extends past its bufferView".into());
            }
        }
        let mut out = Vec::with_capacity(count * ncomp);
        for i in 0..count {
            let base = off + i * stride;
            for c in 0..ncomp {
                out.push(conv(&data[base + c * csize..base + (c + 1) * csize], normalized));
            }
        }
        Ok((out, ncomp, count))
    }
}

// ------------------------------------------------------------------ loading

/// Imports a glTF 2.0 model from a `.gltf` (JSON) or `.glb` (binary container) file in memory.
///
/// `bytes` is the whole file; it is detected as GLB by the `glTF` magic. `resolve` is called with a
/// percent-decoded relative URI and returns the bytes of an external file (buffer or image), or
/// `None` if it is unavailable. `data:` URIs are decoded internally.
///
/// Supported: triangle lists, strips and fans (points and lines are skipped with a warning), node
/// transforms (baked into the vertices of unskinned meshes), POSITION/NORMAL/TEXCOORD_0/COLOR_0,
/// JOINTS_0/WEIGHTS_0 for the first used skin, base-colour textures and `KHR_materials_unlit`.
/// Alpha-blended materials are converted to alpha cut-out (with a warning); unreadable textures are
/// skipped (with a warning).
///
/// Fails with a message for: a truncated or unsupported container (GLB version other than 2, glTF
/// version other than 2.x), malformed JSON, missing or too-short buffers, invalid accessors or
/// indices, a skin without joints or with too many bones, a model that is too large, or a file with
/// no triangle meshes.
pub fn load_gltf(bytes: &[u8], resolve: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<GltfResult, String> {
    let mut warnings: Vec<String> = vec![];
    // --- container
    let (json_text, glb_bin): (String, Option<Vec<u8>>) = if bytes.starts_with(b"glTF") {
        if bytes.len() < 20 {
            return Err("GLB: file is too short".into());
        }
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != 2 {
            return Err(format!("GLB: version {version} is not supported (version 2 is required)"));
        }
        let (mut pos, mut json, mut bin) = (12usize, None, None);
        while pos + 8 <= bytes.len() {
            let len = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
            let ty = u32::from_le_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]]);
            let body = bytes.get(pos + 8..pos + 8 + len).ok_or("GLB: chunk extends past the end of the file")?;
            match ty {
                0x4E4F_534A => json = Some(String::from_utf8_lossy(body).into_owned()),
                0x004E_4942 if bin.is_none() => bin = Some(body.to_vec()),
                _ => {}
            }
            pos += 8 + ((len + 3) & !3);
        }
        (json.ok_or("GLB: no JSON chunk")?, bin)
    } else {
        (String::from_utf8_lossy(bytes).into_owned(), None)
    };
    let root = Json::parse(json_text.trim_start_matches('\u{feff}'))?;
    let ver = root.get("asset").and_then(|a| a.get("version")).and_then(|v| v.as_str()).unwrap_or("");
    if !ver.starts_with('2') {
        return Err(format!("glTF version \"{ver}\" is not supported (version 2.0 is required)"));
    }

    // --- buffers
    let mut buffers = vec![];
    for (i, b) in root.get("buffers").and_then(|b| b.as_array()).unwrap_or(&[]).iter().enumerate() {
        let data = match b.get("uri").and_then(|u| u.as_str()) {
            Some(u) => load_uri(u, resolve)?,
            None if i == 0 => glb_bin.clone().ok_or("buffer has no uri and there is no GLB BIN chunk")?,
            None => return Err("buffer has no uri".into()),
        };
        let want = b.get("byteLength").and_then(|x| x.as_usize()).unwrap_or(data.len());
        if data.len() < want {
            return Err(format!("buffer {i} is shorter than declared ({} < {want})", data.len()));
        }
        buffers.push(data);
    }
    let g = Gltf { root: &root, buffers };

    // --- images / textures / materials
    let mut model = Model { name: String::from("glTF"), ..Default::default() };
    if let Some(n) = root.get("scenes").and_then(|s| s.idx(root.get("scene").and_then(|x| x.as_usize()).unwrap_or(0))).and_then(|s| s.get("name")).and_then(|n| n.as_str()) {
        model.name = n.to_string();
    }
    let mut image_cache: HashMap<(usize, Option<u32>), usize> = HashMap::new();
    let mut load_image = |img_idx: usize, cutoff: Option<f32>, model: &mut Model, warnings: &mut Vec<String>| -> Option<usize> {
        let key = (img_idx, cutoff.map(|c| (c * 1000.0) as u32));
        if let Some(&i) = image_cache.get(&key) {
            return Some(i);
        }
        let im = root.get("images")?.idx(img_idx)?;
        let name = im.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("image_{img_idx}"));
        let bytes = if let Some(u) = im.get("uri").and_then(|u| u.as_str()) {
            load_uri(u, resolve)
        } else if let Some(v) = im.get("bufferView").and_then(|v| v.as_usize()) {
            g.view(v).map(|(d, _)| d.to_vec())
        } else {
            Err("image has neither uri nor bufferView".into())
        };
        let decoded = bytes.and_then(|b| decode_image(&name, &b));
        match decoded {
            Ok(d) => {
                let mut t = Texture { name, w: d.w, h: d.h, px: d.rgba.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect() };
                if let Some(c) = cutoff {
                    let thr = (c * 255.0) as u8;
                    t.px.iter_mut().for_each(|p| p[3] = if p[3] >= thr { 255 } else { 0 });
                }
                model.textures.push(t);
                image_cache.insert(key, model.textures.len() - 1);
                Some(model.textures.len() - 1)
            }
            Err(e) => {
                warnings.push(format!("texture \"{name}\" was not loaded: {e}"));
                None
            }
        }
    };
    for (mi, m) in root.get("materials").and_then(|m| m.as_array()).unwrap_or(&[]).iter().enumerate() {
        let pbr = m.get("pbrMetallicRoughness");
        let color = pbr.and_then(|p| p.get("baseColorFactor")).and_then(|c| c.f32_array()).filter(|c| c.len() >= 3).map_or([1.0; 3], |c| [c[0], c[1], c[2]]);
        let mode = m.get("alphaMode").and_then(|a| a.as_str()).unwrap_or("OPAQUE");
        let cutoff = match mode {
            "MASK" => Some(m.get("alphaCutoff").and_then(|c| c.as_f64()).unwrap_or(0.5) as f32),
            "BLEND" => {
                warnings.push(format!("material \"{}\": transparency was replaced with a cut-out (PS1 has no smooth transparency)", m.get("name").and_then(|n| n.as_str()).unwrap_or("?")));
                Some(0.5)
            }
            _ => None,
        };
        let tex = pbr
            .and_then(|p| p.get("baseColorTexture"))
            .and_then(|t| t.get("index"))
            .and_then(|i| i.as_usize())
            .and_then(|ti| root.get("textures")?.idx(ti)?.get("source")?.as_usize())
            .and_then(|src| load_image(src, cutoff, &mut model, &mut warnings));
        model.materials.push(Material {
            name: m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("Material_{mi}")),
            texture: tex,
            color,
            unlit: m.get("extensions").and_then(|e| e.get("KHR_materials_unlit")).is_some(),
            double_sided: m.get("doubleSided").and_then(|d| d.as_bool()).unwrap_or(false),
            color_key: false,
        });
    }
    let default_mat = model.materials.len();
    model.materials.push(Material { name: "Default".into(), ..Default::default() });

    // --- nodes
    let nodes = root.get("nodes").and_then(|n| n.as_array()).unwrap_or(&[]);
    let n_nodes = nodes.len();
    let children: Vec<Vec<usize>> = nodes.iter().map(|n| n.get("children").and_then(|c| c.as_array()).unwrap_or(&[]).iter().filter_map(|c| c.as_usize()).filter(|&c| c < n_nodes).collect()).collect();
    let mut parent = vec![None; n_nodes];
    for (p, cs) in children.iter().enumerate() {
        for &c in cs {
            parent[c] = Some(p);
        }
    }
    let local = |n: &Json| -> Mat4 {
        if let Some(m) = n.get("matrix").and_then(|m| m.f32_array()).and_then(|a| mat_from_array(&a)) {
            return m;
        }
        let t = n.get("translation").and_then(|t| t.f32_array()).filter(|t| t.len() == 3).map_or(Vec3::ZERO, |t| Vec3::new(t[0], t[1], t[2]));
        let r = n.get("rotation").and_then(|r| r.f32_array()).filter(|r| r.len() == 4).map_or(Quat::IDENTITY, |r| Quat { x: r[0], y: r[1], z: r[2], w: r[3] }.normalized());
        let s = n.get("scale").and_then(|s| s.f32_array()).filter(|s| s.len() == 3).map_or(Vec3::ONE, |s| Vec3::new(s[0], s[1], s[2]));
        Mat4::from_trs(t, r, s)
    };
    let scene_roots: Vec<usize> = match root.get("scenes").and_then(|s| s.idx(root.get("scene").and_then(|x| x.as_usize()).unwrap_or(0))).and_then(|s| s.get("nodes")).and_then(|n| n.as_array()) {
        Some(a) => a.iter().filter_map(|x| x.as_usize()).filter(|&x| x < n_nodes).collect(),
        None => (0..n_nodes).filter(|&i| parent[i].is_none()).collect(),
    };
    // depth-first walk (guarded against cycles): node order and world matrices
    let mut world = vec![Mat4::IDENTITY; n_nodes];
    let mut order: Vec<usize> = vec![];
    let mut seen = vec![false; n_nodes];
    let mut stack: Vec<(usize, Mat4)> = scene_roots.iter().rev().map(|&r| (r, Mat4::IDENTITY)).collect();
    while let Some((n, pw)) = stack.pop() {
        if seen[n] {
            continue;
        }
        seen[n] = true;
        world[n] = pw * local(&nodes[n]);
        order.push(n);
        for &c in children[n].iter().rev() {
            stack.push((c, world[n]));
        }
    }

    // --- skeleton from the first skin actually used by a mesh
    let used_skin = order.iter().filter_map(|&n| nodes[n].get("mesh").and(nodes[n].get("skin")).and_then(|s| s.as_usize())).next();
    let mut joint_to_bone: HashMap<usize, usize> = HashMap::new(); // node index -> bone index
    let mut skin_joints: Vec<usize> = vec![];
    if let Some(si) = used_skin {
        let skin = root.get("skins").and_then(|s| s.idx(si)).ok_or("skin not found")?;
        skin_joints = skin.get("joints").and_then(|j| j.as_array()).unwrap_or(&[]).iter().filter_map(|j| j.as_usize()).filter(|&j| j < n_nodes).collect();
        if skin_joints.is_empty() {
            return Err("skin has no joints".into());
        }
        if skin_joints.len() > u16::MAX as usize {
            return Err("too many bones".into());
        }
        // bind-pose world matrices: from inverseBindMatrices (exact), otherwise from the nodes
        let mut bind_world: Vec<Mat4> = skin_joints.iter().map(|&j| world[j]).collect();
        if let Some(ibm_acc) = skin.get("inverseBindMatrices").and_then(|a| a.as_usize()) {
            let (d, nc, cnt) = g.read(ibm_acc, &mut warnings)?;
            if nc == 16 && cnt >= skin_joints.len() {
                for (k, bw) in bind_world.iter_mut().enumerate() {
                    if let Some(inv) = mat_from_array(&d[k * 16..k * 16 + 16]).and_then(|m| m.inverse()) {
                        *bw = inv;
                    }
                }
            }
        }
        // a bone's parent is its nearest ancestor that is also a joint
        let jset: HashMap<usize, usize> = skin_joints.iter().enumerate().map(|(k, &j)| (j, k)).collect();
        let bone_parent: Vec<Option<usize>> = skin_joints
            .iter()
            .map(|&j| {
                let mut p = parent[j];
                let mut guard = 0;
                while let Some(pn) = p {
                    if let Some(&k) = jset.get(&pn) {
                        return Some(k);
                    }
                    p = parent[pn];
                    guard += 1;
                    if guard > n_nodes {
                        break;
                    }
                }
                None
            })
            .collect();
        // order bones so a parent comes before its children: sort by depth in the joint tree
        let depth = |mut k: usize| {
            let mut d = 0;
            while let Some(p) = bone_parent[k] {
                k = p;
                d += 1;
                if d > skin_joints.len() {
                    break;
                }
            }
            d
        };
        let mut idx: Vec<usize> = (0..skin_joints.len()).collect();
        idx.sort_by_key(|&k| (depth(k), k));
        let mut new_of_old = vec![0usize; skin_joints.len()];
        for (new, &old) in idx.iter().enumerate() {
            new_of_old[old] = new;
        }
        let mut bones = vec![];
        for &old in &idx {
            let w = bind_world[old];
            let (pw, parent_new) = match bone_parent[old] {
                Some(p) => (bind_world[p], Some(new_of_old[p])),
                None => (Mat4::IDENTITY, None),
            };
            let loc = pw.inverse().unwrap_or(Mat4::IDENTITY) * w;
            let (t, r, s) = decompose(&loc);
            let name = nodes[skin_joints[old]].get("name").and_then(|n| n.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("bone_{old}"));
            bones.push(Bone { name, parent: parent_new, rest_t: t, rest_r: r, rest_s: s });
        }
        for (old, &j) in skin_joints.iter().enumerate() {
            joint_to_bone.insert(j, new_of_old[old]);
        }
        model.skeleton = Some(Skeleton { bones });
    }

    // --- meshes
    let mut total_verts = 0usize;
    for &n in &order {
        let node = &nodes[n];
        let Some(mesh_idx) = node.get("mesh").and_then(|m| m.as_usize()) else { continue };
        let Some(gm) = root.get("meshes").and_then(|m| m.idx(mesh_idx)) else { continue };
        let node_skin = node.get("skin").and_then(|s| s.as_usize());
        let skinned = node_skin.is_some() && node_skin == used_skin;
        if node_skin.is_some() && !skinned {
            warnings.push(format!("node \"{}\" uses a second skin; it was imported without bones", node.get("name").and_then(|x| x.as_str()).unwrap_or("?")));
        }
        let w = world[n];
        let mirrored = !skinned && det3(&w) < 0.0;
        let base_name = gm.get("name").and_then(|x| x.as_str()).or_else(|| node.get("name").and_then(|x| x.as_str())).map(|s| s.to_string()).unwrap_or_else(|| format!("mesh_{mesh_idx}"));
        for (pi, prim) in gm.get("primitives").and_then(|p| p.as_array()).unwrap_or(&[]).iter().enumerate() {
            let mode = prim.get("mode").and_then(|m| m.as_usize()).unwrap_or(4);
            if !(4..=6).contains(&mode) {
                warnings.push(format!("\"{base_name}\": primitive with mode {mode} (points/lines) was skipped"));
                continue;
            }
            let attrs = prim.get("attributes").ok_or("primitive has no attributes")?;
            let acc = |name: &str| attrs.get(name).and_then(|a| a.as_usize());
            let (pos, pc, vcount) = g.read(acc("POSITION").ok_or("primitive has no POSITION")?, &mut warnings)?;
            if pc != 3 {
                return Err("POSITION must be VEC3".into());
            }
            total_verts += vcount;
            if total_verts > MAX_VERTS {
                return Err("model is too large".into());
            }
            let nrm = acc("NORMAL").map(|a| g.read(a, &mut warnings)).transpose()?;
            let uv = acc("TEXCOORD_0").map(|a| g.read(a, &mut warnings)).transpose()?;
            let col = acc("COLOR_0").map(|a| g.read(a, &mut warnings)).transpose()?;
            let jnt = acc("JOINTS_0").map(|a| g.read(a, &mut warnings)).transpose()?;
            let wgt = acc("WEIGHTS_0").map(|a| g.read(a, &mut warnings)).transpose()?;
            let mut mesh = Mesh::new(&if gm.get("primitives").map_or(0, |p| p.len()) > 1 { format!("{base_name}.{pi}") } else { base_name.clone() });
            for i in 0..vcount {
                let p = Vec3::new(pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2]);
                let mut v = Vertex::new(if skinned { p } else { w.transform_point(p) }, Vec2::default());
                if let Some((d, nc, c)) = &nrm {
                    if *nc == 3 && i < *c {
                        let nv = Vec3::new(d[i * 3], d[i * 3 + 1], d[i * 3 + 2]);
                        v.nrm = if skinned { nv } else { w.transform_dir(nv).normalized() };
                    }
                }
                if let Some((d, nc, c)) = &uv {
                    if *nc == 2 && i < *c {
                        v.uv = Vec2::new(d[i * 2], d[i * 2 + 1]);
                    }
                }
                if let Some((d, nc, c)) = &col {
                    if (*nc == 3 || *nc == 4) && i < *c {
                        v.color = [d[i * nc], d[i * nc + 1], d[i * nc + 2]];
                    }
                }
                if skinned {
                    if let (Some((jd, jc, jn)), Some((wd, wc, wn))) = (&jnt, &wgt) {
                        if *jc == 4 && *wc == 4 && i < *jn && i < *wn {
                            let mut sum = 0.0;
                            for k in 0..4 {
                                let node_of = skin_joints.get(jd[i * 4 + k] as usize).copied();
                                let bone = node_of.and_then(|nn| joint_to_bone.get(&nn)).copied().unwrap_or(0);
                                v.joints[k] = bone as u16;
                                v.weights[k] = wd[i * 4 + k].max(0.0);
                                sum += v.weights[k];
                            }
                            if sum > 1e-6 {
                                v.weights.iter_mut().for_each(|x| *x /= sum);
                            } else {
                                v.weights = [0.0; 4];
                            }
                        }
                    }
                }
                mesh.verts.push(v);
            }
            // indices
            let idx: Vec<u32> = match prim.get("indices").and_then(|i| i.as_usize()) {
                Some(a) => g.read(a, &mut warnings)?.0.iter().map(|&f| f as u32).collect(),
                None => (0..vcount as u32).collect(),
            };
            if idx.iter().any(|&i| i as usize >= vcount) {
                return Err(format!("\"{base_name}\": vertex index out of range"));
            }
            let mut tris: Vec<[u32; 3]> = vec![];
            match mode {
                4 => tris.extend(idx.chunks_exact(3).map(|c| [c[0], c[1], c[2]])),
                5 => {
                    for k in 0..idx.len().saturating_sub(2) {
                        tris.push(if k % 2 == 0 { [idx[k], idx[k + 1], idx[k + 2]] } else { [idx[k + 1], idx[k], idx[k + 2]] });
                    }
                }
                _ => {
                    for k in 1..idx.len().saturating_sub(1) {
                        tris.push([idx[0], idx[k], idx[k + 1]]);
                    }
                }
            }
            if mirrored {
                tris.iter_mut().for_each(|t| t.swap(1, 2)); // a mirrored node flips the winding order
            }
            mesh.tris = tris;
            mesh.material = prim.get("material").and_then(|m| m.as_usize()).filter(|&m| m < default_mat).unwrap_or(default_mat);
            if nrm.is_none() || mesh.verts.iter().any(|v| v.nrm == Vec3::Y && nrm.is_none()) {
                mesh.recompute_normals();
            }
            model.meshes.push(mesh);
        }
    }
    if model.meshes.is_empty() {
        return Err("the glTF file has no triangle meshes".into());
    }
    model.ensure_material();
    Ok(GltfResult { model, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::png::encode_png;

    fn f32s(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }
    fn pad4(v: &mut Vec<u8>) {
        while v.len() % 4 != 0 {
            v.push(0);
        }
    }
    /// Two-joint "arm": root -> elbow, 4 vertices (2 per joint), 2 triangles, PNG texture stored in a bufferView.
    fn arm_glb() -> Vec<u8> {
        let mut bin: Vec<u8> = vec![];
        let mut views: Vec<(usize, usize)> = vec![];
        let mut add = |bin: &mut Vec<u8>, data: Vec<u8>| {
            pad4(bin);
            let o = bin.len();
            bin.extend(&data);
            views.push((o, data.len()));
        };
        add(&mut bin, f32s(&[0., 0., 0., 1., 0., 0., 0., 1., 0., 1., 1., 0.])); // 0 POSITION
        add(&mut bin, f32s(&[0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1.])); // 1 NORMAL
        add(&mut bin, f32s(&[0., 0., 1., 0., 0., 1., 1., 1.])); // 2 UV
        add(&mut bin, vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]); // 3 JOINTS u8 vec4
        add(&mut bin, f32s(&[1., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0.])); // 4 WEIGHTS
        add(&mut bin, [0u16, 1, 2, 2, 1, 3].iter().flat_map(|i| i.to_le_bytes()).collect()); // 5 indices u16
        // inverseBindMatrices follow skin.joints = [elbow, root]: elbow at (0,1,0) -> inverse = translation (0,-1,0)
        let mut ibm = vec![];
        for t in [[0.0f32, -1., 0.], [0., 0., 0.]] {
            ibm.extend(f32s(&[1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., t[0], t[1], t[2], 1.]));
        }
        add(&mut bin, ibm); // 6
        let tex: Vec<u8> = [255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255].to_vec();
        add(&mut bin, encode_png(2, 2, &tex, true)); // 7 image
        pad4(&mut bin);
        let bv: Vec<String> = views.iter().map(|(o, l)| format!(r#"{{"buffer":0,"byteOffset":{o},"byteLength":{l}}}"#)).collect();
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0,2]}}],
            "nodes":[{{"name":"root","children":[1]}},{{"name":"elbow","translation":[0,1,0]}},{{"name":"body","mesh":0,"skin":0}}],
            "skins":[{{"joints":[1,0],"inverseBindMatrices":6}}],
            "meshes":[{{"name":"arm","primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2,"JOINTS_0":3,"WEIGHTS_0":4}},"indices":5,"material":0}}]}}],
            "materials":[{{"name":"skin","pbrMetallicRoughness":{{"baseColorFactor":[1,0.5,0.25,1],"baseColorTexture":{{"index":0}}}},"doubleSided":true}}],
            "textures":[{{"source":0}}],"images":[{{"bufferView":7,"mimeType":"image/png"}}],
            "accessors":[
              {{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3"}},
              {{"bufferView":1,"componentType":5126,"count":4,"type":"VEC3"}},
              {{"bufferView":2,"componentType":5126,"count":4,"type":"VEC2"}},
              {{"bufferView":3,"componentType":5121,"count":4,"type":"VEC4"}},
              {{"bufferView":4,"componentType":5126,"count":4,"type":"VEC4"}},
              {{"bufferView":5,"componentType":5123,"count":6,"type":"SCALAR"}},
              {{"bufferView":6,"componentType":5126,"count":2,"type":"MAT4"}}],
            "bufferViews":[{}],"buffers":[{{"byteLength":{}}}]}}"#,
            bv.join(","),
            bin.len()
        );
        let mut j = json.into_bytes();
        while j.len() % 4 != 0 {
            j.push(b' ');
        }
        let total = 12 + 8 + j.len() + 8 + bin.len();
        let mut out = b"glTF".to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend((total as u32).to_le_bytes());
        out.extend((j.len() as u32).to_le_bytes());
        out.extend(0x4E4F_534Au32.to_le_bytes());
        out.extend(&j);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(0x004E_4942u32.to_le_bytes());
        out.extend(&bin);
        out
    }

    #[test]
    fn glb_with_skin_texture_and_material() {
        let r = load_gltf(&arm_glb(), &|_| None).expect("the GLB should load");
        let m = &r.model;
        assert_eq!((m.meshes.len(), m.triangle_count(), m.vertex_count()), (1, 2, 4));
        assert_eq!(m.textures.len(), 1);
        assert_eq!(m.textures[0].px[0], [255, 0, 0, 255]);
        assert_eq!(m.materials[0].color, [1.0, 0.5, 0.25]);
        assert!(m.materials[0].double_sided);
        assert_eq!(m.meshes[0].material, 0);
        // skeleton: joints are given as [elbow, root]; the order must be rebuilt so a parent comes before its children
        let sk = m.skeleton.as_ref().unwrap();
        assert_eq!(sk.bones.len(), 2);
        assert_eq!(sk.bones[0].name, "root");
        assert_eq!(sk.bones[1].name, "elbow");
        assert_eq!(sk.bones[1].parent, Some(0));
        assert!((sk.bones[1].rest_t - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5);
        // vertices 0,1 are bound to joints[0]=elbow (bone 1), vertices 2,3 to joints[1]... per the JOINTS data: [0,0,0,0],[0,0,0,0],[1,0,0,0],[1,0,0,0]
        assert_eq!(m.meshes[0].verts[0].joints[0], 1);
        assert_eq!(m.meshes[0].verts[2].joints[0], 0);
        assert!((m.meshes[0].verts[0].weights[0] - 1.0).abs() < 1e-6);
        // UV: v=0 is at the top, same as ours; no flip
        assert_eq!(m.meshes[0].verts[1].uv, Vec2::new(1.0, 0.0));
        // in the rest pose skinning must not change the geometry
        let (ps, _) = crate::raster::skin_model(m, Some(&crate::pose::Pose::rest(2)));
        for (p, v) in ps[0].iter().zip(&m.meshes[0].verts) {
            assert!((*p - v.pos).length() < 1e-4);
        }
    }
    #[test]
    fn truncated_and_corrupt_files_are_errors() {
        let good = arm_glb();
        for cut in [4, 15, 30, good.len() / 2, good.len() - 1] {
            assert!(load_gltf(&good[..cut], &|_| None).is_err(), "truncated at {cut}");
        }
        for seed in 0..60usize {
            let mut b = good.clone();
            let i = 20 + (seed * 104729) % (b.len() - 20);
            b[i] ^= 0x5A;
            let _ = load_gltf(&b, &|_| None); // must not panic
        }
        assert!(load_gltf(b"{\"asset\":{\"version\":\"1.0\"}}", &|_| None).is_err());
    }
    #[test]
    fn gltf_json_with_data_uri_and_node_transform() {
        // one triangle in a data URI; the node is translated by (5,0,0) and rotated/mirrored on X
        let pos: Vec<u8> = f32s(&[0., 0., 0., 1., 0., 0., 0., 1., 0.]);
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for c in pos.chunks(3) {
                let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                s.push(T[(n >> 18) as usize & 63] as char);
                s.push(T[(n >> 12) as usize & 63] as char);
                s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
                s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
            }
            s
        };
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"nodes":[{{"mesh":0,"translation":[5,0,0],"scale":[-1,1,1]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}}],
            "bufferViews":[{{"buffer":0,"byteLength":36}}],
            "buffers":[{{"byteLength":36,"uri":"data:application/octet-stream;base64,{b64}"}}]}}"#
        );
        let r = load_gltf(json.as_bytes(), &|_| None).unwrap();
        let m = &r.model.meshes[0];
        assert!((m.verts[1].pos - Vec3::new(4.0, 0.0, 0.0)).length() < 1e-5, "{:?}", m.verts[1].pos);
        // mirrored node: the winding is flipped, and after recomputation the normal points to +Z like the original CCW triangle
        let n = m.verts[0].nrm;
        assert!(n.z > 0.9, "{n:?}");
        assert!(r.model.skeleton.is_none());
    }
    #[test]
    fn python_generated_strip_with_stride_normalized_uv_and_node_chain() {
        // The file was produced by an independent generator (tests/fixtures/gen_strip_glb.py): byteStride, normalized u16 UV,
        // a triangle strip with u32 indices, and a node chain "translate+rotate" -> "scale".
        let r = load_gltf(include_bytes!("../tests/fixtures/strip.glb"), &|_| None).unwrap();
        let m = &r.model.meshes[0];
        let exp = [[10.0, 0.0], [10.0, 2.0], [8.0, 2.0], [8.0, 0.0], [6.0, 1.0]];
        for (v, e) in m.verts.iter().zip(exp) {
            assert!((v.pos.x - e[0]).abs() < 1e-4 && (v.pos.y - e[1]).abs() < 1e-4 && v.pos.z.abs() < 1e-4, "{:?}", v.pos);
        }
        assert_eq!(m.tris, vec![[0, 1, 3], [3, 1, 2], [3, 2, 4]]);
        assert!((m.verts[4].uv.x - 0.5).abs() < 1e-4);
    }
    #[test]
    fn base64_and_percent() {
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(base64_decode("SGVsbG8h").unwrap(), b"Hello!");
        assert!(base64_decode("@@@").is_err());
        assert_eq!(percent_decode("my%20tex.png"), "my tex.png");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
