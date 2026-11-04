//! Bottom panel: playback, frame operations and the frame strip.
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{dim, toggle, Btn};
use crate::{compat, App};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Context, Rect, ScrollArea, Sense, TopBottomPanel};

impl App {
    pub fn timeline_panel(&mut self, ctx: &Context) {
        let mut panel = TopBottomPanel::bottom("timeline").resizable(true).default_height(self.settings.layout.timeline_height).height_range(150.0..=420.0).frame(compat::frame(BG0, 8.0));
        if self.layout_reset {
            panel = panel.exact_height(self.settings.layout.timeline_height);
        }
        let shown = panel.show(ctx, |ui| {
            self.hover_timeline = ui.rect_contains_pointer(ui.max_rect());
            let Some(anim) = self.ed.animation() else { return };
            let (n, total, fps, dur) = (anim.frames.len(), anim.total_ticks(), anim.fps(), anim.duration_secs());
            let names: Vec<String> = self.ed.project.animations.iter().map(|a| a.name.clone()).collect();
            let cur = self.ed.current_frame;
            let (hold, tween, note) = self.ed.frame().map(|f| (f.hold, f.tween, f.note.clone())).unwrap_or((1, false, String::new()));

            ui.horizontal(|ui| {
                if Btn::new("").icon(Icon::StepBack).tip(&self.tip_for(psxcore::commands::CommandId::PrevFrame, "Previous frame")).show(ui).clicked() {
                    self.ed.step_frame(-1);
                }
                let playing = self.ed.playing;
                if Btn::new(if playing { "Pause" } else { "Play" }).icon(if playing { Icon::Pause } else { Icon::Play }).accent(ACCENT).on(playing).size(vec2(110.0, 34.0)).tip(&self.tip_for(psxcore::commands::CommandId::TogglePlay, "Play or pause")).show(ui).clicked() {
                    self.ed.toggle_play();
                }
                if Btn::new("").icon(Icon::StepFwd).tip(&self.tip_for(psxcore::commands::CommandId::NextFrame, "Next frame")).show(ui).clicked() {
                    self.ed.step_frame(1);
                }
                if Btn::new("").icon(Icon::Loop).on(self.ed.looping).accent(ACCENT).tip("Loop playback").show(ui).clicked() {
                    self.ed.looping = !self.ed.looping;
                }
                ui.add_space(8.0);
                dim(ui, &format!("frame {}/{}   tick {}/{}   {:.1} s", cur + 1, n, self.ed.tick + 1, total, dur));
                ui.add_space(8.0);
                let mut f = fps.round().max(1.0);
                dim(ui, "fps");
                if ui.add(egui::DragValue::new(&mut f).speed(0.2).range(1.0..=60.0)).changed() {
                    self.cont(|ed| ed.set_fps(f as u32, 1));
                }
                ui.add_space(8.0);
                for (i, name) in names.iter().enumerate() {
                    if Btn::new(name).accent(ACCENT).on(self.ed.current_anim == i).size(vec2(24.0 + name.chars().count().min(12) as f32 * 7.5, 30.0)).show(ui).clicked() {
                        self.ed.current_anim = i;
                        self.ed.goto_frame(0);
                    }
                }
                if Btn::new("").icon(Icon::Plus).size(vec2(30.0, 30.0)).tip("New animation").show(ui).clicked() {
                    let name = format!("Animation {}", names.len() + 1);
                    self.ed.new_animation(&name);
                }
            });
            ui.horizontal(|ui| {
                if Btn::new("Frame").icon(Icon::Plus).accent(ACCENT).tip("Adds a frame after the current one (pose and placement are kept)").show(ui).clicked() {
                    self.ed.add_frame();
                }
                if Btn::new("Copy").icon(Icon::Copy).tip("Duplicates the frame including its drawing").show(ui).clicked() {
                    self.ed.duplicate_frame();
                }
                if Btn::new("").icon(Icon::Trash).accent(DANGER).enabled(n > 1).tip("Delete frame").show(ui).clicked() {
                    self.ed.delete_frame();
                }
                if Btn::new("◀").enabled(cur > 0).size(vec2(34.0, 34.0)).tip("Move frame left").show(ui).clicked() {
                    self.ed.move_frame(cur, cur - 1);
                }
                if Btn::new("▶").enabled(cur + 1 < n).size(vec2(34.0, 34.0)).tip("Move frame right").show(ui).clicked() {
                    self.ed.move_frame(cur, cur + 1);
                }
                ui.add_space(8.0);
                let mut h = hold as f32;
                dim(ui, "hold");
                if ui.add(egui::DragValue::new(&mut h).speed(0.1).range(1.0..=200.0).suffix(" t")).changed() {
                    self.cont(|ed| ed.set_hold(cur, h as u32));
                }
                let mut tw = tween;
                if toggle(ui, &mut tw, "tween to next", ACCENT).clicked() {
                    self.ed.set_tween(cur, tw);
                }
                let mut nt = note;
                if ui.add(egui::TextEdit::singleline(&mut nt).hint_text("frame note").desired_width(180.0)).changed() {
                    self.cont(|ed| ed.set_frame_note(&nt));
                }
            });
            self.frame_strip(ui);
        });
        self.track_width(crate::Panel::Timeline, shown.response.rect.height());
    }

    fn frame_strip(&mut self, ui: &mut egui::Ui) {
        let Some(anim) = self.ed.animation() else { return };
        let cells: Vec<(u32, bool, bool, bool, String)> = anim
            .frames
            .iter()
            .map(|f| {
                let emo = f.states.iter().find(|s| Some(s.object) == self.ed.selected_object).and_then(|s| s.emotion);
                let emo_name = emo.and_then(|e| self.ed.selected_asset().and_then(|a| self.ed.project.assets.get(a)).and_then(|a| a.emotions.get(e)).map(|e| e.name.clone())).unwrap_or_default();
                (f.hold, f.tween, f.hand_layer.is_some(), f.hand_only, emo_name)
            })
            .collect();
        let cur = self.ed.current_frame;
        let mut goto = None;
        ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, (hold, tween, hand, hand_only, emo)) in cells.iter().enumerate() {
                    let w = (74.0 + (*hold as f32 - 1.0) * 9.0).min(220.0);
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, 62.0), Sense::click());
                    let hov = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
                    let sel = i == cur;
                    let p = ui.painter();
                    let r: Rect = rect.translate(vec2(0.0, -2.0 * hov));
                    p.rect_filled(r, compat::cr(3.0), mix(if sel { mix(BG2, ACCENT, 0.28) } else { BG2 }, BG3, hov));
                    compat::stroke_rect(p, r, 3.0, crate::compat::stroke(if sel { 2.0 } else { 1.0 }, if sel { ACCENT } else { mix(LINE, ACCENT, hov) }));
                    compat::text(p, r.min + vec2(9.0, 8.0), Align2::LEFT_TOP, &(i + 1).to_string(), 16.0, if sel { ACCENT } else { TEXT });
                    if *hold > 1 {
                        compat::text(p, pos2(r.max.x - 7.0, r.min.y + 9.0), Align2::RIGHT_TOP, &format!("×{hold}"), 12.0, DIM);
                    }
                    let mut x = r.min.x + 9.0;
                    for (on, ch, col) in [(*tween, "~", ACCENT), (*hand, "D", ACCENT), (*hand_only, "O", ACCENT)] {
                        if on {
                            compat::text(p, pos2(x, r.max.y - 8.0), Align2::LEFT_BOTTOM, ch, 15.0, col);
                            x += 16.0;
                        }
                    }
                    if !emo.is_empty() {
                        compat::text(p, pos2(r.max.x - 7.0, r.max.y - 8.0), Align2::RIGHT_BOTTOM, &emo.chars().take(8).collect::<String>(), 11.5, ACCENT);
                    }
                    if resp.clicked() {
                        goto = Some(i);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Go to this frame").clicked() {
                            goto = Some(i);
                            ui.close_menu();
                        }
                        if ui.button("Duplicate frame").clicked() {
                            self.ed.goto_frame(i);
                            self.ed.duplicate_frame();
                            ui.close_menu();
                        }
                        if ui.button("Add frame after").clicked() {
                            self.ed.goto_frame(i);
                            self.ed.add_frame();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Delete frame").clicked() {
                            self.ed.goto_frame(i);
                            self.ed.delete_frame();
                            ui.close_menu();
                        }
                    });
                }
            });
        });
        if let Some(i) = goto {
            self.ed.goto_frame(i);
        }
        let _ = Color32::WHITE;
    }
}
