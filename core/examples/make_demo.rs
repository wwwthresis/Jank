//! Demo project generator: a singing "worm" with a skeleton, emotions and frame-by-frame animation.
//! `cargo run --release --example make_demo -- ../demo/friend.jank`
use psxcore::editor::{Canvas, Editor, Mode};
use psxcore::math::*;
use psxcore::mesh::*;
use psxcore::modeling;
use psxcore::raster::PsxStyle;
use psxcore::texture::Texture;

const SEG_H: f32 = 0.5;
const BONES: usize = 5; // root + 3 segments + head

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "friend.jank".into());

    // --- mesh: three truncated cones + a sphere head; UVs: body v in [0, 0.6], head v in [0.62, 1]
    let mut mesh = Mesh::new("Friend");
    let radii = [0.55f32, 0.46, 0.38, 0.32];
    for (i, w) in radii.windows(2).enumerate() {
        let part = modeling::frustum(w[0], w[1], SEG_H, 8, false);
        modeling::append(&mut mesh, &part, Vec3::new(0.0, SEG_H * i as f32, 0.0));
    }
    let body_end = mesh.verts.len();
    let head = modeling::sphere(0.5, 8, 6);
    modeling::append(&mut mesh, &head, Vec3::new(0.0, SEG_H * 3.0 + 0.35, 0.0));
    // the bottom tip of the sphere is not needed; leave it as is, since the PSX look is pixelated anyway
    let top = SEG_H * 3.0 + 0.85;
    for (i, v) in mesh.verts.iter_mut().enumerate() {
        v.uv.y = if i < body_end { (v.pos.y / (SEG_H * 3.0)).clamp(0.0, 1.0) * 0.6 } else { 0.62 + ((v.pos.y - (SEG_H * 3.0 - 0.15)) / (top - (SEG_H * 3.0 - 0.15))).clamp(0.0, 1.0) * 0.38 };
        // weights: bones sit at heights 0, .5, 1, 1.5, 2 -> linear blend of the two nearest bones
        let h = (v.pos.y / SEG_H).clamp(0.0, (BONES - 1) as f32 - 0.001);
        let k = h.floor() as usize;
        let t = h - k as f32;
        v.joints = [k as u16, (k + 1) as u16, 0, 0];
        v.weights = [1.0 - t, t, 0.0, 0.0];
    }
    mesh.recompute_normals();

    let bone = |name: &str, parent: Option<usize>, y: f32| Bone { name: name.into(), parent, rest_t: Vec3::new(0.0, y, 0.0), rest_r: Quat::IDENTITY, rest_s: Vec3::ONE };
    let mut model = modeling::model_from_mesh("Friend", mesh, 64);
    model.skeleton = Some(Skeleton { bones: vec![bone("root", None, 0.0), bone("segment 1", Some(0), SEG_H), bone("segment 2", Some(1), SEG_H), bone("segment 3", Some(2), SEG_H), bone("head", Some(3), SEG_H)] });

    // --- texture: stripes on the body, a face on each of the four sides of the head
    let mut tex = Texture::new("Main", 64, 64, [120, 200, 130, 255]);
    for y in 0..38 {
        for x in 0..64 {
            let band = ((y / 6) % 2 == 0) as i32;
            let c = if band == 0 { [112, 196, 128, 255] } else { [150, 222, 150, 255] };
            tex.set(x, y, c);
        }
    }
    for y in 40..64 {
        for x in 0..64 {
            tex.set(x, y, [250, 214, 150, 255]);
        }
    }
    let face = |t: &mut Texture, eye: i32, mouth: u8| {
        for z in 0..4 {
            let x0 = 16 * z;
            for dx in [4, 10] {
                for ey in 0..eye {
                    for ex in 0..eye {
                        t.set(x0 + dx + ex, 46 + ey, [40, 30, 50, 255]);
                    }
                }
            }
            match mouth {
                0 => { // smile
                    for dx in 5..=10 { t.set(x0 + dx, 55, [150, 50, 70, 255]); }
                    t.set(x0 + 4, 54, [150, 50, 70, 255]);
                    t.set(x0 + 11, 54, [150, 50, 70, 255]);
                }
                1 => { // "o" mouth
                    for (dx, dy) in [(7, 53), (8, 53), (6, 54), (9, 54), (6, 55), (9, 55), (7, 56), (8, 56)] { t.set(x0 + dx, dy, [150, 50, 70, 255]); }
                }
                _ => { // sad
                    for dx in 5..=10 { t.set(x0 + dx, 55, [90, 80, 140, 255]); }
                    t.set(x0 + 4, 56, [90, 80, 140, 255]);
                    t.set(x0 + 11, 56, [90, 80, 140, 255]);
                    t.set(x0 + 4, 50, [100, 160, 240, 255]); // tear
                }
            }
        }
    };
    let mut base = tex.clone();
    face(&mut base, 2, 0);
    model.textures[0] = base.clone();

    let mut ed = Editor::new();
    let id = ed.import_model(model);
    let asset = ed.selected_asset().unwrap();
    ed.view.orbit(0.55, -0.12);
    ed.set_mode(Mode::Animate);

    // --- emotions: redrawn copies of the texture
    let mut mk = |name: &str, eye: i32, mouth: u8| -> usize {
        let idx = ed.add_emotion(name).unwrap();
        let mut t = tex.clone();
        t.name = name.into();
        face(&mut t, eye, mouth);
        ed.replace_canvas(Canvas::Emotion { asset, idx }, t);
        idx
    };
    let happy = mk("Joy", 2, 0);
    let wow = mk("Surprise", 3, 1);
    let sad = mk("Sadness", 2, 2);

    ed.set_scene_style(PsxStyle::lofi_calm());
    // --- frames: a wave travels along the body; emotions change along the way
    let plan: [(f32, usize); 6] = [(-1.0, happy), (-0.4, happy), (0.4, wow), (1.0, wow), (0.4, sad), (-0.4, sad)];
    for (i, (phase, emo)) in plan.iter().enumerate() {
        if i > 0 {
            ed.add_frame();
        }
        for b in 1..BONES {
            let amp = 16.0 * *phase * if b % 2 == 0 { -1.0 } else { 1.0 };
            ed.set_bone_euler(b, Vec3::new(0.0, 0.0, amp));
        }
        ed.set_bone_euler(4, Vec3::new(8.0 * *phase, 0.0, 0.0));
        ed.set_frame_emotion(id, Some(*emo));
        ed.set_tween(i, true);
        ed.set_hold(i, 2);
        ed.camera_from_view();
    }
    ed.set_frame_note(&"Hello!".to_string());
    ed.goto_frame(0);
    ed.set_mode(Mode::Pose);
    ed.save_as(std::path::Path::new(&out)).expect("save failed");
    println!("{out}");
}
