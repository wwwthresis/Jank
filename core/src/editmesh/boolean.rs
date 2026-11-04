//! Boolean operations (union, difference, intersection) with BSP trees.
//!
//! Both meshes are cut into convex polygons, classified against each other's BSP tree (the
//! classic approach of csg.js), and the surviving polygons are rebuilt into an edit mesh:
//! split points are welded, T-junctions are closed by inserting the missing vertices on
//! neighbouring edges, and coplanar fragments are merged back into larger polygons, so the
//! result stays a clean low-poly mesh that can be edited further.
use super::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Difference,
    Intersect,
}

#[derive(Clone, Copy, Debug)]
struct PV {
    pos: Vec3,
    uv: Vec2,
    color: [f32; 3],
    src: u32,
    /// Which mesh the vertex came from (0 = target, 1 = cutter): skin weights come from `src`.
    from: u8,
}
impl PV {
    fn lerp(&self, o: &PV, t: f32) -> PV {
        PV {
            pos: self.pos.lerp(o.pos, t),
            uv: Vec2::new(self.uv.x + (o.uv.x - self.uv.x) * t, self.uv.y + (o.uv.y - self.uv.y) * t),
            color: [0, 1, 2].map(|i| self.color[i] + (o.color[i] - self.color[i]) * t),
            src: if t < 0.5 { self.src } else { o.src },
            from: if t < 0.5 { self.from } else { o.from },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Plane {
    n: Vec3,
    w: f32,
}

#[derive(Clone, Debug)]
struct Poly {
    v: Vec<PV>,
    plane: Plane,
    part: u16,
    smooth: bool,
}

const EPS: f32 = 1e-5;

impl Plane {
    fn from_poly(v: &[PV]) -> Option<Plane> {
        let pts: Vec<Vec3> = v.iter().map(|p| p.pos).collect();
        let n = newell(&pts);
        if n.length() < 1e-12 {
            return None;
        }
        let n = n.normalized();
        Some(Plane { n, w: n.dot(pts[0]) })
    }
    fn flip(&mut self) {
        self.n = -self.n;
        self.w = -self.w;
    }
    /// Sorts `p` into the four lists (coplanar ones go front/back by facing).
    fn split(&self, p: &Poly, cf: &mut Vec<Poly>, cb: &mut Vec<Poly>, f: &mut Vec<Poly>, b: &mut Vec<Poly>) {
        const COPLANAR: u8 = 0;
        const FRONT: u8 = 1;
        const BACK: u8 = 2;
        let mut kind = 0u8;
        let types: Vec<u8> = p
            .v
            .iter()
            .map(|v| {
                let t = self.n.dot(v.pos) - self.w;
                let k = if t < -EPS {
                    BACK
                } else if t > EPS {
                    FRONT
                } else {
                    COPLANAR
                };
                kind |= k;
                k
            })
            .collect();
        match kind {
            COPLANAR => {
                if self.n.dot(p.plane.n) > 0.0 {
                    cf.push(p.clone())
                } else {
                    cb.push(p.clone())
                }
            }
            FRONT => f.push(p.clone()),
            BACK => b.push(p.clone()),
            _ => {
                let (mut fv, mut bv) = (vec![], vec![]);
                let n = p.v.len();
                for i in 0..n {
                    let j = (i + 1) % n;
                    let (ti, tj) = (types[i], types[j]);
                    let (vi, vj) = (p.v[i], p.v[j]);
                    if ti != BACK {
                        fv.push(vi);
                    }
                    if ti != FRONT {
                        bv.push(vi);
                    }
                    if (ti | tj) == 3 {
                        let t = (self.w - self.n.dot(vi.pos)) / self.n.dot(vj.pos - vi.pos);
                        let v = vi.lerp(&vj, t);
                        fv.push(v);
                        bv.push(v);
                    }
                }
                if fv.len() >= 3 {
                    f.push(Poly { v: fv, ..p.clone() });
                }
                if bv.len() >= 3 {
                    b.push(Poly { v: bv, ..p.clone() });
                }
            }
        }
    }
}

#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polys: Vec<Poly>,
}

impl Node {
    fn new(polys: Vec<Poly>) -> Node {
        let mut n = Node::default();
        n.build(polys);
        n
    }
    fn invert(&mut self) {
        for p in &mut self.polys {
            p.v.reverse();
            p.plane.flip();
        }
        if let Some(pl) = &mut self.plane {
            pl.flip();
        }
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }
    fn clip_polys(&self, polys: Vec<Poly>) -> Vec<Poly> {
        let Some(pl) = self.plane else { return polys };
        let (mut f, mut b) = (vec![], vec![]);
        for p in &polys {
            let (mut cf, mut cb) = (vec![], vec![]);
            pl.split(p, &mut cf, &mut cb, &mut f, &mut b);
            f.extend(cf);
            b.extend(cb);
        }
        let f = match &self.front {
            Some(n) => n.clip_polys(f),
            None => f,
        };
        let b = match &self.back {
            Some(n) => n.clip_polys(b),
            None => vec![],
        };
        let mut out = f;
        out.extend(b);
        out
    }
    fn clip_to(&mut self, other: &Node) {
        self.polys = other.clip_polys(std::mem::take(&mut self.polys));
        if let Some(f) = &mut self.front {
            f.clip_to(other);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(other);
        }
    }
    fn all(&self, out: &mut Vec<Poly>) {
        out.extend(self.polys.iter().cloned());
        if let Some(f) = &self.front {
            f.all(out);
        }
        if let Some(b) = &self.back {
            b.all(out);
        }
    }
    fn build(&mut self, polys: Vec<Poly>) {
        if polys.is_empty() {
            return;
        }
        let pl = *self.plane.get_or_insert(polys[0].plane);
        let (mut f, mut b) = (vec![], vec![]);
        let mut own = vec![];
        let mut own_b = vec![];
        for p in &polys {
            pl.split(p, &mut own, &mut own_b, &mut f, &mut b);
        }
        self.polys.extend(own);
        self.polys.extend(own_b);
        if !f.is_empty() {
            self.front.get_or_insert_with(Default::default).build(f);
        }
        if !b.is_empty() {
            self.back.get_or_insert_with(Default::default).build(b);
        }
    }
}

fn polys_of(em: &EditMesh, from: u8, part: Option<u16>) -> Vec<Poly> {
    let mut out = vec![];
    for (fi, f) in em.faces.iter().enumerate() {
        if em.face_hidden(fi as u32) {
            continue;
        }
        let pts = em.face_points(f);
        let pv: Vec<PV> = f.c.iter().map(|c| PV { pos: em.pos(c.v), uv: c.uv, color: c.color, src: c.v, from }).collect();
        for t in triangulate(&pts) {
            let v = vec![pv[t[0]], pv[t[1]], pv[t[2]]];
            if let Some(plane) = Plane::from_poly(&v) {
                out.push(Poly { v, plane, part: part.unwrap_or(f.part), smooth: f.smooth });
            }
        }
    }
    out
}

impl EditMesh {
    /// Combines this mesh with `cutter` (given in this mesh's space). New polygons from the cutter
    /// use part `cutter_part` of this mesh. Returns false when the result would be empty.
    pub fn boolean(&mut self, cutter: &EditMesh, op: BoolOp, cutter_part: u16) -> bool {
        let cp = cutter_part.min(self.parts.len().saturating_sub(1) as u16);
        let pa = polys_of(self, 0, None);
        let pb = polys_of(cutter, 1, Some(cp));
        if pa.is_empty() && pb.is_empty() {
            return false;
        }
        let mut a = Node::new(pa);
        let mut b = Node::new(pb);
        match op {
            BoolOp::Union => {
                a.clip_to(&b);
                b.clip_to(&a);
                b.invert();
                b.clip_to(&a);
                b.invert();
                let mut rest = vec![];
                b.all(&mut rest);
                a.build(rest);
            }
            BoolOp::Difference => {
                a.invert();
                a.clip_to(&b);
                b.clip_to(&a);
                b.invert();
                b.clip_to(&a);
                b.invert();
                let mut rest = vec![];
                b.all(&mut rest);
                a.build(rest);
                a.invert();
            }
            BoolOp::Intersect => {
                a.invert();
                b.clip_to(&a);
                b.invert();
                a.clip_to(&b);
                b.clip_to(&a);
                let mut rest = vec![];
                b.all(&mut rest);
                a.build(rest);
                a.invert();
            }
        }
        let mut polys = vec![];
        a.all(&mut polys);
        if polys.is_empty() {
            return false;
        }
        self.rebuild_from_polys(cutter, polys);
        true
    }

    /// Boolean inside one mesh: the selected polygons cut the rest (Edit mode "Intersect").
    pub fn boolean_selected(&mut self, op: BoolOp) -> bool {
        if self.sel_f.is_empty() || self.sel_f.len() == self.faces.len() {
            return false;
        }
        let mut cutter = self.clone();
        let sel = self.sel_f.clone();
        for (i, f) in cutter.faces.iter_mut().enumerate() {
            if !sel.contains(&(i as u32)) {
                f.c.clear();
            }
        }
        cutter.compact();
        for &f in &sel {
            self.faces[f as usize].c.clear();
        }
        self.sel_f.clear();
        self.compact();
        let part = cutter.faces.first().map_or(0, |f| f.part);
        // the cutter lives in the same mesh: keep its parts
        let mut cut = cutter;
        for f in &mut cut.faces {
            f.part = part;
        }
        self.boolean(&cut, op, part)
    }

    fn rebuild_from_polys(&mut self, cutter: &EditMesh, polys: Vec<Poly>) {
        let scale = self.verts.iter().chain(cutter.verts.iter()).fold(0.0f32, |m, v| m.max(v.pos.length())).max(1e-3);
        let eps = scale * 2e-5;
        // weld
        let mut grid: HashMap<(i64, i64, i64), Vec<u32>> = HashMap::new();
        let mut verts: Vec<EVert> = vec![];
        let key = |p: Vec3| ((p.x / eps).floor() as i64, (p.y / eps).floor() as i64, (p.z / eps).floor() as i64);
        let mut faces: Vec<(Vec<(u32, PV)>, u16, bool)> = vec![];
        for p in &polys {
            let mut c: Vec<(u32, PV)> = vec![];
            for v in &p.v {
                let k = key(v.pos);
                let mut found = None;
                'search: for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            if let Some(list) = grid.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                                for &i in list {
                                    if (verts[i as usize].pos - v.pos).length() <= eps {
                                        found = Some(i);
                                        break 'search;
                                    }
                                }
                            }
                        }
                    }
                }
                let id = found.unwrap_or_else(|| {
                    let src = if v.from == 0 { self.verts.get(v.src as usize) } else { cutter.verts.get(v.src as usize) };
                    let mut ev = src.cloned().unwrap_or(EVert::at(v.pos));
                    ev.pos = v.pos;
                    verts.push(ev);
                    let id = (verts.len() - 1) as u32;
                    grid.entry(k).or_default().push(id);
                    id
                });
                if c.last().map_or(true, |l| l.0 != id) {
                    c.push((id, *v));
                }
            }
            while c.len() > 1 && c[0].0 == c[c.len() - 1].0 {
                c.pop();
            }
            if c.len() >= 3 {
                faces.push((c, p.part, p.smooth));
            }
        }
        // close T-junctions: vertices lying inside an edge of another polygon are inserted there
        let mut order: Vec<u32> = (0..verts.len() as u32).collect();
        order.sort_by(|&a, &b| verts[a as usize].pos.x.partial_cmp(&verts[b as usize].pos.x).unwrap_or(std::cmp::Ordering::Equal));
        let xs: Vec<f32> = order.iter().map(|&i| verts[i as usize].pos.x).collect();
        for (c, _, _) in &mut faces {
            let mut out: Vec<(u32, PV)> = Vec::with_capacity(c.len());
            let n = c.len();
            for i in 0..n {
                let (a, b) = (c[i], c[(i + 1) % n]);
                out.push(a);
                let (pa, pb) = (verts[a.0 as usize].pos, verts[b.0 as usize].pos);
                let d = pb - pa;
                let len2 = d.dot(d);
                if len2 < eps * eps {
                    continue;
                }
                let (x0, x1) = (pa.x.min(pb.x) - eps, pa.x.max(pb.x) + eps);
                let start = xs.partition_point(|&x| x < x0);
                let mut ins: Vec<(f32, u32)> = vec![];
                for k in start..order.len() {
                    if xs[k] > x1 {
                        break;
                    }
                    let w = order[k];
                    if w == a.0 || w == b.0 {
                        continue;
                    }
                    let pw = verts[w as usize].pos;
                    let t = (pw - pa).dot(d) / len2;
                    if t <= 1e-4 || t >= 1.0 - 1e-4 {
                        continue;
                    }
                    if (pa + d * t - pw).length() <= eps * 2.0 {
                        ins.push((t, w));
                    }
                }
                ins.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
                for (t, w) in ins {
                    out.push((w, a.1.lerp(&b.1, t)));
                }
            }
            *c = out;
        }
        let parts = self.parts.clone();
        let mode = self.mode;
        *self = EditMesh { verts, parts, mode, ..Default::default() };
        for (c, part, smooth) in faces {
            let mut seen = std::collections::HashSet::new();
            if !c.iter().all(|x| seen.insert(x.0)) {
                continue;
            }
            self.faces.push(EFace { c: c.iter().map(|(v, p)| Corner { v: *v, uv: p.uv, color: p.color }).collect(), part, smooth });
        }
        self.merge_coplanar();
        let all: Vec<u32> = (0..self.faces.len() as u32).collect();
        self.tris_to_quads(&all, 0.5);
        // seams where the UVs do not meet
        let topo = self.topo();
        for (e, fs) in &topo.edge_faces {
            if fs.len() == 2 {
                let (f, g) = (&self.faces[fs[0] as usize], &self.faces[fs[1] as usize]);
                let split = [e.0, e.1].iter().any(|&v| match (f.corner_of(v), g.corner_of(v)) {
                    (Some(i), Some(j)) => !f.c[i].same_attrs(&g.c[j]),
                    _ => false,
                });
                if split {
                    self.seams.insert(*e);
                }
            }
        }
        self.compact();
    }

    /// Merges neighbouring polygons that lie in one plane and share part, shading and UVs, then
    /// removes the vertices left in the middle of straight edges.
    pub fn merge_coplanar(&mut self) {
        loop {
            let topo = self.topo();
            let normals: Vec<Vec3> = (0..self.faces.len() as u32).map(|f| self.face_normal(f)).collect();
            let mut dead = vec![false; self.faces.len()];
            let mut merged = 0;
            let mut edges: Vec<(&Edge, &Vec<u32>)> = topo.edge_faces.iter().collect();
            edges.sort_by_key(|(e, _)| **e);
            for (e, fs) in edges {
                if fs.len() != 2 || fs[0] == fs[1] {
                    continue;
                }
                let (fi, gi) = (fs[0] as usize, fs[1] as usize);
                if dead[fi] || dead[gi] {
                    continue;
                }
                let (f, g) = (&self.faces[fi], &self.faces[gi]);
                if f.part != g.part || f.smooth != g.smooth || normals[fi].dot(normals[gi]) < 0.99999 {
                    continue;
                }
                // still sharing this edge with matching attributes?
                let nf = f.c.len();
                let ng = g.c.len();
                let Some(kf) = (0..nf).find(|&k| f.c[k].v == e.0 && f.c[(k + 1) % nf].v == e.1 || f.c[k].v == e.1 && f.c[(k + 1) % nf].v == e.0) else { continue };
                let (a, b) = (f.c[kf].v, f.c[(kf + 1) % nf].v);
                let Some(kg) = (0..ng).find(|&k| g.c[k].v == b && g.c[(k + 1) % ng].v == a) else { continue };
                if !f.c[kf].same_attrs(&g.c[(kg + 1) % ng]) || !f.c[(kf + 1) % nf].same_attrs(&g.c[kg]) {
                    continue;
                }
                let mut c: Vec<Corner> = (1..=nf).map(|s| f.c[(kf + s) % nf]).collect(); // b .. a
                c.extend((2..ng).map(|s| g.c[(kg + s) % ng])); // after a .. before b
                let mut seen = std::collections::HashSet::new();
                if !c.iter().all(|x| seen.insert(x.v)) {
                    continue;
                }
                self.faces[fi].c = c;
                self.faces[gi].c.clear();
                dead[gi] = true;
                dead[fi] = true; // its edges changed: next round
                merged += 1;
            }
            if merged == 0 {
                break;
            }
            self.faces.retain(|f| !f.c.is_empty());
        }
        // vertices in the middle of straight edges
        let topo = self.topo();
        let mut drop = vec![];
        for v in 0..self.verts.len() as u32 {
            let nb = &topo.vert_nbrs[v as usize];
            if nb.len() != 2 || topo.vert_faces[v as usize].is_empty() {
                continue;
            }
            let (p, a, b) = (self.pos(v), self.pos(nb[0]), self.pos(nb[1]));
            let (da, db) = ((a - p).normalized(), (b - p).normalized());
            if da.dot(db) < -0.99999 && topo.vert_faces[v as usize].iter().all(|&f| self.faces[f as usize].c.len() > 3) {
                drop.push(v);
            }
        }
        if !drop.is_empty() {
            let set: BTreeSet<u32> = drop.into_iter().collect();
            for f in &mut self.faces {
                if f.c.iter().filter(|c| !set.contains(&c.v)).count() >= 3 {
                    f.c.retain(|c| !set.contains(&c.v));
                }
            }
        }
        self.compact();
    }
}
