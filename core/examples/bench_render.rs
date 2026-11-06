//! Rendering benchmark: `cargo run --release --example bench_render`
//! Prints the average time of one viewport render for several triangle counts and resolutions.
use psxcore::{compose::*, math::*, modeling::*, project::*, raster::*};
use std::time::Instant;

fn scene(seg: usize, rings: usize) -> (Project, usize) {
    let mut p = Project::new("bench");
    let mut m = sphere(1.0, seg, rings);
    psxcore::decimate::box_unwrap(&mut m);
    let tris = m.tris.len();
    let model = model_from_mesh("ball", m, 64);
    p.place_model(model);
    p.scene.camera = Camera { eye: Vec3::new(2.5, 1.6, 3.2), target: Vec3::ZERO, ..Default::default() };
    (p, tris)
}

fn main() {
    println!("{:>9} {:>10} {:>10} {:>10}", "tris", "320x240", "960x540", "1280x720");
    for (seg, rings) in [(8, 6), (32, 24), (96, 64), (256, 160)] {
        let (mut p, tris) = scene(seg, rings);
        let states = scene_states(&p);
        let mut row = vec![];
        for (w, h) in [(320u32, 240u32), (960, 540), (1280, 720)] {
            p.scene.style = PsxStyle { width: w, height: h, ..PsxStyle::ps1_classic() };
            let mut r = Renderer::new(w, h);
            let spec = FrameSpec { states: &states, camera: p.scene.camera, style: &p.scene.style, hand_layer: None, hand_only: false, scene_layers: true, time: 0.0 };
            render_states(&p, &spec, &mut r);
            let n = if tris > 20000 { 3 } else { 10 };
            let t = Instant::now();
            for _ in 0..n {
                render_states(&p, &spec, &mut r);
            }
            row.push(t.elapsed().as_secs_f64() * 1000.0 / n as f64);
        }
        println!("{:>9} {:>8.1}ms {:>8.1}ms {:>8.1}ms", tris, row[0], row[1], row[2]);
    }
}
