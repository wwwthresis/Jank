//! Editable polygon mesh: the data behind the Edit mode of Model mode.
//!
//! The renderer draws triangle lists whose vertices are split wherever a UV, a colour or a normal
//! changes (`mesh::Mesh`). Modelling needs something else: real polygons (quads and n-gons) that
//! share their vertices, edges that can be selected, marked sharp or as UV seams, and loose
//! (wire) edges. Edit mode therefore converts the meshes of a model into one [`EditMesh`], works
//! on it, and writes triangle meshes back after every change ([`EditMesh::to_meshes`]).
//!
//! * Vertices are shared by every polygon that touches them, so moving one never tears a seam.
//! * Per-corner data (UV, colour) lives in the polygon corners, as in Blender's face corners.
//! * Every polygon remembers its *part*: the source mesh (and with it the material) it belongs to.
//! * Smooth or flat shading is a per-polygon flag; `sharp` edges split smooth normals.
//! * The selection (vertices, edges, faces and the select mode) is part of the mesh, so undo
//!   restores it together with the geometry.
//!
//! The operators live in `ops` (extrude, inset, loop cut, merge, dissolve, fill, bridge, knife…),
//! `bevel` (edge bevel with segments), `select` (loops, rings, paths, grow/shrink) and `uv`.
mod bevel;
mod boolean;
mod ops;
mod select;
mod uv;

pub use bevel::BevelOptions;
pub use boolean::BoolOp;
pub use ops::{DeleteKind, MergeKind};

use crate::math::*;
use crate::mesh::{Mesh, Vertex};
use std::collections::{BTreeSet, HashMap};

/// An undirected edge, stored with the smaller vertex index first (see [`ek`]).
pub type Edge = (u32, u32);

/// The canonical key of the edge between `a` and `b`.
#[inline]
pub fn ek(a: u32, b: u32) -> Edge {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EVert {
    pub pos: Vec3,
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}
impl EVert {
    pub fn at(pos: Vec3) -> EVert {
        EVert { pos, joints: [0; 4], weights: [0.0; 4] }
    }
}

/// One corner of a polygon: the vertex it uses and the attributes that may differ per polygon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corner {
    pub v: u32,
    pub uv: Vec2,
    pub color: [f32; 3],
}
impl Corner {
    pub fn new(v: u32) -> Corner {
        Corner { v, uv: Vec2::default(), color: [1.0; 3] }
    }
    /// Blend of two corners (for new corners on a split edge).
    pub fn lerp(&self, o: &Corner, t: f32, v: u32) -> Corner {
        Corner {
            v,
            uv: Vec2::new(self.uv.x + (o.uv.x - self.uv.x) * t, self.uv.y + (o.uv.y - self.uv.y) * t),
            color: [0, 1, 2].map(|i| self.color[i] + (o.color[i] - self.color[i]) * t),
        }
    }
    fn same_attrs(&self, o: &Corner) -> bool {
        (self.uv.x - o.uv.x).abs() < 1e-5 && (self.uv.y - o.uv.y).abs() < 1e-5 && (0..3).all(|i| (self.color[i] - o.color[i]).abs() < 1e-4)
    }
}

/// A polygon with three or more corners in counter-clockwise order (seen from outside).
#[derive(Clone, Debug, PartialEq)]
pub struct EFace {
    pub c: Vec<Corner>,
    /// Index into [`EditMesh::parts`]: the source mesh and material.
    pub part: u16,
    pub smooth: bool,
}
impl EFace {
    pub fn verts(&self) -> impl Iterator<Item = u32> + '_ {
        self.c.iter().map(|c| c.v)
    }
    /// Directed edges `(v[i], v[i+1])`.
    pub fn edges(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let n = self.c.len();
        (0..n).map(move |i| (self.c[i].v, self.c[(i + 1) % n].v))
    }
    pub fn has_vert(&self, v: u32) -> bool {
        self.c.iter().any(|c| c.v == v)
    }
    pub fn corner_of(&self, v: u32) -> Option<usize> {
        self.c.iter().position(|c| c.v == v)
    }
}

/// A source mesh of the model: its name and material survive the round trip through Edit mode.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: String,
    pub material: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum SelectMode {
    #[default]
    Vertex,
    Edge,
    Face,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct EditMesh {
    pub verts: Vec<EVert>,
    pub faces: Vec<EFace>,
    /// Loose edges that belong to no polygon.
    pub wires: BTreeSet<Edge>,
    /// Edges that split smooth shading.
    pub sharp: BTreeSet<Edge>,
    /// UV seams (also set automatically where the UVs of two polygons do not meet).
    pub seams: BTreeSet<Edge>,
    pub hidden_v: BTreeSet<u32>,
    pub hidden_f: BTreeSet<u32>,
    pub parts: Vec<Part>,
    pub mode: SelectMode,
    pub sel_v: BTreeSet<u32>,
    pub sel_e: BTreeSet<Edge>,
    pub sel_f: BTreeSet<u32>,
    /// The most recently clicked vertex (used by "merge at last" and path selection).
    pub active: Option<u32>,
}

/// Adjacency built on demand (meshes here are small, so it is simply rebuilt after each change).
pub struct Topo {
    /// Faces using each edge.
    pub edge_faces: HashMap<Edge, Vec<u32>>,
    /// Faces using each vertex.
    pub vert_faces: Vec<Vec<u32>>,
    /// Neighbouring vertices of each vertex (through polygon edges and wires).
    pub vert_nbrs: Vec<Vec<u32>>,
}
impl Topo {
    pub fn faces_of(&self, e: Edge) -> &[u32] {
        self.edge_faces.get(&e).map_or(&[], |v| v.as_slice())
    }
    pub fn is_boundary(&self, e: Edge) -> bool {
        self.faces_of(e).len() == 1
    }
}

fn qkey(p: Vec3) -> (i64, i64, i64) {
    ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64, (p.z * 1e4).round() as i64)
}

/// Newell normal of a polygon (not normalised; its length is twice the area).
pub fn newell(pts: &[Vec3]) -> Vec3 {
    let mut n = Vec3::ZERO;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n
}

/// Triangulates a simple polygon (convex or concave) by ear clipping in its own plane.
/// Returns indices into `pts`. Degenerate polygons fall back to a fan.
pub fn triangulate(pts: &[Vec3]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return vec![];
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }
    let nrm = newell(pts).normalized();
    if nrm == Vec3::ZERO {
        return (1..n - 1).map(|i| [0, i, i + 1]).collect();
    }
    // 2D basis in the polygon plane
    let ax = if nrm.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
    let u = ax.cross(nrm).normalized();
    let v = nrm.cross(u);
    let p2: Vec<(f32, f32)> = pts.iter().map(|p| (p.dot(u), p.dot(v))).collect();
    let cross = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    if n == 4 {
        // two ways to split a quad: take the valid one with the better (shorter) diagonal
        let ok = |a: usize, b: usize, c: usize| cross(p2[a], p2[b], p2[c]) > 1e-12;
        let d02 = (pts[0] - pts[2]).length();
        let d13 = (pts[1] - pts[3]).length();
        let s02 = ok(0, 1, 2) && ok(0, 2, 3);
        let s13 = ok(1, 2, 3) && ok(1, 3, 0);
        return if s02 && (!s13 || d02 <= d13 + 1e-6) { vec![[0, 1, 2], [0, 2, 3]] } else if s13 { vec![[1, 2, 3], [1, 3, 0]] } else { vec![[0, 1, 2], [0, 2, 3]] };
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    let inside = |p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)| cross(a, b, p) >= -1e-9 && cross(b, c, p) >= -1e-9 && cross(c, a, p) >= -1e-9;
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (p2[ia], p2[ib], p2[ic]);
            if cross(a, b, c) <= 1e-12 {
                continue; // reflex or degenerate corner
            }
            if idx.iter().any(|&j| j != ia && j != ib && j != ic && inside(p2[j], a, b, c)) {
                continue;
            }
            out.push([ia, ib, ic]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if idx.len() >= 3 {
        for i in 1..idx.len() - 1 {
            out.push([idx[0], idx[i], idx[i + 1]]);
        }
    }
    out
}

/// Simple union-find used to group polygon corners into smooth-normal fans.
struct Dsu(Vec<u32>);
impl Dsu {
    fn new(n: usize) -> Dsu {
        Dsu((0..n as u32).collect())
    }
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            let p = self.0[self.0[x as usize] as usize];
            self.0[x as usize] = p;
            x = p;
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra as usize] = rb;
        }
    }
}

impl EditMesh {
    // ------------------------------------------------------------------ conversion

    /// Builds an edit mesh from the meshes of a model: vertices at the same position (within one
    /// mesh) are welded, pairs of triangles that form a clean flat quad become one quad, edges where
    /// the UVs of two polygons do not meet become seams, and split normals become sharp edges.
    pub fn from_meshes(meshes: &[Mesh]) -> EditMesh {
        let mut em = EditMesh::default();
        let mut tri_normals: Vec<[Vec3; 3]> = vec![];
        for (pi, m) in meshes.iter().enumerate() {
            em.parts.push(Part { name: m.name.clone(), material: m.material });
            let mut weld: HashMap<(i64, i64, i64), u32> = HashMap::new();
            let map: Vec<u32> = m
                .verts
                .iter()
                .map(|v| {
                    *weld.entry(qkey(v.pos)).or_insert_with(|| {
                        em.verts.push(EVert { pos: v.pos, joints: v.joints, weights: v.weights });
                        (em.verts.len() - 1) as u32
                    })
                })
                .collect();
            for t in &m.tris {
                let c: Vec<Corner> = t
                    .iter()
                    .map(|&i| {
                        let v = &m.verts[i as usize];
                        Corner { v: map[i as usize], uv: v.uv, color: v.color }
                    })
                    .collect();
                if c[0].v == c[1].v || c[1].v == c[2].v || c[0].v == c[2].v {
                    continue;
                }
                let p = |k: usize| m.verts[t[k] as usize].pos;
                let fnrm = (p(1) - p(0)).cross(p(2) - p(0)).normalized();
                let ns = [0, 1, 2].map(|k| m.verts[t[k] as usize].nrm);
                let smooth = ns.iter().any(|n| n.dot(fnrm) < 0.9995);
                em.faces.push(EFace { c, part: pi as u16, smooth });
                tri_normals.push(ns);
            }
        }
        // seams and sharp edges from the split vertices of the source
        let topo = em.topo();
        for (e, fs) in &topo.edge_faces {
            if fs.len() != 2 {
                continue;
            }
            let (f, g) = (&em.faces[fs[0] as usize], &em.faces[fs[1] as usize]);
            let mut uv_split = false;
            let mut n_split = false;
            for v in [e.0, e.1] {
                let (ci, cj) = (f.corner_of(v).unwrap(), g.corner_of(v).unwrap());
                if !f.c[ci].same_attrs(&g.c[cj]) {
                    uv_split = true;
                }
                if f.smooth && g.smooth && tri_normals[fs[0] as usize][ci].dot(tri_normals[fs[1] as usize][cj]) < 0.999 {
                    n_split = true;
                }
            }
            if uv_split {
                em.seams.insert(*e);
            }
            if n_split {
                em.sharp.insert(*e);
            }
        }
        let all: Vec<u32> = (0..em.faces.len() as u32).collect();
        em.tris_to_quads(&all, 1.0);
        em
    }

    /// Writes the polygons back as triangle meshes, one per part. Also returns, for every part,
    /// which polygon each output triangle came from (used to map picked triangles to polygons).
    pub fn to_meshes(&self) -> (Vec<Mesh>, Vec<Vec<u32>>) {
        let fnorm: Vec<Vec3> = self.faces.iter().map(|f| newell(&self.face_points(f))).collect();
        // corner ids: offset of each face's first corner
        let mut start = Vec::with_capacity(self.faces.len());
        let mut total = 0u32;
        for f in &self.faces {
            start.push(total);
            total += f.c.len() as u32;
        }
        let mut dsu = Dsu::new(total as usize);
        let mut emap: HashMap<Edge, Vec<(u32, usize)>> = HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            if !f.smooth {
                continue;
            }
            let n = f.c.len();
            for k in 0..n {
                emap.entry(ek(f.c[k].v, f.c[(k + 1) % n].v)).or_default().push((fi as u32, k));
            }
        }
        for (e, list) in &emap {
            if list.len() != 2 || self.sharp.contains(e) {
                continue;
            }
            let ((f, k), (g, l)) = (list[0], list[1]);
            let (nf, ng) = (self.faces[f as usize].c.len(), self.faces[g as usize].c.len());
            let (fa, fb) = (self.faces[f as usize].c[k].v, self.faces[f as usize].c[(k + 1) % nf].v);
            let (ga, _gb) = (self.faces[g as usize].c[l].v, self.faces[g as usize].c[(l + 1) % ng].v);
            // the two faces see the edge in opposite directions when their winding agrees
            let (gk_a, gk_b) = if ga == fb { ((l + 1) % ng, l) } else { (l, (l + 1) % ng) };
            let _ = fa;
            dsu.union(start[f as usize] + k as u32, start[g as usize] + gk_a as u32);
            dsu.union(start[f as usize] + ((k + 1) % nf) as u32, start[g as usize] + gk_b as u32);
        }
        let mut acc = vec![Vec3::ZERO; total as usize];
        for (fi, f) in self.faces.iter().enumerate() {
            if !f.smooth {
                continue;
            }
            for k in 0..f.c.len() {
                let r = dsu.find(start[fi] + k as u32);
                acc[r as usize] += fnorm[fi];
            }
        }
        let mut meshes: Vec<Mesh> = self.parts.iter().map(|p| Mesh { name: p.name.clone(), verts: vec![], tris: vec![], material: p.material }).collect();
        if meshes.is_empty() {
            meshes.push(Mesh::new("Mesh"));
        }
        let mut tri_face: Vec<Vec<u32>> = vec![vec![]; meshes.len()];
        let mut dedupe: Vec<HashMap<(u32, [u32; 2], [u32; 3], [u32; 3]), u32>> = vec![HashMap::new(); meshes.len()];
        for (fi, f) in self.faces.iter().enumerate() {
            if f.c.len() < 3 {
                continue;
            }
            let part = (f.part as usize).min(meshes.len() - 1);
            let flat = fnorm[fi].normalized();
            let ids: Vec<u32> = (0..f.c.len())
                .map(|k| {
                    let c = &f.c[k];
                    let nrm = if f.smooth { acc[dsu.find(start[fi] + k as u32) as usize].normalized() } else { flat };
                    let nrm = if nrm == Vec3::ZERO { Vec3::Y } else { nrm };
                    let key = (c.v, [c.uv.x.to_bits(), c.uv.y.to_bits()], c.color.map(f32::to_bits), [nrm.x.to_bits(), nrm.y.to_bits(), nrm.z.to_bits()]);
                    let mesh = &mut meshes[part];
                    *dedupe[part].entry(key).or_insert_with(|| {
                        let ev = &self.verts[c.v as usize];
                        mesh.verts.push(Vertex { pos: ev.pos, nrm, uv: c.uv, color: c.color, joints: ev.joints, weights: ev.weights });
                        (mesh.verts.len() - 1) as u32
                    })
                })
                .collect();
            for t in triangulate(&self.face_points(f)) {
                meshes[part].tris.push([ids[t[0]], ids[t[1]], ids[t[2]]]);
                tri_face[part].push(fi as u32);
            }
        }
        (meshes, tri_face)
    }

    // ------------------------------------------------------------------ geometry helpers

    pub fn face_points(&self, f: &EFace) -> Vec<Vec3> {
        f.c.iter().map(|c| self.verts[c.v as usize].pos).collect()
    }
    pub fn face_normal(&self, fi: u32) -> Vec3 {
        newell(&self.face_points(&self.faces[fi as usize])).normalized()
    }
    pub fn face_center(&self, fi: u32) -> Vec3 {
        let f = &self.faces[fi as usize];
        let mut s = Vec3::ZERO;
        for c in &f.c {
            s += self.verts[c.v as usize].pos;
        }
        s / f.c.len().max(1) as f32
    }
    pub fn pos(&self, v: u32) -> Vec3 {
        self.verts[v as usize].pos
    }
    /// Area-weighted vertex normal (average of the surrounding polygons).
    pub fn vert_normal(&self, topo: &Topo, v: u32) -> Vec3 {
        let mut n = Vec3::ZERO;
        for &f in &topo.vert_faces[v as usize] {
            n += newell(&self.face_points(&self.faces[f as usize]));
        }
        n.normalized()
    }

    pub fn topo(&self) -> Topo {
        let mut edge_faces: HashMap<Edge, Vec<u32>> = HashMap::new();
        let mut vert_faces = vec![vec![]; self.verts.len()];
        let mut vert_nbrs: Vec<Vec<u32>> = vec![vec![]; self.verts.len()];
        let link = |a: u32, b: u32, nb: &mut Vec<Vec<u32>>| {
            if !nb[a as usize].contains(&b) {
                nb[a as usize].push(b);
            }
            if !nb[b as usize].contains(&a) {
                nb[b as usize].push(a);
            }
        };
        for (fi, f) in self.faces.iter().enumerate() {
            for (a, b) in f.edges() {
                let list = edge_faces.entry(ek(a, b)).or_default();
                if !list.contains(&(fi as u32)) {
                    list.push(fi as u32);
                }
                link(a, b, &mut vert_nbrs);
            }
            for v in f.verts() {
                let l = &mut vert_faces[v as usize];
                if !l.contains(&(fi as u32)) {
                    l.push(fi as u32);
                }
            }
        }
        for &(a, b) in &self.wires {
            if (a as usize) < self.verts.len() && (b as usize) < self.verts.len() {
                link(a, b, &mut vert_nbrs);
            }
        }
        Topo { edge_faces, vert_faces, vert_nbrs }
    }

    /// Every edge (polygon edges and wires) once.
    pub fn all_edges(&self) -> Vec<Edge> {
        let mut s: BTreeSet<Edge> = self.wires.clone();
        for f in &self.faces {
            for (a, b) in f.edges() {
                s.insert(ek(a, b));
            }
        }
        s.into_iter().collect()
    }

    pub fn add_vert(&mut self, pos: Vec3) -> u32 {
        self.verts.push(EVert::at(pos));
        (self.verts.len() - 1) as u32
    }
    /// A copy of vertex `v` (keeping its skin weights) at a new position.
    pub fn dup_vert(&mut self, v: u32, pos: Vec3) -> u32 {
        let mut e = self.verts[v as usize].clone();
        e.pos = pos;
        self.verts.push(e);
        (self.verts.len() - 1) as u32
    }

    /// Removes polygons that collapsed (fewer than three distinct corners), repeated corners,
    /// duplicate polygons and vertices nothing uses, and fixes every index set.
    pub fn compact(&mut self) {
        // drop consecutive repeated corners, then faces that became too small
        for f in &mut self.faces {
            let mut out: Vec<Corner> = Vec::with_capacity(f.c.len());
            for c in &f.c {
                if out.last().map_or(true, |l: &Corner| l.v != c.v) {
                    out.push(*c);
                }
            }
            while out.len() > 1 && out.first().map(|c| c.v) == out.last().map(|c| c.v) {
                out.pop();
            }
            f.c = out;
        }
        let mut seen: std::collections::HashSet<Vec<u32>> = std::collections::HashSet::new();
        let mut keep_face = vec![true; self.faces.len()];
        for (i, f) in self.faces.iter().enumerate() {
            let mut key: Vec<u32> = f.verts().collect();
            key.sort_unstable();
            let distinct = {
                let mut k = key.clone();
                k.dedup();
                k.len()
            };
            if distinct < 3 || distinct != key.len() || !seen.insert(key) {
                keep_face[i] = false;
            }
        }
        let mut fmap = vec![u32::MAX; self.faces.len()];
        let mut nf = 0u32;
        for i in 0..self.faces.len() {
            if keep_face[i] {
                fmap[i] = nf;
                nf += 1;
            }
        }
        let mut k = 0;
        self.faces.retain(|_| {
            k += 1;
            keep_face[k - 1]
        });
        let remap_f = |s: &BTreeSet<u32>| s.iter().filter_map(|&f| fmap.get(f as usize).copied().filter(|&x| x != u32::MAX)).collect::<BTreeSet<u32>>();
        self.sel_f = remap_f(&self.sel_f);
        self.hidden_f = remap_f(&self.hidden_f);
        // wires that duplicate a polygon edge are not loose
        let face_edges: BTreeSet<Edge> = self.faces.iter().flat_map(|f| f.edges().map(|(a, b)| ek(a, b)).collect::<Vec<_>>()).collect();
        self.wires = self.wires.iter().copied().filter(|e| e.0 != e.1 && !face_edges.contains(e)).collect();
        // unused vertices
        let mut used = vec![false; self.verts.len()];
        for f in &self.faces {
            for v in f.verts() {
                used[v as usize] = true;
            }
        }
        for &(a, b) in &self.wires {
            used[a as usize] = true;
            used[b as usize] = true;
        }
        let mut vmap = vec![u32::MAX; self.verts.len()];
        let mut nv = 0u32;
        for (i, u) in used.iter().enumerate() {
            if *u {
                vmap[i] = nv;
                nv += 1;
            }
        }
        if nv as usize != self.verts.len() {
            let mut i = 0;
            self.verts.retain(|_| {
                i += 1;
                used[i - 1]
            });
            for f in &mut self.faces {
                for c in &mut f.c {
                    c.v = vmap[c.v as usize];
                }
            }
        }
        let rv = |v: u32| vmap.get(v as usize).copied().filter(|&x| x != u32::MAX);
        let re = |s: &BTreeSet<Edge>| s.iter().filter_map(|&(a, b)| Some(ek(rv(a)?, rv(b)?))).collect::<BTreeSet<Edge>>();
        self.wires = re(&self.wires);
        self.sharp = re(&self.sharp).intersection(&face_edges_after(self)).copied().collect();
        self.seams = re(&self.seams);
        self.sel_e = re(&self.sel_e);
        self.sel_v = self.sel_v.iter().filter_map(|&v| rv(v)).collect();
        self.hidden_v = self.hidden_v.iter().filter_map(|&v| rv(v)).collect();
        self.active = self.active.and_then(rv);
        let all = self.all_edges().into_iter().collect::<BTreeSet<Edge>>();
        self.seams = self.seams.intersection(&all).copied().collect();
        self.sel_e = self.sel_e.intersection(&all).copied().collect();
        if self.parts.is_empty() {
            self.parts.push(Part { name: "Mesh".into(), material: 0 });
        }
        self.sync_selection();
    }

    /// Reverses the corner order of a polygon (flips it).
    pub fn flip_face(&mut self, fi: u32) {
        self.faces[fi as usize].c.reverse();
    }

    /// A cheap fingerprint of triangle meshes, used to tell whether a stored edit mesh still
    /// matches a model that may have been changed by other tools.
    pub fn fingerprint(meshes: &[Mesh]) -> u64 {
        let mut k = 0xcbf29ce484222325u64;
        let mut mix = |x: u64| k = (k ^ x).wrapping_mul(0x100000001b3);
        for m in meshes {
            mix(m.verts.len() as u64);
            mix(m.tris.len() as u64);
            mix(m.material as u64);
            for v in &m.verts {
                mix(v.pos.x.to_bits() as u64 | (v.pos.y.to_bits() as u64) << 32);
                mix(v.pos.z.to_bits() as u64 | (v.uv.x.to_bits() as u64) << 32);
                mix(v.uv.y.to_bits() as u64);
            }
            for t in &m.tris {
                mix(t[0] as u64 | (t[1] as u64) << 21 | (t[2] as u64) << 42);
            }
        }
        k
    }

    /// Number of (visible) polygons by size: (triangles, quads, n-gons).
    pub fn face_stats(&self) -> (usize, usize, usize) {
        let mut s = (0, 0, 0);
        for f in &self.faces {
            match f.c.len() {
                3 => s.0 += 1,
                4 => s.1 += 1,
                _ => s.2 += 1,
            }
        }
        s
    }
}

fn face_edges_after(em: &EditMesh) -> BTreeSet<Edge> {
    em.faces.iter().flat_map(|f| f.edges().map(|(a, b)| ek(a, b)).collect::<Vec<_>>()).collect()
}

#[cfg(test)]
mod tests;
