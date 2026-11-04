//! Drawing studio panels: colour wheel, palettes, pixel selection and layers.
//! The logic lives in `psxcore` (editor, layers, palette, selection); this file only draws the controls.
use crate::colorpick;
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{dim, section, slider_f, swatch, toggle, Btn};
use crate::{dialogs, App};
use eframe::egui::{self, vec2, Color32, TextureHandle, Ui};
use psxcore::editor::{Canvas, Mode};
use psxcore::palette::{self, Palette};
use psxcore::raster::Renderer;
use psxcore::selection::Combine;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelTool {
    Rect,
    Wand,
    /// A whole UV island of the model (texture canvases only).
    Island,
}

pub struct PaintUi {
    pub hsv: [f32; 3],
    hsv_rgb: [u8; 3],
    pub hex: String,
    pub palettes: Vec<Palette>,
    pub builtin_count: usize,
    pub pal_idx: usize,
    pub sel_tool: Option<SelTool>,
    pub sel_start: Option<(i32, i32)>,
    pub sel_cur: (i32, i32),
    pub wand_tol: f32,
    pub wand_contiguous: bool,
    /// Texture mode edits a picture layer of the scene instead of the model texture.
    pub scene_target: bool,
    /// Draw tool of Animate mode paints the frame's own drawing instead of a scene layer.
    pub hand_target: bool,
    pub ref_on: bool,
    pub ref_rend: Option<Renderer>,
    pub ref_tex: Option<TextureHandle>,
    pub ref_key: u64,
    pub tex_hash: u64,
    pub sel_key: (u64, usize, (u32, u32, u32, u32)),
    pub sel_segs: Vec<(u32, u32, u32, u32)>,
}

impl Default for PaintUi {
    fn default() -> Self {
        let mut palettes = palette::builtin();
        let builtin_count = palettes.len();
        palettes.extend(load_user_palettes());
        PaintUi {
            hsv: [0.0, 0.0, 1.0],
            hsv_rgb: [255, 255, 255],
            hex: "#ffffff".into(),
            palettes,
            builtin_count,
            pal_idx: 0,
            sel_tool: None,
            sel_start: None,
            sel_cur: (0, 0),
            wand_tol: 24.0,
            wand_contiguous: true,
            scene_target: false,
            hand_target: false,
            ref_on: true,
            ref_rend: None,
            ref_tex: None,
            ref_key: 0,
            tex_hash: 0,
            sel_key: (0, 0, (0, 0, 0, 0)),
            sel_segs: Vec::new(),
        }
    }
}

fn palette_dir() -> Option<std::path::PathBuf> {
    psxcore::settings::config_dir().map(|d| d.join("palettes"))
}
fn load_user_palettes() -> Vec<Palette> {
    let Some(dir) = palette_dir() else { return vec![] };
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jankpal")).collect();
        files.sort();
        for f in files {
            let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("Palette").to_string();
            if let Ok(t) = std::fs::read_to_string(&f) {
                if let Ok(p) = Palette::from_text(&t, &stem) {
                    out.push(p);
                }
            }
        }
    }
    out
}
fn file_name_of(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{}.jankpal", s.trim())
}

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

impl App {
    /// The canvas the 2D editor and the pixel tools work on right now.
    pub fn current_canvas(&self) -> Option<Canvas> {
        match self.ed.mode {
            Mode::Animate if self.anim_tool == crate::AnimTool::Hand => {
                if self.paint.hand_target || self.ed.scene_canvas().is_none() {
                    self.ed.hand_canvas()
                } else {
                    self.ed.scene_canvas()
                }
            }
            Mode::Texture => {
                if self.paint.scene_target || self.ed.active_canvas().is_none() {
                    self.ed.scene_canvas()
                } else {
                    self.ed.active_canvas()
                }
            }
            _ => self.ed.active_canvas(),
        }
    }

    // ------------------------------------------------------------------ colour
    pub fn colour_section(&mut self, ui: &mut Ui, accent: Color32) {
        section(ui, "Colour", accent, |ui| {
            let b = self.ed.brush.color;
            let rgb = [b[0], b[1], b[2]];
            if rgb != self.paint.hsv_rgb {
                // changed from outside (eyedropper, palette): follow it, but keep hue when the colour has none
                let h = colorpick::rgb_to_hsv(rgb);
                let keep_h = h[1] < 0.02 || h[2] < 0.02;
                self.paint.hsv = [if keep_h { self.paint.hsv[0] } else { h[0] }, h[1], h[2]];
                self.paint.hsv_rgb = rgb;
                self.paint.hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
            }
            let size = ui.available_width().clamp(120.0, 150.0);
            ui.vertical_centered(|ui| {
                if colorpick::wheel(ui, &mut self.paint.hsv, size) {
                    let c = colorpick::hsv_to_rgb(self.paint.hsv[0], self.paint.hsv[1], self.paint.hsv[2]);
                    self.ed.brush.color = [c[0], c[1], c[2], self.ed.brush.color[3]];
                    self.paint.hsv_rgb = c;
                    self.paint.hex = format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
                }
            });
            ui.horizontal(|ui| {
                let (sw, _) = ui.allocate_exact_size(vec2(26.0, 26.0), egui::Sense::hover());
                ui.painter().rect_filled(sw, crate::compat::cr(2.0), c32(self.ed.brush.color));
                crate::compat::stroke_rect(ui.painter(), sw, 2.0, crate::compat::stroke(1.0, LINE));
                let r = ui.add(egui::TextEdit::singleline(&mut self.paint.hex).desired_width(78.0).font(egui::FontId::monospace(13.0)));
                if r.changed() {
                    if let Some(h) = self.paint.hex.trim().strip_prefix('#').filter(|h| h.len() == 6) {
                        if let Ok(v) = u32::from_str_radix(h, 16) {
                            self.ed.brush.color = [(v >> 16) as u8, (v >> 8) as u8, v as u8, self.ed.brush.color[3]];
                        }
                    }
                }
                crate::widgets::with_tip(r, "Type a colour like #aabbcc");
            });
            let mut a = self.ed.brush.color[3] as f32 / 255.0 * 100.0;
            if slider_f(ui, "opacity", &mut a, 0.0..=100.0) {
                self.ed.brush.color[3] = (a / 100.0 * 255.0).round() as u8;
            }
            if !self.recent.is_empty() {
                dim(ui, "Recent");
                ui.horizontal_wrapped(|ui| {
                    for c in self.recent.clone() {
                        if swatch(ui, c32(c), self.ed.brush.color == c, 20.0).clicked() {
                            self.ed.brush.color = c;
                        }
                    }
                });
            }
        });
    }

    // ------------------------------------------------------------------ palettes
    fn save_palette(&mut self, i: usize) {
        if i < self.paint.builtin_count {
            return;
        }
        let Some(p) = self.paint.palettes.get(i) else { return };
        if let Some(dir) = palette_dir() {
            if std::fs::create_dir_all(&dir).is_ok() {
                if let Err(e) = std::fs::write(dir.join(file_name_of(&p.name)), p.to_text()) {
                    let msg = format!("Could not save the palette: {e}");
                    self.error(msg);
                }
            }
        }
    }
    pub fn palette_section(&mut self, ui: &mut Ui, accent: Color32) {
        section(ui, "Palette", accent, |ui| {
            ui.horizontal_wrapped(|ui| {
                for i in 0..self.paint.palettes.len() {
                    let name = self.paint.palettes[i].name.clone();
                    let w = 18.0 + name.chars().count().min(14) as f32 * 7.2;
                    if Btn::new(&name).accent(accent).on(self.paint.pal_idx == i).size(vec2(w, 26.0)).tip("Choose a palette").show(ui).clicked() {
                        self.paint.pal_idx = i;
                    }
                }
            });
            let idx = self.paint.pal_idx.min(self.paint.palettes.len().saturating_sub(1));
            let user = idx >= self.paint.builtin_count;
            let colors = self.paint.palettes.get(idx).map(|p| p.colors.clone()).unwrap_or_default();
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (k, c) in colors.iter().enumerate() {
                    let r = swatch(ui, c32(*c), self.ed.brush.color[..3] == c[..3], 22.0);
                    let r = crate::widgets::with_tip(r, if user { "Click to use. Right-click to remove" } else { "Click to use" });
                    if r.clicked() {
                        self.ed.brush.color = [c[0], c[1], c[2], self.ed.brush.color[3]];
                    }
                    if user && r.secondary_clicked() {
                        remove = Some(k);
                    }
                }
            });
            if let Some(k) = remove {
                if let Some(pal) = self.paint.palettes.get_mut(idx) {
                    pal.remove(k);
                }
                self.save_palette(idx);
            }
            let mut deleted = false;
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Add colour").icon(Icon::Plus).accent(accent).tip("Adds the current colour to the palette (a built-in palette is copied first)").show(ui).clicked() {
                    let mut at = idx;
                    if !user {
                        let mut p = self.paint.palettes[idx].clone();
                        p.name = format!("{} copy", p.name);
                        self.paint.palettes.push(p);
                        at = self.paint.palettes.len() - 1;
                        self.paint.pal_idx = at;
                    }
                    let c = self.ed.brush.color;
                    self.paint.palettes[at].add(c);
                    self.save_palette(at);
                }
                if Btn::new("From picture").tip("Makes a new palette from the colours of the canvas").show(ui).clicked() {
                    if let Some(c) = self.current_canvas() {
                        if let Some(t) = self.ed.canvas_texture(c) {
                            let n = self.paint.palettes.len() + 1;
                            let p = Palette::from_texture(&t, &format!("Palette {n}"), 32);
                            drop(t);
                            self.paint.palettes.push(p);
                            self.paint.pal_idx = self.paint.palettes.len() - 1;
                            let at = self.paint.pal_idx;
                            self.save_palette(at);
                        }
                    }
                }
                if Btn::new("New").tip("An empty palette of your own").show(ui).clicked() {
                    let n = self.paint.palettes.len() + 1;
                    self.paint.palettes.push(Palette::new(&format!("Palette {n}"), vec![]));
                    self.paint.pal_idx = self.paint.palettes.len() - 1;
                    let at = self.paint.pal_idx;
                    self.save_palette(at);
                }
                if user && Btn::new("Delete").icon(Icon::Trash).accent(DANGER).tip("Deletes this palette").show(ui).clicked() {
                    let p = self.paint.palettes.remove(idx);
                    if let Some(dir) = palette_dir() {
                        let _ = std::fs::remove_file(dir.join(file_name_of(&p.name)));
                    }
                    self.paint.pal_idx = idx.saturating_sub(1);
                    deleted = true;
                }
            });
            // `idx` points at a palette that no longer exists after a delete: the name field waits for the next frame
            if user && !deleted && idx < self.paint.palettes.len() {
                let mut name = self.paint.palettes[idx].name.clone();
                if ui.add(egui::TextEdit::singleline(&mut name).hint_text("palette name").desired_width(ui.available_width())).changed() {
                    let old = self.paint.palettes[idx].name.clone();
                    if let Some(dir) = palette_dir() {
                        let _ = std::fs::remove_file(dir.join(file_name_of(&old)));
                    }
                    self.paint.palettes[idx].name = name;
                    self.save_palette(idx);
                }
            }
        });
    }

    // ------------------------------------------------------------------ selection
    pub fn selection_section(&mut self, ui: &mut Ui, accent: Color32) {
        let Some(canvas) = self.current_canvas() else { return };
        let strip = self.settings.show_toolbar;
        section(ui, "Selection", accent, |ui| {
            if !strip {
            ui.horizontal_wrapped(|ui| {
                let can_island = matches!(canvas, Canvas::Base { .. } | Canvas::Patch { .. } | Canvas::Emotion { .. });
                for (t, icon, tip, ok) in [
                    (SelTool::Rect, Icon::Marquee, "Rectangle select: drag. Shift adds, Ctrl subtracts. A plain click clears", true),
                    (SelTool::Wand, Icon::Wand, "Magic wand: selects the pixels of a similar colour", true),
                    (SelTool::Island, Icon::Island, "UV island: selects the whole island of the model under the cursor", can_island),
                ] {
                    if Btn::new("").icon(icon).accent(accent).on(self.paint.sel_tool == Some(t)).enabled(ok).tip(tip).show(ui).clicked() {
                        self.paint.sel_tool = if self.paint.sel_tool == Some(t) { None } else { Some(t) };
                    }
                }
            });
            }
            if self.paint.sel_tool == Some(SelTool::Wand) {
                slider_f(ui, "tolerance", &mut self.paint.wand_tol, 0.0..=128.0);
                toggle(ui, &mut self.paint.wand_contiguous, "Only touching pixels", accent);
            }
            let n = self.ed.selection_for_public(canvas).map_or(0, |s| s.count());
            dim(ui, &if n > 0 { format!("{n} pixels selected") } else { "Nothing selected: tools act on the whole picture".to_string() });
            ui.horizontal_wrapped(|ui| {
                if Btn::new("All").tip("Select everything (Ctrl+A)").show(ui).clicked() {
                    self.ed.select_all(canvas);
                }
                if Btn::new("None").enabled(n > 0).tip("Clear the selection (Ctrl+D)").show(ui).clicked() {
                    self.ed.select_none();
                }
                if Btn::new("Invert").tip("Swap selected and unselected pixels (Ctrl+I)").show(ui).clicked() {
                    self.ed.select_invert(canvas);
                }
            });
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Copy").icon(Icon::Copy).enabled(n > 0).tip("Copy the selected pixels").show(ui).clicked() {
                    self.ed.selection_copy(canvas);
                }
                if Btn::new("Paste").enabled(self.ed.pixel_clip.is_some()).tip("Paste at the top-left of the selection, or the picture corner").show(ui).clicked() {
                    let (x, y) = self.ed.selection_for_public(canvas).and_then(|s| s.bounds()).map_or((0, 0), |b| (b.0 as i32, b.1 as i32));
                    self.ed.selection_paste(canvas, x, y);
                }
                if Btn::new("Clear").icon(Icon::Trash).accent(DANGER).enabled(n > 0).tip("Make the selected pixels transparent (Delete)").show(ui).clicked() {
                    self.ed.selection_delete(canvas);
                }
            });
        });
    }

    /// Mouse handling of the selection tools on the 2D canvas. `p` is the pointer in canvas pixels.
    pub fn selection_pointer(&mut self, canvas: Canvas, tool: SelTool, p: Option<(f32, f32)>, pressed: bool, down: bool, shift: bool, ctrl: bool) {
        let how = if shift { Combine::Add } else if ctrl { Combine::Subtract } else { Combine::Replace };
        let px = p.map(|(x, y)| (x.floor() as i32, y.floor() as i32));
        if pressed {
            if let Some(q) = px {
                match tool {
                    SelTool::Rect => {
                        self.paint.sel_start = Some(q);
                        self.paint.sel_cur = q;
                    }
                    SelTool::Wand => self.ed.select_wand(canvas, q.0, q.1, self.paint.wand_tol as u8, self.paint.wand_contiguous, how),
                    SelTool::Island => {
                        if let (Some((w, h)), Some(model)) = (self.ed.canvas_texture(canvas).map(|t| (t.w, t.h)), self.ed.selected_model()) {
                            let (u, v) = ((q.0 as f32 + 0.5) / w as f32, (q.1 as f32 + 0.5) / h as f32);
                            let mut found = None;
                            for (mi, mesh) in model.meshes.iter().enumerate() {
                                let isl = psxcore::uvisland::islands(mesh);
                                if let Some(ii) = psxcore::uvisland::island_at(mesh, &isl, u, v) {
                                    found = Some((mi, ii));
                                    break;
                                }
                            }
                            match found {
                                Some((mi, ii)) => {
                                    self.ed.select_uv_island(canvas, mi, ii, how);
                                }
                                None => self.info("No UV island under the cursor"),
                            }
                        }
                    }
                }
            }
        }
        if let (SelTool::Rect, Some(start)) = (tool, self.paint.sel_start) {
            if let Some(q) = px {
                self.paint.sel_cur = q;
            }
            if !down {
                let end = self.paint.sel_cur;
                self.paint.sel_start = None;
                if end == start {
                    if how == Combine::Replace {
                        self.ed.select_none();
                    }
                } else {
                    self.ed.select_rect(canvas, start, end, how);
                }
            }
        }
    }

    // ------------------------------------------------------------------ layers
    pub fn layers_section(&mut self, ui: &mut Ui, accent: Color32) {
        let scene_tool = self.ed.mode == Mode::Animate && self.anim_tool == crate::AnimTool::Hand;
        if scene_tool || (self.ed.mode == Mode::Texture && (self.paint.scene_target || self.ed.active_canvas().is_none())) {
            self.scene_layers_section(ui, accent);
        } else {
            self.texture_layers_section(ui, accent);
        }
    }

    /// Switch between the model texture and the picture drawn into the scene (Texture mode).
    pub fn canvas_target_section(&mut self, ui: &mut Ui, accent: Color32) {
        section(ui, "Draw on", accent, |ui| {
            ui.horizontal(|ui| {
                let has_tex = self.ed.active_canvas().is_some();
                if Btn::new("Model texture").accent(accent).enabled(has_tex).on(has_tex && !self.paint.scene_target).tip("Paint the texture of the selected model").show(ui).clicked() {
                    self.paint.scene_target = false;
                }
                if Btn::new("Scene picture").accent(accent).on(self.paint.scene_target || !has_tex).tip("Paint pictures behind and in front of the 3D scene, or any plain image").show(ui).clicked() {
                    self.paint.scene_target = true;
                }
            });
        });
    }

    fn scene_layers_section(&mut self, ui: &mut Ui, accent: Color32) {
        section(ui, "Layers", accent, |ui| {
            let n = self.ed.project.scene.layers.len();
            let back = self.ed.project.scene.layers_back;
            let active = self.ed.active_layer.min(n.saturating_sub(1));
            if n == 0 {
                dim(ui, "No picture layers yet. A layer behind the 3D scene makes a backdrop; a layer in front makes an overlay. Without any model it is a plain image.");
            }
            let hand_row = self.ed.mode == Mode::Animate && self.anim_tool == crate::AnimTool::Hand;
            if hand_row {
                let on = self.paint.hand_target || n == 0;
                if Btn::new("This frame's drawing").icon(Icon::Hand).accent(accent).on(on).size(vec2(ui.available_width(), 28.0)).tip("A drawing that belongs to the current frame only, always in front").show(ui).clicked() {
                    self.paint.hand_target = true;
                }
            }
            // top to bottom: layers in front, the 3D scene, layers behind
            let rows: Vec<Option<usize>> = (back..n).rev().map(Some).chain(std::iter::once(None)).chain((0..back).rev().map(Some)).collect();
            for row in rows {
                match row {
                    None => {
                        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, crate::compat::cr(2.0), BG0);
                        crate::icons::draw(ui.painter(), Icon::Cube, rect.min + vec2(16.0, 12.0), 15.0, DIM);
                        crate::compat::text(ui.painter(), rect.min + vec2(32.0, 12.0), egui::Align2::LEFT_CENTER, "3D scene", 12.5, DIM);
                    }
                    Some(i) => {
                        let (name, vis, locked) = {
                            let l = &self.ed.project.scene.layers[i];
                            (l.name.clone(), l.visible, l.locked)
                        };
                        ui.horizontal(|ui| {
                            if Btn::new("").icon(if vis { Icon::Eye } else { Icon::EyeOff }).size(vec2(28.0, 26.0)).tip("Show or hide the layer").show(ui).clicked() {
                                self.ed.edit_scene_layer(i, |l| l.visible = !vis);
                            }
                            if Btn::new("").icon(Icon::Lock).on(locked).accent(accent).size(vec2(28.0, 26.0)).tip("Lock the layer so it cannot be painted on").show(ui).clicked() {
                                self.ed.edit_scene_layer(i, |l| l.locked = !locked);
                            }
                            let sel = i == active && !(hand_row && self.paint.hand_target);
                            let w = ui.available_width().max(40.0);
                            if Btn::new(&name).accent(accent).on(sel).size(vec2(w, 26.0)).tip("Choose this layer to paint on").show(ui).clicked() {
                                self.ed.select_scene_layer(i);
                                self.paint.hand_target = false;
                            }
                        });
                    }
                }
            }
            if n > 0 {
                let l = &self.ed.project.scene.layers[active];
                let mut name = l.name.clone();
                let mut op = l.opacity as f32 / 255.0 * 100.0;
                if ui.add(egui::TextEdit::singleline(&mut name).desired_width(ui.available_width())).changed() {
                    self.cont(|ed| {
                        ed.edit_scene_layer(active, |l| l.name = name);
                    });
                }
                if slider_f(ui, "layer opacity", &mut op, 0.0..=100.0) {
                    self.cont(|ed| {
                        ed.edit_scene_layer(active, |l| l.opacity = (op / 100.0 * 255.0).round() as u8);
                    });
                }
            }
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Behind").icon(Icon::Plus).accent(accent).tip("New layer behind the 3D scene (a backdrop)").show(ui).clicked() {
                    self.ed.add_scene_layer(true);
                    self.paint.hand_target = false;
                }
                if Btn::new("In front").icon(Icon::Plus).accent(accent).tip("New layer in front of the 3D scene (an overlay)").show(ui).clicked() {
                    self.ed.add_scene_layer(false);
                    self.paint.hand_target = false;
                }
                if Btn::new("Image").icon(Icon::Image).tip("Load a picture into a new layer behind the scene").show(ui).clicked() {
                    self.dialog = Some(dialogs::browser(dialogs::Purpose::ImportSceneLayer(true)));
                }
            });
            if n > 0 {
                ui.horizontal_wrapped(|ui| {
                    if Btn::new("Up").tip("Move the layer up. Passing the 3D scene moves it in front").show(ui).clicked() {
                        self.ed.move_scene_layer(active, true);
                    }
                    if Btn::new("Down").tip("Move the layer down. Passing the 3D scene moves it behind").show(ui).clicked() {
                        self.ed.move_scene_layer(active, false);
                    }
                    if Btn::new("").icon(Icon::Copy).tip("Duplicate the layer").show(ui).clicked() {
                        self.ed.duplicate_scene_layer(active);
                    }
                    if Btn::new("").icon(Icon::Trash).accent(DANGER).tip("Delete the layer").show(ui).clicked() {
                        self.ed.delete_scene_layer(active);
                    }
                });
            }
            toggle(ui, &mut self.paint.ref_on, "3D reference under picture", accent);
        });
    }

    fn texture_layers_section(&mut self, ui: &mut Ui, accent: Color32) {
        if self.ed.active_canvas().is_none() {
            return;
        }
        section(ui, "Layers", accent, |ui| {
            let Some(stack) = self.ed.texture_stack().cloned() else {
                dim(ui, "This texture is a single picture. Layers let you paint details, shadows or decals separately and hide or reorder them.");
                if Btn::new("Use layers").icon(Icon::Plus).accent(accent).size(vec2(ui.available_width(), 30.0)).tip("The current picture becomes the first layer").show(ui).clicked() {
                    self.ed.enable_texture_layers();
                }
                return;
            };
            let n = stack.layers.len();
            let active = stack.active.min(n - 1);
            for i in (0..n).rev() {
                let l = &stack.layers[i];
                let (vis, locked, name) = (l.visible, l.locked, l.name.clone());
                ui.horizontal(|ui| {
                    if Btn::new("").icon(if vis { Icon::Eye } else { Icon::EyeOff }).size(vec2(28.0, 26.0)).tip("Show or hide the layer").show(ui).clicked() {
                        self.ed.edit_texture_stack(|s| s.layers[i].visible = !vis);
                    }
                    if Btn::new("").icon(Icon::Lock).on(locked).accent(accent).size(vec2(28.0, 26.0)).tip("Lock the layer so it cannot be painted on").show(ui).clicked() {
                        self.ed.edit_texture_stack(|s| s.layers[i].locked = !locked);
                    }
                    let w = ui.available_width().max(40.0);
                    if Btn::new(&name).accent(accent).on(i == active).size(vec2(w, 26.0)).tip("Choose this layer to paint on").show(ui).clicked() {
                        self.ed.edit_texture_stack(|s| s.active = i);
                    }
                });
            }
            let l = &stack.layers[active];
            let mut name = l.name.clone();
            let mut op = l.opacity as f32 / 255.0 * 100.0;
            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(ui.available_width())).changed() {
                self.cont(|ed| {
                    ed.edit_texture_stack(|s| s.layers[active].name = name);
                });
            }
            if slider_f(ui, "layer opacity", &mut op, 0.0..=100.0) {
                self.cont(|ed| {
                    ed.edit_texture_stack(|s| s.layers[active].opacity = (op / 100.0 * 255.0).round() as u8);
                });
            }
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Add").icon(Icon::Plus).accent(accent).tip("New empty layer above the current one").show(ui).clicked() {
                    self.ed.edit_texture_stack(|s| {
                        let k = s.layers.len() + 1;
                        s.add(&format!("Layer {k}"));
                    });
                }
                if Btn::new("").icon(Icon::Copy).tip("Duplicate the layer").show(ui).clicked() {
                    self.ed.edit_texture_stack(|s| {
                        s.duplicate(active);
                    });
                }
                if Btn::new("").icon(Icon::Trash).accent(DANGER).enabled(n > 1).tip("Delete the layer").show(ui).clicked() {
                    self.ed.edit_texture_stack(|s| {
                        s.remove(active);
                    });
                }
            });
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Up").enabled(active + 1 < n).show(ui).clicked() {
                    self.ed.edit_texture_stack(|s| {
                        s.move_up(active);
                    });
                }
                if Btn::new("Down").enabled(active > 0).show(ui).clicked() {
                    self.ed.edit_texture_stack(|s| {
                        s.move_down(active);
                    });
                }
                if Btn::new("Merge down").enabled(active > 0).tip("Blend this layer into the one below").show(ui).clicked() {
                    self.ed.edit_texture_stack(|s| {
                        s.merge_down(active);
                    });
                }
                if Btn::new("Flatten").tip("Merge everything into one picture and stop using layers").show(ui).clicked() {
                    self.ed.disable_texture_layers();
                }
            });
        });
    }
}
