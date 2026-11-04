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
    /// Timing of the tween into the next frame (edited in the graph editor).
    pub ease: Ease,
}

/// Timing curve of a tween: a cubic Bézier from (0, 0) to (1, 1) with two handles, as in CSS
/// `cubic-bezier(x1, y1, x2, y2)`. x is time, y is progress; y may overshoot (anticipation, bounce).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ease {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}
impl Default for Ease {
    fn default() -> Self {
        Ease::LINEAR
    }
}
impl Ease {
    pub const LINEAR: Ease = Ease { x1: 1.0 / 3.0, y1: 1.0 / 3.0, x2: 2.0 / 3.0, y2: 2.0 / 3.0 };
    pub const EASE_IN_OUT: Ease = Ease { x1: 0.42, y1: 0.0, x2: 0.58, y2: 1.0 };
    pub const EASE_IN: Ease = Ease { x1: 0.42, y1: 0.0, x2: 1.0, y2: 1.0 };
    pub const EASE_OUT: Ease = Ease { x1: 0.0, y1: 0.0, x2: 0.58, y2: 1.0 };
    pub const BACK: Ease = Ease { x1: 0.36, y1: -0.4, x2: 0.62, y2: 1.35 };
    pub fn is_linear(&self) -> bool {
        (self.x1 - self.y1).abs() < 1e-4 && (self.x2 - self.y2).abs() < 1e-4
    }
    /// Keeps the curve a function of time (handles stay within 0..1 horizontally).
    pub fn clamped(mut self) -> Ease {
        self.x1 = self.x1.clamp(0.0, 1.0);
        self.x2 = self.x2.clamp(0.0, 1.0);
        self.y1 = self.y1.clamp(-1.0, 2.0);
        self.y2 = self.y2.clamp(-1.0, 2.0);
        self
    }
    fn bez(a: f32, b: f32, s: f32) -> f32 {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    }
    /// Progress at time `t` (0..1).
    pub fn eval(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if self.is_linear() {
            return t;
        }
        // solve x(s) = t by bisection (x is monotonic because the handles stay in 0..1)
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..30 {
            let mid = (lo + hi) * 0.5;
            if Self::bez(self.x1, self.x2, mid) < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Self::bez(self.y1, self.y2, (lo + hi) * 0.5)
    }
}

/// One animated value shown in the graph editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    /// Object position (0..3 = X, Y, Z).
    Pos(usize),
    /// Object rotation in degrees.
    Rot(usize),
    Scale(usize),
    /// Bone rotation as Euler degrees: (bone, axis).
    BoneRot(usize, usize),
    /// Camera eye position.
    CamPos(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    /// Object id (ignored for camera channels).
    pub object: u32,
    pub kind: ChannelKind,
}

impl Channel {
    pub fn label(&self) -> String {
        let ax = |i: usize| ["X", "Y", "Z"][i.min(2)];
        match self.kind {
            ChannelKind::Pos(i) => format!("Location {}", ax(i)),
            ChannelKind::Rot(i) => format!("Rotation {}", ax(i)),
            ChannelKind::Scale(i) => format!("Scale {}", ax(i)),
            ChannelKind::BoneRot(b, i) => format!("Bone {b} rotation {}", ax(i)),
            ChannelKind::CamPos(i) => format!("Camera {}", ax(i)),
        }
    }
    /// The value of the channel in an object state (or camera).
    pub fn read(&self, st: Option<&ObjectState>, cam: &Camera) -> Option<f32> {
        let c = |v: crate::math::Vec3, i: usize| [v.x, v.y, v.z][i.min(2)];
        Some(match self.kind {
            ChannelKind::Pos(i) => c(st?.transform.pos, i),
            ChannelKind::Rot(i) => c(st?.transform.rot_deg, i),
            ChannelKind::Scale(i) => c(st?.transform.scale, i),
            ChannelKind::BoneRot(b, i) => c(crate::gizmo::euler_from_quat(st?.pose.bones.get(b)?.rot), i),
            ChannelKind::CamPos(i) => c(cam.eye, i),
        })
    }
    /// Writes the value into a frame.
    pub fn write(&self, f: &mut AnimFrame, v: f32) -> bool {
        let set = |p: &mut crate::math::Vec3, i: usize| match i {
            0 => p.x = v,
            1 => p.y = v,
            _ => p.z = v,
        };
        if let ChannelKind::CamPos(i) = self.kind {
            set(&mut f.camera.eye, i);
            return true;
        }
        let Some(st) = f.states.iter_mut().find(|s| s.object == self.object) else { return false };
        match self.kind {
            ChannelKind::Pos(i) => set(&mut st.transform.pos, i),
            ChannelKind::Rot(i) => set(&mut st.transform.rot_deg, i),
            ChannelKind::Scale(i) => set(&mut st.transform.scale, i),
            ChannelKind::BoneRot(b, i) => {
                let Some(bp) = st.pose.bones.get_mut(b) else { return false };
                let mut deg = crate::gizmo::euler_from_quat(bp.rot);
                set(&mut deg, i);
                bp.rot = crate::math::Quat::from_euler(crate::math::Vec3::new(deg.x.to_radians(), deg.y.to_radians(), deg.z.to_radians()));
            }
            ChannelKind::CamPos(_) => {}
        }
        true
    }
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
            frames: vec![AnimFrame { states, camera: scene.camera, style: None, hold: 1, tween: false, hand_layer: None, hand_only: false, note: String::new(), ease: Ease::LINEAR }],
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
        self.eval_f(tick as f32)
    }
    /// Like `locate`, for a fractional tick (used to draw smooth curves in the graph editor).
    pub fn locate_f(&self, tick: f32) -> (usize, f32) {
        let total = self.total_ticks().max(1) as f32;
        let mut t = tick.rem_euclid(total);
        for (i, f) in self.frames.iter().enumerate() {
            let hold = f.hold.max(1) as f32;
            if t < hold {
                return (i, t / hold);
            }
            t -= hold;
        }
        (self.frames.len().saturating_sub(1), 0.0)
    }
    /// The tick at which frame `i` starts.
    pub fn frame_start(&self, i: usize) -> u32 {
        self.frames.iter().take(i).map(|f| f.hold.max(1)).sum()
    }
    /// Channels that change somewhere in the animation (the ones worth showing in the graph editor).
    pub fn channels(&self) -> Vec<Channel> {
        let mut out = vec![];
        let Some(first) = self.frames.first() else { return out };
        let mut kinds = vec![];
        for i in 0..3 {
            kinds.push(ChannelKind::Pos(i));
        }
        for i in 0..3 {
            kinds.push(ChannelKind::Rot(i));
        }
        for i in 0..3 {
            kinds.push(ChannelKind::Scale(i));
        }
        for st in &first.states {
            let mut ks = kinds.clone();
            for b in 0..st.pose.bones.len() {
                for i in 0..3 {
                    ks.push(ChannelKind::BoneRot(b, i));
                }
            }
            for k in ks {
                let ch = Channel { object: st.object, kind: k };
                let vals: Vec<f32> = self.frames.iter().filter_map(|f| ch.read(f.states.iter().find(|s| s.object == st.object), &f.camera)).collect();
                if vals.iter().any(|v| (v - vals[0]).abs() > 1e-4) {
                    out.push(ch);
                }
            }
        }
        for i in 0..3 {
            let ch = Channel { object: 0, kind: ChannelKind::CamPos(i) };
            let vals: Vec<f32> = self.frames.iter().filter_map(|f| ch.read(None, &f.camera)).collect();
            if vals.iter().any(|v| (v - vals[0]).abs() > 1e-4) {
                out.push(ch);
            }
        }
        out
    }
    /// Value of a channel at a fractional tick, following tweens and their easing.
    pub fn sample(&self, ch: &Channel, tick: f32) -> Option<f32> {
        let ev = self.eval_f(tick)?;
        ch.read(ev.states.iter().find(|s| s.object == ch.object), &ev.camera)
    }
    /// The scene state at a fractional tick.
    pub fn eval_f(&self, tick: f32) -> Option<Evaluated> {
        let (i, t) = self.locate_f(tick);
        let a = self.frames.get(i)?;
        let next = self.frames.get(i + 1);
        let t = a.ease.eval(t);
        match (a.tween, next) {
            (true, Some(b)) if t != 0.0 => {
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
