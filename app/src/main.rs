//! Jank! — a studio for low-poly and PSX-style models. This crate is a thin shell: all logic lives in `psxcore` (editor, jobs, filebrowser).
mod appinfo;
mod commands_ui;
mod crash;
mod compat;
mod dialogs;
mod gizmo_ui;
mod icons;
mod paintui;
mod colorpick;
mod panels;
mod splash;
mod startup;
mod perf;
mod preview;
mod sysdate;
mod texview;
mod theme;
mod timeline;
mod viewport;
mod widgets;

use dialogs::{Dialog, ExportCfg};
use eframe::egui::{self, Align, Color32, Context, Layout, TopBottomPanel};
use icons::Icon;
use psxcore::commands::CommandId;
use psxcore::editor::{Editor, Mode};
use psxcore::jobs::ExportJob;
use psxcore::raster::Renderer;
use theme::*;
use widgets::Btn;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModelTool {
    Select,
    Sculpt,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AnimTool {
    Pose,
    Hand,
    PaintModel,
    MoveObject,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub age: f32,
}

/// Resizable panels whose size is remembered between sessions.
#[derive(Clone, Copy)]
pub enum Panel {
    Left,
    Right,
    Texture,
    Timeline,
}

pub struct App {
    pub settings: psxcore::settings::Settings,
    pub gizmo: gizmo_ui::GizmoPrefs,
    pub hk: commands_ui::Housekeeping,
    /// Command chosen in the palette, run once the dialog code has finished.
    pub pending: Option<psxcore::commands::CommandId>,
    /// Set for one frame after "Reset panel layout" so the panels take their default sizes.
    pub layout_reset: bool,
    pub ed: Editor,
    pub rend: Renderer,
    /// GPU handle used for the viewport textures (see `compat::GlSurface`).
    pub gl: Option<std::sync::Arc<eframe::glow::Context>>,
    pub surf: [compat::GlSurface; 4],
    pub perf: perf::Perf,
    /// True while the mouse is over the timeline (Delete then removes the current frame).
    pub hover_timeline: bool,
    /// Tab of the right-hand panel.
    pub right_tab: panels::RightTab,
    pub paint: paintui::PaintUi,
    pub ghost: viewport::GhostCache,
    pub pv: preview::PreviewState,
    pub dialog: Option<Dialog>,
    pub job: Option<ExportJob>,
    pub export: ExportCfg,
    pub toasts: Vec<Toast>,
    pub model_tool: ModelTool,
    pub anim_tool: AnimTool,
    pub tex: texview::TexView,
    pub vp: viewport::VpState,
    pub grouping: bool,
    pub extrude_dist: f32,
    pub nudge: f32,
    pub decimate_ratio: f32,
    pub sculpt_radius: f32,
    pub new_emotion: String,
    pub last_title: String,
    pub allow_close: bool,
    pub recent: Vec<[u8; 4]>,
}

pub const SURF_VIEW: usize = 0;
pub const SURF_PREVIEW: usize = 1;
pub const SURF_GHOST_PREV: usize = 2;
pub const SURF_GHOST_NEXT: usize = 3;

pub fn mode_accent(m: Mode) -> Color32 {
    match m {
        Mode::Model => ACCENT,
        Mode::Texture => ACCENT,
        Mode::Pose => ACCENT,
        Mode::Animate => ACCENT,
        Mode::Render => ACCENT,
    }
}
pub fn mode_label(m: Mode) -> &'static str {
    match m {
        Mode::Model => "Model",
        Mode::Texture => "Texture",
        Mode::Pose => "Pose",
        Mode::Animate => "Animate",
        Mode::Render => "Render",
    }
}
pub fn mode_icon(m: Mode) -> Icon {
    match m {
        Mode::Model => Icon::Cube,
        Mode::Texture => Icon::Palette,
        Mode::Pose => Icon::Bone,
        Mode::Animate => Icon::Film,
        Mode::Render => Icon::Camera,
    }
}
pub fn mode_command(m: Mode) -> CommandId {
    match m {
        Mode::Model => CommandId::ModeModel,
        Mode::Texture => CommandId::ModeTexture,
        Mode::Pose => CommandId::ModePose,
        Mode::Animate => CommandId::ModeAnimate,
        Mode::Render => CommandId::ModeRender,
    }
}
pub const MODES: [Mode; 5] = [Mode::Model, Mode::Texture, Mode::Pose, Mode::Animate, Mode::Render];

impl App {
    /// Builds the app from what the start-up thread prepared (see `startup.rs`).
    pub fn from_loaded(settings: psxcore::settings::Settings, problems: Vec<String>, l: startup::Loaded) -> App {
        let mut app = App {
            settings,
            gizmo: gizmo_ui::GizmoPrefs::default(),
            hk: commands_ui::Housekeeping::new(),
            pending: None,
            layout_reset: false,
            ed: l.editor,
            rend: l.rend,
            gl: None,
            surf: Default::default(),
            perf: perf::Perf::default(),
            hover_timeline: false,
            right_tab: panels::RightTab::Scene,
            paint: Default::default(),
            ghost: Default::default(),
            pv: Default::default(),
            dialog: None,
            job: None,
            export: ExportCfg::default(),
            toasts: vec![],
            model_tool: ModelTool::Select,
            anim_tool: AnimTool::Pose,
            tex: texview::TexView::default(),
            vp: viewport::VpState::default(),
            grouping: false,
            extrude_dist: 0.25,
            nudge: 0.1,
            decimate_ratio: 0.5,
            sculpt_radius: 0.6,
            new_emotion: String::new(),
            last_title: String::new(),
            allow_close: false,
            recent: vec![],
        };
        app.report_settings_problems(problems);
        if l.crash_reports > 0 {
            if let Some(d) = crash::crash_dir() {
                app.info(format!("{} crash report(s) are saved in {}", l.crash_reports, d.display()));
            }
        }
        if let Some(n) = l.opened {
            app.info(format!("Opened: {n}"));
        }
        if let Some(e) = l.error {
            app.error(e);
        }
        if let Some(p) = l.import_path {
            app.open_path(std::path::Path::new(&p));
        }
        if let Some(s) = l.snapshot {
            app.dialog = Some(Dialog::Recover(s));
        }
        app
    }

    /// Remembers the current brush colour in the "recent colours" list.
    pub fn remember_color(&mut self) {
        let c = self.ed.brush.color;
        self.recent.retain(|x| *x != c);
        self.recent.insert(0, c);
        self.recent.truncate(12);
    }

    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toasts.push(Toast { text: text.into(), kind, age: 0.0 });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }
    pub fn info(&mut self, text: impl Into<String>) {
        self.toast(text, ToastKind::Info);
    }
    pub fn error(&mut self, text: impl Into<String>) {
        self.toast(text, ToastKind::Error);
    }

    /// Continuous edits (sliders, fields): every change made during one mouse press becomes a single undo step.
    pub fn cont(&mut self, f: impl FnOnce(&mut Editor)) {
        if !self.grouping {
            self.ed.begin_group();
            self.grouping = true;
        }
        f(&mut self.ed);
    }

    /// Opens a file by extension: a `.jank` project or a model.
    pub fn open_path(&mut self, path: &std::path::Path) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == "jank" {
            match Editor::open(path) {
                Ok(ed) => {
                    self.ed = ed;
                    self.info(format!("Opened: {}", self.ed.project.name));
                }
                Err(e) => self.error(e),
            }
        } else {
            match self.ed.import_path(path) {
                Ok(w) if w.is_empty() => {}
                Ok(w) => self.dialog = Some(Dialog::Warnings(w)),
                Err(e) => self.error(e),
            }
        }
    }

    pub fn save(&mut self) {
        match self.ed.save() {
            Ok(()) => self.info("Project saved"),
            Err(_) => self.dialog = Some(dialogs::browser(dialogs::Purpose::SaveProject)),
        }
    }
    pub fn new_project(&mut self) {
        if self.ed.dirty {
            self.dialog = Some(Dialog::ConfirmDiscard(dialogs::After::New));
        } else {
            self.ed.new_project("Untitled");
        }
    }

    fn poll_job(&mut self, ctx: &Context) {
        let Some(job) = self.job.as_mut() else { return };
        if job.is_finished() {
            if let Some(res) = job.take_result() {
                match res {
                    Ok(r) if r.cancelled => self.info("Export cancelled"),
                    Ok(r) => self.info(format!("Done: {} ({} frames, {})", r.path.display(), r.frames, psxcore::filebrowser::human_size(r.bytes))),
                    Err(e) => self.error(e),
                }
            }
            self.job = None;
        } else {
            ctx.request_repaint();
        }
    }

    /// Main menu: every command of the catalogue, grouped the way artists look for them.
    fn menu_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("menu").exact_height(24.0).frame(compat::frame(BG1, 0.0).inner_margin(egui::Margin::symmetric(6, 2))).show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                use CommandId::*;
                let groups: [(&str, &[CommandId]); 6] = [
                    ("File", &[NewProject, OpenProject, SaveProject, SaveProjectAs, ImportModel, ExportOutput, Quit]),
                    ("Edit", &[Undo, Redo, DeleteSelected, CommandPalette, OpenSettings]),
                    ("Model", &[AddCube, AddButterfly, Subdivide, MirrorX, WeldVertices, FlatShade, AutoUv]),
                    ("Animation", &[TogglePlay, PrevFrame, NextFrame, AddFrame, DuplicateFrame]),
                    ("View", &[ModeModel, ModeTexture, ModePose, ModeAnimate, ModeRender, FrameAll, TogglePreview, ToggleStats, GizmoSelect, GizmoMove, GizmoRotate, GizmoScale, GizmoToggleSpace, GizmoToggleSnap]),
                    ("Help", &[OpenAbout, OpenLicenses]),
                ];
                for (name, items) in groups {
                    ui.menu_button(name, |ui| {
                        ui.set_min_width(230.0);
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
            });
        });
    }

    fn top_bar(&mut self, ctx: &Context) {
        self.menu_bar(ctx);
        TopBottomPanel::top("top").exact_height(36.0).frame(compat::frame(BG0, 0.0).inner_margin(egui::Margin::symmetric(8, 4))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                widgets::logo(ui, 20.0);
                ui.add_space(8.0);
                for m in MODES {
                    let on = self.ed.mode == m;
                    if widgets::tab(ui, mode_label(m), Some(mode_icon(m)), on, &self.tip_for(mode_command(m), mode_label(m))).clicked() {
                        self.ed.set_mode(m);
                    }
                }
                ui.add_space(10.0);
                ui.separator();
                if Btn::new("").icon(Icon::Plus).tip(&self.tip_for(CommandId::NewProject, "New project")).show(ui).clicked() {
                    self.new_project();
                }
                if Btn::new("").icon(Icon::Open).tip(&self.tip_for(CommandId::OpenProject, "Open project")).show(ui).clicked() {
                    self.dialog = Some(dialogs::browser(dialogs::Purpose::OpenProject));
                }
                if Btn::new("").icon(Icon::Save).tip(&self.tip_for(CommandId::SaveProject, "Save project")).show(ui).clicked() {
                    self.save();
                }
                if Btn::new("Import").icon(Icon::Cube).tip(&self.tip_for(CommandId::ImportModel, "Import a model: OBJ, STL, PLY, glTF/GLB")).show(ui).clicked() {
                    self.dialog = Some(dialogs::browser(dialogs::Purpose::ImportModel));
                }
                ui.separator();
                if Btn::new("").icon(Icon::Undo).enabled(self.ed.can_undo()).tip(&self.tip_for(CommandId::Undo, "Undo")).show(ui).clicked() {
                    self.ed.undo();
                }
                if Btn::new("").icon(Icon::Redo).enabled(self.ed.can_redo()).tip(&self.tip_for(CommandId::Redo, "Redo")).show(ui).clicked() {
                    self.ed.redo();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if Btn::new("").icon(Icon::Gear).tip(&self.tip_for(CommandId::OpenSettings, "Settings: scale, accessibility, autosave, shortcuts")).show(ui).clicked() {
                        self.run_command(ctx, CommandId::OpenSettings);
                    }
                    if Btn::new("").icon(Icon::Search).tip(&self.tip_for(CommandId::CommandPalette, "Search commands")).show(ui).clicked() {
                        self.run_command(ctx, CommandId::CommandPalette);
                    }
                    if ui.available_width() > 300.0 {
                        ui.label(egui::RichText::new(self.ed.title()).size(12.0).color(DIM));
                    }
                });
            });
        });
    }

    /// One-line status bar: last message, selection facts and the build number.
    fn status_bar(&mut self, ctx: &Context) {
        TopBottomPanel::bottom("status").exact_height(22.0).frame(compat::frame(BG0, 0.0).inner_margin(egui::Margin::symmetric(8, 2)).stroke(compat::stroke(1.0, LINE_SOFT))).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let dim = |t: String| egui::RichText::new(t).size(11.5).color(DIM);
                let msg = if self.ed.status.is_empty() { "Ready".to_string() } else { self.ed.status.clone() };
                ui.label(dim(msg));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(dim(format!("Jank! {}", appinfo::version_string())));
                    ui.separator();
                    let sel = self.ed.selected_object.and_then(|id| self.ed.project.scene.object(id)).map(|o| o.name.clone());
                    let tris = self.ed.selected_model().map_or(0, |m| m.triangle_count());
                    ui.label(dim(match sel {
                        Some(n) => format!("{n}  |  {tris} tris  |  {} faces selected", self.ed.selected_faces.len()),
                        None => "no object selected".to_string(),
                    }));
                    ui.separator();
                    ui.label(dim(format!("{} mode", mode_label(self.ed.mode))));
                });
            });
        });
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }

    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        if self.gl.is_none() {
            self.gl = frame.gl().cloned();
        }
        for s in self.surf.iter_mut() {
            s.ensure(frame);
        }
        let dt = compat::dt(ctx);
        self.perf.note_frame(dt);
        if compat::close_requested(ctx) && !self.allow_close && self.ed.dirty {
            compat::cancel_close(ctx);
            self.dialog = Some(Dialog::ConfirmDiscard(dialogs::After::Quit));
        }
        self.housekeeping(ctx);
        // The timeline sets this while drawing; shortcuts see the value from the previous frame.
        self.shortcuts(ctx);
        self.hover_timeline = false;
        if self.ed.advance(dt) || self.ed.playing {
            ctx.request_repaint();
        }
        self.poll_job(ctx);

        self.top_bar(ctx);
        self.status_bar(ctx);
        if matches!(self.ed.mode, Mode::Pose | Mode::Animate | Mode::Render) {
            self.timeline_panel(ctx);
        }
        self.left_panel(ctx);
        self.right_panel(ctx);
        if self.ed.mode == Mode::Texture || (self.ed.mode == Mode::Animate && self.anim_tool != AnimTool::Pose) {
            self.texture_panel(ctx);
        }
        egui::CentralPanel::default().frame(compat::frame(BG0, 0.0)).show(ctx, |ui| self.viewport_ui(ui));

        self.dialogs(ctx);
        self.run_pending(ctx);
        self.toasts_ui(ctx, dt);
        self.layout_reset = false;

        let title = self.ed.title();
        if title != self.last_title {
            compat::set_title(ctx, &title);
            self.last_title = title;
        }
        if self.grouping && !ctx.input(|i| i.pointer.any_down()) {
            self.ed.end_group();
            self.grouping = false;
        }
    }
}

const MAIN_SIZE: [f32; 2] = [1520.0, 920.0];
const MAIN_MIN: [f32; 2] = [1100.0, 700.0];

/// Command-line options. Anything that is not an option is treated as the path of a model or project to open.
/// `--no-splash` skips the splash screen; `--splash 1-5|default|halloween|christmas|newyear|valentine` and `--date MM-DD` preview a season.
struct Cli {
    path: Option<String>,
    splash: Option<psxcore::splash::Season>,
    date: Option<(u32, u32)>,
    no_splash: bool,
}
fn parse_cli() -> Cli {
    let mut c = Cli { path: None, splash: None, date: None, no_splash: std::env::var_os("JANK_NO_SPLASH").is_some() };
    let env_splash = std::env::var("JANK_SPLASH").ok().and_then(|v| psxcore::splash::Season::parse(&v));
    let env_date = std::env::var("JANK_DATE").ok().and_then(|v| psxcore::splash::parse_month_day(&v));
    c.splash = env_splash;
    c.date = env_date;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--no-splash" => c.no_splash = true,
            "--splash" => c.splash = it.next().and_then(|v| psxcore::splash::Season::parse(&v)).or(c.splash),
            "--date" => c.date = it.next().and_then(|v| psxcore::splash::parse_month_day(&v)).or(c.date),
            _ if c.path.is_none() => c.path = Some(a),
            _ => {}
        }
    }
    c
}

/// The window's first owner: shows the splash while the loader thread works, then becomes the `App`.
struct Shell {
    boot: startup::Boot,
    splash: Option<splash::Splash>,
    app: Option<App>,
    settings: Option<psxcore::settings::Settings>,
    problems: Vec<String>,
}

impl eframe::App for Shell {
    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(a) = self.app.as_mut() {
            eframe::App::on_exit(a, gl);
        }
    }

    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        if self.app.is_none() {
            let loading = self.boot.poll();
            let leave = match self.splash.as_mut() {
                Some(sp) => matches!(sp.ui(ctx, &mut self.boot, !loading), splash::State::Done),
                None => {
                    egui::CentralPanel::default().frame(compat::frame(BG0, 0.0)).show(ctx, |ui| {
                        ui.centered_and_justified(|ui| ui.label(egui::RichText::new(self.boot.label).color(DIM)));
                    });
                    !loading
                }
            };
            if !leave {
                ctx.request_repaint();
                return;
            }
            if std::env::var_os("JANK_LOG").is_some() {
                eprintln!("jank: start-up work took {:.0} ms", self.boot.took.unwrap_or(0.0) * 1000.0);
            }
            let loaded = *self.boot.done.take().expect("loader finished");
            let settings = self.settings.take().unwrap_or_default();
            let app = App::from_loaded(settings, std::mem::take(&mut self.problems), loaded);
            if self.splash.take().is_some() {
                compat::enter_main(ctx, &app.ed.title(), MAIN_SIZE, MAIN_MIN);
            }
            self.app = Some(app);
            ctx.request_repaint();
            return;
        }
        if let Some(a) = self.app.as_mut() {
            eframe::App::update(a, ctx, frame);
        }
    }
}

fn main() -> eframe::Result {
    crash::install();
    let cli = parse_cli();
    let season = cli.splash.unwrap_or_else(|| {
        let (m, d) = cli.date.unwrap_or_else(sysdate::today);
        psxcore::splash::season_for(m, d)
    });
    // settings are tiny and decide whether a splash window is created at all, so they load first
    let (settings, problems) = commands_ui::load_settings();
    let with_splash = !cli.no_splash && settings.show_splash;
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64 / 1000);
    let boot = startup::Boot::spawn(season, seed, cli.path);
    compat::run("Jank!", MAIN_SIZE, MAIN_MIN, with_splash.then_some(splash::SPLASH_SIZE), move |ctx| {
        theme::apply(ctx);
        Box::new(Shell { boot, splash: with_splash.then(splash::Splash::new), app: None, settings: Some(settings), problems })
    })
}
