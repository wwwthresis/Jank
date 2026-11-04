use psxcore::gltf::load_gltf;
fn main() {
    let p = std::env::args().nth(1).expect("usage: gltf_check file.glb");
    let b = std::fs::read(&p).unwrap();
    let r = load_gltf(&b, &|_| None).unwrap();
    for w in &r.warnings { eprintln!("warn: {w}"); }
    for m in &r.model.meshes {
        for v in &m.verts { println!("v {:.4} {:.4} {:.4} uv {:.4} {:.4}", v.pos.x, v.pos.y, v.pos.z, v.uv.x, v.uv.y); }
        for t in &m.tris { println!("t {} {} {}", t[0], t[1], t[2]); }
    }
}
