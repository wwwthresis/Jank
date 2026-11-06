//! Builds the demo project `demo/bunnies.jank`: two plush bunnies on a pink shaggy rug in a cosy evening room. The bunnies,
//! their photo textures, bones and skin weights come from `tools/bunnies/make_bunnies.py` (run it first on the two photos);
//! this program loads them, adds the rug and the tiled floor, the lamp light with soft shadows, and a short idle animation.
//!
//! `cargo run --release --example make_bunny_demo -- bunny_dir ../demo/bunnies.jank [preview.png]`
use psxcore::anim::Ease;
use psxcore::editor::{Editor, Mode, ViewCam};
use psxcore::json::Json;
use psxcore::lighting::{ShadowMode, ShadowQuality};
use psxcore::math::*;
use psxcore::mesh::{Bone, Material, Mesh, Model, Skeleton, Vertex};
use psxcore::raster::{PsxStyle, Renderer};
use psxcore::scene::Transform;
use psxcore::texture::Texture;
use std::path::Path;

fn hash(x: i32, y: i32, s: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ s.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xFFFF) as f32 / 65535.0
}

fn texture(path: &Path, name: &str) -> Texture {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let img = psxcore::decode::image::decode_png(&bytes).expect("a PNG texture");
    Texture { name: name.into(), w: img.w, h: img.h, px: img.rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect() }
}

/// A bunny written by make_bunnies.py: vertices (position, normal, uv, 4 joints, 4 weights), triangles, bones, texture atlas.
fn bunny(dir: &Path, file: &str, title: &str) -> (Model, Vec<(Vec3, Vec3)>) {
    let text = std::fs::read_to_string(dir.join(format!("{file}.json"))).expect("bunny JSON (run tools/bunnies/make_bunnies.py)");
    let doc = Json::parse(&text).expect("valid JSON");
    let mut mesh = Mesh::new(title);
    for v in doc.get("verts").and_then(|v| v.as_array()).expect("verts") {
        let f = v.f32_array().expect("vertex numbers");
        let mut vx = Vertex::new(Vec3::new(f[0], f[1], f[2]), Vec2::new(f[6], f[7]));
        vx.nrm = Vec3::new(f[3], f[4], f[5]);
        vx.joints = [f[8] as u16, f[9] as u16, f[10] as u16, f[11] as u16];
        vx.weights = [f[12], f[13], f[14], f[15]];
        mesh.verts.push(vx);
    }
    for t in doc.get("tris").and_then(|v| v.as_array()).expect("tris") {
        let f = t.f32_array().expect("triangle");
        mesh.tris.push([f[0] as u32, f[1] as u32, f[2] as u32]);
    }
    let mut bones: Vec<Bone> = Vec::new();
    let mut world: Vec<Vec3> = Vec::new();
    let mut ends = Vec::new();
    for b in doc.get("bones").and_then(|v| v.as_array()).expect("bones") {
        let p = b.get("pos").and_then(|p| p.f32_array()).expect("bone position");
        let q = b.get("tip").and_then(|p| p.f32_array()).expect("bone tip");
        let pos = Vec3::new(p[0], p[1], p[2]);
        ends.push((pos, Vec3::new(q[0], q[1], q[2])));
        let parent = b.get("parent").and_then(|p| p.as_f64()).filter(|p| *p >= 0.0).map(|p| p as usize);
        let rest_t = match parent {
            Some(i) => pos - world[i],
            None => pos,
        };
        bones.push(Bone { name: b.get("name").and_then(|n| n.as_str()).unwrap().to_string(), parent, rest_t, rest_r: Quat::IDENTITY, rest_s: Vec3::ONE });
        world.push(pos);
    }
    let model = Model {
        name: title.into(),
        meshes: vec![mesh],
        materials: vec![Material { name: "Fleece".into(), texture: Some(0), ..Default::default() }],
        textures: vec![texture(&dir.join(format!("{file}.png")), "Fleece")],
        skeleton: Some(Skeleton { bones }),
    };
    (model, ends)
}

/// A bunny in the scene and how its bones move.
struct Rig {
    object: u32,
    bones: Vec<(Vec3, Vec3)>,
    names: Vec<String>,
}
impl Rig {
    fn bone(&self, name: &str) -> usize {
        self.names.iter().position(|n| n == name).unwrap_or_else(|| panic!("bone {name}"))
    }
    /// The axis that lifts a bone off the rug (both bunnies lie on their backs, Y is up).
    fn lift(&self, b: usize) -> Vec3 {
        let r = self.bones[b].1 - self.bones[b].0;
        r.cross(Vec3::Y).normalized()
    }
}

/// Sets bone rotations of one object in the current frame: (bone, axis in model space, degrees), combined in order.
fn pose(ed: &mut Editor, rig: &Rig, moves: &[(&str, Vec3, f32)]) {
    let (a, f) = (ed.current_anim, ed.current_frame);
    let st = ed.project.animations[a].frames[f].states.iter_mut().find(|s| s.object == rig.object).expect("object state");
    for b in st.pose.bones.iter_mut() {
        b.rot = Quat::IDENTITY;
    }
    for (name, axis, deg) in moves {
        let b = rig.bone(name);
        let q = Quat::from_axis_angle(axis.normalized(), deg.to_radians());
        st.pose.bones[b].rot = q.mul(st.pose.bones[b].rot).normalized();
    }
}

/// The rug: a rounded rectangle (superellipse) with a fluffy, uneven pile and a rim that sinks to the floor.
fn rug(w: f32, d: f32, pile: f32, tex: Texture) -> Model {
    let (nx, nz) = (30usize, 22usize);
    let mut m = Mesh::new("Rug");
    for j in 0..=nz {
        for i in 0..=nx {
            let (u, v) = (i as f32 / nx as f32 * 2.0 - 1.0, j as f32 / nz as f32 * 2.0 - 1.0);
            // square grid -> superellipse of degree 4
            let r_inf = u.abs().max(v.abs());
            let len = (u * u + v * v).sqrt().max(1e-6);
            let (dx, dz) = (u / len, v / len);
            let r_sq = 1.0 / dx.abs().max(dz.abs());
            let r_se = (dx.abs().powi(4) + dz.abs().powi(4)).powf(-0.25);
            let edge = i == 0 || j == 0 || i == nx || j == nz;
            let shag = if edge { 1.0 + (hash(i as i32, j as i32, 3) - 0.5) * 0.06 } else { 1.0 };
            let k = if r_inf > 0.0 { r_inf * r_se / r_sq * len / r_inf } else { 0.0 } * shag;
            let (x, z) = (dx * k * w * 0.5, dz * k * d * 0.5);
            let fluff = (hash(i as i32, j as i32, 7) - 0.5) * pile * 0.5;
            let y = if edge { pile * 0.15 } else if r_inf > 0.93 { pile * 0.7 + fluff * 0.5 } else { pile + fluff };
            m.verts.push(Vertex::new(Vec3::new(x, y, z), Vec2::new(x / 0.85, z / 0.85)));
        }
    }
    let row = (nx + 1) as u32;
    for j in 0..nz as u32 {
        for i in 0..nx as u32 {
            let (a, b, c, e) = (j * row + i, j * row + i + 1, (j + 1) * row + i, (j + 1) * row + i + 1);
            m.tris.push([a, c, b]);
            m.tris.push([b, c, e]);
        }
    }
    m.recompute_normals();
    Model { name: "Rug".into(), meshes: vec![m], materials: vec![Material { name: "Rug".into(), texture: Some(0), ..Default::default() }], textures: vec![tex], skeleton: None }
}

/// The floor: a big plane with one tile per texture repeat.
fn floor(size: f32, tile: f32, tex: Texture) -> Model {
    let n = 48;
    let mut m = psxcore::modeling::plane(size, size, n, n);
    for v in &mut m.verts {
        v.uv = Vec2::new(v.pos.x / tile, v.pos.z / tile);
    }
    Model { name: "Floor".into(), meshes: vec![m], materials: vec![Material { name: "Tiles".into(), texture: Some(0), ..Default::default() }], textures: vec![tex], skeleton: None }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let usage = "usage: make_bunny_demo bunny_dir out.jank [preview.png]";
    let dir = Path::new(a.get(1).expect(usage));
    let out = a.get(2).expect(usage);

    let mut ed = Editor::new();
    ed.new_project("Bunnies");
    let mut style = PsxStyle::ps1_classic();
    style.name = "Cosy evening".into();
    style.width = 512;
    style.height = 288;
    style.z_buffer = true;
    style.affine = true;
    style.snap = 0.5;
    style.dither = true;
    style.color_bits = 6;
    style.fog_on = true;
    style.fog_near = 4.5;
    style.fog_far = 11.0;
    style.fog_color = [0.10, 0.07, 0.07];
    style.bg_top = [0.06, 0.04, 0.05];
    style.bg_bottom = [0.12, 0.08, 0.08];
    style.ambient = [0.52, 0.46, 0.48];
    style.light_color = [0.93, 0.82, 0.68];
    style.light_dir = Vec3::new(0.62, -0.62, 0.36).normalized();
    style.tint = [1.02, 1.0, 0.98];
    style.saturation = 1.05;
    style.contrast = 1.04;
    style.vignette = 0.35;
    style.grain = 0.03;
    ed.set_scene_style(style);

    // ---- room
    let fl = ed.import_model(floor(16.0, 0.8, texture(&dir.join("tiles.png"), "Tiles")));
    ed.rename_object(fl, "Floor");
    let pile = 0.045;
    let rg = ed.import_model(rug(3.6, 2.6, pile, texture(&dir.join("rug.png"), "Rug")));
    ed.rename_object(rg, "Rug");
    ed.set_object_transform(rg, Transform { pos: Vec3::new(0.1, 0.0, 0.0), rot_deg: Vec3::new(0.0, 8.0, 0.0), scale: Vec3::ONE });

    // ---- the cream bunny lies on its back on the rug, as in the photo
    let (cream, cream_bones) = bunny(dir, "cream_bunny", "Cream bunny");
    let cream_names: Vec<String> = cream.skeleton.as_ref().unwrap().bones.iter().map(|b| b.name.clone()).collect();
    let (lo, _) = cream.bounds().unwrap();
    let cb = ed.import_model(cream);
    ed.rename_object(cb, "Cream bunny");
    let cream_t = Transform { pos: Vec3::new(0.5, pile * 0.8 - lo.y, 0.12), rot_deg: Vec3::new(0.0, -8.0, 0.0), scale: Vec3::ONE };
    ed.set_object_transform(cb, cream_t.clone());

    // ---- the white two-headed bunny stands next to it
    let (white, white_bones) = bunny(dir, "white_bunny", "Two-headed bunny");
    let white_names: Vec<String> = white.skeleton.as_ref().unwrap().bones.iter().map(|b| b.name.clone()).collect();
    let (wlo, _) = white.bounds().unwrap();
    let wb = ed.import_model(white);
    ed.rename_object(wb, "Two-headed bunny");
    let white_t = Transform { pos: Vec3::new(-0.68, pile * 0.8 - wlo.y, 0.05), rot_deg: Vec3::new(0.0, 14.0, 0.0), scale: Vec3::ONE };
    ed.set_object_transform(wb, white_t.clone());

    // ---- lamp light with soft cast shadows
    let mut lit = ed.project.scene.lighting.clone();
    lit.tod_on = false;
    lit.shadows = ShadowMode::Projected;
    lit.shadow_strength = 0.55;
    lit.quality = ShadowQuality::High;
    lit.softness = 0.6;
    ed.set_lighting(lit);

    // ---- a scene layer in front: the warm glow of the lamp from the top left, with a few specks of dust in it
    let (w, h) = (512i32, 288i32);
    let glow = ed.add_scene_layer(false);
    ed.edit_scene_layer(glow, |l| {
        l.name = "Lamp glow".into();
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = ((x as f32 + 60.0) / w as f32, (y as f32 + 50.0) / h as f32 * 0.75);
                let a = 0.30 * (-(dx * dx + dy * dy) * 3.2).exp();
                if a > 0.01 {
                    l.img.set(x, y, [255, 214, 160, (a * 255.0) as u8]);
                }
            }
        }
        for i in 0..26 {
            let (x, y) = ((hash(i, 1, 21) * 230.0) as i32 + 4, (hash(i, 2, 21) * 150.0) as i32 + 4);
            let a = 0.35 + 0.4 * hash(i, 3, 21);
            l.img.set(x, y, [255, 236, 200, (a * 255.0) as u8]);
            if hash(i, 4, 21) > 0.7 {
                l.img.set(x + 1, y, [255, 236, 200, (a * 140.0) as u8]);
            }
        }
    });

    ed.view = ViewCam { target: Vec3::new(-0.08, 0.05, 0.1), yaw: 0.0, pitch: 0.98, dist: 2.5 };
    ed.camera_from_view();

    // ---- idle animation: the white bunny waves and wiggles its ears, the cream bunny kicks its feet in the air
    let cream = Rig { object: cb, bones: cream_bones, names: cream_names };
    let white = Rig { object: wb, bones: white_bones, names: white_names };
    ed.set_mode(Mode::Animate);
    ed.project.animations[0].name = "Idle".into();
    ed.set_fps(12, 1);
    let y = Vec3::Y;
    // four keys around a circle (sine and cosine), then the first key again so that the loop is seamless
    let keys: [(f32, f32); 5] = [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0), (0.0, 1.0)];
    for (i, (s, c)) in keys.iter().enumerate() {
        if i > 0 {
            ed.add_frame();
        }
        let (s, c) = (*s, *c);
        let w = |n: &str| white.lift(white.bone(n));
        pose(&mut ed, &white, &[
            ("Arm_R", y, -16.0 * s - 4.0),
            ("Arm_R", w("Arm_R"), 14.0 + 10.0 * s),
            ("Arm_L", y, 7.0 * c),
            ("Ear_R_Out", y, 10.0 * c),
            ("Ear_L_Out", y, -9.0 * s),
            ("Ear_L_In", w("Ear_L_In"), 9.0 + 7.0 * s),
            ("Ear_R_In", w("Ear_R_In"), 9.0 - 7.0 * c),
            ("Head_L", y, 4.0 * s),
            ("Head_R", y, -4.0 * c),
            ("Leg_L", w("Leg_L"), 6.0 + 6.0 * c),
            ("Leg_R", w("Leg_R"), 6.0 - 6.0 * c),
        ]);
        let k = |n: &str| cream.lift(cream.bone(n));
        pose(&mut ed, &cream, &[
            ("Foot_1", k("Foot_1"), 10.0 + 12.0 * s),
            ("Foot_2", k("Foot_2"), 10.0 + 12.0 * c),
            ("Ear_Long", y, 8.0 * c),
            ("Ear_Top", k("Ear_Top"), 5.0 + 5.0 * s),
            ("Paw_Up", y, 14.0 * s),
            ("Paw_Up", k("Paw_Up"), 8.0 + 6.0 * c),
            ("Paw_Down", y, -6.0 * c),
            ("Head", y, 5.0 * s),
        ]);
        let last = i == keys.len() - 1;
        ed.set_tween(i, !last);
        ed.set_hold(i, if last { 1 } else { 5 });
        let fi = ed.current_frame;
        ed.project.animations[0].frames[fi].ease = Ease::EASE_IN_OUT;
    }
    ed.goto_frame(0);
    ed.set_frame_note("Idle loop: ears, paws and feet on bones");
    ed.set_mode(Mode::Model);
    ed.select_object(None);

    if let Some(png) = a.get(3) {
        let mut r = Renderer::new(1, 1);
        ed.render_preview(&mut r);
        std::fs::write(png, psxcore::encode::png::encode_png(r.frame.w, r.frame.h, r.frame.rgba(), false)).unwrap();
        // every frame of the animation next to it: preview_0.png ...
        let anim = &ed.project.animations[0];
        for i in 0..anim.frames.len() {
            let mut r = Renderer::new(1, 1);
            psxcore::compose::render_frame_index(&ed.project, anim, i, &mut r);
            let p = png.replace(".png", &format!("_{i}.png"));
            std::fs::write(p, psxcore::encode::png::encode_png(r.frame.w, r.frame.h, r.frame.rgba(), false)).unwrap();
        }
    }
    ed.save_as(Path::new(out)).expect("save failed");
    println!("{out}");
}
