//! Builds the demo project `demo/night-shrine.jank`: a small shrine in a pine wood at night, lit only by lamps:
//! stone lanterns and paper lanterns, candles on the steps, a humming vending machine, a moonbeam and fireflies.
//! Most shapes are made with the Edit-mode operators of the editor (spin, array, solidify, poke), the same ones the
//! Mesh menu runs, and the look is the "Night Glow" style with pixel lamps, filmic tone, bloom and contact shadows.
//!
//! `cargo run --release --example make_shrine_demo -- ../demo/night-shrine.jank [preview.png] [theme-picture.png]`
use psxcore::editmesh::{EditMesh, Part};
use psxcore::editor::{Editor, Mode, ViewCam};
use psxcore::lighting::{ShadowMode, ShadowQuality};
use psxcore::lights::{LightKind, LightPreset};
use psxcore::math::*;
use psxcore::mesh::{Material, Mesh, Model, Vertex};
use psxcore::raster::{Camera, PsxStyle, Renderer};
use psxcore::scene::Transform;
use psxcore::texture::Texture;
use std::path::Path;

fn hash(x: i32, y: i32, s: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ s.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xFFFF) as f32 / 65535.0
}

fn shade(c: [u8; 3], k: f32) -> [u8; 4] {
    let f = |v: u8| (v as f32 * k).clamp(0.0, 255.0) as u8;
    [f(c[0]), f(c[1]), f(c[2]), 255]
}

fn tex(name: &str, w: u32, h: u32, f: impl Fn(i32, i32) -> [u8; 4]) -> Texture {
    let mut t = Texture::new(name, w, h, [0, 0, 0, 255]);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            t.set(x, y, f(x, y));
        }
    }
    t
}

/// A colour with blotchy noise (stone, wood, moss).
fn noisy(name: &str, size: u32, base: [u8; 3], amount: f32, seed: u32) -> Texture {
    tex(name, size, size, |x, y| {
        let n = 0.55 * hash(x, y, seed) + 0.45 * hash(x / 3, y / 3, seed + 7);
        shade(base, 1.0 + (n - 0.5) * amount)
    })
}

fn map(mut m: Mesh, f: impl Fn(Vec3) -> Vec3) -> Mesh {
    for v in &mut m.verts {
        v.pos = f(v.pos);
    }
    m.recompute_normals();
    m
}

fn moved(m: Mesh, d: Vec3) -> Mesh {
    map(m, |p| p + d)
}

fn scaled(m: Mesh, s: Vec3) -> Mesh {
    map(m, |p| Vec3::new(p.x * s.x, p.y * s.y, p.z * s.z))
}

/// Back from Edit mode's polygons to one triangle mesh.
fn baked(em: &EditMesh, name: &str) -> Mesh {
    let (parts, _) = em.to_meshes();
    let mut out = Mesh::new(name);
    for m in parts {
        psxcore::modeling::append(&mut out, &m, Vec3::ZERO);
    }
    out.recompute_normals();
    out
}

fn editable(m: &Mesh) -> EditMesh {
    EditMesh::from_meshes(std::slice::from_ref(m))
}

/// Spin (lathe) of a profile given as (radius, height) pairs, like drawing the outline in Edit mode and running Mesh → Spin.
fn lathe(name: &str, profile: &[(f32, f32)], steps: usize) -> Mesh {
    let mut em = EditMesh::default();
    em.parts.push(Part { name: name.into(), material: 0 });
    for &(r, y) in profile {
        em.add_vert(Vec3::new(r, y, 0.0));
    }
    for i in 0..profile.len() as u32 - 1 {
        em.wires.insert((i, i + 1));
    }
    em.select_all(true);
    em.spin(Vec3::ZERO, Vec3::Y, 360.0, steps);
    baked(&em, name)
}

/// A box with its base on y = 0.
fn block(w: f32, h: f32, d: f32) -> Mesh {
    moved(psxcore::modeling::cube(Vec3::new(w, h, d)), Vec3::new(0.0, h * 0.5, 0.0))
}

/// An upright rectangle facing +Z with its bottom edge on y = 0.
fn panel(w: f32, h: f32) -> Mesh {
    let mut m = Mesh::new("Panel");
    let (a, b) = (w * 0.5, h);
    for (p, uv) in [((-a, 0.0), (0.0, 1.0)), ((a, 0.0), (1.0, 1.0)), ((a, b), (1.0, 0.0)), ((-a, b), (0.0, 0.0))] {
        m.verts.push(Vertex::new(Vec3::new(p.0, p.1, 0.0), Vec2::new(uv.0, uv.1)));
    }
    m.tris = vec![[0, 1, 2], [0, 2, 3]];
    m.recompute_normals();
    m
}

/// A model from (mesh, texture, unlit) parts: one material and texture per part.
fn model(name: &str, parts: Vec<(Mesh, Texture, bool)>) -> Model {
    let mut m = Model { name: name.into(), meshes: vec![], materials: vec![], textures: vec![], skeleton: None };
    for (i, (mut mesh, t, unlit)) in parts.into_iter().enumerate() {
        mesh.material = i;
        m.materials.push(Material { name: t.name.clone(), texture: Some(i), unlit, ..Default::default() });
        m.textures.push(t);
        m.meshes.push(mesh);
    }
    m
}

fn place(ed: &mut Editor, model: Model, pos: Vec3, yaw: f32) -> u32 {
    let id = ed.import_model(model);
    ed.set_object_transform(id, Transform { pos, rot_deg: Vec3::new(0.0, yaw, 0.0), scale: Vec3::ONE });
    id
}

fn copy(ed: &mut Editor, of: u32, name: &str, pos: Vec3, yaw: f32, scale: f32) -> u32 {
    let id = ed.duplicate_object(of).expect("object to copy");
    ed.rename_object(id, name);
    ed.set_object_transform(id, Transform { pos, rot_deg: Vec3::new(0.0, yaw, 0.0), scale: Vec3::splat_one(scale) });
    id
}

trait SplatOne {
    fn splat_one(s: f32) -> Vec3;
}
impl SplatOne for Vec3 {
    fn splat_one(s: f32) -> Vec3 {
        Vec3::new(s, s, s)
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let out = a.get(1).cloned().unwrap_or_else(|| "night-shrine.jank".into());
    let mut ed = Editor::new();
    ed.new_project("Night Shrine");

    // ---------------------------------------------------------------- textures
    let stone = || noisy("Stone", 32, [118, 116, 112], 0.55, 1);
    let moss = tex("Moss ground", 64, 64, |x, y| {
        let n = 0.5 * hash(x, y, 3) + 0.3 * hash(x / 4, y / 4, 4) + 0.2 * hash(x / 9, y / 9, 5);
        let c = if hash(x / 2, y / 2, 6) > 0.86 { [104, 112, 78] } else if hash(x / 3, y / 3, 7) > 0.8 { [92, 90, 84] } else { [70, 88, 60] };
        shade(c, 0.75 + n * 0.5)
    });
    let vermilion = noisy("Vermilion", 16, [196, 52, 34], 0.3, 8);
    let black = noisy("Black lacquer", 16, [56, 48, 50], 0.4, 9);
    let wood = tex("Dark wood", 32, 32, |x, y| {
        let grain = ((x as f32 * 0.9 + (y as f32 * 0.21).sin() * 2.0).sin() * 0.5 + 0.5) * 0.25;
        let slat = if x % 8 == 0 { 0.6 } else { 1.0 };
        shade([92, 62, 44], (0.8 + grain + hash(x, y, 10) * 0.15) * slat)
    });
    let shoji = tex("Shoji", 32, 32, |x, y| {
        if x % 8 == 0 || y % 10 == 0 || x == 31 || y == 31 {
            [70, 44, 30, 255]
        } else {
            let k = 0.92 + 0.08 * hash(x, y, 11);
            shade([255, 214, 150], k)
        }
    });
    let roof_tiles = tex("Roof tiles", 32, 32, |x, y| {
        let row = (y / 4) % 2;
        let edge = (x + row * 3) % 6 == 0 || y % 4 == 3;
        shade([58, 64, 78], if edge { 0.6 } else { 0.9 + 0.2 * hash(x, y, 12) })
    });
    let lantern_glow = tex("Lantern window", 16, 16, |x, y| {
        if x % 8 == 0 || y == 0 || y == 15 {
            [46, 38, 34, 255]
        } else {
            shade([255, 196, 110], 0.9 + 0.1 * hash(x, y, 13))
        }
    });
    let paper = tex("Red paper", 32, 32, |x, y| {
        // ribs, and a white disc with a dark crest on the front
        let rib = y % 4 == 0;
        let (dx, dy) = ((x as f32 - 8.0) * 1.5, y as f32 - 16.0);
        let d = (dx * dx + dy * dy).sqrt();
        if d < 7.5 {
            if d < 4.2 && d > 1.8 {
                [40, 20, 24, 255]
            } else {
                [255, 236, 214, 255]
            }
        } else {
            shade([238, 62, 40], if rib { 0.7 } else { 0.95 + 0.05 * hash(x, y, 14) })
        }
    });
    let candle_wax = noisy("Wax", 8, [236, 226, 200], 0.15, 15);
    let flame = tex("Flame", 8, 8, |_, y| if y < 4 { [255, 240, 170, 255] } else { [255, 170, 70, 255] });
    let pine = tex("Pine needles", 32, 32, |x, y| {
        let n = hash(x, y, 16) * 0.6 + hash(x / 2, y / 3, 17) * 0.4;
        shade([30, 58, 44], 0.7 + n * 0.6)
    });
    let bark = noisy("Bark", 16, [74, 54, 42], 0.6, 18);
    let machine_body = tex("Machine body", 16, 16, |x, y| if y < 2 { [40, 40, 48, 255] } else { shade([214, 218, 226], 0.92 + 0.08 * hash(x, y, 19)) });
    let machine_front = tex("Drinks", 32, 64, |x, y| {
        // a glowing sign on top, rows of cans, then the buttons and the slot
        if y < 10 {
            if (x + y / 2) % 7 < 4 && y > 2 && y < 8 {
                [255, 255, 255, 255]
            } else {
                [40, 190, 255, 255]
            }
        } else if y < 44 {
            let (cx, cy) = (x % 8, (y - 10) % 11);
            if cx >= 2 && cx <= 5 && cy >= 2 && cy <= 8 {
                let hue = [[255, 80, 70], [255, 210, 60], [90, 220, 120], [80, 150, 255], [255, 130, 200]][((x / 8 + (y - 10) / 11 * 3) % 5) as usize];
                shade(hue, if cy == 2 { 1.0 } else { 0.85 })
            } else {
                [236, 244, 255, 255]
            }
        } else if y < 48 {
            if x % 4 == 1 { [255, 60, 60, 255] } else { [210, 220, 236, 255] }
        } else if (52..58).contains(&y) && (6..26).contains(&x) {
            [20, 22, 30, 255]
        } else {
            [180, 192, 210, 255]
        }
    });

    // ---------------------------------------------------------------- the ground and the stone path
    let mut ground = psxcore::modeling::plane(36.0, 36.0, 18, 18);
    for v in &mut ground.verts {
        v.uv = Vec2::new(v.uv.x * 9.0, v.uv.y * 9.0);
        // the wood floor rolls a little away from the path
        let (x, z) = (v.pos.x, v.pos.z);
        if x.abs() > 2.5 {
            v.pos.y = (hash((x * 2.0) as i32, (z * 2.0) as i32, 20) - 0.5) * 0.25 + (x.abs() - 2.5) * 0.04;
        }
    }
    ground.recompute_normals();

    // one slab, repeated with Mesh → Array
    let mut slab = editable(&block(1.3, 0.08, 0.62));
    slab.select_all(true);
    slab.array(9, Vec3::new(0.0, 0.0, -0.95), false);
    let path = baked(&slab, "Path");

    // ---------------------------------------------------------------- stone lanterns (tōrō): three spun parts
    let foot = lathe("Foot", &[(0.0, 0.0), (0.34, 0.0), (0.34, 0.1), (0.22, 0.16), (0.11, 0.24), (0.11, 0.76), (0.24, 0.82), (0.27, 0.9), (0.0, 0.9)], 6);
    let window = lathe("Window", &[(0.0, 0.9), (0.2, 0.9), (0.2, 1.2), (0.0, 1.2)], 6);
    let top = lathe("Roof", &[(0.0, 1.2), (0.46, 1.24), (0.42, 1.31), (0.13, 1.45), (0.07, 1.52), (0.09, 1.58), (0.0, 1.68)], 6);
    let mut cap = editable(&top);
    cap.select_all(true);
    let toro = model("Stone lantern", vec![(foot, stone(), false), (window, lantern_glow, true), (baked(&cap, "Roof"), stone(), false)]);
    let first = place(&mut ed, toro, Vec3::new(-1.85, 0.0, 2.2), 0.0);
    ed.rename_object(first, "Stone lantern L1");
    let mut toros = vec![(first, Vec3::new(-1.85, 0.0, 2.2))];
    for (i, p) in [Vec3::new(1.85, 0.0, 2.2), Vec3::new(-1.55, 0.0, -1.6), Vec3::new(1.55, 0.0, -1.6)].into_iter().enumerate() {
        let name = ["Stone lantern R1", "Stone lantern L2", "Stone lantern R2"][i];
        toros.push((copy(&mut ed, first, name, p, 30.0 * i as f32, 1.0), p));
    }

    // ---------------------------------------------------------------- the torii: posts, a curved top beam (Solidify), a tie beam
    let mut parts = vec![];
    for x in [-1.45f32, 1.45] {
        parts.push(moved(psxcore::modeling::cylinder(0.15, 3.0, 8), Vec3::new(x, 0.0, 0.0)));
        parts.push(moved(block(0.42, 0.28, 0.42), Vec3::new(x, 0.0, 0.0)));
    }
    let mut posts = Mesh::new("Posts");
    for p in &parts {
        psxcore::modeling::append(&mut posts, p, Vec3::ZERO);
    }
    posts.recompute_normals();
    let tie = moved(block(3.5, 0.18, 0.2), Vec3::new(0.0, 2.45, 0.0));
    let plaque = moved(block(0.42, 0.55, 0.08), Vec3::new(0.0, 2.6, 0.06));
    // the top beam: a strip bent upwards at both ends, then given thickness
    let mut strip = psxcore::modeling::plane(4.6, 0.46, 12, 1);
    for v in &mut strip.verts {
        let t = v.pos.x / 2.3;
        v.pos.y = 3.02 + 0.16 * t * t * t * t;
    }
    let mut beam = editable(&strip);
    beam.select_all(false);
    beam.solidify(0.22);
    let kasagi = baked(&beam, "Top beam");
    let mut red = Mesh::new("Red");
    for p in [&posts, &tie] {
        psxcore::modeling::append(&mut red, p, Vec3::ZERO);
    }
    red.recompute_normals();
    let torii = model("Torii", vec![(red, vermilion.clone(), false), (kasagi, black.clone(), false), (plaque, black.clone(), false)]);
    place(&mut ed, torii, Vec3::new(0.0, 0.0, -3.6), 0.0);

    // ---------------------------------------------------------------- paper lanterns hanging from the tie beam
    let mut body = vec![];
    for i in 0..=8 {
        let a = (-70.0 + 140.0 * i as f32 / 8.0f32).to_radians();
        body.push((0.21 * a.cos(), 0.27 + 0.27 * a.sin()));
    }
    let mut prof = vec![(0.0, 0.0)];
    prof.extend(body);
    prof.push((0.0, 0.54));
    let chochin_body = lathe("Paper", &prof, 8);
    let mut rings = moved(psxcore::modeling::frustum(0.09, 0.09, 0.05, 8, true), Vec3::new(0.0, -0.03, 0.0));
    psxcore::modeling::append(&mut rings, &psxcore::modeling::frustum(0.09, 0.09, 0.05, 8, true), Vec3::new(0.0, 0.52, 0.0));
    psxcore::modeling::append(&mut rings, &psxcore::modeling::cylinder(0.008, 0.36, 3), Vec3::new(0.0, 0.57, 0.0));
    rings.recompute_normals();
    let chochin = model("Paper lantern", vec![(chochin_body, paper, true), (rings, black.clone(), false)]);
    let pl = place(&mut ed, chochin, Vec3::new(-0.75, 1.52, -3.6), 180.0);
    ed.rename_object(pl, "Paper lantern L");
    let pr = copy(&mut ed, pl, "Paper lantern R", Vec3::new(0.75, 1.52, -3.6), 180.0, 1.0);

    // ---------------------------------------------------------------- the shrine
    let z0 = -7.6;
    let base = block(4.6, 0.55, 3.6);
    let walls = moved(block(3.6, 1.9, 2.7), Vec3::new(0.0, 0.55, 0.0));
    let mut beams = moved(block(3.9, 0.16, 3.0), Vec3::new(0.0, 2.45, 0.0));
    for x in [-1.8f32, 1.8] {
        for z in [-1.35f32, 1.35] {
            psxcore::modeling::append(&mut beams, &psxcore::modeling::cylinder(0.11, 1.9, 6), Vec3::new(x, 0.55, z));
        }
    }
    beams.recompute_normals();
    // the roof: a wide slab whose top is poked up into a hip (Mesh → Poke), and a smaller one above it
    let mut roof = editable(&moved(block(5.2, 0.14, 4.2), Vec3::new(0.0, 2.58, 0.0)));
    let topf = (0..roof.faces.len() as u32).max_by(|&a, &b| roof.face_center(a).y.total_cmp(&roof.face_center(b).y)).unwrap();
    roof.set_mode(psxcore::editmesh::SelectMode::Face);
    roof.click_face(topf, false);
    roof.poke(1.25);
    let mut crest = editable(&moved(block(1.6, 0.1, 0.5), Vec3::new(0.0, 3.86, 0.0)));
    crest.select_all(true);
    let roof_m = baked(&roof, "Roof");
    let mut roof_all = roof_m;
    psxcore::modeling::append(&mut roof_all, &baked(&crest, "Crest"), Vec3::ZERO);
    roof_all.recompute_normals();
    // steps: one step repeated up and back with the array operator
    let mut step = editable(&block(1.6, 0.14, 0.34));
    step.select_all(true);
    step.array(4, Vec3::new(0.0, 0.135, -0.3), false);
    let steps = moved(baked(&step, "Steps"), Vec3::new(0.0, 0.0, 2.25));
    let doors = moved(panel(2.0, 1.45), Vec3::new(0.0, 0.62, 1.36));
    let mut wood_parts = Mesh::new("Wood");
    for p in [&walls, &beams, &steps] {
        psxcore::modeling::append(&mut wood_parts, p, Vec3::ZERO);
    }
    wood_parts.recompute_normals();
    let shrine = model("Shrine", vec![(base, stone(), false), (wood_parts, wood, false), (roof_all, roof_tiles, false), (doors, shoji, true)]);
    place(&mut ed, shrine, Vec3::new(0.0, 0.0, z0), 0.0);
    // an offering box
    let box_m = moved(block(0.9, 0.5, 0.5), Vec3::new(0.0, 0.0, 0.0));
    place(&mut ed, model("Offering box", vec![(box_m, black.clone(), false)]), Vec3::new(0.0, 0.0, z0 + 2.9), 0.0);

    // ---------------------------------------------------------------- candles on the platform
    let wax = moved(psxcore::modeling::cylinder(0.035, 0.16, 6), Vec3::ZERO);
    let tip = moved(scaled(psxcore::modeling::cone(0.02, 0.06, 4), Vec3::new(1.0, 1.0, 1.0)), Vec3::new(0.0, 0.16, 0.0));
    let candle = model("Candle", vec![(wax, candle_wax, false), (tip, flame, true)]);
    let c0 = place(&mut ed, candle, Vec3::new(-1.3, 0.55, z0 + 1.55), 0.0);
    ed.rename_object(c0, "Candle 1");
    let candle_spots = [Vec3::new(-1.3, 0.55, z0 + 1.55), Vec3::new(1.25, 0.55, z0 + 1.6), Vec3::new(1.45, 0.55, z0 + 1.45), Vec3::new(-0.6, 0.0, z0 + 2.85)];
    let mut candles = vec![c0];
    for (i, p) in candle_spots.iter().enumerate().skip(1) {
        candles.push(copy(&mut ed, c0, &format!("Candle {}", i + 1), *p, 0.0, 0.8 + 0.25 * hash(i as i32, 0, 30)));
    }

    // ---------------------------------------------------------------- the vending machine by the wood
    let body_m = block(0.95, 1.85, 0.72);
    let front = moved(panel(0.84, 1.66), Vec3::new(0.0, 0.1, 0.365));
    let vend = model("Vending machine", vec![(body_m, machine_body, false), (front, machine_front, true)]);
    let (vpos, vyaw) = (Vec3::new(4.3, 0.0, -1.4), -52.0f32);
    place(&mut ed, vend, vpos, vyaw);

    // ---------------------------------------------------------------- pines
    let mut needles = Mesh::new("Needles");
    for (i, (r, h, y)) in [(1.25f32, 1.6f32, 0.9f32), (1.0, 1.45, 1.75), (0.72, 1.3, 2.55), (0.42, 1.1, 3.3)].iter().enumerate() {
        let c = psxcore::modeling::frustum(*r, 0.0, *h, 7, true);
        let c = map(c, |p| Vec3::new(p.x, p.y, p.z) + Vec3::new(0.04 * (i as f32 - 1.5), 0.0, 0.0));
        psxcore::modeling::append(&mut needles, &c, Vec3::new(0.0, *y, 0.0));
    }
    needles.recompute_normals();
    let trunk = psxcore::modeling::cylinder(0.16, 1.4, 6);
    let tree = model("Pine", vec![(needles, pine, false), (trunk, bark, false)]);
    let t0 = place(&mut ed, tree, Vec3::new(-4.4, 0.0, -4.5), 0.0);
    ed.rename_object(t0, "Pine 1");
    let trees = [
        (Vec3::new(-5.6, 0.0, -9.5), 1.35),
        (Vec3::new(-3.3, 0.0, -11.5), 1.2),
        (Vec3::new(4.6, 0.0, -6.8), 1.3),
        (Vec3::new(5.8, 0.0, -10.4), 1.5),
        (Vec3::new(-6.2, 0.0, 0.5), 1.15),
        (Vec3::new(6.4, 0.0, 1.8), 1.1),
        (Vec3::new(2.6, 0.0, -12.5), 1.4),
        (Vec3::new(-8.4, 0.0, -5.2), 1.6),
    ];
    for (i, (p, s)) in trees.iter().enumerate() {
        copy(&mut ed, t0, &format!("Pine {}", i + 2), *p, 40.0 * i as f32, *s);
    }

    // the ground and the path come last in the list: opening the project selects the first object
    place(&mut ed, model("Stone path", vec![(path, stone(), false)]), Vec3::new(0.0, 0.0, 3.6), 0.0);
    place(&mut ed, model("Ground", vec![(ground, moss, false)]), Vec3::ZERO, 0.0);

    // ---------------------------------------------------------------- lamps
    let lamp = |ed: &mut Editor, preset: LightPreset, name: &str, pos: Vec3, attach: Option<u32>, f: &dyn Fn(&mut psxcore::lights::Light)| -> u32 {
        let id = ed.add_light(preset);
        let l = ed.project.scene.light_mut(id).unwrap();
        l.name = name.into();
        l.pos = pos;
        l.attach = attach;
        f(l);
        id
    };
    for (i, (obj, _)) in toros.iter().enumerate() {
        lamp(&mut ed, LightPreset::Lantern, &format!("Stone lantern {}", i + 1), Vec3::new(0.0, 1.05, 0.0), Some(*obj), &|l| {
            l.intensity = 2.2;
            l.range = 4.6;
            l.falloff = 0.4;
            l.halo = 0.6;
        });
    }
    for (obj, side) in [(pl, "left"), (pr, "right")] {
        lamp(&mut ed, LightPreset::Lantern, &format!("Paper lantern {side}"), Vec3::new(0.0, 0.27, 0.0), Some(obj), &|l| {
            l.color = [1.0, 0.42, 0.26];
            l.intensity = 1.3;
            l.range = 3.6;
            l.flicker = 0.2;
            l.halo = 0.8;
        });
    }
    for (i, c) in candles.iter().enumerate() {
        lamp(&mut ed, LightPreset::Candle, &format!("Candle {}", i + 1), Vec3::new(0.0, 0.22, 0.0), Some(*c), &|l| {
            l.intensity = 0.9;
            l.range = 1.8;
        });
    }
    // behind the paper doors: a warm glow that spills onto the steps
    lamp(&mut ed, LightPreset::Bulb, "Inside the shrine", Vec3::new(0.0, 1.3, z0 + 1.9), None, &|l| {
        l.color = [1.0, 0.7, 0.42];
        l.intensity = 1.2;
        l.range = 4.0;
        l.falloff = 0.5;
        l.halo = 0.0;
    });
    let yaw = vyaw.to_radians();
    let ahead = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    lamp(&mut ed, LightPreset::Neon, "Vending machine", vpos + ahead * 0.65 + Vec3::new(0.0, 1.1, 0.0), None, &|l| {
        l.color = [0.55, 0.86, 1.0];
        l.intensity = 1.7;
        l.range = 4.2;
        l.halo = 0.35;
        l.flicker = 0.04;
        l.flicker_speed = 3.0;
    });
    lamp(&mut ed, LightPreset::Moonbeam, "Moonbeam", Vec3::new(-2.2, 7.0, -3.6), None, &|l| {
        l.kind = LightKind::Spot;
        l.rot_deg = Vec3::new(0.0, 0.0, -16.0);
        l.intensity = 0.9;
        l.cone_deg = 34.0;
    });
    for i in 0..7 {
        let p = Vec3::new((hash(i, 1, 40) - 0.5) * 9.0, 0.35 + hash(i, 2, 40) * 1.2, -1.0 - hash(i, 3, 40) * 8.0);
        let p = if p.x.abs() < 1.2 { p + Vec3::new(2.2f32.copysign(p.x), 0.0, 0.0) } else { p };
        lamp(&mut ed, LightPreset::Firefly, &format!("Firefly {}", i + 1), p, None, &|l| {
            l.flicker_speed = 0.3 + 0.1 * (i as f32 % 3.0);
            l.halo = 0.7;
            l.intensity = 0.9;
        });
    }

    // ---------------------------------------------------------------- look, moonlight shadows, camera
    let mut style = PsxStyle::night_glow();
    style.width = 480;
    style.height = 270;
    style.light_dir = Vec3::new(-0.35, -1.0, -0.45).normalized();
    style.light_color = [0.30, 0.36, 0.58];
    style.ambient = [0.20, 0.22, 0.34];
    style.fog_near = 9.0;
    style.fog_far = 26.0;
    style.fog_color = [0.07, 0.08, 0.16];
    style.bg_top = [0.02, 0.03, 0.08];
    style.bg_bottom = [0.10, 0.10, 0.20];
    ed.set_scene_style(style);
    let mut lit = ed.project.scene.lighting.clone();
    lit.tod_on = false;
    lit.shadows = ShadowMode::Projected;
    lit.shadow_strength = 0.45;
    lit.quality = ShadowQuality::High;
    lit.softness = 0.55;
    ed.set_lighting(lit);

    // a starry sky with the moon behind everything
    let sky = ed.add_scene_layer(true);
    ed.edit_scene_layer(sky, |l| {
        l.name = "Moon and stars".into();
        let (w, h) = (l.img.w as i32, l.img.h as i32);
        for i in 0..170 {
            let (x, y) = ((hash(i, 1, 50) * w as f32) as i32, (hash(i, 2, 50) * hash(i, 5, 50) * h as f32 * 0.8) as i32);
            let a = 0.35 + 0.65 * hash(i, 3, 50);
            l.img.set(x, y, [220, 228, 255, (a * 255.0) as u8]);
        }
        let (mx, my, r) = (w as f32 * 0.79, h as f32 * 0.17, h as f32 * 0.075);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f32 - mx, y as f32 - my);
                let d = (dx * dx + dy * dy).sqrt();
                if d < r {
                    let crater = hash(x / 3, y / 3, 51) > 0.8;
                    let k = if crater { 0.86 } else { 1.0 };
                    l.img.set(x, y, shade([246, 240, 222], k));
                } else if d < r * 3.2 {
                    let a = 0.22 * (1.0 - (d - r) / (r * 2.2)).powi(2);
                    l.img.set(x, y, [170, 190, 255, (a * 255.0) as u8]);
                }
            }
        }
    });

    let cam = Camera { eye: Vec3::new(0.8, 1.3, 5.5), target: Vec3::new(0.15, 1.65, -4.0), fov_y: 52f32.to_radians(), ..Default::default() };
    ed.project.scene.camera = cam;
    ed.view = ViewCam::from_camera(&cam);
    ed.camera_from_view();
    ed.set_frame_fov(52.0);
    ed.set_mode(Mode::Model);
    ed.select_object(None);
    ed.select_light(None);

    if let Some(png) = a.get(2) {
        let mut r = Renderer::new(1, 1);
        ed.render_preview(&mut r);
        std::fs::write(png, psxcore::encode::png::encode_png(r.frame.w, r.frame.h, r.frame.rgba(), false)).unwrap();
    }
    // the picture of the "Shrine Night" example theme: the same view without film grain (it packs much smaller)
    if let Some(png) = a.get(3) {
        let mut p = ed.project.clone();
        p.scene.style.grain = 0.0;
        p.scene.style.dither = false;
        let states = psxcore::compose::scene_states(&p);
        let mut r = Renderer::new(1, 1);
        let spec = psxcore::compose::FrameSpec { states: &states, camera: p.scene.camera, style: &p.scene.style, hand_layer: None, hand_only: false, scene_layers: true, time: 0.0 };
        psxcore::compose::render_states(&p, &spec, &mut r);
        std::fs::write(png, psxcore::encode::png::encode_png(r.frame.w, r.frame.h, r.frame.rgba(), false)).unwrap();
    }
    ed.save_as(Path::new(&out)).expect("save failed");
    println!("{out}");
}
