//! Builds the butterfly demo project: a rigged butterfly (from a glTF file) hovering over a meadow, with trees, a sky picture behind
//! and grass in front (scene layers), spots painted on a texture layer, sun lighting with cast shadows, and a flutter animation.
//! Every picture except the butterfly's own texture is generated here from scratch, so the demo carries no third-party material.
//!
//! `cargo run --release --example make_butterfly_demo -- butterfly_rigged.glb ../demo/butterfly.jank [preview.png]`
use psxcore::editor::{Editor, Mode, ViewCam};
use psxcore::lighting::ShadowMode;
use psxcore::math::*;
use psxcore::modeling;
use psxcore::raster::{PsxStyle, Renderer};
use psxcore::scene::Transform;
use psxcore::texture::Texture;

fn hash(x: i32, y: i32, s: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ s.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xFFFF) as f32 / 65535.0
}
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn px(c: [f32; 3], a: f32) -> [u8; 4] {
    [(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (a * 255.0) as u8]
}

/// Tileable grass with a few small flowers (64x64).
fn grass_texture() -> Texture {
    let mut t = Texture::new("Meadow", 64, 64, [0, 0, 0, 255]);
    for y in 0..64 {
        for x in 0..64 {
            let n = hash(x / 2, y / 2, 1) * 0.6 + hash(x, y, 2) * 0.4;
            let c = lerp3([0.20, 0.42, 0.14], [0.42, 0.66, 0.24], n);
            t.set(x, y, px(c, 1.0));
        }
    }
    // dark blades
    for i in 0..70 {
        let (x, y) = ((hash(i, 0, 3) * 64.0) as i32, (hash(i, 1, 3) * 64.0) as i32);
        for k in 0..3 {
            t.set((x + k / 2) % 64, (y + 62 - k) % 64, px([0.12, 0.30, 0.10], 1.0));
        }
    }
    // flowers
    for (i, col) in [[1.0, 0.95, 0.5], [1.0, 1.0, 1.0], [0.95, 0.55, 0.75], [1.0, 0.95, 0.5], [1.0, 1.0, 1.0]].iter().enumerate() {
        let (x, y) = ((hash(i as i32, 5, 4) * 62.0) as i32 + 1, (hash(i as i32, 6, 4) * 62.0) as i32 + 1);
        t.set(x, y, px(*col, 1.0));
        t.set(x + 1, y, px(*col, 1.0));
        t.set(x, y + 1, px(*col, 1.0));
    }
    t
}
fn bark_texture() -> Texture {
    let mut t = Texture::new("Bark", 16, 16, [0, 0, 0, 255]);
    for y in 0..16 {
        for x in 0..16 {
            let n = hash(x, y / 4, 7) * 0.5 + hash(x, y, 8) * 0.2;
            t.set(x, y, px(lerp3([0.28, 0.18, 0.10], [0.46, 0.30, 0.17], n), 1.0));
        }
    }
    t
}
fn needles_texture() -> Texture {
    let mut t = Texture::new("Needles", 32, 32, [0, 0, 0, 255]);
    for y in 0..32 {
        for x in 0..32 {
            let n = hash(x, y, 9) * 0.55 + hash(x / 3, y / 3, 10) * 0.45;
            t.set(x, y, px(lerp3([0.07, 0.24, 0.16], [0.17, 0.46, 0.25], n), 1.0));
        }
    }
    t
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (glb, out) = (a.get(1).expect("usage: make_butterfly_demo model.glb out.jank [preview.png]"), a.get(2).expect("output .jank"));
    let (w, h) = (384u32, 216u32);

    let mut ed = Editor::new();
    ed.new_project("Butterfly demo");
    let mut style = PsxStyle::ps1_classic();
    style.name = "Meadow".into();
    style.width = w;
    style.height = h;
    style.z_buffer = true; // shadows and the layered ground need real depth
    style.dither = true;
    style.color_bits = 5;
    style.fog_on = true;
    style.fog_near = 7.0;
    style.fog_far = 30.0;
    style.saturation = 1.1;
    style.vignette = 0.25;
    ed.set_scene_style(style);

    // ---- ground
    let mut g = modeling::plane(64.0, 64.0, 24, 24);
    for v in &mut g.verts {
        v.uv = Vec2::new(v.uv.x * 24.0, v.uv.y * 24.0);
    }
    let mut gm = modeling::model_from_mesh("Meadow", g, 64);
    gm.textures[0] = grass_texture();
    let ground = ed.import_model(gm);
    ed.rename_object(ground, "Meadow");

    // ---- pines: a trunk and a cone each (generated textures)
    for (i, (x, z, s)) in [(-4.2f32, 0.5f32, 1.0f32), (2.6, -5.5, 1.3), (-2.0, -8.0, 1.2), (6.0, -2.5, 0.9)].iter().enumerate() {
        let mut trunk = modeling::model_from_mesh("Trunk", modeling::cylinder(0.5, 1.0, 8), 16);
        trunk.textures[0] = bark_texture();
        let (lo, _) = trunk.bounds().unwrap();
        let tid = ed.import_model(trunk);
        ed.rename_object(tid, &format!("Trunk {}", i + 1));
        ed.set_object_transform(tid, Transform { pos: Vec3::new(*x, -lo.y * 0.9 * s, *z), rot_deg: Vec3::ZERO, scale: Vec3::new(0.35 * s, 0.9 * s, 0.35 * s) });
        let mut crown = modeling::model_from_mesh("Crown", modeling::cone(0.5, 1.0, 8), 16);
        crown.textures[0] = needles_texture();
        let (clo, _) = crown.bounds().unwrap();
        let cid = ed.import_model(crown);
        ed.rename_object(cid, &format!("Pine {}", i + 1));
        ed.set_object_transform(cid, Transform { pos: Vec3::new(*x, 0.8 * s - clo.y * 2.6 * s, *z), rot_deg: Vec3::new(0.0, 20.0 * i as f32, 0.0), scale: Vec3::new(1.7 * s, 2.6 * s, 1.7 * s) });
    }

    // ---- the rigged butterfly (kept as imported: skeleton, weights, texture)
    let warn = ed.import_path(std::path::Path::new(glb)).expect("import the rigged butterfly");
    for w in warn {
        eprintln!("import warning: {w}");
    }
    let bf = ed.selected_object.expect("butterfly object");
    ed.rename_object(bf, "Butterfly");
    let model = ed.selected_model().unwrap();
    let bones = model.skeleton.as_ref().map_or(0, |s| s.bones.len());
    assert_eq!(bones, 5, "the butterfly keeps its skeleton");
    ed.set_object_transform(bf, Transform { pos: Vec3::new(0.0, 1.05, 0.0), rot_deg: Vec3::new(0.0, 15.0, 0.0), scale: Vec3::new(1.3, 1.3, 1.3) });

    // ---- texture layer on the butterfly: pale spots along the wing edges, only where the wing really is
    ed.select_object(Some(bf));
    assert!(ed.enable_texture_layers());
    let base = ed.selected_model().unwrap().textures[0].clone();
    ed.edit_texture_stack(|s| {
        s.add("Spots");
        let layer = &mut s.layers[s.active];
        layer.name = "Spots".into();
        let mut n = 0;
        for y in (6..150).step_by(5) {
            for x in (3..78).step_by(5) {
                let p = base.get(x, y).unwrap_or([0; 4]);
                // a spot where the wing is solid and blue-black, and its neighbourhood is solid too (so it never leaves the wing)
                let inside = [(-2, 0), (2, 0), (0, -2), (0, 2)].iter().all(|(dx, dy)| base.get(x + dx, y + dy).is_some_and(|q| q[3] == 255));
                if p[3] == 255 && inside && p[2] as i32 - p[0] as i32 > 60 && hash(x, y, 11) > 0.55 {
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        layer.img.set(x + dx, y + dy, [255, 244, 190, 255]);
                    }
                    n += 1;
                }
            }
        }
        assert!(n > 4, "found places for spots: {n}");
    });

    // ---- scene layers: a sky picture behind the 3D scene, grass blades in front of it
    let back = ed.add_scene_layer(true);
    ed.edit_scene_layer(back, |l| {
        l.name = "Hills and clouds".into();
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let fx = x as f32 / w as f32;
                // far hills
                let far = 84.0 + (fx * 9.0).sin() * 6.0 + (fx * 23.0).sin() * 2.0;
                let near = 98.0 + (fx * 5.0 + 1.0).sin() * 7.0 + (fx * 17.0).sin() * 2.0;
                if (y as f32) >= near {
                    l.img.set(x, y, px([0.22, 0.46, 0.26], 1.0));
                } else if (y as f32) >= far {
                    l.img.set(x, y, px([0.40, 0.58, 0.60], 1.0));
                }
            }
        }
        // clouds: flat pixel ellipses, a little see-through
        for (cx, cy, rx, ry) in [(70.0f32, 34.0f32, 34.0f32, 7.0f32), (110.0, 40.0, 24.0, 6.0), (270.0, 26.0, 40.0, 8.0), (310.0, 50.0, 26.0, 5.0)] {
            for y in (cy - ry) as i32..=(cy + ry) as i32 {
                for x in (cx - rx) as i32..=(cx + rx) as i32 {
                    let d = ((x as f32 - cx) / rx).powi(2) + ((y as f32 - cy) / ry).powi(2);
                    if d <= 1.0 && l.img.get(x, y).is_some_and(|p| p[3] == 0) {
                        l.img.set(x, y, px([1.0, 0.98, 0.95], if d < 0.5 { 0.9 } else { 0.6 }));
                    }
                }
            }
        }
    });
    let front = ed.add_scene_layer(false);
    ed.edit_scene_layer(front, |l| {
        l.name = "Grass in front".into();
        for x in 0..w as i32 {
            let hgt = 3 + (hash(x / 2, 0, 12) * 12.0) as i32 + if x < 60 || x > w as i32 - 60 { 8 } else { 0 };
            let shade = hash(x, 1, 13);
            for k in 0..hgt {
                let tip = k as f32 / hgt as f32;
                l.img.set(x, h as i32 - 1 - k, px(lerp3([0.10, 0.30, 0.10], [0.30, 0.55, 0.18], tip * 0.7 + shade * 0.3), 1.0));
            }
        }
        // a few flowers in the corners
        for (fx, fy, c) in [(22i32, 176i32, [1.0f32, 0.9, 0.4]), (36, 184, [1.0, 1.0, 1.0]), (352, 178, [0.95, 0.5, 0.7]), (338, 188, [1.0, 0.9, 0.4])] {
            for (dx, dy) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
                l.img.set(fx + dx, fy + dy, px(c, 1.0));
            }
            l.img.set(fx, fy, px([0.9, 0.5, 0.1], 1.0));
            for k in 1..14 {
                l.img.set(fx, fy + k, px([0.12, 0.38, 0.12], 1.0));
            }
        }
    });

    // ---- lighting: late-afternoon sun, cast shadows
    let mut lit = ed.project.scene.lighting.clone();
    lit.tod_on = true;
    lit.time = 14.5;
    lit.azimuth = 40.0;
    lit.shadows = ShadowMode::Projected;
    lit.shadow_strength = 0.5;
    lit.quality = psxcore::lighting::ShadowQuality::High;
    lit.softness = 0.45;
    ed.set_lighting(lit);

    // ---- flutter animation: four frames, tweened; wings flap, the butterfly bobs and turns
    ed.view = ViewCam { target: Vec3::new(0.0, 0.95, 0.0), yaw: 0.75, pitch: 0.32, dist: 4.2 };
    ed.camera_from_view(); // the scene camera (used outside the animation, e.g. by the preview in Texture mode)
    ed.set_mode(Mode::Animate);
    ed.project.animations[0].name = "Flutter".into();
    let plan: [(f32, f32, f32, f32); 4] = [(28.0, 1.05, 15.0, -0.15), (8.0, 1.00, 22.0, 0.0), (-18.0, 0.92, 30.0, 0.12), (8.0, 1.00, 22.0, 0.0)];
    for (i, (flap, y, yaw, dx)) in plan.iter().enumerate() {
        if i > 0 {
            ed.add_frame();
        }
        ed.set_bone_euler(1, Vec3::new(0.0, 0.0, *flap));
        ed.set_bone_euler(2, Vec3::new(0.0, 0.0, -*flap));
        ed.set_bone_euler(3, Vec3::new(0.0, 0.0, *flap * 0.45));
        ed.set_bone_euler(4, Vec3::new(0.0, 0.0, -*flap * 0.45));
        ed.set_bone_euler(0, Vec3::new(-6.0 + *flap * 0.1, 0.0, 0.0));
        ed.set_object_transform(bf, Transform { pos: Vec3::new(*dx, *y, 0.0), rot_deg: Vec3::new(0.0, *yaw, 0.0), scale: Vec3::new(1.3, 1.3, 1.3) });
        ed.camera_from_view();
        ed.set_tween(i, true);
        ed.set_hold(i, 2);
    }
    ed.set_frame_note("Rigging demo: wing bones flap, the body tilts");
    ed.goto_frame(0);
    ed.set_mode(Mode::Pose);
    ed.save_as(std::path::Path::new(out)).expect("save failed");
    println!("{out}");

    if let Some(png) = a.get(3) {
        let mut r = Renderer::new(1, 1);
        ed.render_preview(&mut r);
        std::fs::write(png, psxcore::encode::png::encode_png(r.frame.w, r.frame.h, r.frame.rgba(), false)).unwrap();
    }
}
