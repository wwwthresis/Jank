//! Mesh operators of Edit mode. Every operator works on the current selection, leaves a valid
//! mesh behind (no dangling indices, consistent winding where it can) and selects its result,
//! so the next step (usually a move) can follow at once.
use super::*;
use crate::fasthash::{HashMap, HashSet};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteKind {
    Verts,
    Edges,
    Faces,
    /// Removes the polygons but keeps their edges and vertices.
    OnlyFaces,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeKind {
    /// Every selected vertex to their common centre.
    Center,
    /// Every selected vertex to the active (last clicked) one.
    AtActive,
    /// Each connected group of selected vertices to its own centre.
    Collapse,
}


impl EditMesh {
    // ------------------------------------------------------------------ low-level building blocks

    /// Inserts a new vertex on edge (a, b) at fraction `t` from `a`, in every polygon that uses the
    /// edge, and splits a wire edge the same way. Returns the new vertex.
    pub fn split_edge(&mut self, a: u32, b: u32, t: f32) -> u32 {
        let p = self.pos(a).lerp(self.pos(b), t);
        let mut v = self.dup_vert(if t < 0.5 { a } else { b }, p);
        // skin weights: take the nearer end (blending weights of different bones is not meaningful here)
        let _ = &mut v;
        for f in &mut self.faces {
            let n = f.c.len();
            let mut k = 0;
            while k < f.c.len() {
                let (c0, c1) = (f.c[k], f.c[(k + 1) % f.c.len()]);
                if c0.v == a && c1.v == b {
                    f.c.insert(k + 1, c0.lerp(&c1, t, v));
                    break;
                }
                if c0.v == b && c1.v == a {
                    f.c.insert(k + 1, c0.lerp(&c1, 1.0 - t, v));
                    break;
                }
                k += 1;
            }
            let _ = n;
        }
        let e = ek(a, b);
        if self.wires.remove(&e) {
            self.wires.insert(ek(a, v));
            self.wires.insert(ek(v, b));
        }
        for set in [&mut self.sharp, &mut self.seams] {
            if set.remove(&e) {
                set.insert(ek(a, v));
                set.insert(ek(v, b));
            }
        }
        v
    }

    /// Splits polygon `fi` along the chord between corners `i` and `j` (not neighbours).
    /// The polygon keeps corners i..=j; a new polygon gets j..=i. Returns the new polygon.
    pub fn split_face(&mut self, fi: u32, i: usize, j: usize) -> Option<u32> {
        let f = self.faces.get(fi as usize)?.clone();
        let n = f.c.len();
        if i == j || (i + 1) % n == j || (j + 1) % n == i {
            return None;
        }
        let mut a = vec![];
        let mut k = i;
        loop {
            a.push(f.c[k]);
            if k == j {
                break;
            }
            k = (k + 1) % n;
        }
        let mut b = vec![];
        let mut k = j;
        loop {
            b.push(f.c[k]);
            if k == i {
                break;
            }
            k = (k + 1) % n;
        }
        self.faces[fi as usize].c = a;
        self.faces.push(EFace { c: b, part: f.part, smooth: f.smooth });
        Some((self.faces.len() - 1) as u32)
    }

    /// Removes back-and-forth corners (x, y, x) left over after merging polygons around a vertex.
    fn remove_spikes(c: &mut Vec<Corner>) {
        loop {
            let n = c.len();
            if n < 3 {
                return;
            }
            let mut hit = None;
            for i in 0..n {
                if c[(i + n - 1) % n].v == c[(i + 1) % n].v {
                    hit = Some(i);
                    break;
                }
            }
            let Some(i) = hit else { return };
            let j = (i + 1) % n;
            // drop the tip and one copy of the repeated corner
            let (hi, lo) = if i > j { (i, j) } else { (j, i) };
            c.remove(hi);
            c.remove(lo);
        }
    }

    /// Copies `other` into this mesh (its parts are appended) and selects only the copy.
    pub fn append(&mut self, other: &EditMesh, offset: Vec3) {
        let vbase = self.verts.len() as u32;
        let pbase = self.parts.len() as u16;
        self.verts.extend(other.verts.iter().map(|v| EVert { pos: v.pos + offset, ..v.clone() }));
        self.parts.extend(other.parts.iter().cloned());
        let fbase = self.faces.len() as u32;
        for f in &other.faces {
            self.faces.push(EFace { c: f.c.iter().map(|c| Corner { v: c.v + vbase, ..*c }).collect(), part: f.part + pbase, smooth: f.smooth });
        }
        self.wires.extend(other.wires.iter().map(|&(a, b)| (a + vbase, b + vbase)));
        self.sharp.extend(other.sharp.iter().map(|&(a, b)| (a + vbase, b + vbase)));
        self.seams.extend(other.seams.iter().map(|&(a, b)| (a + vbase, b + vbase)));
        self.sel_v = (vbase..self.verts.len() as u32).collect();
        self.sel_f = (fbase..self.faces.len() as u32).collect();
        self.sel_e.clear();
        let mode = self.mode;
        self.mode = SelectMode::Vertex;
        self.sync_selection();
        self.mode = mode;
        if mode == SelectMode::Face {
            self.sel_f = (fbase..self.faces.len() as u32).collect();
        }
        self.sync_selection();
    }

    /// Like [`append`](Self::append) but merges the parts of `other` into part `part` of this mesh
    /// (used when adding a primitive while editing: it takes the material of the edited mesh).
    pub fn append_into_part(&mut self, other: &EditMesh, offset: Vec3, part: u16) {
        let mut o = other.clone();
        o.parts.clear();
        for f in &mut o.faces {
            f.part = 0;
        }
        let n = self.parts.len() as u16;
        self.append(&o, offset);
        for f in self.faces.iter_mut() {
            if f.part >= n {
                f.part = part;
            }
        }
        self.parts.truncate(n as usize);
    }

    // ------------------------------------------------------------------ extrude

    /// Extrude (E): selected faces as one region, otherwise selected edges, otherwise vertices.
    /// The new geometry is selected and stays in place; the caller moves it (returns the direction
    /// to move along: the average normal of what was extruded).
    pub fn extrude(&mut self) -> Vec3 {
        if !self.sel_f.is_empty() {
            let region = self.sel_f.clone();
            return self.extrude_region(&region);
        }
        if !self.sel_e.is_empty() {
            return self.extrude_edges();
        }
        self.extrude_verts()
    }

    fn extrude_region(&mut self, region: &BTreeSet<u32>) -> Vec3 {
        let topo = self.topo();
        let mut normal = Vec3::ZERO;
        for &f in region {
            normal += newell(&self.face_points(&self.faces[f as usize]));
        }
        // boundary edges of the region, directed as they appear in the region's faces
        let mut boundary: Vec<(u32, u32, u32, usize)> = vec![]; // (a, b, face, corner index of a)
        for &f in region {
            let face = &self.faces[f as usize];
            let n = face.c.len();
            for k in 0..n {
                let (a, b) = (face.c[k].v, face.c[(k + 1) % n].v);
                let inside = topo.faces_of(ek(a, b)).iter().filter(|g| region.contains(g)).count();
                if inside == 1 {
                    boundary.push((a, b, f, k));
                }
            }
        }
        // vertices that must stay behind: those on the region's rim or used outside the region
        let region_verts: BTreeSet<u32> = region.iter().flat_map(|&f| self.faces[f as usize].verts().collect::<Vec<_>>()).collect();
        let rim: BTreeSet<u32> = boundary.iter().flat_map(|&(a, b, _, _)| [a, b]).collect();
        let mut map: HashMap<u32, u32> = HashMap::default();
        for &v in &region_verts {
            let outside = topo.vert_faces[v as usize].iter().any(|g| !region.contains(g)) || self.wires.iter().any(|&(a, b)| a == v || b == v);
            if rim.contains(&v) || outside {
                let p = self.pos(v);
                map.insert(v, self.dup_vert(v, p));
            }
        }
        // side walls
        for &(a, b, f, k) in &boundary {
            let face = &self.faces[f as usize];
            let n = face.c.len();
            let (ca, cb) = (face.c[k], face.c[(k + 1) % n]);
            let (na, nb) = (*map.get(&a).unwrap_or(&a), *map.get(&b).unwrap_or(&b));
            let wall = vec![ca, cb, Corner { v: nb, ..cb }, Corner { v: na, ..ca }];
            let (part, smooth) = (face.part, face.smooth);
            self.faces.push(EFace { c: wall, part, smooth });
        }
        for &f in region {
            for c in &mut self.faces[f as usize].c {
                if let Some(&n) = map.get(&c.v) {
                    c.v = n;
                }
            }
        }
        // sharp/seam marks travel with the cap
        self.sel_f = region.clone();
        self.mode_flush_faces();
        normal.normalized()
    }

    pub(super) fn mode_flush_faces(&mut self) {
        let f = self.sel_f.clone();
        self.sel_v = f.iter().flat_map(|&x| self.faces[x as usize].verts().collect::<Vec<_>>()).collect();
        self.sel_e = f.iter().flat_map(|&x| self.faces[x as usize].edges().map(|(a, b)| ek(a, b)).collect::<Vec<_>>()).collect();
        if self.mode != SelectMode::Face {
            self.sync_selection();
        }
    }

    fn extrude_edges(&mut self) -> Vec3 {
        let topo = self.topo();
        let edges: Vec<Edge> = self.sel_e.iter().copied().collect();
        let mut map: HashMap<u32, u32> = HashMap::default();
        let mut normal = Vec3::ZERO;
        let mut new_edges = vec![];
        for &(a, b) in &edges {
            for v in [a, b] {
                if !map.contains_key(&v) {
                    let p = self.pos(v);
                    let n = self.dup_vert(v, p);
                    map.insert(v, n);
                }
            }
            let (na, nb) = (map[&a], map[&b]);
            let fs = topo.faces_of((a, b)).to_vec();
            let (mut ca, mut cb) = (Corner::new(a), Corner::new(b));
            let mut part = 0;
            let mut smooth = false;
            // orient the new quad against the polygon that already uses the edge
            let quad = if let Some(&f) = fs.first() {
                let face = &self.faces[f as usize];
                normal += newell(&self.face_points(face));
                part = face.part;
                smooth = face.smooth;
                ca = face.c[face.corner_of(a).unwrap()];
                cb = face.c[face.corner_of(b).unwrap()];
                let forward = face.edges().any(|(x, y)| x == a && y == b);
                if forward {
                    vec![cb, ca, Corner { v: na, ..ca }, Corner { v: nb, ..cb }]
                } else {
                    vec![ca, cb, Corner { v: nb, ..cb }, Corner { v: na, ..ca }]
                }
            } else {
                vec![ca, cb, Corner { v: nb, ..cb }, Corner { v: na, ..ca }]
            };
            self.wires.remove(&(a, b));
            self.faces.push(EFace { c: quad, part, smooth });
            new_edges.push(ek(na, nb));
        }
        // isolated selected vertices without a selected edge become wire extrusions
        let in_edges: BTreeSet<u32> = edges.iter().flat_map(|e| [e.0, e.1]).collect();
        let lone: Vec<u32> = self.sel_v.iter().copied().filter(|v| !in_edges.contains(v)).collect();
        let mut new_verts: BTreeSet<u32> = map.values().copied().collect();
        for v in lone {
            let p = self.pos(v);
            let n = self.dup_vert(v, p);
            self.wires.insert(ek(v, n));
            new_verts.insert(n);
        }
        if normal == Vec3::ZERO {
            normal = Vec3::Y;
        }
        self.sel_f.clear();
        self.sel_e = new_edges.into_iter().collect();
        self.sel_v = new_verts;
        if self.mode == SelectMode::Face {
            self.mode = SelectMode::Edge;
        }
        self.sync_selection();
        normal.normalized()
    }

    fn extrude_verts(&mut self) -> Vec3 {
        let topo = self.topo();
        let mut normal = Vec3::ZERO;
        let mut new = BTreeSet::new();
        for v in self.sel_v.clone() {
            normal += self.vert_normal(&topo, v);
            let p = self.pos(v);
            let n = self.dup_vert(v, p);
            self.wires.insert(ek(v, n));
            new.insert(n);
        }
        self.sel_v = new;
        self.sel_e.clear();
        self.sel_f.clear();
        self.mode = SelectMode::Vertex;
        self.sync_selection();
        if normal == Vec3::ZERO {
            Vec3::Y
        } else {
            normal.normalized()
        }
    }

    /// Extrudes every selected face on its own (Alt+E → Individual faces). Every face keeps its
    /// index and all of them stay selected; returns how many were extruded.
    pub fn extrude_individual(&mut self) -> usize {
        let faces: Vec<u32> = self.sel_f.iter().copied().collect();
        for &f in &faces {
            let one: BTreeSet<u32> = [f].into();
            self.extrude_region(&one);
        }
        self.sel_f = faces.iter().copied().collect();
        self.mode_flush_faces();
        faces.len()
    }

    // ------------------------------------------------------------------ inset

    /// Inset (I): makes a smaller copy of the selected faces inside them, joined by a rim of quads.
    /// `individual` insets every face on its own; `depth` pushes the inner faces along their normal.
    pub fn inset(&mut self, thickness: f32, depth: f32, individual: bool) -> bool {
        let region: Vec<u32> = self.sel_f.iter().copied().collect();
        if region.is_empty() {
            return false;
        }
        if individual {
            for f in region.clone() {
                self.inset_region(&[f].into(), thickness, depth);
            }
        } else {
            self.inset_region(&region.iter().copied().collect(), thickness, depth);
        }
        self.sel_f = region.into_iter().collect();
        self.mode_flush_faces();
        true
    }

    fn inset_region(&mut self, region: &BTreeSet<u32>, thickness: f32, depth: f32) {
        let topo = self.topo();
        let mut next: HashMap<u32, u32> = HashMap::default();
        let mut prev: HashMap<u32, u32> = HashMap::default();
        let mut boundary = vec![];
        for &f in region {
            let face = &self.faces[f as usize];
            let n = face.c.len();
            for k in 0..n {
                let (a, b) = (face.c[k].v, face.c[(k + 1) % n].v);
                if topo.faces_of(ek(a, b)).iter().filter(|g| region.contains(g)).count() == 1 {
                    next.insert(a, b);
                    prev.insert(b, a);
                    boundary.push((f, k));
                }
            }
        }
        // per-vertex normal over the region
        let mut vn: HashMap<u32, Vec3> = HashMap::default();
        for &f in region {
            let n = newell(&self.face_points(&self.faces[f as usize]));
            for v in self.faces[f as usize].verts() {
                *vn.entry(v).or_insert(Vec3::ZERO) += n;
            }
        }
        let mut map: HashMap<u32, u32> = HashMap::default();
        for (&v, &nx) in &next {
            let Some(&pv) = prev.get(&v) else { continue };
            let n = vn.get(&v).copied().unwrap_or(Vec3::Y).normalized();
            let p = self.pos(v);
            let d1 = (p - self.pos(pv)).normalized();
            let d2 = (self.pos(nx) - p).normalized();
            let in1 = n.cross(d1).normalized();
            let in2 = n.cross(d2).normalized();
            let den = (1.0 + in1.dot(in2)).max(0.25);
            let m = (in1 + in2) * (thickness / den);
            let nv = self.dup_vert(v, p + m + n * depth);
            map.insert(v, nv);
        }
        // interior vertices of the region follow the depth
        let interior: BTreeSet<u32> = region.iter().flat_map(|&f| self.faces[f as usize].verts().collect::<Vec<_>>()).filter(|v| !map.contains_key(v)).collect();
        for v in interior {
            let n = vn.get(&v).copied().unwrap_or(Vec3::Y).normalized();
            self.verts[v as usize].pos = self.pos(v) + n * depth;
        }
        for &(f, k) in &boundary {
            let face = &self.faces[f as usize];
            let n = face.c.len();
            let (ca, cb) = (face.c[k], face.c[(k + 1) % n]);
            let (Some(&na), Some(&nb)) = (map.get(&ca.v), map.get(&cb.v)) else { continue };
            let rim = vec![ca, cb, Corner { v: nb, ..cb }, Corner { v: na, ..ca }];
            let (part, smooth) = (face.part, face.smooth);
            self.faces.push(EFace { c: rim, part, smooth });
        }
        for &f in region {
            for c in &mut self.faces[f as usize].c {
                if let Some(&n) = map.get(&c.v) {
                    c.v = n;
                }
            }
        }
    }

    // ------------------------------------------------------------------ loop cut

    /// The ring through `e` with every edge oriented consistently (so a slide factor means the same side).
    fn oriented_ring(&self, topo: &Topo, e: Edge) -> (Vec<(u32, u32)>, Vec<(u32, usize)>) {
        let mut edges: Vec<(u32, u32)> = vec![(e.0, e.1)];
        let mut quads: Vec<(u32, usize)> = vec![]; // (face, corner index of the ring edge's start in that face)
        let mut seen: BTreeSet<Edge> = [ek(e.0, e.1)].into();
        let mut seen_f: BTreeSet<u32> = BTreeSet::new();
        let fs = topo.faces_of(ek(e.0, e.1)).to_vec();
        for (side, &start) in fs.iter().enumerate().take(2) {
            let (mut cur, mut f) = ((e.0, e.1), start);
            let mut chain: Vec<(u32, u32)> = vec![];
            loop {
                if !seen_f.insert(f) {
                    break;
                }
                let face = &self.faces[f as usize];
                if face.c.len() != 4 {
                    break;
                }
                // corner index k where the face has edge cur in either direction
                let Some(k) = (0..4).find(|&k| {
                    let (a, b) = (face.c[k].v, face.c[(k + 1) % 4].v);
                    (a, b) == cur || (b, a) == cur
                }) else {
                    break;
                };
                let (a, b) = (face.c[k].v, face.c[(k + 1) % 4].v);
                // opposite edge, oriented so that its first vertex faces cur.0
                let (p2, p3) = (face.c[(k + 2) % 4].v, face.c[(k + 3) % 4].v);
                let opp = if (a, b) == cur { (p3, p2) } else { (p2, p3) };
                quads.push((f, k));
                if !seen.insert(ek(opp.0, opp.1)) {
                    break;
                }
                chain.push(opp);
                let Some(&g) = topo.faces_of(ek(opp.0, opp.1)).iter().find(|&&g| g != f) else { break };
                cur = opp;
                f = g;
            }
            if side == 0 {
                edges.extend(chain);
            } else {
                let mut c = chain;
                c.reverse();
                c.extend(edges);
                edges = c;
            }
        }
        (edges, quads)
    }

    /// Edge loops across the ring of `e` (Ctrl+R). `cuts` new loops; with one cut, `slide` in -1..1
    /// moves it towards either side. Returns false when `e` crosses no quad.
    pub fn loop_cut(&mut self, e: Edge, cuts: usize, slide: f32) -> bool {
        let topo = self.topo();
        let (edges, quads) = self.oriented_ring(&topo, e);
        if quads.is_empty() {
            return false;
        }
        let cuts = cuts.clamp(1, 32);
        let ts: Vec<f32> = if cuts == 1 { vec![(0.5 + slide.clamp(-0.98, 0.98) * 0.5).clamp(0.01, 0.99)] } else { (1..=cuts).map(|k| k as f32 / (cuts + 1) as f32).collect() };
        // new vertices per oriented ring edge
        let mut cutv: HashMap<(u32, u32), Vec<u32>> = HashMap::default();
        for &(a, b) in &edges {
            let ids: Vec<u32> = ts
                .iter()
                .map(|&t| {
                    let p = self.pos(a).lerp(self.pos(b), t);
                    self.dup_vert(if t < 0.5 { a } else { b }, p)
                })
                .collect();
            cutv.insert((a, b), ids);
        }
        let get = |a: u32, b: u32, cutv: &HashMap<(u32, u32), Vec<u32>>| -> Option<(Vec<u32>, bool)> {
            if let Some(v) = cutv.get(&(a, b)) {
                return Some((v.clone(), false));
            }
            cutv.get(&(b, a)).map(|v| (v.clone(), true))
        };
        let quad_set: BTreeSet<u32> = quads.iter().map(|q| q.0).collect();
        let mut new_faces: Vec<EFace> = vec![];
        let mut drop: BTreeSet<u32> = BTreeSet::new();
        for (fi, face) in self.faces.iter_mut().enumerate() {
            let fi = fi as u32;
            if quad_set.contains(&fi) {
                // rotate so that corner 0..1 is a ring edge and 2..3 the opposite one
                let k = (0..4).find(|&k| get(face.c[k].v, face.c[(k + 1) % 4].v, &cutv).is_some() && get(face.c[(k + 2) % 4].v, face.c[(k + 3) % 4].v, &cutv).is_some());
                let Some(k) = k else { continue };
                let c: Vec<Corner> = (0..4).map(|i| face.c[(k + i) % 4]).collect();
                let (s, s_rev) = get(c[0].v, c[1].v, &cutv).unwrap();
                let (t, t_rev) = get(c[3].v, c[2].v, &cutv).unwrap();
                // parameter of every cut vertex measured from c0 (and from c3 on the opposite edge)
                let s_corner = |i: usize| -> Corner {
                    let tt = if s_rev { 1.0 - ts[ts.len() - 1 - i] } else { ts[i] };
                    let v = if s_rev { s[ts.len() - 1 - i] } else { s[i] };
                    c[0].lerp(&c[1], tt, v)
                };
                let t_corner = |i: usize| -> Corner {
                    let tt = if t_rev { 1.0 - ts[ts.len() - 1 - i] } else { ts[i] };
                    let v = if t_rev { t[ts.len() - 1 - i] } else { t[i] };
                    c[3].lerp(&c[2], tt, v)
                };
                let n = ts.len();
                let mut rows: Vec<(Corner, Corner)> = vec![(c[0], c[3])];
                for i in 0..n {
                    rows.push((s_corner(i), t_corner(i)));
                }
                rows.push((c[1], c[2]));
                for w in rows.windows(2) {
                    new_faces.push(EFace { c: vec![w[0].0, w[1].0, w[1].1, w[0].1], part: face.part, smooth: face.smooth });
                }
                drop.insert(fi);
            } else {
                // a polygon at the end of the ring gets the new vertices in its outline
                let mut k = 0;
                while k < face.c.len() {
                    let n = face.c.len();
                    let (c0, c1) = (face.c[k], face.c[(k + 1) % n]);
                    if let Some((ids, rev)) = get(c0.v, c1.v, &cutv) {
                        let m = ids.len();
                        for i in 0..m {
                            let (v, tt) = if rev { (ids[m - 1 - i], 1.0 - ts[m - 1 - i]) } else { (ids[i], ts[i]) };
                            face.c.insert(k + 1 + i, c0.lerp(&c1, tt, v));
                        }
                        k += m;
                    }
                    k += 1;
                }
            }
        }
        let mut idx = 0u32;
        self.faces.retain(|_| {
            idx += 1;
            !drop.contains(&(idx - 1))
        });
        self.faces.extend(new_faces);
        // the new loops become the selection
        let mut sel = BTreeSet::new();
        for ids in cutv.values() {
            sel.extend(ids.iter().copied());
        }
        self.sel_v = sel;
        self.sel_f.clear();
        self.sel_e.clear();
        let mode = self.mode;
        self.mode = SelectMode::Vertex;
        self.sync_selection();
        self.mode = if mode == SelectMode::Face { SelectMode::Edge } else { mode };
        if self.mode == SelectMode::Edge {
            // keep the loop edges themselves selected
            let s = self.sel_v.clone();
            self.sel_e = self.all_edges().into_iter().filter(|e| s.contains(&e.0) && s.contains(&e.1)).collect();
        }
        self.sync_selection();
        self.compact();
        true
    }

    // ------------------------------------------------------------------ subdivide, connect, knife

    /// Subdivide: splits the selected edges in the middle; fully selected polygons become a grid
    /// (quads into four quads, triangles into four triangles, n-gons into a fan of quads).
    pub fn subdivide(&mut self) -> bool {
        let edges: Vec<Edge> = self.sel_e.iter().copied().collect();
        if edges.is_empty() {
            return false;
        }
        let full: BTreeSet<u32> = self.sel_f.clone();
        let full_c: Vec<Vec<u32>> = full.iter().map(|&f| self.faces[f as usize].verts().collect()).collect();
        let mut mids: HashMap<Edge, u32> = HashMap::default();
        for &(a, b) in &edges {
            let m = self.split_edge(a, b, 0.5);
            mids.insert((a, b), m);
        }
        let mut new_sel: BTreeSet<u32> = self.sel_v.clone();
        new_sel.extend(mids.values().copied());
        // grid inside the fully selected polygons
        let mut added = vec![];
        for (i, &f) in full.iter().enumerate() {
            let orig = &full_c[i];
            let face = self.faces[f as usize].clone();
            let corners = face.c.clone();
            let is_orig = |v: u32| orig.contains(&v);
            if orig.len() == 3 {
                // triangle: three corner triangles and the middle one
                let pos_of = |v: u32| corners.iter().position(|c| c.v == v).unwrap();
                let o: Vec<usize> = orig.iter().map(|&v| pos_of(v)).collect();
                let n = corners.len();
                let m = |k: usize| corners[(o[k] + 1) % n];
                let (m0, m1, m2) = (m(0), m(1), m(2));
                let c = |k: usize| corners[o[k]];
                if corners.len() != 6 {
                    continue;
                }
                self.faces[f as usize].c = vec![c(0), m0, m2];
                for t in [vec![m0, c(1), m1], vec![m2, m1, c(2)], vec![m0, m1, m2]] {
                    added.push(EFace { c: t, part: face.part, smooth: face.smooth });
                }
            } else {
                // centre vertex and a quad per original corner
                let center = corners.iter().fold(Vec3::ZERO, |s, c| s + self.verts[c.v as usize].pos) / corners.len() as f32;
                let cv = self.dup_vert(corners[0].v, center);
                new_sel.insert(cv);
                let cuv = corners.iter().fold(Vec2::default(), |s, c| s + c.uv) * (1.0 / corners.len() as f32);
                let ccol = [0, 1, 2].map(|i| corners.iter().map(|c| c.color[i]).sum::<f32>() / corners.len() as f32);
                let cc = Corner { v: cv, uv: cuv, color: ccol };
                let n = corners.len();
                let mut quads = vec![];
                for k in 0..n {
                    if !is_orig(corners[k].v) {
                        continue;
                    }
                    let prev = corners[(k + n - 1) % n];
                    let next = corners[(k + 1) % n];
                    if is_orig(prev.v) || is_orig(next.v) {
                        continue; // only polygons whose every edge was split
                    }
                    quads.push(vec![prev, corners[k], next, cc]);
                }
                if quads.len() != orig.len() {
                    continue;
                }
                self.faces[f as usize].c = quads.remove(0);
                for q in quads {
                    added.push(EFace { c: q, part: face.part, smooth: face.smooth });
                }
            }
        }
        let base = self.faces.len() as u32;
        self.faces.extend(added);
        let _ = base;
        let s = new_sel;
        let mode = self.mode;
        self.mode = SelectMode::Vertex;
        self.sel_v = s;
        self.sync_selection();
        self.mode = mode;
        if mode == SelectMode::Edge {
            let sv = self.sel_v.clone();
            self.sel_e = self.all_edges().into_iter().filter(|e| sv.contains(&e.0) && sv.contains(&e.1)).collect();
        }
        if mode == SelectMode::Face {
            let sv = self.sel_v.clone();
            self.sel_f = (0..self.faces.len() as u32).filter(|&f| self.faces[f as usize].verts().all(|v| sv.contains(&v))).collect();
        }
        self.sync_selection();
        true
    }

    /// Connect vertex path (J): splits polygons between their selected corners.
    pub fn connect_verts(&mut self) -> usize {
        let sel = self.sel_v.clone();
        let mut made = 0;
        let mut new_edges = vec![];
        let count = self.faces.len();
        for fi in 0..count {
            let n = self.faces[fi].c.len();
            let picks: Vec<usize> = (0..n).filter(|&k| sel.contains(&self.faces[fi].c[k].v)).collect();
            if picks.len() < 2 {
                continue;
            }
            if picks.len() == 2 {
                let (i, j) = (picks[0], picks[1]);
                let (vi, vj) = (self.faces[fi].c[i].v, self.faces[fi].c[j].v);
                if self.split_face(fi as u32, i, j).is_some() {
                    made += 1;
                    new_edges.push(ek(vi, vj));
                }
                continue;
            }
            // more than two: cut off every piece between consecutive picks, keep the inner polygon
            let face = self.faces[fi].clone();
            let mut pieces = vec![];
            for w in 0..picks.len() {
                let (i, j) = (picks[w], picks[(w + 1) % picks.len()]);
                let len = (j + n - i) % n;
                if len <= 1 {
                    continue;
                }
                let piece: Vec<Corner> = (0..=len).map(|s| face.c[(i + s) % n]).collect();
                new_edges.push(ek(face.c[i].v, face.c[j].v));
                pieces.push(piece);
            }
            if pieces.is_empty() {
                continue;
            }
            self.faces[fi].c = picks.iter().map(|&k| face.c[k]).collect();
            for p in pieces {
                self.faces.push(EFace { c: p, part: face.part, smooth: face.smooth });
                made += 1;
            }
        }
        if made > 0 {
            self.compact();
            if self.mode == SelectMode::Edge {
                self.sel_e.extend(new_edges);
                self.sync_selection();
            }
        }
        made
    }

    /// Cuts the candidate polygons with a plane (the knife and bisect tools). `keep` decides,
    /// for an intersection point, whether it lies on the drawn cut (for a knife line: between its
    /// ends on screen). Returns the number of polygons that were split.
    pub fn cut_plane(&mut self, faces: &BTreeSet<u32>, origin: Vec3, normal: Vec3, keep: &dyn Fn(Vec3) -> bool) -> usize {
        let n = normal.normalized();
        if n == Vec3::ZERO {
            return 0;
        }
        let d = |p: Vec3| (p - origin).dot(n);
        let scale = self.verts.iter().fold(0.0f32, |m, v| m.max(v.pos.length())).max(1.0);
        let eps = 1e-5 * scale;
        // edges of the candidates that cross the plane
        let mut cuts: Vec<(u32, u32, f32)> = vec![];
        let mut seen = BTreeSet::new();
        for &f in faces {
            let Some(face) = self.faces.get(f as usize) else { continue };
            for (a, b) in face.edges() {
                if !seen.insert(ek(a, b)) {
                    continue;
                }
                let (da, db) = (d(self.pos(a)), d(self.pos(b)));
                if (da > eps && db < -eps) || (da < -eps && db > eps) {
                    let t = da / (da - db);
                    let p = self.pos(a).lerp(self.pos(b), t);
                    if keep(p) {
                        cuts.push((a, b, t));
                    }
                }
            }
        }
        let mut on_plane: BTreeSet<u32> = BTreeSet::new();
        for (a, b, t) in cuts {
            let v = self.split_edge(a, b, t);
            on_plane.insert(v);
        }
        for &f in faces {
            if let Some(face) = self.faces.get(f as usize) {
                for v in face.verts() {
                    if d(self.pos(v)).abs() <= eps && keep(self.pos(v)) {
                        on_plane.insert(v);
                    }
                }
            }
        }
        let mut split = 0;
        let mut new_edges = vec![];
        let mut work: Vec<u32> = faces.iter().copied().collect();
        while let Some(f) = work.pop() {
            let Some(face) = self.faces.get(f as usize) else { continue };
            let picks: Vec<usize> = (0..face.c.len()).filter(|&k| on_plane.contains(&face.c[k].v)).collect();
            if picks.len() < 2 {
                continue;
            }
            // split at the first pair that is not already an edge of the polygon
            let nc = face.c.len();
            let mut done = false;
            'outer: for x in 0..picks.len() {
                for y in x + 1..picks.len() {
                    let (i, j) = (picks[x], picks[y]);
                    if (i + 1) % nc == j || (j + 1) % nc == i {
                        continue;
                    }
                    let (vi, vj) = (face.c[i].v, face.c[j].v);
                    // the chord must actually run inside the polygon: its midpoint on the plane side test
                    if let Some(g) = self.split_face(f, i, j) {
                        split += 1;
                        new_edges.push(ek(vi, vj));
                        work.push(f);
                        work.push(g);
                    }
                    done = true;
                    break 'outer;
                }
            }
            let _ = done;
        }
        if split > 0 || !on_plane.is_empty() {
            self.sel_v = on_plane;
            self.sel_f.clear();
            self.sel_e.clear();
            let mode = self.mode;
            self.mode = SelectMode::Vertex;
            self.sync_selection();
            self.mode = if mode == SelectMode::Face { SelectMode::Edge } else { mode };
            if self.mode == SelectMode::Edge {
                self.sel_e = new_edges.into_iter().collect();
            }
            self.sync_selection();
        }
        split
    }

    // ------------------------------------------------------------------ merge

    fn remap_verts(&mut self, map: &HashMap<u32, u32>) {
        if map.is_empty() {
            return;
        }
        let r = |v: u32| *map.get(&v).unwrap_or(&v);
        for f in &mut self.faces {
            for c in &mut f.c {
                c.v = r(c.v);
            }
        }
        let re = |s: &BTreeSet<Edge>| s.iter().map(|&(a, b)| ek(r(a), r(b))).filter(|e| e.0 != e.1).collect::<BTreeSet<Edge>>();
        self.wires = re(&self.wires);
        self.sharp = re(&self.sharp);
        self.seams = re(&self.seams);
        self.sel_e = re(&self.sel_e);
        self.sel_v = self.sel_v.iter().map(|&v| r(v)).collect();
        self.active = self.active.map(r);
    }

    /// Merge (M). Returns the number of vertices removed.
    pub fn merge(&mut self, kind: MergeKind) -> usize {
        let sel: Vec<u32> = self.sel_v.iter().copied().collect();
        if sel.len() < 2 {
            return 0;
        }
        let groups: Vec<Vec<u32>> = match kind {
            MergeKind::Center | MergeKind::AtActive => vec![sel.clone()],
            MergeKind::Collapse => {
                // connected groups through selected edges
                let s: BTreeSet<u32> = sel.iter().copied().collect();
                let topo = self.topo();
                let mut seen = BTreeSet::new();
                let mut out = vec![];
                for &v in &sel {
                    if !seen.insert(v) {
                        continue;
                    }
                    let mut g = vec![v];
                    let mut i = 0;
                    while i < g.len() {
                        for &nb in &topo.vert_nbrs[g[i] as usize] {
                            if s.contains(&nb) && seen.insert(nb) {
                                g.push(nb);
                            }
                        }
                        i += 1;
                    }
                    out.push(g);
                }
                out
            }
        };
        let before = self.verts.len();
        let mut map = HashMap::default();
        let mut keep = BTreeSet::new();
        for g in groups {
            if g.len() < 2 {
                keep.insert(g[0]);
                continue;
            }
            let target = match kind {
                MergeKind::AtActive => self.active.filter(|a| g.contains(a)).unwrap_or(*g.last().unwrap()),
                _ => g[0],
            };
            let p = match kind {
                MergeKind::AtActive => self.pos(target),
                _ => g.iter().fold(Vec3::ZERO, |s, &v| s + self.pos(v)) / g.len() as f32,
            };
            self.verts[target as usize].pos = p;
            for &v in &g {
                if v != target {
                    map.insert(v, target);
                }
            }
            keep.insert(target);
        }
        self.remap_verts(&map);
        self.sel_v = keep;
        let mode = self.mode;
        self.mode = SelectMode::Vertex;
        self.sync_selection();
        self.mode = mode;
        self.compact();
        before - self.verts.len()
    }

    /// Merge by distance: welds selected vertices (all, when nothing is selected) that are closer
    /// than `dist`. Returns how many vertices were removed.
    pub fn merge_by_distance(&mut self, dist: f32) -> usize {
        let cand: Vec<u32> = if self.sel_v.is_empty() { (0..self.verts.len() as u32).collect() } else { self.sel_v.iter().copied().collect() };
        let dist = dist.max(1e-7);
        let cell = |p: Vec3| ((p.x / dist).floor() as i64, (p.y / dist).floor() as i64, (p.z / dist).floor() as i64);
        let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::default();
        for &v in &cand {
            grid.entry(cell(self.pos(v))).or_default().push(v);
        }
        let mut map: HashMap<u32, u32> = HashMap::default();
        let mut taken: HashSet<u32> = HashSet::default();
        for &v in &cand {
            if taken.contains(&v) {
                continue;
            }
            taken.insert(v);
            let (cx, cy, cz) = cell(self.pos(v));
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(list) = grid.get(&(cx + dx, cy + dy, cz + dz)) {
                            for &o in list {
                                if !taken.contains(&o) && (self.pos(o) - self.pos(v)).length() <= dist {
                                    taken.insert(o);
                                    map.insert(o, v);
                                }
                            }
                        }
                    }
                }
            }
        }
        let before = self.verts.len();
        self.remap_verts(&map);
        self.compact();
        before - self.verts.len()
    }

    // ------------------------------------------------------------------ dissolve and delete

    /// Merges the two polygons on each side of every selected edge into one (dissolve edges).
    pub fn dissolve_edges(&mut self) -> usize {
        let edges: Vec<Edge> = self.sel_e.iter().copied().collect();
        let n = self.dissolve_edge_list(&edges);
        if n > 0 {
            self.compact();
        }
        n
    }

    fn dissolve_edge_list(&mut self, edges: &[Edge]) -> usize {
        let mut dead: BTreeSet<u32> = BTreeSet::new();
        let mut merged = 0;
        let mut keep_sel: BTreeSet<u32> = BTreeSet::new();
        for &(a, b) in edges {
            // find the live faces using this edge now
            let fs: Vec<u32> = (0..self.faces.len() as u32).filter(|f| !dead.contains(f) && self.faces[*f as usize].edges().any(|(x, y)| ek(x, y) == (a, b))).collect();
            if fs.len() != 2 {
                continue;
            }
            let (f, g) = (fs[0], fs[1]);
            let (ff, gg) = (&self.faces[f as usize], &self.faces[g as usize]);
            let nf = ff.c.len();
            let ng = gg.c.len();
            let Some(kf) = (0..nf).find(|&k| ek(ff.c[k].v, ff.c[(k + 1) % nf].v) == (a, b)) else { continue };
            let Some(kg) = (0..ng).find(|&k| ek(gg.c[k].v, gg.c[(k + 1) % ng].v) == (a, b)) else { continue };
            if ff.c[kf].v == gg.c[kg].v {
                continue; // inconsistent winding: merging would twist the polygon
            }
            // f: from the end of the shared edge around to its start; g: the same on the other side
            let fpart: Vec<Corner> = (1..=nf).map(|s| ff.c[(kf + s) % nf]).collect(); // b .. a
            let gpart: Vec<Corner> = (1..=ng).map(|s| gg.c[(kg + s) % ng]).collect(); // a .. b
            let mut c = fpart;
            c.extend_from_slice(&gpart[1..gpart.len() - 1]);
            Self::remove_spikes(&mut c);
            self.faces[f as usize].c = c;
            dead.insert(g);
            keep_sel.insert(f);
            merged += 1;
            self.sharp.remove(&(a, b));
            self.seams.remove(&(a, b));
        }
        for &d in &dead {
            self.faces[d as usize].c.clear();
        }
        if merged > 0 {
            self.sel_f = keep_sel.difference(&dead).copied().collect();
            self.mode_flush_faces();
        }
        merged
    }

    /// Merges every connected group of selected polygons into one polygon (dissolve faces).
    pub fn dissolve_faces(&mut self) -> usize {
        let topo = self.topo();
        let sel = self.sel_f.clone();
        let inner: Vec<Edge> = topo.edge_faces.iter().filter(|(_, fs)| fs.len() == 2 && fs.iter().all(|f| sel.contains(f))).map(|(e, _)| *e).collect();
        let mut inner = inner;
        inner.sort();
        let n = self.dissolve_edge_list(&inner);
        // vertices that end up inside a polygon with only two neighbours are removed
        self.compact();
        n
    }

    /// Removes the selected vertices but keeps the surface closed by merging the polygons around them.
    pub fn dissolve_verts(&mut self) -> usize {
        let verts: Vec<u32> = self.sel_v.iter().copied().collect();
        let mut count = 0;
        for v in verts {
            let topo = self.topo();
            let edges: Vec<Edge> = topo.vert_nbrs[v as usize].iter().map(|&n| ek(v, n)).filter(|e| topo.faces_of(*e).len() == 2).collect();
            self.dissolve_edge_list(&edges);
            // remove v from the polygons that still use it
            for f in &mut self.faces {
                if f.c.len() > 3 {
                    if let Some(k) = f.corner_of(v) {
                        f.c.remove(k);
                    }
                }
            }
            self.wires.retain(|&(a, b)| a != v && b != v);
            count += 1;
        }
        self.sel_v.clear();
        self.sel_e.clear();
        self.sel_f.clear();
        self.compact();
        count
    }

    /// Delete (X).
    pub fn delete(&mut self, kind: DeleteKind) -> bool {
        if self.sel_v.is_empty() {
            return false;
        }
        match kind {
            DeleteKind::Verts => {
                let s = self.sel_v.clone();
                for f in &mut self.faces {
                    if f.verts().any(|v| s.contains(&v)) {
                        f.c.clear();
                    }
                }
                self.wires.retain(|e| !s.contains(&e.0) && !s.contains(&e.1));
                // the selected vertices themselves go as well, even if they were alone
                for &v in &s {
                    self.hidden_v.remove(&v);
                }
                let keep: BTreeSet<u32> = BTreeSet::new();
                self.sel_v = keep;
            }
            DeleteKind::Edges => {
                let s = self.sel_e.clone();
                for f in &mut self.faces {
                    if f.edges().any(|(a, b)| s.contains(&ek(a, b))) {
                        f.c.clear();
                    }
                }
                self.wires.retain(|e| !s.contains(e));
            }
            DeleteKind::Faces => {
                for &f in &self.sel_f {
                    self.faces[f as usize].c.clear();
                }
            }
            DeleteKind::OnlyFaces => {
                let sel = self.sel_f.clone();
                let mut keep_edges = vec![];
                for &f in &sel {
                    keep_edges.extend(self.faces[f as usize].edges().map(|(a, b)| ek(a, b)));
                }
                for &f in &sel {
                    self.faces[f as usize].c.clear();
                }
                let rest: BTreeSet<Edge> = self.faces.iter().flat_map(|f| f.edges().map(|(a, b)| ek(a, b)).collect::<Vec<_>>()).collect();
                for e in keep_edges {
                    if !rest.contains(&e) {
                        self.wires.insert(e);
                    }
                }
            }
        }
        self.sel_v.clear();
        self.sel_e.clear();
        self.sel_f.clear();
        self.compact();
        true
    }

    // ------------------------------------------------------------------ fill, bridge

    /// Chains a set of edges into loops or open paths (vertex sequences).
    fn chains(edges: &BTreeSet<Edge>) -> Vec<(Vec<u32>, bool)> {
        let mut adj: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &(a, b) in edges {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
        let mut used: BTreeSet<Edge> = BTreeSet::new();
        let mut out = vec![];
        // open chains start at a vertex with one neighbour
        let starts: Vec<u32> = adj.iter().filter(|(_, n)| n.len() == 1).map(|(v, _)| *v).chain(adj.keys().copied()).collect();
        for s in starts {
            if adj[&s].iter().all(|&n| used.contains(&ek(s, n))) {
                continue;
            }
            let mut path = vec![s];
            let mut cur = s;
            let mut closed = false;
            loop {
                let Some(&nx) = adj[&cur].iter().find(|&&n| !used.contains(&ek(cur, n))) else { break };
                used.insert(ek(cur, nx));
                if nx == s {
                    closed = true;
                    break;
                }
                path.push(nx);
                cur = nx;
                if adj[&cur].len() > 2 {
                    break;
                }
            }
            out.push((path, closed));
        }
        out
    }

    /// Fill (F): closed loops of selected boundary edges become polygons; two lone vertices get an
    /// edge; three or more selected vertices without edges become one polygon.
    pub fn fill(&mut self) -> bool {
        let topo = self.topo();
        let sel_e: BTreeSet<Edge> = self.sel_e.iter().copied().filter(|e| topo.faces_of(*e).len() < 2).collect();
        let mut made = false;
        let mut new_faces = vec![];
        for (path, closed) in Self::chains(&sel_e) {
            if !closed || path.len() < 3 {
                continue;
            }
            // orient against an existing neighbour polygon
            let mut verts = path.clone();
            let mut flip = false;
            for i in 0..verts.len() {
                let (a, b) = (verts[i], verts[(i + 1) % verts.len()]);
                if let Some(&f) = topo.faces_of(ek(a, b)).first() {
                    flip = self.faces[f as usize].edges().any(|(x, y)| x == a && y == b);
                    break;
                }
            }
            if flip {
                verts.reverse();
            }
            let (part, smooth) = self.neighbour_part(&topo, &verts);
            let c = verts.iter().map(|&v| self.corner_for(&topo, v)).collect();
            for i in 0..verts.len() {
                self.wires.remove(&ek(verts[i], verts[(i + 1) % verts.len()]));
            }
            new_faces.push(EFace { c, part, smooth });
            made = true;
        }
        if !made {
            let lone: Vec<u32> = self.sel_v.iter().copied().collect();
            if lone.len() == 2 && !topo.vert_nbrs[lone[0] as usize].contains(&lone[1]) {
                self.wires.insert(ek(lone[0], lone[1]));
                self.sync_selection();
                return true;
            }
            if lone.len() >= 3 && self.sel_e.is_empty() {
                // order around the centre in the best-fit plane
                let c = lone.iter().fold(Vec3::ZERO, |s, &v| s + self.pos(v)) / lone.len() as f32;
                let mut n = Vec3::ZERO;
                for i in 0..lone.len() {
                    n += (self.pos(lone[i]) - c).cross(self.pos(lone[(i + 1) % lone.len()]) - c);
                }
                let mut nn = lone.iter().fold(Vec3::ZERO, |s, &v| s + self.vert_normal(&topo, v));
                if nn == Vec3::ZERO {
                    nn = if n == Vec3::ZERO { Vec3::Y } else { n };
                }
                let nn = nn.normalized();
                let ax = if nn.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
                let u = ax.cross(nn).normalized();
                let w = nn.cross(u);
                let mut order = lone.clone();
                order.sort_by(|&a, &b| {
                    let (pa, pb) = (self.pos(a) - c, self.pos(b) - c);
                    pa.dot(w).atan2(pa.dot(u)).partial_cmp(&pb.dot(w).atan2(pb.dot(u))).unwrap_or(std::cmp::Ordering::Equal)
                });
                let (part, smooth) = self.neighbour_part(&topo, &order);
                let cs = order.iter().map(|&v| self.corner_for(&topo, v)).collect();
                new_faces.push(EFace { c: cs, part, smooth });
                made = true;
            }
        }
        let base = self.faces.len() as u32;
        self.faces.extend(new_faces);
        if made {
            self.sel_f = (base..self.faces.len() as u32).collect();
            self.mode_flush_faces();
            self.compact();
        }
        made
    }

    pub(super) fn neighbour_part(&self, topo: &Topo, verts: &[u32]) -> (u16, bool) {
        for &v in verts {
            if let Some(&f) = topo.vert_faces.get(v as usize).and_then(|l| l.first()) {
                let face = &self.faces[f as usize];
                return (face.part, face.smooth);
            }
        }
        (0, false)
    }
    /// Corner attributes for a vertex taken from any polygon that already uses it.
    pub(super) fn corner_for(&self, topo: &Topo, v: u32) -> Corner {
        if let Some(&f) = topo.vert_faces.get(v as usize).and_then(|l| l.first()) {
            let face = &self.faces[f as usize];
            if let Some(k) = face.corner_of(v) {
                return face.c[k];
            }
        }
        Corner::new(v)
    }

    /// Bridge edge loops: joins two selected loops (or open chains) with the same number of
    /// vertices by a band of quads. Selected faces inside the loops are removed first.
    pub fn bridge(&mut self) -> bool {
        if !self.sel_f.is_empty() {
            // bridging two face regions: use their outlines, drop the faces
            let topo = self.topo();
            let sel = self.sel_f.clone();
            let rim: BTreeSet<Edge> = topo.edge_faces.iter().filter(|(_, fs)| fs.iter().filter(|f| sel.contains(f)).count() == 1).map(|(e, _)| *e).collect();
            for &f in &sel {
                self.faces[f as usize].c.clear();
            }
            self.sel_f.clear();
            self.sel_e = rim;
            let mode = self.mode;
            self.mode = SelectMode::Edge;
            self.sel_v = self.sel_e.iter().flat_map(|e| [e.0, e.1]).collect();
            let ok = self.bridge_edges();
            self.mode = mode;
            self.compact();
            return ok;
        }
        let ok = self.bridge_edges();
        if ok {
            self.compact();
        }
        ok
    }

    fn bridge_edges(&mut self) -> bool {
        let loops = Self::chains(&self.sel_e);
        if loops.len() != 2 || loops[0].0.len() != loops[1].0.len() || loops[0].1 != loops[1].1 || loops[0].0.len() < 2 {
            return false;
        }
        let closed = loops[0].1;
        let a = loops[0].0.clone();
        let mut b = loops[1].0.clone();
        let n = a.len();
        let cost = |a: &[u32], b: &[u32], m: &EditMesh| a.iter().zip(b).map(|(&x, &y)| (m.pos(x) - m.pos(y)).length()).sum::<f32>();
        // best direction and (for loops) best starting vertex
        let mut best: Option<(f32, Vec<u32>)> = None;
        for rev in [false, true] {
            let mut bb = b.clone();
            if rev {
                bb.reverse();
            }
            let shifts = if closed { n } else { 1 };
            for s in 0..shifts {
                let cand: Vec<u32> = (0..n).map(|i| bb[(i + s) % n]).collect();
                let c = cost(&a, &cand, self);
                if best.as_ref().map_or(true, |(bc, _)| c < *bc) {
                    best = Some((c, cand));
                }
            }
        }
        b = best.unwrap().1;
        let topo = self.topo();
        let segs = if closed { n } else { n - 1 };
        let mut quads: Vec<Vec<u32>> = (0..segs).map(|i| vec![a[i], a[(i + 1) % n], b[(i + 1) % n], b[i]]).collect();
        // orient against the polygons already using loop A
        let mut flip = false;
        for q in &quads {
            if let Some(&f) = topo.faces_of(ek(q[0], q[1])).first() {
                flip = self.faces[f as usize].edges().any(|(x, y)| x == q[0] && y == q[1]);
                break;
            }
            if let Some(&f) = topo.faces_of(ek(q[2], q[3])).first() {
                flip = self.faces[f as usize].edges().any(|(x, y)| x == q[2] && y == q[3]);
                break;
            }
        }
        if flip {
            for q in &mut quads {
                q.reverse();
            }
        }
        let (part, smooth) = self.neighbour_part(&topo, &a);
        let base = self.faces.len() as u32;
        for q in quads {
            let c = q.iter().map(|&v| self.corner_for(&topo, v)).collect();
            self.faces.push(EFace { c, part, smooth });
        }
        for e in self.sel_e.clone() {
            self.wires.remove(&e);
        }
        self.sel_f = (base..self.faces.len() as u32).collect();
        self.mode_flush_faces();
        true
    }

    // ------------------------------------------------------------------ triangles and quads, normals

    /// Splits the selected polygons into triangles (Ctrl+T).
    pub fn triangulate_faces(&mut self) -> usize {
        let sel: Vec<u32> = self.sel_f.iter().copied().collect();
        let mut made = 0;
        let mut new_sel = BTreeSet::new();
        for f in sel {
            let face = self.faces[f as usize].clone();
            if face.c.len() <= 3 {
                new_sel.insert(f);
                continue;
            }
            let tris = triangulate(&self.face_points(&face));
            for (i, t) in tris.iter().enumerate() {
                let c = vec![face.c[t[0]], face.c[t[1]], face.c[t[2]]];
                if i == 0 {
                    self.faces[f as usize].c = c;
                    new_sel.insert(f);
                } else {
                    self.faces.push(EFace { c, part: face.part, smooth: face.smooth });
                    new_sel.insert((self.faces.len() - 1) as u32);
                    made += 1;
                }
            }
        }
        self.sel_f = new_sel;
        self.mode_flush_faces();
        made
    }

    /// Joins neighbouring triangles of `faces` into quads where the result is flat (within
    /// `max_angle_deg`), convex and does not cross a UV or colour discontinuity, sharp edge or seam.
    pub fn tris_to_quads(&mut self, faces: &[u32], max_angle_deg: f32) -> usize {
        let set: BTreeSet<u32> = faces.iter().copied().filter(|&f| self.faces.get(f as usize).is_some_and(|x| x.c.len() == 3)).collect();
        if set.len() < 2 {
            return 0;
        }
        let topo = self.topo();
        let cos_lim = max_angle_deg.to_radians().cos();
        let mut cands: Vec<(f32, u32, u32, Edge)> = vec![];
        for (&e, fs) in &topo.edge_faces {
            if fs.len() != 2 || !set.contains(&fs[0]) || !set.contains(&fs[1]) || self.sharp.contains(&e) || self.seams.contains(&e) {
                continue;
            }
            let (f, g) = (fs[0], fs[1]);
            let (ff, gg) = (&self.faces[f as usize], &self.faces[g as usize]);
            if ff.part != gg.part || ff.smooth != gg.smooth {
                continue;
            }
            let (nf, ng) = (self.face_normal(f), self.face_normal(g));
            let dot = nf.dot(ng);
            if dot < cos_lim {
                continue;
            }
            let Some(q) = self.quad_of(f, g, e) else { continue };
            let pts: Vec<Vec3> = q.iter().map(|c| self.pos(c.v)).collect();
            // convexity: every corner turns the same way as the normal
            let n = (nf + ng).normalized();
            let convex = (0..4).all(|i| (pts[(i + 1) % 4] - pts[i]).cross(pts[(i + 2) % 4] - pts[(i + 1) % 4]).dot(n) > 1e-9);
            if !convex {
                continue;
            }
            // prefer square-ish quads: penalise corner angles far from 90 degrees
            let mut err = (1.0 - dot) * 4.0;
            for i in 0..4 {
                let (a, b, c) = (pts[(i + 3) % 4], pts[i], pts[(i + 1) % 4]);
                let cosang = (a - b).normalized().dot((c - b).normalized());
                err += cosang.abs();
            }
            cands.push((err, f, g, e));
        }
        cands.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut used: BTreeSet<u32> = BTreeSet::new();
        let mut made = 0;
        for (_, f, g, e) in cands {
            if used.contains(&f) || used.contains(&g) {
                continue;
            }
            let Some(q) = self.quad_of(f, g, e) else { continue };
            self.faces[f as usize].c = q;
            self.faces[g as usize].c.clear();
            used.insert(f);
            used.insert(g);
            made += 1;
        }
        if made > 0 {
            if self.sel_f.contains(&0) || !self.sel_f.is_empty() {
                // keep merged faces selected
            }
            self.compact();
        }
        made
    }

    /// The quad made of triangles f and g sharing edge e, if their winding agrees and UVs meet.
    fn quad_of(&self, f: u32, g: u32, e: Edge) -> Option<Vec<Corner>> {
        let (ff, gg) = (&self.faces[f as usize], &self.faces[g as usize]);
        let kf = (0..3).find(|&k| ek(ff.c[k].v, ff.c[(k + 1) % 3].v) == e)?;
        let kg = (0..3).find(|&k| ek(gg.c[k].v, gg.c[(k + 1) % 3].v) == e)?;
        let (s1, s2) = (ff.c[kf], ff.c[(kf + 1) % 3]);
        let x = ff.c[(kf + 2) % 3];
        let (g1, g2) = (gg.c[kg], gg.c[(kg + 1) % 3]);
        let y = gg.c[(kg + 2) % 3];
        if g1.v != s2.v || g2.v != s1.v {
            return None;
        }
        if !g1.same_attrs(&s2) || !g2.same_attrs(&s1) {
            return None;
        }
        Some(vec![x, s1, y, s2])
    }

    /// Flips the selected polygons.
    pub fn flip_normals(&mut self) -> usize {
        let sel: Vec<u32> = self.sel_f.iter().copied().collect();
        for &f in &sel {
            self.flip_face(f);
        }
        sel.len()
    }

    /// Recalculate normals (Shift+N): consistent winding inside every connected piece of the
    /// selected polygons, pointing outwards (or inwards with `inside`).
    pub fn recalc_normals(&mut self, inside: bool) -> usize {
        let sel: BTreeSet<u32> = if self.sel_f.is_empty() { (0..self.faces.len() as u32).collect() } else { self.sel_f.clone() };
        let topo = self.topo();
        let mut done: BTreeSet<u32> = BTreeSet::new();
        let mut flipped = 0;
        for &seed in &sel {
            if done.contains(&seed) {
                continue;
            }
            let mut comp = vec![seed];
            done.insert(seed);
            let mut i = 0;
            while i < comp.len() {
                let f = comp[i];
                i += 1;
                let edges: Vec<(u32, u32)> = self.faces[f as usize].edges().collect();
                for (a, b) in edges {
                    for &g in topo.faces_of(ek(a, b)) {
                        if g == f || !sel.contains(&g) || done.contains(&g) {
                            continue;
                        }
                        // g must run the shared edge the other way
                        if self.faces[g as usize].edges().any(|(x, y)| x == a && y == b) {
                            self.flip_face(g);
                            flipped += 1;
                        }
                        done.insert(g);
                        comp.push(g);
                    }
                }
            }
            // outward: positive signed volume relative to the piece's centre
            let mut c = Vec3::ZERO;
            let mut cnt = 0.0;
            for &f in &comp {
                c += self.face_center(f);
                cnt += 1.0;
            }
            let c = c / cnt;
            let mut vol = 0.0;
            for &f in &comp {
                let p = self.face_points(&self.faces[f as usize]);
                for t in 1..p.len() - 1 {
                    vol += (p[0] - c).dot((p[t] - c).cross(p[t + 1] - c));
                }
            }
            if (vol < 0.0) != inside {
                for &f in &comp {
                    self.flip_face(f);
                }
                flipped += comp.len();
            }
        }
        flipped
    }

    /// Smooth vertices: moves every selected vertex towards the average of its neighbours.
    pub fn smooth_verts(&mut self, factor: f32, iterations: usize) {
        let topo = self.topo();
        let sel: Vec<u32> = self.sel_v.iter().copied().collect();
        for _ in 0..iterations.max(1) {
            let new: Vec<(u32, Vec3)> = sel
                .iter()
                .filter_map(|&v| {
                    let nb = &topo.vert_nbrs[v as usize];
                    if nb.is_empty() {
                        return None;
                    }
                    let avg = nb.iter().fold(Vec3::ZERO, |s, &n| s + self.pos(n)) / nb.len() as f32;
                    Some((v, self.pos(v).lerp(avg, factor.clamp(0.0, 1.0))))
                })
                .collect();
            for (v, p) in new {
                self.verts[v as usize].pos = p;
            }
        }
    }

    // ------------------------------------------------------------------ duplicate, split, separate

    /// Duplicate (Shift+D): copies the selected polygons (or edges, or vertices) and selects the copy.
    pub fn duplicate(&mut self) -> bool {
        if self.sel_v.is_empty() {
            return false;
        }
        let mut map: HashMap<u32, u32> = HashMap::default();
        let sel_v: Vec<u32> = self.sel_v.iter().copied().collect();
        for v in sel_v {
            let p = self.pos(v);
            map.insert(v, self.dup_vert(v, p));
        }
        let base = self.faces.len() as u32;
        let faces: Vec<u32> = self.sel_f.iter().copied().collect();
        for f in &faces {
            let face = self.faces[*f as usize].clone();
            self.faces.push(EFace { c: face.c.iter().map(|c| Corner { v: map[&c.v], ..*c }).collect(), ..face });
        }
        let in_faces: BTreeSet<Edge> = faces.iter().flat_map(|&f| self.faces[f as usize].edges().map(|(a, b)| ek(a, b)).collect::<Vec<_>>()).collect();
        for e in self.sel_e.clone() {
            if !in_faces.contains(&e) {
                self.wires.insert(ek(map[&e.0], map[&e.1]));
            }
        }
        for set in [&mut self.sharp, &mut self.seams] {
            let extra: Vec<Edge> = set.iter().filter(|e| map.contains_key(&e.0) && map.contains_key(&e.1)).map(|e| ek(map[&e.0], map[&e.1])).collect();
            set.extend(extra);
        }
        self.sel_v = map.values().copied().collect();
        self.sel_f = (base..self.faces.len() as u32).collect();
        self.sel_e = self.sel_e.iter().map(|e| ek(map[&e.0], map[&e.1])).collect();
        self.sync_selection();
        true
    }

    /// Split (Y): detaches the selected polygons from the rest (they get their own vertices).
    pub fn split(&mut self) -> bool {
        if self.sel_f.is_empty() {
            return false;
        }
        let old: Vec<u32> = self.sel_f.iter().copied().collect();
        let mode = self.mode;
        self.mode = SelectMode::Face;
        self.sync_selection();
        self.duplicate();
        for f in old {
            self.faces[f as usize].c.clear();
        }
        let sel = self.sel_f.clone();
        self.compact();
        // indices shifted by the removal: the copies are the last polygons
        let n = sel.len() as u32;
        let total = self.faces.len() as u32;
        self.sel_f = (total.saturating_sub(n)..total).collect();
        self.mode_flush_faces();
        self.mode = mode;
        self.sync_selection();
        true
    }

    /// Separate (P): removes the selected polygons and returns them as a mesh of their own.
    pub fn separate(&mut self) -> Option<EditMesh> {
        if self.sel_f.is_empty() {
            return None;
        }
        let mut out = EditMesh { parts: self.parts.clone(), ..Default::default() };
        let mut map: HashMap<u32, u32> = HashMap::default();
        for &f in &self.sel_f {
            let face = &self.faces[f as usize];
            let c = face
                .c
                .iter()
                .map(|c| {
                    let v = *map.entry(c.v).or_insert_with(|| {
                        out.verts.push(self.verts[c.v as usize].clone());
                        (out.verts.len() - 1) as u32
                    });
                    Corner { v, ..*c }
                })
                .collect();
            out.faces.push(EFace { c, part: face.part, smooth: face.smooth });
        }
        for (set, oset) in [(&self.sharp, &mut out.sharp), (&self.seams, &mut out.seams)] {
            for e in set {
                if let (Some(&a), Some(&b)) = (map.get(&e.0), map.get(&e.1)) {
                    oset.insert(ek(a, b));
                }
            }
        }
        for f in self.sel_f.clone() {
            self.faces[f as usize].c.clear();
        }
        self.sel_v.clear();
        self.sel_e.clear();
        self.sel_f.clear();
        self.compact();
        out.compact();
        Some(out)
    }

    // ------------------------------------------------------------------ marks and shading

    pub fn mark_sharp(&mut self, on: bool) -> usize {
        let e: Vec<Edge> = self.sel_e.iter().copied().collect();
        for x in &e {
            if on {
                self.sharp.insert(*x);
            } else {
                self.sharp.remove(x);
            }
        }
        e.len()
    }
    pub fn mark_seam(&mut self, on: bool) -> usize {
        let e: Vec<Edge> = self.sel_e.iter().copied().collect();
        for x in &e {
            if on {
                self.seams.insert(*x);
            } else {
                self.seams.remove(x);
            }
        }
        e.len()
    }
    /// Smooth or flat shading of the selected polygons (all, when none is selected).
    pub fn set_smooth(&mut self, on: bool) -> usize {
        let sel: Vec<u32> = if self.sel_f.is_empty() { (0..self.faces.len() as u32).collect() } else { self.sel_f.iter().copied().collect() };
        for &f in &sel {
            self.faces[f as usize].smooth = on;
        }
        sel.len()
    }

    // ------------------------------------------------------------------ symmetry

    /// Symmetrize: rebuilds one half of the mesh as the mirror image of the other half along
    /// `axis` (0 = X, 1 = Y, 2 = Z). With `positive` the +side is kept and copied to the −side.
    pub fn symmetrize(&mut self, axis: usize, positive: bool) -> bool {
        let comp = |p: Vec3| [p.x, p.y, p.z][axis.min(2)];
        let scale = self.verts.iter().fold(0.0f32, |m, v| m.max(v.pos.length())).max(1.0);
        let eps = 1e-4 * scale;
        let side = |x: f32| if positive { x } else { -x };
        // split the polygons that cross the mirror plane
        let all: BTreeSet<u32> = (0..self.faces.len() as u32).collect();
        let mut nrm = Vec3::ZERO;
        match axis {
            0 => nrm.x = 1.0,
            1 => nrm.y = 1.0,
            _ => nrm.z = 1.0,
        }
        self.cut_plane(&all, Vec3::ZERO, nrm, &|_| true);
        // drop polygons that lie on the side being replaced
        for f in &mut self.faces {
            let pts: Vec<f32> = f.c.iter().map(|c| side(comp(self.verts[c.v as usize].pos))).collect();
            if pts.iter().all(|&x| x <= eps) && pts.iter().any(|&x| x < -eps) {
                f.c.clear();
            }
        }
        self.wires.retain(|&(a, b)| side(comp(self.verts[a as usize].pos)) > -eps || side(comp(self.verts[b as usize].pos)) > -eps);
        self.compact();
        // snap vertices near the plane onto it, mirror the rest
        let n = self.verts.len();
        let mut map: Vec<u32> = (0..n as u32).collect();
        for i in 0..n {
            let mut p = self.verts[i].pos;
            let x = comp(p);
            if x.abs() <= eps {
                match axis {
                    0 => p.x = 0.0,
                    1 => p.y = 0.0,
                    _ => p.z = 0.0,
                }
                self.verts[i].pos = p;
            } else {
                let mut q = p;
                match axis {
                    0 => q.x = -q.x,
                    1 => q.y = -q.y,
                    _ => q.z = -q.z,
                }
                map[i] = self.dup_vert(i as u32, q);
            }
        }
        let count = self.faces.len();
        for fi in 0..count {
            let f = self.faces[fi].clone();
            if f.c.iter().all(|c| map[c.v as usize] == c.v) {
                continue; // lies in the mirror plane
            }
            let mut c: Vec<Corner> = f.c.iter().map(|c| Corner { v: map[c.v as usize], ..*c }).collect();
            c.reverse();
            self.faces.push(EFace { c, part: f.part, smooth: f.smooth });
        }
        let extra: Vec<Edge> = self.wires.iter().map(|&(a, b)| ek(map[a as usize], map[b as usize])).collect();
        self.wires.extend(extra);
        for set in [&mut self.sharp, &mut self.seams] {
            let extra: Vec<Edge> = set.iter().map(|&(a, b)| ek(map[a as usize], map[b as usize])).collect();
            set.extend(extra);
        }
        self.compact();
        true
    }
}
