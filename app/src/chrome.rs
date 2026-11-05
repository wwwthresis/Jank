//! The window chrome around the viewport, laid out like Obsidian with Aseprite's tool column:
//! * a slim menu bar with the wordmark and the document title,
//! * the **ribbon**: a narrow, full-height icon column on the far left with the five modes and the
//!   file / history actions (command palette and settings sit at its bottom),
//! * the **tool strip**: an Aseprite-style column of pixel-icon tool cells right next to the
//!   viewport, showing the tools of the current mode and the current colour,
//! * the status bar.
use crate::icons::Icon;
use crate::paintui::SelTool;
use crate::theme::*;
use crate::widgets::{self, ribbon_button, tool_cell};
use crate::{compat, dialogs, mode_command, mode_icon, mode_label, AnimTool, App, ModelTool, MODES};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, Context, Layout, Sense, SidePanel, TopBottomPanel};
use psxcore::commands::CommandId;
use psxcore::editor::Mode;
use psxcore::paint::Tool;
use psxcore::settings::{Choice, Guides};

/// Width of the ribbon and of the tool strip, in points.
const RIBBON_W: f32 = 40.0;
const STRIP_W: f32 = 36.0;

pub fn hairline(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().line_segment([pos2(r.min.x + 7.0, r.center().y), pos2(r.max.x - 7.0, r.center().y)], compat::stroke(1.0, LINE()));
}

impl App {
    /// Main menu: every command of the catalogue, grouped the way artists look for them.
    pub fn menu_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("menu").exact_height(26.0).frame(compat::frame(BG0(), 0.0).inner_margin(egui::Margin::symmetric(8, 2)).stroke(compat::stroke(1.0, LINE_SOFT()))).show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                widgets::logo(ui, 18.0);
                ui.add_space(6.0);
                use CommandId::*;
                let groups: [(&str, &[CommandId]); 8] = [
                    ("File", &[NewProject, OpenProject, SaveProject, SaveProjectAs, ImportModel, ExportOutput, Quit]),
                    ("Edit", &[Undo, Redo, DeleteSelected, CommandPalette, RadialMenu, OpenSettings]),
                    ("Model", &[EditModeToggle, AddCube, AddButterfly, BooleanUnion, BooleanDifference, BooleanIntersect, Subdivide, MirrorX, WeldVertices, FlatShade, AutoUv, AddReference]),
                    (
                        "Mesh",
                        &[
                            MeshExtrude, MeshInset, MeshBevel, MeshBevelVertices, MeshLoopCut, MeshKnife, MeshSubdivide, MeshFill, MeshBridge, MeshConnect, MeshMerge, MeshDissolve, MeshDelete, MeshDuplicate, MeshSplit, MeshSeparate, MeshTriangulate, MeshTrisToQuads, MeshFlipNormals,
                            MeshRecalcNormals, MeshSmooth, MeshSymmetrize, MeshShadeSmooth, MeshShadeFlat, MeshMarkSeam, MeshClearSeam, MeshUnwrap, MeshSelectAll, MeshSelectInvert, MeshSelectLinked, MeshHide, MeshReveal, MeshXray, MeshProportional,
                        ],
                    ),
                    ("Paint", &[ToolBrush, ToolEraser, ToolFill, ToolGradient, ToolSmudge, ToolBlur, ToolPicker, SwapColors, BrushSmaller, BrushBigger, ToggleSymmetry, ToggleProjectionPaint]),
                    ("Animation", &[TogglePlay, PrevFrame, NextFrame, AddFrame, DuplicateFrame, ToggleIk]),
                    ("View", &[ModeModel, ModeTexture, ModePose, ModeAnimate, ModeRender, FrameAll, TogglePreview, ToggleStats, GizmoSelect, GizmoMove, GizmoRotate, GizmoScale, GizmoToggleSpace, GizmoToggleSnap]),
                    ("Help", &[WhatsNew, OpenAbout, OpenLicenses]),
                ];
                for (name, items) in groups {
                    ui.menu_button(name, |ui| {
                        ui.set_min_width(230.0);
                        if name == "Mesh" && !self.ed.edit_active() {
                            ui.label(egui::RichText::new("Works in Edit mode (Tab)").size(11.5).color(DIM()));
                        }
                        for &id in items {
                            let label = self.settings.keymap.label(id);
                            let mut b = egui::Button::new(id.title());
                            if !label.is_empty() {
                                b = b.shortcut_text(label);
                            }
                            if ui.add(b).on_hover_text(id.info().description).clicked() {
                                self.run_command(ctx, id);
                                ui.close_menu();
                            }
                        }
                    });
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Obsidian-like "quick switcher" pill that opens the command palette
                    let keys = self.settings.keymap.label(CommandPalette);
                    let (r, resp) = ui.allocate_exact_size(vec2(210.0_f32.min(ui.available_width() * 0.35), 20.0), Sense::click());
                    let p = ui.painter();
                    p.rect_filled(r, compat::cr(RADIUS + 1.0), if resp.hovered() { BG2() } else { BG1() });
                    compat::stroke_rect(p, r, RADIUS + 1.0, compat::stroke(1.0, if resp.hovered() { LINE() } else { LINE_SOFT() }));
                    crate::icons::draw(p, Icon::Search, pos2(r.min.x + 12.0, r.center().y), 16.0, DIM());
                    compat::text(p, pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, "Search commands", 12.0, DIM());
                    if !keys.is_empty() {
                        compat::text(p, pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, &keys, 11.0, alpha(DIM(), 160));
                    }
                    if widgets::with_tip(resp, "Command palette: run any command by name").clicked() {
                        self.run_command(ctx, CommandPalette);
                    }
                    ui.add_space(8.0);
                    if ui.available_width() > 180.0 {
                        let dirty = if self.ed.dirty { "  \u{2022}" } else { "" };
                        ui.label(egui::RichText::new(format!("{}{dirty}", self.ed.project.name)).size(12.0).color(DIM()));
                    }
                });
            });
        });
    }

    /// The Obsidian-style ribbon: modes on top, file and history actions below, palette and settings at the bottom.
    pub fn ribbon(&mut self, ctx: &Context) {
        SidePanel::left("ribbon").exact_width(RIBBON_W).resizable(false).frame(compat::frame(BG0(), 0.0).inner_margin(egui::Margin::symmetric(4, 6)).stroke(compat::stroke(1.0, LINE_SOFT()))).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
            for m in MODES {
                let tip = self.tip_for(mode_command(m), mode_label(m));
                if ribbon_button(ui, mode_icon(m), self.ed.mode == m, &tip).clicked() {
                    self.ed.set_mode(m);
                }
            }
            hairline(ui);
            if ribbon_button(ui, Icon::File, false, &self.tip_for(CommandId::NewProject, "New project")).clicked() {
                self.new_project();
            }
            if ribbon_button(ui, Icon::Folder, false, &self.tip_for(CommandId::OpenProject, "Open project")).clicked() {
                self.dialog = Some(dialogs::browser(dialogs::Purpose::OpenProject));
            }
            if ribbon_button(ui, Icon::Save, false, &self.tip_for(CommandId::SaveProject, "Save project")).clicked() {
                self.save();
            }
            if ribbon_button(ui, Icon::Open, false, &self.tip_for(CommandId::ImportModel, "Import a model: OBJ, STL, PLY, glTF/GLB")).clicked() {
                self.dialog = Some(dialogs::browser(dialogs::Purpose::ImportModel));
            }
            if ribbon_button(ui, Icon::Export, false, &self.tip_for(CommandId::ExportOutput, "Export pictures and animations")).clicked() {
                self.right_tab = crate::panels::RightTab::Output;
            }
            hairline(ui);
            ui.add_enabled_ui(self.ed.can_undo(), |ui| {
                if ribbon_button(ui, Icon::Undo, false, &self.tip_for(CommandId::Undo, "Undo")).clicked() {
                    self.ed.undo();
                }
            });
            ui.add_enabled_ui(self.ed.can_redo(), |ui| {
                if ribbon_button(ui, Icon::Redo, false, &self.tip_for(CommandId::Redo, "Redo")).clicked() {
                    self.ed.redo();
                }
            });
            ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                if ribbon_button(ui, Icon::Gear, self.dialog.as_ref().is_some_and(|d| matches!(d, dialogs::Dialog::Settings(_))), &self.tip_for(CommandId::OpenSettings, "Settings")).clicked() {
                    self.run_command(ctx, CommandId::OpenSettings);
                }
                if ribbon_button(ui, Icon::Command, false, &self.tip_for(CommandId::CommandPalette, "Command palette")).clicked() {
                    self.run_command(ctx, CommandId::CommandPalette);
                }
                if ribbon_button(ui, Icon::Info, false, "About Jank!").clicked() {
                    self.run_command(ctx, CommandId::OpenAbout);
                }
            });
        });
    }

    /// True when the current mode draws with the brush tools (Texture, or Animate with Draw / Paint).
    pub fn painting(&self) -> bool {
        self.ed.mode == Mode::Texture || (self.ed.mode == Mode::Animate && matches!(self.anim_tool, AnimTool::Hand | AnimTool::PaintModel))
    }

    /// The Aseprite-style tool column next to the viewport.
    pub fn tool_strip(&mut self, ctx: &Context) {
        if !self.settings.show_toolbar {
            return;
        }
        SidePanel::left("tools").exact_width(STRIP_W).resizable(false).frame(compat::frame(BG1(), 0.0).inner_margin(egui::Margin::symmetric(4, 6)).stroke(compat::stroke(1.0, SHADE()))).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
            match self.ed.mode {
                Mode::Model if self.ed.edit_active() => self.edit_cells(ui, ctx),
                Mode::Model => {
                    for (t, icon, tip) in [(ModelTool::Select, Icon::Select, "Select: click a face, Shift adds"), (ModelTool::Sculpt, Icon::Sculpt, "Sculpt: drag to push and pull the surface")] {
                        if tool_cell(ui, icon, self.model_tool == t, tip).clicked() {
                            self.model_tool = t;
                        }
                    }
                    let tip = self.tip_for(CommandId::EditModeToggle, "Edit mode: vertices, edges and polygons");
                    if tool_cell(ui, Icon::Vertex, false, &tip).clicked() {
                        self.run_command(ctx, CommandId::EditModeToggle);
                    }
                    hairline(ui);
                    for (id, icon, tip) in [(CommandId::AddCube, Icon::Cube, "Add a cube"), (CommandId::MirrorX, Icon::Mirror, "Mirror the model along X"), (CommandId::BooleanDifference, Icon::Boolean, "Boolean: cut the selected object with the cutter"), (CommandId::AddReference, Icon::Reference, "Add a reference picture"), (CommandId::FrameAll, Icon::Search, "Frame everything in the view")] {
                        let tip = self.tip_for(id, tip);
                        if tool_cell(ui, icon, false, &tip).clicked() {
                            self.run_command(ctx, id);
                        }
                    }
                }
                Mode::Pose => self.pose_cells(ui),
                Mode::Animate => {
                    for (t, icon, tip) in [(AnimTool::Pose, Icon::Bone, "Pose: drag joints"), (AnimTool::Hand, Icon::Pencil, "Draw over the frame by hand"), (AnimTool::PaintModel, Icon::Brush, "Paint on the model: emotions and per-frame textures"), (AnimTool::MoveObject, Icon::Move, "Move the object across the screen plane")] {
                        if tool_cell(ui, icon, self.anim_tool == t, tip).clicked() {
                            self.anim_tool = t;
                        }
                    }
                    if self.painting() {
                        hairline(ui);
                        self.paint_cells(ui);
                    }
                }
                Mode::Texture => self.paint_cells(ui),
                Mode::Render => {
                    let g = self.settings.guides;
                    let tip = format!("Composition guides: {} (click to cycle)", g.label());
                    if tool_cell(ui, Icon::Guides, g != Guides::Off, &tip).clicked() {
                        let i = Guides::ALL.iter().position(|x| *x == g).unwrap_or(0);
                        self.settings.guides = Guides::ALL[(i + 1) % Guides::ALL.len()];
                        self.touch_settings();
                    }
                    let tip = self.tip_for(CommandId::TogglePreview, "Show or hide the render preview window");
                    if tool_cell(ui, Icon::Image, self.settings.preview.visible, &tip).clicked() {
                        self.run_command(ctx, CommandId::TogglePreview);
                    }
                    if tool_cell(ui, Icon::Camera, self.ed.through_camera, "Look through the frame camera").clicked() {
                        self.ed.through_camera = !self.ed.through_camera;
                    }
                }
            }
            if self.painting() {
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| self.colour_cell(ui));
            }
        });
    }

    fn pose_cells(&mut self, ui: &mut egui::Ui) {
        tool_cell(ui, Icon::Bone, true, "Pose: drag joints to bend the skeleton");
        let tip = self.tip_for(CommandId::ToggleIk, "Inverse kinematics: the joint follows the mouse, parent bones bend");
        if tool_cell(ui, Icon::Ik, self.ed.ik_chain > 0, &tip).clicked() {
            self.ed.ik_chain = if self.ed.ik_chain > 0 { 0 } else { self.ik_len.max(1) };
        }
        let tip = self.tip_for(CommandId::FrameAll, "Frame everything in the view");
        if tool_cell(ui, Icon::Search, false, &tip).clicked() {
            self.ed.frame_all();
        }
    }

    /// Brush tools and selection tools, Aseprite order: pencil, brush, eraser, fill, eyedropper, shapes, selections.
    fn paint_cells(&mut self, ui: &mut egui::Ui) {
        let b = self.ed.brush;
        let none = self.paint.sel_tool.is_none();
        let cells: [(Tool, Option<bool>, Icon, &str, Option<CommandId>); 12] = [
            (Tool::Brush, Some(true), Icon::Pencil, "Pencil: hard pixel brush", Some(CommandId::ToolBrush)),
            (Tool::Brush, Some(false), Icon::Brush, "Brush: soft round brush", None),
            (Tool::Eraser, None, Icon::Eraser, "Eraser", Some(CommandId::ToolEraser)),
            (Tool::Fill, None, Icon::Fill, "Paint bucket", Some(CommandId::ToolFill)),
            (Tool::Gradient, None, Icon::Gradient, "Gradient: drag from the main to the second colour", Some(CommandId::ToolGradient)),
            (Tool::Picker, None, Icon::Picker, "Eyedropper", Some(CommandId::ToolPicker)),
            (Tool::Smudge, None, Icon::Smudge, "Smudge", Some(CommandId::ToolSmudge)),
            (Tool::Blur, None, Icon::Blur, "Blur", Some(CommandId::ToolBlur)),
            (Tool::Dodge, None, Icon::Sun, "Dodge / burn: lighten or darken (choose in the Brush section)", None),
            (Tool::Line, None, Icon::Line, "Line", None),
            (Tool::Rect, None, Icon::Rect, "Rectangle", None),
            (Tool::RectFilled, None, Icon::RectFilled, "Filled rectangle", None),
        ];
        for (t, pixel, icon, tip, cmd) in cells {
            let on = none && (b.tool == t || (t == Tool::Dodge && b.tool == Tool::Burn)) && pixel.is_none_or(|px| px == b.pixel);
            let tip = match cmd {
                Some(id) => self.tip_for(id, tip),
                None => tip.to_string(),
            };
            if tool_cell(ui, icon, on, &tip).clicked() {
                self.ed.brush.tool = t;
                if let Some(px) = pixel {
                    self.ed.brush.pixel = px;
                }
                self.paint.sel_tool = None;
            }
        }
        if let Some(canvas) = self.current_canvas() {
            hairline(ui);
            let can_island = matches!(canvas, psxcore::editor::Canvas::Base { .. } | psxcore::editor::Canvas::Patch { .. } | psxcore::editor::Canvas::Emotion { .. });
            for (t, icon, tip, ok) in [
                (SelTool::Rect, Icon::Marquee, "Rectangle select: drag. Shift adds, Ctrl subtracts", true),
                (SelTool::Wand, Icon::Wand, "Magic wand: pixels of a similar colour", true),
                (SelTool::Island, Icon::Island, "UV island: the whole island under the cursor", can_island),
            ] {
                if !ok {
                    continue;
                }
                if tool_cell(ui, icon, self.paint.sel_tool == Some(t), tip).clicked() {
                    self.paint.sel_tool = if self.paint.sel_tool == Some(t) { None } else { Some(t) };
                }
            }
        }
    }

    /// Main and second colour as Photoshop-style overlapping squares at the bottom of the tool strip;
    /// clicking the second square (or the arrows) swaps them.
    fn colour_cell(&mut self, ui: &mut egui::Ui) {
        let [r, g, b, a] = self.ed.brush.color;
        let (area, resp) = ui.allocate_exact_size(vec2(28.0, 36.0), Sense::click());
        let bg = egui::Rect::from_min_size(pos2(area.max.x - 18.0, area.max.y - 18.0), vec2(17.0, 17.0));
        let rect = egui::Rect::from_min_size(area.min + vec2(0.0, 0.0), vec2(21.0, 21.0));
        let p = ui.painter();
        let [br, bgc, bb, _] = self.ed.brush.bg;
        p.rect_filled(bg, compat::cr(0.0), Color32::from_rgb(br, bgc, bb));
        compat::stroke_rect(p, bg, 0.0, compat::stroke(1.0, Color32::WHITE));
        compat::stroke_rect(p, bg.expand(1.0), 0.0, compat::stroke(1.0, SHADE()));
        crate::icons::draw(p, Icon::Swap, pos2(area.min.x + 6.0, area.max.y - 6.0), 11.0, DIM());
        if resp.clicked() {
            let swap = resp.interact_pointer_pos().is_some_and(|q| !rect.contains(q));
            if swap {
                self.ed.brush.swap_colors();
            }
        }
        let p = ui.painter();
        // checkerboard behind translucent colours
        for (i, q) in [rect.left_top(), pos2(rect.center().x, rect.min.y), pos2(rect.min.x, rect.center().y), rect.center()].into_iter().enumerate() {
            p.rect_filled(egui::Rect::from_min_size(q, rect.size() * 0.5), compat::cr(0.0), if i == 0 || i == 3 { Color32::from_gray(90) } else { Color32::from_gray(60) });
        }
        p.rect_filled(rect, compat::cr(0.0), Color32::from_rgba_unmultiplied(r, g, b, a));
        compat::stroke_rect(p, rect, 0.0, compat::stroke(1.0, Color32::WHITE));
        compat::stroke_rect(p, rect.expand(1.0), 0.0, compat::stroke(1.0, SHADE()));
        let [r2, g2, b2, _] = self.ed.brush.bg;
        let tip = format!("Main colour #{r:02x}{g:02x}{b:02x} (alpha {a}) · second colour #{r2:02x}{g2:02x}{b2:02x}\nClick the second colour to swap ({})", self.settings.keymap.label(CommandId::SwapColors));
        widgets::with_tip(resp, &tip);
    }

    /// Obsidian-like status bar: message on the left, facts and the build on the right.
    pub fn status_bar(&mut self, ctx: &Context) {
        if !self.settings.show_status_bar {
            return;
        }
        TopBottomPanel::bottom("status").exact_height(22.0).frame(compat::frame(BG0(), 0.0).inner_margin(egui::Margin::symmetric(10, 2)).stroke(compat::stroke(1.0, LINE_SOFT()))).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let dim = |t: String| egui::RichText::new(t).size(11.5).color(DIM());
                crate::icons::draw(ui.painter(), mode_icon(self.ed.mode), pos2(ui.cursor().min.x + 7.0, ui.max_rect().center().y), 16.0, accent());
                ui.add_space(18.0);
                let msg = if self.ed.status.is_empty() { "Ready".to_string() } else { self.ed.status.clone() };
                let msg = match self.edit_summary() {
                    Some(s) if self.ed.status.is_empty() || self.ed.status.starts_with("Edit mode") => s,
                    _ => msg,
                };
                ui.label(dim(msg));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(dim(format!("Jank! {}", crate::appinfo::version_string())));
                    let sep = |ui: &mut egui::Ui| {
                        ui.label(egui::RichText::new("\u{00b7}").color(LINE()));
                    };
                    if self.settings.status_fps {
                        sep(ui);
                        ui.label(dim(format!("{:.0} fps  {:.1} ms", self.perf.fps(), self.perf.draw_ms)));
                    }
                    sep(ui);
                    let sel = self.ed.selected_object.and_then(|id| self.ed.project.scene.object(id)).map(|o| o.name.clone());
                    let tris = self.ed.selected_model().map_or(0, |m| m.triangle_count());
                    ui.label(dim(match sel {
                        Some(n) => format!("{n}  {tris} tris  {} faces selected", self.ed.selected_faces.len()),
                        None => "no object selected".to_string(),
                    }));
                    sep(ui);
                    ui.label(dim(format!("{} mode", mode_label(self.ed.mode))));
                });
            });
        });
    }
}
