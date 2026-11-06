//! Prints what a glTF/GLB model contains: meshes, materials, textures, bones.
use psxcore::gltf::load_gltf;
fn main() {
    let p = std::env::args().nth(1).expect("usage: model_info file.glb");
    let r = load_gltf(&std::fs::read(&p).unwrap(), &|_| None).unwrap();
    for w in &r.warnings { println!("warning: {w}"); }
    let m = &r.model;
    println!("name {:?}, triangles {}", m.name, m.triangle_count());
    for (i, me) in m.meshes.iter().enumerate() { println!("mesh {i} {:?}: {} verts {} tris material {}", me.name, me.verts.len(), me.tris.len(), me.material); }
    for (i, a) in m.materials.iter().enumerate() { println!("material {i} {:?} tex {:?} color {:?} double {}", a.name, a.texture, a.color, a.double_sided); }
    for (i, t) in m.textures.iter().enumerate() { println!("texture {i} {:?} {}x{}", t.name, t.w, t.h); }
    if let Some(s) = &m.skeleton { for (i, b) in s.bones.iter().enumerate() { println!("bone {i} {:?} parent {:?} t {:?} r {:?}", b.name, b.parent, b.rest_t, b.rest_r); } }
    if let Some((lo, hi)) = m.bounds() { println!("bounds {lo:?} {hi:?}"); }
}
