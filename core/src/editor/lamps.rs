//! Lamps in the editor: adding them from presets, selecting and picking them in the viewport, moving them with the gizmo and
//! hanging them on objects. Every change is one undo step like any other scene edit.
use super::*;
use crate::lights::{Light, LightPreset};

/// A lamp as it appears in the viewport: its screen position and, for spotlights, where the beam points.
#[derive(Clone, Debug)]
pub struct ScreenLight {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    /// The end of a short line along the beam of a spotlight.
    pub beam: Option<(f32, f32)>,
    pub color: [f32; 3],
    pub enabled: bool,
    pub selected: bool,
}

impl Editor {
    /// World matrix of an object as it stands in the current view (the frame pose in Pose and Animate mode).
    fn object_world(&self, id: u32) -> Option<Mat4> {
        self.object_transform(id).map(|t| t.matrix())
    }
    /// Where a lamp is in the world right now (attached lamps follow their object).
    pub fn light_world_pos(&self, id: u32) -> Option<Vec3> {
        let l = self.project.scene.light(id)?;
        match l.attach.and_then(|o| self.object_world(o)) {
            Some(m) => Some(m.transform_point(l.pos)),
            None => Some(l.pos),
        }
    }
    fn light_world_dir(&self, id: u32) -> Option<Vec3> {
        let l = self.project.scene.light(id)?;
        match l.attach.and_then(|o| self.object_world(o)) {
            Some(m) => Some(m.transform_dir(l.direction()).normalized()),
            None => Some(l.direction()),
        }
    }
    /// Adds a lamp from a preset in front of the camera (above the selected object when there is one) and selects it.
    pub fn add_light(&mut self, preset: LightPreset) -> u32 {
        self.end_group();
        self.record(Slot::Scene);
        let base = match self.selected_object.and_then(|o| self.object_transform(o)) {
            Some(t) => t.pos,
            None => self.view.target,
        };
        let id = self.project.scene.add_light(preset);
        if let Some(l) = self.project.scene.light_mut(id) {
            l.pos = base + l.pos;
        }
        self.select_light(Some(id));
        self.dirty = true;
        id
    }
    /// Selects a lamp (and nothing else) or clears the lamp selection.
    pub fn select_light(&mut self, id: Option<u32>) {
        if id.is_some() {
            self.select_object(None);
        }
        self.selected_light = id.filter(|&i| self.project.scene.light(i).is_some());
    }
    /// The selected lamp, if it still exists.
    pub fn selected_light(&self) -> Option<&Light> {
        self.project.scene.light(self.selected_light?)
    }
    /// Changes a lamp (one undo step; inside a drag group only the first change is stored). Values are kept in range.
    pub fn edit_light(&mut self, id: u32, f: impl FnOnce(&mut Light)) -> bool {
        if self.project.scene.light(id).is_none() {
            return false;
        }
        self.record(Slot::Scene);
        if let Some(l) = self.project.scene.light_mut(id) {
            f(l);
            l.clamp();
        }
        self.dirty = true;
        true
    }
    pub fn delete_light(&mut self, id: u32) -> bool {
        if self.project.scene.light(id).is_none() {
            return false;
        }
        self.end_group();
        self.record(Slot::Scene);
        self.project.scene.remove_light(id);
        if self.selected_light == Some(id) {
            self.selected_light = None;
        }
        self.dirty = true;
        true
    }
    /// A copy of a lamp, a little to the side, selected.
    pub fn duplicate_light(&mut self, id: u32) -> Option<u32> {
        let src = self.project.scene.light(id)?.clone();
        self.end_group();
        self.record(Slot::Scene);
        let new = self.project.scene.add_light(LightPreset::Bulb);
        if let Some(l) = self.project.scene.light_mut(new) {
            let name = format!("{} copy", src.name);
            *l = Light { id: new, name, pos: src.pos + Vec3::new(0.5, 0.0, 0.0), ..src };
        }
        self.select_light(Some(new));
        self.dirty = true;
        Some(new)
    }
    /// Hangs a lamp on an object (it then follows the object in every frame) or frees it. The lamp stays where it is.
    pub fn attach_light(&mut self, id: u32, object: Option<u32>) -> bool {
        let Some(world) = self.light_world_pos(id) else { return false };
        let local = match object {
            Some(o) => {
                let Some(inv) = self.object_world(o).and_then(|m| m.inverse()) else { return false };
                inv.transform_point(world)
            }
            None => world,
        };
        self.edit_light(id, |l| {
            l.attach = object;
            l.pos = local;
        })
    }
    /// The lamp whose viewport marker is under a screen position (within `radius` pixels).
    pub fn light_pick(&self, sx: f32, sy: f32, w: u32, h: u32, radius: f32) -> Option<u32> {
        let mut best: Option<(f32, u32)> = None;
        for s in self.screen_lights(w, h) {
            let d = ((s.x - sx).powi(2) + (s.y - sy).powi(2)).sqrt();
            if d <= radius && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, s.id));
            }
        }
        best.map(|(_, i)| i)
    }
    /// The lamps seen by the viewport camera, for drawing their markers. None in Render mode.
    pub fn screen_lights(&self, w: u32, h: u32) -> Vec<ScreenLight> {
        if self.mode == Mode::Render || (self.animating() && self.through_camera && self.playing) {
            return Vec::new();
        }
        let cam = self.active_camera();
        let mut out = Vec::new();
        for l in &self.project.scene.lights {
            let Some(p) = self.light_world_pos(l.id) else { continue };
            let Some((x, y, _)) = cam.project(p, w, h) else { continue };
            let beam = if l.kind == crate::lights::LightKind::Spot {
                let d = self.light_world_dir(l.id).unwrap_or(Vec3::new(0.0, -1.0, 0.0));
                let len = (p - cam.eye).length() * 0.12;
                cam.project(p + d * len, w, h).map(|(bx, by, _)| (bx, by))
            } else {
                None
            };
            out.push(ScreenLight { id: l.id, x, y, beam, color: l.color, enabled: l.enabled, selected: self.selected_light == Some(l.id) });
        }
        out
    }
    /// True when the move/rotate handles have something to work on: the selected object or lamp.
    pub fn has_gizmo_target(&self) -> bool {
        self.selected_light().is_some() || self.selected_object.is_some()
    }
    /// Position and rotation the handles show: the selected lamp's place in the world, otherwise the selected object.
    pub fn gizmo_transform(&self) -> Option<Transform> {
        if let Some(l) = self.selected_light() {
            return Some(Transform { pos: self.light_world_pos(l.id)?, rot_deg: l.rot_deg, scale: Vec3::ONE });
        }
        self.object_transform(self.selected_object?)
    }
    /// Applies a handle drag to the selected lamp (position, and rotation for the beam) or object.
    pub fn set_gizmo_transform(&mut self, t: Transform) {
        if let Some(id) = self.selected_light().map(|l| l.id) {
            let attach = self.project.scene.light(id).and_then(|l| l.attach);
            let local = match attach.and_then(|o| self.object_world(o)).and_then(|m| m.inverse()) {
                Some(inv) => inv.transform_point(t.pos),
                None => t.pos,
            };
            self.edit_light(id, |l| {
                l.pos = local;
                l.rot_deg = t.rot_deg;
            });
            return;
        }
        if let Some(id) = self.selected_object {
            self.set_object_transform(id, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamps_are_added_selected_edited_and_undone() {
        let mut ed = Editor::new();
        let obj = ed.add_primitive(Primitive::Cube);
        assert_eq!(ed.selected_object, Some(obj));
        let l = ed.add_light(LightPreset::Lantern);
        assert_eq!((ed.selected_light, ed.selected_object), (Some(l), None), "a new lamp is selected on its own");
        assert!(ed.has_gizmo_target());
        ed.edit_light(l, |x| x.intensity = 99.0);
        assert_eq!(ed.project.scene.light(l).unwrap().intensity, 8.0, "values are kept in range");
        ed.undo();
        assert_eq!(ed.project.scene.light(l).unwrap().intensity, Light::new(0, LightPreset::Lantern).intensity);
        ed.undo();
        assert!(ed.project.scene.lights.is_empty(), "adding is one undo step");
        ed.redo();
        ed.select_object(Some(obj));
        assert_eq!(ed.selected_light, None, "selecting an object clears the lamp selection");
    }
    #[test]
    fn attaching_keeps_the_lamp_in_place_and_the_gizmo_moves_it_in_world_space() {
        let mut ed = Editor::new();
        let obj = ed.add_primitive(Primitive::Cube);
        let mut t = ed.object_transform(obj).unwrap();
        t.pos = Vec3::new(2.0, 0.0, 0.0);
        t.rot_deg = Vec3::new(0.0, 90.0, 0.0);
        t.scale = Vec3::new(2.0, 2.0, 2.0);
        ed.set_object_transform(obj, t);
        let l = ed.add_light(LightPreset::Bulb);
        let before = ed.light_world_pos(l).unwrap();
        assert!(ed.attach_light(l, Some(obj)));
        let after = ed.light_world_pos(l).unwrap();
        assert!((before - after).length() < 1e-4, "{before:?} vs {after:?}");
        let mut g = ed.gizmo_transform().unwrap();
        g.pos = g.pos + Vec3::new(0.0, 1.0, 0.0);
        ed.set_gizmo_transform(g);
        assert!((ed.light_world_pos(l).unwrap() - (after + Vec3::new(0.0, 1.0, 0.0))).length() < 1e-4);
        // the lamp follows its object
        let mut t2 = ed.object_transform(obj).unwrap();
        t2.pos.z += 3.0;
        ed.set_object_transform(obj, t2);
        assert!((ed.light_world_pos(l).unwrap().z - (after.z + 3.0)).abs() < 1e-4);
        assert!(ed.attach_light(l, None));
        assert!(ed.project.scene.light(l).unwrap().attach.is_none());
        let id = ed.duplicate_light(l).unwrap();
        assert_ne!(id, l);
        assert_eq!(ed.selected_light, Some(id));
        assert!(ed.delete_light(id));
        assert_eq!(ed.selected_light, None);
    }
}
