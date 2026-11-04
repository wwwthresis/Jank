//! Data model: vertices, meshes, materials, skeleton, model.
use crate::math::*;
use crate::texture::Texture;

#[derive(Clone, Debug, PartialEq)]
pub struct Vertex {
    pub pos: Vec3,
    pub nrm: Vec3,
    pub uv: Vec2,
    pub color: [f32; 3],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}
impl Vertex {
    pub fn new(pos: Vec3, uv: Vec2) -> Self {
        Self { pos, nrm: Vec3::Y, uv, color: [1.0; 3], joints: [0; 4], weights: [0.0; 4] }
    }
    pub fn is_skinned(&self) -> bool {
        self.weights.iter().sum::<f32>() > 1e-6
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    pub name: String,
    pub verts: Vec<Vertex>,
    pub tris: Vec<[u32; 3]>,
    /// Index into `Model::materials`.
    pub material: usize,
}
impl Mesh {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), verts: vec![], tris: vec![], material: 0 }
    }
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.verts.iter();
        let first = it.next()?.pos;
        Some(it.fold((first, first), |(lo, hi), v| (lo.min(v.pos), hi.max(v.pos))))
    }
    /// Flat per-face normals, smoothed across shared vertices.
    pub fn recompute_normals(&mut self) {
        for v in &mut self.verts {
            v.nrm = Vec3::ZERO;
        }
        for t in &self.tris {
            let (a, b, c) = (self.verts[t[0] as usize].pos, self.verts[t[1] as usize].pos, self.verts[t[2] as usize].pos);
            let n = (b - a).cross(c - a); // length = 2 * area, so this weights by area
            for &i in t {
                self.verts[i as usize].nrm += n;
            }
        }
        for v in &mut self.verts {
            v.nrm = v.nrm.normalized();
        }
    }
    /// Removes degenerate triangles and unused vertices.
    pub fn cleanup(&mut self) {
        self.tris.retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
        let mut remap = vec![u32::MAX; self.verts.len()];
        let mut out = Vec::new();
        for t in &mut self.tris {
            for i in t.iter_mut() {
                let o = *i as usize;
                if remap[o] == u32::MAX {
                    remap[o] = out.len() as u32;
                    out.push(self.verts[o].clone());
                }
                *i = remap[o];
            }
        }
        self.verts = out;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    /// Index into `Model::textures`.
    pub texture: Option<usize>,
    pub color: [f32; 3],
    /// Unlit (like many PSX models with baked lighting).
    pub unlit: bool,
    pub double_sided: bool,
    /// Black/magenta acts as a colour-key transparency (as on PS1).
    pub color_key: bool,
}
impl Default for Material {
    fn default() -> Self {
        Self { name: "Material".into(), texture: None, color: [1.0; 3], unlit: false, double_sided: false, color_key: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub rest_t: Vec3,
    pub rest_r: Quat,
    pub rest_s: Vec3,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skeleton {
    /// Bones must be ordered so that a parent always precedes its children.
    pub bones: Vec<Bone>,
}
impl Skeleton {
    pub fn find(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }
    pub fn rest_local(&self, i: usize) -> Mat4 {
        let b = &self.bones[i];
        Mat4::from_trs(b.rest_t, b.rest_r, b.rest_s)
    }
    /// World matrices of the bones in the rest pose.
    pub fn rest_world(&self) -> Vec<Mat4> {
        let mut w: Vec<Mat4> = Vec::with_capacity(self.bones.len());
        for i in 0..self.bones.len() {
            let l = self.rest_local(i);
            w.push(match self.bones[i].parent {
                Some(p) => w[p] * l,
                None => l,
            });
        }
        w
    }
    pub fn inverse_bind(&self) -> Vec<Mat4> {
        self.rest_world().iter().map(|m| m.inverse().unwrap_or(Mat4::IDENTITY)).collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub name: String,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub skeleton: Option<Skeleton>,
}
impl Model {
    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(|m| m.tris.len()).sum()
    }
    pub fn vertex_count(&self) -> usize {
        self.meshes.iter().map(|m| m.verts.len()).sum()
    }
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        self.meshes.iter().filter_map(|m| m.bounds()).reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
    }
    /// Centre and radius of the bounding sphere, used for the auto camera.
    pub fn center_radius(&self) -> (Vec3, f32) {
        match self.bounds() {
            Some((lo, hi)) => ((lo + hi) * 0.5, ((hi - lo) * 0.5).length().max(1e-3)),
            None => (Vec3::ZERO, 1.0),
        }
    }
    /// Ensures at least one material exists so that mesh material indices stay valid.
    pub fn ensure_material(&mut self) {
        if self.materials.is_empty() {
            self.materials.push(Material::default());
        }
        let n = self.materials.len();
        for m in &mut self.meshes {
            if m.material >= n {
                m.material = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normals_and_cleanup() {
        let mut m = Mesh::new("q");
        for p in [(0., 0., 0.), (1., 0., 0.), (0., 1., 0.), (5., 5., 5.)] {
            m.verts.push(Vertex::new(Vec3::new(p.0, p.1, p.2), Vec2::default()));
        }
        m.tris.push([0, 1, 2]);
        m.tris.push([0, 0, 1]); // degenerate
        m.cleanup();
        assert_eq!(m.tris.len(), 1);
        assert_eq!(m.verts.len(), 3);
        m.recompute_normals();
        assert!((m.verts[0].nrm - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-5);
    }
}
