//! Edit mode of Model mode: the polygons, edges and vertices of one object are edited directly
//! in the viewport, as in Blender. The edit mesh ([`EditMesh`]) is the working copy; after every
//! change it is written back to the triangle meshes of the model, so the viewport (and every other
//! mode) always shows the result. Modal tools (grab, rotate, scale, extrude, inset, bevel, loop
//! cut, shrink/fatten) follow the mouse until they are confirmed or cancelled.
use super::*;
use crate::editmesh::{BevelOptions, BoolOp, DeleteKind, Edge, EditMesh, MergeKind, SelectMode};
use std::collections::HashMap;

/// An element under the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshHit {
    Vert(u32),
    Edge(Edge),
    Face(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshDragKind {
    Move,
    Rotate,
    Scale,
    ShrinkFatten,
    Inset,
    Bevel,
    BevelVerts,
    /// Sliding a freshly placed loop cut.
    LoopSlide,
}

/// Constraint of a modal transform (X, Y, Z; Shift+X = everything but X; extrusions move along
/// their normal).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum AxisLock {
    #[default]
    Free,
    Axis(usize),
    Plane(usize),
    Normal,
}

/// How a click changes the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickMod {
    Replace,
    Extend,
    /// Alt: edge loop (face loop in Face mode).
    Loop,
    /// Ctrl+Alt: edge ring.
    Ring,
    /// Ctrl: shortest path from the active vertex.
    Path,
}

/// Snapping during a modal transform.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct DragSnap {
    /// Round to steps (move: `step`, rotate: 5°, scale: 0.1).
    pub increment: bool,
    pub step: f32,
    /// Snap the moved selection onto the vertex under the cursor.
    pub vertex: bool,
    /// Ten times slower (Shift).
    pub precise: bool,
}

pub struct MeshDrag {
    pub kind: MeshDragKind,
    pub axis: AxisLock,
    /// Typed value (as in Blender: G 2 Enter).
    pub numeric: String,
    /// The amount currently applied (distance, angle, factor, width), for the status bar.
    pub value: f32,
    pub segments: usize,
    start: (f32, f32),
    last: (f32, f32),
    pivot: Vec3,
    pivot_px: (f32, f32),
    normal: Vec3,
    base: EditMesh,
    weights: Vec<(u32, f32)>,
    mirror: HashMap<u32, u32>,
    loop_edge: Option<(Edge, usize)>,
    w: u32,
    h: u32,
    snap: DragSnap,
}

pub struct EditSession {
    pub object: u32,
    pub asset: usize,
    pub mesh: EditMesh,
    /// Per mesh of the model: triangle -> polygon.
    pub tri_face: Vec<Vec<u32>>,
    pub drag: Option<MeshDrag>,
    /// See through the mesh: select hidden elements too.
    pub xray: bool,
    pub proportional: bool,
    /// Radius of proportional editing in world units.
    pub prop_radius: f32,
    /// Mirror vertex moves across X (left and right halves stay symmetric).
    pub mirror_x: bool,
    pub hover: Option<MeshHit>,
    /// Points of the knife line in frame pixels.
    pub knife: Vec<(f32, f32)>,
    /// Loop cut preview: hovered edge, cuts and the preview segments in model space.
    pub loop_preview: Option<(Edge, usize, Vec<(Vec3, Vec3)>)>,
    pub bevel_segments: usize,
    pub inset_individual: bool,
}

/// Screen positions of the elements for drawing the Edit-mode overlay.
#[derive(Default, Clone, Debug)]
pub struct EditOverlay {
    /// (x, y, selected, visible)
    pub verts: Vec<(f32, f32, bool, bool)>,
    /// (a, b, flags): bit 0 selected, 1 seam, 2 sharp, 3 visible, 4 loose
    pub edges: Vec<((f32, f32), (f32, f32), u8)>,
    /// Face centre dots (Face mode): (x, y, selected, visible)
    pub dots: Vec<(f32, f32, bool, bool)>,
    pub active: Option<(f32, f32)>,
    pub hover: Vec<((f32, f32), (f32, f32))>,
    pub hover_dot: Option<(f32, f32)>,
    pub loop_preview: Vec<((f32, f32), (f32, f32))>,
}

pub const EDGE_SEL: u8 = 1;
pub const EDGE_SEAM: u8 = 2;
pub const EDGE_SHARP: u8 = 4;
pub const EDGE_VISIBLE: u8 = 8;
pub const EDGE_LOOSE: u8 = 16;

fn smoothstep_falloff(t: f32) -> f32 {
    let x = (1.0 - t).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-9 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    let (qx, qy) = (a.0 + dx * t, a.1 + dy * t);
    (((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt(), t)
}
fn axis_vec(i: usize) -> Vec3 {
    match i {
        0 => Vec3::new(1.0, 0.0, 0.0),
        1 => Vec3::new(0.0, 1.0, 0.0),
        _ => Vec3::new(0.0, 0.0, 1.0),
    }
}
/// Point inside polygon (even-odd rule).
pub fn point_in_poly(p: (f32, f32), poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1 + 1e-12) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

impl Editor {
    // -------------------------------------------------------------- entering and leaving

    pub fn edit_active(&self) -> bool {
        self.edit.is_some()
    }
    /// Enters Edit mode for the selected object (Model mode only).
    pub fn edit_enter(&mut self) -> bool {
        if self.edit.is_some() {
            return true;
        }
        if self.mode != Mode::Model {
            self.set_mode(Mode::Model);
        }
        let Some(object) = self.selected_object else {
            self.status = "Select an object to edit its mesh".into();
            return false;
        };
        let Some(asset) = self.selected_asset() else { return false };
        let a = &self.project.assets[asset];
        let fp = EditMesh::fingerprint(&a.model.meshes);
        let mut mesh = match &a.topology {
            Some(t) if t.0 == fp => t.1.clone(),
            _ => EditMesh::from_meshes(&a.model.meshes),
        };
        mesh.sync_selection();
        self.end_group();
        self.selected_faces.clear();
        let tri_face = mesh.to_meshes().1;
        self.edit = Some(EditSession {
            object,
            asset,
            mesh,
            tri_face,
            drag: None,
            xray: false,
            proportional: false,
            prop_radius: 1.0,
            mirror_x: false,
            hover: None,
            knife: vec![],
            loop_preview: None,
            bevel_segments: 1,
            inset_individual: false,
        });
        self.edit_commit(true);
        self.status = "Edit mode: 1/2/3 vertex, edge, face · G R S move, rotate, scale · E extrude · Tab to leave".into();
        true
    }
    /// Leaves Edit mode (the mesh is already written back after every change).
    pub fn edit_exit(&mut self) {
        if self.edit.as_ref().is_some_and(|s| s.drag.is_some()) {
            self.edit_drag_end(true);
        }
        if self.edit.is_some() {
            self.edit_commit(true);
        }
        self.edit = None;
        self.end_group();
    }
    pub fn edit_toggle(&mut self) -> bool {
        if self.edit.is_some() {
            self.edit_exit();
            false
        } else {
            self.edit_enter()
        }
    }
    /// Writes the edit mesh into the model. `full` also stores the polygons with the asset (for
    /// undo and saving); during a drag only the triangles are refreshed.
    fn edit_commit(&mut self, full: bool) {
        let Some(s) = &mut self.edit else { return };
        let (meshes, map) = s.mesh.to_meshes();
        s.tri_face = map;
        let Some(asset) = self.project.assets.get_mut(s.asset) else { return };
        asset.model.meshes = meshes;
        if full {
            let fp = EditMesh::fingerprint(&asset.model.meshes);
            asset.topology = Some(Box::new((fp, s.mesh.clone())));
        }
        self.dirty = true;
    }
    /// After undo/redo: the session follows the model.
    pub(super) fn edit_resync(&mut self) {
        let Some(s) = &mut self.edit else { return };
        let Some(a) = self.project.assets.get(s.asset) else {
            self.edit = None;
            return;
        };
        if self.project.scene.object(s.object).is_none() {
            self.edit = None;
            return;
        }
        let fp = EditMesh::fingerprint(&a.model.meshes);
        s.mesh = match &a.topology {
            Some(t) if t.0 == fp => t.1.clone(),
            _ => EditMesh::from_meshes(&a.model.meshes),
        };
        s.drag = None;
        s.tri_face = s.mesh.to_meshes().1;
    }
    /// Runs an operator on the edit mesh as one undo step.
    pub fn edit_op<R>(&mut self, f: impl FnOnce(&mut EditMesh) -> R) -> Option<R> {
        let asset = self.edit.as_ref()?.asset;
        self.end_group();
        self.record(Slot::Mesh(asset));
        let r = f(&mut self.edit.as_mut()?.mesh);
        self.edit_commit(true);
        Some(r)
    }
    pub fn edit_mesh(&self) -> Option<&EditMesh> {
        self.edit.as_ref().map(|s| &s.mesh)
    }
    /// Selection changes are not undo steps of their own in most editors, but here they are cheap
    /// and make "oops, lost my selection" recoverable.
    fn edit_select(&mut self, f: impl FnOnce(&mut EditMesh)) {
        let Some(asset) = self.edit.as_ref().map(|s| s.asset) else { return };
        let before = self.edit.as_ref().map(|s| (s.mesh.sel_v.clone(), s.mesh.sel_e.clone(), s.mesh.sel_f.clone(), s.mesh.mode));
        let mut m = self.edit.as_ref().unwrap().mesh.clone();
        f(&mut m);
        if before != Some((m.sel_v.clone(), m.sel_e.clone(), m.sel_f.clone(), m.mode)) {
            self.end_group();
            self.record(Slot::Mesh(asset));
            if let Some(s) = &mut self.edit {
                s.mesh = m;
            }
            self.edit_commit(true);
        }
    }

    // -------------------------------------------------------------- geometry helpers

    fn edit_world(&self) -> Option<(Mat4, Mat4)> {
        let s = self.edit.as_ref()?;
        let m = self.object_transform(s.object)?.matrix();
        Some((m, m.inverse().unwrap_or(Mat4::IDENTITY)))
    }
    /// Screen position (frame pixels) and view depth of every vertex.
    pub fn edit_screen(&self, w: u32, h: u32) -> Vec<Option<(f32, f32, f32)>> {
        let (Some(s), Some((m, _))) = (self.edit.as_ref(), self.edit_world()) else { return vec![] };
        let cam = self.active_camera();
        s.mesh.verts.iter().map(|v| cam.project(m.transform_point(v.pos), w, h)).collect()
    }
    fn visible_at(&self, frame: &Frame, x: f32, y: f32, depth: f32) -> bool {
        if self.edit.as_ref().is_some_and(|s| s.xray) {
            return true;
        }
        let cam = self.active_camera();
        let (cx, cy) = (x.floor() as i32, y.floor() as i32);
        let mut far: Option<f32> = Some(0.0);
        for dy in -1..=1 {
            for dx in -1..=1 {
                match frame.view_depth(cx + dx, cy + dy, cam.near, cam.far) {
                    None => far = None,
                    Some(d) => {
                        if let Some(f) = far {
                            far = Some(f.max(d));
                        }
                    }
                }
            }
        }
        match far {
            None => true, // next to the background: on the silhouette
            Some(f) => depth <= f * 1.004 + 0.002,
        }
    }
    fn vis_list(&self, frame: &Frame, scr: &[Option<(f32, f32, f32)>]) -> Vec<bool> {
        scr.iter().map(|p| p.is_some_and(|(x, y, d)| self.visible_at(frame, x, y, d))).collect()
    }
    fn face_screen_center(&self, scr: &[Option<(f32, f32, f32)>], f: u32, w: u32, h: u32) -> Option<(f32, f32, f32)> {
        let s = self.edit.as_ref()?;
        let (m, _) = self.edit_world()?;
        let _ = scr;
        self.active_camera().project(m.transform_point(s.mesh.face_center(f)), w, h)
    }

    // -------------------------------------------------------------- picking and selection

    /// The element of the current select mode under (x, y), within `radius` pixels.
    pub fn edit_pick(&self, frame: &Frame, x: f32, y: f32, radius: f32) -> Option<MeshHit> {
        let s = self.edit.as_ref()?;
        let (w, h) = (frame.w, frame.h);
        let scr = self.edit_screen(w, h);
        let vis = self.vis_list(frame, &scr);
        let em = &s.mesh;
        match em.mode {
            SelectMode::Vertex => {
                let mut best: Option<(f32, u32)> = None;
                for (i, p) in scr.iter().enumerate() {
                    let Some((sx, sy, _)) = p else { continue };
                    if !vis[i] || em.vert_hidden(i as u32) {
                        continue;
                    }
                    let d = ((sx - x).powi(2) + (sy - y).powi(2)).sqrt();
                    if d <= radius && best.map_or(true, |b| d < b.0) {
                        best = Some((d, i as u32));
                    }
                }
                best.map(|b| MeshHit::Vert(b.1))
            }
            SelectMode::Edge => self.pick_edge(frame, &scr, &vis, x, y, radius).map(MeshHit::Edge),
            SelectMode::Face => {
                if !s.xray {
                    let hit = frame.hit_at(x as i32, y as i32);
                    if let Some(info) = self.pick_info(hit).filter(|i| i.object == s.object) {
                        let f = s.tri_face.get(info.mesh).and_then(|m| m.get(info.tri)).copied()?;
                        return (!em.face_hidden(f)).then_some(MeshHit::Face(f));
                    }
                    return None;
                }
                // X-ray: nearest face centre
                let mut best: Option<(f32, u32)> = None;
                for f in 0..em.faces.len() as u32 {
                    if em.face_hidden(f) {
                        continue;
                    }
                    let Some((sx, sy, _)) = self.face_screen_center(&scr, f, w, h) else { continue };
                    let d = ((sx - x).powi(2) + (sy - y).powi(2)).sqrt();
                    if d <= radius * 2.0 && best.map_or(true, |b| d < b.0) {
                        best = Some((d, f));
                    }
                }
                best.map(|b| MeshHit::Face(b.1))
            }
        }
    }
    fn pick_edge(&self, frame: &Frame, scr: &[Option<(f32, f32, f32)>], vis: &[bool], x: f32, y: f32, radius: f32) -> Option<Edge> {
        let em = &self.edit.as_ref()?.mesh;
        let mut best: Option<(f32, Edge)> = None;
        for e in em.all_edges() {
            let (Some(a), Some(b)) = (scr[e.0 as usize], scr[e.1 as usize]) else { continue };
            if em.vert_hidden(e.0) || em.vert_hidden(e.1) {
                continue;
            }
            let (d, t) = seg_dist((x, y), (a.0, a.1), (b.0, b.1));
            if d > radius {
                continue;
            }
            // visible if either end or the point under the cursor is visible
            let depth = a.2 + (b.2 - a.2) * t;
            let mid = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if !(vis[e.0 as usize] || vis[e.1 as usize] || self.visible_at(frame, mid.0, mid.1, depth)) {
                continue;
            }
            if best.map_or(true, |bb| d < bb.0) {
                best = Some((d, e));
            }
        }
        best.map(|b| b.1)
    }
    /// Remembers the element under the cursor (pre-selection highlight).
    pub fn edit_hover(&mut self, frame: Option<&Frame>, x: f32, y: f32, radius: f32) {
        let hit = frame.and_then(|f| self.edit_pick(f, x, y, radius));
        if let Some(s) = &mut self.edit {
            s.hover = hit;
        }
    }
    /// A click in the viewport. Returns false when nothing was under the cursor.
    pub fn edit_click(&mut self, frame: &Frame, x: f32, y: f32, radius: f32, how: ClickMod) -> bool {
        let Some(s) = self.edit.as_ref() else { return false };
        let mode = s.mesh.mode;
        match how {
            ClickMod::Loop | ClickMod::Ring => {
                let scr = self.edit_screen(frame.w, frame.h);
                let vis = self.vis_list(frame, &scr);
                let Some(e) = self.pick_edge(frame, &scr, &vis, x, y, radius * 1.5) else { return false };
                let ring = how == ClickMod::Ring;
                self.edit_select(|m| m.select_loop(e, false, ring));
                true
            }
            ClickMod::Path => {
                let hit = self.edit_pick(frame, x, y, radius);
                let v = match hit {
                    Some(MeshHit::Vert(v)) => v,
                    Some(MeshHit::Edge(e)) => e.1,
                    Some(MeshHit::Face(f)) => self.edit.as_ref().unwrap().mesh.faces[f as usize].c[0].v,
                    None => return false,
                };
                self.edit_select(|m| m.select_path_to(v));
                true
            }
            _ => {
                let extend = how == ClickMod::Extend;
                match self.edit_pick(frame, x, y, radius) {
                    Some(MeshHit::Vert(v)) => self.edit_select(|m| m.click_vert(v, extend)),
                    Some(MeshHit::Edge(e)) => self.edit_select(|m| m.click_edge(e, extend)),
                    Some(MeshHit::Face(f)) => self.edit_select(|m| m.click_face(f, extend)),
                    None => {
                        if !extend {
                            self.edit_select(|m| m.select_all(false));
                        }
                        return false;
                    }
                }
                let _ = mode;
                true
            }
        }
    }
    /// Box or lasso selection. `poly` with two points is a rectangle (corner to corner).
    /// `op`: 0 = replace, 1 = add, 2 = subtract.
    pub fn edit_select_region(&mut self, frame: &Frame, poly: &[(f32, f32)], op: u8) {
        let Some(s) = self.edit.as_ref() else { return };
        let (w, h) = (frame.w, frame.h);
        let scr = self.edit_screen(w, h);
        let vis = self.vis_list(frame, &scr);
        let rect = poly.len() == 2;
        let inside = |p: (f32, f32)| {
            if rect {
                let (a, b) = (poly[0], poly[1]);
                p.0 >= a.0.min(b.0) && p.0 <= a.0.max(b.0) && p.1 >= a.1.min(b.1) && p.1 <= a.1.max(b.1)
            } else {
                point_in_poly(p, poly)
            }
        };
        let em = &s.mesh;
        let verts: Vec<u32> = (0..em.verts.len() as u32).filter(|&v| vis[v as usize] && !em.vert_hidden(v) && scr[v as usize].is_some_and(|p| inside((p.0, p.1)))).collect();
        let vset: BTreeSet<u32> = verts.iter().copied().collect();
        let edges: Vec<Edge> = em.all_edges().into_iter().filter(|e| vset.contains(&e.0) && vset.contains(&e.1)).collect();
        let mut faces = vec![];
        if em.mode == SelectMode::Face {
            for f in 0..em.faces.len() as u32 {
                if em.face_hidden(f) {
                    continue;
                }
                if let Some((x, y, d)) = self.face_screen_center(&scr, f, w, h) {
                    if inside((x, y)) && self.visible_at(frame, x, y, d) {
                        faces.push(f);
                    }
                }
            }
        }
        self.edit_select(|m| {
            if op == 0 {
                m.select_all(false);
            }
            m.select_many(&verts, &edges, &faces, op == 2);
        });
    }
    /// L: selects everything connected to the element under the cursor.
    pub fn edit_select_linked_at(&mut self, frame: &Frame, x: f32, y: f32, radius: f32, deselect: bool) -> bool {
        let v = match self.edit_pick(frame, x, y, radius) {
            Some(MeshHit::Vert(v)) => v,
            Some(MeshHit::Edge(e)) => e.0,
            Some(MeshHit::Face(f)) => self.edit.as_ref().unwrap().mesh.faces[f as usize].c[0].v,
            None => return false,
        };
        self.edit_select(|m| m.select_linked_from(v, deselect));
        true
    }
    pub fn edit_set_select_mode(&mut self, mode: SelectMode) {
        self.edit_select(|m| m.set_mode(mode));
    }
    pub fn edit_select_all(&mut self, on: bool) {
        self.edit_select(|m| m.select_all(on));
    }
    pub fn edit_select_toggle(&mut self) {
        self.edit_select(|m| {
            let any = m.any_selected();
            m.select_all(!any)
        });
    }
    pub fn edit_select_invert(&mut self) {
        self.edit_select(|m| m.select_invert());
    }
    pub fn edit_select_linked(&mut self) {
        self.edit_select(|m| m.select_linked());
    }
    pub fn edit_grow(&mut self, grow: bool) {
        self.edit_select(|m| if grow { m.grow() } else { m.shrink() });
    }

    // -------------------------------------------------------------- overlay

    /// Everything the viewport draws on top of the mesh in Edit mode, in frame pixels.
    pub fn edit_overlay(&self, frame: &Frame) -> EditOverlay {
        let mut o = EditOverlay::default();
        let Some(s) = self.edit.as_ref() else { return o };
        let (w, h) = (frame.w, frame.h);
        let scr = self.edit_screen(w, h);
        let vis = self.vis_list(frame, &scr);
        let em = &s.mesh;
        let topo = em.topo();
        let p2 = |v: u32| scr[v as usize].map(|p| (p.0, p.1));
        for e in em.all_edges() {
            if em.vert_hidden(e.0) || em.vert_hidden(e.1) {
                continue;
            }
            let fs = topo.faces_of(e);
            if !fs.is_empty() && fs.iter().all(|&f| em.face_hidden(f)) {
                continue;
            }
            let (Some(a), Some(b)) = (p2(e.0), p2(e.1)) else { continue };
            let mut fl = 0;
            if em.sel_e.contains(&e) {
                fl |= EDGE_SEL;
            }
            if em.seams.contains(&e) {
                fl |= EDGE_SEAM;
            }
            if em.sharp.contains(&e) {
                fl |= EDGE_SHARP;
            }
            if vis[e.0 as usize] || vis[e.1 as usize] {
                fl |= EDGE_VISIBLE;
            }
            if fs.is_empty() {
                fl |= EDGE_LOOSE;
            }
            o.edges.push((a, b, fl));
        }
        if em.mode == SelectMode::Vertex {
            for v in 0..em.verts.len() as u32 {
                if em.vert_hidden(v) || topo.vert_nbrs[v as usize].is_empty() && !em.faces.iter().any(|f| f.has_vert(v)) {
                    continue;
                }
                if let Some(p) = p2(v) {
                    o.verts.push((p.0, p.1, em.sel_v.contains(&v), vis[v as usize]));
                }
            }
        }
        if em.mode == SelectMode::Face {
            for f in 0..em.faces.len() as u32 {
                if em.face_hidden(f) {
                    continue;
                }
                if let Some((x, y, d)) = self.face_screen_center(&scr, f, w, h) {
                    o.dots.push((x, y, em.sel_f.contains(&f), self.visible_at(frame, x, y, d)));
                }
            }
        }
        o.active = em.active.and_then(|v| p2(v)).filter(|_| em.mode == SelectMode::Vertex);
        match s.hover {
            Some(MeshHit::Vert(v)) => {
                o.hover_dot = p2(v);
            }
            Some(MeshHit::Edge(e)) => {
                if let (Some(a), Some(b)) = (p2(e.0), p2(e.1)) {
                    o.hover.push((a, b));
                }
            }
            Some(MeshHit::Face(f)) => {
                if let Some(face) = em.faces.get(f as usize) {
                    for (a, b) in face.edges() {
                        if let (Some(a), Some(b)) = (p2(a), p2(b)) {
                            o.hover.push((a, b));
                        }
                    }
                }
            }
            None => {}
        }
        if let (Some((_, _, segs)), Some((m, _))) = (&s.loop_preview, self.edit_world()) {
            let cam = self.active_camera();
            for (a, b) in segs {
                if let (Some(a), Some(b)) = (cam.project(m.transform_point(*a), w, h), cam.project(m.transform_point(*b), w, h)) {
                    o.loop_preview.push(((a.0, a.1), (b.0, b.1)));
                }
            }
        }
        o
    }
    /// Colour of a triangle of the edited object in the face-selection overlay.
    pub fn edit_face_state(&self, obj: u16, mesh: u16, tri: u32) -> u8 {
        let Some(s) = self.edit.as_ref() else { return 0 };
        if crate::compose::hit_object_id(s.object) != obj {
            return 0;
        }
        let Some(f) = s.tri_face.get(mesh as usize).and_then(|m| m.get(tri as usize)).copied() else { return 0 };
        let mut st = 0;
        if s.mesh.sel_f.contains(&f) {
            st |= 1;
        }
        if s.hover == Some(MeshHit::Face(f)) {
            st |= 2;
        }
        st
    }

    // -------------------------------------------------------------- loop cut and knife

    /// Ctrl+R hover: shows where a loop cut across the edge under the cursor would go.
    pub fn edit_loopcut_preview(&mut self, frame: &Frame, x: f32, y: f32, radius: f32, cuts: usize) {
        let scr = self.edit_screen(frame.w, frame.h);
        let vis = self.vis_list(frame, &scr);
        let e = self.pick_edge(frame, &scr, &vis, x, y, radius * 2.0);
        let Some(s) = &mut self.edit else { return };
        let Some(e) = e else {
            s.loop_preview = None;
            return;
        };
        if s.loop_preview.as_ref().is_some_and(|p| p.0 == e && p.1 == cuts) {
            return;
        }
        let mut m = s.mesh.clone();
        let n0 = m.verts.len() as u32;
        let mut segs = vec![];
        if m.loop_cut(e, cuts.max(1), 0.0) {
            for (a, b) in m.all_edges() {
                if a >= n0 && b >= n0 {
                    segs.push((m.pos(a), m.pos(b)));
                }
            }
        }
        s.loop_preview = Some((e, cuts.max(1), segs));
    }
    /// Click with the loop cut tool: cuts, then slides the new loop with the mouse.
    pub fn edit_loopcut_apply(&mut self, mouse: (f32, f32), w: u32, h: u32) -> bool {
        let Some((e, cuts, _)) = self.edit.as_ref().and_then(|s| s.loop_preview.clone()) else { return false };
        let Some(asset) = self.edit.as_ref().map(|s| s.asset) else { return false };
        self.end_group();
        self.begin_group();
        self.record(Slot::Mesh(asset));
        let base = self.edit.as_ref().unwrap().mesh.clone();
        let ok = self.edit.as_mut().unwrap().mesh.loop_cut(e, cuts, 0.0);
        if !ok {
            self.end_group();
            return false;
        }
        self.edit_commit(false);
        let s = self.edit.as_mut().unwrap();
        s.loop_preview = None;
        s.drag = Some(MeshDrag {
            kind: MeshDragKind::LoopSlide,
            axis: AxisLock::Free,
            numeric: String::new(),
            value: 0.0,
            segments: cuts,
            start: mouse,
            last: mouse,
            pivot: Vec3::ZERO,
            pivot_px: mouse,
            normal: Vec3::ZERO,
            base,
            weights: vec![],
            mirror: HashMap::new(),
            loop_edge: Some((e, cuts)),
            w,
            h,
            snap: DragSnap::default(),
        });
        true
    }
    /// Cuts along the knife line (frame pixels). Only what is visible is cut unless X-ray is on.
    pub fn edit_knife(&mut self, frame: &Frame, pts: &[(f32, f32)]) -> usize {
        if pts.len() < 2 || self.edit.is_none() {
            return 0;
        }
        let Some((m, inv)) = self.edit_world() else { return 0 };
        let cam = self.active_camera();
        let (w, h) = (frame.w, frame.h);
        let xray = self.edit.as_ref().unwrap().xray;
        let asset = self.edit.as_ref().unwrap().asset;
        self.end_group();
        self.record(Slot::Mesh(asset));
        let mut total = 0;
        for seg in pts.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            if ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() < 1.0 {
                continue;
            }
            let (o, da) = cam.ray(a.0, a.1, w, h);
            let (_, db) = cam.ray(b.0, b.1, w, h);
            let n_world = da.cross(db).normalized();
            let origin = inv.transform_point(o);
            let normal = inv.transform_dir(n_world).normalized();
            // the part of the plane between the two rays (and visible)
            let me = &*self;
            let keep = |p: Vec3| -> bool {
                let wp = m.transform_point(p);
                let Some((sx, sy, d)) = cam.project(wp, w, h) else { return false };
                let (dist, t) = seg_dist((sx, sy), a, b);
                let _ = dist;
                if !(0.0..=1.0).contains(&t) || t <= 0.0 && ((sx - a.0).powi(2) + (sy - a.1).powi(2)) > 4.0 || t >= 1.0 && ((sx - b.0).powi(2) + (sy - b.1).powi(2)) > 4.0 {
                    return false;
                }
                xray || me.visible_at(frame, sx, sy, d)
            };
            let em = &self.edit.as_ref().unwrap().mesh;
            let faces: BTreeSet<u32> = (0..em.faces.len() as u32).filter(|&f| !em.face_hidden(f)).collect();
            let mut mesh = em.clone();
            let n = mesh.cut_plane(&faces, origin, normal, &keep);
            if n > 0 {
                self.edit.as_mut().unwrap().mesh = mesh;
                total += n;
            }
        }
        self.edit_commit(true);
        if let Some(s) = &mut self.edit {
            s.knife.clear();
        }
        total
    }

    // -------------------------------------------------------------- modal transforms

    /// Starts a modal tool at mouse position `mouse` (frame pixels of a `w`×`h` viewport).
    pub fn edit_drag_begin(&mut self, kind: MeshDragKind, mouse: (f32, f32), w: u32, h: u32) -> bool {
        let Some(s) = self.edit.as_ref() else { return false };
        if s.drag.is_some() {
            return false;
        }
        if kind == MeshDragKind::LoopSlide {
            // sliding needs the edge ring of a fresh loop cut (see `edit_loopcut_apply`)
            self.status = "Edge slide: place a loop cut (Ctrl+R) and the new loop slides with the mouse".into();
            return false;
        }
        if !s.mesh.any_selected() {
            self.status = "Nothing is selected".into();
            return false;
        }
        let asset = s.asset;
        self.end_group();
        self.begin_group();
        self.record(Slot::Mesh(asset));
        self.start_drag(kind, mouse, w, h)
    }
    fn start_drag(&mut self, kind: MeshDragKind, mouse: (f32, f32), w: u32, h: u32) -> bool {
        let Some((m, _)) = self.edit_world() else { return false };
        let s = self.edit.as_ref().unwrap();
        let em = &s.mesh;
        let pivot = match (em.active, em.sel_v.len()) {
            (Some(a), 1) if em.sel_v.contains(&a) => em.pos(a),
            _ => em.selection_center().unwrap_or(Vec3::ZERO),
        };
        let normal = em.selection_normal();
        let cam = self.active_camera();
        let pp = cam.project(m.transform_point(pivot), w, h).map(|p| (p.0, p.1)).unwrap_or(mouse);
        // proportional weights and mirror partners
        let mut weights: Vec<(u32, f32)> = em.sel_v.iter().map(|&v| (v, 1.0)).collect();
        if s.proportional && matches!(kind, MeshDragKind::Move | MeshDragKind::Rotate | MeshDragKind::Scale) {
            let sel: Vec<Vec3> = em.sel_v.iter().map(|&v| m.transform_point(em.pos(v))).collect();
            let r = s.prop_radius.max(1e-4);
            for v in 0..em.verts.len() as u32 {
                if em.sel_v.contains(&v) || em.vert_hidden(v) {
                    continue;
                }
                let p = m.transform_point(em.pos(v));
                let d = sel.iter().fold(f32::MAX, |a, q| a.min((p - *q).length()));
                if d < r {
                    weights.push((v, smoothstep_falloff(d / r)));
                }
            }
        }
        let mut mirror = HashMap::new();
        if s.mirror_x {
            let key = |p: Vec3| ((p.x * 1e3).round() as i64, (p.y * 1e3).round() as i64, (p.z * 1e3).round() as i64);
            let mut by: HashMap<(i64, i64, i64), u32> = HashMap::new();
            for (i, v) in em.verts.iter().enumerate() {
                by.insert(key(v.pos), i as u32);
            }
            for (i, v) in em.verts.iter().enumerate() {
                if let Some(&j) = by.get(&key(Vec3::new(-v.pos.x, v.pos.y, v.pos.z))) {
                    mirror.insert(i as u32, j);
                }
            }
        }
        let segments = s.bevel_segments;
        let base = em.clone();
        let s = self.edit.as_mut().unwrap();
        s.drag = Some(MeshDrag { kind, axis: AxisLock::Free, numeric: String::new(), value: 0.0, segments, start: mouse, last: mouse, pivot, pivot_px: pp, normal, base, weights, mirror, loop_edge: None, w, h, snap: DragSnap::default() });
        true
    }
    /// E: extrudes the selection and moves it (along the normal for faces).
    pub fn edit_extrude_begin(&mut self, mouse: (f32, f32), w: u32, h: u32) -> bool {
        let Some(asset) = self.edit.as_ref().map(|s| s.asset) else { return false };
        if !self.edit.as_ref().unwrap().mesh.any_selected() {
            return false;
        }
        self.end_group();
        self.begin_group();
        self.record(Slot::Mesh(asset));
        let dir = self.edit.as_mut().unwrap().mesh.extrude();
        self.edit_commit(false);
        self.begin_drag_in_group(MeshDragKind::Move, mouse, w, h, if dir.length() > 1e-6 { Some(dir.normalized()) } else { None })
    }
    /// Shift+D: duplicates the selection and moves the copy.
    pub fn edit_duplicate_begin(&mut self, mouse: (f32, f32), w: u32, h: u32) -> bool {
        let Some(asset) = self.edit.as_ref().map(|s| s.asset) else { return false };
        self.end_group();
        self.begin_group();
        self.record(Slot::Mesh(asset));
        if !self.edit.as_mut().unwrap().mesh.duplicate() {
            self.end_group();
            return false;
        }
        self.edit_commit(false);
        self.begin_drag_in_group(MeshDragKind::Move, mouse, w, h, None)
    }
    fn begin_drag_in_group(&mut self, kind: MeshDragKind, mouse: (f32, f32), w: u32, h: u32, normal: Option<Vec3>) -> bool {
        if !self.start_drag(kind, mouse, w, h) {
            self.end_group();
            return false;
        }
        if let (Some(n), Some(d)) = (normal, self.edit.as_mut().and_then(|s| s.drag.as_mut())) {
            d.axis = AxisLock::Normal;
            d.normal = n;
        }
        true
    }
    pub fn edit_drag_active(&self) -> Option<&MeshDrag> {
        self.edit.as_ref().and_then(|s| s.drag.as_ref())
    }
    /// X / Y / Z during a drag; pressing the same axis again frees it. `plane` (Shift) locks
    /// the two other axes.
    pub fn edit_drag_axis(&mut self, axis: usize, plane: bool) {
        let Some(d) = self.edit.as_mut().and_then(|s| s.drag.as_mut()) else { return };
        let want = if plane { AxisLock::Plane(axis) } else { AxisLock::Axis(axis) };
        d.axis = if d.axis == want { AxisLock::Free } else { want };
        let m = d.last;
        self.edit_drag_update(m, d_snap(self));
    }
    /// Typed digits, '.', '-' and backspace ('\u{8}') during a drag.
    pub fn edit_drag_char(&mut self, c: char) {
        let Some(d) = self.edit.as_mut().and_then(|s| s.drag.as_mut()) else { return };
        match c {
            '\u{8}' => {
                d.numeric.pop();
            }
            '-' => {
                if d.numeric.starts_with('-') {
                    d.numeric.remove(0);
                } else {
                    d.numeric.insert(0, '-');
                }
            }
            '0'..='9' | '.' => d.numeric.push(c),
            _ => return,
        }
        let m = d.last;
        self.edit_drag_update(m, d_snap(self));
    }
    /// Mouse wheel during a drag: bevel segments, loop cut count or the proportional radius.
    pub fn edit_drag_wheel(&mut self, steps: i32) {
        let wm = self.edit_world().map(|x| x.0).unwrap_or(Mat4::IDENTITY);
        let Some(s) = self.edit.as_mut() else { return };
        let Some(d) = s.drag.as_mut() else { return };
        match d.kind {
            MeshDragKind::Bevel => {
                d.segments = (d.segments as i32 + steps).clamp(1, 32) as usize;
                s.bevel_segments = d.segments;
            }
            MeshDragKind::LoopSlide => {}
            _ if s.proportional => {
                s.prop_radius = (s.prop_radius * 1.1f32.powi(steps)).clamp(0.001, 1000.0);
                // recompute the weights
                let r = s.prop_radius;
                let em = &d.base;
                let m = wm;
                let sel: Vec<Vec3> = em.sel_v.iter().map(|&v| m.transform_point(em.pos(v))).collect();
                let mut weights: Vec<(u32, f32)> = em.sel_v.iter().map(|&v| (v, 1.0)).collect();
                for v in 0..em.verts.len() as u32 {
                    if em.sel_v.contains(&v) {
                        continue;
                    }
                    let p = m.transform_point(em.pos(v));
                    let dd = sel.iter().fold(f32::MAX, |a, q| a.min((p - *q).length()));
                    if dd < r {
                        weights.push((v, smoothstep_falloff(dd / r)));
                    }
                }
                d.weights = weights;
            }
            _ => return,
        }
        let m = self.edit.as_ref().unwrap().drag.as_ref().unwrap().last;
        self.edit_drag_update(m, d_snap(self));
    }
    /// Follows the mouse.
    pub fn edit_drag_update(&mut self, mouse: (f32, f32), snap: DragSnap) {
        let Some((m, inv)) = self.edit_world() else { return };
        let cam = self.active_camera();
        let Some(s) = self.edit.as_mut() else { return };
        let Some(d) = s.drag.as_mut() else { return };
        d.last = mouse;
        d.snap = snap;
        let (w, h) = (d.w, d.h);
        let num: Option<f32> = d.numeric.parse::<f32>().ok();
        let pw = m.transform_point(d.pivot);
        let pv = cam.project(pw, w, h).map_or(cam.target.length(), |p| p.2);
        let wpp = 2.0 * pv * (cam.fov_y * 0.5).tan() / h.max(1) as f32;
        let slow = if snap.precise { 0.1 } else { 1.0 };
        let mm = (d.start.0 + (mouse.0 - d.start.0) * slow, d.start.1 + (mouse.1 - d.start.1) * slow);
        let dist = |p: (f32, f32)| ((p.0 - d.pivot_px.0).powi(2) + (p.1 - d.pivot_px.1).powi(2)).sqrt();
        let fwd = (cam.target - cam.eye).normalized();
        let mut mesh = d.base.clone();
        match d.kind {
            MeshDragKind::Move => {
                let ray = |p: (f32, f32)| cam.ray(p.0, p.1, w, h);
                let plane_hit = |n: Vec3, p: (f32, f32)| -> Option<Vec3> {
                    let (o, dir) = ray(p);
                    let den = dir.dot(n);
                    if den.abs() < 1e-6 {
                        return None;
                    }
                    Some(o + dir * ((pw - o).dot(n) / den))
                };
                let line_t = |a: Vec3, p: (f32, f32)| -> f32 {
                    // closest point on the line pw + a t to the ray
                    let (o, dir) = ray(p);
                    let w0 = pw - o;
                    let (b, dd, e) = (a.dot(dir), a.dot(w0), dir.dot(w0));
                    let den = 1.0 - b * b;
                    if den.abs() < 1e-6 {
                        return 0.0;
                    }
                    (b * e - dd) / den
                };
                let mut delta = match d.axis {
                    AxisLock::Free => match (plane_hit(fwd, d.start), plane_hit(fwd, mm)) {
                        (Some(a), Some(b)) => b - a,
                        _ => Vec3::ZERO,
                    },
                    AxisLock::Plane(i) => {
                        let n = axis_vec(i);
                        match (plane_hit(n, d.start), plane_hit(n, mm)) {
                            (Some(a), Some(b)) if n.dot(fwd).abs() > 0.05 => b - a,
                            _ => Vec3::ZERO,
                        }
                    }
                    AxisLock::Axis(i) => {
                        let a = axis_vec(i);
                        a * (line_t(a, mm) - line_t(a, d.start))
                    }
                    AxisLock::Normal => {
                        let a = m.transform_dir(d.normal).normalized();
                        a * (line_t(a, mm) - line_t(a, d.start))
                    }
                };
                if let Some(v) = num {
                    delta = match d.axis {
                        AxisLock::Axis(i) => axis_vec(i) * v,
                        AxisLock::Normal => m.transform_dir(d.normal).normalized() * v,
                        _ => Vec3::new(v, 0.0, 0.0),
                    };
                } else if snap.vertex {
                    // onto the nearest other vertex on screen
                    let moving: BTreeSet<u32> = d.weights.iter().map(|x| x.0).collect();
                    let mut best: Option<(f32, Vec3)> = None;
                    for (i, v) in d.base.verts.iter().enumerate() {
                        if moving.contains(&(i as u32)) {
                            continue;
                        }
                        let wp = m.transform_point(v.pos);
                        if let Some((sx, sy, _)) = cam.project(wp, w, h) {
                            let dd = ((sx - mouse.0).powi(2) + (sy - mouse.1).powi(2)).sqrt();
                            if dd < 24.0 && best.map_or(true, |b| dd < b.0) {
                                best = Some((dd, wp));
                            }
                        }
                    }
                    if let Some((_, t)) = best {
                        delta = t - pw;
                        if let AxisLock::Axis(i) = d.axis {
                            delta = axis_vec(i) * delta.dot(axis_vec(i));
                        }
                    }
                } else if snap.increment {
                    let st = snap.step.max(1e-4);
                    let r = |x: f32| (x / st).round() * st;
                    delta = Vec3::new(r(delta.x), r(delta.y), r(delta.z));
                }
                d.value = delta.length();
                let dm = inv.transform_dir(delta);
                for &(v, wgt) in &d.weights {
                    mesh.verts[v as usize].pos = d.base.verts[v as usize].pos + dm * wgt;
                }
            }
            MeshDragKind::Rotate => {
                let ang = |p: (f32, f32)| (p.1 - d.pivot_px.1).atan2(p.0 - d.pivot_px.0);
                let mut a = ang(mm) - ang(d.start);
                let mut axis = fwd;
                match d.axis {
                    AxisLock::Axis(i) | AxisLock::Plane(i) => {
                        axis = axis_vec(i);
                        if axis.dot(fwd) < 0.0 {
                            a = -a;
                        }
                    }
                    AxisLock::Normal => axis = m.transform_dir(d.normal).normalized(),
                    AxisLock::Free => {}
                }
                if let Some(v) = num {
                    a = -v.to_radians();
                    if axis.dot(fwd) < 0.0 {
                        a = -a;
                    }
                } else if snap.increment {
                    let st = 5f32.to_radians();
                    a = (a / st).round() * st;
                }
                d.value = -a.to_degrees();
                for &(v, wgt) in &d.weights {
                    let q = Quat::from_axis_angle(axis, a * wgt);
                    let p = m.transform_point(d.base.verts[v as usize].pos);
                    mesh.verts[v as usize].pos = inv.transform_point(pw + q.rotate(p - pw));
                }
            }
            MeshDragKind::Scale => {
                let d0 = dist(d.start).max(1.0);
                let mut f = dist(mm) / d0;
                if let Some(v) = num {
                    f = v;
                } else if snap.increment {
                    f = (f * 10.0).round() / 10.0;
                }
                d.value = f;
                for &(v, wgt) in &d.weights {
                    let p = m.transform_point(d.base.verts[v as usize].pos);
                    let off = p - pw;
                    let k = 1.0 + (f - 1.0) * wgt;
                    let new = match d.axis {
                        AxisLock::Free => off * k,
                        AxisLock::Axis(i) => {
                            let a = axis_vec(i);
                            off + a * (off.dot(a) * (k - 1.0))
                        }
                        AxisLock::Plane(i) => {
                            let a = axis_vec(i);
                            let along = a * off.dot(a);
                            along + (off - along) * k
                        }
                        AxisLock::Normal => {
                            let a = m.transform_dir(d.normal).normalized();
                            off + a * (off.dot(a) * (k - 1.0))
                        }
                    };
                    mesh.verts[v as usize].pos = inv.transform_point(pw + new);
                }
            }
            MeshDragKind::ShrinkFatten => {
                let mut amount = (dist(mm) - dist(d.start)) * wpp;
                if let Some(v) = num {
                    amount = v;
                }
                d.value = amount;
                let topo = d.base.topo();
                let scale = inv.transform_dir(Vec3::new(1.0, 0.0, 0.0)).length();
                for &(v, _) in &d.weights {
                    let n = d.base.vert_normal(&topo, v);
                    mesh.verts[v as usize].pos = d.base.verts[v as usize].pos + n * (amount * scale);
                }
            }
            MeshDragKind::Inset => {
                let mut t = ((dist(d.start) - dist(mm)) * wpp).max(0.0);
                if let Some(v) = num {
                    t = v.max(0.0);
                }
                d.value = t;
                let scale = inv.transform_dir(Vec3::new(1.0, 0.0, 0.0)).length();
                if t > 0.0 {
                    mesh.inset(t * scale, 0.0, s.inset_individual);
                }
            }
            MeshDragKind::Bevel | MeshDragKind::BevelVerts => {
                let mut t = ((dist(mm) - dist(d.start)) * wpp).max(0.0);
                if let Some(v) = num {
                    t = v.max(0.0);
                }
                d.value = t;
                let scale = inv.transform_dir(Vec3::new(1.0, 0.0, 0.0)).length();
                if t > 0.0 {
                    if d.kind == MeshDragKind::Bevel {
                        mesh.bevel(BevelOptions { width: t * scale, segments: d.segments, clamp: true });
                    } else {
                        mesh.bevel_verts(t * scale);
                    }
                }
            }
            MeshDragKind::LoopSlide => {
                let Some((e, cuts)) = d.loop_edge else { return };
                let pa = cam.project(m.transform_point(d.base.pos(e.0)), w, h);
                let pb = cam.project(m.transform_point(d.base.pos(e.1)), w, h);
                let mut slide = 0.0;
                if let (Some(a), Some(b)) = (pa, pb) {
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let l2 = (dx * dx + dy * dy).max(1.0);
                    slide = (((mm.0 - d.start.0) * dx + (mm.1 - d.start.1) * dy) / l2 * 2.0).clamp(-0.95, 0.95);
                }
                if let Some(v) = num {
                    slide = v.clamp(-0.95, 0.95);
                }
                d.value = slide;
                mesh.loop_cut(e, cuts, if cuts == 1 { slide } else { 0.0 });
            }
        }
        // X mirror: partners follow
        if !d.mirror.is_empty() && matches!(d.kind, MeshDragKind::Move | MeshDragKind::Rotate | MeshDragKind::Scale | MeshDragKind::ShrinkFatten) {
            let moved: Vec<u32> = d.weights.iter().map(|x| x.0).collect();
            let moved_set: BTreeSet<u32> = moved.iter().copied().collect();
            for v in moved {
                if let Some(&p) = d.mirror.get(&v) {
                    let q = mesh.verts[v as usize].pos;
                    if p == v {
                        mesh.verts[v as usize].pos.x = 0.0;
                    } else if !moved_set.contains(&p) {
                        mesh.verts[p as usize].pos = Vec3::new(-q.x, q.y, q.z);
                    }
                }
            }
        }
        s.mesh = mesh;
        self.edit_commit(false);
    }
    /// Confirms (Enter, click) or cancels (Esc, right click) the running modal tool.
    pub fn edit_drag_end(&mut self, confirm: bool) {
        let Some(s) = self.edit.as_mut() else { return };
        let Some(d) = s.drag.take() else { return };
        if !confirm {
            s.mesh = d.base;
            if let (MeshDragKind::LoopSlide, Some((e, cuts))) = (d.kind, d.loop_edge) {
                // cancelling the slide keeps the cut in the middle
                s.mesh.loop_cut(e, cuts, 0.0);
            }
        }
        self.edit_commit(true);
        self.end_group();
    }

    // -------------------------------------------------------------- operators

    pub fn edit_delete(&mut self, kind: DeleteKind) -> bool {
        self.edit_op(|m| m.delete(kind)).unwrap_or(false)
    }
    /// X / Delete: removes what the select mode selects.
    pub fn edit_delete_selected(&mut self) -> bool {
        let kind = match self.edit.as_ref().map(|s| s.mesh.mode) {
            Some(SelectMode::Vertex) => DeleteKind::Verts,
            Some(SelectMode::Edge) => DeleteKind::Edges,
            Some(SelectMode::Face) => DeleteKind::Faces,
            None => return false,
        };
        self.edit_delete(kind)
    }
    pub fn edit_dissolve(&mut self) -> bool {
        let mode = self.edit.as_ref().map(|s| s.mesh.mode);
        self.edit_op(|m| match mode {
            Some(SelectMode::Vertex) => m.dissolve_verts() > 0,
            Some(SelectMode::Edge) => m.dissolve_edges() > 0,
            _ => m.dissolve_faces() > 0,
        })
        .unwrap_or(false)
    }
    pub fn edit_merge(&mut self, kind: MergeKind) -> usize {
        self.edit_op(|m| m.merge(kind)).unwrap_or(0)
    }
    pub fn edit_merge_by_distance(&mut self, dist: f32) -> usize {
        self.edit_op(|m| m.merge_by_distance(dist)).unwrap_or(0)
    }
    pub fn edit_bevel_apply(&mut self, o: BevelOptions) -> bool {
        self.edit_op(|m| m.bevel(o)).unwrap_or(false)
    }
    /// Adds a primitive into the edited mesh at the centre of the view.
    pub fn edit_add_primitive(&mut self, kind: Primitive) -> bool {
        let Some((_, inv)) = self.edit_world() else { return false };
        let mesh = match kind {
            Primitive::Cube | Primitive::Butterfly => modeling::cube(Vec3::new(1.0, 1.0, 1.0)),
            Primitive::Plane => modeling::plane(2.0, 2.0, 1, 1),
            Primitive::Cylinder => modeling::cylinder(0.5, 1.0, 8),
            Primitive::Cone => modeling::cone(0.5, 1.0, 8),
            Primitive::Sphere => modeling::sphere(0.5, 8, 6),
            Primitive::Torus => modeling::torus(0.5, 0.2, 10, 6),
        };
        let mut add = EditMesh::from_meshes(&[mesh]);
        let at = inv.transform_point(self.view.target);
        let sc = inv.transform_dir(Vec3::new(1.0, 0.0, 0.0)).length();
        for v in &mut add.verts {
            v.pos = v.pos * sc;
        }
        let part = self.edit.as_ref().and_then(|s| s.mesh.faces.first().map(|f| f.part)).unwrap_or(0);
        self.edit_op(|m| {
            m.set_mode(m.mode);
            m.append_into_part(&add, at, part)
        })
        .is_some()
    }
    /// P: moves the selected polygons into a new object (with a copy of the materials).
    pub fn edit_separate(&mut self) -> Option<u32> {
        let s = self.edit.as_ref()?;
        if s.mesh.sel_f.is_empty() || s.mesh.sel_f.len() == s.mesh.faces.len() {
            self.status = "Select some (not all) faces to separate".into();
            return None;
        }
        let (asset, object) = (s.asset, s.object);
        self.end_group();
        self.begin_group();
        self.record(Slot::Whole);
        let part = self.edit.as_mut()?.mesh.separate()?;
        self.edit_commit(true);
        let mut model = self.project.assets[asset].model.clone();
        model.meshes = part.to_meshes().0;
        model.name = format!("{} part", model.name);
        let new_asset = self.project.add_asset(model);
        let name = self.project.assets[new_asset].model.name.clone();
        let id = self.project.scene.add_object(&name, new_asset);
        let t = self.project.scene.object(object).map(|o| o.transform.clone()).unwrap_or_default();
        if let Some(o) = self.project.scene.object_mut(id) {
            o.transform = t.clone();
        }
        let bones = self.project.bones_of(new_asset);
        for a in &mut self.project.animations {
            for f in &mut a.frames {
                let tr = f.states.iter().find(|s| s.object == object).map(|s| s.transform.clone()).unwrap_or(t.clone());
                f.states.push(ObjectState::base(id, tr, bones));
            }
        }
        self.end_group();
        Some(id)
    }
    /// Boolean between the selected polygons and the rest of the mesh.
    pub fn edit_boolean_selected(&mut self, op: BoolOp) -> bool {
        self.edit_op(|m| m.boolean_selected(op)).unwrap_or(false)
    }
    /// UV unwrap of the selected polygons (all when nothing is selected).
    pub fn edit_unwrap(&mut self, how: u8, w: u32, h: u32) -> usize {
        let Some((m, _)) = self.edit_world() else { return 0 };
        let cam = self.active_camera();
        self.edit_op(|em| {
            let faces: Vec<u32> = if em.sel_f.is_empty() { (0..em.faces.len() as u32).collect() } else { em.sel_f.iter().copied().collect() };
            match how {
                1 => em.uv_cube_project(&faces, 0.02),
                2 => {
                    em.uv_project(&faces, &|p| {
                        let q = cam.project(m.transform_point(p), w, h).unwrap_or((0.0, 0.0, 0.0));
                        Vec2::new(q.0, q.1)
                    });
                    1
                }
                _ => em.uv_unwrap(&faces, 0.02),
            }
        })
        .unwrap_or(0)
    }
    /// Tris, quads and n-gons of the edited mesh, and its vertex and edge counts.
    pub fn edit_stats(&self) -> Option<(usize, usize, usize, usize, usize)> {
        let m = &self.edit.as_ref()?.mesh;
        let (t, q, n) = m.face_stats();
        Some((m.verts.len(), m.all_edges().len(), t, q, n))
    }
}

fn d_snap(ed: &Editor) -> DragSnap {
    ed.edit.as_ref().and_then(|s| s.drag.as_ref()).map(|d| d.snap).unwrap_or_default()
}

// -------------------------------------------------------------- object-mode booleans

impl Editor {
    /// Boolean of the selected object with `cutter`: the selected object's mesh is replaced by the
    /// result. The cutter is hidden afterwards (or deleted with `delete_cutter`).
    pub fn boolean_with(&mut self, cutter: u32, op: BoolOp, delete_cutter: bool) -> bool {
        let Some(target) = self.selected_object else { return false };
        if target == cutter {
            return false;
        }
        let (Some(ta), Some(ca)) = (self.project.scene.object(target).map(|o| o.asset), self.project.scene.object(cutter).map(|o| o.asset)) else { return false };
        let (Some(tt), Some(ct)) = (self.object_transform(target), self.object_transform(cutter)) else { return false };
        let to_target = tt.matrix().inverse().unwrap_or(Mat4::IDENTITY) * ct.matrix();
        let mut cm = EditMesh::from_meshes(&self.project.assets[ca].model.meshes);
        for v in &mut cm.verts {
            v.pos = to_target.transform_point(v.pos);
        }
        // a mirrored cutter turns inside out: flip it back
        let (x, y, z) = (to_target.transform_dir(Vec3::new(1.0, 0.0, 0.0)), to_target.transform_dir(Vec3::new(0.0, 1.0, 0.0)), to_target.transform_dir(Vec3::new(0.0, 0.0, 1.0)));
        if x.cross(y).dot(z) < 0.0 {
            for f in 0..cm.faces.len() as u32 {
                cm.flip_face(f);
            }
        }
        let was_edit = self.edit.is_some();
        if was_edit {
            self.edit_exit();
        }
        let a = &self.project.assets[ta];
        let fp = EditMesh::fingerprint(&a.model.meshes);
        let mut em = match &a.topology {
            Some(t) if t.0 == fp => t.1.clone(),
            _ => EditMesh::from_meshes(&a.model.meshes),
        };
        if !em.boolean(&cm, op, 0) {
            self.status = "The boolean result is empty".into();
            return false;
        }
        self.end_group();
        self.begin_group();
        self.record(Slot::Mesh(ta));
        let (meshes, _) = em.to_meshes();
        let asset = &mut self.project.assets[ta];
        asset.model.meshes = meshes;
        let fp = EditMesh::fingerprint(&asset.model.meshes);
        asset.topology = Some(Box::new((fp, em)));
        if delete_cutter {
            self.end_group();
            self.delete_object(cutter);
        } else {
            self.record(Slot::Scene);
            if let Some(o) = self.project.scene.object_mut(cutter) {
                o.visible = false;
            }
        }
        self.end_group();
        self.select_object(Some(target));
        if was_edit {
            self.edit_enter();
        }
        true
    }
}
