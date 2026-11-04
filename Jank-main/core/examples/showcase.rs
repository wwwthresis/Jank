use psxcore::{anim::*, math::*, modeling::*, project::*, raster::*, texture::*, export::*, encode::{avi::AviCodec, sinks::*}};
fn painted(name: &str, base: Rgba, accent: Rgba) -> Texture {
    let mut t = Texture::new(name, 32, 32, base);
    for y in 0..32 { for x in 0..32 { if (x + y) % 8 < 2 { t.set(x, y, accent); } } }
    t.rect(0, 0, 31, 31, [20, 20, 30, 255], false);
    t.dither_quantize(5);
    t
}
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let mut p = Project::new("showcase");
    // a "character" built from primitives: cylinder body + sphere head + cone hat
    let mut body = cylinder(0.5, 1.2, 8); flat_shade(&mut body);
    let mut head = sphere(0.45, 8, 6); let hi: Vec<usize> = (0..head.verts.len()).collect(); translate_vertices(&mut head, &hi, Vec3::new(0.0, 1.65, 0.0)); flat_shade(&mut head);
    let mut hat = cone(0.5, 0.7, 6); let ci: Vec<usize> = (0..hat.verts.len()).collect(); translate_vertices(&mut hat, &ci, Vec3::new(0.0, 1.95, 0.0)); flat_shade(&mut hat);
    append(&mut body, &head, Vec3::ZERO); append(&mut body, &hat, Vec3::ZERO);
    psxcore::decimate::box_unwrap(&mut body);
    let mut hero = model_from_mesh("Hero", body, 32);
    hero.textures[0] = painted("skin", [190, 120, 90, 255], [220, 160, 120, 255]);
    let hero_id = p.place_model(hero);
    // ground and tree
    let mut ground = plane(14.0, 14.0, 7, 7); flat_shade(&mut ground); psxcore::decimate::box_unwrap(&mut ground);
    let mut g = model_from_mesh("Ground", ground, 32); g.textures[0] = painted("grass", [70, 130, 60, 255], [90, 160, 70, 255]);
    let gid = p.place_model(g);
    let mut tree = cone(0.9, 2.4, 6); flat_shade(&mut tree); psxcore::decimate::box_unwrap(&mut tree);
    let mut t = model_from_mesh("Tree", tree, 32); t.textures[0] = painted("leaf", [40, 100, 60, 255], [60, 130, 80, 255]);
    let tid = p.place_model(t);
    p.scene.object_mut(tid).unwrap().transform.pos = Vec3::new(-2.5, 0.0, -2.0);
    let _ = (gid, hero_id);
    p.scene.camera = Camera { eye: Vec3::new(3.2, 2.2, 5.0), target: Vec3::new(-0.3, 1.0, 0.0), fov_y: 0.85, ..Default::default() };
    let mut anim = Animation::from_scene("idle", &p.scene, &|a| p.bones_of(a));
    for (name, style) in [("ps1", PsxStyle::ps1_classic()), ("lofi", PsxStyle::lofi_calm()), ("clean", PsxStyle::clean_lowpoly())] {
        p.scene.style = style;
        let a = anim.clone();
        let png = render_photo(&p, &a, 0, 960, 720, ImageFormat::Png).unwrap();
        std::fs::write(format!("{dir}/shot_{name}.png"), png).unwrap();
    }
    // video: the hero turns around while the camera orbits
    p.scene.style = PsxStyle::lofi_calm();
    anim.frames[0].tween = true; anim.frames[0].hold = 12;
    anim.duplicate_frame(0);
    anim.frames[1].states[0].transform.rot_deg = Vec3::new(0.0, 160.0, 0.0);
    anim.frames[1].camera.eye = Vec3::new(-3.0, 2.0, 5.0);
    anim.frames[1].tween = false;
    let mut sink: Box<dyn VideoSink> = Box::new(AviSink::create(std::path::Path::new(&format!("{dir}/demo.avi")), 640, 480, 12, 1, AviCodec::MjpegQuality(90)).unwrap());
    let n = bake_video(&p, &anim, 640, 480, sink.as_mut(), &mut |_, _| true).unwrap();
    sink.finish().unwrap();
    p.animations.push(anim);
    p.save(std::path::Path::new(&format!("{dir}/demo.jank"))).unwrap();
    println!("frames={n}");
}
