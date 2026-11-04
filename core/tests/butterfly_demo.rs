//! The shipped demo project (`demo/butterfly.jank`) opens as a complete scene and survives a save/reopen round trip.
use psxcore::lighting::ShadowMode;
use psxcore::project::Project;
use std::path::Path;

fn demo() -> Project {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../demo/butterfly.jank");
    Project::load(&path).expect("demo/butterfly.jank opens")
}

#[test]
fn demo_has_rig_pose_layers_lighting_and_textures() {
    let p = demo();
    let bf = p.assets.iter().position(|a| a.model.name != "Meadow" && a.model.skeleton.is_some()).expect("the rigged butterfly asset");
    let skel = p.assets[bf].model.skeleton.as_ref().unwrap();
    let names: Vec<&str> = skel.bones.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Body", "Wing_R", "Wing_L", "Wing_R_Tip", "Wing_L_Tip"], "bones are kept as imported");
    assert_eq!(p.assets[bf].model.triangle_count(), 136);
    assert_eq!(p.assets[bf].model.textures[0].w, 128, "the butterfly texture is inside the project");
    // the saved pose: wings are rotated away from the rest pose, mirrored left and right
    let anim = &p.animations[0];
    assert_eq!(anim.frames.len(), 4);
    let obj = p.scene.objects.iter().find(|o| o.asset == bf).unwrap().id;
    let st = anim.frames[0].states.iter().find(|s| s.object == obj).unwrap();
    let (r, l) = (st.pose.bones[1].rot, st.pose.bones[2].rot);
    assert!(r.z.abs() > 0.05 && (r.z + l.z).abs() < 1e-4, "right and left wing are posed symmetrically: {r:?} {l:?}");
    // frames differ (the animation really flaps)
    let a = anim.frames[0].states.iter().find(|s| s.object == obj).unwrap().pose.bones[1].rot;
    let b = anim.frames[2].states.iter().find(|s| s.object == obj).unwrap().pose.bones[1].rot;
    assert!((a.z - b.z).abs() > 0.1);
    // environment, layers, lighting, texture layers
    assert!(p.scene.objects.len() >= 10, "ground, trees and the butterfly");
    assert_eq!(p.scene.layers.len(), 2);
    assert_eq!(p.scene.layers_back, 1, "one picture behind the scene, one in front");
    assert!(p.scene.lighting.tod_on && p.scene.lighting.shadows == ShadowMode::Projected);
    assert!(p.scene.style.z_buffer);
    assert!(p.assets[bf].stack(0).is_some_and(|s| s.layers.len() == 2), "the butterfly texture has layers");
}

#[test]
fn demo_roundtrips_and_renders_the_same_after_reopening() {
    let p = demo();
    let bytes = p.to_bytes();
    let q = Project::from_bytes(&bytes).unwrap();
    assert_eq!(p, q);
    assert_eq!(bytes, q.to_bytes());
    // render the first frame from the file and from the reopened copy
    let draw = |p: &Project| {
        let anim = &p.animations[0];
        let mut r = psxcore::raster::Renderer::new(1, 1);
        assert!(psxcore::compose::render_frame_index(p, anim, 0, &mut r));
        r.frame.color.clone()
    };
    let (a, b) = (draw(&p), draw(&q));
    assert_eq!(a, b);
    assert!(a.iter().any(|c| c[2] > c[0] as u8 + 40), "the picture is not empty");
}
