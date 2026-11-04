//! UV islands: groups of triangles that touch each other in texture space. Painting artists think
//! in islands ("the face", "the left arm"), so the texture editor can highlight one, select its
//! pixels, or paint only inside it.

use crate::mesh::Mesh;
use crate::selection::Selection;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    /// Triangle indices into `Mesh::tris`.
    pub tris: Vec<usize>,
    /// (u0, v0, u1, v1) of the island in UV space.
    pub bounds: (f32, f32, f32, f32),
}

/// Quantised UV key so that vertices that sit on the same UV spot are treated as one point.
fn key(uv: crate::math::Vec2) -> (i32, i32) {
    ((uv.x * 4096.0).round() as i32, (uv.y * 4096.0).round() as i32)
}

struct Dsu(Vec<usize>);
impl Dsu {
    fn find(&mut self, a: usize) -> usize {
        let mut r = a;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut c = a;
        while self.0[c] != r {
            let n = self.0[c];
            self.0[c] = r;
            c = n;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb)] = ra.min(rb);
        }
    }
}

/// Splits a mesh into islands. Islands are ordered by their first triangle.
pub fn islands(mesh: &Mesh) -> Vec<Island> {
    let n = mesh.tris.len();
    let mut dsu = Dsu((0..n).collect());
    let mut first_user: HashMap<(i32, i32), usize> = HashMap::new();
    for (ti, t) in mesh.tris.iter().enumerate() {
        for &vi in t {
            let k = key(mesh.verts[vi as usize].uv);
            match first_user.get(&k) {
                Some(&other) => dsu.union(ti, other),
                None => {
                    first_user.insert(k, ti);
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for ti in 0..n {
        let r = dsu.find(ti);
        groups.entry(r).or_default().push(ti);
    }
    let mut v: Vec<Island> = groups
        .into_values()
        .map(|tris| {
            let (mut u0, mut v0, mut u1, mut v1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for &ti in &tris {
                for &vi in &mesh.tris[ti] {
                    let uv = mesh.verts[vi as usize].uv;
                    u0 = u0.min(uv.x);
                    v0 = v0.min(uv.y);
                    u1 = u1.max(uv.x);
                    v1 = v1.max(uv.y);
                }
            }
            Island { tris, bounds: (u0, v0, u1, v1) }
        })
        .collect();
    v.sort_by_key(|i| i.tris[0]);
    v
}

/// Index of the island that has a triangle containing the UV point, if any.
pub fn island_at(mesh: &Mesh, islands: &[Island], u: f32, v: f32) -> Option<usize> {
    islands.iter().position(|isl| {
        isl.tris.iter().any(|&ti| {
            let p: Vec<_> = mesh.tris[ti].iter().map(|&i| mesh.verts[i as usize].uv).collect();
            let sign = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| (a.0 - c.0) * (b.1 - c.1) - (b.0 - c.0) * (a.1 - c.1);
            let (a, b, c) = ((p[0].x, p[0].y), (p[1].x, p[1].y), (p[2].x, p[2].y));
            let pt = (u, v);
            let (d1, d2, d3) = (sign(pt, a, b), sign(pt, b, c), sign(pt, c, a));
            let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            !(neg && pos)
        })
    })
}

/// Pixel mask of an island on a `w` x `h` canvas.
pub fn island_mask(mesh: &Mesh, island: &Island, w: u32, h: u32) -> Selection {
    let mut s = Selection::empty(w, h);
    for &ti in &island.tris {
        let p: Vec<(f32, f32)> = mesh.tris[ti].iter().map(|&i| {
            let uv = mesh.verts[i as usize].uv;
            (uv.x * w as f32, uv.y * h as f32)
        }).collect();
        s.add_triangle([p[0], p[1], p[2]]);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Vec2, Vec3};
    use crate::mesh::Vertex;

    fn quad(mesh: &mut Mesh, u: f32, v: f32, size: f32) {
        let base = mesh.verts.len() as u32;
        for (du, dv) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            mesh.verts.push(Vertex::new(Vec3::ZERO, Vec2 { x: u + du * size, y: v + dv * size }));
        }
        mesh.tris.push([base, base + 1, base + 2]);
        mesh.tris.push([base, base + 2, base + 3]);
    }

    fn two_quads() -> Mesh {
        let mut m = Mesh::new("t");
        quad(&mut m, 0.0, 0.0, 0.25);
        quad(&mut m, 0.5, 0.5, 0.25);
        m
    }

    #[test]
    fn two_separate_quads_are_two_islands() {
        let m = two_quads();
        let isl = islands(&m);
        assert_eq!(isl.len(), 2);
        assert_eq!(isl[0].tris, vec![0, 1]);
        assert_eq!(isl[1].tris, vec![2, 3]);
        assert_eq!(isl[1].bounds, (0.5, 0.5, 0.75, 0.75));
    }

    #[test]
    fn touching_triangles_merge_into_one_island() {
        let mut m = Mesh::new("t");
        quad(&mut m, 0.0, 0.0, 0.25);
        quad(&mut m, 0.25, 0.0, 0.25); // shares an edge in UV space
        assert_eq!(islands(&m).len(), 1);
    }

    #[test]
    fn island_lookup_and_mask() {
        let m = two_quads();
        let isl = islands(&m);
        assert_eq!(island_at(&m, &isl, 0.1, 0.1), Some(0));
        assert_eq!(island_at(&m, &isl, 0.6, 0.6), Some(1));
        assert_eq!(island_at(&m, &isl, 0.4, 0.4), None);
        let mask = island_mask(&m, &isl[1], 16, 16);
        assert!(mask.contains(10, 10) && !mask.contains(2, 2));
        let n = mask.count();
        assert!((14..=25).contains(&n), "a 0.25 x 0.25 quad on 16x16 covers about 16 pixels, got {n}");
    }

    #[test]
    fn an_empty_mesh_has_no_islands() {
        assert!(islands(&Mesh::new("t")).is_empty());
    }
}
