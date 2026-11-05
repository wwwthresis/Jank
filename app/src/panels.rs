//! Side panels: tools of the current mode (left) and the scene / output settings (right).
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{dim, drag3, section, slider_f, slider_f_tip, slider_u, toggle, toggle_tip, Btn};
use crate::{compat, dialogs, App, AnimTool, ModelTool};
use eframe::egui::{self, vec2, Color32, Context, ScrollArea, SidePanel, Ui};
use psxcore::editor::{Mode, PaintLayer, Primitive};
use psxcore::math::Vec3;
use psxcore::paint::Tool;
use psxcore::raster::PsxStyle;
use psxcore::scene::Transform;

fn to_c32(c: [f32; 3]) -> Color32 {
    let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(f(c[0]), f(c[1]), f(c[2]))
}
fn from_c32(c: Color32) -> [f32; 3] {
    [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0]
}
/// Colour editor for an [r, g, b] triple in 0..1 with a caption. Returns true if the colour changed.
pub(crate) fn rgb_edit(ui: &mut Ui, label: &str, c: &mut [f32; 3]) -> bool {
    ui.horizontal(|ui| {
        let mut col = to_c32(*c);
        let changed = ui.color_edit_button_srgba(&mut col).changed();
        if changed {
            *c = from_c32(col);
        }
        dim(ui, label);
        changed
    })
    .inner
}

/// Which part of the render style a tab edits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RightTab {
    /// Objects and their placement.
    Scene,
    /// Resolution, retro rendering and mood.
    Look,
    /// Sky, fog and light.
    World,
    /// Export.
    Output,
}

/// Render resolution and the retro look of the final picture.
pub fn look_editor(ui: &mut Ui, s: &mut PsxStyle) -> bool {
    let mut ch = false;
    section(ui, "Style presets", accent(), |ui| {
        dim(ui, "A starting point for the whole look. The render resolution stays as it is.");
        ui.horizontal_wrapped(|ui| {
            for p in PsxStyle::presets() {
                let tip = preset_tip(&p.name);
                if Btn::new(&p.name).accent(accent()).on(s.name == p.name).tip(tip).show(ui).clicked() {
                    let (w, h) = (s.width, s.height);
                    *s = PsxStyle { width: w, height: h, ..p };
                    ch = true;
                }
            }
        });
    });
    section(ui, "Render resolution", accent(), |ui| {
        ui.horizontal_wrapped(|ui| {
            for (w, h) in [(160u32, 120u32), (256, 224), (320, 240), (384, 216), (512, 384), (640, 480), (1280, 720)] {
                let l = format!("{w}×{h}");
                if Btn::new(&l).accent(accent()).on(s.width == w && s.height == h).size(vec2(24.0 + l.chars().count() as f32 * 7.2, 28.0)).show(ui).clicked() {
                    s.width = w;
                    s.height = h;
                    ch = true;
                }
            }
        });
        ch |= slider_u(ui, "width", &mut s.width, 64..=1920);
        ch |= slider_u(ui, "height", &mut s.height, 48..=1080);
    });
    section(ui, "Retro rendering", accent(), |ui| {
        ch |= toggle(ui, &mut s.affine, "Affine textures (wobble)", accent()).clicked();
        ch |= toggle(ui, &mut s.dither, "Dithering", accent()).clicked();
        ch |= toggle(ui, &mut s.z_buffer, "Z-buffer (off = PS1 painter's order)", accent()).clicked();
        ch |= slider_f(ui, "vertex snap", &mut s.snap, 0.0..=4.0);
        ch |= slider_u(ui, "colour bits", &mut s.color_bits, 1..=8);
    });
    section(ui, "Mood", accent(), |ui| {
        ch |= rgb_edit(ui, "tint", &mut s.tint);
        ch |= slider_f(ui, "saturation", &mut s.saturation, 0.0..=2.0);
        ch |= slider_f(ui, "contrast", &mut s.contrast, 0.3..=2.0);
        ch |= slider_f(ui, "brightness", &mut s.brightness, 0.3..=2.0);
        ch |= slider_f(ui, "vignette", &mut s.vignette, 0.0..=1.0);
        ch |= slider_f(ui, "scanlines", &mut s.scanlines, 0.0..=1.0);
        ch |= slider_f(ui, "film grain", &mut s.grain, 0.0..=1.0);
    });
    ch |= modern_effects(ui, s);
    s.width = s.width.clamp(16, 4096);
    s.height = s.height.clamp(16, 4096);
    ch
}

fn preset_tip(name: &str) -> &'static str {
    match name {
        "PS1 Classic" => "Wobbly affine textures, vertex snapping, dithering and 15-bit colour",
        "Lo-Fi Calm" => "Warm haze, a soft evening sky and film grain",
        "Clean Low-Poly" => "Sharp and clean: no wobble, no dithering, full colour",
        "Horror Fog" => "Dense dark fog, cold colours and heavy grain",
        "Night Glow" => "Night scene for lamps: pixel lamps, soft tone curve, bloom and contact shadows",
        "CRT Arcade" => "An old arcade screen: curved glass, RGB mask, colour fringes",
        _ => "Apply this style",
    }
}

/// Modern touches on top of the retro picture: lamps per pixel, a tone curve, glow, occlusion and a CRT screen.
fn modern_effects(ui: &mut Ui, s: &mut PsxStyle) -> bool {
    use psxcore::raster::Tonemap;
    let mut ch = false;
    section(ui, "Modern effects", accent(), |ui| {
        ch |= toggle_tip(ui, &mut s.pixel_lights, "Pixel lamps", accent(), "Lamps are worked out for every pixel: round, smooth pools of light even on big faces. Off: per vertex, the PS1 way").clicked();
        ui.horizontal(|ui| {
            dim(ui, "tone curve");
            for t in [Tonemap::Off, Tonemap::Soft, Tonemap::Filmic] {
                let tip = match t {
                    Tonemap::Off => "Bright light is clipped to white",
                    Tonemap::Soft => "Very bright light rolls off softly; dark and mid tones stay as they are",
                    _ => "A film-like curve: rich contrast, highlights never burn out",
                };
                if Btn::new(t.label()).accent(accent()).on(s.tonemap == t).tip(tip).show(ui).clicked() {
                    s.tonemap = t;
                    ch = true;
                }
            }
        });
        if s.tonemap != Tonemap::Off {
            ch |= slider_f_tip(ui, "exposure", &mut s.exposure, 0.2..=4.0, "Overall brightness before the tone curve");
        }
        ch |= slider_f_tip(ui, "bloom", &mut s.bloom, 0.0..=1.5, "Glow around bright parts of the picture");
        if s.bloom > 0.0 {
            ch |= slider_f_tip(ui, "bloom threshold", &mut s.bloom_threshold, 0.0..=0.98, "How bright a pixel must be to glow");
            ch |= slider_f_tip(ui, "bloom size", &mut s.bloom_radius, 0.0..=1.0, "How far the glow spreads");
        }
        ch |= slider_f_tip(ui, "contact shadows", &mut s.ao, 0.0..=1.0, "Ambient occlusion: soft darkening in corners, creases and where objects touch");
        if s.ao > 0.0 {
            ch |= slider_f_tip(ui, "shadow reach", &mut s.ao_radius, 0.05..=3.0, "Size of the contact shadows in metres");
        }
        ch |= slider_f_tip(ui, "lamp halos", &mut s.halo, 0.0..=2.0, "Soft glow drawn around visible lamps (each lamp has its own halo amount)");
        ch |= slider_f_tip(ui, "CRT curve", &mut s.crt_curve, 0.0..=1.0, "Bulging glass of an old screen");
        ch |= slider_f_tip(ui, "CRT mask", &mut s.crt_mask, 0.0..=1.0, "The RGB stripes of a shadow mask");
        ch |= slider_f_tip(ui, "colour fringes", &mut s.chroma, 0.0..=1.0, "Red and blue pulled apart towards the edges");
        if s.modern() {
            dim(ui, "The viewport can draw these cheaper: Settings → Viewport → Viewport effects.");
        }
    });
    ch
}

/// Time of day (sun and moon) and shadows.
pub fn lighting_editor(ui: &mut Ui, l: &mut psxcore::lighting::Lighting) -> bool {
    use psxcore::lighting::{daylight, ShadowMode, ShadowQuality};
    let mut ch = false;
    section(ui, "Time of day", accent(), |ui| {
        ch |= toggle_tip(ui, &mut l.tod_on, "Sun and moon", accent(), "Light direction and colour, ambient light, sky and fog follow the hour. The sun crosses the sky from 06:00 to 18:00, the moon from 18:00 to 06:00").clicked();
        if l.tod_on {
            let hh = l.time.floor() as u32 % 24;
            let mm = ((l.time - l.time.floor()) * 60.0) as u32 % 60;
            let d = daylight(l.time, l.azimuth);
            dim(ui, &format!("{hh:02}:{mm:02}   {} at {:.0} degrees", if d.sun { "sun" } else { "moon" }, d.elevation_deg));
            ch |= slider_f_tip(ui, "hour", &mut l.time, 0.0..=23.99, "Time of day in hours: 6 is sunrise, 12 noon, 18 sunset, 0 midnight");
            ch |= slider_f_tip(ui, "direction", &mut l.azimuth, 0.0..=360.0, "Compass direction the sun and moon travel along, in degrees");
        }
    });
    section(ui, "Shadows", accent(), |ui| {
        ui.horizontal_wrapped(|ui| {
            for m in ShadowMode::ALL {
                let tip = match m {
                    ShadowMode::Off => "No shadows. Fastest",
                    ShadowMode::Blob => "A soft round spot behind every object. Cheap and very PS1",
                    ShadowMode::Projected => "The real shape of every object, on any surface. Cut-out textures such as wings and leaves cast cut-out shadows",
                };
                if Btn::new(m.label()).accent(accent()).on(l.shadows == m).tip(tip).show(ui).clicked() {
                    l.shadows = m;
                    ch = true;
                }
            }
        });
        if l.shadows != ShadowMode::Off {
            ch |= slider_f_tip(ui, "strength", &mut l.shadow_strength, 0.0..=1.0, "How dark the shadows are");
            ch |= slider_f_tip(ui, "softness", &mut l.softness, 0.0..=1.0, "How soft the edge of a shadow is");
            if l.shadows == ShadowMode::Projected {
                ui.horizontal_wrapped(|ui| {
                    for q in ShadowQuality::ALL {
                        let tip = format!("Shadow map of {0}x{0} texels. Sharper edges cost more time and memory", q.resolution());
                        if Btn::new(q.label()).accent(accent()).on(l.quality == q).tip(&tip).show(ui).clicked() {
                            l.quality = q;
                            ch = true;
                        }
                    }
                });
                dim(ui, "Shadow quality");
            }
        }
    });
    ch
}

/// Sky, fog and light of the world.
pub fn world_editor(ui: &mut Ui, s: &mut PsxStyle) -> bool {
    let mut ch = false;
    section(ui, "Sky", accent(), |ui| {
        ch |= rgb_edit(ui, "sky top", &mut s.bg_top);
        ch |= rgb_edit(ui, "sky bottom", &mut s.bg_bottom);
    });
    section(ui, "Fog", accent(), |ui| {
        ch |= toggle(ui, &mut s.fog_on, "Fog", accent()).clicked();
        ch |= slider_f(ui, "fog start", &mut s.fog_near, 0.0..=60.0);
        ch |= slider_f(ui, "fog end", &mut s.fog_far, 1.0..=120.0);
        ch |= rgb_edit(ui, "fog colour", &mut s.fog_color);
    });
    section(ui, "Light", accent(), |ui| {
        ch |= rgb_edit(ui, "ambient light", &mut s.ambient);
        ch |= rgb_edit(ui, "light", &mut s.light_color);
    });
    ch
}

/// Brushes and colour: shared by the Texture and Animate modes.
fn paint_tools(app: &mut App, ui: &mut Ui, accent: Color32, layers: bool) {
    let strip = app.settings.show_toolbar;
    section(ui, "Brush", accent, |ui| {
        if !strip {
        ui.horizontal_wrapped(|ui| {
            for (t, icon, tip) in [
                (Tool::Brush, Icon::Brush, "Brush"),
                (Tool::Eraser, Icon::Eraser, "Eraser"),
                (Tool::Fill, Icon::Fill, "Fill"),
                (Tool::Picker, Icon::Picker, "Eyedropper"),
                (Tool::Line, Icon::Line, "Line"),
                (Tool::Rect, Icon::Rect, "Rectangle outline"),
                (Tool::RectFilled, Icon::RectFilled, "Filled rectangle"),
                (Tool::Gradient, Icon::Gradient, "Gradient: drag from the main colour to the second colour"),
                (Tool::Smudge, Icon::Smudge, "Smudge: push colours along the stroke"),
                (Tool::Blur, Icon::Blur, "Blur: soften details"),
                (Tool::Dodge, Icon::Sun, "Dodge: lighten"),
                (Tool::Burn, Icon::Sparkle, "Burn: darken"),
            ] {
                if Btn::new("").icon(icon).accent(accent).on(app.paint.sel_tool.is_none() && app.ed.brush.tool == t).tip(tip).show(ui).clicked() {
                    app.ed.brush.tool = t;
                    app.paint.sel_tool = None;
                }
            }
        });
        }
        let mut b = app.ed.brush;
        toggle(ui, &mut b.pixel, "Pixel brush", accent);
        slider_f(ui, "size", &mut b.size, 1.0..=64.0);
        if !b.pixel {
            slider_f(ui, "softness", &mut b.hardness, 0.0..=1.0);
        }
        if b.tool == Tool::Fill {
            let mut tol = b.tolerance as f32;
            slider_f(ui, "tolerance", &mut tol, 0.0..=128.0);
            b.tolerance = tol as u32;
        }
        if matches!(b.tool, Tool::Smudge | Tool::Blur | Tool::Dodge | Tool::Burn) {
            slider_f(ui, "strength", &mut b.strength, 0.05..=1.0);
        }
        if matches!(b.tool, Tool::Dodge | Tool::Burn) {
            ui.horizontal(|ui| {
                for (t, l) in [(Tool::Dodge, "Dodge (lighten)"), (Tool::Burn, "Burn (darken)")] {
                    if Btn::new(l).accent(accent).on(b.tool == t).show(ui).clicked() {
                        b.tool = t;
                    }
                }
            });
        }
        let sym_tip = app.tip_for(psxcore::commands::CommandId::ToggleSymmetry, "Every dab is mirrored left/right across the middle of the canvas");
        crate::widgets::toggle_tip(ui, &mut b.symmetry, "Symmetric painting", accent, &sym_tip);
        b.size = b.size.round().max(1.0);
        app.ed.brush = b;
        let mut st = app.settings.stabilizer;
        if crate::widgets::slider_f_tip(ui, "stabilizer", &mut st, 0.0..=1.0, "Smooths shaky strokes: the brush trails behind the mouse (0 = off)") {
            app.settings.stabilizer = st;
            app.touch_settings();
        }
        if app.ed.mode != psxcore::editor::Mode::Animate || app.anim_tool == crate::AnimTool::PaintModel {
            let mut pp = app.ed.project_paint;
            let tip = app.tip_for(psxcore::commands::CommandId::ToggleProjectionPaint, "The brush paints through the view onto whatever is under it, across UV seams and objects (brush and eraser)");
            if crate::widgets::toggle_tip(ui, &mut pp, "Projection painting in 3D", accent, &tip).changed() {
                app.ed.project_paint = pp;
            }
        }
    });
    if layers {
        app.layers_section(ui, accent);
    }
    app.colour_section(ui, accent);
    app.selection_section(ui, accent);
    app.palette_section(ui, accent);
}

/// One-line description under the mode name in the left pane header.
fn mode_hint(m: Mode) -> &'static str {
    match m {
        Mode::Model => "shape low-poly meshes",
        Mode::Texture => "paint pixels on the model",
        Mode::Pose => "bend the skeleton",
        Mode::Animate => "frames, drawing, emotions",
        Mode::Render => "camera and final picture",
    }
}

/// Icon for an object row in the scene list, guessed from its name.
fn object_icon(name: &str) -> Icon {
    let n = name.to_ascii_lowercase();
    if n.contains("butterfly") {
        Icon::Butterfly
    } else if n.contains("sphere") || n.contains("ball") {
        Icon::Sphere
    } else if n.contains("plane") || n.contains("meadow") || n.contains("ground") || n.contains("floor") {
        Icon::Plane
    } else if n.contains("cylinder") || n.contains("trunk") {
        Icon::Cylinder
    } else if n.contains("cone") || n.contains("pine") || n.contains("tree") {
        Icon::Cone
    } else if n.contains("torus") || n.contains("ring") {
        Icon::Torus
    } else {
        Icon::Cube
    }
}

/// Obsidian-style pane header: the mode icon in the accent colour, the mode name and a dim caption.
fn pane_header(ui: &mut Ui, icon: Icon, title: &str, hint: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), egui::Sense::hover());
    let p = ui.painter();
    crate::icons::draw(p, icon, egui::pos2(rect.min.x + 9.0, rect.center().y), 16.0, accent());
    let font = egui::FontId::new(13.5, egui::FontFamily::Name("strong".into()));
    let g = p.layout_no_wrap(title.to_string(), font, TEXT());
    let tw = g.size().x;
    p.galley(egui::pos2(rect.min.x + 24.0, rect.center().y - g.size().y * 0.5), g, TEXT());
    compat::text(p, egui::pos2(rect.min.x + 32.0 + tw, rect.center().y + 0.5), egui::Align2::LEFT_CENTER, hint, 11.5, DIM());
    p.line_segment([egui::pos2(rect.min.x - 10.0, rect.max.y), egui::pos2(rect.max.x + 10.0, rect.max.y)], compat::stroke(1.0, LINE_SOFT()));
}

impl App {
    pub fn left_panel(&mut self, ctx: &Context) {
        let mut panel = SidePanel::left("left").resizable(true).default_width(self.settings.layout.left_width).width_range(210.0..=380.0).frame(compat::frame(panel_fill(PANEL()), 10.0));
        if self.layout_reset {
            panel = panel.exact_width(self.settings.layout.left_width);
        }
        let shown = panel.show(ctx, |ui| {
            self.art.paint_panel(ui, crate::themeart::Slot::Panel, vec2(10.0, 10.0));
            pane_header(ui, crate::mode_icon(self.ed.mode), crate::mode_label(self.ed.mode), mode_hint(self.ed.mode));
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.ed.mode {
                Mode::Model if self.ed.edit_active() => self.edit_tools(ui, ctx),
                Mode::Model => self.model_tools(ui),
                Mode::Texture => {
                    self.canvas_target_section(ui, accent());
                    self.indexed_section(ui);
                    paint_tools(self, ui, accent(), true)
                }
                Mode::Pose => self.pose_tools(ui),
                Mode::Animate => self.anim_tools(ui),
                Mode::Render => self.render_tools(ui),
            });
        });
        self.track_width(crate::Panel::Left, shown.response.rect.width());
    }

    fn model_tools(&mut self, ui: &mut Ui) {
        let has = self.ed.selected_object.is_some();
        let tip = self.tip_for(psxcore::commands::CommandId::EditModeToggle, "Edit the vertices, edges and polygons of the selected object");
        if Btn::new("Edit mesh").icon(Icon::Vertex).accent(accent()).enabled(has).size(vec2(ui.available_width(), 34.0)).tip(&tip).show(ui).clicked() {
            self.pending = Some(psxcore::commands::CommandId::EditModeToggle);
        }
        ui.add_space(4.0);
        section(ui, "Add", accent(), |ui| {
            ui.horizontal_wrapped(|ui| {
                for (p, icon, tip) in [(Primitive::Cube, Icon::Cube, "Cube"), (Primitive::Plane, Icon::Plane, "Plane"), (Primitive::Cylinder, Icon::Cylinder, "Cylinder"), (Primitive::Cone, Icon::Cone, "Cone"), (Primitive::Sphere, Icon::Sphere, "Sphere"), (Primitive::Torus, Icon::Torus, "Torus"), (Primitive::Butterfly, Icon::Butterfly, "Butterfly: a ready-made model with a transparent texture")] {
                    if Btn::new("").icon(icon).accent(accent()).tip(tip).show(ui).clicked() {
                        self.ed.add_primitive(p);
                    }
                }
            });
            if Btn::new("Import model").icon(Icon::Open).accent(accent()).size(vec2(ui.available_width(), 34.0)).show(ui).clicked() {
                self.dialog = Some(dialogs::browser(dialogs::Purpose::ImportModel));
            }
        });
        let strip = self.settings.show_toolbar;
        section(ui, "Mouse tool", accent(), |ui| {
            if strip {
                dim(ui, if self.model_tool == ModelTool::Select { "Select: click a face, Shift adds. Tools are in the strip next to the view." } else { "Sculpt: drag a vertex to push and pull the surface." });
            } else {
            ui.horizontal(|ui| {
                if Btn::new("Select").icon(Icon::Select).accent(accent()).on(self.model_tool == ModelTool::Select).tip("Click a face to select it; Shift adds to the selection").show(ui).clicked() {
                    self.model_tool = ModelTool::Select;
                }
                if Btn::new("Sculpt").icon(Icon::Sculpt).accent(accent()).on(self.model_tool == ModelTool::Sculpt).tip("Drag a vertex to push and pull the surface").show(ui).clicked() {
                    self.model_tool = ModelTool::Sculpt;
                }
            });
            }
            if self.model_tool == ModelTool::Sculpt {
                slider_f(ui, "radius", &mut self.sculpt_radius, 0.05..=3.0);
            }
        });
        let nf = self.ed.selected_faces.len();
        section(ui, &format!("Faces ({nf})"), accent(), |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.extrude_dist).speed(0.01).range(-5.0..=5.0));
                if Btn::new("Extrude").icon(Icon::Plus).accent(accent()).enabled(nf > 0).tip("Pull the selected faces out; a negative distance pushes them in").show(ui).clicked() {
                    self.ed.extrude_selected_faces(self.extrude_dist);
                }
            });
            dim(ui, "Move along the model axes");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.nudge).speed(0.005).range(0.001..=2.0));
            });
            ui.horizontal_wrapped(|ui| {
                let n = self.nudge;
                for (l, d, c) in [("X−", Vec3::new(-n, 0.0, 0.0), accent()), ("X+", Vec3::new(n, 0.0, 0.0), accent()), ("Y−", Vec3::new(0.0, -n, 0.0), accent()), ("Y+", Vec3::new(0.0, n, 0.0), accent()), ("Z−", Vec3::new(0.0, 0.0, -n), accent()), ("Z+", Vec3::new(0.0, 0.0, n), accent())] {
                    if Btn::new(l).accent(c).enabled(nf > 0).size(vec2(40.0, 30.0)).show(ui).clicked() {
                        self.ed.move_selected_faces(d);
                    }
                }
            });
            if Btn::new("Delete faces (Del)").icon(Icon::Trash).accent(DANGER()).enabled(nf > 0).size(vec2(ui.available_width(), 32.0)).show(ui).clicked() {
                self.ed.delete_selected_faces();
            }
        });
        section(ui, "Model shape", accent(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Subdivide").tip("Splits every triangle into 4").show(ui).clicked() {
                    self.ed.subdivide_selected();
                }
                if Btn::new("Mirror X").icon(Icon::Mirror).tip("Copies the left half of the model onto the right side").show(ui).clicked() {
                    self.ed.mirror_x_selected();
                }
                if Btn::new("Flat shade").tip("Gives every face a single flat colour of light, typical for retro models").show(ui).clicked() {
                    self.ed.flat_shade_selected();
                }
                if Btn::new("Weld").tip("Merges vertices that sit in the same place").show(ui).clicked() {
                    self.ed.weld_selected();
                }
                if Btn::new("Auto UV").tip("Creates a new automatic box-projection UV layout").show(ui).clicked() {
                    self.ed.unwrap_selected();
                }
            });
        });
        self.boolean_section(ui);
        section(ui, "Reduce polygons", accent(), |ui| {
            let now = self.ed.selected_model().map_or(0, |m| m.triangle_count());
            dim(ui, &format!("Now: {now} triangles"));
            slider_f(ui, "keep", &mut self.decimate_ratio, 0.05..=1.0);
            dim(ui, &format!("about {} triangles", (now as f32 * self.decimate_ratio) as usize));
            if Btn::new("Simplify").icon(Icon::Scissors).accent(accent()).size(vec2(ui.available_width(), 34.0)).tip("Keeps the silhouette and UV seams").show(ui).clicked() {
                if let Some((a, b)) = self.ed.decimate_selected(self.decimate_ratio) {
                    self.info(format!("Reduced from {a} to {b} triangles"));
                }
            }
        });
        self.refs_section(ui);
    }

    fn pose_tools(&mut self, ui: &mut Ui) {
        let Some(model) = self.ed.selected_model() else {
            dim(ui, "Select an object that has a skeleton.");
            return;
        };
        let Some(skel) = model.skeleton.clone() else {
            section(ui, "Skeleton", accent(), |ui| {
                dim(ui, "This model has no bones.");
                dim(ui, "Import a model with a skeleton (glTF/GLB) to pose it.");
            });
            return;
        };
        section(ui, "Bones", accent(), |ui| {
            dim(ui, "Click a joint in the viewport or here. Drag a joint: left/right tilts it sideways, up/down tilts it forward and back.");
            for (i, b) in skel.bones.iter().enumerate() {
                let mut depth = 0;
                let mut cur = b.parent;
                while let Some(p) = cur {
                    depth += 1;
                    cur = skel.bones[p].parent;
                    if depth > 32 {
                        break;
                    }
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let (gap, _) = ui.allocate_exact_size(vec2(depth as f32 * 14.0, 24.0), egui::Sense::hover());
                    for d in 0..depth {
                        let x = gap.min.x + 13.0 + d as f32 * 14.0;
                        ui.painter().line_segment([egui::pos2(x, gap.min.y - 2.0), egui::pos2(x, gap.max.y + 2.0)], compat::stroke(1.0, LINE()));
                    }
                    if crate::widgets::tree_row(ui, Icon::Bone, &b.name, self.ed.selected_bone == Some(i), None, "Select this bone").row.clicked() {
                        self.ed.selected_bone = Some(i);
                    }
                });
            }
        });
        self.ik_section(ui);
        if let Some(b) = self.ed.selected_bone {
            section(ui, "Rotate bone", accent(), |ui| {
                for (axis, name, c) in [(Vec3::new(1.0, 0.0, 0.0), "X", AXIS_X()), (Vec3::new(0.0, 1.0, 0.0), "Y", AXIS_Y()), (Vec3::new(0.0, 0.0, 1.0), "Z", AXIS_Z())] {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{name} axis")).color(c));
                        for d in [-15.0f32, -5.0, 5.0, 15.0] {
                            let l = format!("{d:+}°");
                            if Btn::new(&l).accent(c).size(vec2(44.0, 28.0)).show(ui).clicked() {
                                self.ed.rotate_bone_world(b, axis, d);
                            }
                        }
                    });
                }
                if Btn::new("Reset bone").icon(Icon::Undo).size(vec2(ui.available_width(), 30.0)).show(ui).clicked() {
                    self.ed.reset_bone(b);
                }
            });
        }
        section(ui, "Whole pose", accent(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if Btn::new("Reset").icon(Icon::Undo).show(ui).clicked() {
                    self.ed.reset_pose();
                }
                if Btn::new("Mirror").icon(Icon::Mirror).tip("Swaps left and right bones (matched by L/R in the bone names)").show(ui).clicked() {
                    self.ed.mirror_pose();
                }
                if Btn::new("Copy").icon(Icon::Copy).show(ui).clicked() {
                    self.ed.copy_pose();
                }
                if Btn::new("Paste").enabled(self.ed.clip_pose.is_some()).show(ui).clicked() {
                    self.ed.paste_pose();
                }
            });
        });
    }

    fn anim_tools(&mut self, ui: &mut Ui) {
        if !self.settings.show_toolbar {
        section(ui, "Tool", accent(), |ui| {
            ui.horizontal_wrapped(|ui| {
                for (t, icon, name, tip) in [(AnimTool::Pose, Icon::Bone, "Pose", "Drag joints to pose the model"), (AnimTool::Hand, Icon::Hand, "Draw", "Draw by hand over the frame"), (AnimTool::PaintModel, Icon::Brush, "Paint", "Paint on the model: emotions and per-frame texture changes"), (AnimTool::MoveObject, Icon::Move, "Move", "Drag the object across the screen plane")] {
                    if Btn::new(name).icon(icon).accent(accent()).on(self.anim_tool == t).tip(tip).show(ui).clicked() {
                        self.anim_tool = t;
                    }
                }
            });
        });
        }
        match self.anim_tool {
            AnimTool::Pose => self.pose_tools(ui),
            AnimTool::Hand => {
                paint_tools(self, ui, accent(), true);
                section(ui, "Frame drawing", accent(), |ui| {
                    let mut only = self.ed.frame().is_some_and(|f| f.hand_only);
                    if toggle(ui, &mut only, "Drawing only (hide 3D)", accent()).clicked() {
                        self.ed.set_hand_only(only);
                    }
                    if Btn::new("Clear drawing").icon(Icon::Trash).accent(DANGER()).size(vec2(ui.available_width(), 30.0)).show(ui).clicked() {
                        self.ed.clear_hand_layer();
                    }
                });
            }
            AnimTool::PaintModel => {
                paint_tools(self, ui, accent(), false);
                self.emotions_section(ui);
            }
            AnimTool::MoveObject => dim(ui, "Drag the object with the mouse. Changes apply to the current frame only."),
        }
        section(ui, "Frame camera", accent(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if Btn::new("View to camera").tip("Stores the current viewport view as this frame's camera").show(ui).clicked() {
                    self.ed.camera_from_view();
                    self.ed.through_camera = true;
                }
                if Btn::new("Camera to view").tip("Moves the viewport to this frame's camera so you can adjust it").show(ui).clicked() {
                    self.ed.view_from_camera();
                }
            });
            toggle(ui, &mut self.ed.through_camera, "Look through the frame camera", accent());
            let mut fov = self.ed.frame().map_or(50.0, |f| f.camera.fov_y.to_degrees());
            if slider_f(ui, "field of view", &mut fov, 10.0..=120.0) {
                self.cont(|ed| ed.set_frame_fov(fov));
            }
            toggle(ui, &mut self.ed.onion, "Onion skin (neighbour frames)", accent());
        });
    }

    fn emotions_section(&mut self, ui: &mut Ui) {
        let Some(asset) = self.ed.selected_asset() else { return };
        let Some(obj) = self.ed.selected_object else { return };
        let list: Vec<String> = self.ed.project.assets[asset].emotions.iter().map(|e| e.name.clone()).collect();
        let cur = self.ed.frame().and_then(|f| f.states.iter().find(|s| s.object == obj)).and_then(|s| s.emotion);
        section(ui, "Emotions", accent(), |ui| {
            if list.is_empty() {
                dim(ui, "An emotion is a ready-made texture variant (face, eyes). Create the first one from the current texture.");
            }
            for (i, n) in list.iter().enumerate() {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 74.0).max(60.0);
                    if Btn::new(n).accent(accent()).on(cur == Some(i)).size(vec2(w, 28.0)).tip("Use in this frame").show(ui).clicked() {
                        self.ed.set_frame_emotion(obj, Some(i));
                        self.ed.active_emotion = Some(i);
                    }
                    if Btn::new("").icon(Icon::Trash).accent(DANGER()).size(vec2(30.0, 28.0)).tip("Delete emotion").show(ui).clicked() {
                        self.ed.delete_emotion(i);
                    }
                });
            }
            if Btn::new("No emotion").enabled(cur.is_some()).size(vec2(ui.available_width(), 28.0)).show(ui).clicked() {
                self.ed.set_frame_emotion(obj, None);
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_emotion).hint_text("name").desired_width(110.0));
                if Btn::new("Add").icon(Icon::Plus).accent(accent()).show(ui).clicked() {
                    let name = if self.new_emotion.trim().is_empty() { format!("Emotion {}", list.len() + 1) } else { self.new_emotion.trim().to_string() };
                    if let Some(i) = self.ed.add_emotion(&name) {
                        self.ed.set_frame_emotion(obj, Some(i));
                        self.ed.paint_layer = PaintLayer::Emotion;
                    }
                    self.new_emotion.clear();
                }
            });
            if Btn::new("Remove frame repaint").size(vec2(ui.available_width(), 28.0)).tip("Restores the texture without this frame's one-off edits").show(ui).clicked() {
                self.ed.clear_frame_patch(obj);
            }
        });
    }

    fn render_tools(&mut self, ui: &mut Ui) {
        section(ui, "Camera", accent(), |ui| {
            dim(ui, "The final picture is drawn through the frame camera of the current frame. Resolution, look and light are on the right.");
            toggle(ui, &mut self.ed.through_camera, "Look through the frame camera", accent());
            let mut fov = self.ed.frame().map_or(50.0, |f| f.camera.fov_y.to_degrees());
            if slider_f(ui, "field of view", &mut fov, 10.0..=120.0) {
                self.cont(|ed| ed.set_frame_fov(fov));
            }
            ui.horizontal_wrapped(|ui| {
                if Btn::new("View to camera").tip("Stores the current viewport view as this frame's camera").show(ui).clicked() {
                    self.ed.camera_from_view();
                    self.ed.through_camera = true;
                }
                if Btn::new("Camera to view").tip("Moves the viewport to this frame's camera").show(ui).clicked() {
                    self.ed.view_from_camera();
                }
            });
        });
        section(ui, "Render preview window", accent(), |ui| {
            let mut pv = self.settings.preview;
            let mut ch = toggle(ui, &mut pv.visible, "Show the preview window", accent()).clicked();
            ui.horizontal_wrapped(|ui| {
                for (c, l) in [(psxcore::settings::Corner::TopLeft, "Top left"), (psxcore::settings::Corner::TopRight, "Top right"), (psxcore::settings::Corner::BottomLeft, "Bottom left"), (psxcore::settings::Corner::BottomRight, "Bottom right")] {
                    if Btn::new(l).accent(accent()).on(pv.corner == c).tip("Corner of the viewport").show(ui).clicked() {
                        pv.corner = c;
                        ch = true;
                    }
                }
            });
            ch |= slider_f(ui, "window width", &mut pv.width, psxcore::settings::PREVIEW_WIDTH_MIN..=psxcore::settings::PREVIEW_WIDTH_MAX);
            if ch {
                self.settings.preview = pv;
                self.touch_settings();
            }
        });
    }

    pub fn right_panel(&mut self, ctx: &Context) {
        let mut panel = SidePanel::right("right").resizable(true).default_width(self.settings.layout.right_width).width_range(240.0..=440.0).frame(compat::frame(panel_fill(PANEL()), 10.0));
        if self.layout_reset {
            panel = panel.exact_width(self.settings.layout.right_width);
        }
        let shown = panel.show(ctx, |ui| {
            self.art.paint_panel(ui, crate::themeart::Slot::Panel, vec2(10.0, 10.0));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let w = ((ui.available_width() - 6.0) / 4.0).floor();
                for (t, icon, tip) in [
                    (RightTab::Scene, Icon::List, "Scene: objects and their placement"),
                    (RightTab::Look, Icon::Sliders, "Look: render resolution, retro effects and mood"),
                    (RightTab::World, Icon::Globe, "World: sky, fog and light"),
                    (RightTab::Output, Icon::Export, "Output: export pictures and animations"),
                ] {
                    if crate::widgets::tab(ui, "", Some(icon), self.right_tab == t, tip, w).clicked() {
                        self.right_tab = t;
                    }
                }
            });
            let (icon, title, hint) = match self.right_tab {
                RightTab::Scene => (Icon::List, "Scene", "objects and placement"),
                RightTab::Look => (Icon::Sliders, "Look", "resolution, retro effects, mood"),
                RightTab::World => (Icon::Globe, "World", "sky, fog and light"),
                RightTab::Output => (Icon::Export, "Output", "pictures and animations"),
            };
            pane_header(ui, icon, title, hint);
            ui.add_space(4.0);
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.right_tab {
                RightTab::Scene => self.scene_section(ui),
                RightTab::Look => {
                    let mut s = self.ed.project.scene.style.clone();
                    if look_editor(ui, &mut s) {
                        self.cont(|ed| ed.set_scene_style(s));
                    }
                }
                RightTab::World => {
                    let mut l = self.ed.project.scene.lighting.clone();
                    if lighting_editor(ui, &mut l) {
                        self.cont(|ed| ed.set_lighting(l.clone()));
                    }
                    let mut s = self.ed.project.scene.style.clone();
                    let followed = l.tod_on;
                    if followed {
                        dim(ui, "Sky, fog colours and light below are replaced by the time of day while it is on.");
                    }
                    let changed = ui.add_enabled_ui(!followed, |ui| world_editor(ui, &mut s)).inner;
                    if changed {
                        self.cont(|ed| ed.set_scene_style(s));
                    }
                }
                RightTab::Output => self.export_panel(ui),
            });
        });
        self.track_width(crate::Panel::Right, shown.response.rect.width());
    }

    fn scene_section(&mut self, ui: &mut Ui) {
        let objs: Vec<(u32, String, bool)> = self.ed.project.scene.objects.iter().map(|o| (o.id, o.name.clone(), o.visible)).collect();
        section(ui, "Scene", accent(), |ui| {
            if objs.is_empty() {
                dim(ui, "Empty. Add a primitive or import a model.");
            }
            for (id, name, vis) in &objs {
                {
                    let icon = object_icon(name);
                    let r = crate::widgets::tree_row(ui, icon, name, self.ed.selected_object == Some(*id), Some(*vis), "Select. Right-click for more actions");
                    if r.eye {
                        self.ed.set_object_visible(*id, !*vis);
                    } else if r.row.clicked() {
                        self.ed.select_object(Some(*id));
                    }
                    let row = r.row;
                    row.context_menu(|ui| {
                        if ui.button("Select").clicked() {
                            self.ed.select_object(Some(*id));
                            ui.close_menu();
                        }
                        if ui.button(if *vis { "Hide" } else { "Show" }).clicked() {
                            self.ed.set_object_visible(*id, !*vis);
                            ui.close_menu();
                        }
                        if ui.button("Duplicate").clicked() {
                            self.ed.duplicate_object(*id);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Delete").clicked() {
                            self.ed.delete_object(*id);
                            ui.close_menu();
                        }
                    });
                }
            }
            if let Some(id) = self.ed.selected_object {
                ui.horizontal(|ui| {
                    if Btn::new("Duplicate").icon(Icon::Copy).show(ui).clicked() {
                        self.ed.duplicate_object(id);
                    }
                    if Btn::new("Delete").icon(Icon::Trash).accent(DANGER()).show(ui).clicked() {
                        self.ed.delete_object(id);
                    }
                });
            }
        });
        self.placement_section(ui);
        self.lights_section(ui);
    }

    fn placement_section(&mut self, ui: &mut Ui) {
        let Some(id) = self.ed.selected_object else { return };
        let Some(t) = self.ed.object_transform(id) else { return };
        section(ui, if matches!(self.ed.mode, Mode::Pose | Mode::Animate) { "Placement (this frame)" } else { "Placement" }, accent(), |ui| {
            let mut pos = [t.pos.x, t.pos.y, t.pos.z];
            let mut rot = [t.rot_deg.x, t.rot_deg.y, t.rot_deg.z];
            let mut scl = [t.scale.x, t.scale.y, t.scale.z];
            let mut ch = drag3(ui, "position", &mut pos, 0.02);
            ch |= drag3(ui, "rotation °", &mut rot, 0.5);
            ch |= drag3(ui, "scale", &mut scl, 0.01);
            if ch {
                let nt = Transform { pos: Vec3::new(pos[0], pos[1], pos[2]), rot_deg: Vec3::new(rot[0], rot[1], rot[2]), scale: Vec3::new(scl[0].max(0.001), scl[1].max(0.001), scl[2].max(0.001)) };
                self.cont(|ed| ed.set_object_transform(id, nt));
            }
            if let Some(o) = self.ed.project.scene.object(id) {
                let mut tint = o.tint;
                if rgb_edit(ui, "object tint", &mut tint) {
                    self.cont(|ed| ed.set_object_tint(id, tint));
                }
            }
        });
    }
}
