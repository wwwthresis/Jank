//! The Jank! editor core: all state and all logic, independent of any GUI.
//! The interface only calls the methods defined here and displays the result, so everything can be tested without a window.
use crate::anim::*;
use crate::compose::{hit_object_id, render_states, scene_states, FrameSpec};
use crate::decimate;
use crate::layers::{Layer, LayerStack};
use crate::math::*;
use crate::mesh::*;
use crate::modeling;
use crate::paint::{self, Brush, Tool};
use crate::pose::*;
use crate::project::*;
use crate::raster::*;
use crate::scene::*;
use crate::selection::{Clip, Combine, Selection};
use crate::texture::*;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ modes and the viewport camera

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Model,
    Texture,
    Pose,
    Animate,
    Render,
}

/// Where painting on the model goes in Animate mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintLayer {
    /// Into the model's own texture (affects every frame).
    Base,
    /// Into this frame only (a one-off repaint, for example a blink).
    FramePatch,
    /// Into the image of the selected emotion (affects every frame that uses it).
    Emotion,
}

/// The free orbit camera of the viewport (not to be confused with a frame's camera).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewCam {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
}
impl Default for ViewCam {
    fn default() -> Self {
        Self { target: Vec3::ZERO, yaw: 0.6, pitch: 0.35, dist: 5.0 }
    }
}
impl ViewCam {
    pub fn camera(&self) -> Camera {
        Camera::orbit(self.target, self.yaw, self.pitch, self.dist)
    }
    pub fn from_camera(c: &Camera) -> ViewCam {
        let d = c.eye - c.target;
        let dist = d.length().max(0.01);
        ViewCam { target: c.target, yaw: d.x.atan2(d.z), pitch: (d.y / dist).clamp(-1.0, 1.0).asin(), dist }
    }
    /// Orbits by a mouse movement given in pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.01;
        self.pitch = (self.pitch + dy * 0.01).clamp(-1.55, 1.55);
    }
    /// Mouse wheel: a positive value zooms in.
    pub fn zoom(&mut self, wheel: f32) {
        self.dist = (self.dist * (-wheel * 0.1).exp()).clamp(0.05, 500.0);
    }
    /// Pans in the screen plane; `view_h` is the viewport height in pixels.
    pub fn pan(&mut self, dx: f32, dy: f32, view_h: f32) {
        let cam = self.camera();
        let fwd = (cam.target - cam.eye).normalized();
        let right = fwd.cross(Vec3::Y).normalized();
        let up = right.cross(fwd);
        let k = 2.0 * self.dist * (cam.fov_y * 0.5).tan() / view_h.max(1.0);
        self.target = self.target + right * (-dx * k) + up * (dy * k);
    }
    /// Frames the bounding box `lo..hi` so that it fills the view.
    pub fn frame(&mut self, lo: Vec3, hi: Vec3) {
        self.target = (lo + hi) * 0.5;
        let r = ((hi - lo).length() * 0.5).max(0.05);
        self.dist = (r / (0.9f32 * 0.5).tan() * 1.15).clamp(0.05, 500.0);
    }
}

// ------------------------------------------------------------------ undo/redo

/// What an undo snapshot holds: the address of a "slot" of the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Texture(usize, usize),
    EmotionImg(usize, usize),
    Emotions(usize),
    Model(usize),
    /// The scene without its layers (objects, camera, style).
    Scene,
    /// The picture of one scene layer.
    SceneLayer(usize),
    /// The list of scene layers with their order and settings (and pictures).
    SceneLayers,
    /// A texture together with its layer stack.
    TexStack(usize, usize),
    Anim(usize),
    Frame(usize, usize),
    Whole,
    /// The triangle meshes of a model together with its Edit-mode polygons.
    Mesh(usize),
    /// The indexed-colour data (palettes) of an asset's textures.
    Indexed(usize),
}
enum Data {
    SceneLayers(Vec<Layer>, usize),
    TexStack(Box<(Option<LayerStack>, Texture)>),
    Texture(Texture),
    Emotions(Vec<Emotion>),
    Model(Box<Model>),
    Scene(Box<Scene>),
    Anim(Box<Animation>),
    Frame(Box<AnimFrame>),
    Whole(Box<Project>),
    Mesh(Box<(Vec<Mesh>, Option<Box<(u64, crate::editmesh::EditMesh)>>)>),
    Indexed(Vec<(usize, crate::palette::Indexed)>),
}
struct Entry {
    group: u64,
    slot: Slot,
    data: Data,
}
const UNDO_LIMIT: usize = 200;
/// Upper bound for the memory held by the undo and redo history (pictures dominate it).
const UNDO_BYTES: usize = 256 << 20;

impl Data {
    fn bytes(&self) -> usize {
        let tex = |t: &Texture| t.px.len() * 4;
        match self {
            Data::Texture(t) => tex(t),
            Data::TexStack(b) => tex(&b.1) + b.0.as_ref().map_or(0, |s| s.layers.iter().map(|l| tex(&l.img)).sum()),
            Data::SceneLayers(l, _) => l.iter().map(|l| tex(&l.img)).sum(),
            Data::Emotions(v) => v.iter().map(|e| tex(&e.image)).sum(),
            Data::Model(m) => m.textures.iter().map(tex).sum::<usize>() + 4096,
            Data::Mesh(m) => m.0.iter().map(|x| x.verts.len() * 72 + x.tris.len() * 12).sum::<usize>() + m.1.as_ref().map_or(0, |t| t.1.verts.len() * 48 + t.1.faces.len() * 120) + 1024,
            Data::Frame(f) => f.hand_layer.as_ref().map_or(0, tex) + f.states.iter().map(|s| s.tex_patch.iter().map(|(_, t)| tex(t)).sum::<usize>()).sum::<usize>() + 1024,
            Data::Indexed(v) => v.iter().map(|(_, x)| x.idx.len()).sum::<usize>() + 1024,
            _ => 4096,
        }
    }
}

/// A canvas that can be painted on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Canvas {
    Base { asset: usize, tex: usize },
    Emotion { asset: usize, idx: usize },
    Patch { anim: usize, frame: usize, object: u32, tex: usize },
    /// The hand-drawn layer of a frame (in render pixels).
    Hand { anim: usize, frame: usize },
    /// A layer of the scene picture (drawn behind or in front of the 3D objects).
    Scene { layer: usize },
}

/// What is under the cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickInfo {
    pub object: u32,
    pub asset: usize,
    pub mesh: usize,
    pub tri: usize,
    pub tex: Option<usize>,
    pub u: f32,
    pub v: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenBone {
    pub bone: usize,
    pub head: (f32, f32),
    pub tail: (f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    Cube,
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    /// A ready-made textured model (geometry and picture are built into the program).
    Butterfly,
}

/// Operations that act on a whole texture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TexOp {
    FlipH,
    FlipV,
    Offset(i32, i32),
    Resize(u32, u32),
    Quantize15,
    Dither(u32),
    ReducePalette(usize),
    Hsv(f32, f32, f32),
    Fill(Rgba),
    ReplaceColor(Rgba, Rgba, u32),
}

pub struct Editor {
    pub project: Project,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub mode: Mode,
    pub view: ViewCam,
    /// Show the viewport with the retro look (otherwise a clean low-poly view without noise, easier for modeling).
    pub psx_preview: bool,
    /// In Pose and Animate modes, look through the frame's camera.
    pub through_camera: bool,
    pub selected_object: Option<u32>,
    pub selected_bone: Option<usize>,
    pub selected_faces: BTreeSet<(usize, usize)>,
    pub brush: Brush,
    pub paint_layer: PaintLayer,
    pub active_texture: usize,
    pub active_emotion: Option<usize>,
    pub current_anim: usize,
    pub current_frame: usize,
    pub playing: bool,
    pub looping: bool,
    pub tick: u32,
    pub onion: bool,
    pub status: String,
    pub clip_pose: Option<Pose>,
    /// Pixel selection of the 2D editor and the canvas it belongs to.
    pub selection: Option<Selection>,
    pub selection_canvas: Option<Canvas>,
    /// Pixels copied with `selection_copy`.
    pub pixel_clip: Option<Clip>,
    /// The scene layer that is painted on.
    pub active_layer: usize,
    play_accum: f32,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    next_group: u64,
    open_group: Option<u64>,
    touched: Vec<Slot>,
    stroke_last: Option<(Canvas, (f32, f32))>,
    shape: Option<(Canvas, (f32, f32), Texture)>,
    sculpt_target: Option<(usize, usize, usize)>,
    /// Edit mode (polygon editing of one object), see `editmode`.
    pub edit: Option<EditSession>,
    /// Inverse kinematics while dragging bones: how many bones above the dragged one follow (0 = off).
    pub ik_chain: usize,
    /// The selected reference picture.
    pub selected_ref: Option<usize>,
    /// Paint on the model through the screen (a round brush on the visible surface) instead of through the UV map.
    pub project_paint: bool,
    proj: Option<ProjStroke>,
}

#[path = "editor/editmode.rs"]
mod editmode;
pub use editmode::*;
#[path = "editor/extras.rs"]
mod extras;
pub use extras::*;

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

fn pkey(p: Vec3) -> (i64, i64, i64) {
    ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64, (p.z * 1e4).round() as i64)
}
fn conj(q: Quat) -> Quat {
    Quat { x: -q.x, y: -q.y, z: -q.z, w: q.w }
}
fn rot_of(t: &Transform) -> Quat {
    Quat::from_euler(Vec3::new(t.rot_deg.x.to_radians(), t.rot_deg.y.to_radians(), t.rot_deg.z.to_radians()))
}

impl Editor {
    pub fn new() -> Self {
        Self {
            project: Project::new("Untitled"),
            path: None,
            dirty: false,
            mode: Mode::Model,
            view: ViewCam::default(),
            psx_preview: true,
            through_camera: false,
            selected_object: None,
            selected_bone: None,
            selected_faces: BTreeSet::new(),
            brush: Brush::default(),
            paint_layer: PaintLayer::FramePatch,
            active_texture: 0,
            active_emotion: None,
            current_anim: 0,
            current_frame: 0,
            playing: false,
            looping: true,
            tick: 0,
            onion: true,
            status: String::new(),
            clip_pose: None,
            selection: None,
            selection_canvas: None,
            pixel_clip: None,
            active_layer: 0,
            play_accum: 0.0,
            undo: vec![],
            redo: vec![],
            next_group: 1,
            open_group: None,
            touched: vec![],
            stroke_last: None,
            shape: None,
            sculpt_target: None,
            edit: None,
            ik_chain: 0,
            selected_ref: None,
            project_paint: false,
            proj: None,
        }
    }

    pub fn title(&self) -> String {
        format!("{}{} — Jank!", self.project.name, if self.dirty { " •" } else { "" })
    }

    // -------------------------------------------------------------- undo / redo

    fn read_slot(&self, s: Slot) -> Option<Data> {
        let p = &self.project;
        Some(match s {
            Slot::Texture(a, t) => Data::Texture(p.assets.get(a)?.model.textures.get(t)?.clone()),
            Slot::EmotionImg(a, i) => Data::Texture(p.assets.get(a)?.emotions.get(i)?.image.clone()),
            Slot::Emotions(a) => Data::Emotions(p.assets.get(a)?.emotions.clone()),
            Slot::Model(a) => Data::Model(Box::new(p.assets.get(a)?.model.clone())),
            Slot::Scene => Data::Scene(Box::new(p.scene.clone_without_layers())),
            Slot::SceneLayer(i) => Data::Texture(p.scene.layers.get(i)?.img.clone()),
            Slot::SceneLayers => Data::SceneLayers(p.scene.layers.clone(), p.scene.layers_back),
            Slot::TexStack(a, t) => {
                let asset = p.assets.get(a)?;
                Data::TexStack(Box::new((asset.stack(t).cloned(), asset.model.textures.get(t)?.clone())))
            }
            Slot::Anim(i) => Data::Anim(Box::new(p.animations.get(i)?.clone())),
            Slot::Frame(a, f) => Data::Frame(Box::new(p.animations.get(a)?.frames.get(f)?.clone())),
            Slot::Whole => Data::Whole(Box::new(p.clone())),
            Slot::Mesh(a) => {
                let asset = p.assets.get(a)?;
                Data::Mesh(Box::new((asset.model.meshes.clone(), asset.topology.clone())))
            }
            Slot::Indexed(a) => Data::Indexed(p.assets.get(a)?.indexed.clone()),
        })
    }
    fn write_slot(&mut self, s: Slot, d: Data) {
        let p = &mut self.project;
        match (s, d) {
            (Slot::Texture(a, t), Data::Texture(x)) => {
                if let Some(d) = p.assets.get_mut(a).and_then(|a| a.model.textures.get_mut(t)) {
                    *d = x;
                }
            }
            (Slot::EmotionImg(a, i), Data::Texture(x)) => {
                if let Some(e) = p.assets.get_mut(a).and_then(|a| a.emotions.get_mut(i)) {
                    e.image = x;
                }
            }
            (Slot::Emotions(a), Data::Emotions(x)) => {
                if let Some(d) = p.assets.get_mut(a) {
                    d.emotions = x;
                }
            }
            (Slot::Model(a), Data::Model(x)) => {
                if let Some(d) = p.assets.get_mut(a) {
                    d.model = *x;
                }
            }
            (Slot::Scene, Data::Scene(x)) => {
                let (layers, back) = (std::mem::take(&mut p.scene.layers), p.scene.layers_back);
                p.scene = *x;
                p.scene.layers = layers;
                p.scene.layers_back = back;
            }
            (Slot::SceneLayer(i), Data::Texture(x)) => {
                if let Some(l) = p.scene.layers.get_mut(i) {
                    l.img = x;
                }
            }
            (Slot::SceneLayers, Data::SceneLayers(l, back)) => {
                p.scene.layers = l;
                p.scene.layers_back = back;
                self.active_layer = self.active_layer.min(self.project.scene.layers.len().saturating_sub(1));
            }
            (Slot::TexStack(a, t), Data::TexStack(b)) => {
                let (stack, tex) = *b;
                if let Some(asset) = p.assets.get_mut(a) {
                    asset.layers.retain(|(i, _)| *i != t);
                    if let Some(s) = stack {
                        asset.layers.push((t, s));
                    }
                    if let Some(d) = asset.model.textures.get_mut(t) {
                        *d = tex;
                    }
                }
            }
            (Slot::Anim(i), Data::Anim(x)) => {
                if let Some(d) = p.animations.get_mut(i) {
                    *d = *x;
                }
            }
            (Slot::Frame(a, f), Data::Frame(x)) => {
                if let Some(d) = p.animations.get_mut(a).and_then(|a| a.frames.get_mut(f)) {
                    *d = *x;
                }
            }
            (Slot::Whole, Data::Whole(x)) => *p = *x,
            (Slot::Mesh(a), Data::Mesh(x)) => {
                if let Some(d) = p.assets.get_mut(a) {
                    let (meshes, topo) = *x;
                    d.model.meshes = meshes;
                    d.topology = topo;
                }
            }
            (Slot::Indexed(a), Data::Indexed(x)) => {
                if let Some(d) = p.assets.get_mut(a) {
                    d.indexed = x;
                }
            }
            _ => {}
        }
        self.clamp_cursor();
    }
    /// Stores the state of a slot BEFORE it is changed. Inside an undo group each slot is stored only once.
    fn record(&mut self, slot: Slot) {
        if self.open_group.is_some() && self.touched.contains(&slot) {
            return;
        }
        if let Some(data) = self.read_slot(slot) {
            self.push_entry(slot, data);
        }
    }
    fn push_entry(&mut self, slot: Slot, data: Data) {
        if self.open_group.is_some() && self.touched.contains(&slot) {
            return;
        }
        let group = match self.open_group {
            Some(g) => g,
            None => {
                self.next_group += 1;
                self.next_group
            }
        };
        if self.open_group.is_some() {
            self.touched.push(slot);
        }
        self.undo.push(Entry { group, slot, data });
        let mut bytes: usize = self.undo.iter().map(|e| e.data.bytes()).sum();
        while self.undo.len() > UNDO_LIMIT || (bytes > UNDO_BYTES && self.undo.len() > 1) {
            let old = self.undo.remove(0);
            bytes = bytes.saturating_sub(old.data.bytes());
        }
        self.redo.clear();
        self.dirty = true;
    }
    /// Everything recorded between `begin_group` and `end_group` is undone as a single step (a brush stroke, a drag).
    pub fn begin_group(&mut self) {
        if self.open_group.is_none() {
            self.next_group += 1;
            self.open_group = Some(self.next_group);
            self.touched.clear();
        }
    }
    pub fn end_group(&mut self) {
        if self.open_group.is_some() && !self.touched.is_empty() {
            self.sync_indexed_touched();
        }
        self.proj = None;
        self.open_group = None;
        self.touched.clear();
        self.stroke_last = None;
        self.shape = None;
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self) -> bool {
        self.step(true)
    }
    pub fn redo(&mut self) -> bool {
        self.step(false)
    }
    fn step(&mut self, undo: bool) -> bool {
        self.end_group();
        let mut src = std::mem::take(if undo { &mut self.undo } else { &mut self.redo });
        let Some(g) = src.last().map(|e| e.group) else {
            *(if undo { &mut self.undo } else { &mut self.redo }) = src;
            return false;
        };
        let mut batch = vec![];
        while src.last().is_some_and(|e| e.group == g) {
            batch.push(src.pop().unwrap());
        }
        *(if undo { &mut self.undo } else { &mut self.redo }) = src;
        let mut inverse = vec![];
        for e in batch {
            if let Some(cur) = self.read_slot(e.slot) {
                inverse.push(Entry { group: g, slot: e.slot, data: cur });
            }
            self.write_slot(e.slot, e.data);
        }
        // reverse order, so that redo walks through the same steps in the opposite direction
        inverse.reverse();
        let dst = if undo { &mut self.redo } else { &mut self.undo };
        dst.extend(inverse);
        self.dirty = true;
        self.edit_resync();
        true
    }

    // -------------------------------------------------------------- project: files

    pub fn new_project(&mut self, name: &str) {
        *self = Editor::new();
        self.project.name = name.to_string();
    }
    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        if self.edit.as_ref().is_some_and(|s| s.drag.is_some()) {
            self.edit_drag_end(true);
        }
        self.project.save(path).map_err(|e| format!("Could not save: {e}"))?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.status = format!("Saved: {}", path.display());
        Ok(())
    }
    pub fn save(&mut self) -> Result<(), String> {
        let p = self.path.clone().ok_or("This project has no file yet: choose Save As")?;
        self.save_as(&p)
    }
    pub fn open(path: &Path) -> Result<Editor, String> {
        let project = Project::load(path).map_err(|e| format!("Could not open \"{}\": {e}", path.display()))?;
        Ok(Editor::with_project(project, Some(path.to_path_buf())))
    }
    /// Opens a project held in memory, such as the starter scene built into the program. It has no file until it is saved.
    pub fn open_bytes(bytes: &[u8]) -> Result<Editor, String> {
        let project = Project::from_bytes(bytes).map_err(|e| format!("Could not open the project: {e}"))?;
        Ok(Editor::with_project(project, None))
    }
    fn with_project(project: Project, path: Option<std::path::PathBuf>) -> Editor {
        let mut ed = Editor::new();
        ed.project = project;
        ed.path = path;
        // a rigged object is the likeliest thing to work on; otherwise the first one
        let rigged = ed.project.scene.objects.iter().find(|o| ed.project.assets.get(o.asset).is_some_and(|a| a.model.skeleton.is_some())).map(|o| o.id);
        ed.selected_object = rigged.or_else(|| ed.project.scene.objects.first().map(|o| o.id));
        ed.frame_all();
        // a project with a camera set up for its first frame opens looking through it, so it appears the way it was composed
        if let Some(cam) = ed.project.animations.first().and_then(|a| a.frames.first()).map(|f| f.camera).filter(|c| *c != Camera::default()) {
            ed.view = ViewCam::from_camera(&cam);
        }
        ed
    }

    // -------------------------------------------------------------- mode and selection

    pub fn set_mode(&mut self, mode: Mode) {
        if mode != Mode::Model && self.edit.is_some() {
            self.edit_exit();
        }
        self.end_group();
        self.mode = mode;
        self.playing = false;
        if matches!(mode, Mode::Pose | Mode::Animate | Mode::Render) {
            self.ensure_animation();
        }
        if mode == Mode::Render {
            self.through_camera = true;
        }
    }
    pub fn select_object(&mut self, id: Option<u32>) {
        if self.selected_object != id {
            self.selected_bone = None;
            self.selected_faces.clear();
            self.active_texture = 0;
            self.active_emotion = None;
        }
        self.selected_object = id;
    }
    pub fn selected_asset(&self) -> Option<usize> {
        self.project.scene.object(self.selected_object?).map(|o| o.asset)
    }
    pub fn selected_model(&self) -> Option<&Model> {
        self.project.assets.get(self.selected_asset()?).map(|a| &a.model)
    }
    fn object_for_hit(&self, hit: Hit) -> Option<&SceneObject> {
        self.project.scene.objects.iter().find(|o| hit_object_id(o.id) == hit.obj)
    }
    pub fn pick_info(&self, hit: Hit) -> Option<PickInfo> {
        if !hit.is_some() {
            return None;
        }
        let o = self.object_for_hit(hit)?;
        let model = &self.project.assets.get(o.asset)?.model;
        let mesh = model.meshes.get(hit.mesh as usize)?;
        let tex = model.materials.get(mesh.material).and_then(|m| m.texture).filter(|&t| t < model.textures.len());
        Some(PickInfo { object: o.id, asset: o.asset, mesh: hit.mesh as usize, tri: hit.tri as usize, tex, u: hit.u, v: hit.v })
    }
    /// A click on the model in the viewport: selects the object (and the face in Model mode).
    pub fn click_select(&mut self, frame: &Frame, x: i32, y: i32, additive: bool) {
        let hit = frame.hit_at(x, y);
        match self.pick_info(hit) {
            Some(info) => {
                if self.selected_object != Some(info.object) {
                    self.select_object(Some(info.object));
                }
                if self.mode == Mode::Model {
                    let key = (info.mesh, info.tri);
                    if additive {
                        if !self.selected_faces.remove(&key) {
                            self.selected_faces.insert(key);
                        }
                    } else {
                        self.selected_faces.clear();
                        self.selected_faces.insert(key);
                    }
                }
                if let Some(t) = info.tex {
                    self.active_texture = t;
                }
            }
            None if !additive => {
                self.selected_faces.clear();
            }
            None => {}
        }
    }

    // -------------------------------------------------------------- viewport camera and rendering

    fn clamp_cursor(&mut self) {
        if self.current_anim >= self.project.animations.len() {
            self.current_anim = self.project.animations.len().saturating_sub(1);
        }
        let n = self.project.animations.get(self.current_anim).map_or(0, |a| a.frames.len());
        if self.current_frame >= n {
            self.current_frame = n.saturating_sub(1);
        }
        if self.selected_object.is_some_and(|id| self.project.scene.object(id).is_none()) {
            self.selected_object = None;
            self.selected_bone = None;
        }
        self.selected_faces.clear();
    }
    pub fn animation(&self) -> Option<&Animation> {
        self.project.animations.get(self.current_anim)
    }
    pub fn frame(&self) -> Option<&AnimFrame> {
        self.animation()?.frames.get(self.current_frame)
    }
    fn animating(&self) -> bool {
        matches!(self.mode, Mode::Pose | Mode::Animate | Mode::Render) && self.animation().is_some_and(|a| !a.frames.is_empty())
    }
    /// The camera the viewport is currently looking through.
    pub fn active_camera(&self) -> Camera {
        if self.animating() && self.through_camera {
            if self.playing {
                if let Some(ev) = self.animation().and_then(|a| a.eval(self.tick)) {
                    return ev.camera;
                }
            }
            if let Some(f) = self.frame() {
                return f.camera;
            }
        }
        self.view.camera()
    }
    pub fn viewport_style(&self) -> PsxStyle {
        let base = self.frame().filter(|_| self.animating()).and_then(|f| f.style.clone()).unwrap_or_else(|| self.project.scene.style.clone());
        if (!self.psx_preview || self.edit.is_some()) && matches!(self.mode, Mode::Model | Mode::Texture | Mode::Pose) {
            let mut s = PsxStyle::clean_lowpoly();
            s.width = base.width;
            s.height = base.height;
            return s;
        }
        base
    }
    /// Renders the viewport into `r` at the render resolution of the style. Afterwards `r.frame.hit_at` can be used for picking and painting.
    pub fn render_viewport(&self, r: &mut Renderer) {
        let style = self.viewport_style();
        let cam = self.active_camera();
        self.render_core(r, cam, &style);
        self.draw_refs(r, &cam);
    }
    /// Renders the main viewport at an arbitrary pixel size (the size of the widget on screen), independent of the
    /// project's render resolution. Vertex snapping scales with the width so the retro wobble keeps the same look.
    pub fn render_viewport_sized(&self, r: &mut Renderer, w: u32, h: u32) {
        let mut style = self.viewport_style();
        style.snap *= w as f32 / style.width.max(1) as f32;
        let aspect = style.width as f32 / style.height.max(1) as f32;
        style.width = w.max(1);
        style.height = h.max(1);
        r.layer_aspect = Some(aspect);
        let cam = self.active_camera();
        self.render_core(r, cam, &style);
        r.layer_aspect = None;
        self.draw_refs(r, &cam);
    }
    /// The camera the final picture is taken with (not the editor's free camera).
    pub fn final_camera(&self) -> Camera {
        if self.animating() {
            if self.playing {
                if let Some(ev) = self.animation().and_then(|a| a.eval(self.tick)) {
                    return ev.camera;
                }
            }
            if let Some(f) = self.frame() {
                return f.camera;
            }
        }
        self.project.scene.camera
    }
    /// Pixel size of the final render (the project's chosen resolution).
    pub fn render_size(&self) -> (u32, u32) {
        let s = self.frame().filter(|_| self.animating()).and_then(|f| f.style.as_ref()).unwrap_or(&self.project.scene.style);
        (s.width.max(1), s.height.max(1))
    }
    /// Renders the real final picture: the render style at the chosen resolution, seen through the final camera.
    pub fn render_preview(&self, r: &mut Renderer) {
        let style = self.frame().filter(|_| self.animating()).and_then(|f| f.style.clone()).unwrap_or_else(|| self.project.scene.style.clone());
        self.render_core(r, self.final_camera(), &style);
    }
    /// The final picture of the current frame without the scene layers: the reference under a layer being drawn.
    pub fn render_reference(&self, r: &mut Renderer) {
        let style = self.frame().filter(|_| self.animating()).and_then(|f| f.style.clone()).unwrap_or_else(|| self.project.scene.style.clone());
        self.render_core_opts(r, self.final_camera(), &style, false);
    }
    fn render_core(&self, r: &mut Renderer, cam: Camera, style: &PsxStyle) {
        self.render_core_opts(r, cam, style, true);
    }
    fn render_core_opts(&self, r: &mut Renderer, cam: Camera, style: &PsxStyle, layers: bool) {
        if self.animating() {
            let a = self.animation().unwrap();
            let (states, f): (Cow<[ObjectState]>, &AnimFrame) = if self.playing {
                match a.eval(self.tick) {
                    Some(ev) => (Cow::Owned(ev.states), &a.frames[ev.frame_index]),
                    None => (Cow::Borrowed(&a.frames[0].states[..]), &a.frames[0]),
                }
            } else {
                let f = &a.frames[self.current_frame.min(a.frames.len() - 1)];
                (Cow::Borrowed(&f.states[..]), f)
            };
            render_states(&self.project, &FrameSpec { states: &states, camera: cam, style, hand_layer: f.hand_layer.as_ref(), hand_only: f.hand_only, scene_layers: layers }, r);
        } else {
            let states = scene_states(&self.project);
            render_states(&self.project, &FrameSpec { states: &states, camera: cam, style, hand_layer: None, hand_only: false, scene_layers: layers }, r);
        }
    }
    /// Renders an arbitrary frame of the current animation (used for the onion-skin ghosts of neighbouring frames).
    pub fn render_frame_into(&self, idx: usize, r: &mut Renderer) -> bool {
        let Some(f) = self.animation().and_then(|a| a.frames.get(idx)) else { return false };
        let style = f.style.clone().unwrap_or_else(|| self.project.scene.style.clone());
        self.render_frame_with(f, r, style);
        true
    }
    /// Like `render_frame_into`, but at an explicit pixel size (the size of the main viewport).
    pub fn render_frame_sized(&self, idx: usize, r: &mut Renderer, w: u32, h: u32) -> bool {
        let Some(f) = self.animation().and_then(|a| a.frames.get(idx)) else { return false };
        let mut style = f.style.clone().unwrap_or_else(|| self.project.scene.style.clone());
        style.snap *= w as f32 / style.width.max(1) as f32;
        style.width = w.max(1);
        style.height = h.max(1);
        self.render_frame_with(f, r, style);
        true
    }
    fn render_frame_with(&self, f: &AnimFrame, r: &mut Renderer, style: PsxStyle) {
        let cam = if self.through_camera { f.camera } else { self.view.camera() };
        render_states(&self.project, &FrameSpec { states: &f.states, camera: cam, style: &style, hand_layer: f.hand_layer.as_ref(), hand_only: f.hand_only, scene_layers: true }, r);
    }
    pub fn onion_indices(&self) -> (Option<usize>, Option<usize>) {
        let Some(a) = self.animation() else { return (None, None) };
        let i = self.current_frame;
        ((i > 0).then(|| i - 1), (i + 1 < a.frames.len()).then(|| i + 1))
    }
    /// Frames every object of the scene in the viewport.
    pub fn frame_all(&mut self) {
        let mut lo = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut hi = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
        let mut any = false;
        for o in &self.project.scene.objects {
            let Some(a) = self.project.assets.get(o.asset) else { continue };
            if let Some((l, h)) = a.model.bounds() {
                let m = o.transform.matrix();
                for k in 0..8 {
                    let c = Vec3::new(if k & 1 == 0 { l.x } else { h.x }, if k & 2 == 0 { l.y } else { h.y }, if k & 4 == 0 { l.z } else { h.z });
                    let w = m.transform_point(c);
                    lo = lo.min(w);
                    hi = hi.max(w);
                    any = true;
                }
            }
        }
        if any {
            self.view.frame(lo, hi);
        }
    }

    // -------------------------------------------------------------- scene objects

    /// Adds a model to the project: guarantees a texture and UVs (so there is something to paint on), places it in the scene and in every frame. Returns the new object id.
    pub fn import_model(&mut self, mut model: Model) -> u32 {
        self.end_group();
        self.record(Slot::Whole);
        for m in &mut model.meshes {
            if !m.verts.is_empty() && m.verts.iter().all(|v| v.uv == Vec2::default()) {
                decimate::box_unwrap(m);
            }
        }
        model.ensure_material();
        if model.textures.is_empty() {
            model.textures.push(Texture::checker("Main", 64, 64, 8, [200, 200, 205, 255], [150, 150, 160, 255]));
        }
        let ntex = model.textures.len();
        for m in &mut model.materials {
            if m.texture.map_or(true, |t| t >= ntex) {
                m.texture = Some(0);
            }
        }
        let first = self.project.scene.objects.is_empty();
        let id = self.project.place_model(model);
        let (t, bones) = {
            let o = self.project.scene.object(id).unwrap();
            (o.transform.clone(), self.project.bones_of(o.asset))
        };
        for a in &mut self.project.animations {
            for f in &mut a.frames {
                f.states.push(ObjectState::base(id, t.clone(), bones));
            }
        }
        self.select_object(Some(id));
        if first {
            self.frame_all();
            self.camera_everywhere_from_view();
        }
        self.dirty = true;
        id
    }
    pub fn import_path(&mut self, path: &Path) -> Result<Vec<String>, String> {
        let loaded = crate::import::load_from_path(path)?;
        let tris = loaded.model.triangle_count();
        self.import_model(loaded.model);
        self.status = format!("Imported: {} ({} triangles)", path.file_name().and_then(|n| n.to_str()).unwrap_or("model"), tris);
        Ok(loaded.warnings)
    }
    pub fn add_primitive(&mut self, kind: Primitive) -> u32 {
        if kind == Primitive::Butterfly {
            return self.import_model(modeling::butterfly());
        }
        let (name, mesh) = match kind {
            Primitive::Cube => ("Cube", modeling::cube(Vec3::new(1.0, 1.0, 1.0))),
            Primitive::Plane => ("Plane", modeling::plane(2.0, 2.0, 1, 1)),
            Primitive::Cylinder => ("Cylinder", modeling::cylinder(0.5, 1.0, 8)),
            Primitive::Cone => ("Cone", modeling::cone(0.5, 1.0, 8)),
            Primitive::Sphere => ("Sphere", modeling::sphere(0.5, 8, 6)),
            Primitive::Torus => ("Torus", modeling::torus(0.5, 0.2, 10, 6)),
            Primitive::Butterfly => unreachable!("handled above"),
        };
        self.import_model(modeling::model_from_mesh(name, mesh, 64))
    }
    pub fn delete_object(&mut self, id: u32) -> bool {
        if self.project.scene.object(id).is_none() {
            return false;
        }
        if self.edit.as_ref().is_some_and(|s| s.object == id) {
            self.edit_exit();
        }
        self.end_group();
        self.record(Slot::Whole);
        self.project.scene.remove_object(id);
        for a in &mut self.project.animations {
            for f in &mut a.frames {
                f.states.retain(|s| s.object != id);
            }
        }
        if self.selected_object == Some(id) {
            self.select_object(self.project.scene.objects.first().map(|o| o.id));
        }
        true
    }
    /// Duplicates an object (sharing the same asset), shifted slightly sideways.
    pub fn duplicate_object(&mut self, id: u32) -> Option<u32> {
        let src = self.project.scene.object(id)?.clone();
        self.end_group();
        self.record(Slot::Whole);
        let new = self.project.scene.add_object(&format!("{} copy", src.name), src.asset);
        let mut t = src.transform.clone();
        t.pos.x += 0.5;
        if let Some(o) = self.project.scene.object_mut(new) {
            o.transform = t.clone();
            o.tint = src.tint;
        }
        let bones = self.project.bones_of(src.asset);
        for a in &mut self.project.animations {
            for f in &mut a.frames {
                let mut st = ObjectState::base(new, t.clone(), bones);
                if let Some(s) = f.states.iter().find(|s| s.object == id) {
                    st.pose = s.pose.clone();
                    st.emotion = s.emotion;
                    st.visible = s.visible;
                    st.transform.pos.x += 0.5;
                }
                f.states.push(st);
            }
        }
        self.select_object(Some(new));
        Some(new)
    }
    pub fn rename_object(&mut self, id: u32, name: &str) {
        self.record(Slot::Scene);
        if let Some(o) = self.project.scene.object_mut(id) {
            o.name = name.to_string();
        }
    }
    pub fn set_object_visible(&mut self, id: u32, v: bool) {
        self.record(Slot::Scene);
        if let Some(o) = self.project.scene.object_mut(id) {
            o.visible = v;
        }
    }
    pub fn set_object_tint(&mut self, id: u32, tint: [f32; 3]) {
        self.record(Slot::Scene);
        if let Some(o) = self.project.scene.object_mut(id) {
            o.tint = tint;
        }
    }
    fn state_mut(&mut self, object: u32) -> Option<&mut ObjectState> {
        let (ai, fi) = (self.current_anim, self.current_frame);
        let bones = self.project.scene.object(object).map_or(0, |o| self.project.bones_of(o.asset));
        let t = self.project.scene.object(object)?.transform.clone();
        let f = self.project.animations.get_mut(ai)?.frames.get_mut(fi)?;
        if !f.states.iter().any(|s| s.object == object) {
            f.states.push(ObjectState::base(object, t, bones));
        }
        f.states.iter_mut().find(|s| s.object == object)
    }
    fn edits_frame(&self) -> bool {
        matches!(self.mode, Mode::Pose | Mode::Animate) && self.animation().is_some_and(|a| !a.frames.is_empty())
    }
    pub fn object_transform(&self, id: u32) -> Option<Transform> {
        if self.edits_frame() {
            if let Some(s) = self.frame().and_then(|f| f.states.iter().find(|s| s.object == id)) {
                return Some(s.transform.clone());
            }
        }
        self.project.scene.object(id).map(|o| o.transform.clone())
    }
    /// Sets an object's move/rotate/scale: in Pose and Animate modes it only affects the current frame.
    pub fn set_object_transform(&mut self, id: u32, t: Transform) {
        if self.edits_frame() {
            self.record(Slot::Frame(self.current_anim, self.current_frame));
            if let Some(s) = self.state_mut(id) {
                s.transform = t;
            }
        } else {
            self.record(Slot::Scene);
            if let Some(o) = self.project.scene.object_mut(id) {
                o.transform = t;
            }
        }
    }

    /// Converts a direction from world (scene) space into the object's model space, taking rotation and scale into account.
    pub fn world_to_model_dir(&self, id: u32, v: Vec3) -> Vec3 {
        let Some(t) = self.object_transform(id) else { return v };
        let r = conj(rot_of(&t)).rotate(v);
        let nz = |s: f32| if s.abs() < 1e-6 { 1.0 } else { s };
        Vec3::new(r.x / nz(t.scale.x), r.y / nz(t.scale.y), r.z / nz(t.scale.z))
    }

    // -------------------------------------------------------------- painting

    fn canvas_for(&self, object: u32, asset: usize, tex: usize) -> Option<Canvas> {
        let layer = if self.mode == Mode::Animate { self.paint_layer } else { PaintLayer::Base };
        match layer {
            PaintLayer::Base => Some(Canvas::Base { asset, tex }),
            PaintLayer::FramePatch => {
                if self.animation().is_some_and(|a| self.current_frame < a.frames.len()) {
                    Some(Canvas::Patch { anim: self.current_anim, frame: self.current_frame, object, tex })
                } else {
                    Some(Canvas::Base { asset, tex })
                }
            }
            PaintLayer::Emotion => {
                let idx = self.frame()?.states.iter().find(|s| s.object == object)?.emotion?;
                let e = self.project.assets.get(asset)?.emotions.get(idx)?;
                (e.texture == tex).then_some(Canvas::Emotion { asset, idx })
            }
        }
    }
    /// The canvas currently shown by the 2D texture editor.
    pub fn active_canvas(&self) -> Option<Canvas> {
        let id = self.selected_object?;
        let asset = self.selected_asset()?;
        let n = self.project.assets.get(asset)?.model.textures.len();
        if n == 0 {
            return None;
        }
        self.canvas_for(id, asset, self.active_texture.min(n - 1))
    }
    pub fn hand_canvas(&self) -> Option<Canvas> {
        self.animation().filter(|a| self.current_frame < a.frames.len()).map(|_| Canvas::Hand { anim: self.current_anim, frame: self.current_frame })
    }
    /// What the 2D editor displays; for a frame repaint that does not exist yet this is the original texture.
    pub fn canvas_texture(&self, c: Canvas) -> Option<Cow<'_, Texture>> {
        let p = &self.project;
        match c {
            Canvas::Base { asset, tex } => p.assets.get(asset)?.model.textures.get(tex).map(Cow::Borrowed),
            Canvas::Emotion { asset, idx } => p.assets.get(asset)?.emotions.get(idx).map(|e| Cow::Borrowed(&e.image)),
            Canvas::Patch { anim, frame, object, tex } => {
                let f = p.animations.get(anim)?.frames.get(frame)?;
                let st = f.states.iter().find(|s| s.object == object);
                if let Some((_, t)) = st.and_then(|s| s.tex_patch.iter().find(|(i, _)| *i == tex)) {
                    return Some(Cow::Borrowed(t));
                }
                let asset = p.scene.object(object)?.asset;
                let a = p.assets.get(asset)?;
                if let Some(e) = st.and_then(|s| s.emotion).and_then(|i| a.emotions.get(i)).filter(|e| e.texture == tex) {
                    return Some(Cow::Borrowed(&e.image));
                }
                a.model.textures.get(tex).map(Cow::Borrowed)
            }
            Canvas::Scene { layer } => p.scene.layers.get(layer).map(|l| Cow::Borrowed(&l.img)),
            Canvas::Hand { anim, frame } => {
                let f = p.animations.get(anim)?.frames.get(frame)?;
                match &f.hand_layer {
                    Some(t) => Some(Cow::Borrowed(t)),
                    None => {
                        let st = f.style.as_ref().unwrap_or(&p.scene.style);
                        Some(Cow::Owned(Texture::new("Drawing", st.width, st.height, [0, 0, 0, 0])))
                    }
                }
            }
        }
    }
    fn canvas_slot(&self, c: Canvas) -> Slot {
        match c {
            Canvas::Base { asset, tex } => {
                if self.project.assets.get(asset).is_some_and(|a| a.stack(tex).is_some()) {
                    Slot::TexStack(asset, tex)
                } else {
                    Slot::Texture(asset, tex)
                }
            }
            Canvas::Scene { layer } => Slot::SceneLayer(layer),
            Canvas::Emotion { asset, idx } => Slot::EmotionImg(asset, idx),
            Canvas::Patch { anim, frame, .. } | Canvas::Hand { anim, frame } => Slot::Frame(anim, frame),
        }
    }
    /// Creates the repaint or layer on the first touch.
    fn ensure_canvas(&mut self, c: Canvas) -> bool {
        match c {
            Canvas::Patch { anim, frame, object, tex } => {
                let Some(src) = self.canvas_texture(c).map(|t| t.into_owned()) else { return false };
                let Some(st) = self.project.animations.get_mut(anim).and_then(|a| a.frames.get_mut(frame)).and_then(|f| f.states.iter_mut().find(|s| s.object == object)) else {
                    return false;
                };
                if !st.tex_patch.iter().any(|(i, _)| *i == tex) {
                    st.tex_patch.push((tex, src));
                }
                true
            }
            Canvas::Hand { anim, frame } => {
                let Some(fresh) = self.canvas_texture(c).map(|t| t.into_owned()) else { return false };
                match self.project.animations.get_mut(anim).and_then(|a| a.frames.get_mut(frame)) {
                    Some(f) => {
                        if f.hand_layer.is_none() {
                            f.hand_layer = Some(fresh);
                        }
                        true
                    }
                    None => false,
                }
            }
            _ => self.canvas_texture(c).is_some(),
        }
    }
    fn canvas_mut(&mut self, c: Canvas) -> Option<&mut Texture> {
        let p = &mut self.project;
        match c {
            Canvas::Base { asset, tex } => {
                let a = p.assets.get_mut(asset)?;
                if a.stack(tex).is_some() {
                    return a.stack_mut(tex)?.active_layer_mut().map(|l| &mut l.img);
                }
                a.model.textures.get_mut(tex)
            }
            Canvas::Scene { layer } => p.scene.layers.get_mut(layer).map(|l| &mut l.img),
            Canvas::Emotion { asset, idx } => p.assets.get_mut(asset)?.emotions.get_mut(idx).map(|e| &mut e.image),
            Canvas::Patch { anim, frame, object, tex } => p.animations.get_mut(anim)?.frames.get_mut(frame)?.states.iter_mut().find(|s| s.object == object)?.tex_patch.iter_mut().find(|(i, _)| *i == tex).map(|(_, t)| t),
            Canvas::Hand { anim, frame } => p.animations.get_mut(anim)?.frames.get_mut(frame)?.hand_layer.as_mut(),
        }
    }
    /// The layer (if any) that painting on `c` would change.
    fn target_layer(&self, c: Canvas) -> Option<&Layer> {
        match c {
            Canvas::Scene { layer } => self.project.scene.layers.get(layer),
            Canvas::Base { asset, tex } => self.project.assets.get(asset)?.stack(tex)?.active_layer(),
            _ => None,
        }
    }
    /// The picture that painting on `c` changes: the active layer of a layered texture, otherwise what `canvas_texture` shows.
    fn edit_texture(&self, c: Canvas) -> Option<Cow<'_, Texture>> {
        match self.target_layer(c) {
            Some(l) => Some(Cow::Borrowed(&l.img)),
            None => self.canvas_texture(c),
        }
    }
    /// After a layer of a layered texture changed: the texture the model uses shows the merged result.
    fn sync_stack(&mut self, c: Canvas) {
        if let Canvas::Base { asset, tex } = c {
            if let Some(a) = self.project.assets.get_mut(asset) {
                if let Some(s) = a.stack(tex) {
                    let name = a.model.textures.get(tex).map(|t| t.name.clone()).unwrap_or_default();
                    let flat = s.flatten(&name);
                    if let Some(t) = a.model.textures.get_mut(tex) {
                        if t.px.len() == flat.px.len() {
                            *t = flat;
                        }
                    }
                }
            }
        }
    }
    fn canvas_prepare(&mut self, c: Canvas) -> bool {
        if let Some(l) = self.target_layer(c) {
            if l.locked {
                self.status = format!("Layer \"{}\" is locked", l.name);
                return false;
            }
            if !l.visible {
                self.status = format!("Layer \"{}\" is hidden: show it to paint on it", l.name);
                return false;
            }
        }
        let slot = self.canvas_slot(c);
        if self.read_slot(slot).is_none() {
            return false;
        }
        self.record(slot);
        self.ensure_canvas(c)
    }

    /// Button press in the 2D editor (or on the hand-drawn layer); coordinates are in canvas pixels.
    pub fn canvas_press(&mut self, c: Canvas, p: (f32, f32)) {
        self.begin_group();
        if self.brush.tool == Tool::Picker {
            if let Some(col) = self.canvas_texture(c).and_then(|t| paint::pick_color(&t, p)) {
                self.brush.color = col;
            }
            return;
        }
        if !self.canvas_prepare(c) {
            return;
        }
        let b = self.brush;
        let before = self.sel_before(c);
        let Some(t) = self.canvas_mut(c) else { return };
        match b.tool {
            Tool::Line | Tool::Rect | Tool::RectFilled | Tool::Gradient => self.shape = Some((c, p, t.clone())),
            _ => {
                paint::apply_point(t, &b, p);
                self.stroke_last = Some((c, p));
            }
        }
        self.sel_after(c, before);
        self.sync_stack(c);
    }
    pub fn canvas_drag(&mut self, c: Canvas, p: (f32, f32)) {
        let b = self.brush;
        if let Some((sc, start, base)) = self.shape.clone() {
            if sc == c {
                let sel = self.selection_for(c).cloned();
                if let Some(t) = self.canvas_mut(c) {
                    *t = base.clone();
                    paint::apply_shape(t, &b, start, p);
                    if let Some(s) = &sel {
                        s.restrict(&base, t);
                    }
                }
                self.sync_stack(c);
            }
            return;
        }
        let last = self.stroke_last.filter(|(lc, _)| *lc == c).map(|(_, lp)| lp);
        // a stroke only continues when its press was accepted (a locked or hidden layer refuses it)
        if b.tool.is_continuous() && last.is_some() {
            let before = self.sel_before(c);
            if let Some(t) = self.canvas_mut(c) {
                paint::apply_stroke(t, &b, last, p);
                self.stroke_last = Some((c, p));
            }
            self.sel_after(c, before);
        self.sync_stack(c);
        }
    }
    pub fn canvas_release(&mut self, c: Canvas, p: (f32, f32)) {
        self.canvas_drag(c, p);
        self.end_group();
    }

    /// Paints on the model in the viewport. `frame` is the most recently rendered frame (the UV under the cursor is read from it).
    /// Returns true if something was painted or a colour was picked.
    pub fn paint_press(&mut self, frame: &Frame, x: i32, y: i32) -> bool {
        if self.project_paint && matches!(self.brush.tool, Tool::Brush | Tool::Eraser) {
            return self.proj_press(frame, x, y);
        }
        let Some(info) = self.pick_info(frame.hit_at(x, y)) else { return false };
        let Some(tex) = info.tex else { return false };
        let Some(c) = self.canvas_for(info.object, info.asset, tex) else {
            self.status = "Select an emotion for this object first".into();
            return false;
        };
        let Some((w, h)) = self.canvas_texture(c).map(|t| (t.w, t.h)) else { return false };
        self.active_texture = tex;
        self.canvas_press(c, (info.u * w as f32, info.v * h as f32));
        true
    }
    pub fn paint_drag(&mut self, frame: &Frame, from: (i32, i32), to: (i32, i32)) -> usize {
        if self.proj.is_some() {
            return self.proj_drag(frame, from, to);
        }
        if !self.brush.tool.is_continuous() {
            return 0;
        }
        let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs()).max(1);
        let mut dabs = 0;
        for i in 1..=steps {
            let k = i as f32 / steps as f32;
            let (x, y) = ((from.0 as f32 + (to.0 - from.0) as f32 * k).round() as i32, (from.1 as f32 + (to.1 - from.1) as f32 * k).round() as i32);
            let Some(info) = self.pick_info(frame.hit_at(x, y)) else {
                self.stroke_last = None;
                continue;
            };
            let Some(tex) = info.tex else { continue };
            let Some(c) = self.canvas_for(info.object, info.asset, tex) else { continue };
            let Some((w, h)) = self.canvas_texture(c).map(|t| (t.w, t.h)) else { continue };
            let p = (info.u * w as f32, info.v * h as f32);
            // connect two stamps with a line only if both are on the same texture and the UV did not jump across to another island
            let last = self.stroke_last.filter(|(lc, lp)| *lc == c && ((p.0 - lp.0).powi(2) + (p.1 - lp.1).powi(2)).sqrt() < 0.25 * w.max(h) as f32).map(|(_, lp)| lp);
            if !self.canvas_prepare(c) {
                continue;
            }
            let b = self.brush;
            let before = self.sel_before(c);
            if let Some(t) = self.canvas_mut(c) {
                paint::apply_stroke(t, &b, last, p);
                self.stroke_last = Some((c, p));
                dabs += 1;
            }
            self.sel_after(c, before);
        self.sync_stack(c);
        }
        dabs
    }
    pub fn paint_release(&mut self) {
        self.end_group();
    }

    /// Applies an operation to the whole texture of a canvas.
    pub fn texture_op(&mut self, c: Canvas, op: TexOp) -> bool {
        self.end_group();
        self.begin_group();
        let r = self.texture_op_inner(c, op);
        self.end_group();
        r
    }
    fn texture_op_inner(&mut self, c: Canvas, op: TexOp) -> bool {
        if !self.canvas_prepare(c) {
            return false;
        }
        let sel = self.selection_for(c).cloned();
        // flips, offsets and resizing move every layer of a layered texture together; colour operations change the active layer only
        let geometry = matches!(op, TexOp::FlipH | TexOp::FlipV | TexOp::Offset(..) | TexOp::Resize(..));
        if geometry {
            if let Canvas::Base { asset, tex } = c {
                if let Some(s) = self.project.assets.get_mut(asset).and_then(|a| a.stack_mut(tex)) {
                    for l in s.layers.iter_mut() {
                        Self::apply_op(&mut l.img, op, None);
                    }
                    if matches!(op, TexOp::Resize(..)) {
                        self.selection = None;
                    }
                    self.sync_stack(c);
                    // the merged texture is replaced as a whole when the size changed
                    if let Some(a) = self.project.assets.get_mut(asset) {
                        if let (Some(s), Some(t)) = (a.stack(tex), a.model.textures.get(tex)) {
                            if s.size() != (t.w, t.h) {
                                let flat = s.flatten(&t.name.clone());
                                a.model.textures[tex] = flat;
                            }
                        }
                    }
                    return true;
                }
            }
        }
        let Some(t) = self.canvas_mut(c) else { return false };
        Self::apply_op(t, op, sel.as_ref());
        if matches!(op, TexOp::Resize(..)) {
            self.selection = None;
        }
        self.sync_stack(c);
        true
    }
    fn apply_op(t: &mut Texture, op: TexOp, sel: Option<&Selection>) {
        match op {
            TexOp::FlipH => t.flip_h(),
            TexOp::FlipV => t.flip_v(),
            TexOp::Offset(dx, dy) => t.offset_wrap(dx, dy),
            TexOp::Resize(w, h) => {
                let (w, h) = (w.clamp(1, 2048), h.clamp(1, 2048));
                *t = t.resized(w, h);
            }
            TexOp::Quantize15 => t.quantize_15bit(),
            TexOp::Dither(bits) => t.dither_quantize(bits.clamp(1, 8)),
            TexOp::ReducePalette(n) => {
                t.reduce_palette(n.clamp(2, 256));
            }
            TexOp::Hsv(h, s, v) => t.adjust_hsv(h, s, v),
            TexOp::Fill(col) => match sel {
                Some(s) if s.w == t.w && s.h == t.h => s.fill(t, col),
                _ => t.px.iter_mut().for_each(|p| *p = col),
            },
            TexOp::ReplaceColor(a, b, tol) => {
                t.replace_color(a, b, tol);
            }
        }
    }
    // -------------------------------------------------------------- pixel selection

    /// The selection if it belongs to this canvas and still matches its size.
    pub fn selection_for(&self, c: Canvas) -> Option<&Selection> {
        if self.selection_canvas != Some(c) {
            return None;
        }
        let s = self.selection.as_ref()?;
        let t = self.canvas_texture(c)?;
        (s.w == t.w && s.h == t.h).then_some(s)
    }
    pub fn selection_for_public(&self, c: Canvas) -> Option<&Selection> {
        self.selection_for(c)
    }
    /// A copy of the texture taken before painting, only when a selection must be enforced (and the texture is small enough to copy cheaply).
    fn sel_before(&self, c: Canvas) -> Option<Texture> {
        self.selection_for(c)?;
        let t = self.edit_texture(c)?;
        (t.px.len() <= 1_048_576).then(|| t.into_owned())
    }
    fn sel_after(&mut self, c: Canvas, before: Option<Texture>) {
        let Some(b) = before else { return };
        let Some(sel) = self.selection_for(c).cloned() else { return };
        if let Some(t) = self.canvas_mut(c) {
            sel.restrict(&b, t);
        }
    }
    fn canvas_size(&self, c: Canvas) -> Option<(u32, u32)> {
        self.canvas_texture(c).map(|t| (t.w, t.h))
    }
    /// Applies a freshly built mask to the current selection of `c`.
    pub fn select_with(&mut self, c: Canvas, mask: Selection, how: Combine) {
        let keep = self.selection_for(c).cloned();
        self.selection = Some(match (keep, how) {
            (Some(mut old), Combine::Add) | (Some(mut old), Combine::Subtract) => {
                old.combine(&mask, how);
                old
            }
            _ => mask,
        });
        self.selection_canvas = Some(c);
    }
    pub fn select_rect(&mut self, c: Canvas, a: (i32, i32), b: (i32, i32), how: Combine) {
        if let Some((w, h)) = self.canvas_size(c) {
            self.select_with(c, Selection::rect(w, h, a, b), how);
        }
    }
    pub fn select_wand(&mut self, c: Canvas, x: i32, y: i32, tolerance: u8, contiguous: bool, how: Combine) {
        let mask = self.canvas_texture(c).map(|t| Selection::wand(&t, x, y, tolerance, contiguous));
        if let Some(m) = mask {
            self.select_with(c, m, how);
        }
    }
    pub fn select_all(&mut self, c: Canvas) {
        if let Some((w, h)) = self.canvas_size(c) {
            self.select_with(c, Selection::all(w, h), Combine::Replace);
        }
    }
    pub fn select_none(&mut self) {
        self.selection = None;
        self.selection_canvas = None;
    }
    pub fn select_invert(&mut self, c: Canvas) {
        if self.selection_for(c).is_some() {
            if let Some(s) = self.selection.as_mut() {
                s.invert();
            }
        } else {
            self.select_all(c);
        }
    }
    /// Selects the pixels of one UV island of the selected model's mesh (`mesh` index and island index).
    pub fn select_uv_island(&mut self, c: Canvas, mesh: usize, island: usize, how: Combine) -> bool {
        let Some((w, h)) = self.canvas_size(c) else { return false };
        let Some(model) = self.selected_model() else { return false };
        let Some(m) = model.meshes.get(mesh) else { return false };
        let isl = crate::uvisland::islands(m);
        let Some(i) = isl.get(island) else { return false };
        let mask = crate::uvisland::island_mask(m, i, w, h);
        self.select_with(c, mask, how);
        true
    }
    /// Copies the selected pixels into the editor clipboard.
    pub fn selection_copy(&mut self, c: Canvas) -> bool {
        let clip = match (self.selection_for(c), self.canvas_texture(c)) {
            (Some(s), Some(t)) => s.copy_from(&t),
            _ => None,
        };
        let ok = clip.is_some();
        if ok {
            self.pixel_clip = clip;
        }
        ok
    }
    /// Makes the selected pixels transparent. One undo step.
    pub fn selection_delete(&mut self, c: Canvas) -> bool {
        let Some(sel) = self.selection_for(c).cloned() else { return false };
        self.end_group();
        if !self.canvas_prepare(c) {
            return false;
        }
        match self.canvas_mut(c) {
            Some(t) => {
                sel.fill(t, [0, 0, 0, 0]);
                self.sync_stack(c);
                true
            }
            None => false,
        }
    }
    /// Pastes the clipboard with its top-left corner at `(x, y)`. One undo step.
    pub fn selection_paste(&mut self, c: Canvas, x: i32, y: i32) -> bool {
        let Some(clip) = self.pixel_clip.clone() else { return false };
        self.end_group();
        if !self.canvas_prepare(c) {
            return false;
        }
        match self.canvas_mut(c) {
            Some(t) => {
                clip.paste_into(t, x, y);
                self.sync_stack(c);
                true
            }
            None => false,
        }
    }

    /// Replaces the whole content of a canvas (for example with a loaded image) as a single undo step.
    pub fn replace_canvas(&mut self, c: Canvas, tex: Texture) -> bool {
        self.end_group();
        self.begin_group();
        if !self.canvas_prepare(c) {
            self.end_group();
            return false;
        }
        let ok = match self.canvas_mut(c) {
            Some(t) => {
                let name = t.name.clone();
                *t = Texture { name, ..tex };
                self.sync_stack(c);
                true
            }
            None => false,
        };
        self.end_group();
        ok
    }
    /// Loads a PNG/JPEG/BMP/TGA/PPM file from disk into a canvas. Images larger than 2048 px are scaled down:
    /// that is plenty for low-resolution game textures and keeps memory use predictable.
    pub fn import_image_into(&mut self, c: Canvas, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Could not read \"{}\": {e}", path.display()))?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
        let img = crate::decode::image::decode_image(name, &bytes)?;
        let mut t = Texture { name: name.to_string(), w: img.w, h: img.h, px: img.rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect() };
        let m = t.w.max(t.h);
        if m > 2048 {
            let k = 2048.0 / m as f32;
            t = t.resized(((t.w as f32 * k) as u32).max(1), ((t.h as f32 * k) as u32).max(1));
        }
        if self.replace_canvas(c, t) {
            Ok(())
        } else {
            Err("There is no canvas to load into (select an object first)".into())
        }
    }
    /// Saves the texture of a canvas as a PNG file, writing through a temporary file so a failure never damages an existing file.
    pub fn export_texture_png(&self, c: Canvas, path: &Path) -> Result<(), String> {
        let t = self.canvas_texture(c).ok_or("There is no texture to save")?;
        let bytes = crate::encode::png::encode_png(t.w, t.h, &t.to_rgba_bytes(), true);
        let mut part = path.as_os_str().to_os_string();
        part.push(".part");
        let part = PathBuf::from(part);
        std::fs::write(&part, bytes).map_err(|e| format!("Could not write \"{}\": {e}", path.display()))?;
        std::fs::rename(&part, path).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            format!("Could not write \"{}\": {e}", path.display())
        })
    }
    pub fn new_texture(&mut self, w: u32, h: u32) -> Option<usize> {
        let asset = self.selected_asset()?;
        self.end_group();
        self.record(Slot::Model(asset));
        let m = &mut self.project.assets[asset].model;
        let n = m.textures.len();
        m.textures.push(Texture::new(&format!("Texture {}", n + 1), w.clamp(1, 2048), h.clamp(1, 2048), [128, 128, 128, 255]));
        self.active_texture = n;
        Some(n)
    }

    // -------------------------------------------------------------- scene layers (pictures behind / in front of the 3D scene)

    /// The canvas of the active scene layer.
    pub fn scene_canvas(&self) -> Option<Canvas> {
        let n = self.project.scene.layers.len();
        (n > 0).then(|| Canvas::Scene { layer: self.active_layer.min(n - 1) })
    }
    /// Adds an empty layer at the size of the render: behind the 3D objects (on top of the layers already behind) or in front of them.
    pub fn add_scene_layer(&mut self, behind: bool) -> usize {
        self.end_group();
        self.record(Slot::SceneLayers);
        let (w, h) = self.render_size();
        let sc = &mut self.project.scene;
        let n = sc.layers.len() + 1;
        let layer = Layer::new(&format!("Layer {n}"), w, h);
        let at = if behind {
            let at = sc.layers_back;
            sc.layers.insert(at, layer);
            sc.layers_back += 1;
            at
        } else {
            sc.layers.push(layer);
            sc.layers.len() - 1
        };
        self.active_layer = at;
        self.status = format!("Added a layer {} the 3D scene", if behind { "behind" } else { "in front of" });
        at
    }
    pub fn delete_scene_layer(&mut self, i: usize) -> bool {
        if i >= self.project.scene.layers.len() {
            return false;
        }
        self.end_group();
        self.record(Slot::SceneLayers);
        self.project.scene.remove_layer(i);
        self.active_layer = self.active_layer.min(self.project.scene.layers.len().saturating_sub(1));
        if self.selection_canvas.is_some_and(|c| matches!(c, Canvas::Scene { .. })) {
            self.select_none();
        }
        true
    }
    pub fn duplicate_scene_layer(&mut self, i: usize) -> bool {
        let Some(mut l) = self.project.scene.layers.get(i).cloned() else { return false };
        self.end_group();
        self.record(Slot::SceneLayers);
        l.name = format!("{} copy", l.name);
        let back = self.project.scene.layers_back;
        self.project.scene.layers.insert(i + 1, l);
        if i < back {
            self.project.scene.layers_back += 1;
        }
        self.active_layer = i + 1;
        true
    }
    /// Moves a layer up (`up`) or down; a layer that passes the 3D scene changes sides.
    pub fn move_scene_layer(&mut self, i: usize, up: bool) -> bool {
        self.end_group();
        self.record(Slot::SceneLayers);
        let r = if up { self.project.scene.move_layer_up(i) } else { self.project.scene.move_layer_down(i) };
        if let Some(n) = r {
            if self.active_layer == i {
                self.active_layer = n;
            } else if self.active_layer == n {
                self.active_layer = i;
            }
        }
        r.is_some()
    }
    /// Changes the settings of a scene layer (name, visibility, lock, opacity) as one undo step.
    pub fn edit_scene_layer(&mut self, i: usize, f: impl FnOnce(&mut Layer)) -> bool {
        if i >= self.project.scene.layers.len() {
            return false;
        }
        self.end_group();
        self.record(Slot::SceneLayers);
        f(&mut self.project.scene.layers[i]);
        true
    }
    pub fn select_scene_layer(&mut self, i: usize) {
        if i < self.project.scene.layers.len() {
            self.active_layer = i;
        }
    }
    /// Loads an image file into a new scene layer (behind the 3D objects by default). The layer takes the size of the image,
    /// so a picture of any size can be used as a backdrop; it is stretched to the render size when drawn.
    pub fn import_image_as_scene_layer(&mut self, path: &Path, behind: bool) -> Result<usize, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Could not read \"{}\": {e}", path.display()))?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
        let img = crate::decode::image::decode_image(&name, &bytes)?;
        let mut t = Texture { name: name.clone(), w: img.w, h: img.h, px: img.rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect() };
        let m = t.w.max(t.h);
        if m > 2048 {
            let k = 2048.0 / m as f32;
            t = t.resized(((t.w as f32 * k) as u32).max(1), ((t.h as f32 * k) as u32).max(1));
        }
        let at = self.add_scene_layer(behind);
        self.project.scene.layers[at].name = name;
        self.project.scene.layers[at].img = t;
        Ok(at)
    }

    // -------------------------------------------------------------- texture layers

    fn stack_address(&self) -> Option<(usize, usize)> {
        let asset = self.selected_asset()?;
        let n = self.project.assets.get(asset)?.model.textures.len();
        (n > 0).then(|| (asset, self.active_texture.min(n - 1)))
    }
    /// The layer stack of the active texture of the selected model, if layers are switched on for it.
    pub fn texture_stack(&self) -> Option<&LayerStack> {
        let (a, t) = self.stack_address()?;
        self.project.assets.get(a)?.stack(t)
    }
    /// Starts using layers for the active texture: the current picture becomes the first layer.
    pub fn enable_texture_layers(&mut self) -> bool {
        let Some((a, t)) = self.stack_address() else { return false };
        if self.project.assets[a].stack(t).is_some() {
            return false;
        }
        if self.project.assets[a].indexed(t).is_some() {
            self.status = "Turn off indexed colour first: layers work on full-colour pictures".into();
            return false;
        }
        self.end_group();
        self.record(Slot::TexStack(a, t));
        let base = self.project.assets[a].model.textures[t].clone();
        self.project.assets[a].layers.push((t, LayerStack::single(base)));
        true
    }
    /// Merges all layers into the texture and stops using layers for it.
    pub fn disable_texture_layers(&mut self) -> bool {
        let Some((a, t)) = self.stack_address() else { return false };
        if self.project.assets[a].stack(t).is_none() {
            return false;
        }
        self.end_group();
        self.record(Slot::TexStack(a, t));
        self.project.assets[a].layers.retain(|(i, _)| *i != t);
        true
    }
    /// Runs a change of the layer stack of the active texture as one undo step, then refreshes the merged texture.
    pub fn edit_texture_stack(&mut self, f: impl FnOnce(&mut LayerStack)) -> bool {
        let Some((a, t)) = self.stack_address() else { return false };
        if self.project.assets[a].stack(t).is_none() {
            return false;
        }
        self.end_group();
        self.record(Slot::TexStack(a, t));
        if let Some(s) = self.project.assets[a].stack_mut(t) {
            f(s);
        }
        self.sync_stack(Canvas::Base { asset: a, tex: t });
        true
    }

    // -------------------------------------------------------------- emotions

    pub fn add_emotion(&mut self, name: &str) -> Option<usize> {
        let asset = self.selected_asset()?;
        let tex = self.active_texture.min(self.project.assets[asset].model.textures.len().checked_sub(1)?);
        self.end_group();
        self.record(Slot::Emotions(asset));
        // the starting image is whatever is currently visible on the model in this frame
        let obj = self.selected_object?;
        let image = self.canvas_texture(Canvas::Patch { anim: self.current_anim, frame: self.current_frame, object: obj, tex }).map(|t| t.into_owned()).or_else(|| self.project.assets[asset].model.textures.get(tex).cloned())?;
        let a = &mut self.project.assets[asset];
        a.emotions.push(Emotion { name: name.to_string(), texture: tex, image });
        let idx = a.emotions.len() - 1;
        self.active_emotion = Some(idx);
        Some(idx)
    }
    pub fn delete_emotion(&mut self, idx: usize) -> bool {
        let Some(asset) = self.selected_asset() else { return false };
        if idx >= self.project.assets[asset].emotions.len() {
            return false;
        }
        self.end_group();
        self.record(Slot::Whole);
        self.project.assets[asset].emotions.remove(idx);
        // frames that used the emotion go back to the base texture; the indices of later emotions shift down
        let objs: Vec<u32> = self.project.scene.objects.iter().filter(|o| o.asset == asset).map(|o| o.id).collect();
        for a in &mut self.project.animations {
            for f in &mut a.frames {
                for s in f.states.iter_mut().filter(|s| objs.contains(&s.object)) {
                    s.emotion = match s.emotion {
                        Some(e) if e == idx => None,
                        Some(e) if e > idx => Some(e - 1),
                        other => other,
                    };
                }
            }
        }
        self.active_emotion = None;
        true
    }
    pub fn set_frame_emotion(&mut self, object: u32, emotion: Option<usize>) {
        if !self.animation().is_some_and(|a| self.current_frame < a.frames.len()) {
            return;
        }
        self.end_group();
        self.record(Slot::Frame(self.current_anim, self.current_frame));
        if let Some(s) = self.state_mut(object) {
            s.emotion = emotion;
        }
    }
    /// Removes the one-off texture repaint of this frame.
    pub fn clear_frame_patch(&mut self, object: u32) {
        self.end_group();
        self.record(Slot::Frame(self.current_anim, self.current_frame));
        if let Some(s) = self.state_mut(object) {
            s.tex_patch.clear();
        }
    }

    // -------------------------------------------------------------- model operations

    fn with_model<R>(&mut self, f: impl FnOnce(&mut Model) -> R) -> Option<R> {
        let asset = self.selected_asset()?;
        self.end_group();
        self.record(Slot::Model(asset));
        let r = f(&mut self.project.assets.get_mut(asset)?.model);
        self.selected_faces.clear();
        Some(r)
    }
    fn faces_by_mesh(&self) -> Vec<(usize, Vec<usize>)> {
        let mut out: Vec<(usize, Vec<usize>)> = vec![];
        for &(m, t) in &self.selected_faces {
            match out.last_mut() {
                Some((lm, v)) if *lm == m => v.push(t),
                _ => out.push((m, vec![t])),
            }
        }
        out
    }
    /// Reduces the polygon count to the fraction `ratio` (0..1). Returns (triangles before, triangles after).
    pub fn decimate_selected(&mut self, ratio: f32) -> Option<(usize, usize)> {
        self.with_model(|m| {
            let r = decimate::decimate_model(m, ratio.clamp(0.01, 1.0), 2.0);
            (r.before, r.after)
        })
    }
    pub fn unwrap_selected(&mut self) -> bool {
        self.with_model(|m| m.meshes.iter_mut().for_each(decimate::box_unwrap)).is_some()
    }
    pub fn flat_shade_selected(&mut self) -> bool {
        self.with_model(|m| m.meshes.iter_mut().for_each(modeling::flat_shade)).is_some()
    }
    pub fn weld_selected(&mut self) -> bool {
        self.with_model(|m| m.meshes.iter_mut().for_each(|x| modeling::weld(x, true))).is_some()
    }
    pub fn subdivide_selected(&mut self) -> bool {
        self.with_model(|m| m.meshes.iter_mut().for_each(modeling::subdivide)).is_some()
    }
    pub fn mirror_x_selected(&mut self) -> bool {
        self.with_model(|m| m.meshes.iter_mut().for_each(|x| modeling::mirror_x(x, 1e-4))).is_some()
    }
    pub fn delete_selected_faces(&mut self) -> bool {
        let groups = self.faces_by_mesh();
        if groups.is_empty() {
            return false;
        }
        self.with_model(|m| {
            for (mi, faces) in &groups {
                if let Some(mesh) = m.meshes.get_mut(*mi) {
                    modeling::delete_faces(mesh, faces);
                }
            }
        })
        .is_some()
    }
    pub fn extrude_selected_faces(&mut self, dist: f32) -> bool {
        let groups = self.faces_by_mesh();
        if groups.is_empty() {
            return false;
        }
        let mut new_sel = BTreeSet::new();
        let ok = self
            .with_model(|m| {
                for (mi, faces) in &groups {
                    if let Some(mesh) = m.meshes.get_mut(*mi) {
                        for t in modeling::extrude_faces(mesh, faces, dist) {
                            new_sel.insert((*mi, t));
                        }
                    }
                }
            })
            .is_some();
        self.selected_faces = new_sel; // the selection moves to the new cap so the extrusion can be continued
        ok
    }
    /// Moves the selected faces in model space (vertices at the same position move together, so seams never tear).
    pub fn move_selected_faces(&mut self, delta: Vec3) -> bool {
        let groups = self.faces_by_mesh();
        if groups.is_empty() {
            return false;
        }
        let keep = self.selected_faces.clone();
        // within one drag the undo snapshot is taken only once
        if let Some(asset) = self.selected_asset() {
            self.record(Slot::Model(asset));
        } else {
            return false;
        }
        let asset = self.selected_asset().unwrap();
        let m = &mut self.project.assets[asset].model;
        for (mi, faces) in &groups {
            let Some(mesh) = m.meshes.get_mut(*mi) else { continue };
            let mut keys = std::collections::HashSet::new();
            for &f in faces {
                if let Some(t) = mesh.tris.get(f) {
                    for &v in t {
                        keys.insert(pkey(mesh.verts[v as usize].pos));
                    }
                }
            }
            let ids: Vec<usize> = (0..mesh.verts.len()).filter(|&i| keys.contains(&pkey(mesh.verts[i].pos))).collect();
            modeling::translate_vertices(mesh, &ids, delta);
            mesh.recompute_normals();
        }
        self.selected_faces = keep;
        true
    }
    /// Sculpt, step 1: picks the vertex under the cursor (the one nearest to the hit point in UV space) and opens an undo group.
    pub fn sculpt_begin(&mut self, frame: &Frame, x: i32, y: i32) -> bool {
        let hit = frame.hit_at(x, y);
        let Some(info) = self.pick_info(hit) else { return false };
        let Some(mesh) = self.project.assets.get(info.asset).and_then(|a| a.model.meshes.get(info.mesh)) else { return false };
        let Some(t) = mesh.tris.get(info.tri).copied() else { return false };
        let d = |i: u32| {
            let uv = mesh.verts[i as usize].uv;
            (uv.x - hit.u).powi(2) + (uv.y - hit.v).powi(2)
        };
        let best = t.iter().copied().min_by(|a, b| d(*a).partial_cmp(&d(*b)).unwrap_or(std::cmp::Ordering::Equal)).unwrap() as usize;
        self.begin_group();
        self.record(Slot::Model(info.asset));
        self.sculpt_target = Some((info.asset, info.mesh, best));
        true
    }
    /// Step 2 (on every mouse movement): moves the vertex with a soft falloff; `delta` is in model space.
    pub fn sculpt_drag(&mut self, delta: Vec3, radius: f32) -> bool {
        let Some((a, m, v)) = self.sculpt_target else { return false };
        let Some(mesh) = self.project.assets.get_mut(a).and_then(|a| a.model.meshes.get_mut(m)) else { return false };
        if v >= mesh.verts.len() {
            return false;
        }
        modeling::move_with_falloff(mesh, v, delta, radius);
        mesh.recompute_normals();
        true
    }
    pub fn sculpt_end(&mut self) {
        self.sculpt_target = None;
        self.end_group();
    }
    /// Outlines of the selected faces in world (scene) space, used to highlight them in the viewport.
    pub fn selected_face_world_tris(&self) -> Vec<[Vec3; 3]> {
        let (Some(id), Some(model)) = (self.selected_object, self.selected_model()) else { return vec![] };
        let Some(t) = self.object_transform(id) else { return vec![] };
        let m = t.matrix();
        self.selected_faces
            .iter()
            .filter_map(|&(mi, ti)| {
                let mesh = model.meshes.get(mi)?;
                let tri = mesh.tris.get(ti)?;
                let p = |k: usize| m.transform_point(mesh.verts[tri[k] as usize].pos);
                Some([p(0), p(1), p(2)])
            })
            .collect()
    }

    // -------------------------------------------------------------- poses and bones

    pub fn ensure_animation(&mut self) {
        if self.project.animations.is_empty() {
            self.record(Slot::Whole);
            let a = Animation::from_scene("Animation 1", &self.project.scene, &|i| self.project.bones_of(i));
            self.project.animations.push(a);
            self.current_anim = 0;
            self.current_frame = 0;
        }
    }
    pub fn object_pose(&self, id: u32) -> Option<Pose> {
        let bones = self.project.scene.object(id).map(|o| self.project.bones_of(o.asset))?;
        let mut p = self.frame().and_then(|f| f.states.iter().find(|s| s.object == id)).map(|s| s.pose.clone()).unwrap_or_else(|| Pose::rest(bones));
        p.fit(bones);
        Some(p)
    }
    /// The bones of the selected object in screen coordinates (w×h is the size of the rendered frame).
    pub fn screen_bones(&self, w: u32, h: u32) -> Vec<ScreenBone> {
        let (Some(id), Some(model)) = (self.selected_object, self.selected_model()) else { return vec![] };
        let (Some(skel), Some(pose), Some(t)) = (model.skeleton.as_ref(), self.object_pose(id), self.object_transform(id)) else { return vec![] };
        let cam = self.active_camera();
        let world = world_matrices(skel, &pose);
        let om = t.matrix();
        let pos: Vec<Vec3> = world.iter().map(|m| om.transform_point(m.transform_point(Vec3::ZERO))).collect();
        skel.bones
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let tail = cam.project(pos[i], w, h)?;
                let head = match b.parent {
                    Some(p) => cam.project(pos[p], w, h).unwrap_or(tail),
                    None => tail,
                };
                Some(ScreenBone { bone: i, head: (head.0, head.1), tail: (tail.0, tail.1) })
            })
            .collect()
    }
    /// World position of a bone's joint (its pivot) for the selected object in the current pose.
    pub fn bone_joint_world(&self, bone: usize) -> Option<Vec3> {
        let (id, model) = (self.selected_object?, self.selected_model()?);
        let skel = model.skeleton.as_ref()?;
        if bone >= skel.bones.len() {
            return None;
        }
        let pose = self.object_pose(id)?;
        let world = world_matrices(skel, &pose);
        Some(self.object_transform(id)?.matrix().transform_point(world[bone].transform_point(Vec3::ZERO)))
    }
    /// Picks a bone by click: the nearest bone segment within `max_px` pixels.
    pub fn pick_bone(&mut self, sx: f32, sy: f32, w: u32, h: u32, max_px: f32) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for b in self.screen_bones(w, h) {
            let (ax, ay, bx, by) = (b.head.0, b.head.1, b.tail.0, b.tail.1);
            let (dx, dy) = (bx - ax, by - ay);
            let l2 = dx * dx + dy * dy;
            let t = if l2 < 1e-6 { 0.0 } else { (((sx - ax) * dx + (sy - ay) * dy) / l2).clamp(0.0, 1.0) };
            // joints (segment ends) take priority, because they are what users grab with the mouse
            let d = ((sx - (ax + dx * t)).powi(2) + (sy - (ay + dy * t)).powi(2)).sqrt().min(((sx - bx).powi(2) + (sy - by).powi(2)).sqrt() * 0.8);
            if d <= max_px && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, b.bone));
            }
        }
        self.selected_bone = best.map(|(_, i)| i);
        self.selected_bone
    }
    fn pose_edit(&mut self, f: impl FnOnce(&mut Pose, &Skeleton)) -> bool {
        let Some(id) = self.selected_object else { return false };
        let Some(asset) = self.selected_asset() else { return false };
        if self.project.assets[asset].model.skeleton.is_none() || !self.animation().is_some_and(|a| self.current_frame < a.frames.len()) {
            return false;
        }
        self.record(Slot::Frame(self.current_anim, self.current_frame));
        let skel = self.project.assets[asset].model.skeleton.clone().unwrap();
        let n = skel.bones.len();
        let Some(s) = self.state_mut(id) else { return false };
        s.pose.fit(n);
        f(&mut s.pose, &skel);
        true
    }
    /// Rotates a bone by `deg` degrees around an axis given in world (scene) space, for example a camera axis.
    pub fn rotate_bone_world(&mut self, bone: usize, axis_world: Vec3, deg: f32) -> bool {
        let Some(id) = self.selected_object else { return false };
        let Some(tr) = self.object_transform(id) else { return false };
        let axis_model = conj(rot_of(&tr)).rotate(axis_world);
        self.pose_edit(|pose, skel| {
            if bone >= skel.bones.len() {
                return;
            }
            // accumulated rotation of the parent chain in the current pose
            let mut chain = vec![];
            let mut cur = skel.bones[bone].parent;
            while let Some(p) = cur {
                chain.push(p);
                cur = skel.bones[p].parent;
                if chain.len() > skel.bones.len() {
                    break;
                }
            }
            let mut parent_rot = Quat::IDENTITY;
            for &p in chain.iter().rev() {
                parent_rot = parent_rot.mul(skel.bones[p].rest_r.mul(pose.bones[p].rot));
            }
            let frame_q = parent_rot.mul(skel.bones[bone].rest_r);
            let q = Quat::from_axis_angle(axis_model, deg.to_radians());
            pose.bones[bone].rot = conj(frame_q).mul(q).mul(frame_q).mul(pose.bones[bone].rot).normalized();
        })
    }
    /// Sets a bone's deviation from the rest pose as Euler angles in degrees (used by sliders).
    pub fn set_bone_euler(&mut self, bone: usize, deg: Vec3) -> bool {
        self.pose_edit(|pose, skel| {
            if bone < skel.bones.len() {
                pose.bones[bone].rot = Quat::from_euler(Vec3::new(deg.x.to_radians(), deg.y.to_radians(), deg.z.to_radians()));
            }
        })
    }
    pub fn reset_bone(&mut self, bone: usize) -> bool {
        self.pose_edit(|pose, skel| {
            if bone < skel.bones.len() {
                pose.bones[bone] = BonePose::default();
            }
        })
    }
    pub fn reset_pose(&mut self) -> bool {
        self.pose_edit(|pose, skel| *pose = Pose::rest(skel.bones.len()))
    }
    pub fn mirror_pose(&mut self) -> bool {
        self.pose_edit(|pose, skel| *pose = pose.mirrored(skel))
    }
    pub fn copy_pose(&mut self) -> bool {
        match self.selected_object.and_then(|id| self.object_pose(id)) {
            Some(p) => {
                self.clip_pose = Some(p);
                true
            }
            None => false,
        }
    }
    pub fn paste_pose(&mut self) -> bool {
        let Some(clip) = self.clip_pose.clone() else { return false };
        self.pose_edit(|pose, skel| {
            *pose = clip;
            pose.fit(skel.bones.len());
        })
    }

    // -------------------------------------------------------------- animation: frames and time

    pub fn new_animation(&mut self, name: &str) -> usize {
        self.end_group();
        self.record(Slot::Whole);
        let a = Animation::from_scene(name, &self.project.scene, &|i| self.project.bones_of(i));
        self.project.animations.push(a);
        self.current_anim = self.project.animations.len() - 1;
        self.current_frame = 0;
        self.tick = 0;
        self.current_anim
    }
    pub fn frame_start_tick(&self, i: usize) -> u32 {
        self.animation().map_or(0, |a| a.frames.iter().take(i).map(|f| f.hold.max(1)).sum())
    }
    pub fn goto_frame(&mut self, i: usize) {
        let n = self.animation().map_or(0, |a| a.frames.len());
        if n == 0 {
            return;
        }
        self.end_group();
        self.current_frame = i.min(n - 1);
        self.tick = self.frame_start_tick(self.current_frame);
        self.play_accum = 0.0;
    }
    pub fn step_frame(&mut self, d: i32) {
        let n = self.animation().map_or(0, |a| a.frames.len()) as i32;
        if n > 0 {
            self.goto_frame((self.current_frame as i32 + d).rem_euclid(n) as usize);
        }
    }
    /// Edits the structure of an animation. An undo step is created only if the animation actually changed
    /// (a failed operation, such as deleting the last frame, leaves no empty step behind).
    fn anim_edit<R>(&mut self, f: impl FnOnce(&mut Animation) -> R) -> Option<R> {
        self.end_group();
        let ai = self.current_anim;
        let before = self.project.animations.get(ai)?.clone();
        let r = f(self.project.animations.get_mut(ai)?);
        if self.project.animations[ai] != before {
            self.push_entry(Slot::Anim(ai), Data::Anim(Box::new(before)));
        }
        Some(r)
    }
    /// A new frame after the current one: same poses and objects (stop-motion continues from what was there),
    /// but without the hand drawing and without one-off repaints.
    pub fn add_frame(&mut self) -> Option<usize> {
        let cur = self.current_frame;
        let i = self.anim_edit(|a| {
            let i = a.duplicate_frame(cur)?;
            let f = &mut a.frames[i];
            f.hand_layer = None;
            f.hand_only = false;
            f.note.clear();
            f.states.iter_mut().for_each(|s| s.tex_patch.clear());
            Some(i)
        })??;
        self.goto_frame(i);
        Some(i)
    }
    pub fn duplicate_frame(&mut self) -> Option<usize> {
        let cur = self.current_frame;
        let i = self.anim_edit(|a| a.duplicate_frame(cur))??;
        self.goto_frame(i);
        Some(i)
    }
    pub fn delete_frame(&mut self) -> bool {
        let cur = self.current_frame;
        let ok = self.anim_edit(|a| a.delete_frame(cur)).unwrap_or(false);
        if ok {
            self.goto_frame(cur.saturating_sub(1).min(cur));
        }
        ok
    }
    pub fn move_frame(&mut self, from: usize, to: usize) -> bool {
        let ok = self.anim_edit(|a| a.move_frame(from, to)).unwrap_or(false);
        if ok {
            self.goto_frame(to);
        }
        ok
    }
    pub fn set_hold(&mut self, frame: usize, hold: u32) {
        self.record(Slot::Frame(self.current_anim, frame));
        if let Some(f) = self.project.animations.get_mut(self.current_anim).and_then(|a| a.frames.get_mut(frame)) {
            f.hold = hold.clamp(1, 10_000);
        }
    }
    pub fn set_tween(&mut self, frame: usize, on: bool) {
        self.record(Slot::Frame(self.current_anim, frame));
        if let Some(f) = self.project.animations.get_mut(self.current_anim).and_then(|a| a.frames.get_mut(frame)) {
            f.tween = on;
        }
    }
    pub fn set_hand_only(&mut self, on: bool) {
        self.record(Slot::Frame(self.current_anim, self.current_frame));
        if let Some(f) = self.project.animations.get_mut(self.current_anim).and_then(|a| a.frames.get_mut(self.current_frame)) {
            f.hand_only = on;
        }
    }
    pub fn clear_hand_layer(&mut self) {
        self.record(Slot::Frame(self.current_anim, self.current_frame));
        if let Some(f) = self.project.animations.get_mut(self.current_anim).and_then(|a| a.frames.get_mut(self.current_frame)) {
            f.hand_layer = None;
        }
    }
    pub fn set_frame_note(&mut self, note: &str) {
        self.record(Slot::Frame(self.current_anim, self.current_frame));
        if let Some(f) = self.project.animations.get_mut(self.current_anim).and_then(|a| a.frames.get_mut(self.current_frame)) {
            f.note = note.to_string();
        }
    }
    pub fn set_fps(&mut self, num: u32, den: u32) {
        self.anim_edit(|a| {
            a.fps_num = num.clamp(1, 1000);
            a.fps_den = den.clamp(1, 1001);
        });
    }
    /// Sets the scene camera and the camera of every frame to the viewport camera (each frame keeps its field of view).
    /// Needed when the first object is added: before that no camera could have been placed deliberately, and a frame
    /// shot with the default camera past a large or offset model looks as if rendering were broken.
    fn camera_everywhere_from_view(&mut self) {
        let cam = self.view.camera();
        self.project.scene.camera = cam;
        for a in &mut self.project.animations {
            for f in &mut a.frames {
                let fov = f.camera.fov_y;
                f.camera = Camera { fov_y: fov, ..cam };
            }
        }
    }
    /// Sets the current frame's camera to the viewport camera.
    pub fn camera_from_view(&mut self) {
        let cam = self.view.camera();
        if self.animation().is_some_and(|a| self.current_frame < a.frames.len()) {
            self.record(Slot::Frame(self.current_anim, self.current_frame));
            if let Some(f) = self.project.animations.get_mut(self.current_anim).and_then(|a| a.frames.get_mut(self.current_frame)) {
                let fov = f.camera.fov_y;
                f.camera = Camera { fov_y: fov, ..cam };
            }
        } else {
            self.record(Slot::Scene);
            self.project.scene.camera = cam;
        }
    }
    /// Sets the viewport camera to the current frame's camera.
    pub fn view_from_camera(&mut self) {
        if let Some(f) = self.frame() {
            self.view = ViewCam::from_camera(&f.camera);
        }
    }
    pub fn set_frame_fov(&mut self, fov_deg: f32) {
        self.record(Slot::Frame(self.current_anim, self.current_frame));
        if let Some(f) = self.project.animations.get_mut(self.current_anim).and_then(|a| a.frames.get_mut(self.current_frame)) {
            f.camera.fov_y = fov_deg.clamp(10.0, 120.0).to_radians();
        }
    }
    /// Sets the scene style (used by every frame that has no style override of its own).
    pub fn set_scene_style(&mut self, style: PsxStyle) {
        self.record(Slot::Scene);
        self.project.scene.style = style;
    }
    /// Sets time of day and shadows of the scene (undoable like any other scene edit).
    pub fn set_lighting(&mut self, lighting: crate::lighting::Lighting) {
        self.record(Slot::Scene);
        self.project.scene.lighting = lighting;
    }
    /// Overrides the style for the current frame only (`None` removes the override).
    pub fn set_frame_style(&mut self, style: Option<PsxStyle>) {
        self.record(Slot::Frame(self.current_anim, self.current_frame));
        if let Some(f) = self.project.animations.get_mut(self.current_anim).and_then(|a| a.frames.get_mut(self.current_frame)) {
            f.style = style;
        }
    }

    pub fn play(&mut self) {
        if self.animation().is_some_and(|a| !a.frames.is_empty()) {
            self.end_group();
            self.playing = true;
            self.play_accum = 0.0;
        }
    }
    pub fn pause(&mut self) {
        self.playing = false;
    }
    pub fn toggle_play(&mut self) {
        if self.playing {
            self.pause()
        } else {
            self.play()
        }
    }
    /// Advances playback time by `dt` seconds. Returns true if the picture changed.
    pub fn advance(&mut self, dt: f32) -> bool {
        if !self.playing {
            return false;
        }
        let Some(a) = self.animation() else { return false };
        let (fps, total) = (a.fps(), a.total_ticks().max(1));
        self.play_accum += dt.max(0.0) * fps;
        let whole = self.play_accum.floor();
        if whole < 1.0 {
            return false;
        }
        self.play_accum -= whole;
        let mut t = self.tick as u64 + whole as u64;
        if t >= total as u64 {
            if self.looping {
                t %= total as u64;
            } else {
                t = total as u64 - 1;
                self.playing = false;
            }
        }
        self.tick = t as u32;
        if let Some(a) = self.animation() {
            self.current_frame = a.locate(self.tick).0;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(ed: &Editor) -> Renderer {
        let mut r = Renderer::new(1, 1);
        ed.render_viewport(&mut r);
        r
    }
    fn center_hit(r: &Renderer) -> (i32, i32) {
        let (cx, cy) = (r.frame.w as i32 / 2, r.frame.h as i32 / 2);
        assert!(r.frame.hit_at(cx, cy).is_some(), "the model must be in the centre of the frame");
        (cx, cy)
    }
    #[test]
    fn first_far_model_is_visible_in_render_mode_camera() {
        let mut ed = Editor::new();
        // a small cube far from the origin: the default frame camera would not see it
        let mut m = modeling::model_from_mesh("Distant", modeling::cube(Vec3::new(1.0, 1.0, 1.0)), 16);
        for v in &mut m.meshes[0].verts {
            v.pos.x += 300.0;
        }
        ed.import_model(m);
        ed.set_mode(Mode::Render);
        let mut r = Renderer::new(1, 1);
        ed.render_viewport(&mut r);
        let (cx, cy) = (r.frame.w as i32 / 2, r.frame.h as i32 / 2);
        assert!(r.frame.hit_at(cx, cy).is_some(), "the animation frame must look at the first imported model");
    }
    fn cube_editor() -> Editor {
        let mut ed = Editor::new();
        ed.add_primitive(Primitive::Cube);
        ed
    }
    /// An "arm" skeleton: root, shoulder (1 higher), elbow (another 1 higher); all rest rotations are zero.
    fn rigged() -> Editor {
        let mut ed = Editor::new();
        let mut m = modeling::model_from_mesh("Arm", modeling::cube(Vec3::new(0.5, 2.0, 0.5)), 32);
        let b = |name: &str, parent: Option<usize>, y: f32| Bone { name: name.into(), parent, rest_t: Vec3::new(0.0, y, 0.0), rest_r: Quat::IDENTITY, rest_s: Vec3::ONE };
        m.skeleton = Some(Skeleton { bones: vec![b("root", None, 0.0), b("shoulder", Some(0), 1.0), b("elbow", Some(1), 1.0)] });
        ed.import_model(m);
        ed
    }
    fn elbow_world(ed: &Editor) -> Vec3 {
        let id = ed.selected_object.unwrap();
        let sk = ed.selected_model().unwrap().skeleton.clone().unwrap();
        let w = world_matrices(&sk, &ed.object_pose(id).unwrap());
        w[2].transform_point(Vec3::ZERO)
    }

    #[test]
    fn import_adds_texture_places_object_and_selects() {
        let ed = cube_editor();
        assert_eq!(ed.project.scene.objects.len(), 1);
        assert_eq!(ed.selected_object, Some(1));
        assert!(!ed.selected_model().unwrap().textures.is_empty());
        assert!(ed.dirty);
        // a model without a texture and without UVs gets both
        let mut m = modeling::cube(Vec3::new(1.0, 1.0, 1.0));
        m.verts.iter_mut().for_each(|v| v.uv = Vec2::default());
        let mut model = modeling::model_from_mesh("x", m, 8);
        model.textures.clear();
        model.materials[0].texture = None;
        let mut ed = Editor::new();
        ed.import_model(model);
        let mm = ed.selected_model().unwrap();
        assert_eq!(mm.textures.len(), 1);
        assert!(mm.meshes[0].verts.iter().any(|v| v.uv != Vec2::default()));
    }

    #[test]
    fn paint_on_model_changes_texture_and_one_undo_reverts_whole_stroke() {
        let mut ed = cube_editor();
        let before = ed.selected_model().unwrap().textures[0].clone();
        let r = rendered(&ed);
        let (cx, cy) = center_hit(&r);
        ed.brush = Brush { color: [255, 0, 255, 255], size: 2.0, ..Default::default() };
        assert!(ed.paint_press(&r.frame, cx, cy));
        let dabs = ed.paint_drag(&r.frame, (cx, cy), (cx + 30, cy + 10));
        ed.paint_release();
        assert!(dabs > 5);
        let after = ed.selected_model().unwrap().textures[0].clone();
        assert_ne!(after, before);
        assert!(after.px.iter().filter(|p| **p == [255, 0, 255, 255]).count() > 4);
        assert!(ed.undo());
        assert_eq!(ed.selected_model().unwrap().textures[0], before, "a whole stroke is undone in one step");
        assert!(ed.redo());
        assert_eq!(ed.selected_model().unwrap().textures[0], after);
    }

    #[test]
    fn eyedropper_takes_color_from_model() {
        let mut ed = cube_editor();
        let r = rendered(&ed);
        let (cx, cy) = center_hit(&r);
        ed.brush.tool = Tool::Picker;
        ed.brush.color = [1, 2, 3, 255];
        assert!(ed.paint_press(&r.frame, cx, cy));
        ed.paint_release();
        assert_ne!(ed.brush.color, [1, 2, 3, 255]);
        let depth = ed.undo.len();
        ed.paint_release();
        assert_eq!(ed.undo.len(), depth, "the eyedropper records nothing in the history");
    }

    #[test]
    fn frame_patch_is_per_frame_and_base_is_untouched() {
        let mut ed = cube_editor();
        ed.set_mode(Mode::Animate);
        ed.paint_layer = PaintLayer::FramePatch;
        let base = ed.selected_model().unwrap().textures[0].clone();
        ed.add_frame();
        ed.goto_frame(1);
        let r = rendered(&ed);
        let (cx, cy) = center_hit(&r);
        ed.brush = Brush { color: [0, 255, 0, 255], size: 3.0, ..Default::default() };
        assert!(ed.paint_press(&r.frame, cx, cy));
        ed.paint_release();
        assert_eq!(ed.selected_model().unwrap().textures[0], base, "the base texture is untouched");
        let id = ed.selected_object.unwrap();
        let patch = |ed: &Editor, f: usize| ed.animation().unwrap().frames[f].states.iter().find(|s| s.object == id).unwrap().tex_patch.len();
        assert_eq!(patch(&ed, 1), 1);
        assert_eq!(patch(&ed, 0), 0);
        // undo removes the repaint itself
        assert!(ed.undo());
        assert_eq!(patch(&ed, 1), 0);
    }

    #[test]
    fn emotions_swap_texture_per_frame() {
        let mut ed = cube_editor();
        ed.set_mode(Mode::Animate);
        let calm = ed.add_emotion("Calm").unwrap();
        ed.texture_op(Canvas::Emotion { asset: 0, idx: calm }, TexOp::Fill([255, 0, 0, 255]));
        ed.paint_layer = PaintLayer::Base;
        let r0 = rendered(&ed);
        ed.add_frame();
        let id = ed.selected_object.unwrap();
        ed.set_frame_emotion(id, Some(calm));
        let r1 = rendered(&ed);
        assert_ne!(r0.frame.color, r1.frame.color, "an emotion must change the picture of the frame");
        // deleting an emotion returns the frame to the base texture
        assert!(ed.delete_emotion(calm));
        assert_eq!(rendered(&ed).frame.color, r0.frame.color);
    }

    #[test]
    fn hand_layer_draws_over_3d_and_hand_only_hides_model() {
        let mut ed = cube_editor();
        ed.set_mode(Mode::Animate);
        let c = ed.hand_canvas().unwrap();
        ed.brush = Brush { color: [0, 0, 255, 255], size: 4.0, ..Default::default() };
        ed.canvas_press(c, (10.0, 10.0));
        ed.canvas_release(c, (60.0, 10.0));
        let r = rendered(&ed);
        assert_eq!(r.frame.color[10 * r.frame.w as usize + 30], [0, 0, 255, 255]);
        ed.set_hand_only(true);
        let r = rendered(&ed);
        let (cx, cy) = ((r.frame.w / 2) as usize, (r.frame.h / 2) as usize);
        assert!(!r.frame.hit_at(cx as i32, cy as i32).is_some(), "in drawing-only mode the 3D model is hidden");
        assert!(ed.undo()); // reverts drawing-only mode
        assert!(!ed.frame().unwrap().hand_only);
        assert!(ed.frame().unwrap().hand_layer.is_some());
        assert!(ed.undo()); // reverts the whole stroke
        assert!(ed.frame().unwrap().hand_layer.is_none());
    }

    #[test]
    fn shape_tool_previews_without_leaving_trails() {
        let mut ed = cube_editor();
        let c = ed.active_canvas().unwrap();
        ed.brush = Brush { tool: Tool::RectFilled, color: [9, 9, 9, 255], ..Default::default() };
        ed.canvas_press(c, (2.0, 2.0));
        ed.canvas_drag(c, (30.0, 30.0));
        ed.canvas_drag(c, (6.0, 6.0));
        ed.canvas_release(c, (6.0, 6.0));
        let t = ed.canvas_texture(c).unwrap();
        assert_eq!(t.px.iter().filter(|p| **p == [9, 9, 9, 255]).count(), 5 * 5, "only the last rectangle is left");
    }

    #[test]
    fn bone_rotation_in_world_axis_moves_child_correctly() {
        let mut ed = rigged();
        ed.set_mode(Mode::Pose);
        assert!((elbow_world(&ed) - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-5);
        // shoulder rotated +90° around the world Z axis: the elbow (0,1,0) relative to the shoulder becomes (-1,0,0); the shoulder is at (0,1,0)
        assert!(ed.rotate_bone_world(1, Vec3::new(0.0, 0.0, 1.0), 90.0));
        assert!((elbow_world(&ed) - Vec3::new(-1.0, 1.0, 0.0)).length() < 1e-4, "{:?}", elbow_world(&ed));
        // rotating again around the same world axis accumulates (it is not a rotation around the already-rotated local axis)
        assert!(ed.rotate_bone_world(1, Vec3::new(0.0, 0.0, 1.0), 90.0));
        assert!((elbow_world(&ed) - Vec3::new(0.0, 0.0, 0.0)).length() < 1e-4, "{:?}", elbow_world(&ed));
        // even with a rotated root the axis stays a world axis
        ed.reset_pose();
        ed.rotate_bone_world(0, Vec3::new(0.0, 1.0, 0.0), 90.0);
        ed.rotate_bone_world(2, Vec3::new(0.0, 0.0, 1.0), 90.0);
        // rotating the root around Y does not change the vertical; the elbow stays at (0,2,0) because it spins around itself
        assert!((elbow_world(&ed) - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-4);
        assert!(ed.undo());
    }

    #[test]
    fn rotation_respects_object_transform() {
        let mut ed = rigged();
        ed.set_mode(Mode::Pose);
        let id = ed.selected_object.unwrap();
        let mut t = ed.object_transform(id).unwrap();
        t.rot_deg = Vec3::new(0.0, 90.0, 0.0); // the object is rotated around Y
        ed.set_object_transform(id, t.clone());
        // the world X axis (right on screen) corresponds to the model Z axis (because the object is rotated)
        ed.rotate_bone_world(1, Vec3::new(1.0, 0.0, 0.0), 90.0);
        let sk = ed.selected_model().unwrap().skeleton.clone().unwrap();
        let w = world_matrices(&sk, &ed.object_pose(id).unwrap());
        let elbow_world = t.matrix().transform_point(w[2].transform_point(Vec3::ZERO));
        // world X axis: the elbow (0,1,0) relative to the shoulder rotates 90° around X: (0,1,0) to (0,0,1), so in world space (0,1,1)
        assert!((elbow_world - Vec3::new(0.0, 1.0, 1.0)).length() < 1e-4, "{elbow_world:?}");
    }

    #[test]
    fn bone_pick_and_pose_clipboard() {
        let mut ed = rigged();
        ed.set_mode(Mode::Pose);
        let r = rendered(&ed);
        let sb = ed.screen_bones(r.frame.w, r.frame.h);
        assert_eq!(sb.len(), 3);
        let elbow = sb.iter().find(|b| b.bone == 2).unwrap();
        assert_eq!(ed.pick_bone(elbow.tail.0 + 1.0, elbow.tail.1, r.frame.w, r.frame.h, 12.0), Some(2));
        assert_eq!(ed.pick_bone(0.0, 0.0, r.frame.w, r.frame.h, 5.0), None);
        ed.rotate_bone_world(1, Vec3::new(0.0, 0.0, 1.0), 40.0);
        assert!(ed.copy_pose());
        ed.add_frame();
        ed.reset_pose();
        let id = ed.selected_object.unwrap();
        assert_eq!(ed.object_pose(id).unwrap(), Pose::rest(3));
        assert!(ed.paste_pose());
        assert_ne!(ed.object_pose(id).unwrap(), Pose::rest(3));
        // a pose cannot be edited without a skeleton
        let mut ed2 = cube_editor();
        ed2.set_mode(Mode::Pose);
        assert!(!ed2.reset_pose());
    }

    #[test]
    fn timeline_and_playback() {
        let mut ed = cube_editor();
        ed.set_mode(Mode::Animate);
        ed.set_fps(10, 1);
        assert_eq!(ed.animation().unwrap().frames.len(), 1);
        ed.add_frame();
        ed.add_frame();
        assert_eq!(ed.current_frame, 2);
        ed.set_hold(0, 3);
        ed.set_tween(1, true);
        assert_eq!(ed.animation().unwrap().total_ticks(), 5);
        assert_eq!(ed.frame_start_tick(2), 4);
        ed.goto_frame(0);
        ed.play();
        // 0.35 s at 10 fps is 3 ticks: still frame 0 (held for 3 ticks), then frame 1
        assert!(ed.advance(0.35));
        assert_eq!(ed.tick, 3);
        assert_eq!(ed.current_frame, 1);
        ed.advance(0.1);
        assert_eq!(ed.current_frame, 2);
        ed.advance(0.1); // the end wraps around to the loop start
        assert_eq!(ed.tick, 0);
        ed.looping = false;
        ed.advance(10.0);
        assert!(!ed.playing);
        assert_eq!(ed.tick, 4);
        // reordering and deleting
        assert!(ed.move_frame(2, 0));
        assert_eq!(ed.current_frame, 0);
        assert!(ed.delete_frame());
        assert!(ed.delete_frame());
        assert!(!ed.delete_frame(), "the last frame cannot be deleted");
        assert!(ed.undo());
        assert_eq!(ed.animation().unwrap().frames.len(), 2);
    }

    #[test]
    fn new_object_appears_in_existing_animation_frames() {
        let mut ed = cube_editor();
        ed.set_mode(Mode::Animate);
        ed.add_frame();
        let id2 = ed.add_primitive(Primitive::Sphere);
        for f in &ed.animation().unwrap().frames {
            assert!(f.states.iter().any(|s| s.object == id2));
        }
        // objects 2 and 3 are distinct in the pick buffer (an earlier 0xFFFE mask merged them)
        let id3 = ed.add_primitive(Primitive::Cone);
        assert_ne!(hit_object_id(id2), hit_object_id(id3));
        assert!(ed.delete_object(id2));
        assert!(ed.animation().unwrap().frames.iter().all(|f| f.states.iter().all(|s| s.object != id2)));
        assert!(ed.undo());
        assert!(ed.animation().unwrap().frames.iter().all(|f| f.states.iter().any(|s| s.object == id2)));
    }

    #[test]
    fn transform_edits_go_to_frame_in_animate_and_scene_in_model_mode() {
        let mut ed = cube_editor();
        let id = ed.selected_object.unwrap();
        let mut t = ed.object_transform(id).unwrap();
        t.pos.x = 3.0;
        ed.set_object_transform(id, t.clone());
        assert_eq!(ed.project.scene.object(id).unwrap().transform.pos.x, 3.0);
        ed.set_mode(Mode::Animate);
        ed.add_frame();
        t.pos.x = 7.0;
        ed.set_object_transform(id, t);
        assert_eq!(ed.project.scene.object(id).unwrap().transform.pos.x, 3.0, "the scene is untouched");
        assert_eq!(ed.frame().unwrap().states[0].transform.pos.x, 7.0);
        assert_eq!(ed.animation().unwrap().frames[0].states[0].transform.pos.x, 3.0);
    }

    #[test]
    fn butterfly_primitive_is_embedded_textured_and_survives_save_and_reopen() {
        let mut ed = Editor::new();
        let id = ed.add_primitive(Primitive::Butterfly);
        let m = ed.selected_model().unwrap();
        assert_eq!(m.triangle_count(), 12);
        assert_eq!((m.textures[0].w, m.textures[0].h), (128, 256));
        assert!(m.textures[0].px.iter().any(|p| p[3] == 0) && m.textures[0].px.iter().any(|p| p[3] == 255), "the picture has transparent and solid parts");
        assert_eq!(m.materials[0].texture, Some(0));
        let (lo, hi) = m.bounds().unwrap();
        assert!(((hi.x - lo.x) - 1.0).abs() < 1e-3 && ((lo.x + hi.x) * 0.5).abs() < 1e-3, "one unit of wingspan, centred");
        let first = rendered(&ed);
        let blue = |r: &Renderer| r.frame.color.iter().filter(|c| c[2] > 120 && c[2] as u32 > c[0] as u32 + 60).count();
        assert!(blue(&first) > 20, "the wing texture is visible: {} blue pixels", blue(&first));
        // save and reopen: geometry, texture and the object itself come back unchanged
        let bytes = ed.project.to_bytes();
        let back = crate::project::Project::from_bytes(&bytes).unwrap();
        assert_eq!(back, ed.project);
        ed.project = back; // same view as before, so the pictures must be identical
        ed.select_object(Some(id));
        assert_eq!(rendered(&ed).frame.color, first.frame.color, "the reopened butterfly looks exactly as before");
    }
    #[test]
    fn modeling_ops_are_undoable_and_work_on_faces() {
        let mut ed = Editor::new();
        ed.add_primitive(Primitive::Sphere);
        let n0 = ed.selected_model().unwrap().triangle_count();
        let (b, a) = ed.decimate_selected(0.5).unwrap();
        assert!(a < b && b == n0);
        assert!(ed.undo());
        assert_eq!(ed.selected_model().unwrap().triangle_count(), n0);

        ed.add_primitive(Primitive::Cube);
        let r = rendered(&ed);
        let (cx, cy) = center_hit(&r);
        ed.mode = Mode::Model;
        ed.click_select(&r.frame, cx, cy, false);
        assert_eq!(ed.selected_faces.len(), 1);
        let tris = ed.selected_model().unwrap().triangle_count();
        assert!(ed.extrude_selected_faces(0.5));
        assert!(ed.selected_model().unwrap().triangle_count() > tris);
        assert_eq!(ed.selected_face_world_tris().len(), ed.selected_faces.len());
        let top = ed.selected_faces.clone();
        assert!(ed.move_selected_faces(Vec3::new(0.0, 0.25, 0.0)));
        assert_eq!(ed.selected_faces, top, "the selection is kept while moving");
        assert!(ed.delete_selected_faces());
        assert!(ed.selected_faces.is_empty());
        // three operations are three undo steps, and everything returns to the original cube
        ed.undo();
        ed.undo();
        ed.undo();
        assert_eq!(ed.selected_model().unwrap().triangle_count(), tris);
    }

    #[test]
    fn camera_helpers_roundtrip() {
        let v = ViewCam { target: Vec3::new(1.0, 2.0, 3.0), yaw: 0.7, pitch: 0.3, dist: 6.0 };
        let c = ViewCam::from_camera(&v.camera());
        assert!((c.yaw - v.yaw).abs() < 1e-4 && (c.pitch - v.pitch).abs() < 1e-4 && (c.dist - v.dist).abs() < 1e-3);
        let mut v2 = v;
        v2.pan(10.0, 0.0, 300.0);
        assert!((v2.target - v.target).length() > 0.0);
        v2.zoom(100.0);
        assert!(v2.dist >= 0.05);
        v2.orbit(0.0, 1e6);
        assert!(v2.pitch <= 1.55);
        let mut ed = cube_editor();
        ed.set_mode(Mode::Animate);
        ed.view = v;
        ed.camera_from_view();
        let cam = ed.frame().unwrap().camera;
        assert!((cam.eye - v.camera().eye).length() < 1e-4);
        ed.view = ViewCam::default();
        ed.view_from_camera();
        assert!((ed.view.dist - 6.0).abs() < 1e-2);
    }

    #[test]
    fn save_open_roundtrip_keeps_everything_and_dirty_flag() {
        let mut ed = cube_editor();
        ed.set_mode(Mode::Animate);
        ed.add_emotion("Joy");
        ed.add_frame();
        let dir = std::env::temp_dir().join(format!("jank_ed_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.jank");
        assert!(ed.save().is_err(), "without a path there is nowhere to save");
        ed.save_as(&p).unwrap();
        assert!(!ed.dirty);
        ed.add_frame();
        assert!(ed.dirty);
        let loaded = Editor::open(&p).unwrap();
        assert_eq!(loaded.project.animations[0].frames.len(), 2);
        assert_eq!(loaded.project.assets[0].emotions[0].name, "Joy");
        assert!(loaded.selected_object.is_some());
        assert!(Editor::open(&dir.join("missing.jank")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn texture_ops_and_new_texture() {
        let mut ed = cube_editor();
        let c = ed.active_canvas().unwrap();
        assert!(ed.texture_op(c, TexOp::Resize(16, 16)));
        assert_eq!(ed.canvas_texture(c).unwrap().w, 16);
        assert!(ed.texture_op(c, TexOp::Fill([1, 2, 3, 255])));
        assert!(ed.texture_op(c, TexOp::FlipH));
        ed.undo();
        ed.undo();
        ed.undo();
        assert_eq!(ed.canvas_texture(c).unwrap().w, 64);
        assert_eq!(ed.new_texture(8, 8), Some(1));
        assert_eq!(ed.selected_model().unwrap().textures.len(), 2);
    }

    #[test]
    fn texture_png_export_import_roundtrip_is_one_undo_step() {
        let mut ed = cube_editor();
        let c = ed.active_canvas().unwrap();
        ed.texture_op(c, TexOp::Fill([10, 200, 30, 255]));
        let dir = std::env::temp_dir().join(format!("jank_tx_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.png");
        ed.export_texture_png(c, &p).unwrap();
        assert!(!dir.join("t.png.part").exists());
        let saved = ed.canvas_texture(c).unwrap().into_owned();
        ed.texture_op(c, TexOp::Fill([1, 1, 1, 255]));
        ed.import_image_into(c, &p).unwrap();
        assert_eq!(ed.canvas_texture(c).unwrap().px, saved.px);
        ed.undo();
        assert_eq!(ed.canvas_texture(c).unwrap().px[0], [1, 1, 1, 255]);
        assert!(ed.import_image_into(c, &dir.join("missing.png")).is_err());
        std::fs::write(dir.join("bad.png"), b"not a png").unwrap();
        assert!(ed.import_image_into(c, &dir.join("bad.png")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sculpt_moves_vertex_once_per_group_and_world_dir_follows_object() {
        let mut ed = cube_editor();
        let r = rendered(&ed);
        let (cx, cy) = center_hit(&r);
        let before = ed.selected_model().unwrap().clone();
        assert!(ed.sculpt_begin(&r.frame, cx, cy));
        for _ in 0..5 {
            assert!(ed.sculpt_drag(Vec3::new(0.0, 0.05, 0.0), 0.3));
        }
        ed.sculpt_end();
        assert_ne!(*ed.selected_model().unwrap(), before);
        assert!(ed.undo());
        assert_eq!(*ed.selected_model().unwrap(), before, "all sculpting of one drag is undone in one step");
        assert!(!ed.sculpt_drag(Vec3::ZERO, 0.1), "without begin there is nothing to sculpt");
        let id = ed.selected_object.unwrap();
        let mut t = ed.object_transform(id).unwrap();
        t.rot_deg = Vec3::new(0.0, 90.0, 0.0);
        t.scale = Vec3::new(2.0, 2.0, 2.0);
        ed.set_object_transform(id, t);
        // a 90° rotation around Y maps model +Z to world +X, so world X maps to model +Z, half as long because of the scale
        let d = ed.world_to_model_dir(id, Vec3::new(1.0, 0.0, 0.0));
        assert!((d - Vec3::new(0.0, 0.0, 0.5)).length() < 1e-5, "{d:?}");
    }

    #[test]
    fn scene_layers_are_painted_ordered_and_undone() {
        let mut ed = cube_editor();
        let (w, _h) = ed.render_size();
        let back = ed.add_scene_layer(true);
        let front = ed.add_scene_layer(false);
        assert_eq!((ed.project.scene.layers.len(), ed.project.scene.layers_back), (2, 1));
        assert!(ed.project.scene.layer_is_back(back) && !ed.project.scene.layer_is_back(front));
        assert_eq!(ed.project.scene.layers[0].img.w, w);
        ed.brush = Brush { color: [255, 0, 0, 255], size: 3.0, ..Default::default() };
        ed.select_scene_layer(back);
        let c = ed.scene_canvas().unwrap();
        assert_eq!(c, Canvas::Scene { layer: back });
        ed.canvas_press(c, (4.0, 4.0));
        ed.canvas_release(c, (20.0, 4.0));
        assert_eq!(ed.project.scene.layers[back].img.get(10, 4), Some([255, 0, 0, 255]));
        // the front layer is untouched, and the stroke is one undo step
        assert!(ed.project.scene.layers[front].img.px.iter().all(|p| p[3] == 0));
        assert!(ed.undo());
        assert!(ed.project.scene.layers[back].img.px.iter().all(|p| p[3] == 0), "undo removes the stroke");
        assert!(ed.redo());
        assert_eq!(ed.project.scene.layers[back].img.get(10, 4), Some([255, 0, 0, 255]));
        // a locked or hidden layer refuses to be painted on
        ed.edit_scene_layer(back, |l| l.locked = true);
        ed.canvas_press(c, (30.0, 30.0));
        ed.canvas_release(c, (31.0, 30.0));
        assert_eq!(ed.project.scene.layers[back].img.get(30, 30), Some([0, 0, 0, 0]));
        // moving the back layer up passes the 3D scene
        assert!(ed.move_scene_layer(back, true));
        assert_eq!(ed.project.scene.layers_back, 0);
        // layer structure changes are undoable too
        assert!(ed.undo());
        assert_eq!(ed.project.scene.layers_back, 1);
        assert!(ed.delete_scene_layer(front));
        assert_eq!(ed.project.scene.layers.len(), 1);
        assert!(ed.undo());
        assert_eq!(ed.project.scene.layers.len(), 2);
        // undoing an object change never throws away layer pictures
        let id = ed.selected_object.unwrap();
        let mut t = ed.object_transform(id).unwrap();
        t.pos.x += 1.0;
        ed.set_object_transform(id, t);
        assert!(ed.undo());
        assert_eq!(ed.project.scene.layers.len(), 2);
        assert_eq!(ed.project.scene.layers[0].img.get(10, 4), Some([255, 0, 0, 255]));
    }

    #[test]
    fn texture_layers_merge_into_the_model_texture() {
        let mut ed = cube_editor();
        let base_c = ed.active_canvas().unwrap();
        let before = ed.canvas_texture(base_c).unwrap().into_owned();
        assert!(ed.enable_texture_layers());
        assert!(!ed.enable_texture_layers(), "already on");
        assert_eq!(ed.texture_stack().unwrap().layers.len(), 1);
        assert!(ed.edit_texture_stack(|s| {
            s.add("paint");
        }));
        assert_eq!(ed.texture_stack().unwrap().active, 1);
        ed.brush = Brush { color: [1, 2, 3, 255], size: 1.0, pixel: true, ..Default::default() };
        ed.canvas_press(base_c, (5.5, 5.5));
        ed.canvas_release(base_c, (5.5, 5.5));
        assert_eq!(ed.canvas_texture(base_c).unwrap().get(5, 5), Some([1, 2, 3, 255]), "the merged texture shows the stroke");
        assert_eq!(ed.texture_stack().unwrap().layers[0].img.get(5, 5), before.get(5, 5), "the lower layer is untouched");
        assert_eq!(ed.texture_stack().unwrap().layers[1].img.get(5, 5), Some([1, 2, 3, 255]));
        // hiding the layer removes it from the merged picture
        assert!(ed.edit_texture_stack(|s| s.layers[1].visible = false));
        assert_eq!(ed.canvas_texture(base_c).unwrap().get(5, 5), before.get(5, 5));
        ed.canvas_press(base_c, (7.5, 7.5));
        assert_eq!(ed.canvas_texture(base_c).unwrap().get(7, 7), before.get(7, 7), "a hidden layer cannot be painted on");
        ed.canvas_release(base_c, (7.5, 7.5));
        // undo brings the layer back, then removes the stroke, then the stack
        assert!(ed.undo());
        assert_eq!(ed.canvas_texture(base_c).unwrap().get(5, 5), Some([1, 2, 3, 255]));
        assert!(ed.undo());
        assert_eq!(ed.canvas_texture(base_c).unwrap().get(5, 5), before.get(5, 5));
        assert!(ed.undo()); // add layer
        assert!(ed.undo()); // enable layers
        assert!(ed.texture_stack().is_none());
        assert_eq!(*ed.canvas_texture(base_c).unwrap(), before);
        // flipping a layered texture moves every layer
        ed.enable_texture_layers();
        ed.edit_texture_stack(|s| {
            s.add("b");
        });
        ed.texture_op(base_c, TexOp::FlipH);
        let s = ed.texture_stack().unwrap();
        assert_eq!(s.layers.len(), 2);
        assert_eq!(s.layers[0].img.get(0, 0), before.get(before.w as i32 - 1, 0));
    }

    #[test]
    fn undo_history_stays_within_the_memory_budget() {
        let mut ed = cube_editor();
        ed.add_scene_layer(true);
        ed.edit_scene_layer(0, |l| l.img = Texture::new("big", 2048, 2048, [0, 0, 0, 0])); // 16 MB per snapshot
        ed.brush = Brush { color: [9, 9, 9, 255], size: 2.0, ..Default::default() };
        let c = ed.scene_canvas().unwrap();
        for i in 0..40 {
            ed.canvas_press(c, (i as f32, 3.0));
            ed.canvas_release(c, (i as f32 + 1.0, 3.0));
        }
        let bytes: usize = ed.undo.iter().map(|e| e.data.bytes()).sum();
        assert!(bytes <= UNDO_BYTES + (16 << 20), "history holds {bytes} bytes");
        assert!(ed.undo(), "recent steps are still undoable");
    }

    fn tri_count(ed: &Editor) -> usize {
        ed.selected_model().unwrap().meshes.iter().map(|m| m.tris.len()).sum()
    }
    #[test]
    fn edit_mode_extrude_bevel_undo_and_save_keep_polygons() {
        let mut ed = cube_editor();
        let tris0 = tri_count(&ed);
        assert!(ed.edit_enter());
        assert_eq!(ed.edit_stats().unwrap().3, 6, "a cube has six quads");
        ed.edit_set_select_mode(crate::editmesh::SelectMode::Face);
        ed.edit_select_all(false);
        ed.edit_op(|m| {
            m.sel_f.insert(0);
            m.sync_selection();
        });
        assert!(ed.edit_extrude_begin((100.0, 100.0), 200, 200));
        ed.edit_drag_update((100.0, 60.0), DragSnap::default());
        ed.edit_drag_end(true);
        assert_eq!(ed.edit_stats().unwrap().3, 10, "extruding a quad adds four side faces");
        ed.edit_select_all(true);
        ed.edit_set_select_mode(crate::editmesh::SelectMode::Edge);
        assert!(ed.edit_bevel_apply(crate::editmesh::BevelOptions { width: 0.05, segments: 1, clamp: true }));
        assert!(tri_count(&ed) > tris0);
        ed.edit_exit();
        // the polygons are saved with the project and restored when editing again
        let dir = std::env::temp_dir().join(format!("jank_edit_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("e.jank");
        ed.save_as(&path).unwrap();
        let mut back = Editor::open(&path).unwrap();
        back.select_object(ed.selected_object);
        let faces = ed.project.assets[0].topology.as_ref().unwrap().1.faces.len();
        assert!(back.edit_enter());
        let st = back.edit_stats().unwrap();
        assert_eq!(st.2 + st.3 + st.4, faces);
        let _ = std::fs::remove_dir_all(&dir);
        // undo walks back to the plain cube
        while ed.can_undo() && tri_count(&ed) != tris0 {
            ed.undo();
        }
        assert_eq!(tri_count(&ed), tris0);
    }
    #[test]
    fn object_boolean_difference_cuts_and_removes_the_cutter() {
        let mut ed = cube_editor();
        let a = ed.selected_object.unwrap();
        let b = ed.add_primitive(Primitive::Cube);
        let mut t = ed.object_transform(b).unwrap();
        t.pos = Vec3::new(0.5, 0.5, 0.5);
        ed.set_object_transform(b, t);
        ed.select_object(Some(a));
        let before = tri_count(&ed);
        assert!(ed.boolean_with(b, crate::editmesh::BoolOp::Difference, true));
        assert!(ed.project.scene.object(b).is_none());
        assert!(tri_count(&ed) > before);
        assert!(ed.undo());
        assert!(ed.project.scene.object(b).is_some());
    }
    #[test]
    fn ik_drag_pulls_the_joint_towards_the_mouse() {
        let mut ed = rigged();
        ed.set_mode(Mode::Pose);
        ed.ensure_animation();
        ed.ik_chain = 2;
        let (w, h) = (400, 300);
        let start = ed.bone_joint_world(2).unwrap();
        let cam = ed.active_camera();
        let goal_world = start + Vec3::new(1.0, -0.8, 0.0);
        let s = cam.project(goal_world, w, h).unwrap();
        ed.begin_group();
        assert!(ed.ik_drag(2, s.0, s.1, w, h));
        ed.end_group();
        let end = ed.bone_joint_world(2).unwrap();
        let se = cam.project(end, w, h).unwrap();
        let d0 = ((cam.project(start, w, h).unwrap().0 - s.0).powi(2) + (cam.project(start, w, h).unwrap().1 - s.1).powi(2)).sqrt();
        let d1 = ((se.0 - s.0).powi(2) + (se.1 - s.1).powi(2)).sqrt();
        assert!(d1 < d0 * 0.3, "the joint follows the mouse: {d0} -> {d1}");
        assert!(ed.undo());
        assert!((ed.bone_joint_world(2).unwrap() - start).length() < 1e-4);
    }
    #[test]
    fn reference_pictures_show_in_the_viewport_only() {
        let mut ed = cube_editor();
        let red = Texture::new("ref", 8, 8, [255, 0, 0, 255]);
        let i = ed.ref_add(red, "ref");
        ed.ref_edit(i, |r| {
            r.on_top = true;
            r.opacity = 1.0;
            r.size = 50.0;
        });
        ed.selected_ref = None;
        let r = rendered(&ed);
        let (cx, cy) = center_hit(&r);
        assert_eq!(r.frame.color[(cy as u32 * r.frame.w + cx as u32) as usize][..3], [255, 0, 0], "drawn over the model");
        assert!(r.frame.hit_at(cx, cy).is_some(), "the picture is never picked as a model");
        assert_eq!(ed.ref_pick(cx as f32, cy as f32, r.frame.w, r.frame.h), Some(i));
        let mut fin = Renderer::new(1, 1);
        ed.render_preview(&mut fin);
        assert!(fin.frame.color.iter().all(|c| c[..3] != [255, 0, 0]), "never in the final picture");
        // behind the model when not on top
        ed.ref_edit(i, |r| {
            r.on_top = false;
            r.transform.pos = Vec3::new(0.0, 0.0, -5.0);
        });
        let r = rendered(&ed);
        assert_ne!(r.frame.color[(cy as u32 * r.frame.w + cx as u32) as usize][..3], [255, 0, 0]);
        assert!(ed.ref_remove(i));
        assert!(ed.project.scene.refs.is_empty());
        assert!(ed.undo());
        assert_eq!(ed.project.scene.refs.len(), 1);
    }
    #[test]
    fn indexed_texture_snaps_paint_and_recolours_by_palette() {
        let mut ed = cube_editor();
        let pal = crate::palette::Palette::new("two", vec![[0, 0, 0, 255], [255, 255, 255, 255]]);
        assert!(ed.indexed_enable(0, 0, pal));
        let in_pal = |ed: &Editor| ed.project.assets[0].model.textures[0].px.iter().all(|p| p[3] < 128 || p[..3] == [0, 0, 0] || p[..3] == [255, 255, 255]);
        assert!(in_pal(&ed));
        let r = rendered(&ed);
        let (cx, cy) = center_hit(&r);
        ed.brush = Brush { color: [250, 240, 230, 255], size: 3.0, pixel: false, ..Default::default() };
        assert!(ed.paint_press(&r.frame, cx, cy));
        ed.paint_drag(&r.frame, (cx, cy), (cx + 10, cy));
        ed.paint_release();
        assert!(in_pal(&ed), "painted pixels snap to the palette");
        assert!(ed.indexed_set_color(0, 0, 1, [255, 0, 0, 255]));
        let reds = ed.project.assets[0].model.textures[0].px.iter().filter(|p| p[..3] == [255, 0, 0]).count();
        assert!(reds > 0);
        assert!(ed.undo());
        assert_eq!(ed.project.assets[0].model.textures[0].px.iter().filter(|p| p[..3] == [255, 0, 0]).count(), 0);
        assert!(!ed.enable_texture_layers(), "layers and indexed colour exclude each other");
    }
    #[test]
    fn projection_paint_is_one_undo_step() {
        let mut ed = cube_editor();
        let before = ed.selected_model().unwrap().textures[0].clone();
        let r = rendered(&ed);
        let (cx, cy) = center_hit(&r);
        ed.project_paint = true;
        ed.brush = Brush { color: [0, 255, 0, 255], size: 4.0, pixel: true, ..Default::default() };
        assert!(ed.paint_press(&r.frame, cx, cy));
        assert!(ed.proj_radius().unwrap() >= 1.0);
        assert!(ed.paint_drag(&r.frame, (cx, cy), (cx + 20, cy + 5)) > 0);
        ed.paint_release();
        let after = ed.selected_model().unwrap().textures[0].clone();
        assert!(after.px.iter().filter(|p| **p == [0, 255, 0, 255]).count() > 4);
        assert!(ed.undo());
        assert_eq!(ed.selected_model().unwrap().textures[0], before);
    }
    #[test]
    fn graph_keys_and_easing() {
        let mut ed = cube_editor();
        let id = ed.selected_object.unwrap();
        ed.set_mode(Mode::Animate);
        ed.add_frame();
        let ch = crate::anim::Channel { object: id, kind: crate::anim::ChannelKind::Pos(0) };
        assert!(ed.graph_set_key(1, &ch, 4.0));
        assert_eq!(ed.graph_key(1, &ch), Some(4.0));
        assert!(ed.graph_channels().contains(&ch));
        let hold = ed.animation().unwrap().frames[0].hold as f32;
        assert!(ed.graph_set_ease(0, crate::anim::Ease::LINEAR));
        let lin = ed.graph_sample(&ch, hold * 0.25).unwrap();
        assert!((lin - 1.0).abs() < 0.05, "linear in-between: {lin}");
        ed.graph_set_ease(0, crate::anim::Ease::EASE_IN);
        let slow = ed.graph_sample(&ch, hold * 0.25).unwrap();
        assert!(slow < lin, "ease-in starts slower: {slow} < {lin}");
    }
}
