//! Writes raw RGBA renders of a few scenes: `cargo run --release --example dump_render <dir>`
//! Used to compare two rasterizer versions (see `cmp_render.py` in the notes of the performance work).
use psxcore::{compose::*, math::*, modeling::*, project::*, raster::*};
fn main() {
    let dir = std::env::args().nth(1).expect("output dir");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, seg, rings, style) in [("ps1", 12usize, 8usize, PsxStyle::ps1_classic()), ("clean", 24, 16, PsxStyle::clean_lowpoly()), ("fog", 32, 20, PsxStyle::horror_fog())] {
        let mut p = Project::new("d");
        let mut m = sphere(1.0, seg, rings);
        psxcore::decimate::box_unwrap(&mut m);
        let mut model = model_from_mesh("ball", m, 32);
        for y in 0..32 { for x in 0..32 { if (x / 4 + y / 4) % 2 == 0 { model.textures[0].set(x, y, [200, 80, 60, 255]); } else { model.textures[0].set(x, y, [60, 90, 200, 255]); } } }
        p.place_model(model);
        let mut g = plane(8.0, 8.0, 4, 4);
        psxcore::decimate::box_unwrap(&mut g);
        let gm = model_from_mesh("g", g, 16);
        let gid = p.place_model(gm);
        p.scene.object_mut(gid).unwrap().transform.pos = Vec3::new(0.0, -1.0, 0.0);
        p.scene.camera = Camera { eye: Vec3::new(2.8, 1.8, 3.4), target: Vec3::ZERO, ..Default::default() };
        for (w, h) in [(320u32, 240u32), (641, 359)] {
            p.scene.style = PsxStyle { width: w, height: h, ..style.clone() };
            let states = scene_states(&p);
            let mut r = Renderer::new(w, h);
            render_states(&p, &FrameSpec { states: &states, camera: p.scene.camera, style: &p.scene.style, hand_layer: None, hand_only: false, scene_layers: true, time: 0.0 }, &mut r);
            let bytes: Vec<u8> = r.frame.color.iter().flat_map(|c| *c).collect();
            std::fs::write(format!("{dir}/{name}_{w}x{h}.rgba"), bytes).unwrap();
        }
    }
}
