//! The bunny demo project (`demo/bunnies.jank`) is complete: two rigged plush bunnies with photo
//! textures, the rug and the floor, lamp light with cast shadows, a glow layer and an idle loop; it survives a round trip.
use psxcore::lighting::ShadowMode;
use psxcore::project::Project;
use std::path::Path;

fn demo() -> Project {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../demo/bunnies.jank");
    Project::load(&path).expect("demo/bunnies.jank opens")
}

#[test]
fn bunny_scene_has_two_rigged_bunnies() {
    let p = demo();
    let names: Vec<&str> = p.scene.objects.iter().map(|o| o.name.as_str()).collect();
    for n in ["Floor", "Rug", "Cream bunny", "Two-headed bunny"] {
        assert!(names.contains(&n), "{n} is in the scene: {names:?}");
    }
    let bunny = |name: &str| {
        let o = p.scene.objects.iter().find(|o| o.name == name).unwrap();
        &p.assets[o.asset].model
    };
    let white = bunny("Two-headed bunny");
    let cream = bunny("Cream bunny");
    for (m, bones) in [(white, 11), (cream, 8)] {
        let skel = m.skeleton.as_ref().expect("the bunny has bones");
        assert_eq!(skel.bones.len(), bones);
        assert!(m.triangle_count() > 1000 && m.triangle_count() < 2500, "low-poly: {}", m.triangle_count());
        let t = &m.textures[0];
        assert_eq!((t.w, t.h), (768, 384), "front and back of the photo texture");
        for v in &m.meshes[0].verts {
            let s: f32 = v.weights.iter().sum();
            assert!((s - 1.0).abs() < 1e-3, "skin weights add up to one");
            assert!(v.joints.iter().zip(v.weights).all(|(j, w)| w == 0.0 || (*j as usize) < bones));
            assert!((0.0..=1.0).contains(&v.uv.x) && (0.0..=1.0).contains(&v.uv.y));
        }
    }
    let white_names: Vec<&str> = white.skeleton.as_ref().unwrap().bones.iter().map(|b| b.name.as_str()).collect();
    assert!(white_names.contains(&"Head_L") && white_names.contains(&"Head_R"), "two heads: {white_names:?}");
    assert!(p.scene.lighting.shadows == ShadowMode::Projected && p.scene.style.z_buffer);
    assert_eq!(p.scene.layers.len(), 1, "the lamp glow in front");
}

#[test]
fn bunny_scene_surface_is_closed() {
    // front and back halves meet at the rim: every edge (by position) belongs to exactly two triangles
    let p = demo();
    for o in p.scene.objects.iter().filter(|o| o.name.contains("bunny")) {
        let m = &p.assets[o.asset].model.meshes[0];
        let key = |i: u32| {
            let v = m.verts[i as usize].pos;
            ((v.x * 1e4).round() as i64, (v.y * 1e4).round() as i64, (v.z * 1e4).round() as i64)
        };
        let mut edges = std::collections::HashMap::new();
        for t in &m.tris {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (ka, kb) = (key(a), key(b));
                *edges.entry(if ka < kb { (ka, kb) } else { (kb, ka) }).or_insert(0) += 1;
            }
        }
        let open = edges.values().filter(|n| **n != 2).count();
        assert_eq!(open, 0, "{}: {open} open edges", o.name);
    }
}

#[test]
fn bunny_scene_animates_and_roundtrips() {
    let p = demo();
    let anim = &p.animations[0];
    assert_eq!(anim.name, "Idle");
    assert_eq!(anim.frames.len(), 5);
    assert!(anim.frames[..4].iter().all(|f| f.tween) && !anim.frames[4].tween, "a tweened loop");
    // the last key repeats the first, so the loop has no jump
    for (a, b) in anim.frames[0].states.iter().zip(&anim.frames[4].states) {
        assert_eq!(a.pose, b.pose);
    }
    let bytes = p.to_bytes();
    let q = Project::from_bytes(&bytes).unwrap();
    assert_eq!(p, q);
    let draw = |p: &Project, f: usize| {
        let mut r = psxcore::raster::Renderer::new(1, 1);
        assert!(psxcore::compose::render_frame_index(p, &p.animations[0], f, &mut r));
        r.frame.color.clone()
    };
    let (a, b) = (draw(&p, 0), draw(&q, 0));
    assert_eq!(a, b);
    assert_ne!(a, draw(&p, 1), "the bunnies move between keys");
    // pink rug and light fleece are both in the picture
    assert!(a.iter().any(|c| c[0] as i32 > c[1] as i32 + 40));
    assert!(a.iter().any(|c| c[0] > 200 && c[1] > 190 && c[2] > 170));
}
