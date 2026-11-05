//! Benchmark of the modern effects: `cargo run --release --example bench_effects -- [threads] [project.jank]`
//! Renders a test scene with four lanterns (or the given project, e.g. `../demo/night-shrine.jank`) at 1280x720 and prints the
//! average time of one frame for each effect on its own and for the full "Night Glow" look.
use psxcore::{compose::*, lights::*, math::*, modeling::*, project::*, raster::*};

fn test_scene() -> Project {
    let mut p = Project::new("Effects bench");
    let mut m = sphere(0.6, 12, 8);
    psxcore::decimate::box_unwrap(&mut m);
    let b = p.place_model(model_from_mesh("Ball", m, 16));
    p.scene.object_mut(b).unwrap().transform.pos = Vec3::new(0.8, -0.4, 0.0);
    let c = p.place_model(model_from_mesh("Box", cube(Vec3::new(0.8, 1.6, 0.8)), 16));
    p.scene.object_mut(c).unwrap().transform.pos = Vec3::new(-1.0, -0.2, -0.6);
    let g = p.place_model(model_from_mesh("Ground", plane(10.0, 10.0, 1, 1), 16));
    p.scene.object_mut(g).unwrap().transform.pos = Vec3::new(0.0, -1.0, 0.0);
    p.scene.camera = Camera { eye: Vec3::new(2.8, 1.8, 3.4), target: Vec3::ZERO, ..Default::default() };
    for i in 0..4 {
        let l = p.scene.add_light(LightPreset::Lantern);
        p.scene.light_mut(l).unwrap().pos = Vec3::new(i as f32 - 1.5, -0.3, 0.9);
    }
    p
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let threads: usize = args.get(1).map_or(default_threads(), |v| v.parse().expect("number of threads"));
    let mut p = match args.get(2) {
        Some(path) => Project::load(std::path::Path::new(path)).expect("a Jank! project"),
        None => test_scene(),
    };
    let camera = p.animations.first().and_then(|a| a.frames.first()).map_or(p.scene.camera, |f| f.camera);
    let base = PsxStyle { width: 1280, height: 720, ..PsxStyle::ps1_classic() };
    for (name, st) in [
        ("plain", base.clone()),
        ("pixel", PsxStyle { pixel_lights: true, ..base.clone() }),
        ("tone", PsxStyle { tonemap: Tonemap::Filmic, ..base.clone() }),
        ("ao", PsxStyle { ao: 0.6, ..base.clone() }),
        ("bloom", PsxStyle { bloom: 0.5, ..base.clone() }),
        ("crt", PsxStyle { crt_curve: 0.5, crt_mask: 0.4, chroma: 0.3, ..base.clone() }),
        ("all", PsxStyle { width: 1280, height: 720, ..PsxStyle::night_glow() }),
    ] {
        p.scene.style = st;
        let states = scene_states(&p);
        let mut r = Renderer::new(1, 1);
        r.threads = threads;
        let spec = FrameSpec { states: &states, camera, style: &p.scene.style, hand_layer: None, hand_only: false, scene_layers: true, time: 0.0 };
        render_states(&p, &spec, &mut r);
        let t0 = std::time::Instant::now();
        for _ in 0..10 {
            render_states(&p, &spec, &mut r);
        }
        println!("{name:6} {:.2} ms", t0.elapsed().as_secs_f64() * 100.0);
    }
}
