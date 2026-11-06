//! Shadow benchmark: `cargo run --release --example bench_shadows [project.jank]`
//! Average time of one viewport render of the demo scene for every shadow mode and quality.
//! "static" renders the same scene again (the usual case while painting or orbiting the camera);
//! "moving" changes an object every frame, so the shadow map has to be rebuilt each time.
use psxcore::editor::Editor;
use psxcore::lighting::{ShadowMode, ShadowQuality};
use psxcore::raster::Renderer;
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "../demo/butterfly.jank".to_string());
    let mut ed = Editor::open(std::path::Path::new(&path)).expect("open demo project");
    let (w, h) = (1280u32, 720u32);
    let mut r = Renderer::new(w, h);
    println!("{:<18} {:>10} {:>10}   {}", "shadows", "static", "moving", "picture checksum");
    let mut cases = vec![(ShadowMode::Off, ShadowQuality::Low), (ShadowMode::Blob, ShadowQuality::Low)];
    cases.extend(ShadowQuality::ALL.into_iter().map(|q| (ShadowMode::Projected, q)));
    for (mode, q) in cases {
        let mut l = ed.project.scene.lighting.clone();
        l.shadows = mode;
        l.quality = q;
        l.softness = 0.6;
        l.shadow_strength = 0.5;
        ed.project.scene.lighting = l;
        ed.render_viewport_sized(&mut r, w, h);
        let n = 12;
        let t = Instant::now();
        for _ in 0..n {
            ed.render_viewport_sized(&mut r, w, h);
        }
        let still = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        let t = Instant::now();
        for i in 0..n {
            if let Some(o) = ed.project.scene.objects.last_mut() {
                o.transform.rot_deg.y += 1.0 + i as f32 * 0.01;
            }
            ed.render_viewport_sized(&mut r, w, h);
        }
        let moving = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        // checksum of the last picture: optimisations must not change a single pixel
        let mut sum: u64 = 0xcbf29ce484222325;
        for px in r.frame.color.iter() {
            for c in px {
                sum = (sum ^ *c as u64).wrapping_mul(0x100000001b3);
            }
        }
        let name = if mode == ShadowMode::Projected { format!("Cast {}", q.label()) } else { mode.label().to_string() };
        println!("{:<18} {:>8.1}ms {:>8.1}ms   {:016x}", name, still, moving, sum);
    }
}
