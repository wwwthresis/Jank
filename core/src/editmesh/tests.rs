use super::*;
use crate::modeling;

fn cube() -> EditMesh {
    EditMesh::from_meshes(&[modeling::cube(Vec3::ONE)])
}

/// Every index valid, polygons have distinct corners, and each directed edge is used once.
/// With `closed`, every edge has exactly two polygons with opposite directions.
fn check(em: &EditMesh, closed: bool) {
    let mut directed = std::collections::HashSet::new();
    for (fi, f) in em.faces.iter().enumerate() {
        assert!(f.c.len() >= 3, "face {fi} too small");
        let mut vs: Vec<u32> = f.verts().collect();
        vs.sort_unstable();
        vs.dedup();
        assert_eq!(vs.len(), f.c.len(), "face {fi} repeats a vertex");
        for v in f.verts() {
            assert!((v as usize) < em.verts.len());
        }
        for e in f.edges() {
            assert!(directed.insert(e), "directed edge {e:?} used twice (face {fi})");
        }
    }
    if closed {
        for &(a, b) in &directed {
            assert!(directed.contains(&(b, a)), "edge ({a},{b}) is open");
        }
    }
    for s in [&em.sel_v] {
        assert!(s.iter().all(|&v| (v as usize) < em.verts.len()));
    }
    assert!(em.sel_f.iter().all(|&f| (f as usize) < em.faces.len()));
}

fn volume(em: &EditMesh) -> f32 {
    let (ms, _) = em.to_meshes();
    ms.iter().map(modeling::signed_volume).sum()
}

fn select_faces(em: &mut EditMesh, faces: &[u32]) {
    em.set_mode(SelectMode::Face);
    em.select_all(false);
    for &f in faces {
        em.click_face(f, true);
    }
}

fn top_face(em: &EditMesh) -> u32 {
    (0..em.faces.len() as u32).max_by(|&a, &b| em.face_center(a).y.partial_cmp(&em.face_center(b).y).unwrap()).unwrap()
}

#[test]
fn cube_round_trip_makes_quads() {
    let em = cube();
    assert_eq!(em.verts.len(), 8);
    assert_eq!(em.face_stats(), (0, 6, 0));
    check(&em, true);
    // the 3x2 atlas splits UVs on the edges between the cells
    assert!(!em.seams.is_empty());
    let (ms, maps) = em.to_meshes();
    assert_eq!(ms.len(), 1);
    assert_eq!(ms[0].tris.len(), 12);
    assert_eq!(maps[0].len(), 12);
    assert!((volume(&em) - 1.0).abs() < 1e-4);
    let back = EditMesh::from_meshes(&ms);
    assert_eq!(back.face_stats(), (0, 6, 0));
}

#[test]
fn extrude_top_face() {
    let mut em = cube();
    let t = top_face(&em);
    select_faces(&mut em, &[t]);
    let n = em.extrude();
    assert!(n.y > 0.9);
    for v in em.selected_verts() {
        em.verts[v as usize].pos.y += 1.0;
    }
    check(&em, true);
    assert_eq!(em.faces.len(), 10);
    assert!((volume(&em) - 2.0).abs() < 1e-4);
}

#[test]
fn inset_and_individual() {
    let mut em = cube();
    let t = top_face(&em);
    select_faces(&mut em, &[t]);
    assert!(em.inset(0.1, 0.0, false));
    check(&em, true);
    assert_eq!(em.faces.len(), 10);
    assert!((volume(&em) - 1.0).abs() < 1e-4);
    let mut em = cube();
    em.select_all(true);
    assert!(em.inset(0.1, -0.1, true));
    check(&em, true);
    assert_eq!(em.faces.len(), 30);
}

#[test]
fn loop_cut_cube() {
    let mut em = cube();
    let e = em.all_edges()[0];
    assert!(em.loop_cut(e, 2, 0.0));
    check(&em, true);
    assert_eq!(em.faces.len(), 6 + 4 * 2);
    assert_eq!(em.verts.len(), 8 + 8);
    assert!((volume(&em) - 1.0).abs() < 1e-4);
}

#[test]
fn subdivide_cube() {
    let mut em = cube();
    em.select_all(true);
    assert!(em.subdivide());
    check(&em, true);
    assert_eq!(em.faces.len(), 24);
}

#[test]
fn bevel_one_edge() {
    for segs in [1, 3] {
        let mut em = cube();
        em.set_mode(SelectMode::Edge);
        let e = em.all_edges()[0];
        em.click_edge(e, false);
        assert!(em.bevel(BevelOptions { width: 0.1, segments: segs, clamp: true }));
        check(&em, true);
        assert_eq!(em.faces.len(), 6 + segs, "segments {segs}");
        assert!(volume(&em) < 1.0 && volume(&em) > 0.98);
    }
}

#[test]
fn bevel_all_edges() {
    for segs in [1, 2, 4] {
        let mut em = cube();
        em.set_mode(SelectMode::Edge);
        em.select_all(true);
        assert!(em.bevel(BevelOptions { width: 0.1, segments: segs, clamp: true }));
        check(&em, true);
        // 6 faces + 12 strips + 8 corner caps
        assert_eq!(em.faces.len(), 6 + 12 * segs + 8, "segments {segs}");
        let v = volume(&em);
        assert!(v < 1.0 && v > 0.9, "volume {v}");
    }
}

#[test]
fn bevel_edge_loop() {
    // the top loop: vertices with two bevelled edges share their arcs
    let mut em = cube();
    let t = top_face(&em);
    let edges: Vec<Edge> = em.faces[t as usize].edges().map(|(a, b)| ek(a, b)).collect();
    em.set_mode(SelectMode::Edge);
    em.select_all(false);
    for e in edges {
        em.click_edge(e, true);
    }
    assert!(em.bevel(BevelOptions { width: 0.2, segments: 3, clamp: true }));
    check(&em, true);
    assert_eq!(em.faces.len(), 6 + 4 * 3);
}

#[test]
fn bevel_vertex() {
    let mut em = cube();
    em.set_mode(SelectMode::Vertex);
    em.click_vert(0, false);
    assert!(em.bevel_verts(0.2));
    check(&em, true);
    assert_eq!(em.faces.len(), 7);
    assert_eq!(em.verts.len(), 10);
}

#[test]
fn merge_and_dissolve() {
    let mut em = cube();
    let t = top_face(&em);
    let verts: Vec<u32> = em.faces[t as usize].verts().collect();
    em.set_mode(SelectMode::Vertex);
    em.select_all(false);
    for v in &verts {
        em.click_vert(*v, true);
    }
    assert!(em.merge(MergeKind::Center) > 0);
    check(&em, true);
    assert_eq!(em.verts.len(), 5); // a pyramid
    assert_eq!(em.faces.len(), 5);
    let mut em = cube();
    let e = em.all_edges()[0];
    em.set_mode(SelectMode::Edge);
    em.click_edge(e, false);
    assert_eq!(em.dissolve_edges(), 1);
    check(&em, true);
    assert_eq!(em.faces.len(), 5);
}

#[test]
fn delete_and_fill() {
    let mut em = cube();
    let t = top_face(&em);
    select_faces(&mut em, &[t]);
    assert!(em.delete(DeleteKind::Faces));
    check(&em, false);
    assert_eq!(em.faces.len(), 5);
    // select the hole's border and fill it
    let topo = em.topo();
    let border: Vec<Edge> = topo.edge_faces.iter().filter(|(_, f)| f.len() == 1).map(|(e, _)| *e).collect();
    assert_eq!(border.len(), 4);
    em.set_mode(SelectMode::Edge);
    em.select_all(false);
    for e in border {
        em.click_edge(e, true);
    }
    assert!(em.fill());
    check(&em, true);
    assert!((volume(&em) - 1.0).abs() < 1e-4);
}

#[test]
fn bridge_two_loops() {
    // two separate quads facing each other become a tube
    let mut em = cube();
    let faces: Vec<u32> = (0..6).filter(|&f| em.face_normal(f).x.abs() > 0.9).collect();
    select_faces(&mut em, &faces);
    let other: Vec<u32> = (0..6).filter(|f| !faces.contains(f)).collect();
    select_faces(&mut em, &other);
    assert!(em.delete(DeleteKind::Faces));
    em.select_all(true);
    assert!(em.bridge());
    check(&em, false);
    assert_eq!(em.faces.len(), 4);
}

#[test]
fn knife_plane_cut() {
    let mut em = cube();
    let all: BTreeSet<u32> = (0..6).collect();
    let n = em.cut_plane(&all, Vec3::ZERO, Vec3::Y, &|_| true);
    assert_eq!(n, 4);
    check(&em, true);
    assert_eq!(em.faces.len(), 10);
    assert!((volume(&em) - 1.0).abs() < 1e-4);
}

#[test]
fn symmetrize_x() {
    let mut em = cube();
    for v in &mut em.verts {
        if v.pos.x > 0.0 {
            v.pos.x = 1.5;
        }
    }
    assert!(em.symmetrize(0, true));
    check(&em, true);
    for v in &em.verts {
        assert!((v.pos.x.abs() - 1.5).abs() < 1e-4 || v.pos.x.abs() < 1e-4, "x = {}", v.pos.x);
    }
}

#[test]
fn loops_and_rings() {
    let mut em = cube();
    let e = em.all_edges()[0];
    assert!(em.loop_cut(e, 1, 0.0));
    let topo = em.topo();
    // a new loop edge: both ends are new vertices (index >= 8)
    let le = em.all_edges().into_iter().find(|e| e.0 >= 8 && e.1 >= 8).unwrap();
    assert_eq!(em.edge_loop(&topo, le).len(), 4);
    assert_eq!(em.edge_ring(&topo, le).len() >= 2, true);
    em.select_loop(le, false, false);
    assert_eq!(em.sel_e.len(), 4);
}

#[test]
fn unwrap_produces_islands_in_unit_square() {
    let mut em = cube();
    let all: Vec<u32> = (0..em.faces.len() as u32).collect();
    let n = em.uv_unwrap(&all, 0.02);
    assert!(n >= 1);
    for f in &em.faces {
        for c in &f.c {
            assert!(c.uv.x >= -1e-4 && c.uv.x <= 1.0001 && c.uv.y >= -1e-4 && c.uv.y <= 1.0001, "{:?}", c.uv);
        }
    }
    // a flat grid unwraps without distortion: edge length ratios are kept
    let mut em = EditMesh::from_meshes(&[modeling::plane(2.0, 1.0, 4, 2)]);
    em.seams.clear();
    let all: Vec<u32> = (0..em.faces.len() as u32).collect();
    assert_eq!(em.uv_unwrap(&all, 0.0), 1);
    let f = &em.faces[0];
    let (a, b, c) = (f.c[0], f.c[1], f.c[2]);
    let d = |p: Vec2, q: Vec2| ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt();
    let r3 = (em.pos(a.v) - em.pos(b.v)).length() / (em.pos(b.v) - em.pos(c.v)).length();
    let r2 = d(a.uv, b.uv) / d(b.uv, c.uv);
    assert!((r3 - r2).abs() < 1e-3, "{r3} vs {r2}");
}

#[test]
fn separate_and_duplicate() {
    let mut em = cube();
    let t = top_face(&em);
    select_faces(&mut em, &[t]);
    assert!(em.duplicate());
    check(&em, false);
    assert_eq!(em.faces.len(), 7);
    let part = em.separate().unwrap();
    assert_eq!(part.faces.len(), 1);
    assert_eq!(em.faces.len(), 6);
    check(&em, true);
}

#[test]
fn grow_shrink_and_path() {
    let mut em = cube();
    em.set_mode(SelectMode::Vertex);
    em.click_vert(0, false);
    em.grow();
    assert_eq!(em.sel_v.len(), 4);
    em.shrink();
    assert_eq!(em.sel_v.len(), 1);
    let topo = em.topo();
    let far = (0..8u32).max_by(|&a, &b| (em.pos(a) - em.pos(0)).length().partial_cmp(&(em.pos(b) - em.pos(0)).length()).unwrap()).unwrap();
    assert_eq!(em.shortest_path(&topo, 0, far).len(), 4);
}

#[test]
fn triangulate_and_quads_back() {
    let mut em = cube();
    em.select_all(true);
    assert_eq!(em.triangulate_faces(), 6);
    assert_eq!(em.faces.len(), 12);
    let all: Vec<u32> = (0..12).collect();
    assert_eq!(em.tris_to_quads(&all, 5.0), 6);
    check(&em, true);
}

#[test]
fn recalc_normals_fixes_flipped_face() {
    let mut em = cube();
    em.flip_face(0);
    em.select_all(true);
    assert!(em.recalc_normals(false) >= 1);
    check(&em, true);
    assert!(volume(&em) > 0.0);
}

fn moved(mut em: EditMesh, d: Vec3, s: f32) -> EditMesh {
    for v in &mut em.verts {
        v.pos = v.pos * s + d;
    }
    em
}

#[test]
fn boolean_difference_union_intersect() {
    // a cube minus a smaller cube poking out of its top
    let cutter = moved(cube(), Vec3::new(0.0, 0.5, 0.0), 0.5);
    let mut em = cube();
    assert!(em.boolean(&cutter, BoolOp::Difference, 0));
    check(&em, true);
    let v = volume(&em);
    assert!((v - (1.0 - 0.125 * 0.5)).abs() < 1e-3, "difference volume {v}");
    // the flat sides merge back: the outer faces are still few
    assert!(em.faces.len() <= 20, "{} faces", em.faces.len());

    let mut em = cube();
    assert!(em.boolean(&cutter, BoolOp::Union, 0));
    check(&em, true);
    let v = volume(&em);
    assert!((v - (1.0 + 0.125 * 0.5)).abs() < 1e-3, "union volume {v}");

    let mut em = cube();
    assert!(em.boolean(&cutter, BoolOp::Intersect, 0));
    check(&em, true);
    let v = volume(&em);
    assert!((v - 0.125 * 0.5).abs() < 1e-3, "intersect volume {v}");

    // a hole straight through
    let tube = moved(EditMesh::from_meshes(&[modeling::cube(Vec3::new(0.4, 2.0, 0.4))]), Vec3::ZERO, 1.0);
    let mut em = cube();
    assert!(em.boolean(&tube, BoolOp::Difference, 0));
    check(&em, true);
    let v = volume(&em);
    assert!((v - (1.0 - 0.16)).abs() < 1e-3, "hole volume {v}");
}
