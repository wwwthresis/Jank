//! Edge and vertex bevel.
//!
//! Edge bevel works per vertex. The polygons around a vertex form a fan; the bevelled edges cut
//! the fan into *sectors*. Every sector gets one new vertex: inside the polygon (a mitre) when
//! the sector is a single polygon between two bevelled edges, otherwise slid along an unbevelled
//! edge of the sector. A vertex with only one bevelled edge (the end of a bevel) gets one new
//! vertex on each side instead. Each bevelled edge then becomes a strip of `segments` quads
//! between the arcs at its two ends; arcs are rational quadratic curves (true circular arcs for
//! symmetric corners) and are shared between the strips that meet at a vertex, so the result has
//! no cracks. Holes left at corners where three or more bevelled edges meet are capped.
use super::*;
use crate::fasthash::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BevelOptions {
    /// Distance of the new edges from the old one.
    pub width: f32,
    /// Number of quads across the bevel (1 = a flat chamfer).
    pub segments: usize,
    /// Keep new vertices from sliding past the middle of their edges.
    pub clamp: bool,
}
impl Default for BevelOptions {
    fn default() -> Self {
        BevelOptions { width: 0.1, segments: 1, clamp: true }
    }
}

/// One polygon around a vertex: (face, corner index, previous vertex, next vertex).
#[derive(Clone, Copy)]
struct FanFace {
    f: u32,
    k: usize,
    prev: u32,
    next: u32,
}

/// The polygons around `v` in order (each one's `next` is the following one's `prev`), and
/// whether the fan is closed. `None` for non-manifold vertices.
fn fan(em: &EditMesh, topo: &Topo, v: u32) -> Option<(Vec<FanFace>, bool)> {
    let mut items = vec![];
    for &f in topo.vert_faces.get(v as usize)? {
        let face = &em.faces[f as usize];
        let n = face.c.len();
        let k = face.corner_of(v)?;
        if face.verts().filter(|&x| x == v).count() != 1 {
            return None;
        }
        items.push(FanFace { f, k, prev: face.c[(k + n - 1) % n].v, next: face.c[(k + 1) % n].v });
    }
    if items.is_empty() {
        return None;
    }
    let mut by_prev: HashMap<u32, usize> = HashMap::default();
    for (i, it) in items.iter().enumerate() {
        if by_prev.insert(it.prev, i).is_some() {
            return None;
        }
    }
    let nexts: HashSet<u32> = items.iter().map(|i| i.next).collect();
    if nexts.len() != items.len() {
        return None;
    }
    // an open fan starts at the polygon whose previous edge has no neighbour
    let start = items.iter().position(|it| !nexts.contains(&it.prev));
    let closed = start.is_none();
    let mut order = vec![];
    let mut cur = start.unwrap_or(0);
    let mut seen = vec![false; items.len()];
    loop {
        if seen[cur] {
            break;
        }
        seen[cur] = true;
        order.push(items[cur]);
        match by_prev.get(&items[cur].next) {
            Some(&nx) => cur = nx,
            None => break,
        }
    }
    if order.len() != items.len() {
        return None; // more than one fan: a "bow-tie" vertex
    }
    Some((order, closed))
}

fn dir(a: Vec3, b: Vec3) -> Vec3 {
    (b - a).normalized()
}
fn sin_between(a: Vec3, b: Vec3) -> f32 {
    a.cross(b).length()
}

/// Point on a rational quadratic curve from `a` to `b` with control `c` (a circular arc when
/// |c-a| == |c-b|).
fn arc_point(a: Vec3, c: Vec3, b: Vec3, t: f32) -> Vec3 {
    let (da, db) = ((a - c).normalized(), (b - c).normalized());
    let cos_alpha = da.dot(db).clamp(-1.0, 1.0);
    // weight = sin(alpha / 2) where alpha is the angle at the control point
    let w = ((1.0 - cos_alpha) * 0.5).max(0.0).sqrt();
    let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t);
    (a * b0 + c * b1 + b * b2) / (b0 + b1 + b2)
}

impl EditMesh {
    /// Corner attributes for a new vertex at `pos` inside polygon `f` near its corner `k`: the
    /// offset is expressed in the two edges of the corner and the UV/colour follow it.
    fn corner_near(&self, f: u32, k: usize, pos: Vec3, v: u32) -> Corner {
        let face = &self.faces[f as usize];
        let n = face.c.len();
        let (c0, cp, cn) = (face.c[k], face.c[(k + n - 1) % n], face.c[(k + 1) % n]);
        let o = self.pos(c0.v);
        let (e1, e2, d) = (self.pos(cp.v) - o, self.pos(cn.v) - o, pos - o);
        let (a11, a12, a22) = (e1.dot(e1), e1.dot(e2), e2.dot(e2));
        let (b1, b2) = (d.dot(e1), d.dot(e2));
        let det = a11 * a22 - a12 * a12;
        let (s, t) = if det.abs() > 1e-12 { ((b1 * a22 - b2 * a12) / det, (b2 * a11 - b1 * a12) / det) } else { (0.0, 0.0) };
        Corner {
            v,
            uv: Vec2::new(c0.uv.x + (cp.uv.x - c0.uv.x) * s + (cn.uv.x - c0.uv.x) * t, c0.uv.y + (cp.uv.y - c0.uv.y) * s + (cn.uv.y - c0.uv.y) * t),
            color: [0, 1, 2].map(|i| (c0.color[i] + (cp.color[i] - c0.color[i]) * s + (cn.color[i] - c0.color[i]) * t).clamp(0.0, 1.0)),
        }
    }

    /// Bevels the selected edges (in Face mode: the edges of the selected polygons).
    /// Only edges between exactly two polygons with manifold ends are bevelled.
    pub fn bevel(&mut self, o: BevelOptions) -> bool {
        let w = o.width.max(1e-5);
        let segs = o.segments.clamp(1, 32);
        let topo = self.topo();
        let mut edges: BTreeSet<Edge> = self.sel_e.iter().copied().filter(|e| topo.faces_of(*e).len() == 2).collect();
        let mut fans: HashMap<u32, (Vec<FanFace>, bool)> = HashMap::default();
        for &(a, b) in &edges.clone() {
            for v in [a, b] {
                if !fans.contains_key(&v) {
                    match fan(self, &topo, v) {
                        Some(fv) => {
                            fans.insert(v, fv);
                        }
                        None => {
                            edges.remove(&(a, b));
                        }
                    }
                }
            }
        }
        edges.retain(|&(a, b)| fans.contains_key(&a) && fans.contains_key(&b));
        if edges.is_empty() {
            return false;
        }
        let p0: Vec<Vec3> = self.verts.iter().map(|v| v.pos).collect();
        let elen = |a: u32, b: u32| (p0[a as usize] - p0[b as usize]).length();
        // replacement corners per (face, original vertex)
        let mut repl: HashMap<(u32, u32), Vec<Corner>> = HashMap::default();
        // arcs: (vertex, low, high) -> interior vertices from low to high
        let mut arcs: HashMap<(u32, u32, u32), Vec<u32>> = HashMap::default();
        let mut kept: HashSet<u32> = HashSet::default();
        let mut new_verts: HashSet<u32> = HashSet::default();
        let mut verts: Vec<u32> = fans.keys().copied().filter(|v| edges.iter().any(|e| e.0 == *v || e.1 == *v)).collect();
        verts.sort_unstable();
        // pending terminal arcs to put into the cap polygon of valence-3 ends
        let mut cap_arcs: Vec<(u32, u32, usize, u32, u32, Vec3)> = vec![];
        for &v in &verts {
            let (fv, closed) = fans[&v].clone();
            let pv = self.pos(v);
            let spoke_bev = |x: u32| edges.contains(&ek(v, x));
            let nb = fv.iter().filter(|it| spoke_bev(it.next)).count() + if closed { 0 } else { usize::from(spoke_bev(fv[0].prev)) };
            let clamp_d = |d: f32, x: u32| if o.clamp { d.min(elen(v, x) * 0.48) } else { d };
            if nb == 1 && closed {
                // the end of a bevel: one vertex on each side edge of the bevelled edge
                let m = fv.len();
                let Some(j) = fv.iter().position(|it| spoke_bev(it.prev)) else { continue };
                if m < 3 {
                    continue;
                }
                let order: Vec<FanFace> = (0..m).map(|i| fv[(j + i) % m]).collect();
                let x = order[0].prev; // the other end of the bevelled edge
                let de = dir(pv, self.pos(x));
                let (ul, ur) = (order[0].next, order[m - 1].prev);
                let slide = |u: u32| {
                    let du = dir(pv, self.pos(u));
                    let d = w / sin_between(du, de).max(0.2);
                    pv + du * clamp_d(d, u)
                };
                let (pl, pr) = (slide(ul), slide(ur));
                let al = self.dup_vert(v, pl);
                let ar = self.dup_vert(v, pr);
                new_verts.insert(al);
                new_verts.insert(ar);
                repl.insert((order[0].f, v), vec![self.corner_near(order[0].f, order[0].k, pl, al)]);
                repl.insert((order[m - 1].f, v), vec![self.corner_near(order[m - 1].f, order[m - 1].k, pr, ar)]);
                if m == 3 {
                    // the middle polygon is the cap: v is replaced by the arc
                    cap_arcs.push((v, order[1].f, order[1].k, al, ar, de));
                } else {
                    kept.insert(v);
                    let f1 = order[1];
                    let c = self.corner_near(f1.f, f1.k, pl, al);
                    repl.insert((f1.f, v), vec![c, self.faces[f1.f as usize].c[f1.k]]);
                    let fl = order[m - 2];
                    let c = self.corner_near(fl.f, fl.k, pr, ar);
                    repl.insert((fl.f, v), vec![self.faces[fl.f as usize].c[fl.k], c]);
                }
                continue;
            }
            // sectors between bevelled spokes
            let m = fv.len();
            let mut sectors: Vec<Vec<usize>> = vec![];
            if closed {
                let Some(j) = fv.iter().position(|it| spoke_bev(it.prev)) else { continue };
                let mut cur = vec![];
                for i in 0..m {
                    let idx = (j + i) % m;
                    cur.push(idx);
                    if spoke_bev(fv[idx].next) {
                        sectors.push(std::mem::take(&mut cur));
                    }
                }
                if !cur.is_empty() {
                    sectors.push(cur);
                }
            } else {
                let mut cur = vec![];
                for idx in 0..m {
                    cur.push(idx);
                    if spoke_bev(fv[idx].next) && idx + 1 < m {
                        sectors.push(std::mem::take(&mut cur));
                    }
                }
                sectors.push(cur);
            }
            for sec in sectors {
                let first = fv[sec[0]];
                let last = fv[*sec.last().unwrap()];
                let pos = if sec.len() == 1 && spoke_bev(first.prev) && spoke_bev(first.next) {
                    // mitre inside the polygon
                    let (d1, d2) = (dir(pv, self.pos(first.prev)), dir(pv, self.pos(first.next)));
                    let s = sin_between(d1, d2);
                    if s < 0.05 {
                        let nrm = self.face_normal(first.f);
                        pv + nrm.cross(d2).normalized() * w
                    } else {
                        let mut off = (d1 + d2) * (w / s);
                        if o.clamp {
                            let lim = elen(v, first.prev).min(elen(v, first.next)) * 0.48;
                            if off.length() > lim {
                                off = off.normalized() * lim;
                            }
                        }
                        pv + off
                    }
                } else {
                    // slide along the middle unbevelled spoke of the sector
                    let mut spokes: Vec<u32> = vec![];
                    if !spoke_bev(first.prev) {
                        spokes.push(first.prev);
                    }
                    for w2 in sec.windows(2) {
                        spokes.push(fv[w2[0]].next);
                    }
                    if !spoke_bev(last.next) {
                        spokes.push(last.next);
                    }
                    let spokes: Vec<u32> = spokes.into_iter().filter(|&x| !spoke_bev(x)).collect();
                    let Some(&u) = spokes.get(spokes.len() / 2) else { continue };
                    let du = dir(pv, self.pos(u));
                    let mut s = 0.0;
                    let mut cnt = 0.0;
                    for b in [first.prev, last.next] {
                        if spoke_bev(b) {
                            s += sin_between(du, dir(pv, self.pos(b)));
                            cnt += 1.0;
                        }
                    }
                    let s = if cnt > 0.0 { (s / cnt).max(0.2) } else { 1.0 };
                    pv + du * clamp_d(w / s, u)
                };
                let nv = self.dup_vert(v, pos);
                new_verts.insert(nv);
                for &i in &sec {
                    let it = fv[i];
                    repl.insert((it.f, v), vec![self.corner_near(it.f, it.k, pos, nv)]);
                }
            }
        }
        // arcs (shared between the strips meeting at a vertex)
        let mut arc = |em: &mut EditMesh, v: u32, a: u32, b: u32, de: Vec3, new_verts: &mut HashSet<u32>| -> Vec<u32> {
            let key = (v, a.min(b), a.max(b));
            if !arcs.contains_key(&key) {
                let (pa, pb, pv) = (em.pos(key.1), em.pos(key.2), em.pos(v));
                let along = ((pa - pv).dot(de) + (pb - pv).dot(de)) * 0.5;
                let ctrl = pv + de * along;
                let mut inner = vec![];
                for j in 1..segs {
                    let p = arc_point(pa, ctrl, pb, j as f32 / segs as f32);
                    let nv = em.dup_vert(v, p);
                    new_verts.insert(nv);
                    inner.push(nv);
                }
                arcs.insert(key, inner);
            }
            let mut out = vec![key.1];
            out.extend(arcs[&key].iter().copied());
            out.push(key.2);
            if a != key.1 {
                out.reverse();
            }
            out
        };
        // strips
        let mut strips: Vec<EFace> = vec![];
        for &(a, b) in &edges {
            let fs = topo.faces_of((a, b));
            let (mut f1, mut f2) = (fs[0], fs[1]);
            if !self.faces[f1 as usize].edges().any(|(x, y)| x == a && y == b) {
                std::mem::swap(&mut f1, &mut f2);
            }
            let get = |f: u32, v: u32| repl.get(&(f, v)).and_then(|r| if r.len() == 1 { Some(r[0]) } else { None });
            let (Some(s1a), Some(s2a), Some(s1b), Some(s2b)) = (get(f1, a), get(f2, a), get(f1, b), get(f2, b)) else { continue };
            let de_a = dir(self.pos(a), self.pos(b));
            let ra = arc(self, a, s1a.v, s2a.v, de_a, &mut new_verts);
            let rb = arc(self, b, s1b.v, s2b.v, -de_a, &mut new_verts);
            let face = &self.faces[f1 as usize];
            let (part, smooth) = (face.part, face.smooth || segs > 1);
            let mix = |c1: Corner, c2: Corner, j: usize, v: u32| c1.lerp(&c2, j as f32 / segs as f32, v);
            for j in 0..segs {
                strips.push(EFace {
                    c: vec![mix(s1b, s2b, j, rb[j]), mix(s1a, s2a, j, ra[j]), mix(s1a, s2a, j + 1, ra[j + 1]), mix(s1b, s2b, j + 1, rb[j + 1])],
                    part,
                    smooth,
                });
            }
        }
        // cap polygons at valence-3 ends get the arc instead of the vertex
        for (v, f, k, al, ar, de) in cap_arcs {
            // the cap's previous neighbour lies on the spoke of `al` (fan order), so the arc runs al -> ar
            let (x0, x1) = (al, ar);
            let seq = arc(self, v, x0, x1, de, &mut new_verts);
            let cs = seq.iter().map(|&nv| self.corner_near(f, k, self.pos(nv), nv)).collect();
            repl.insert((f, v), cs);
        }
        // rebuild the polygons
        let touched: Vec<u32> = repl.keys().map(|k| k.0).collect::<BTreeSet<u32>>().into_iter().collect();
        for f in touched {
            let old = self.faces[f as usize].c.clone();
            let mut c = Vec::with_capacity(old.len() + 4);
            for corner in &old {
                match repl.get(&(f, corner.v)) {
                    Some(r) => c.extend(r.iter().copied()),
                    None => c.push(*corner),
                }
            }
            self.faces[f as usize].c = c;
        }
        let first_new = self.faces.len() as u32;
        self.faces.extend(strips);
        // cap the holes whose border consists only of new (or kept end) vertices
        let mut directed: HashSet<(u32, u32)> = HashSet::default();
        for f in &self.faces {
            for e in f.edges() {
                directed.insert(e);
            }
        }
        let inside = |x: u32| new_verts.contains(&x) || kept.contains(&x);
        let mut next: HashMap<u32, Option<u32>> = HashMap::default();
        for &(a, b) in &directed {
            if !directed.contains(&(b, a)) && inside(a) && inside(b) {
                // the cap runs b -> a
                let e = next.entry(b).or_insert(Some(a));
                if *e != Some(a) {
                    *e = None; // ambiguous
                }
            }
        }
        let mut used: HashSet<u32> = HashSet::default();
        let mut caps = vec![];
        let mut starts: Vec<u32> = next.keys().copied().collect();
        starts.sort_unstable();
        for s in starts {
            if used.contains(&s) {
                continue;
            }
            let mut lp = vec![s];
            let mut cur = s;
            let ok = loop {
                let Some(Some(nx)) = next.get(&cur) else { break false };
                if *nx == s {
                    break true;
                }
                if lp.contains(nx) || lp.len() > 256 {
                    break false;
                }
                lp.push(*nx);
                cur = *nx;
            };
            if ok && lp.len() >= 3 {
                used.extend(lp.iter().copied());
                caps.push(lp);
            }
        }
        let topo2 = self.topo();
        for lp in caps {
            let (part, smooth) = self.neighbour_part(&topo2, &lp);
            let c = lp.iter().map(|&v| self.corner_for(&topo2, v)).collect();
            self.faces.push(EFace { c, part, smooth: smooth || segs > 1 });
        }
        for e in &edges {
            self.sharp.remove(e);
            self.seams.remove(e);
        }
        self.sel_f = (first_new..self.faces.len() as u32).collect();
        self.mode_flush_faces();
        self.compact();
        true
    }

    /// Bevels the selected vertices: each one is replaced by a small polygon whose corners lie
    /// `width` along each of its edges.
    pub fn bevel_verts(&mut self, width: f32) -> bool {
        let topo = self.topo();
        let sel: Vec<u32> = self.sel_v.iter().copied().collect();
        let mut caps = vec![];
        let mut any = false;
        let mut done: HashSet<(u32, u32)> = HashSet::default();
        let mut on_edge: HashMap<(u32, u32), u32> = HashMap::default();
        for v in sel {
            let Some((fv, closed)) = fan(self, &topo, v) else { continue };
            if !closed || fv.len() < 3 {
                continue;
            }
            let pv = self.pos(v);
            let mut ring = vec![];
            for it in &fv {
                let y = it.next;
                let d = (self.pos(y) - pv).length();
                let t = (width / d.max(1e-6)).min(if self.sel_v.contains(&y) { 0.45 } else { 0.9 });
                let nv = self.dup_vert(v, pv.lerp(self.pos(y), t));
                on_edge.insert((v, y), nv);
                ring.push(nv);
            }
            for it in &fv {
                let (xp, xn) = (on_edge[&(v, it.prev)], on_edge[&(v, it.next)]);
                let (cp, cn) = (self.corner_near(it.f, it.k, self.pos(xp), xp), self.corner_near(it.f, it.k, self.pos(xn), xn));
                done.insert((it.f, v));
                let face = &mut self.faces[it.f as usize];
                let k = face.corner_of(v).unwrap();
                face.c.splice(k..=k, [cp, cn]);
            }
            ring.reverse();
            caps.push((ring, fv[0].f));
            any = true;
        }
        if !any {
            return false;
        }
        let first = self.faces.len() as u32;
        let topo2 = self.topo();
        for (ring, f) in caps {
            let face = &self.faces[f as usize];
            let (part, smooth) = (face.part, face.smooth);
            let c: Vec<Corner> = ring.iter().map(|&v| self.corner_for(&topo2, v)).collect();
            self.faces.push(EFace { c, part, smooth });
        }
        self.sel_f = (first..self.faces.len() as u32).collect();
        self.mode_flush_faces();
        self.compact();
        true
    }
}
