//! Artist-friendly modeling: primitives and simple operations on meshes.
//! The whole low-poly pipeline: start from a cube, extrude faces, mirror, flat-shade, then paint.
use crate::math::*;
use crate::mesh::*;
use crate::texture::Texture;
use std::collections::HashMap;

fn pkey(p: Vec3) -> (i64, i64, i64) {
    ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64, (p.z * 1e4).round() as i64)
}

/// Wraps a mesh into a ready-to-use model with a material and a checker texture, so there is immediately something to paint on.
pub fn model_from_mesh(name: &str, mut mesh: Mesh, tex_size: u32) -> Model {
    mesh.material = 0;
    Model {
        name: name.into(),
        meshes: vec![mesh],
        materials: vec![Material { name: "Main".into(), texture: Some(0), ..Default::default() }],
        textures: vec![Texture::checker("Main", tex_size, tex_size, (tex_size / 8).max(1), [200, 200, 205, 255], [150, 150, 160, 255])],
        skeleton: None,
    }
}

/// The built-in butterfly: 12 triangles with a 128x256 texture that has transparent parts. Geometry and picture are embedded in
/// the program (the files live in `core/assets/`), so the model needs nothing from disk. About one unit across, centred on the origin.
pub fn butterfly() -> Model {
    const OBJ: &str = include_str!("../assets/butterfly.obj");
    const PNG: &[u8] = include_bytes!("../assets/butterfly_texture.png");
    let mut m = crate::import::parse_obj(OBJ, None).expect("the embedded butterfly OBJ is valid").model;
    let img = crate::decode::image::decode_png(PNG).expect("the embedded butterfly texture is valid");
    m.name = "Butterfly".into();
    m.textures = vec![Texture { name: "Butterfly".into(), w: img.w, h: img.h, px: img.rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect() }];
    m.materials = vec![Material { name: "Butterfly".into(), texture: Some(0), double_sided: true, ..Material::default() }];
    for mesh in &mut m.meshes {
        mesh.material = 0;
    }
    // scale to about one unit of wingspan and centre the bounding box on the origin
    let (lo, hi) = m.bounds().expect("the butterfly has geometry");
    let (c, s) = ((lo + hi) * 0.5, 1.0 / (hi.x - lo.x).max(1e-3));
    for mesh in &mut m.meshes {
        for v in &mut mesh.verts {
            v.pos = (v.pos - c) * s;
        }
    }
    m
}

// ---------------------------------------------------------------- primitives

fn quad(m: &mut Mesh, c: [Vec3; 4], uv: [Vec2; 4]) {
    let base = m.verts.len() as u32;
    for i in 0..4 {
        m.verts.push(Vertex::new(c[i], uv[i]));
    }
    m.tris.push([base, base + 1, base + 2]);
    m.tris.push([base, base + 2, base + 3]);
}

/// A cube whose faces occupy the cells of a 3x2 atlas (convenient for painting every side separately).
pub fn cube(size: Vec3) -> Mesh {
    let h = size * 0.5;
    let s = |x: f32, y: f32, z: f32| Vec3::new(x * h.x, y * h.y, z * h.z);
    let faces: [[Vec3; 4]; 6] = [
        [s(1., -1., 1.), s(1., -1., -1.), s(1., 1., -1.), s(1., 1., 1.)],
        [s(-1., -1., -1.), s(-1., -1., 1.), s(-1., 1., 1.), s(-1., 1., -1.)],
        [s(-1., 1., 1.), s(1., 1., 1.), s(1., 1., -1.), s(-1., 1., -1.)],
        [s(-1., -1., -1.), s(1., -1., -1.), s(1., -1., 1.), s(-1., -1., 1.)],
        [s(-1., -1., 1.), s(1., -1., 1.), s(1., 1., 1.), s(-1., 1., 1.)],
        [s(1., -1., -1.), s(-1., -1., -1.), s(-1., 1., -1.), s(1., 1., -1.)],
    ];
    let mut m = Mesh::new("Cube");
    for (i, f) in faces.iter().enumerate() {
        let (cx, cy) = ((i % 3) as f32, (i / 3) as f32);
        let uv = |u: f32, v: f32| Vec2::new((cx + u) / 3.0, (cy + v) / 2.0);
        quad(&mut m, *f, [uv(0.0, 1.0), uv(1.0, 1.0), uv(1.0, 0.0), uv(0.0, 0.0)]);
    }
    m.recompute_normals();
    m
}

pub fn plane(w: f32, d: f32, nx: usize, nz: usize) -> Mesh {
    let (nx, nz) = (nx.max(1), nz.max(1));
    let mut m = Mesh::new("Plane");
    for z in 0..=nz {
        for x in 0..=nx {
            let (u, v) = (x as f32 / nx as f32, z as f32 / nz as f32);
            m.verts.push(Vertex::new(Vec3::new((u - 0.5) * w, 0.0, (v - 0.5) * d), Vec2::new(u, v)));
        }
    }
    let row = (nx + 1) as u32;
    for z in 0..nz as u32 {
        for x in 0..nx as u32 {
            let (a, b, c, d2) = (z * row + x, z * row + x + 1, (z + 1) * row + x, (z + 1) * row + x + 1);
            m.tris.push([a, c, b]);
            m.tris.push([b, c, d2]);
        }
    }
    m.recompute_normals();
    m
}

/// A truncated cone or cylinder: `r_top = 0` gives a cone. The base sits at y = 0 and the top at y = h.
pub fn frustum(r_bottom: f32, r_top: f32, h: f32, seg: usize, caps: bool) -> Mesh {
    let seg = seg.max(3);
    let mut m = Mesh::new(if r_top == 0.0 { "Cone" } else { "Cylinder" });
    for s in 0..=seg {
        let t = s as f32 / seg as f32;
        let a = t * std::f32::consts::TAU;
        let (c, sn) = (a.cos(), a.sin());
        m.verts.push(Vertex::new(Vec3::new(c * r_bottom, 0.0, sn * r_bottom), Vec2::new(t, 1.0)));
        m.verts.push(Vertex::new(Vec3::new(c * r_top, h, sn * r_top), Vec2::new(t, 0.0)));
    }
    for s in 0..seg as u32 {
        let (b0, t0, b1, t1) = (s * 2, s * 2 + 1, s * 2 + 2, s * 2 + 3);
        m.tris.push([b0, t0, t1]);
        m.tris.push([b0, t1, b1]);
    }
    let mut cap = |y: f32, r: f32, up: bool| {
        let center = m.verts.len() as u32;
        m.verts.push(Vertex::new(Vec3::new(0.0, y, 0.0), Vec2::new(0.5, 0.5)));
        for s in 0..=seg {
            let a = s as f32 / seg as f32 * std::f32::consts::TAU;
            m.verts.push(Vertex::new(Vec3::new(a.cos() * r, y, a.sin() * r), Vec2::new(0.5 + a.cos() * 0.5, 0.5 + a.sin() * 0.5)));
        }
        for s in 0..seg as u32 {
            let (p, q) = (center + 1 + s, center + 2 + s);
            m.tris.push(if up { [center, q, p] } else { [center, p, q] });
        }
    };
    if caps {
        if r_bottom > 0.0 {
            cap(0.0, r_bottom, false);
        }
        if r_top > 0.0 {
            cap(h, r_top, true);
        }
    }
    m.cleanup();
    m.recompute_normals();
    m
}
pub fn cylinder(r: f32, h: f32, seg: usize) -> Mesh {
    frustum(r, r, h, seg, true)
}
pub fn cone(r: f32, h: f32, seg: usize) -> Mesh {
    frustum(r, 0.0, h, seg, true)
}

pub fn sphere(radius: f32, seg: usize, rings: usize) -> Mesh {
    let (seg, rings) = (seg.max(3), rings.max(2));
    let mut m = Mesh::new("Sphere");
    for r in 0..=rings {
        let phi = std::f32::consts::PI * r as f32 / rings as f32;
        for s in 0..=seg {
            let th = std::f32::consts::TAU * s as f32 / seg as f32;
            m.verts.push(Vertex::new(
                Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin()) * radius,
                Vec2::new(s as f32 / seg as f32, r as f32 / rings as f32),
            ));
        }
    }
    let w = (seg + 1) as u32;
    for r in 0..rings as u32 {
        for s in 0..seg as u32 {
            let (a, b, c, d) = (r * w + s, r * w + s + 1, (r + 1) * w + s, (r + 1) * w + s + 1);
            // at the poles one of the two triangles degenerates into a line, so it is skipped
            if r != 0 {
                m.tris.push([a, b, c]);
            }
            if r as usize != rings - 1 {
                m.tris.push([b, d, c]);
            }
        }
    }
    m.cleanup();
    m.recompute_normals();
    m
}

pub fn torus(major: f32, minor: f32, seg: usize, ring: usize) -> Mesh {
    let (seg, ring) = (seg.max(3), ring.max(3));
    let mut m = Mesh::new("Torus");
    for i in 0..=seg {
        let a = i as f32 / seg as f32 * std::f32::consts::TAU;
        for j in 0..=ring {
            let b = j as f32 / ring as f32 * std::f32::consts::TAU;
            let r = major + minor * b.cos();
            m.verts.push(Vertex::new(Vec3::new(a.cos() * r, minor * b.sin(), a.sin() * r), Vec2::new(i as f32 / seg as f32, j as f32 / ring as f32)));
        }
    }
    let w = (ring + 1) as u32;
    for i in 0..seg as u32 {
        for j in 0..ring as u32 {
            let (a, b, c, d) = (i * w + j, i * w + j + 1, (i + 1) * w + j, (i + 1) * w + j + 1);
            m.tris.push([a, b, c]);
            m.tris.push([b, d, c]);
        }
    }
    m.cleanup();
    m.recompute_normals();
    m
}

// ---------------------------------------------------------------- operations

/// Flat shading: every triangle gets its own vertices and its face normal (the classic low-poly look).
pub fn flat_shade(mesh: &mut Mesh) {
    let mut verts = Vec::with_capacity(mesh.tris.len() * 3);
    let mut tris = Vec::with_capacity(mesh.tris.len());
    for t in &mesh.tris {
        let (a, b, c) = (&mesh.verts[t[0] as usize], &mesh.verts[t[1] as usize], &mesh.verts[t[2] as usize]);
        let n = (b.pos - a.pos).cross(c.pos - a.pos).normalized();
        let base = verts.len() as u32;
        for v in [a, b, c] {
            let mut v = v.clone();
            v.nrm = n;
            verts.push(v);
        }
        tris.push([base, base + 1, base + 2]);
    }
    mesh.verts = verts;
    mesh.tris = tris;
}

/// Welds vertices that share a position. With `keep_uv` set, UV seams are kept (vertices with different UVs are not merged).
pub fn weld(mesh: &mut Mesh, keep_uv: bool) {
    let mut map: HashMap<((i64, i64, i64), (i64, i64)), u32> = HashMap::new();
    let mut verts: Vec<Vertex> = Vec::new();
    let mut remap = vec![0u32; mesh.verts.len()];
    for (i, v) in mesh.verts.iter().enumerate() {
        let uvk = if keep_uv { ((v.uv.x * 1e4).round() as i64, (v.uv.y * 1e4).round() as i64) } else { (0, 0) };
        remap[i] = *map.entry((pkey(v.pos), uvk)).or_insert_with(|| {
            verts.push(v.clone());
            (verts.len() - 1) as u32
        });
    }
    for t in &mut mesh.tris {
        for i in t.iter_mut() {
            *i = remap[*i as usize];
        }
    }
    mesh.verts = verts;
    mesh.cleanup();
    mesh.recompute_normals();
}

/// Extrudes the selected faces along their average normal. A negative distance pushes them inward.
/// Returns the indices of the new (top) triangles so the extrusion can be repeated.
pub fn extrude_faces(mesh: &mut Mesh, faces: &[usize], dist: f32) -> Vec<usize> {
    let faces: Vec<usize> = {
        let mut f: Vec<usize> = faces.iter().copied().filter(|&f| f < mesh.tris.len()).collect();
        f.sort_unstable();
        f.dedup();
        f
    };
    if faces.is_empty() {
        return vec![];
    }
    // the average normal of the selection (weighted by area)
    let mut n = Vec3::ZERO;
    for &f in &faces {
        let t = mesh.tris[f];
        let (a, b, c) = (mesh.verts[t[0] as usize].pos, mesh.verts[t[1] as usize].pos, mesh.verts[t[2] as usize].pos);
        n += (b - a).cross(c - a);
    }
    let n = n.normalized();
    let off = n * dist;

    // the edges of the selection by position; a boundary edge occurs in exactly one selected face
    let mut edge_count: HashMap<((i64, i64, i64), (i64, i64, i64)), u32> = HashMap::new();
    let ekey = |a: Vec3, b: Vec3| {
        let (ka, kb) = (pkey(a), pkey(b));
        if ka <= kb { (ka, kb) } else { (kb, ka) }
    };
    for &f in &faces {
        let t = mesh.tris[f];
        for k in 0..3 {
            let (a, b) = (mesh.verts[t[k] as usize].pos, mesh.verts[t[(k + 1) % 3] as usize].pos);
            *edge_count.entry(ekey(a, b)).or_insert(0) += 1;
        }
    }
    // one top vertex per position (the cap stays connected)
    let mut top: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut new_top_tris = Vec::new();
    let mut walls: Vec<[Vertex; 4]> = Vec::new();
    let mut cap_tris: Vec<(usize, [u32; 3])> = Vec::new();
    for &f in &faces {
        let t = mesh.tris[f];
        let mut idx = [0u32; 3];
        for k in 0..3 {
            let v = mesh.verts[t[k] as usize].clone();
            idx[k] = *top.entry(pkey(v.pos)).or_insert_with(|| {
                let mut nv = v.clone();
                nv.pos = v.pos + off;
                mesh.verts.push(nv);
                (mesh.verts.len() - 1) as u32
            });
        }
        cap_tris.push((f, idx));
        for k in 0..3 {
            let (ia, ib) = (t[k] as usize, t[(k + 1) % 3] as usize);
            let (a, b) = (mesh.verts[ia].clone(), mesh.verts[ib].clone());
            if edge_count[&ekey(a.pos, b.pos)] == 1 {
                let (mut at, mut bt) = (a.clone(), b.clone());
                at.pos = a.pos + off;
                bt.pos = b.pos + off;
                walls.push([a, b, bt, at]);
            }
        }
    }
    for (f, idx) in cap_tris {
        mesh.tris[f] = idx; // the face itself moves to the top
        new_top_tris.push(f);
    }
    // The winding of the side wall is topologically the same for pulling out and pushing in
    // (edge a to b at the base meets the reversed edge of the neighbouring side face);
    // with a negative distance the wall normal flips geometrically on its own.
    for w in walls {
        let base = mesh.verts.len() as u32;
        mesh.verts.extend(w);
        mesh.tris.push([base, base + 1, base + 2]);
        mesh.tris.push([base, base + 2, base + 3]);
    }
    mesh.recompute_normals();
    new_top_tris
}

pub fn delete_faces(mesh: &mut Mesh, faces: &[usize]) {
    let kill: std::collections::HashSet<usize> = faces.iter().copied().collect();
    let tris: Vec<[u32; 3]> = mesh.tris.iter().enumerate().filter(|(i, _)| !kill.contains(i)).map(|(_, t)| *t).collect();
    mesh.tris = tris;
    mesh.cleanup();
}

/// Splits every triangle into 4 at the edge midpoints. The new vertices stay on the planes of the original faces.
pub fn subdivide(mesh: &mut Mesh) {
    let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
    let mut tris = Vec::with_capacity(mesh.tris.len() * 4);
    let old = mesh.tris.clone();
    for t in old {
        let mut m = [0u32; 3];
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let key = (a.min(b), a.max(b));
            m[k] = *mid.entry(key).or_insert_with(|| {
                let (va, vb) = (mesh.verts[a as usize].clone(), mesh.verts[b as usize].clone());
                let mut v = va.clone();
                v.pos = va.pos.lerp(vb.pos, 0.5);
                v.nrm = (va.nrm + vb.nrm).normalized();
                v.uv = (va.uv + vb.uv) * 0.5;
                for c in 0..3 {
                    v.color[c] = (va.color[c] + vb.color[c]) * 0.5;
                }
                // bone weights are taken from the nearest source vertex (good enough for low-poly meshes)
                mesh.verts.push(v);
                (mesh.verts.len() - 1) as u32
            });
        }
        tris.push([t[0], m[0], m[2]]);
        tris.push([m[0], t[1], m[1]]);
        tris.push([m[2], m[1], t[2]]);
        tris.push([m[0], m[1], m[2]]);
    }
    mesh.tris = tris;
}

/// Mirrors along X by adding a mirrored copy. Vertices on the symmetry plane are welded.
pub fn mirror_x(mesh: &mut Mesh, eps: f32) {
    let n = mesh.verts.len();
    let mut map = vec![0u32; n];
    for i in 0..n {
        let v = mesh.verts[i].clone();
        if v.pos.x.abs() <= eps {
            map[i] = i as u32;
        } else {
            let mut m = v;
            m.pos.x = -m.pos.x;
            m.nrm.x = -m.nrm.x;
            mesh.verts.push(m);
            map[i] = (mesh.verts.len() - 1) as u32;
        }
    }
    let copies: Vec<[u32; 3]> = mesh.tris.iter().map(|t| [map[t[0] as usize], map[t[2] as usize], map[t[1] as usize]]).collect();
    mesh.tris.extend(copies);
    mesh.cleanup();
}

/// Moves the vertices with the given indices by a delta. Used for sculpting with the mouse in 3D.
pub fn translate_vertices(mesh: &mut Mesh, ids: &[usize], delta: Vec3) {
    for &i in ids {
        if let Some(v) = mesh.verts.get_mut(i) {
            v.pos += delta;
        }
    }
}

/// Moves every vertex that shares a position with vertex `i` (so UV seams do not tear), with a soft falloff.
pub fn move_with_falloff(mesh: &mut Mesh, i: usize, delta: Vec3, radius: f32) {
    let Some(c) = mesh.verts.get(i).map(|v| v.pos) else { return };
    for v in &mut mesh.verts {
        let d = (v.pos - c).length();
        let k = if radius <= 0.0 { if d < 1e-4 { 1.0 } else { 0.0 } } else { (1.0 - d / radius).clamp(0.0, 1.0) };
        let k = k * k * (3.0 - 2.0 * k); // smoothstep
        if k > 0.0 {
            v.pos += delta * k;
        }
    }
}

/// Appends a second mesh to the first one (merging objects), shifted by `offset`.
pub fn append(dst: &mut Mesh, src: &Mesh, offset: Vec3) {
    let base = dst.verts.len() as u32;
    for v in &src.verts {
        let mut v = v.clone();
        v.pos += offset;
        dst.verts.push(v);
    }
    dst.tris.extend(src.tris.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
}

/// The signed volume of a closed mesh (the sign tells whether the normals point outward).
pub fn signed_volume(mesh: &Mesh) -> f32 {
    mesh.tris
        .iter()
        .map(|t| {
            let (a, b, c) = (mesh.verts[t[0] as usize].pos, mesh.verts[t[1] as usize].pos, mesh.verts[t[2] as usize].pos);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Closedness: every edge (by position) occurs in exactly two faces.
    fn is_closed(m: &Mesh) -> bool {
        let mut e: HashMap<((i64, i64, i64), (i64, i64, i64)), (u32, u32)> = HashMap::new();
        for t in &m.tris {
            for k in 0..3 {
                let (a, b) = (pkey(m.verts[t[k] as usize].pos), pkey(m.verts[t[(k + 1) % 3] as usize].pos));
                let (key, fwd) = if a <= b { ((a, b), true) } else { ((b, a), false) };
                let c = e.entry(key).or_insert((0, 0));
                if fwd { c.0 += 1 } else { c.1 += 1 }
            }
        }
        // closed and consistently oriented: every edge is traversed exactly once in each direction
        e.values().all(|&(f, b)| f == 1 && b == 1)
    }

    #[test]
    fn cube_is_closed_outward_volume_8() {
        let c = cube(Vec3::new(2.0, 2.0, 2.0));
        assert_eq!(c.tris.len(), 12);
        assert!(is_closed(&c));
        assert!((signed_volume(&c) - 8.0).abs() < 1e-4);
    }
    #[test]
    fn all_primitives_valid() {
        for m in [plane(2.0, 2.0, 3, 3), cylinder(1.0, 2.0, 8), cone(1.0, 2.0, 6), sphere(1.0, 8, 6), torus(2.0, 0.5, 10, 6)] {
            assert!(!m.tris.is_empty());
            assert!(m.tris.iter().flatten().all(|&i| (i as usize) < m.verts.len()));
        }
        assert!(is_closed(&cylinder(1.0, 2.0, 8)));
        assert!(is_closed(&sphere(1.0, 8, 6)));
        assert!(signed_volume(&sphere(1.0, 12, 8)) > 2.5, "sphere normals point outward, volume close to 4.19 times the inscribed fraction");
        assert!(signed_volume(&cylinder(1.0, 2.0, 16)) > 5.0);
        assert!(signed_volume(&torus(2.0, 0.5, 12, 8)) > 0.0, "torus normals point outward");
        assert!(signed_volume(&cone(1.0, 2.0, 12)) > 0.0);
    }
    #[test]
    fn extrude_top_of_cube() {
        let mut c = cube(Vec3::new(2.0, 2.0, 2.0));
        // the +Y face is the third quad (index 2), triangles 4 and 5
        let top = extrude_faces(&mut c, &[4, 5], 1.0);
        assert_eq!(top.len(), 2);
        assert_eq!(c.tris.len(), 12 + 8);
        assert!(is_closed(&c), "after extrude the mesh must stay closed and consistently oriented");
        assert!((signed_volume(&c) - 12.0).abs() < 1e-3, "volume {}", signed_volume(&c));
        let (_, hi) = c.bounds().unwrap();
        assert!((hi.y - 2.0).abs() < 1e-4);
    }
    #[test]
    fn extrude_inward_reduces_volume() {
        let mut c = cube(Vec3::new(2.0, 2.0, 2.0));
        extrude_faces(&mut c, &[4, 5], -0.5);
        assert!(is_closed(&c));
        assert!((signed_volume(&c) - 6.0).abs() < 1e-3, "volume {}", signed_volume(&c));
    }
    #[test]
    fn subdivide_and_flat_shade() {
        let mut c = cube(Vec3::new(1.0, 1.0, 1.0));
        subdivide(&mut c);
        assert_eq!(c.tris.len(), 48);
        assert!(is_closed(&c));
        flat_shade(&mut c);
        assert_eq!(c.verts.len(), c.tris.len() * 3);
        assert!(c.verts.iter().all(|v| (v.nrm.length() - 1.0).abs() < 1e-4));
    }
    #[test]
    fn weld_cube_gives_8_positions() {
        let mut c = cube(Vec3::new(1.0, 1.0, 1.0));
        weld(&mut c, false);
        assert_eq!(c.verts.len(), 8);
        assert_eq!(c.tris.len(), 12);
    }
    #[test]
    fn mirror_makes_symmetric_closed_mesh() {
        // half a cube with x in [0,1] becomes a closed cube with x in [-1,1] after mirroring
        let mut c = cube(Vec3::new(1.0, 1.0, 1.0));
        let ids: Vec<usize> = (0..c.verts.len()).collect();
        translate_vertices(&mut c, &ids, Vec3::new(0.5, 0.0, 0.0));
        // remove the -X face (it lies on the symmetry plane), triangles 2 and 3
        delete_faces(&mut c, &[2, 3]);
        mirror_x(&mut c, 1e-4);
        weld(&mut c, false);
        let (lo, hi) = c.bounds().unwrap();
        assert!((lo.x + 1.0).abs() < 1e-4 && (hi.x - 1.0).abs() < 1e-4);
        assert!(is_closed(&c));
        assert!(signed_volume(&c) > 0.0);
    }
    #[test]
    fn falloff_move_is_smooth() {
        let mut p = plane(4.0, 4.0, 4, 4);
        let i = 12; // centre
        move_with_falloff(&mut p, i, Vec3::new(0.0, 1.0, 0.0), 2.0);
        assert!((p.verts[i].pos.y - 1.0).abs() < 1e-5);
        assert!(p.verts[0].pos.y.abs() < 1e-5, "a distant vertex does not move");
    }
    #[test]
    fn model_from_mesh_ready_to_paint() {
        let m = model_from_mesh("Test", cube(Vec3::ONE), 64);
        assert_eq!(m.textures.len(), 1);
        assert_eq!(m.materials[0].texture, Some(0));
    }
}
