//! Polygon reduction by edge collapse driven by quadric error metrics (Garland and Heckbert, 1997).
//! Topology is computed by POSITION: vertices split by a UV seam share one group and move
//! together, so seams never tear. Open boundaries are held by penalty planes, and face flips are forbidden.
use crate::math::*;
use crate::mesh::*;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct DecimateOptions {
    pub target_tris: usize,
    /// Weight of the protection of open boundaries (0 = no protection).
    pub boundary_weight: f64,
}
impl Default for DecimateOptions {
    fn default() -> Self {
        Self { target_tris: 500, boundary_weight: 100.0 }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecimateReport {
    pub before: usize,
    pub after: usize,
}

/// A symmetric 4x4 quadric stored as [a2,ab,ac,ad, b2,bc,bd, c2,cd, d2].
#[derive(Clone, Copy, Default)]
struct Q([f64; 10]);
impl Q {
    fn from_plane(a: f64, b: f64, c: f64, d: f64, w: f64) -> Q {
        Q([a * a * w, a * b * w, a * c * w, a * d * w, b * b * w, b * c * w, b * d * w, c * c * w, c * d * w, d * d * w])
    }
    fn add(&mut self, o: &Q) {
        for i in 0..10 {
            self.0[i] += o.0[i];
        }
    }
    fn sum(a: &Q, b: &Q) -> Q {
        let mut r = *a;
        r.add(b);
        r
    }
    fn eval(&self, p: Vec3) -> f64 {
        let (x, y, z) = (p.x as f64, p.y as f64, p.z as f64);
        let q = &self.0;
        q[0] * x * x + 2.0 * q[1] * x * y + 2.0 * q[2] * x * z + 2.0 * q[3] * x + q[4] * y * y + 2.0 * q[5] * y * z + 2.0 * q[6] * y + q[7] * z * z + 2.0 * q[8] * z + q[9]
    }
}

#[derive(Clone)]
struct Group {
    pos: Vec3,
    q: Q,
    verts: Vec<u32>,
    alive: bool,
    version: u32,
}

#[derive(PartialEq)]
struct Cand {
    cost: f64,
    a: u32,
    b: u32,
    va: u32,
    vb: u32,
    pos: Vec3,
}
impl Eq for Cand {}
impl Ord for Cand {
    fn cmp(&self, o: &Self) -> Ordering {
        // BinaryHeap is a max-heap; the lowest cost is needed, so the ordering is reversed
        o.cost.partial_cmp(&self.cost).unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for Cand {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

fn pos_key(p: Vec3) -> (i64, i64, i64) {
    ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64, (p.z * 1e4).round() as i64)
}

pub fn decimate(mesh: &mut Mesh, opts: &DecimateOptions) -> DecimateReport {
    mesh.cleanup();
    let before = mesh.tris.len();
    let target = opts.target_tris.max(4);
    if before <= target {
        return DecimateReport { before, after: before };
    }

    // --- groups by position
    let mut key_to_group: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut vgroup = vec![0u32; mesh.verts.len()];
    let mut groups: Vec<Group> = Vec::new();
    for (i, v) in mesh.verts.iter().enumerate() {
        let g = *key_to_group.entry(pos_key(v.pos)).or_insert_with(|| {
            groups.push(Group { pos: v.pos, q: Q::default(), verts: vec![], alive: true, version: 0 });
            (groups.len() - 1) as u32
        });
        groups[g as usize].verts.push(i as u32);
        vgroup[i] = g;
    }
    let tg = |t: &[u32; 3], vg: &Vec<u32>| [vg[t[0] as usize], vg[t[1] as usize], vg[t[2] as usize]];

    let tris: Vec<[u32; 3]> = mesh.tris.clone();
    let mut tri_alive = vec![true; tris.len()];
    let mut adj: Vec<Vec<u32>> = vec![vec![]; groups.len()];
    let mut edge_count: HashMap<(u32, u32), (u32, u32)> = HashMap::new(); // edge to (number of faces, face index)
    let mut alive_tris = 0usize;

    for (ti, t) in tris.iter().enumerate() {
        let g = tg(t, &vgroup);
        if g[0] == g[1] || g[1] == g[2] || g[0] == g[2] {
            tri_alive[ti] = false; // degenerate by position
            continue;
        }
        alive_tris += 1;
        let (p0, p1, p2) = (groups[g[0] as usize].pos, groups[g[1] as usize].pos, groups[g[2] as usize].pos);
        let n = (p1 - p0).cross(p2 - p0);
        let area = n.length() as f64 * 0.5;
        let nn = n.normalized();
        let d = -(nn.dot(p0)) as f64;
        let q = Q::from_plane(nn.x as f64, nn.y as f64, nn.z as f64, d, area);
        for &gi in &g {
            groups[gi as usize].q.add(&q);
            adj[gi as usize].push(ti as u32);
        }
        for k in 0..3 {
            let (a, b) = (g[k], g[(k + 1) % 3]);
            let key = (a.min(b), a.max(b));
            let e = edge_count.entry(key).or_insert((0, ti as u32));
            e.0 += 1;
        }
    }
    // --- boundary protection: a plane through the edge, perpendicular to the face
    if opts.boundary_weight > 0.0 {
        for (&(a, b), &(cnt, ti)) in &edge_count {
            if cnt != 1 {
                continue;
            }
            let g = tg(&tris[ti as usize], &vgroup);
            let (p0, p1, p2) = (groups[g[0] as usize].pos, groups[g[1] as usize].pos, groups[g[2] as usize].pos);
            let fn_ = (p1 - p0).cross(p2 - p0).normalized();
            let (pa, pb) = (groups[a as usize].pos, groups[b as usize].pos);
            let e = pb - pa;
            let pn = e.cross(fn_).normalized();
            let d = -(pn.dot(pa)) as f64;
            let q = Q::from_plane(pn.x as f64, pn.y as f64, pn.z as f64, d, opts.boundary_weight * (e.length() as f64).max(1e-3));
            groups[a as usize].q.add(&q);
            groups[b as usize].q.add(&q);
        }
    }

    let eval_pair = |groups: &Vec<Group>, a: u32, b: u32| -> (f64, Vec3) {
        let (ga, gb) = (&groups[a as usize], &groups[b as usize]);
        let q = Q::sum(&ga.q, &gb.q);
        let mid = ga.pos.lerp(gb.pos, 0.5);
        let mut best = (q.eval(ga.pos), ga.pos);
        for p in [gb.pos, mid] {
            let c = q.eval(p);
            if c < best.0 {
                best = (c, p);
            }
        }
        best
    };

    let mut heap = BinaryHeap::new();
    for &(a, b) in edge_count.keys() {
        let (cost, pos) = eval_pair(&groups, a, b);
        heap.push(Cand { cost, a, b, va: 0, vb: 0, pos });
    }

    while alive_tris > target {
        let Some(c) = heap.pop() else { break };
        let (a, b) = (c.a as usize, c.b as usize);
        if !groups[a].alive || !groups[b].alive || groups[a].version != c.va || groups[b].version != c.vb {
            continue;
        }
        // --- check for flipped and degenerate faces
        let mut ok = true;
        let mut touching: Vec<u32> = adj[a].iter().chain(adj[b].iter()).copied().filter(|&t| tri_alive[t as usize]).collect();
        touching.sort_unstable();
        touching.dedup();
        for &ti in &touching {
            let g = tg(&tris[ti as usize], &vgroup);
            if g.contains(&c.a) && g.contains(&c.b) {
                continue; // this face disappears
            }
            let pos_of = |gi: u32, newp: bool| if newp && (gi == c.a || gi == c.b) { c.pos } else { groups[gi as usize].pos };
            let n0 = (pos_of(g[1], false) - pos_of(g[0], false)).cross(pos_of(g[2], false) - pos_of(g[0], false));
            let n1 = (pos_of(g[1], true) - pos_of(g[0], true)).cross(pos_of(g[2], true) - pos_of(g[0], true));
            if n1.length() < 1e-9 || n0.normalized().dot(n1.normalized()) < 0.2 {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }
        // --- collapse b into a
        let bq = groups[b].q;
        groups[a].q.add(&bq);
        groups[a].pos = c.pos;
        let bverts = std::mem::take(&mut groups[b].verts);
        for &v in &bverts {
            vgroup[v as usize] = c.a;
        }
        groups[a].verts.extend(bverts);
        groups[b].alive = false;
        groups[a].version += 1;
        let badj = std::mem::take(&mut adj[b]);
        adj[a].extend(badj);
        for &ti in &touching {
            let g = tg(&tris[ti as usize], &vgroup);
            if g[0] == g[1] || g[1] == g[2] || g[0] == g[2] {
                tri_alive[ti as usize] = false;
                alive_tris -= 1;
            }
        }
        adj[a].retain(|&t| tri_alive[t as usize]);
        adj[a].sort_unstable();
        adj[a].dedup();
        // --- new candidates for the neighbours of a
        let mut nb: HashSet<u32> = HashSet::new();
        for &ti in &adj[a] {
            for gi in tg(&tris[ti as usize], &vgroup) {
                if gi != c.a {
                    nb.insert(gi);
                }
            }
        }
        for n in nb {
            let (cost, pos) = eval_pair(&groups, c.a, n);
            heap.push(Cand { cost, a: c.a, b: n, va: groups[a].version, vb: groups[n as usize].version, pos });
        }
    }

    // --- write the result
    for g in groups.iter().filter(|g| g.alive) {
        for &v in &g.verts {
            mesh.verts[v as usize].pos = g.pos;
        }
    }
    let new_tris: Vec<[u32; 3]> = tris.iter().enumerate().filter(|(i, _)| tri_alive[*i]).map(|(_, t)| *t).collect();
    mesh.tris = new_tris;
    mesh.cleanup();
    mesh.recompute_normals();
    DecimateReport { before, after: mesh.tris.len() }
}

/// Reduces the whole model to `ratio` (0..1) of its current triangle count (at least 4 per mesh).
pub fn decimate_model(model: &mut Model, ratio: f32, boundary_weight: f64) -> DecimateReport {
    let before = model.triangle_count();
    let ratio = ratio.clamp(0.01, 1.0);
    for m in &mut model.meshes {
        let target = ((m.tris.len() as f32 * ratio).round() as usize).max(4);
        decimate(m, &DecimateOptions { target_tris: target, boundary_weight });
    }
    DecimateReport { before, after: model.triangle_count() }
}

/// Automatic box UV unwrap: every face is projected along its dominant axis
/// into one of the 6 cells of a 3x2 atlas. A good starting point for painting a texture.
pub fn box_unwrap(mesh: &mut Mesh) {
    let Some((lo, hi)) = mesh.bounds() else { return };
    let size = Vec3::new((hi.x - lo.x).max(1e-6), (hi.y - lo.y).max(1e-6), (hi.z - lo.z).max(1e-6));
    let mut out_verts: Vec<Vertex> = Vec::new();
    let mut map: HashMap<(u32, u8), u32> = HashMap::new();
    let mut tris = Vec::with_capacity(mesh.tris.len());
    for t in &mesh.tris {
        let (a, b, c) = (mesh.verts[t[0] as usize].pos, mesh.verts[t[1] as usize].pos, mesh.verts[t[2] as usize].pos);
        let n = (b - a).cross(c - a);
        let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
        // cell: 0..2 are the X, Y, Z axes with a positive sign; 3..5 with a negative sign
        let (axis, neg) = if ax >= ay && ax >= az { (0, n.x < 0.0) } else if ay >= az { (1, n.y < 0.0) } else { (2, n.z < 0.0) };
        let cell = axis as u8 + if neg { 3 } else { 0 };
        let mut idx = [0u32; 3];
        for k in 0..3 {
            let vi = t[k];
            idx[k] = *map.entry((vi, cell)).or_insert_with(|| {
                let mut v = mesh.verts[vi as usize].clone();
                let p = v.pos - lo;
                let (mut u, mut w) = match axis {
                    0 => (p.z / size.z, 1.0 - p.y / size.y),
                    1 => (p.x / size.x, 1.0 - p.z / size.z),
                    _ => (p.x / size.x, 1.0 - p.y / size.y),
                };
                if neg {
                    u = 1.0 - u;
                }
                let (cx, cy) = ((cell % 3) as f32, (cell / 3) as f32);
                let pad = 0.01;
                u = pad + u.clamp(0.0, 1.0) * (1.0 - 2.0 * pad);
                w = pad + w.clamp(0.0, 1.0) * (1.0 - 2.0 * pad);
                v.uv = Vec2::new((cx + u) / 3.0, (cy + w) / 2.0);
                out_verts.push(v);
                (out_verts.len() - 1) as u32
            });
        }
        tris.push(idx);
    }
    mesh.verts = out_verts;
    mesh.tris = tris;
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn uv_sphere(seg: usize, rings: usize) -> Mesh {
        let mut m = Mesh::new("sphere");
        for r in 0..=rings {
            let phi = std::f32::consts::PI * r as f32 / rings as f32;
            for s in 0..=seg {
                let th = 2.0 * std::f32::consts::PI * s as f32 / seg as f32;
                let p = Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin());
                m.verts.push(Vertex::new(p, Vec2::new(s as f32 / seg as f32, r as f32 / rings as f32)));
            }
        }
        let w = seg + 1;
        for r in 0..rings {
            for s in 0..seg {
                let (a, b, c, d) = ((r * w + s) as u32, (r * w + s + 1) as u32, ((r + 1) * w + s) as u32, ((r + 1) * w + s + 1) as u32);
                m.tris.push([a, c, b]);
                m.tris.push([b, c, d]);
            }
        }
        m.cleanup();
        m.recompute_normals();
        m
    }
    fn grid(n: usize) -> Mesh {
        let mut m = Mesh::new("grid");
        for y in 0..=n {
            for x in 0..=n {
                m.verts.push(Vertex::new(Vec3::new(x as f32, 0.0, y as f32), Vec2::new(x as f32 / n as f32, y as f32 / n as f32)));
            }
        }
        let w = n + 1;
        for y in 0..n {
            for x in 0..n {
                let (a, b, c, d) = ((y * w + x) as u32, (y * w + x + 1) as u32, ((y + 1) * w + x) as u32, ((y + 1) * w + x + 1) as u32);
                m.tris.push([a, c, b]);
                m.tris.push([b, c, d]);
            }
        }
        m
    }
    fn valid(m: &Mesh) -> bool {
        m.tris.iter().all(|t| t.iter().all(|&i| (i as usize) < m.verts.len()) && t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
    }

    #[test]
    fn sphere_reduces_close_to_target_and_stays_round() {
        let mut m = uv_sphere(32, 24);
        let before = m.tris.len();
        let rep = decimate(&mut m, &DecimateOptions { target_tris: 200, boundary_weight: 100.0 });
        assert!(rep.before == before && rep.after <= 230 && rep.after >= 100, "after={}", rep.after);
        assert!(valid(&m));
        // the shape is preserved: the vertices stay near the unit sphere
        let max_dev = m.verts.iter().map(|v| (v.pos.length() - 1.0).abs()).fold(0.0f32, f32::max);
        assert!(max_dev < 0.15, "max_dev={max_dev}");
    }
    #[test]
    fn grid_boundary_is_preserved() {
        let mut m = grid(16);
        let (lo, hi) = m.bounds().unwrap();
        decimate(&mut m, &DecimateOptions { target_tris: 20, boundary_weight: 100.0 });
        assert!(valid(&m));
        let (lo2, hi2) = m.bounds().unwrap();
        assert!((lo - lo2).length() < 1e-3 && (hi - hi2).length() < 1e-3, "the extents of the plane must not shrink");
        assert!(m.tris.len() <= 40);
    }
    #[test]
    fn noop_when_already_small() {
        let mut m = grid(2);
        let r = decimate(&mut m, &DecimateOptions { target_tris: 100, boundary_weight: 100.0 });
        assert_eq!((r.before, r.after), (8, 8));
    }
    #[test]
    fn model_ratio() {
        let mut model = Model { meshes: vec![uv_sphere(24, 16)], ..Default::default() };
        let rep = decimate_model(&mut model, 0.25, 100.0);
        assert!(rep.after < rep.before / 3);
    }
    #[test]
    fn box_unwrap_uvs_in_range() {
        let mut m = uv_sphere(8, 6);
        let n = m.verts.len();
        box_unwrap(&mut m);
        assert!(m.verts.len() >= n);
        assert!(m.verts.iter().all(|v| (0.0..=1.0).contains(&v.uv.x) && (0.0..=1.0).contains(&v.uv.y)));
        assert!(valid(&m));
    }
}
