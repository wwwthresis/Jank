//! Lamps in the Scene panel: the list, the "Add lamp" presets, the properties of the selected lamp, and their markers in the
//! viewport.
use crate::icons::Icon;
use crate::panels::rgb_edit;
use crate::theme::*;
use crate::widgets::{dim, drag3_tip, section, slider_f_tip, toggle_tip, Btn};
use crate::{compat, App};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Painter, Rect, Ui};
use psxcore::lights::{LightKind, LightPreset};
use psxcore::math::Vec3;

impl App {
    /// "Add lamp" button with a menu of presets.
    fn add_lamp_menu(&mut self, ui: &mut Ui) {
        let mut pick = None;
        ui.menu_button("Add lamp…", |ui| {
            ui.set_min_width(220.0);
            for p in LightPreset::ALL {
                let icon = if psxcore::lights::Light::new(0, p).kind == LightKind::Spot { Icon::Spot } else { Icon::Lamp };
                let r = ui.horizontal(|ui| {
                    crate::widgets::icon_inline(ui, icon, accent());
                    ui.button(p.label())
                });
                if crate::widgets::with_tip(r.inner, p.description()).clicked() {
                    pick = Some(p);
                    ui.close_menu();
                }
            }
        });
        if let Some(p) = pick {
            let id = self.ed.add_light(p);
            let name = self.ed.project.scene.light(id).map(|l| l.name.clone()).unwrap_or_default();
            self.info(format!("Added {name}. Drag the handles to place it"));
        }
    }

    /// The lamps of the scene and the selected lamp's settings.
    pub fn lights_section(&mut self, ui: &mut Ui) {
        let lamps: Vec<(u32, String, bool, LightKind)> = self.ed.project.scene.lights.iter().map(|l| (l.id, l.name.clone(), l.enabled, l.kind)).collect();
        section(ui, &format!("Lamps ({})", lamps.len()), accent(), |ui| {
            if lamps.is_empty() {
                dim(ui, "No lamps yet. Lamps light the scene around them: candles, lanterns, neon, spotlights.");
            }
            for (id, name, on, kind) in &lamps {
                let icon = if *kind == LightKind::Spot { Icon::Spot } else { Icon::Lamp };
                let r = crate::widgets::tree_row(ui, icon, name, self.ed.selected_light == Some(*id), Some(*on), "Select. The eye switches the lamp on and off. Right-click for more");
                if r.eye {
                    let v = !*on;
                    self.ed.edit_light(*id, |l| l.enabled = v);
                } else if r.row.clicked() {
                    self.ed.select_light(Some(*id));
                }
                r.row.context_menu(|ui| {
                    if ui.button("Duplicate").clicked() {
                        self.ed.duplicate_light(*id);
                        ui.close_menu();
                    }
                    if ui.button("Delete").clicked() {
                        self.ed.delete_light(*id);
                        ui.close_menu();
                    }
                });
            }
            ui.horizontal(|ui| {
                self.add_lamp_menu(ui);
                if let Some(id) = self.ed.selected_light {
                    if Btn::new("").icon(Icon::Copy).tip("Duplicate the lamp").show(ui).clicked() {
                        self.ed.duplicate_light(id);
                    }
                    if Btn::new("").icon(Icon::Trash).accent(DANGER()).tip("Delete the lamp").show(ui).clicked() {
                        self.ed.delete_light(id);
                    }
                }
            });
        });
        self.lamp_properties(ui);
    }

    fn lamp_properties(&mut self, ui: &mut Ui) {
        let Some(l) = self.ed.selected_light().cloned() else { return };
        let id = l.id;
        section(ui, "Lamp", accent(), |ui| {
            // name: edited in a buffer and stored when the field loses focus (one undo step)
            let key = egui::Id::new(("lamp_name", id));
            let mut buf = ui.data_mut(|d| d.get_temp::<String>(key)).unwrap_or_else(|| l.name.clone());
            let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(f32::INFINITY).hint_text("Name"));
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(key, buf.clone()));
            }
            if r.lost_focus() {
                let name = buf.trim().to_string();
                if !name.is_empty() && name != l.name {
                    self.ed.edit_light(id, |x| x.name = name);
                }
                ui.data_mut(|d| d.remove::<String>(key));
            }
            let mut m = l.clone();
            ui.horizontal(|ui| {
                for k in LightKind::ALL {
                    let (label, icon) = if k == LightKind::Spot { ("Spot", Icon::Spot) } else { ("Point", Icon::Lamp) };
                    if Btn::new(label).icon(icon).on(m.kind == k).accent(accent()).tip(if k == LightKind::Spot { "A cone of light, like a torch or a stage spotlight" } else { "Light in every direction, like a bulb or a candle" }).show(ui).clicked() {
                        m.kind = k;
                    }
                }
            });
            rgb_edit(ui, "colour", &mut m.color);
            slider_f_tip(ui, "brightness", &mut m.intensity, 0.0..=8.0, "How strong the lamp is. Above 1 it can burn surfaces to white (a tone curve in Look keeps that soft)");
            slider_f_tip(ui, "reach", &mut m.range, 0.1..=40.0, "Distance in metres where the light has faded out completely");
            slider_f_tip(ui, "falloff", &mut m.falloff, 0.0..=1.0, "0: even light up to the edge. 1: bright near the lamp, fading quickly");
            if m.kind == LightKind::Spot {
                slider_f_tip(ui, "cone °", &mut m.cone_deg, 1.0..=170.0, "Width of the beam");
                slider_f_tip(ui, "soft edge", &mut m.blend, 0.0..=1.0, "0: hard edge of the beam. 1: very soft");
            }
            slider_f_tip(ui, "flicker", &mut m.flicker, 0.0..=1.0, "Candle- or fire-like wavering during playback (the same every time it plays)");
            if m.flicker > 0.0 {
                slider_f_tip(ui, "flicker speed", &mut m.flicker_speed, 0.05..=8.0, "How fast the light wavers");
            }
            slider_f_tip(ui, "halo", &mut m.halo, 0.0..=2.0, "Glow around the lamp itself in the picture (needs Halos in Look → Modern effects)");
            let mut on = m.enabled;
            if toggle_tip(ui, &mut on, "On", accent(), "Switch the lamp on or off").changed() {
                m.enabled = on;
            }
            if m != l {
                self.cont(|ed| {
                    ed.edit_light(id, |x| *x = m);
                });
            }
        });
        section(ui, "Lamp placement", accent(), |ui| {
            let world = self.ed.light_world_pos(id).unwrap_or(l.pos);
            let mut pos = [world.x, world.y, world.z];
            if drag3_tip(ui, "position", &mut pos, 0.02, "Where the lamp is in the world (also: drag the handles in the viewport)") {
                let mut t = psxcore::scene::Transform { pos: Vec3::new(pos[0], pos[1], pos[2]), rot_deg: l.rot_deg, scale: Vec3::ONE };
                if self.ed.selected_light != Some(id) {
                    return;
                }
                t.rot_deg = l.rot_deg;
                self.cont(|ed| ed.set_gizmo_transform(t));
            }
            if l.kind == LightKind::Spot {
                let mut rot = [l.rot_deg.x, l.rot_deg.y, l.rot_deg.z];
                if drag3_tip(ui, "aim °", &mut rot, 0.5, "Rotation of the beam; 0 0 0 points straight down") {
                    let r = Vec3::new(rot[0], rot[1], rot[2]);
                    self.cont(|ed| {
                        ed.edit_light(id, |x| x.rot_deg = r);
                    });
                }
            }
            // hang the lamp on an object: it then follows the object in every frame
            let objects: Vec<(u32, String)> = self.ed.project.scene.objects.iter().map(|o| (o.id, o.name.clone())).collect();
            let current = l.attach.and_then(|a| objects.iter().find(|o| o.0 == a)).map_or("nothing (stays in place)".to_string(), |o| o.1.clone());
            let mut pick: Option<Option<u32>> = None;
            ui.horizontal(|ui| {
                dim(ui, "attached to");
                egui::ComboBox::from_id_salt(("lamp_attach", id)).selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
                    if ui.selectable_label(l.attach.is_none(), "nothing (stays in place)").clicked() {
                        pick = Some(None);
                    }
                    for (oid, name) in &objects {
                        if ui.selectable_label(l.attach == Some(*oid), name).clicked() {
                            pick = Some(Some(*oid));
                        }
                    }
                });
            });
            if let Some(a) = pick {
                self.ed.end_group();
                self.ed.attach_light(id, a);
            }
            if l.attach.is_some() {
                dim(ui, "The lamp moves with its object, also in animation frames.");
            }
        });
        if !self.ed.project.scene.style.pixel_lights {
            dim(ui, "Tip: Look → Modern effects → Pixel lamps draws smooth pools of light instead of light per vertex.");
        }
    }

    /// Lamp markers over the viewport: a small sun in the lamp's colour, a line along the beam of a spotlight.
    pub fn lamp_markers(&self, p: &Painter, img: Rect, sc: f32, w: u32, h: u32) {
        let show = matches!(self.ed.mode, psxcore::editor::Mode::Model | psxcore::editor::Mode::Animate | psxcore::editor::Mode::Pose);
        if !show {
            return;
        }
        let at = |x: f32, y: f32| pos2(img.min.x + x * sc, img.min.y + y * sc);
        for s in self.ed.screen_lights(w, h) {
            let c = at(s.x, s.y);
            if !img.expand(20.0).contains(c) {
                continue;
            }
            let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
            let col = Color32::from_rgb(f(s.color[0]), f(s.color[1]), f(s.color[2]));
            let col = if s.enabled { col } else { Color32::from_gray(110) };
            if let Some((bx, by)) = s.beam {
                p.line_segment([c, at(bx, by)], compat::stroke(1.5, alpha(col, 200)));
            }
            let r = if s.selected { 7.0 } else { 5.5 };
            if s.selected {
                p.circle_stroke(c, r + 4.0, compat::stroke(2.0, accent()));
            }
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * (r + 1.5), c + d * (r + 4.0)], compat::stroke(1.5, alpha(col, 220)));
            }
            p.circle_filled(c, r, col);
            p.circle_stroke(c, r, compat::stroke(1.0, Color32::from_black_alpha(160)));
            if s.selected {
                if let Some(l) = self.ed.project.scene.light(s.id) {
                    compat::text(p, c + vec2(0.0, -r - 8.0), Align2::CENTER_BOTTOM, &l.name, 12.0, alpha(Color32::WHITE, 220));
                }
            }
        }
    }
}
