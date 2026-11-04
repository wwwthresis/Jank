//! Frame-by-frame animation: every frame stores the complete scene state (as in stop-motion),
//! can be held for several ticks, can optionally blend smoothly into the next frame (tween),
//! and carries a character emotion (a texture swap), a hand-drawn layer on top, and its own camera.
use crate::pose::Pose;
use crate::raster::{Camera, PsxStyle};
use crate::scene::{Scene, Transform};
use crate::texture::Texture;

/// A named emotion: a ready-made variant of a model texture (face, eyes, blush and so on).
#[derive(Clone, Debug, PartialEq)]
pub struct Emotion {
    pub name: String,
    /// Which texture of the model this emotion replaces.
    pub texture: usize,
    pub image: Texture,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectState {
    pub object: u32,
    pub transform: Transform,
    pub pose: Pose,
    pub visible: bool,
    /// Index into `Asset::emotions`.
    pub emotion: Option<usize>,
    /// One-off repaint of a texture in this frame only: (texture index, image). It takes precedence over the emotion.
    pub tex_patch: Vec<(usize, Texture)>,
}
impl ObjectState {
    pub fn base(object: u32, transform: Transform, bones: usize) -> Self {
        Self { object, transform, pose: Pose::rest(bones), visible: true, emotion: None, tex_patch: vec![] }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimFrame {
    pub states: Vec<ObjectState>,
    pub camera: Camera,
    /// Style override for this frame (a light flicker, a flash and so on).
    pub style: Option<PsxStyle>,
    /// How many ticks (output frames) the frame is held, at least 1.
    pub hold: u32,
    /// Blend smoothly into the next frame over the `hold` duration.
    pub tween: bool,
    /// A hand-drawn RGBA layer that is composited on top of the 3D image.
    pub hand_layer: Option<Texture>,
    /// Show only the drawing (hide the 3D image): fully hand-drawn frame-by-frame animation.
    pub hand_only: bool,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub name: String,
    pub fps_num: u32,
    pub fps_den: u32,
    pub frames: Vec<AnimFrame>,
}

/// The result of evaluating the animation at tick `t`.
pub struct Evaluated {
    pub frame_index: usize,
    pub states: Vec<ObjectState>,
    pub camera: Camera,
}

impl Animation {
    /// A new animation with a single frame captured from the scene. `bones_of(asset)` returns the bone count of an asset.
    pub fn from_scene(name: &str, scene: &Scene, bones_of: &dyn Fn(usize) -> usize) -> Animation {
        let states = scene.objects.iter().map(|o| ObjectState::base(o.id, o.transform.clone(), bones_of(o.asset))).collect();
        Animation {
            name: name.into(),
            fps_num: 12,
            fps_den: 1,
            frames: vec![AnimFrame { states, camera: scene.camera, style: None, hold: 1, tween: false, hand_layer: None, hand_only: false, note: String::new() }],
        }
    }
    pub fn fps(&self) -> f32 {
        self.fps_num as f32 / self.fps_den.max(1) as f32
    }
    pub fn total_ticks(&self) -> u32 {
        self.frames.iter().map(|f| f.hold.max(1)).sum()
    }
    pub fn duration_secs(&self) -> f32 {
        self.total_ticks() as f32 / self.fps().max(1e-3)
    }
    /// Duplicates frame `i` and inserts the copy right after it. Returns the index of the copy.
    pub fn duplicate_frame(&mut self, i: usize) -> Option<usize> {
        let f = self.frames.get(i)?.clone();
        self.frames.insert(i + 1, f);
        Some(i + 1)
    }
    pub fn delete_frame(&mut self, i: usize) -> bool {
        if self.frames.len() <= 1 || i >= self.frames.len() {
            return false; // the last frame cannot be deleted
        }
        self.frames.remove(i);
        true
    }
    pub fn move_frame(&mut self, from: usize, to: usize) -> bool {
        if from >= self.frames.len() || to >= self.frames.len() {
            return false;
        }
        let f = self.frames.remove(from);
        self.frames.insert(to, f);
        true
    }
    /// Returns (frame index, fraction 0..1 within its hold) for a tick. A tick past the end wraps around.
    pub fn locate(&self, tick: u32) -> (usize, f32) {
        let total = self.total_ticks().max(1);
        let mut t = tick % total;
        for (i, f) in self.frames.iter().enumerate() {
            let hold = f.hold.max(1);
            if t < hold {
                return (i, t as f32 / hold as f32);
            }
            t -= hold;
        }
        (self.frames.len().saturating_sub(1), 0.0)
    }
    /// The scene state at tick `tick`, taking tweening into account.
    pub fn eval(&self, tick: u32) -> Option<Evaluated> {
        let (i, t) = self.locate(tick);
        let a = self.frames.get(i)?;
        let next = self.frames.get(i + 1);
        match (a.tween, next) {
            (true, Some(b)) if t > 0.0 => {
                let states = a
                    .states
                    .iter()
                    .map(|sa| match b.states.iter().find(|sb| sb.object == sa.object) {
                        Some(sb) => ObjectState {
                            object: sa.object,
                            transform: sa.transform.lerp(&sb.transform, t),
                            pose: if sa.pose.bones.len() == sb.pose.bones.len() { sa.pose.lerp(&sb.pose, t) } else { sa.pose.clone() },
                            visible: sa.visible,
                            emotion: sa.emotion, // emotions and repaints are never blended: they switch per frame
                            tex_patch: sa.tex_patch.clone(),
                        },
                        None => sa.clone(),
                    })
                    .collect();
                let camera = Camera {
                    eye: a.camera.eye.lerp(b.camera.eye, t),
                    target: a.camera.target.lerp(b.camera.target, t),
                    fov_y: a.camera.fov_y + (b.camera.fov_y - a.camera.fov_y) * t,
                    ..a.camera
                };
                Some(Evaluated { frame_index: i, states, camera })
            }
            _ => Some(Evaluated { frame_index: i, states: a.states.clone(), camera: a.camera }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::*;
    fn anim3() -> Animation {
        let mut s = Scene::default();
        let id = s.add_object("hero", 0);
        let mut a = Animation::from_scene("walk", &s, &|_| 2);
        a.frames[0].hold = 2;
        let mut f2 = a.frames[0].clone();
        f2.hold = 3;
        f2.states[0].transform.pos = Vec3::new(10.0, 0.0, 0.0);
        a.frames.push(f2);
        assert_eq!(a.frames[0].states[0].object, id);
        a
    }
    #[test]
    fn timeline_math() {
        let a = anim3();
        assert_eq!(a.total_ticks(), 5);
        assert_eq!(a.locate(0).0, 0);
        assert_eq!(a.locate(1).0, 0);
        assert_eq!(a.locate(2).0, 1);
        assert_eq!(a.locate(4).0, 1);
        assert_eq!(a.locate(5).0, 0, "looping");
    }
    #[test]
    fn stepped_vs_tween() {
        let mut a = anim3();
        let step = a.eval(1).unwrap();
        assert_eq!(step.states[0].transform.pos.x, 0.0, "without tween the value steps");
        a.frames[0].tween = true;
        let mid = a.eval(1).unwrap(); // t = 0.5 within hold = 2
        assert!((mid.states[0].transform.pos.x - 5.0).abs() < 1e-4);
    }
    #[test]
    fn frame_editing() {
        let mut a = anim3();
        assert_eq!(a.duplicate_frame(0), Some(1));
        assert_eq!(a.frames.len(), 3);
        assert!(a.move_frame(0, 2));
        assert!(a.delete_frame(0));
        let mut one = Animation { frames: vec![a.frames[0].clone()], ..a.clone() };
        assert!(!one.delete_frame(0), "the last frame is protected");
    }
    #[test]
    fn fps_and_duration() {
        let mut a = anim3();
        a.fps_num = 24000;
        a.fps_den = 1001;
        assert!((a.fps() - 23.976).abs() < 0.01);
        assert!(a.duration_secs() > 0.2);
    }
}
