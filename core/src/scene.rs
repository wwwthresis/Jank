//! Scene: objects (references to assets), the camera, and the PSX style / mood settings.
use crate::math::*;
use crate::layers::Layer;
use crate::lighting::Lighting;
use crate::lights::{Light, LightPreset};
use crate::raster::{Camera, PsxStyle};

#[derive(Clone, Debug, PartialEq)]
pub struct Transform {
    pub pos: Vec3,
    /// Euler angles in degrees (XYZ order); friendlier for artists than quaternions.
    pub rot_deg: Vec3,
    pub scale: Vec3,
}
impl Default for Transform {
    fn default() -> Self {
        Self { pos: Vec3::ZERO, rot_deg: Vec3::ZERO, scale: Vec3::ONE }
    }
}
impl Transform {
    pub fn matrix(&self) -> Mat4 {
        let r = Vec3::new(self.rot_deg.x.to_radians(), self.rot_deg.y.to_radians(), self.rot_deg.z.to_radians());
        Mat4::from_trs(self.pos, Quat::from_euler(r), self.scale)
    }
    /// Component-wise linear interpolation of position, rotation (Euler degrees) and scale, used for
    /// tweening between frames. `t = 0` returns `self`, `t = 1` returns `o`. Plain Euler lerp is
    /// adequate for small rotations but does not take the shortest path for large ones.
    pub fn lerp(&self, o: &Transform, t: f32) -> Transform {
        Transform { pos: self.pos.lerp(o.pos, t), rot_deg: self.rot_deg.lerp(o.rot_deg, t), scale: self.scale.lerp(o.scale, t) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneObject {
    pub id: u32,
    pub name: String,
    /// Index into `Project::assets`.
    pub asset: usize,
    pub transform: Transform,
    pub tint: [f32; 3],
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub objects: Vec<SceneObject>,
    pub camera: Camera,
    pub style: PsxStyle,
    /// Pictures drawn into the scene, bottom first. The first `layers_back` are behind the 3D objects, the rest in front.
    pub layers: Vec<Layer>,
    pub layers_back: usize,
    pub lighting: Lighting,
    /// Reference pictures placed in 3D space to model by (never part of the final render).
    pub refs: Vec<RefImage>,
    /// Lamps: point and spot lights (see `lights.rs`).
    pub lights: Vec<Light>,
    next_id: u32,
}

/// A reference picture standing in the scene: a front or side drawing, a photo, a blueprint.
#[derive(Clone, Debug, PartialEq)]
pub struct RefImage {
    pub name: String,
    /// Shared, so undo snapshots of the scene do not copy the pixels.
    pub image: std::sync::Arc<crate::texture::Texture>,
    pub transform: Transform,
    /// Height of the picture in world units (the width follows its proportions).
    pub size: f32,
    /// 0..1
    pub opacity: f32,
    pub visible: bool,
    /// Locked pictures cannot be picked or moved by accident.
    pub locked: bool,
    /// Draw over the models instead of being hidden behind them.
    pub on_top: bool,
}
impl RefImage {
    pub fn new(name: &str, image: crate::texture::Texture) -> RefImage {
        RefImage { name: name.into(), image: std::sync::Arc::new(image), transform: Transform::default(), size: 2.0, opacity: 0.5, visible: true, locked: false, on_top: false }
    }
    /// The four corners in world space (top-left, top-right, bottom-right, bottom-left).
    pub fn corners(&self) -> [Vec3; 4] {
        let h = self.size * 0.5;
        let w = h * self.image.w.max(1) as f32 / self.image.h.max(1) as f32;
        let m = self.transform.matrix();
        [Vec3::new(-w, h, 0.0), Vec3::new(w, h, 0.0), Vec3::new(w, -h, 0.0), Vec3::new(-w, -h, 0.0)].map(|p| m.transform_point(p))
    }
}
impl Default for Scene {
    fn default() -> Self {
        Self { objects: vec![], camera: Camera::default(), style: PsxStyle::ps1_classic(), layers: vec![], layers_back: 0, lighting: Lighting::default(), refs: vec![], lights: vec![], next_id: 1 }
    }
}
impl Scene {
    /// A copy without the layer pictures (used by undo snapshots, which store layers separately).
    pub fn clone_without_layers(&self) -> Scene {
        Scene { objects: self.objects.clone(), camera: self.camera, style: self.style.clone(), layers: Vec::new(), layers_back: 0, lighting: self.lighting.clone(), refs: self.refs.clone(), lights: self.lights.clone(), next_id: self.next_id }
    }
    /// True if layer `i` is drawn behind the 3D objects.
    pub fn layer_is_back(&self, i: usize) -> bool {
        i < self.layers_back
    }
    /// Inserts a layer directly above layer `above` (same side of the 3D scene); `None` puts it on top of everything in front.
    pub fn insert_layer(&mut self, above: Option<usize>, layer: Layer) -> usize {
        let at = match above {
            Some(i) if i < self.layers.len() => i + 1,
            _ => self.layers.len(),
        };
        if at <= self.layers_back && above.is_some() {
            self.layers_back += 1;
        }
        self.layers.insert(at, layer);
        at
    }
    pub fn remove_layer(&mut self, i: usize) -> bool {
        if i >= self.layers.len() {
            return false;
        }
        if i < self.layers_back {
            self.layers_back -= 1;
        }
        self.layers.remove(i);
        true
    }
    /// Moves a layer up one step; passing the 3D scene moves it from behind to in front. Returns the new index.
    pub fn move_layer_up(&mut self, i: usize) -> Option<usize> {
        if i >= self.layers.len() {
            return None;
        }
        if i + 1 == self.layers_back {
            self.layers_back -= 1;
            return Some(i);
        }
        if i + 1 < self.layers.len() {
            self.layers.swap(i, i + 1);
            return Some(i + 1);
        }
        None
    }
    pub fn move_layer_down(&mut self, i: usize) -> Option<usize> {
        if i >= self.layers.len() {
            return None;
        }
        if i == self.layers_back {
            self.layers_back += 1;
            return Some(i);
        }
        if i > 0 {
            self.layers.swap(i - 1, i);
            return Some(i - 1);
        }
        None
    }
    pub fn add_object(&mut self, name: &str, asset: usize) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.objects.push(SceneObject { id, name: name.into(), asset, transform: Transform::default(), tint: [1.0; 3], visible: true });
        id
    }
    pub fn remove_object(&mut self, id: u32) -> bool {
        let Some(m) = self.object(id).map(|o| o.transform.matrix()) else { return false };
        self.objects.retain(|o| o.id != id);
        // lamps attached to the object stay where they were
        for l in self.lights.iter_mut().filter(|l| l.attach == Some(id)) {
            l.pos = m.transform_point(l.pos);
            l.attach = None;
        }
        true
    }
    pub fn object(&self, id: u32) -> Option<&SceneObject> {
        self.objects.iter().find(|o| o.id == id)
    }
    pub fn object_mut(&mut self, id: u32) -> Option<&mut SceneObject> {
        self.objects.iter_mut().find(|o| o.id == id)
    }
    pub fn next_id(&self) -> u32 {
        self.next_id
    }
    /// Adds a lamp from a preset and returns its id.
    pub fn add_light(&mut self, preset: LightPreset) -> u32 {
        let id = self.lights.iter().map(|l| l.id).max().unwrap_or(0) + 1;
        let mut l = Light::new(id, preset);
        let same = self.lights.iter().filter(|o| o.name.starts_with(preset.label())).count();
        if same > 0 {
            l.name = format!("{} {}", preset.label(), same + 1);
        }
        self.lights.push(l);
        id
    }
    pub fn light(&self, id: u32) -> Option<&Light> {
        self.lights.iter().find(|l| l.id == id)
    }
    pub fn light_mut(&mut self, id: u32) -> Option<&mut Light> {
        self.lights.iter_mut().find(|l| l.id == id)
    }
    pub fn remove_light(&mut self, id: u32) -> bool {
        let n = self.lights.len();
        self.lights.retain(|l| l.id != id);
        n != self.lights.len()
    }
    /// Restores the id counter when loading from a file. The counter never drops to or below the
    /// largest id already used by an object, so ids stay unique.
    pub fn set_next_id(&mut self, v: u32) {
        let max = self.objects.iter().map(|o| o.id).max().unwrap_or(0);
        self.next_id = v.max(max + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_stable_and_never_reused() {
        let mut s = Scene::default();
        let a = s.add_object("a", 0);
        let b = s.add_object("b", 0);
        assert!(s.remove_object(a));
        let c = s.add_object("c", 0);
        assert!(c > b && s.object(a).is_none());
        assert!(!s.remove_object(a));
    }
    #[test]
    fn scene_layer_order_across_the_3d_marker() {
        let mut s = Scene::default();
        let a = s.insert_layer(None, Layer::new("a", 4, 4)); // front
        assert_eq!((a, s.layers_back), (0, 0));
        s.layers_back = 1; // make "a" a back layer
        let b = s.insert_layer(Some(0), Layer::new("b", 4, 4)); // above a, same side (back)
        assert_eq!((b, s.layers_back), (1, 2));
        assert_eq!(s.move_layer_up(1), Some(1), "b crosses the 3D scene");
        assert_eq!(s.layers_back, 1);
        assert!(!s.layer_is_back(1));
        assert_eq!(s.move_layer_down(1), Some(1));
        assert_eq!(s.layers_back, 2);
        assert!(s.remove_layer(0));
        assert_eq!((s.layers.len(), s.layers_back), (1, 1));
    }
    #[test]
    fn transform_matrix_places_point() {
        let t = Transform { pos: Vec3::new(1.0, 2.0, 3.0), rot_deg: Vec3::new(0.0, 90.0, 0.0), scale: Vec3::ONE };
        let p = t.matrix().transform_point(Vec3::new(1.0, 0.0, 0.0));
        assert!((p - Vec3::new(1.0, 2.0, 2.0)).length() < 1e-4, "{p:?}");
    }
}
