//! UV tools of Edit mode: unwrap (least-squares conformal maps, cut at seams), projection from the
//! view, cube projection and island packing.
use super::*;
use crate::fasthash::HashMap;

/// A UV island being built: its polygons and, per corner, the index of its UV vertex.
struct Chart {
    faces: Vec<u32>,
    /// (face, corner) -> uv vertex
    corner_vert: HashMap<(u32, usize), usize>,
    /// 3D position of every uv vertex
    pos: Vec<Vec3>,
}

impl EditMesh {
    /// Splits `faces` into charts: polygons joined by edges that are not seams (and not in `cut`).
    fn charts(&self, faces: &[u32], cut: &dyn Fn(u32, u32) -> bool) -> Vec<Chart> {
        let set: BTreeSet<u32> = faces.iter().copied().collect();
        // corner ids
        let mut ids: HashMap<(u32, usize), usize> = HashMap::default();
        let mut list = vec![];
        for &f in &set {
            for k in 0..self.faces[f as usize].c.len() {
                ids.insert((f, k), list.len());
                list.push((f, k));
            }
        }
        let mut dsu = Dsu::new(list.len());
        let mut fdsu = Dsu::new(self.faces.len());
        let topo = self.topo();
        for (e, fs) in &topo.edge_faces {
            if fs.len() != 2 || !set.contains(&fs[0]) || !set.contains(&fs[1]) || self.seams.contains(e) || cut(fs[0], fs[1]) {
                continue;
            }
            let (f, g) = (fs[0], fs[1]);
            fdsu.union(f, g);
            for v in [e.0, e.1] {
                let (Some(a), Some(b)) = (self.faces[f as usize].corner_of(v), self.faces[g as usize].corner_of(v)) else { continue };
                dsu.union(ids[&(f, a)] as u32, ids[&(g, b)] as u32);
            }
        }
        let mut by_root: HashMap<u32, usize> = HashMap::default();
        let mut charts: Vec<Chart> = vec![];
        for &f in &set {
            let r = fdsu.find(f);
            let ci = *by_root.entry(r).or_insert_with(|| {
                charts.push(Chart { faces: vec![], corner_vert: HashMap::default(), pos: vec![] });
                charts.len() - 1
            });
            charts[ci].faces.push(f);
        }
        for ch in &mut charts {
            let mut root_vert: HashMap<u32, usize> = HashMap::default();
            for &f in &ch.faces {
                for (k, c) in self.faces[f as usize].c.iter().enumerate() {
                    let r = dsu.find(ids[&(f, k)] as u32);
                    let pos = &mut ch.pos;
                    let vi = *root_vert.entry(r).or_insert_with(|| {
                        pos.push(self.pos(c.v));
                        pos.len() - 1
                    });
                    ch.corner_vert.insert((f, k), vi);
                }
            }
        }
        charts
    }

    /// Unwraps the given polygons: each island (cut at seams) is flattened with least-squares
    /// conformal maps and the islands are packed into the unit square. Islands without any cut
    /// (closed surfaces) are first split by the dominant direction of their polygons.
    /// Returns the number of islands.
    pub fn uv_unwrap(&mut self, faces: &[u32], margin: f32) -> usize {
        if faces.is_empty() {
            return 0;
        }
        let mut charts = self.charts(faces, &|_, _| false);
        // closed charts: cut by dominant axis
        let axis = |em: &EditMesh, f: u32| {
            let n = em.face_normal(f);
            let a = [n.x.abs(), n.y.abs(), n.z.abs()];
            let i = if a[0] >= a[1] && a[0] >= a[2] { 0 } else if a[1] >= a[2] { 1 } else { 2 };
            i * 2 + usize::from([n.x, n.y, n.z][i] < 0.0)
        };
        let mut out = vec![];
        for ch in charts.drain(..) {
            if self.chart_closed(&ch) {
                let sub = self.charts(&ch.faces, &|f, g| axis(self, f) != axis(self, g));
                out.extend(sub);
            } else {
                out.push(ch);
            }
        }
        let mut islands: Vec<(Vec<u32>, HashMap<(u32, usize), usize>, Vec<Vec2>)> = vec![];
        for ch in out {
            let uv = lscm(self, &ch).unwrap_or_else(|| planar(self, &ch));
            islands.push((ch.faces, ch.corner_vert, uv));
        }
        let n = islands.len();
        let mut packed: Vec<(Vec<u32>, HashMap<(u32, usize), usize>, Vec<Vec2>)> = vec![];
        let mut boxes = vec![];
        for (faces, cv, mut uv) in islands {
            // scale to 3D area, rotate to the smallest bounding box
            let (a3, a2) = self.chart_areas(&faces, &cv, &uv);
            let s = if a2 > 1e-12 { (a3 / a2).sqrt() } else { 1.0 };
            for p in &mut uv {
                *p = Vec2::new(p.x * s, p.y * s);
            }
            fit_rotation(&mut uv);
            let (lo, hi) = bounds(&uv);
            for p in &mut uv {
                *p = Vec2::new(p.x - lo.x, p.y - lo.y);
            }
            boxes.push(Vec2::new(hi.x - lo.x, hi.y - lo.y));
            packed.push((faces, cv, uv));
        }
        let offs = shelf_pack(&boxes, margin);
        let (total_w, total_h) = offs.iter().zip(&boxes).fold((0f32, 0f32), |(w, h), (o, b)| (w.max(o.x + b.x), h.max(o.y + b.y)));
        let scale = (1.0 - 2.0 * margin) / total_w.max(total_h).max(1e-9);
        for ((faces, cv, uv), o) in packed.into_iter().zip(offs) {
            for f in faces {
                for k in 0..self.faces[f as usize].c.len() {
                    let p = uv[cv[&(f, k)]];
                    self.faces[f as usize].c[k].uv = Vec2::new(margin + (p.x + o.x) * scale, 1.0 - (margin + (p.y + o.y) * scale));
                }
            }
        }
        self.mark_seams_from_uvs(faces);
        n
    }

    fn chart_closed(&self, ch: &Chart) -> bool {
        // boundary edges of the chart: directed corner pairs without a twin
        let mut edges: HashMap<(usize, usize), usize> = HashMap::default();
        for &f in &ch.faces {
            let n = self.faces[f as usize].c.len();
            for k in 0..n {
                let (a, b) = (ch.corner_vert[&(f, k)], ch.corner_vert[&(f, (k + 1) % n)]);
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        edges.values().all(|&c| c >= 2)
    }

    fn chart_areas(&self, faces: &[u32], cv: &HashMap<(u32, usize), usize>, uv: &[Vec2]) -> (f32, f32) {
        let (mut a3, mut a2) = (0.0, 0.0);
        for &f in faces {
            let face = &self.faces[f as usize];
            a3 += newell(&self.face_points(face)).length() * 0.5;
            let n = face.c.len();
            let mut s = 0.0;
            for k in 0..n {
                let (p, q) = (uv[cv[&(f, k)]], uv[cv[&(f, (k + 1) % n)]]);
                s += p.x * q.y - q.x * p.y;
            }
            a2 += s.abs() * 0.5;
        }
        (a3, a2)
    }

    /// Marks seams where the UVs of neighbouring polygons no longer meet.
    fn mark_seams_from_uvs(&mut self, faces: &[u32]) {
        let set: BTreeSet<u32> = faces.iter().copied().collect();
        let topo = self.topo();
        for (e, fs) in &topo.edge_faces {
            if fs.len() != 2 || !(set.contains(&fs[0]) || set.contains(&fs[1])) {
                continue;
            }
            let (f, g) = (&self.faces[fs[0] as usize], &self.faces[fs[1] as usize]);
            let split = [e.0, e.1].iter().any(|&v| {
                let (a, b) = (f.c[f.corner_of(v).unwrap()].uv, g.c[g.corner_of(v).unwrap()].uv);
                (a.x - b.x).abs() > 1e-5 || (a.y - b.y).abs() > 1e-5
            });
            if split {
                self.seams.insert(*e);
            }
        }
    }

    /// Projects the polygons with `proj` (for example the view: world point -> screen point) and
    /// fits the result into the unit square, keeping its proportions.
    pub fn uv_project(&mut self, faces: &[u32], proj: &dyn Fn(Vec3) -> Vec2) {
        let pts: Vec<Vec2> = faces.iter().flat_map(|&f| self.faces[f as usize].c.iter().map(|c| proj(self.pos(c.v))).collect::<Vec<_>>()).collect();
        if pts.is_empty() {
            return;
        }
        let (lo, hi) = bounds(&pts);
        let s = 1.0 / (hi.x - lo.x).max(hi.y - lo.y).max(1e-9);
        let (ox, oy) = ((1.0 - (hi.x - lo.x) * s) * 0.5, (1.0 - (hi.y - lo.y) * s) * 0.5);
        let mut i = 0;
        for &f in faces {
            for k in 0..self.faces[f as usize].c.len() {
                let p = pts[i];
                i += 1;
                self.faces[f as usize].c[k].uv = Vec2::new(ox + (p.x - lo.x) * s, oy + (p.y - lo.y) * s);
            }
        }
        self.mark_seams_from_uvs(faces);
    }

    /// Cube projection: every polygon is projected along its dominant axis; the six groups are
    /// unwrapped as separate islands and packed.
    pub fn uv_cube_project(&mut self, faces: &[u32], margin: f32) -> usize {
        let saved = self.seams.clone();
        let topo = self.topo();
        let axis = |f: u32| {
            let n = self.face_normal(f);
            let a = [n.x.abs(), n.y.abs(), n.z.abs()];
            let i = if a[0] >= a[1] && a[0] >= a[2] { 0 } else if a[1] >= a[2] { 1 } else { 2 };
            i * 2 + usize::from([n.x, n.y, n.z][i] < 0.0)
        };
        let cuts: Vec<Edge> = topo.edge_faces.iter().filter(|(_, fs)| fs.len() == 2 && axis(fs[0]) != axis(fs[1])).map(|(e, _)| *e).collect();
        self.seams.extend(cuts);
        // planar projection per group gives the classic box mapping, LSCM keeps it undistorted
        let n = self.uv_unwrap(faces, margin);
        self.seams.extend(saved);
        n
    }
}

fn bounds(p: &[Vec2]) -> (Vec2, Vec2) {
    let mut lo = Vec2::new(f32::MAX, f32::MAX);
    let mut hi = Vec2::new(f32::MIN, f32::MIN);
    for q in p {
        lo = Vec2::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = Vec2::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    (lo, hi)
}

/// Rotates the points to the angle (in 3° steps) with the smallest bounding box, standing up.
fn fit_rotation(uv: &mut [Vec2]) {
    let mut best = (f32::MAX, 0.0f32);
    for step in 0..30 {
        let a = (step as f32 * 3.0).to_radians();
        let (s, c) = a.sin_cos();
        let r: Vec<Vec2> = uv.iter().map(|p| Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)).collect();
        let (lo, hi) = bounds(&r);
        let area = (hi.x - lo.x) * (hi.y - lo.y);
        if area < best.0 - 1e-9 {
            best = (area, a);
        }
    }
    let (s, c) = best.1.sin_cos();
    for p in uv.iter_mut() {
        *p = Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
    }
    let (lo, hi) = bounds(uv);
    if hi.x - lo.x > hi.y - lo.y {
        for p in uv.iter_mut() {
            *p = Vec2::new(-p.y, p.x);
        }
    }
}

/// Shelf packing of boxes; returns the offset of each box. `margin` is relative to the result.
fn shelf_pack(boxes: &[Vec2], margin: f32) -> Vec<Vec2> {
    let area: f32 = boxes.iter().map(|b| b.x * b.y).sum();
    let pad = area.sqrt() * margin * 2.0;
    let widest = boxes.iter().fold(0f32, |m, b| m.max(b.x));
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&a, &b| boxes[b].y.partial_cmp(&boxes[a].y).unwrap_or(std::cmp::Ordering::Equal));
    // try a few shelf widths and keep the squarest result
    let mut best: Option<(f32, Vec<Vec2>)> = None;
    for k in 0..12 {
        let width = (area.sqrt() * (0.8 + k as f32 * 0.1)).max(widest);
        let mut offs = vec![Vec2::default(); boxes.len()];
        let (mut x, mut y, mut row_h) = (0f32, 0f32, 0f32);
        let mut w_used = 0f32;
        for &i in &order {
            let b = boxes[i];
            if x > 0.0 && x + b.x > width {
                x = 0.0;
                y += row_h + pad;
                row_h = 0.0;
            }
            offs[i] = Vec2::new(x, y);
            x += b.x + pad;
            w_used = w_used.max(x - pad);
            row_h = row_h.max(b.y);
        }
        let side = w_used.max(y + row_h);
        if best.as_ref().map_or(true, |(s, _)| side < *s) {
            best = Some((side, offs));
        }
    }
    best.map(|b| b.1).unwrap_or_default()
}

/// Flat projection of a chart onto its average plane (fallback when LSCM cannot solve it).
fn planar(em: &EditMesh, ch: &Chart) -> Vec<Vec2> {
    let mut n = Vec3::ZERO;
    for &f in &ch.faces {
        n += newell(&em.face_points(&em.faces[f as usize]));
    }
    let n = if n.length() < 1e-9 { Vec3::Y } else { n.normalized() };
    let ax = if n.y.abs() < 0.9 { Vec3::Y } else { Vec3::new(0.0, 0.0, 1.0) };
    let u = ax.cross(n).normalized();
    let v = n.cross(u);
    ch.pos.iter().map(|p| Vec2::new(p.dot(u), p.dot(v))).collect()
}

/// Least-squares conformal map of one chart, solved by conjugate gradients on the normal
/// equations with the two most distant vertices pinned.
fn lscm(em: &EditMesh, ch: &Chart) -> Option<Vec<Vec2>> {
    let nv = ch.pos.len();
    if nv < 3 {
        return None;
    }
    // triangles in chart vertex indices
    let mut tris: Vec<[usize; 3]> = vec![];
    for &f in &ch.faces {
        let face = &em.faces[f as usize];
        let pts = em.face_points(face);
        for t in triangulate(&pts) {
            tris.push(t.map(|k| ch.corner_vert[&(f, k)]));
        }
    }
    if tris.len() < 2 {
        return None;
    }
    // pins: extremes along the longest axis of the bounding box
    let (mut lo, mut hi) = (Vec3::new(f32::MAX, f32::MAX, f32::MAX), Vec3::new(f32::MIN, f32::MIN, f32::MIN));
    for p in &ch.pos {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let ext = hi - lo;
    let axis = if ext.x >= ext.y && ext.x >= ext.z { Vec3::new(1.0, 0.0, 0.0) } else if ext.y >= ext.z { Vec3::Y } else { Vec3::new(0.0, 0.0, 1.0) };
    let (mut p0, mut p1) = (0, 0);
    for i in 0..nv {
        if ch.pos[i].dot(axis) < ch.pos[p0].dot(axis) {
            p0 = i;
        }
        if ch.pos[i].dot(axis) > ch.pos[p1].dot(axis) {
            p1 = i;
        }
    }
    if p0 == p1 {
        return None;
    }
    let dist = (ch.pos[p1] - ch.pos[p0]).length();
    let pinned = |i: usize| i == p0 || i == p1;
    let pin_val = |i: usize, comp: usize| if i == p1 && comp == 0 { dist as f64 } else { 0.0 };
    // free variable index: (vertex, component) -> column
    let mut col = vec![usize::MAX; nv * 2];
    let mut nfree = 0;
    for i in 0..nv {
        if !pinned(i) {
            col[i * 2] = nfree;
            col[i * 2 + 1] = nfree + 1;
            nfree += 2;
        }
    }
    // rows: (free entries, constant from pins)
    let mut rows: Vec<(Vec<(usize, f64)>, f64)> = vec![];
    for t in &tris {
        let (a, b, c) = (ch.pos[t[0]], ch.pos[t[1]], ch.pos[t[2]]);
        let nrm = (b - a).cross(c - a);
        let area2 = nrm.length();
        if area2 < 1e-12 {
            continue;
        }
        let x = (b - a).normalized();
        let y = nrm.normalized().cross(x);
        let q = [(0.0f64, 0.0f64), (((b - a).dot(x)) as f64, ((b - a).dot(y)) as f64), (((c - a).dot(x)) as f64, ((c - a).dot(y)) as f64)];
        let s = 1.0 / (area2 as f64).sqrt();
        let wr = [(q[2].0 - q[1].0) * s, (q[0].0 - q[2].0) * s, (q[1].0 - q[0].0) * s];
        let wi = [(q[2].1 - q[1].1) * s, (q[0].1 - q[2].1) * s, (q[1].1 - q[0].1) * s];
        // real part: sum wr*u - wi*v ; imaginary part: sum wi*u + wr*v
        for part in 0..2 {
            let mut ent = vec![];
            let mut cst = 0.0;
            for k in 0..3 {
                let (cu, cv) = if part == 0 { (wr[k], -wi[k]) } else { (wi[k], wr[k]) };
                let vi = t[k];
                for (comp, coef) in [(0, cu), (1, cv)] {
                    if pinned(vi) {
                        cst += coef * pin_val(vi, comp);
                    } else {
                        ent.push((col[vi * 2 + comp], coef));
                    }
                }
            }
            rows.push((ent, cst));
        }
    }
    if nfree == 0 {
        return None;
    }
    // CG on AᵀA x = -Aᵀ c
    let apply = |x: &[f64], out: &mut [f64]| {
        out.iter_mut().for_each(|v| *v = 0.0);
        for (ent, _) in &rows {
            let d: f64 = ent.iter().map(|&(j, a)| a * x[j]).sum();
            for &(j, a) in ent {
                out[j] += a * d;
            }
        }
    };
    let mut bvec = vec![0.0f64; nfree];
    for (ent, c) in &rows {
        for &(j, a) in ent {
            bvec[j] -= a * c;
        }
    }
    // start from the planar projection (scaled) for faster convergence
    let mut x = vec![0.0f64; nfree];
    let mut r = bvec.clone();
    let mut ax = vec![0.0f64; nfree];
    apply(&x, &mut ax);
    for i in 0..nfree {
        r[i] -= ax[i];
    }
    let mut p = r.clone();
    let mut rr: f64 = r.iter().map(|v| v * v).sum();
    let tol = rr.max(1e-30) * 1e-14;
    let mut ap = vec![0.0f64; nfree];
    for _ in 0..(nfree * 4).max(200).min(20000) {
        if rr <= tol {
            break;
        }
        apply(&p, &mut ap);
        let pap: f64 = p.iter().zip(&ap).map(|(a, b)| a * b).sum();
        if pap.abs() < 1e-300 {
            break;
        }
        let alpha = rr / pap;
        for i in 0..nfree {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        let rr2: f64 = r.iter().map(|v| v * v).sum();
        let beta = rr2 / rr;
        rr = rr2;
        for i in 0..nfree {
            p[i] = r[i] + beta * p[i];
        }
    }
    let mut uv = vec![Vec2::default(); nv];
    for i in 0..nv {
        uv[i] = if pinned(i) {
            Vec2::new(pin_val(i, 0) as f32, pin_val(i, 1) as f32)
        } else {
            Vec2::new(x[col[i * 2]] as f32, x[col[i * 2 + 1]] as f32)
        };
        if !uv[i].x.is_finite() || !uv[i].y.is_finite() {
            return None;
        }
    }
    // orientation: polygons must keep their winding in UV space
    let mut signed = 0.0;
    for t in &tris {
        let (a, b, c) = (uv[t[0]], uv[t[1]], uv[t[2]]);
        signed += (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
    }
    if signed < 0.0 {
        for p in &mut uv {
            p.x = -p.x;
        }
    }
    let (lo2, hi2) = bounds(&uv);
    if (hi2.x - lo2.x).max(hi2.y - lo2.y) < 1e-9 {
        return None;
    }
    Some(uv)
}
