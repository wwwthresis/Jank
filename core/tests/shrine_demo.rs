//! The Night Shrine demo project (`demo/night-shrine.jank`) is complete: the shrine, the torii, stone and paper lanterns,
//! candles, a vending machine and pines, lit only by lamps (several attached to the objects that carry them) in the
//! "Night Glow" look; it renders warm light pools and survives a round trip.
use psxcore::compose::{render_states, scene_states, FrameSpec};
use psxcore::lights::LightKind;
use psxcore::project::Project;
use psxcore::raster::{Renderer, Tonemap};
use std::path::Path;

fn demo() -> Project {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../demo/night-shrine.jank");
    Project::load(&path).expect("demo/night-shrine.jank opens")
}

#[test]
fn shrine_scene_has_its_buildings_and_lamps() {
    let p = demo();
    let names: Vec<&str> = p.scene.objects.iter().map(|o| o.name.as_str()).collect();
    for n in ["Shrine", "Torii", "Stone lantern L1", "Paper lantern R", "Vending machine", "Stone path", "Ground"] {
        assert!(names.contains(&n), "{n} is in the scene: {names:?}");
    }
    let l = &p.scene.lights;
    assert!(l.len() >= 20, "lamps: {}", l.len());
    let attached = l.iter().filter(|x| x.attach.is_some()).count();
    assert!(attached >= 10, "lanterns and candles carry their lamps: {attached}");
    for x in l.iter().filter_map(|x| x.attach) {
        assert!(p.scene.object(x).is_some(), "every attached lamp has its object");
    }
    assert!(l.iter().any(|x| x.kind == LightKind::Spot), "the moonbeam");
    assert!(l.iter().any(|x| x.flicker > 0.2), "candles flicker");
    let s = &p.scene.style;
    assert!(s.pixel_lights && s.tonemap == Tonemap::Filmic && s.bloom > 0.0 && s.ao > 0.0 && s.halo > 0.0, "Night Glow look");
    assert_eq!(p.scene.layers_back, 1, "the starry sky behind the scene");
    // the lanterns share one model (duplicates of one asset)
    let toro: Vec<usize> = p.scene.objects.iter().filter(|o| o.name.starts_with("Stone lantern")).map(|o| o.asset).collect();
    assert_eq!(toro.len(), 4);
    assert!(toro.iter().all(|&a| a == toro[0]));
}

#[test]
fn shrine_scene_renders_warm_lamp_light_and_round_trips() {
    let p = demo();
    let states = scene_states(&p);
    let mut r = Renderer::new(1, 1);
    let spec = FrameSpec { states: &states, camera: p.scene.camera, style: &p.scene.style, hand_layer: None, hand_only: false, scene_layers: true, time: 0.0 };
    render_states(&p, &spec, &mut r);
    let px = &r.frame.color;
    assert_eq!((r.frame.w, r.frame.h), (480, 270));
    let warm = px.iter().filter(|c| c[0] as i32 > c[2] as i32 + 60 && c[0] > 150).count();
    let dark = px.iter().filter(|c| (c[0] as u32 + c[1] as u32 + c[2] as u32) < 90).count();
    assert!(warm > 1500, "lantern light: {warm} warm pixels");
    assert!(dark > px.len() / 4, "it is night: {dark} dark pixels");
    let bytes = p.to_bytes();
    let back = Project::from_bytes(&bytes).expect("reads back");
    assert_eq!(back.scene.lights, p.scene.lights);
    assert_eq!(back.scene.style, p.scene.style);
    assert_eq!(back.scene.objects.len(), p.scene.objects.len());
}
