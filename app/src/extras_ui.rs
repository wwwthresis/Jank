//! Smaller pieces of UI around the core features: the Krita-style radial menu (Space), the
//! "What's new" window, reference pictures, booleans between objects, indexed colour, inverse
//! kinematics, the graph editor of the timeline, themes in Settings and the hover highlight.
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{self, dim, section, slider_f, toggle_tip, Btn};
use crate::{compat, App};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Context, Key, Pos2, Rect, Sense, Shape, Ui};
use psxcore::anim::{Channel, ChannelKind, Ease};
use psxcore::commands::CommandId;
use psxcore::editmesh::BoolOp;
use psxcore::editor::{Canvas, Mode, RefView};

// ====================================================================== radial menu

/// The pop-up radial menu (Krita's pop-up palette): hold Space, point at an item, release.
/// A quick tap keeps it open until a click.
pub struct Radial {
    pub center: Pos2,
    pub items: Vec<(CommandId, Icon, &'static str)>,
    /// Recent colours shown on the inner ring while painting.
    pub colors: Vec<[u8; 4]>,
    pub opened: f64,
    /// Space is still held.
    pub held: bool,
    pub hot: Option<usize>,
    pub hot_color: Option<usize>,
}

const R_OUT: f32 = 132.0;
const R_ITEM: f32 = 96.0;
const R_COLOR: f32 = 50.0;

impl App {
    /// The commands offered by the radial menu in the current context.
    fn radial_items(&self) -> Vec<(CommandId, Icon, &'static str)> {
        use CommandId::*;
        if self.ed.edit_active() {
            return vec![
                (MeshExtrude, Icon::Extrude, "Extrude"),
                (MeshInset, Icon::Inset, "Inset"),
                (MeshBevel, Icon::Bevel, "Bevel"),
                (MeshLoopCut, Icon::LoopCut, "Loop cut"),
                (MeshKnife, Icon::Knife, "Knife"),
                (MeshMerge, Icon::Island, "Merge"),
                (MeshSubdivide, Icon::Grid, "Subdivide"),
                (MeshSelectVertex, Icon::Vertex, "Vertices"),
                (MeshSelectEdge, Icon::Edge, "Edges"),
                (MeshSelectFace, Icon::Face, "Faces"),
                (MeshXray, Icon::Eye, "X-ray"),
                (EditModeToggle, Icon::Cube, "Object mode"),
            ];
        }
        if self.painting() {
            return vec![
                (ToolBrush, Icon::Brush, "Brush"),
                (ToolEraser, Icon::Eraser, "Eraser"),
                (ToolFill, Icon::Fill, "Fill"),
                (ToolGradient, Icon::Gradient, "Gradient"),
                (ToolSmudge, Icon::Smudge, "Smudge"),
                (ToolBlur, Icon::Blur, "Blur"),
                (ToolPicker, Icon::Picker, "Eyedropper"),
                (SwapColors, Icon::Swap, "Swap colours"),
                (ToggleSymmetry, Icon::Symmetry, "Symmetry"),
                (ToggleProjectionPaint, Icon::Projection, "Project"),
                (Undo, Icon::Undo, "Undo"),
                (Redo, Icon::Redo, "Redo"),
            ];
        }
        match self.ed.mode {
            Mode::Model => vec![
                (EditModeToggle, Icon::Vertex, "Edit mesh"),
                (GizmoMove, Icon::Move, "Move"),
                (GizmoRotate, Icon::Rotate, "Rotate"),
                (GizmoScale, Icon::Scale, "Scale"),
                (AddCube, Icon::Cube, "Add cube"),
                (BooleanUnion, Icon::Boolean, "Union"),
                (BooleanDifference, Icon::Boolean, "Difference"),
                (AddReference, Icon::Reference, "Reference"),
                (FrameAll, Icon::Search, "Frame all"),
                (Undo, Icon::Undo, "Undo"),
            ],
            Mode::Pose | Mode::Animate => vec![
                (TogglePlay, Icon::Play, "Play"),
                (PrevFrame, Icon::StepBack, "Previous"),
                (NextFrame, Icon::StepFwd, "Next"),
                (AddFrame, Icon::Plus, "Add frame"),
                (DuplicateFrame, Icon::Copy, "Duplicate"),
                (ToggleIk, Icon::Ik, "IK"),
                (FrameAll, Icon::Search, "Frame all"),
                (Undo, Icon::Undo, "Undo"),
            ],
            _ => vec![
                (TogglePreview, Icon::Image, "Preview"),
                (ExportOutput, Icon::Export, "Export"),
                (FrameAll, Icon::Search, "Frame all"),
                (SaveProject, Icon::Save, "Save"),
                (Undo, Icon::Undo, "Undo"),
                (CommandPalette, Icon::Command, "Commands"),
            ],
        }
    }

    pub fn radial_open(&mut self, ctx: &Context) {
        if self.radial.is_some() {
            return;
        }
        let center = ctx.input(|i| i.pointer.latest_pos()).unwrap_or_else(|| compat::screen_rect(ctx).center());
        let sr = compat::screen_rect(ctx).shrink(R_OUT + 4.0);
        let center = pos2(center.x.clamp(sr.min.x, sr.max.x.max(sr.min.x)), center.y.clamp(sr.min.y, sr.max.y.max(sr.min.y)));
        let colors = if self.painting() { self.recent.iter().copied().take(12).collect() } else { vec![] };
        self.radial = Some(Radial { center, items: self.radial_items(), colors, opened: ctx.input(|i| i.time), held: true, hot: None, hot_color: None });
    }

    /// Draws the radial menu and handles its pointer and keys.
    pub fn radial_ui(&mut self, ctx: &Context) {
        let Some(mut r) = self.radial.take() else { return };
        let (pos, time, clicked, space_down, esc) = ctx.input(|i| (i.pointer.latest_pos(), i.time, i.pointer.any_click() || i.pointer.primary_released(), i.key_down(Key::Space), i.key_pressed(Key::Escape)));
        let n = r.items.len().max(1);
        let ang = |i: usize| -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
        r.hot = None;
        r.hot_color = None;
        if let Some(p) = pos {
            let d = p - r.center;
            let dist = d.length();
            if dist > R_COLOR + 14.0 && dist < R_OUT + 60.0 {
                let a = d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2;
                let a = a.rem_euclid(std::f32::consts::TAU);
                r.hot = Some(((a / (std::f32::consts::TAU / n as f32)) + 0.5) as usize % n);
            } else if !r.colors.is_empty() && dist > 18.0 && dist <= R_COLOR + 14.0 {
                let m = r.colors.len();
                let a = (d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
                r.hot_color = Some(((a / (std::f32::consts::TAU / m as f32)) + 0.5) as usize % m);
            }
        }
        let mut close = esc;
        let mut run = None;
        let mut color = None;
        if r.held && !space_down {
            r.held = false;
            if r.hot.is_some() || r.hot_color.is_some() {
                run = r.hot;
                color = r.hot_color;
                close = true;
            } else if time - r.opened > 0.35 {
                close = true;
            }
        } else if !r.held && clicked {
            run = r.hot;
            color = r.hot_color;
            close = true;
        }
        // draw
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("radial"));
        let p = ctx.layer_painter(layer);
        let c = r.center;
        p.circle_filled(c, R_OUT, alpha(BG0(), if is_light() { 240 } else { 225 }));
        p.circle_stroke(c, R_OUT, compat::stroke(1.0, LINE()));
        p.circle_stroke(c, R_COLOR + 14.0, compat::stroke(1.0, LINE_SOFT()));
        for (i, (id, icon, label)) in r.items.iter().enumerate() {
            let a = ang(i);
            let q = c + vec2(a.cos(), a.sin()) * R_ITEM;
            let hot = r.hot == Some(i);
            let on = self.radial_item_on(*id);
            p.circle_filled(q, 22.0, if hot { accent() } else if on { mix(BG2(), accent(), 0.35) } else { BG2() });
            p.circle_stroke(q, 22.0, compat::stroke(1.0, if hot { Color32::WHITE } else { LINE() }));
            crate::icons::draw(&p, *icon, q, 20.0, if hot { Color32::WHITE } else { TEXT() });
            if hot {
                let key = self.settings.keymap.label(*id);
                compat::text(&p, c + vec2(0.0, -8.0), Align2::CENTER_CENTER, label, 14.0, TEXT());
                if !key.is_empty() {
                    compat::text(&p, c + vec2(0.0, 10.0), Align2::CENTER_CENTER, &key, 11.5, DIM());
                }
            }
        }
        let m = r.colors.len();
        for (i, col) in r.colors.iter().enumerate() {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / m as f32;
            let q = c + vec2(a.cos(), a.sin()) * R_COLOR;
            let hot = r.hot_color == Some(i);
            p.circle_filled(q, if hot { 11.0 } else { 9.0 }, Color32::from_rgb(col[0], col[1], col[2]));
            p.circle_stroke(q, if hot { 11.0 } else { 9.0 }, compat::stroke(if hot { 2.0 } else { 1.0 }, if hot { Color32::WHITE } else { SHADE() }));
        }
        if r.hot.is_none() {
            if r.colors.is_empty() || r.hot_color.is_none() {
                let [cr, cg, cb, _] = self.ed.brush.color;
                if self.painting() {
                    p.circle_filled(c, 16.0, Color32::from_rgb(cr, cg, cb));
                    p.circle_stroke(c, 16.0, compat::stroke(1.0, Color32::WHITE));
                } else {
                    compat::text(&p, c, Align2::CENTER_CENTER, if r.held { "release on an item" } else { "click an item" }, 11.0, DIM());
                }
            }
        }
        ctx.request_repaint();
        if let Some(i) = color {
            if let Some(c) = r.colors.get(i) {
                self.ed.brush.color = *c;
            }
        }
        if !close {
            self.radial = Some(r);
            return;
        }
        // the click that closes the menu must not reach the viewport
        self.vp.block_until_release = true;
        if let Some(i) = run {
            let id = r.items[i].0;
            self.run_command(ctx, id);
        }
    }
    fn radial_item_on(&self, id: CommandId) -> bool {
        use psxcore::paint::Tool;
        let t = self.ed.brush.tool;
        match id {
            CommandId::ToolBrush => t == Tool::Brush,
            CommandId::ToolEraser => t == Tool::Eraser,
            CommandId::ToolFill => t == Tool::Fill,
            CommandId::ToolGradient => t == Tool::Gradient,
            CommandId::ToolSmudge => t == Tool::Smudge,
            CommandId::ToolBlur => t == Tool::Blur,
            CommandId::ToolPicker => t == Tool::Picker,
            CommandId::ToggleSymmetry => self.ed.brush.symmetry,
            CommandId::ToggleProjectionPaint => self.ed.project_paint,
            CommandId::ToggleIk => self.ed.ik_chain > 0,
            CommandId::MeshXray => self.ed.edit.as_ref().is_some_and(|s| s.xray),
            CommandId::MeshSelectVertex | CommandId::MeshSelectEdge | CommandId::MeshSelectFace => {
                let m = self.ed.edit_mesh().map(|m| m.mode);
                use psxcore::editmesh::SelectMode as S;
                matches!((id, m), (CommandId::MeshSelectVertex, Some(S::Vertex)) | (CommandId::MeshSelectEdge, Some(S::Edge)) | (CommandId::MeshSelectFace, Some(S::Face)))
            }
            CommandId::TogglePlay => self.ed.playing,
            _ => false,
        }
    }

    // ================================================================== commands of this file

    /// Paint-context commands (tool keys, colour swap, brush size).
    pub fn run_paint_command(&mut self, id: CommandId) {
        use psxcore::paint::Tool;
        let tool = |app: &mut App, t: Tool| {
            app.ed.brush.tool = t;
            app.paint.sel_tool = None;
        };
        match id {
            CommandId::ToolBrush => tool(self, Tool::Brush),
            CommandId::ToolEraser => tool(self, Tool::Eraser),
            CommandId::ToolFill => tool(self, Tool::Fill),
            CommandId::ToolGradient => tool(self, Tool::Gradient),
            CommandId::ToolPicker => tool(self, Tool::Picker),
            CommandId::ToolSmudge => tool(self, Tool::Smudge),
            CommandId::ToolBlur => tool(self, Tool::Blur),
            CommandId::SwapColors => self.ed.brush.swap_colors(),
            CommandId::BrushSmaller => self.ed.brush.size = (self.ed.brush.size * 0.8).floor().clamp(1.0, 64.0),
            CommandId::BrushBigger => self.ed.brush.size = (self.ed.brush.size * 1.25).ceil().clamp(1.0, 64.0),
            CommandId::ToggleSymmetry => {
                self.ed.brush.symmetry = !self.ed.brush.symmetry;
                self.info(if self.ed.brush.symmetry { "Symmetric painting on (mirrored left/right)" } else { "Symmetric painting off" });
            }
            CommandId::ToggleProjectionPaint => {
                self.ed.project_paint = !self.ed.project_paint;
                self.info(if self.ed.project_paint { "Projection painting on: the brush paints through the view, across seams and objects" } else { "Projection painting off: the brush paints in the UV map" });
            }
            _ => {}
        }
    }

    /// Boolean between the selected object and the chosen cutter object (or, in Edit mode,
    /// between the selected polygons and the rest of the mesh).
    pub fn run_boolean(&mut self, op: BoolOp) {
        if self.ed.edit_active() {
            if !self.ed.edit_boolean_selected(op) {
                self.info("Boolean: select closed polygons that overlap the rest of the mesh");
            }
            return;
        }
        let Some(target) = self.ed.selected_object else {
            self.info("Boolean: select the object to change first");
            return;
        };
        let cutter = self.bool_cutter.filter(|c| *c != target && self.ed.project.scene.object(*c).is_some());
        let cutter = cutter.or_else(|| {
            // with exactly two objects the other one is the obvious cutter
            let others: Vec<u32> = self.ed.project.scene.objects.iter().filter(|o| o.id != target && o.visible).map(|o| o.id).collect();
            (others.len() == 1).then(|| others[0])
        });
        let Some(cutter) = cutter else {
            self.info("Boolean: choose the cutter object in the Boolean section of the left panel");
            return;
        };
        if self.ed.boolean_with(cutter, op, self.bool_delete) {
            self.info("Boolean done");
            if self.bool_delete {
                self.bool_cutter = None;
            }
        } else {
            self.error("Boolean failed: both objects need closed surfaces that overlap");
        }
    }

    // ================================================================== Model mode sections

    pub fn boolean_section(&mut self, ui: &mut Ui) {
        section(ui, "Boolean", accent(), |ui| {
            let target = self.ed.selected_object;
            let names: Vec<(u32, String)> = self.ed.project.scene.objects.iter().filter(|o| Some(o.id) != target).map(|o| (o.id, o.name.clone())).collect();
            if target.is_none() || names.is_empty() {
                dim(ui, "Select an object and add a second one to cut with.");
                return;
            }
            let cur = self.bool_cutter.and_then(|c| names.iter().find(|n| n.0 == c)).map(|n| n.1.clone()).unwrap_or_else(|| "choose…".into());
            ui.horizontal(|ui| {
                dim(ui, "cutter");
                egui::ComboBox::from_id_salt("bool_cutter").selected_text(cur).width(150.0).show_ui(ui, |ui| {
                    for (id, n) in &names {
                        if ui.selectable_label(self.bool_cutter == Some(*id), n).clicked() {
                            self.bool_cutter = Some(*id);
                        }
                    }
                });
            });
            toggle_tip(ui, &mut self.bool_delete, "Delete the cutter afterwards", accent(), "Off: the cutter stays (hidden) so the cut can be repeated");
            ui.horizontal_wrapped(|ui| {
                for (op, l, id) in [(BoolOp::Union, "Union", CommandId::BooleanUnion), (BoolOp::Difference, "Difference", CommandId::BooleanDifference), (BoolOp::Intersect, "Intersect", CommandId::BooleanIntersect)] {
                    let tip = self.tip_for(id, id.info().description);
                    if Btn::new(l).icon(Icon::Boolean).accent(accent()).tip(&tip).show(ui).clicked() {
                        self.run_boolean(op);
                    }
                }
            });
        });
    }

    pub fn refs_section(&mut self, ui: &mut Ui) {
        section(ui, &format!("Reference pictures ({})", self.ed.project.scene.refs.len()), accent(), |ui| {
            dim(ui, "Pictures in the 3D view to model and pose from. They are never rendered or exported.");
            if Btn::new("Add picture…").icon(Icon::Reference).accent(accent()).size(vec2(ui.available_width(), 30.0)).show(ui).clicked() {
                self.dialog = Some(crate::dialogs::browser(crate::dialogs::Purpose::AddReference));
            }
            let rows: Vec<(String, bool)> = self.ed.project.scene.refs.iter().map(|r| (r.name.clone(), r.visible)).collect();
            for (i, (name, vis)) in rows.iter().enumerate() {
                let r = widgets::tree_row(ui, Icon::Reference, name, self.ed.selected_ref == Some(i), Some(*vis), "Select this picture");
                if r.eye {
                    self.ed.ref_edit(i, |r| r.visible = !r.visible);
                } else if r.row.clicked() {
                    self.ed.selected_ref = if self.ed.selected_ref == Some(i) { None } else { Some(i) };
                }
            }
            let Some(i) = self.ed.selected_ref.filter(|&i| i < self.ed.project.scene.refs.len()) else { return };
            let mut r = self.ed.project.scene.refs[i].clone();
            let before = (r.opacity, r.size, r.transform.pos, r.transform.rot_deg, r.visible, r.locked, r.on_top);
            ui.add_space(4.0);
            slider_f(ui, "opacity", &mut r.opacity, 0.05..=1.0);
            slider_f(ui, "height", &mut r.size, 0.1..=50.0);
            let mut pos = [r.transform.pos.x, r.transform.pos.y, r.transform.pos.z];
            if widgets::drag3(ui, "position", &mut pos, 0.02) {
                r.transform.pos = psxcore::math::Vec3::new(pos[0], pos[1], pos[2]);
            }
            let mut rot = [r.transform.rot_deg.x, r.transform.rot_deg.y, r.transform.rot_deg.z];
            if widgets::drag3(ui, "rotation", &mut rot, 0.5) {
                r.transform.rot_deg = psxcore::math::Vec3::new(rot[0], rot[1], rot[2]);
            }
            ui.horizontal_wrapped(|ui| {
                toggle_tip(ui, &mut r.locked, "Locked", accent(), "Locked pictures cannot be clicked or moved in the view");
                toggle_tip(ui, &mut r.on_top, "In front", accent(), "Draw over the models instead of behind them");
            });
            if before != (r.opacity, r.size, r.transform.pos, r.transform.rot_deg, r.visible, r.locked, r.on_top) {
                self.cont(|ed| {
                    ed.ref_edit(i, |x| {
                        x.opacity = r.opacity;
                        x.size = r.size;
                        x.transform = r.transform;
                        x.locked = r.locked;
                        x.on_top = r.on_top;
                    });
                });
            }
            ui.horizontal_wrapped(|ui| {
                dim(ui, "face");
                for (v, l) in [(RefView::Front, "Front"), (RefView::Side, "Side"), (RefView::Top, "Top"), (RefView::Camera, "View")] {
                    if Btn::new(l).tip("Turn the picture to this view").show(ui).clicked() {
                        self.ed.ref_align(i, v);
                    }
                }
                if Btn::new("").icon(Icon::Trash).accent(DANGER()).tip("Remove this picture").show(ui).clicked() {
                    self.ed.ref_remove(i);
                }
            });
            dim(ui, "Drag a picture in the view (Select tool) to move it.");
        });
    }

    // ================================================================== Pose mode

    pub fn ik_section(&mut self, ui: &mut Ui) {
        section(ui, "Inverse kinematics", accent(), |ui| {
            let mut on = self.ed.ik_chain > 0;
            let tip = self.tip_for(CommandId::ToggleIk, "Dragging the end of a bone moves the bones above it too, so a hand or foot can be placed directly");
            if toggle_tip(ui, &mut on, "IK while dragging", accent(), &tip).changed() {
                self.ed.ik_chain = if on { self.ik_len.max(1) } else { 0 };
            }
            if on {
                let mut n = self.ed.ik_chain as f32;
                if slider_f(ui, "chain bones", &mut n, 1.0..=8.0) {
                    self.ed.ik_chain = n.round().max(1.0) as usize;
                    self.ik_len = self.ed.ik_chain;
                }
                dim(ui, "Drag a joint: the joint follows the mouse, its parent bones bend to reach it.");
            }
        });
    }

    // ================================================================== Texture mode: indexed colour

    pub fn indexed_section(&mut self, ui: &mut Ui) {
        let Some(Canvas::Base { asset, tex }) = self.current_canvas() else { return };
        section(ui, "Indexed colour", accent(), |ui| {
            let ix = self.ed.indexed(asset, tex).map(|ix| (ix.palette.clone(), ix.usage()));
            match ix {
                None => {
                    dim(ui, "Lock this texture to a palette, like the PlayStation's CLUT textures: every pixel stores a palette index, and changing a palette colour recolours the whole texture.");
                    let pal = self.paint.palettes.get(self.paint.pal_idx).cloned();
                    let name = pal.as_ref().map_or("—".to_string(), |p| format!("{} ({} colours)", p.name, p.colors.len()));
                    dim(ui, &format!("Palette: {name}"));
                    if Btn::new("Convert to indexed").icon(Icon::Palette).accent(accent()).enabled(pal.is_some()).size(vec2(ui.available_width(), 30.0)).tip("Uses the palette chosen in the Palette section below").show(ui).clicked() {
                        if let Some(p) = pal {
                            if self.ed.indexed_enable(asset, tex, p) {
                                self.info("The texture is now indexed");
                            } else {
                                self.error("This texture cannot be indexed (it has layers)");
                            }
                        }
                    }
                }
                Some((pal, usage)) => {
                    dim(ui, &format!("{} colours. Click to paint with a colour, right-click to change it everywhere.", pal.colors.len()));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                        for (i, c) in pal.colors.iter().enumerate() {
                            let used = usage.get(i).copied().unwrap_or(0);
                            let resp = widgets::swatch(ui, Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]), self.ed.brush.color == *c, 20.0);
                            let resp = widgets::with_tip(resp, &format!("#{:02x}{:02x}{:02x} · index {i} · {used} pixels", c[0], c[1], c[2]));
                            if resp.clicked() {
                                self.ed.brush.color = *c;
                            }
                            if resp.secondary_clicked() {
                                self.idx_edit = Some((i, [c[0], c[1], c[2]]));
                            }
                        }
                    });
                    if let Some((i, mut rgb)) = self.idx_edit {
                        ui.horizontal(|ui| {
                            dim(ui, &format!("index {i}"));
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                self.idx_edit = Some((i, rgb));
                                let a = pal.colors.get(i).map_or(255, |c| c[3]);
                                self.cont(|ed| {
                                    ed.indexed_set_color(asset, tex, i, [rgb[0], rgb[1], rgb[2], a]);
                                });
                            }
                            if Btn::new("Done").show(ui).clicked() {
                                self.idx_edit = None;
                            }
                        });
                    }
                    if Btn::new("Back to full colour").tip("Keeps the picture, forgets the indices").show(ui).clicked() {
                        self.ed.indexed_disable(asset, tex);
                        self.idx_edit = None;
                    }
                }
            }
        });
    }

    // ================================================================== What's new

    pub fn whats_new_dialog(&mut self, ctx: &Context) -> Option<crate::dialogs::Dialog> {
        let mut open = true;
        let mut run = None;
        let size = vec2(620.0, 560.0);
        egui::Window::new("What's new in Jank!").collapsible(false).resizable(true).default_size(size).default_pos(compat::centered_pos(ctx, size)).show(ctx, |ui| {
            ui.set_min_width(580.0);
            ui.horizontal(|ui| {
                crate::icons::draw(ui.painter(), Icon::Sparkle, ui.cursor().min + vec2(14.0, 16.0), 24.0, accent());
                ui.add_space(30.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(format!("Jank! {}", crate::appinfo::version_string())).size(16.0).strong());
                    dim(ui, "Modelling, painting and animation got much deeper. Click a heading to try it.");
                });
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(430.0).auto_shrink([false, false]).show(ui, |ui| {
                for (icon, title, text, cmd) in WHATS_NEW {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                        crate::icons::draw(ui.painter(), *icon, r.center(), 20.0, accent());
                        ui.vertical(|ui| {
                            let t = egui::RichText::new(*title).size(14.0).strong().color(TEXT());
                            match cmd {
                                Some(c) => {
                                    if ui.add(egui::Label::new(t).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                        run = Some(*c);
                                    }
                                }
                                None => {
                                    ui.label(t);
                                }
                            }
                            ui.add(egui::Label::new(egui::RichText::new(*text).size(12.5).color(DIM())).wrap());
                        });
                    });
                    ui.add_space(6.0);
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if Btn::new("Got it").accent(accent()).on(true).size(vec2(110.0, 30.0)).show(ui).clicked() {
                    open = false;
                }
                dim(ui, "This window is in Help → What's new.");
            });
        });
        if let Some(c) = run {
            self.pending = Some(c);
            return None;
        }
        open.then_some(crate::dialogs::Dialog::WhatsNew)
    }

    /// Shows "What's new" once after an update (not on the very first start).
    pub fn whats_new_at_start(&mut self, settings_existed: bool) {
        let build = crate::appinfo::build_number();
        if self.settings.last_seen_build < build {
            if settings_existed && self.dialog.is_none() {
                self.dialog = Some(crate::dialogs::Dialog::WhatsNew);
            }
            self.settings.last_seen_build = build;
            self.touch_settings();
        }
    }

    // ================================================================== themes

    pub fn current_theme(&self) -> &psxcore::themes::Theme {
        self.themes.iter().find(|t| t.id() == self.settings.theme).unwrap_or(&self.themes[0])
    }

    pub fn reload_themes(&mut self) {
        let dir = psxcore::themes::themes_dir();
        let (t, problems) = psxcore::themes::load_all(dir.as_deref());
        self.themes = t;
        if let Some(p) = problems.first() {
            self.error(format!("Theme file problem: {p}"));
        }
    }

    pub fn themes_section(&mut self, ui: &mut Ui) {
        section(ui, "Theme", accent(), |ui| {
            dim(ui, "Colours of the whole interface. Themes are plain text files you can copy and edit.");
            let mut pick = None;
            ui.horizontal_wrapped(|ui| {
                for t in &self.themes {
                    let (rect, resp) = ui.allocate_exact_size(vec2(112.0, 64.0), Sense::click());
                    let p = ui.painter();
                    let c = |i: usize| Color32::from_rgb(t.colors[i][0], t.colors[i][1], t.colors[i][2]);
                    let on = self.settings.theme == t.id();
                    p.rect_filled(rect, compat::cr(RADIUS + 1.0), c(0));
                    let panel = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(34.0, 34.0));
                    p.rect_filled(panel, compat::cr(2.0), c(1));
                    p.rect_filled(Rect::from_min_size(rect.min + vec2(46.0, 8.0), vec2(58.0, 12.0)), compat::cr(2.0), c(2));
                    let acc = t.accent.unwrap_or(self.settings.accent.rgb());
                    p.rect_filled(Rect::from_min_size(rect.min + vec2(46.0, 26.0), vec2(30.0, 12.0)), compat::cr(2.0), Color32::from_rgb(acc[0], acc[1], acc[2]));
                    for (k, i) in [13usize, 14, 15].iter().enumerate() {
                        p.circle_filled(rect.min + vec2(84.0 + k as f32 * 8.0, 32.0), 3.0, c(*i));
                    }
                    compat::text(p, pos2(rect.min.x + 6.0, rect.max.y - 6.0), Align2::LEFT_BOTTOM, &t.name, 11.5, c(6));
                    compat::stroke_rect(p, rect, RADIUS + 1.0, compat::stroke(if on { 2.0 } else { 1.0 }, if on { accent() } else if resp.hovered() { c(7) } else { c(4) }));
                    let tip = match &t.path {
                        Some(path) => format!("{} — {}", t.name, path.display()),
                        None => format!("{} (built in)", t.name),
                    };
                    if widgets::with_tip(resp, &tip).clicked() {
                        pick = Some(t.id());
                    }
                }
            });
            if let Some(id) = pick {
                self.settings.theme = id;
                self.touch_settings();
            }
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Reload theme files").icon(Icon::Loop).tip("Read the theme folder again after editing a file").show(ui).clicked() {
                    self.reload_themes();
                    self.info(format!("{} themes", self.themes.len()));
                }
                if Btn::new("Save a copy to edit").icon(Icon::Save).tip("Writes the current theme as a .theme file into the theme folder").show(ui).clicked() {
                    let mut t = self.current_theme().clone();
                    if t.path.is_none() {
                        t.name = format!("{} custom", t.name);
                    }
                    match psxcore::themes::themes_dir() {
                        Some(dir) => match psxcore::themes::save(&t, &dir) {
                            Ok(p) => {
                                self.settings.theme = t.id();
                                self.touch_settings();
                                self.reload_themes();
                                self.info(format!("Theme saved: {}", p.display()));
                            }
                            Err(e) => self.error(e),
                        },
                        None => self.error("No settings folder on this system"),
                    }
                }
            });
            if let Some(d) = psxcore::themes::themes_dir() {
                dim(ui, &format!("Theme folder: {}", d.display()));
            }
            if self.current_theme().accent.is_some() {
                dim(ui, "This theme brings its own accent colour (it replaces the accent below).");
            }
        });
    }

    // ================================================================== hover highlight and face tint

    /// Picture drawn over the viewport: selected / hovered polygons in Edit mode and an outline
    /// around the object under the mouse. Returns false when there is nothing to show.
    pub fn update_overlay(&mut self, ctx: &Context, rect: Rect, sc: f32, w: u32, h: u32) -> bool {
        let edit = self.ed.edit_active();
        let hovering = self.settings.hover_highlight && !matches!(self.ed.mode, Mode::Render) && self.vp.drag.is_none() && self.vp.gizmo.is_none() && self.radial.is_none() && !self.ed.playing;
        let hover_obj = if hovering && !edit {
            ctx.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)).and_then(|p| {
                let (x, y) = (((p.x - rect.min.x) / sc) as i32, ((p.y - rect.min.y) / sc) as i32);
                self.ed.pick_info(self.rend.frame.hit_at(x, y)).map(|i| i.object)
            })
        } else {
            None
        };
        // hovering the selected object adds nothing
        let hover_obj = hover_obj.filter(|id| Some(*id) != self.ed.selected_object || self.ed.mode != Mode::Model);
        self.vp.hover_obj = hover_obj;
        if !edit && hover_obj.is_none() {
            self.vp.overlay_key = 0;
            return false;
        }
        let mut k = 0xcbf29ce484222325u64;
        let mut mix_k = |x: u64| k = (k ^ x).wrapping_mul(0x100000001b3);
        mix_k(self.vp.gen);
        mix_k(hover_obj.map_or(u64::MAX, |v| v as u64));
        mix_k(((w as u64) << 32) | h as u64);
        if let Some(s) = self.ed.edit.as_ref() {
            mix_k(s.mesh.sel_f.len() as u64);
            for f in s.mesh.sel_f.iter().take(4096) {
                mix_k(*f as u64);
            }
            mix_k(match s.hover {
                Some(psxcore::editor::MeshHit::Face(f)) => f as u64 + 1,
                _ => 0,
            });
        }
        let acc = accent();
        mix_k(u32::from_ne_bytes(acc.to_array()) as u64);
        if k == self.vp.overlay_key {
            return true;
        }
        self.vp.overlay_key = k;
        let pm = |c: Color32, a: u8| -> [u8; 4] {
            let f = a as u32;
            [(c.r() as u32 * f / 255) as u8, (c.g() as u32 * f / 255) as u8, (c.b() as u32 * f / 255) as u8, a]
        };
        let (sel_c, hov_c) = (pm(acc, 80), pm(Color32::WHITE, 40));
        let ed = &self.ed;
        let tint = |o: u16, m: u16, t: u32| -> [u8; 4] {
            if !edit {
                return [0; 4];
            }
            let st = ed.edit_face_state(o, m, t);
            if st & 1 != 0 {
                sel_c
            } else if st & 2 != 0 {
                hov_c
            } else {
                [0; 4]
            }
        };
        let outline: Vec<(u16, [u8; 4])> = hover_obj.map(|id| (psxcore::compose::hit_object_id(id), pm(mix(acc, Color32::WHITE, 0.45), 230))).into_iter().collect();
        let width = ((1.6 / sc).round() as i32).max(1);
        let px = self.rend.frame.overlay(&tint, &outline, width);
        if let Some(gl) = self.gl.clone() {
            self.surf[crate::SURF_OVERLAY].upload(&gl, w, h, &px);
        }
        true
    }

    // ================================================================== graph editor

    /// The curves view of the timeline: animated values over time with draggable keys, plus the
    /// easing of the in-between that starts at the current frame.
    pub fn graph_editor(&mut self, ui: &mut Ui) {
        let Some(anim) = self.ed.animation() else { return };
        let total = anim.total_ticks().max(1) as f32;
        let n = anim.frames.len();
        let starts: Vec<f32> = (0..n).map(|i| anim.frame_start(i) as f32).collect();
        let tweens: Vec<bool> = anim.frames.iter().map(|f| f.tween).collect();
        let cur = self.ed.current_frame;
        let cur_ease = anim.frames.get(cur).map(|f| f.ease).unwrap_or_default();
        let all = self.ed.graph_channels();
        let sel = self.ed.selected_object;
        let chans: Vec<Channel> = all.iter().copied().filter(|c| !self.graph.only_selected || matches!(c.kind, ChannelKind::CamPos(_)) || Some(c.object) == sel).collect();
        let avail = ui.available_size();
        let h = avail.y.max(90.0);
        ui.horizontal(|ui| {
            // ---- channel list
            ui.allocate_ui(vec2(190.0, h), |ui| {
                ui.set_min_size(vec2(190.0, h));
                ui.vertical(|ui| {
                    toggle_tip(ui, &mut self.graph.only_selected, "Selected object only", accent(), "Show only the channels of the selected object (and the camera)");
                    toggle_tip(ui, &mut self.graph.normalize, "Normalize", accent(), "Scale every curve to the same height, so small and large values can be compared");
                    egui::ScrollArea::vertical().id_salt("graph_ch").auto_shrink([false, false]).show(ui, |ui| {
                        if chans.is_empty() {
                            dim(ui, "Nothing moves yet: change a pose or a placement on another frame.");
                        }
                        for (ci, ch) in chans.iter().enumerate() {
                            let hidden = self.graph.hidden.contains(ch);
                            let col = channel_color(ch, ci);
                            let obj = match ch.kind {
                                ChannelKind::CamPos(_) => "Camera".to_string(),
                                _ => self.ed.project.scene.object(ch.object).map(|o| o.name.clone()).unwrap_or_default(),
                            };
                            let what = match ch.kind {
                                ChannelKind::BoneRot(b, i) => {
                                    let bone = self.ed.project.scene.object(ch.object).and_then(|o| self.ed.project.assets.get(o.asset)).and_then(|a| a.model.skeleton.as_ref()).and_then(|s| s.bones.get(b)).map(|b| b.name.clone());
                                    match bone {
                                        Some(n) => format!("{n} rotation {}", ["X", "Y", "Z"][i.min(2)]),
                                        None => ch.label(),
                                    }
                                }
                                _ => ch.label(),
                            };
                            let label = if matches!(ch.kind, ChannelKind::CamPos(_)) { what } else { format!("{obj}: {what}") };
                            ui.horizontal(|ui| {
                                let (r, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                                ui.painter().rect_filled(r.shrink(2.0), compat::cr(2.0), if hidden { alpha(col, 50) } else { col });
                                let l = ui.add(egui::Label::new(egui::RichText::new(label).size(11.5).color(if hidden { DIM() } else { TEXT() })).sense(Sense::click()));
                                if resp.clicked() || l.clicked() {
                                    if hidden {
                                        self.graph.hidden.retain(|c| c != ch);
                                    } else {
                                        self.graph.hidden.push(*ch);
                                    }
                                }
                            });
                        }
                    });
                });
            });
            // ---- ease editor of the current frame
            let ease_w = 170.0;
            let plot_w = (ui.available_width() - ease_w - 12.0).max(120.0);
            let (rect, bg) = ui.allocate_exact_size(vec2(plot_w, h), Sense::click_and_drag());
            self.graph_plot(ui, rect, &bg, &chans, total, &starts, &tweens);
            ui.allocate_ui(vec2(ease_w, h), |ui| {
                ui.vertical(|ui| {
                    dim(ui, &format!("Ease: frame {} → {}", cur + 1, (cur + 2).min(n)));
                    let mut e = cur_ease;
                    if ease_editor(ui, &mut e, 96.0) {
                        self.cont(|ed| {
                            ed.graph_set_ease(cur, e);
                        });
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                        for (l, p) in [("Linear", Ease::LINEAR), ("In", Ease::EASE_IN), ("Out", Ease::EASE_OUT), ("In-out", Ease::EASE_IN_OUT), ("Back", Ease::BACK)] {
                            if Btn::new(l).on(cur_ease == p).accent(accent()).size(vec2(52.0, 24.0)).show(ui).clicked() {
                                self.ed.graph_set_ease(cur, p);
                            }
                        }
                    });
                    if !tweens.get(cur).copied().unwrap_or(false) {
                        dim(ui, "(choosing an ease turns the tween on)");
                    }
                });
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn graph_plot(&mut self, ui: &mut Ui, rect: Rect, bg: &egui::Response, chans: &[Channel], total: f32, starts: &[f32], tweens: &[bool]) {
        let p = ui.painter().with_clip_rect(rect);
        p.rect_filled(rect, compat::cr(2.0), BG0());
        let inner = rect.shrink2(vec2(10.0, 12.0));
        let tx = |t: f32| inner.min.x + t / total * inner.width();
        // grid: one line per frame start, labels on top
        for (i, s) in starts.iter().enumerate() {
            let x = tx(*s);
            p.line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], compat::stroke(1.0, LINE_SOFT()));
            compat::text(&p, pos2(x + 2.0, rect.min.y + 1.0), Align2::LEFT_TOP, &(i + 1).to_string(), 10.0, DIM());
        }
        let ph = tx(self.ed.tick as f32 + 0.5);
        p.line_segment([pos2(ph, rect.min.y), pos2(ph, rect.max.y)], compat::stroke(1.5, accent()));
        let visible: Vec<(usize, Channel)> = chans.iter().copied().enumerate().filter(|(_, c)| !self.graph.hidden.contains(c)).collect();
        // value ranges
        let samples = (inner.width() / 3.0).clamp(16.0, 600.0) as usize;
        let mut curves: Vec<(usize, Channel, Vec<f32>, Vec<Option<f32>>)> = vec![];
        for (ci, ch) in &visible {
            let vals: Vec<f32> = (0..=samples).map(|s| self.ed.graph_sample(ch, (s as f32 / samples as f32 * total).min(total - 0.001)).unwrap_or(0.0)).collect();
            let keys: Vec<Option<f32>> = (0..starts.len()).map(|i| self.ed.graph_key(i, ch)).collect();
            curves.push((*ci, *ch, vals, keys));
        }
        let range_of = |vals: &[f32], keys: &[Option<f32>]| {
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            for v in vals.iter().copied().chain(keys.iter().flatten().copied()) {
                lo = lo.min(v);
                hi = hi.max(v);
            }
            if lo > hi {
                (0.0, 1.0)
            } else if hi - lo < 1e-3 {
                (lo - 0.5, hi + 0.5)
            } else {
                let pad = (hi - lo) * 0.08;
                (lo - pad, hi + pad)
            }
        };
        let global = {
            let mut lo = f32::MAX;
            let mut hi = f32::MIN;
            for (_, _, v, k) in &curves {
                let (a, b) = range_of(v, k);
                lo = lo.min(a);
                hi = hi.max(b);
            }
            if lo > hi {
                (0.0, 1.0)
            } else {
                (lo, hi)
            }
        };
        if !self.graph.normalize && !curves.is_empty() {
            for f in [0.0f32, 0.5, 1.0] {
                let v = global.0 + (global.1 - global.0) * f;
                let y = inner.max.y - f * inner.height();
                p.line_segment([pos2(rect.min.x, y), pos2(rect.max.x, y)], compat::stroke(1.0, alpha(LINE_SOFT(), 120)));
                compat::text(&p, pos2(rect.max.x - 4.0, y - 1.0), Align2::RIGHT_BOTTOM, &format!("{v:.2}"), 10.0, DIM());
            }
        }
        let mut grab: Option<(usize, Channel, (f32, f32))> = None;
        let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
        let mut hot: Option<(usize, Channel, (f32, f32))> = None;
        for (ci, ch, vals, keys) in &curves {
            let (lo, hi) = if self.graph.normalize { range_of(vals, keys) } else { global };
            let ty = |v: f32| inner.max.y - (v - lo) / (hi - lo).max(1e-6) * inner.height();
            let col = channel_color(ch, *ci);
            let pts: Vec<Pos2> = vals.iter().enumerate().map(|(s, v)| pos2(inner.min.x + s as f32 / samples as f32 * inner.width(), ty(*v))).collect();
            p.add(Shape::line(pts, compat::stroke(1.5, col)));
            for (i, k) in keys.iter().enumerate() {
                let Some(v) = k else { continue };
                let q = pos2(tx(starts[i]), ty(*v));
                let is_drag = self.graph.drag.as_ref().is_some_and(|d| d.0 == i && d.1 == *ch);
                let near = pointer.is_some_and(|m| (m - q).length() < 7.0);
                if near && hot.is_none() {
                    hot = Some((i, *ch, (lo, hi)));
                }
                let sz = if is_drag || near { 5.0 } else { 3.5 };
                p.rect_filled(Rect::from_center_size(q, vec2(sz * 2.0, sz * 2.0)), compat::cr(1.0), if is_drag { Color32::WHITE } else { col });
                compat::stroke_rect(&p, Rect::from_center_size(q, vec2(sz * 2.0, sz * 2.0)), 1.0, compat::stroke(1.0, BG0()));
                if near && bg.drag_started() {
                    grab = Some((i, *ch, (lo, hi)));
                }
            }
        }
        // in-betweens without a tween are holds: mark them
        for (i, t) in tweens.iter().enumerate() {
            if !t && i + 1 < starts.len() {
                compat::text(&p, pos2(tx(starts[i]) + 3.0, rect.max.y - 2.0), Align2::LEFT_BOTTOM, "hold", 9.5, alpha(DIM(), 160));
            }
        }
        if let Some(g) = grab {
            self.graph.drag = Some(g);
            self.ed.goto_frame(g.0);
        }
        if let (Some((i, ch, (lo, hi))), true) = (self.graph.drag, bg.dragged()) {
            if let Some(m) = pointer {
                let v = lo + (inner.max.y - m.y) / inner.height().max(1.0) * (hi - lo);
                let v = if ui.ctx().input(|i| i.modifiers.ctrl) { (v * 10.0).round() / 10.0 } else { v };
                self.cont(|ed| {
                    ed.graph_set_key(i, &ch, v);
                });
                compat::text(&p, m + vec2(10.0, -10.0), Align2::LEFT_BOTTOM, &format!("{}: {v:.3}", ch.label()), 11.5, TEXT());
            }
        }
        if !bg.dragged() && !bg.drag_started() {
            self.graph.drag = None;
        }
        if bg.clicked() && hot.is_none() {
            if let Some(m) = pointer {
                let t = ((m.x - inner.min.x) / inner.width() * total).clamp(0.0, total - 1.0);
                let i = starts.iter().rposition(|s| *s <= t).unwrap_or(0);
                self.ed.goto_frame(i);
            }
        } else if bg.clicked() {
            if let Some((i, _, _)) = hot {
                self.ed.goto_frame(i);
            }
        }
        if hot.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if curves.is_empty() {
            compat::text(&p, rect.center(), Align2::CENTER_CENTER, "No animated values to show", 12.0, DIM());
        }
        compat::stroke_rect(&p, rect, 2.0, compat::stroke(1.0, LINE_SOFT()));
    }
}

/// State of the graph editor.
#[derive(Default)]
pub struct GraphState {
    pub open: bool,
    pub only_selected: bool,
    pub normalize: bool,
    pub hidden: Vec<Channel>,
    /// Key being dragged: (frame, channel, value range of its curve).
    pub drag: Option<(usize, Channel, (f32, f32))>,
}

fn channel_color(ch: &Channel, i: usize) -> Color32 {
    let axis = match ch.kind {
        ChannelKind::Pos(a) | ChannelKind::Rot(a) | ChannelKind::Scale(a) | ChannelKind::CamPos(a) => a,
        ChannelKind::BoneRot(_, a) => a,
    };
    let base = [AXIS_X(), AXIS_Y(), AXIS_Z()][axis.min(2)];
    match ch.kind {
        ChannelKind::Pos(_) => base,
        ChannelKind::Rot(_) => mix(base, Color32::WHITE, 0.35),
        ChannelKind::Scale(_) => mix(base, Color32::BLACK, 0.3),
        ChannelKind::CamPos(_) => mix(base, Color32::from_rgb(255, 210, 80), 0.5),
        ChannelKind::BoneRot(b, _) => mix(base, [Color32::from_rgb(200, 120, 255), Color32::from_rgb(255, 160, 80), Color32::from_rgb(80, 220, 220)][(b + i) % 3], 0.4),
    }
}

/// A small cubic-Bézier editor with two draggable handles. Returns true when the ease changed.
fn ease_editor(ui: &mut Ui, e: &mut Ease, size: f32) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(size + 40.0, size + 40.0), Sense::hover());
    let box_ = Rect::from_center_size(rect.center(), vec2(size, size));
    let p = ui.painter();
    p.rect_filled(rect, compat::cr(2.0), BG0());
    compat::stroke_rect(p, box_, 0.0, compat::stroke(1.0, LINE()));
    let to = |x: f32, y: f32| pos2(box_.min.x + x * size, box_.max.y - y * size);
    let from = |q: Pos2| ((q.x - box_.min.x) / size, (box_.max.y - q.y) / size);
    let pts: Vec<Pos2> = (0..=40).map(|i| {
        let t = i as f32 / 40.0;
        to(t, e.eval(t))
    }).collect();
    p.add(Shape::line(pts, compat::stroke(2.0, accent())));
    let mut changed = false;
    for k in 0..2 {
        let (hx, hy) = if k == 0 { (e.x1, e.y1) } else { (e.x2, e.y2) };
        let anchor = if k == 0 { to(0.0, 0.0) } else { to(1.0, 1.0) };
        let q = to(hx, hy);
        let id = ui.id().with(("ease_h", k));
        let r = ui.interact(Rect::from_center_size(q, vec2(14.0, 14.0)), id, Sense::drag());
        let p = ui.painter();
        p.line_segment([anchor, q], compat::stroke(1.0, DIM()));
        p.circle_filled(q, if r.hovered() || r.dragged() { 6.0 } else { 4.5 }, if r.dragged() { Color32::WHITE } else { TEXT() });
        if r.dragged() {
            if let Some(m) = r.interact_pointer_pos() {
                let (x, y) = from(m);
                let n = Ease { x1: if k == 0 { x } else { e.x1 }, y1: if k == 0 { y } else { e.y1 }, x2: if k == 1 { x } else { e.x2 }, y2: if k == 1 { y } else { e.y2 } }.clamped();
                if n != *e {
                    *e = n;
                    changed = true;
                }
            }
        }
    }
    changed
}

/// Lines of the "What's new" window: icon, title, text, command to try it.
const WHATS_NEW: &[(Icon, &str, &str, Option<CommandId>)] = &[
    (Icon::Vertex, "Edit mode for meshes (Tab)", "Edit vertices, edges and polygons like in Blender: 1/2/3 switch the select mode; G, R, S move, rotate and scale with X/Y/Z locks and typed values; E extrude, I inset, Ctrl+B bevel, Ctrl+R loop cut and slide, K knife, M merge, F fill, J connect, X delete, Ctrl+X dissolve, P separate, Shift+D duplicate, U unwrap. Box select, Alt+click loops, Ctrl+click paths, L linked, H hide, proportional editing (O), mirror X and X-ray (Alt+Z). Quads and n-gons are kept.", Some(CommandId::EditModeToggle)),
    (Icon::Boolean, "Boolean operations", "Union, difference and intersection between two objects (Model mode → Boolean) or between the selected polygons and the rest of a mesh in Edit mode.", None),
    (Icon::Palette, "Indexed colour and projection painting", "Lock a texture to a palette (Texture mode → Indexed colour): every pixel stores a palette index and recolouring a palette entry repaints the whole texture, like PlayStation CLUTs. Projection painting paints through the view, across UV seams and several objects at once.", Some(CommandId::ToggleProjectionPaint)),
    (Icon::Ik, "Inverse kinematics", "In Pose mode, turn on IK (Shift+I) and drag a hand or a foot: the bones above it bend to follow.", Some(CommandId::ToggleIk)),
    (Icon::Graph, "Graph editor", "The timeline has a Curves view: every animated value as a curve, keys you can drag, and Bézier easing (in, out, in-out, back) for each in-between.", None),
    (Icon::Reference, "Reference pictures in 3D", "Place front, side or top drawings in the scene to model from. They show only in the editing view and are never rendered.", Some(CommandId::AddReference)),
    (Icon::Radial, "Radial menu (Space)", "Hold Space over the view, point at a tool and release, as in Krita. It changes with the context: mesh tools in Edit mode, brushes and recent colours while painting. Play/pause moved to Shift+Space.", None),
    (Icon::Gradient, "Better painting", "New tools: smudge, blur, dodge, burn and gradient; a second colour with X to swap; [ and ] change the brush size; symmetric painting; a stroke stabilizer for smooth lines.", None),
    (Icon::Eye, "Hover highlight", "The object under the mouse is outlined, so you see what a click will select. It can be turned off in Settings → Viewport.", None),
    (Icon::Sliders, "Themes", "Six built-in themes, including a light one. Themes are text files: save a copy, edit the colours and reload (Settings → Appearance).", Some(CommandId::OpenSettings)),
];
