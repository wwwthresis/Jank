//! Shape operators of Edit mode: spin (lathe), solidify, array, poke and to sphere. Like the other
//! operators they work on the selection, leave a valid mesh and select their result.
use super::*;
use crate::fasthash::HashMap;
use std::collections::BTreeSet;

/// `p` turned around the line through `center` along the unit vector `axis` by `angle` radians.
fn rotate_about(p: Vec3, center: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    let v = p - center;
    let (s, c) = angle.sin_cos();
    center + v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

/// Removes repeated neighbours (also across the wrap) from a polygon's corner list.
fn dedup_ring(mut c: Vec<Corner>) -> Vec<Corner> {
    c.dedup_by(|a, b| a.v == b.v);
    while c.len() > 1 && c[0].v == c[c.len() - 1].v {
        c.pop();
    }
    c
}

impl EditMesh {
    /// Spin (lathe): sweeps the selected edges (or the selected polygons) around the line through
    /// `center` along `axis`, in `steps` steps over `angle_deg` degrees. A full turn closes on
    /// itself; vertices on the axis are shared by every step, so domes and cones get triangles
    /// instead of slivers. Swept polygons move to the end of the sweep (a free-standing polygon
    /// also keeps a start cap, so the result is solid). The last copy of the profile is selected.
    /// Returns the number of new polygons.
    pub fn spin(&mut self, center: Vec3, axis: Vec3, angle_deg: f32, steps: usize) -> usize {
        if axis.length() < 1e-9 || self.sel_v.is_empty() {
            return 0;
        }
        let axis = axis.normalized();
        let angle = angle_deg.clamp(-360.0, 360.0).to_radians();
        let full = angle_deg.abs() >= 359.99;
        let steps = steps.clamp(if full { 3 } else { 1 }, 512);
        let topo = self.topo();
        let region: BTreeSet<u32> = self.sel_f.clone();
        // the profile: directed edges with the corners (uv, colour) they carry
        let mut profile: Vec<(Corner, Corner, Option<u32>)> = vec![];
        if !region.is_empty() {
            for &f in &region {
                let face = &self.faces[f as usize];
                let n = face.c.len();
                for k in 0..n {
                    let (a, b) = (face.c[k], face.c[(k + 1) % n]);
                    if topo.faces_of(ek(a.v, b.v)).iter().filter(|g| region.contains(g)).count() == 1 {
                        profile.push((a, b, Some(f)));
                    }
                }
            }
        } else {
            for &(a, b) in &self.sel_e {
                // run the edge against a polygon that already uses it, like extrude
                let fs = topo.faces_of((a, b));
                if let Some(&f) = fs.first() {
                    let face = &self.faces[f as usize];
                    let (ca, cb) = (face.c[face.corner_of(a).unwrap()], face.c[face.corner_of(b).unwrap()]);
                    if face.edges().any(|(x, y)| x == a && y == b) {
                        profile.push((cb, ca, Some(f)));
                    } else {
                        profile.push((ca, cb, Some(f)));
                    }
                } else {
                    profile.push((Corner::new(a), Corner::new(b), None));
                }
            }
        }
        let in_profile: BTreeSet<u32> = profile.iter().flat_map(|(a, b, _)| [a.v, b.v]).collect();
        let region_verts: BTreeSet<u32> = region.iter().flat_map(|&f| self.faces[f as usize].verts().collect::<Vec<_>>()).collect();
        let lone: Vec<u32> = if region.is_empty() { self.sel_v.iter().copied().filter(|v| !in_profile.contains(v)).collect() } else { vec![] };
        let moving: BTreeSet<u32> = in_profile.iter().chain(region_verts.iter()).chain(lone.iter()).copied().collect();
        // the size of the profile decides what "on the axis" means
        let mut reach = 0.0f32;
        for &v in &moving {
            let d = self.pos(v) - center;
            reach = reach.max((d - axis * axis.dot(d)).length());
        }
        let on_axis = |p: Vec3| {
            let d = p - center;
            (d - axis * axis.dot(d)).length() <= (reach * 1e-4).max(1e-6)
        };
        // ring[k][v]: vertex v after k steps (ring 0 is the original)
        let last = if full { steps - 1 } else { steps };
        let mut rings: Vec<HashMap<u32, u32>> = vec![moving.iter().map(|&v| (v, v)).collect()];
        for k in 1..=last {
            let a = angle * k as f32 / steps as f32;
            let mut ring = HashMap::default();
            for &v in &moving {
                let p = self.pos(v);
                let n = if on_axis(p) { v } else { self.dup_vert(v, rotate_about(p, center, axis, a)) };
                ring.insert(v, n);
            }
            rings.push(ring);
        }
        let at = |k: usize, v: u32| -> u32 { if full && k == steps { v } else { rings[k][&v] } };
        // a cylinder-like UV: u around the axis, v along the profile's longer extent
        let (mut hmin, mut hmax, mut rmin, mut rmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &v in &in_profile {
            let d = self.pos(v) - center;
            let h = axis.dot(d);
            let r = (d - axis * h).length();
            (hmin, hmax, rmin, rmax) = (hmin.min(h), hmax.max(h), rmin.min(r), rmax.max(r));
        }
        let use_h = hmax - hmin >= rmax - rmin;
        let (lo, span) = if use_h { (hmin, (hmax - hmin).max(1e-6)) } else { (rmin, (rmax - rmin).max(1e-6)) };
        let vcoord = |p: Vec3| {
            let d = p - center;
            let h = axis.dot(d);
            let x = if use_h { h } else { (d - axis * h).length() };
            1.0 - (x - lo) / span
        };
        // a polygon sweeping "backwards" (against its normal) would come out inside out
        let mut flip = false;
        if !region.is_empty() {
            let (mut nsum, mut csum) = (Vec3::ZERO, Vec3::ZERO);
            for &f in &region {
                nsum += newell(&self.face_points(&self.faces[f as usize]));
                csum += self.face_center(f);
            }
            let c = csum / region.len() as f32;
            flip = nsum.dot(axis.cross(c - center) * angle.signum()) < 0.0;
        }
        let first_new = self.faces.len() as u32;
        let mut walls = vec![];
        for &(ca, cb, f) in &profile {
            let (part, smooth) = f.map_or((0, false), |f| (self.faces[f as usize].part, self.faces[f as usize].smooth));
            let (va, vb) = (vcoord(self.pos(ca.v)), vcoord(self.pos(cb.v)));
            for k in 0..steps {
                let (u0, u1) = (k as f32 / steps as f32, (k + 1) as f32 / steps as f32);
                let corner = |c: &Corner, k: usize, u: f32, v: f32| Corner { v: at(k, c.v), uv: Vec2::new(u, v), color: c.color };
                let quad = vec![corner(&ca, k, u0, va), corner(&cb, k, u0, vb), corner(&cb, k + 1, u1, vb), corner(&ca, k + 1, u1, va)];
                let quad = dedup_ring(quad);
                if quad.len() >= 3 {
                    walls.push(self.faces.len() as u32);
                    self.faces.push(EFace { c: quad, part, smooth });
                }
            }
            self.wires.remove(&ek(ca.v, cb.v));
        }
        for &v in &lone {
            for k in 0..steps {
                let (a, b) = (at(k, v), at(k + 1, v));
                if a != b {
                    self.wires.insert(ek(a, b));
                }
            }
        }
        // caps
        let mut end_cap = vec![];
        if !region.is_empty() {
            let free = profile.iter().all(|(a, b, _)| topo.faces_of(ek(a.v, b.v)).len() == 1);
            if full {
                for &f in &region {
                    self.faces[f as usize].c.clear();
                }
            } else {
                if free {
                    // the start stays closed: a copy facing back
                    for &f in &region {
                        let mut face = self.faces[f as usize].clone();
                        face.c.reverse();
                        walls.push(self.faces.len() as u32);
                        self.faces.push(face);
                    }
                }
                for &f in &region {
                    for c in &mut self.faces[f as usize].c {
                        c.v = at(steps, c.v);
                    }
                    end_cap.push(f);
                }
            }
            if flip {
                for &f in walls.iter().chain(end_cap.iter()) {
                    self.flip_face(f);
                }
            }
        } else if profile.iter().all(|p| p.2.is_none()) && !walls.is_empty() {
            // a free-standing profile: point the new surface outwards
            let keep = std::mem::replace(&mut self.sel_f, walls.iter().copied().collect());
            self.recalc_normals(false);
            self.sel_f = keep;
        }
        let made = self.faces.len() as u32 - first_new;
        // select the last copy of the profile
        let end = if full { 0 } else { steps };
        if !region.is_empty() && !full {
            self.mode = SelectMode::Face;
            self.sel_f = end_cap.into_iter().collect();
            self.mode_flush_faces();
        } else {
            self.sel_f.clear();
            self.sel_v = moving.iter().map(|&v| at(end, v)).collect();
            self.sel_e = profile.iter().map(|(a, b, _)| ek(at(end, a.v), at(end, b.v))).filter(|e| e.0 != e.1).collect();
            if self.mode == SelectMode::Face {
                self.mode = SelectMode::Edge;
            }
            if self.sel_e.is_empty() {
                self.mode = SelectMode::Vertex;
            }
            self.sync_selection();
        }
        if full && !region.is_empty() {
            self.compact();
            self.select_all(false);
        }
        made as usize
    }

    /// Solidify: gives the selected polygons (every polygon when none is selected) a thickness.
    /// A copy is pushed back along the normals by `thickness` (forwards when negative) and joined
    /// to the original along the open border. Returns the number of new polygons.
    pub fn solidify(&mut self, thickness: f32) -> usize {
        let region: BTreeSet<u32> = if self.sel_f.is_empty() { (0..self.faces.len() as u32).filter(|&f| !self.face_hidden(f)).collect() } else { self.sel_f.clone() };
        if region.is_empty() || thickness.abs() < 1e-7 {
            return 0;
        }
        let topo = self.topo();
        let unit: HashMap<u32, Vec3> = region.iter().map(|&f| (f, self.face_normal(f))).collect();
        let verts: BTreeSet<u32> = region.iter().flat_map(|&f| self.faces[f as usize].verts().collect::<Vec<_>>()).collect();
        let mut map: HashMap<u32, u32> = HashMap::default();
        for &v in &verts {
            let around: Vec<Vec3> = topo.vert_faces[v as usize].iter().filter_map(|f| unit.get(f).copied()).collect();
            let mut n = around.iter().fold(Vec3::ZERO, |s, &x| s + x);
            if n.length() < 1e-9 {
                n = Vec3::Y;
            }
            let n = n.normalized();
            // even thickness: corners move further so the flat walls keep their distance
            let d = around.iter().fold(1.0f32, |m, x| m.min(n.dot(*x))).max(0.5);
            let p = self.pos(v) - n * (thickness / d);
            map.insert(v, self.dup_vert(v, p));
        }
        let first = self.faces.len() as u32;
        for &f in &region {
            let face = self.faces[f as usize].clone();
            let mut c: Vec<Corner> = face.c.iter().map(|c| Corner { v: map[&c.v], ..*c }).collect();
            c.reverse();
            self.faces.push(EFace { c, ..face });
        }
        for &f in &region {
            let face = self.faces[f as usize].clone();
            let n = face.c.len();
            for k in 0..n {
                let (ca, cb) = (face.c[k], face.c[(k + 1) % n]);
                if topo.faces_of(ek(ca.v, cb.v)).iter().filter(|g| region.contains(g)).count() != 1 {
                    continue;
                }
                let rim = vec![cb, ca, Corner { v: map[&ca.v], ..ca }, Corner { v: map[&cb.v], ..cb }];
                self.faces.push(EFace { c: rim, part: face.part, smooth: face.smooth });
            }
        }
        let made = self.faces.len() as u32 - first;
        let all: BTreeSet<u32> = region.iter().copied().chain(first..self.faces.len() as u32).collect();
        if thickness < 0.0 {
            for &f in &all {
                self.flip_face(f);
            }
        }
        for set in [&mut self.sharp, &mut self.seams] {
            let extra: Vec<Edge> = set.iter().filter(|e| map.contains_key(&e.0) && map.contains_key(&e.1)).map(|e| ek(map[&e.0], map[&e.1])).collect();
            set.extend(extra);
        }
        self.mode = SelectMode::Face;
        self.sel_f = all;
        self.mode_flush_faces();
        made as usize
    }

    /// Size of the selection's bounding box (of the whole mesh when nothing is selected).
    pub fn selection_size(&self) -> Vec3 {
        let vs: Vec<u32> = if self.sel_v.is_empty() { (0..self.verts.len() as u32).collect() } else { self.sel_v.iter().copied().collect() };
        if vs.is_empty() {
            return Vec3::ZERO;
        }
        let (mut lo, mut hi) = (Vec3::new(f32::MAX, f32::MAX, f32::MAX), Vec3::new(f32::MIN, f32::MIN, f32::MIN));
        for v in vs {
            let p = self.pos(v);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        hi - lo
    }

    /// Array: adds `count - 1` copies of the selection (everything when nothing is selected), each
    /// shifted by `offset` from the one before. With `weld`, vertices where neighbouring copies
    /// touch are merged. Selects the original and every copy; returns the number of copies.
    pub fn array(&mut self, count: usize, offset: Vec3, weld: bool) -> usize {
        if self.sel_v.is_empty() {
            self.select_all(true);
        }
        if self.sel_v.is_empty() {
            return 0;
        }
        let count = count.clamp(1, 512);
        let (sv, se, sf) = (self.sel_v.clone(), self.sel_e.clone(), self.sel_f.clone());
        let (mut all_v, mut all_e, mut all_f) = (sv.clone(), se.clone(), sf.clone());
        for i in 1..count {
            self.sel_v = sv.clone();
            self.sel_e = se.clone();
            self.sel_f = sf.clone();
            if !self.duplicate() {
                break;
            }
            let shift = offset * i as f32;
            for &v in &self.sel_v {
                self.verts[v as usize].pos += shift;
            }
            all_v.extend(self.sel_v.iter().copied());
            all_e.extend(self.sel_e.iter().copied());
            all_f.extend(self.sel_f.iter().copied());
        }
        self.sel_v = all_v;
        self.sel_e = all_e;
        self.sel_f = all_f;
        self.sync_selection();
        if weld && count > 1 {
            let size = self.selection_size().length();
            self.merge_by_distance((size * 1e-5).max(1e-6));
        }
        count - 1
    }

    /// Poke: splits every selected polygon into a fan of triangles around a new middle vertex,
    /// pushed along the polygon's normal by `depth`. Selects the triangles; returns the number of
    /// polygons poked.
    pub fn poke(&mut self, depth: f32) -> usize {
        let faces: Vec<u32> = self.sel_f.iter().copied().collect();
        let mut new = BTreeSet::new();
        for &f in &faces {
            let face = self.faces[f as usize].clone();
            let n = face.c.len();
            let inv = 1.0 / n as f32;
            let mut p = Vec3::ZERO;
            let mut uv = Vec2::default();
            let mut color = [0.0f32; 3];
            for c in &face.c {
                p += self.pos(c.v);
                uv = Vec2::new(uv.x + c.uv.x * inv, uv.y + c.uv.y * inv);
                for i in 0..3 {
                    color[i] += c.color[i] * inv;
                }
            }
            let p = p * inv + self.face_normal(f) * depth;
            let mid = self.dup_vert(face.c[0].v, p);
            let mc = Corner { v: mid, uv, color };
            for k in 0..n {
                let tri = vec![face.c[k], face.c[(k + 1) % n], mc];
                let fi = if k == 0 {
                    self.faces[f as usize].c = tri;
                    f
                } else {
                    self.faces.push(EFace { c: tri, ..face.clone() });
                    self.faces.len() as u32 - 1
                };
                new.insert(fi);
            }
        }
        if !faces.is_empty() {
            self.mode = SelectMode::Face;
            self.sel_f = new;
            self.mode_flush_faces();
        }
        faces.len()
    }

    /// To sphere: moves the selected vertices towards a sphere around their centre (its radius is
    /// their average distance from the centre); `factor` 0 leaves them, 1 makes a perfect sphere.
    /// Returns the number of vertices moved.
    pub fn to_sphere(&mut self, factor: f32) -> usize {
        let Some(c) = self.selection_center() else { return 0 };
        let sel: Vec<u32> = self.sel_v.iter().copied().collect();
        let r = sel.iter().map(|&v| (self.pos(v) - c).length()).sum::<f32>() / sel.len() as f32;
        if r < 1e-9 {
            return 0;
        }
        let t = factor.clamp(0.0, 1.0);
        let mut moved = 0;
        for v in sel {
            let d = self.pos(v) - c;
            if d.length() < 1e-9 {
                continue;
            }
            let target = c + d.normalized() * r;
            self.verts[v as usize].pos = self.pos(v).lerp(target, t);
            moved += 1;
        }
        moved
    }
}
