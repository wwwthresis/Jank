//! Bottom panel: playback, frame operations and the frame strip.
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{dim, toggle, Btn};
use crate::{compat, App};
use eframe::egui::{self, pos2, vec2, Align2, Context, Rect, ScrollArea, Sense, TopBottomPanel};

impl App {
    pub fn timeline_panel(&mut self, ctx: &Context) {
        let mut panel = TopBottomPanel::bottom("timeline").resizable(true).default_height(self.settings.layout.timeline_height).height_range(150.0..=420.0).frame(compat::frame(panel_fill(TIMELINE()), 8.0).stroke(compat::stroke(1.0, SHADE())));
        if self.layout_reset {
            panel = panel.exact_height(self.settings.layout.timeline_height);
        }
        let shown = panel.show(ctx, |ui| {
            self.art.paint_panel(ui, crate::themeart::Slot::Timeline, vec2(8.0, 8.0));
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
                if Btn::new("").icon(if playing { Icon::Pause } else { Icon::Play }).accent(accent()).on(playing).size(vec2(40.0, 34.0)).tip(&self.tip_for(psxcore::commands::CommandId::TogglePlay, "Play or pause")).show(ui).clicked() {
                    self.ed.toggle_play();
                }
                if Btn::new("").icon(Icon::StepFwd).tip(&self.tip_for(psxcore::commands::CommandId::NextFrame, "Next frame")).show(ui).clicked() {
                    self.ed.step_frame(1);
                }
                if Btn::new("").icon(Icon::Loop).on(self.ed.looping).accent(accent()).tip("Loop playback").show(ui).clicked() {
                    self.ed.looping = !self.ed.looping;
                }
                if Btn::new("").icon(Icon::Onion).on(self.ed.onion).accent(accent()).tip("Onion skin: show the previous and next frame as ghosts (strength in Settings)").show(ui).clicked() {
                    self.ed.onion = !self.ed.onion;
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
                    if crate::widgets::tab(ui, name, Some(Icon::Film), self.ed.current_anim == i, "Switch to this animation", 0.0).clicked() {
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
                if Btn::new("Frame").icon(Icon::Plus).accent(accent()).tip("Adds a frame after the current one (pose and placement are kept)").show(ui).clicked() {
                    self.ed.add_frame();
                }
                if Btn::new("Copy").icon(Icon::Copy).tip("Duplicates the frame including its drawing").show(ui).clicked() {
                    self.ed.duplicate_frame();
                }
                if Btn::new("").icon(Icon::Trash).accent(DANGER()).enabled(n > 1).tip("Delete frame").show(ui).clicked() {
                    self.ed.delete_frame();
                }
                if Btn::new("←").enabled(cur > 0).size(vec2(34.0, 34.0)).tip("Move frame left").show(ui).clicked() {
                    self.ed.move_frame(cur, cur - 1);
                }
                if Btn::new("→").enabled(cur + 1 < n).size(vec2(34.0, 34.0)).tip("Move frame right").show(ui).clicked() {
                    self.ed.move_frame(cur, cur + 1);
                }
                ui.add_space(8.0);
                let mut h = hold as f32;
                dim(ui, "hold");
                if ui.add(egui::DragValue::new(&mut h).speed(0.1).range(1.0..=200.0).suffix(" t")).changed() {
                    self.cont(|ed| ed.set_hold(cur, h as u32));
                }
                let mut tw = tween;
                if toggle(ui, &mut tw, "tween to next", accent()).clicked() {
                    self.ed.set_tween(cur, tw);
                }
                let mut nt = note;
                if ui.add(egui::TextEdit::singleline(&mut nt).hint_text("frame note").desired_width(180.0)).changed() {
                    self.cont(|ed| ed.set_frame_note(&nt));
                }
                ui.add_space(8.0);
                if crate::widgets::tab(ui, "Curves", Some(Icon::Graph), self.graph.open, "Graph editor: animation curves of the bones, drag keys to change them", 0.0).clicked() {
                    self.graph.open = !self.graph.open;
                }
            });
            if self.graph.open {
                self.graph_editor(ui);
            } else {
                self.frame_strip(ui);
            }
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
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, (hold, tween, hand, hand_only, emo)) in cells.iter().enumerate() {
                    // Aseprite-style column: a frame-number header over a cel; held frames are wider
                    let w = (44.0 + (*hold as f32 - 1.0) * 8.0).min(200.0);
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, 64.0), Sense::click());
                    let hov = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.08);
                    let sel = i == cur;
                    let p = ui.painter();
                    let head = Rect::from_min_size(rect.min, vec2(w, 18.0));
                    let cel = Rect::from_min_max(pos2(rect.min.x, head.max.y + 1.0), rect.max);
                    p.rect_filled(head, compat::cr(0.0), if sel { PLAYHEAD() } else { mix(BG0(), TIMELINE_FRAME(), hov) });
                    compat::text(p, head.center(), Align2::CENTER_CENTER, &(i + 1).to_string(), 11.5, if sel { BRIGHT() } else { DIM() });
                    let col_fill = if sel { mix(TIMELINE_FRAME(), PLAYHEAD(), 0.22) } else { mix(TIMELINE_FRAME(), BG3(), hov) };
                    crate::widgets::bevel(p, cel.shrink2(vec2(1.0, 0.0)), accent(), hov, false, false);
                    p.rect_filled(cel.shrink2(vec2(2.0, 2.0)), compat::cr(1.0), col_fill);
                    if sel {
                        compat::stroke_rect(p, cel.shrink2(vec2(1.0, 0.0)), 1.0, crate::compat::stroke(1.0, PLAYHEAD()));
                    }
                    // the cel dot: filled when the frame has its own drawing, ring otherwise
                    let dot = pos2(cel.min.x + 12.0, cel.min.y + 13.0);
                    if *hand {
                        p.circle_filled(dot, 4.5, if sel { BRIGHT() } else { TEXT() });
                    } else {
                        p.circle_stroke(dot, 4.0, crate::compat::stroke(1.5, if sel { BRIGHT() } else { DIM() }));
                    }
                    if *hold > 1 {
                        compat::text(p, pos2(cel.max.x - 6.0, cel.min.y + 6.0), Align2::RIGHT_TOP, &format!("×{hold}"), 11.0, DIM());
                    }
                    if *tween {
                        // a tween is an arrow that reaches into the next cel
                        let y = cel.center().y + 6.0;
                        p.line_segment([pos2(cel.min.x + 8.0, y), pos2(cel.max.x + 2.0, y)], crate::compat::stroke(1.5, accent()));
                        p.add(egui::Shape::convex_polygon(vec![pos2(cel.max.x + 2.0, y - 3.5), pos2(cel.max.x + 6.0, y), pos2(cel.max.x + 2.0, y + 3.5)], accent(), egui::Stroke::NONE));
                    }
                    if *hand_only {
                        crate::icons::draw(p, Icon::EyeOff, pos2(cel.max.x - 12.0, cel.max.y - 10.0), 16.0, DIM());
                    }
                    if !emo.is_empty() {
                        compat::text(p, pos2(cel.min.x + 6.0, cel.max.y - 5.0), Align2::LEFT_BOTTOM, &emo.chars().take(((w - 10.0) / 7.0) as usize).collect::<String>(), 11.0, mix(accent(), BRIGHT(), 0.4));
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
    }
}
