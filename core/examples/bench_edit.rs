//! Edit-mode benchmark on a heavy mesh: `cargo run --release --example bench_edit [segments rings]`
//! Times what happens per frame (overlay, hover) and per action (click, select all, move, extrude, undo).
use psxcore::editmesh::SelectMode;
use psxcore::editor::{ClickMod, DragSnap, Editor, MeshDragKind, ViewCam};
use psxcore::math::Vec3;
use psxcore::modeling;
use psxcore::raster::Renderer;
use std::time::Instant;

fn time<R>(label: &str, n: usize, mut f: impl FnMut() -> R) -> R {
    let mut r = f();
    let t = Instant::now();
    for _ in 0..n {
        r = f();
    }
    println!("{:<34} {:>9.2} ms", label, t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    r
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3))
}

fn main() {
    let a: Vec<usize> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let (seg, rings) = (a.first().copied().unwrap_or(200), a.get(1).copied().unwrap_or(100));
    let mut ed = Editor::new();
    ed.new_project("bench");
    let mut m = modeling::sphere(1.0, seg, rings);
    psxcore::decimate::box_unwrap(&mut m);
    let id = ed.import_model(modeling::model_from_mesh("Ball", m, 64));
    ed.select_object(Some(id));
    ed.view = ViewCam { target: Vec3::ZERO, yaw: 0.6, pitch: 0.3, dist: 3.2 };
    let (w, h) = (1280u32, 720u32);
    let mut r = Renderer::new(w, h);
    let t = Instant::now();
    assert!(ed.edit_enter());
    println!("{:<34} {:>9.2} ms", "enter Edit mode", t.elapsed().as_secs_f64() * 1000.0);
    let st = ed.edit_stats().unwrap();
    println!("mesh: {} verts, {} edges, {} tris, {} quads, {} n-gons", st.0, st.1, st.2, st.3, st.4);
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    time("render viewport", 5, || ed.render_viewport_sized(&mut r, w, h));
    for mode in [SelectMode::Vertex, SelectMode::Edge, SelectMode::Face] {
        ed.edit_set_select_mode(mode);
        let o = time(&format!("overlay ({mode:?})"), 5, || ed.edit_overlay(&r.frame));
        let _ = o.edges.len();
        time(&format!("hover ({mode:?})"), 5, || ed.edit_hover(Some(&r.frame), cx + 3.0, cy + 2.0, 8.0));
    }
    ed.edit_set_select_mode(SelectMode::Face);
    ed.edit_click(&r.frame, cx, cy, 8.0, ClickMod::Replace);
    ed.edit_grow(true);
    let tint = |o: u16, m: u16, t: u32| -> [u8; 4] { if ed.edit_face_state(o, m, t) & 1 != 0 { [40, 80, 200, 80] } else { [0; 4] } };
    let px = time("highlight picture (selection tint)", 5, || r.frame.overlay(&tint, &[(1, [255, 255, 255, 230])], 2));
    let (meshes, _) = ed.edit_mesh().unwrap().to_meshes();
    println!("checksums: mesh {:016x}  highlight {:016x}", fnv(&format!("{meshes:?}").into_bytes()), fnv(&px));
    time("click select (Face)", 5, || ed.edit_click(&r.frame, cx, cy, 8.0, ClickMod::Replace));
    time("select all", 5, || ed.edit_select_all(true));
    ed.edit_select_all(false);
    ed.edit_click(&r.frame, cx, cy, 8.0, ClickMod::Replace);
    ed.edit_grow(true);
    ed.edit_grow(true);
    let snap = DragSnap { increment: false, step: 0.1, vertex: false, precise: false };
    let t = Instant::now();
    assert!(ed.edit_drag_begin(MeshDragKind::Move, (cx, cy), w, h));
    let begin = t.elapsed().as_secs_f64() * 1000.0;
    let mut k = 0.0;
    time("move: one mouse step", 10, || {
        k += 1.0;
        ed.edit_drag_update((cx + k, cy), snap)
    });
    let t = Instant::now();
    ed.edit_drag_end(true);
    println!("{:<34} {:>9.2} ms / {:.2} ms", "move: begin / end", begin, t.elapsed().as_secs_f64() * 1000.0);
    ed.edit_select_all(true);
    let t = Instant::now();
    assert!(ed.edit_drag_begin(MeshDragKind::Move, (cx, cy), w, h));
    let begin = t.elapsed().as_secs_f64() * 1000.0;
    time("move all: one mouse step", 5, || {
        k += 1.0;
        ed.edit_drag_update((cx + k, cy), snap)
    });
    ed.edit_drag_end(true);
    println!("{:<34} {:>9.2} ms", "move all: begin", begin);
    ed.edit_select_all(false);
    ed.edit_click(&r.frame, cx, cy, 8.0, ClickMod::Replace);
    let t = Instant::now();
    ed.edit_extrude_begin((cx, cy), w, h);
    ed.edit_drag_end(true);
    println!("{:<34} {:>9.2} ms", "extrude one face", t.elapsed().as_secs_f64() * 1000.0);
    let t = Instant::now();
    ed.undo();
    println!("{:<34} {:>9.2} ms", "undo", t.elapsed().as_secs_f64() * 1000.0);
    let t = Instant::now();
    ed.edit_exit();
    println!("{:<34} {:>9.2} ms", "leave Edit mode", t.elapsed().as_secs_f64() * 1000.0);
}
