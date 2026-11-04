//! Jank! — a studio for low-poly and PSX-style models. This crate is a thin shell: all logic lives in `psxcore` (editor, jobs, filebrowser).
mod appinfo;
mod chrome;
mod commands_ui;
mod crash;
mod compat;
mod dialogs;
mod discord_ui;
mod editui;
mod extras_ui;
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
use eframe::egui::{self, Color32, Context};
use icons::Icon;
use psxcore::commands::CommandId;
use psxcore::editor::{Editor, Mode};
use psxcore::jobs::ExportJob;
use psxcore::raster::Renderer;
use theme::*;

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
    pub surf: [compat::GlSurface; 5],
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
    /// Edit mode (mesh editing) state of the viewport.
    pub edui: editui::EditUi,
    /// The open radial menu (Space).
    pub radial: Option<extras_ui::Radial>,
    pub graph: extras_ui::GraphState,
    /// Built-in themes and the user's theme files.
    pub themes: Vec<psxcore::themes::Theme>,
    /// The object that cuts the selected one in a boolean.
    pub bool_cutter: Option<u32>,
    pub bool_delete: bool,
    /// Chain length used when IK is switched on.
    pub ik_len: usize,
    /// Palette entry of an indexed texture whose colour is being changed.
    pub idx_edit: Option<(usize, [u8; 3])>,
    /// Width of the viewport widget in points (for pick radii).
    pub vp_width: f32,
    /// Tab presses taken away from egui's focus navigation (Tab toggles Edit mode).
    pub tab_chords: Vec<psxcore::keymap::Chord>,
    /// Discord Rich Presence connection and timer.
    pub rpc: discord_ui::Rpc,
}

pub const SURF_VIEW: usize = 0;
pub const SURF_PREVIEW: usize = 1;
pub const SURF_GHOST_PREV: usize = 2;
pub const SURF_GHOST_NEXT: usize = 3;
/// Hover outline and Edit-mode face tint.
pub const SURF_OVERLAY: usize = 4;

pub fn mode_accent(m: Mode) -> Color32 {
    match m {
        Mode::Model => accent(),
        Mode::Texture => accent(),
        Mode::Pose => accent(),
        Mode::Animate => accent(),
        Mode::Render => accent(),
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
            edui: Default::default(),
            radial: None,
            graph: extras_ui::GraphState { only_selected: true, ..Default::default() },
            themes: psxcore::themes::builtin(),
            bool_cutter: None,
            bool_delete: false,
            ik_len: 2,
            idx_edit: None,
            vp_width: 1.0,
            tab_chords: vec![],
            rpc: Default::default(),
        };
        let settings_existed = psxcore::settings::settings_path().is_some_and(|p| p.exists());
        app.reload_themes();
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
        app.whats_new_at_start(settings_existed);
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
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }

    /// Tab belongs to the editor (Edit mode) unless a text field has the keyboard: egui would
    /// otherwise use it to move the keyboard focus between widgets.
    fn raw_input_hook(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        if ctx.wants_keyboard_input() || self.dialog.is_some() {
            return;
        }
        let chords = &mut self.tab_chords;
        raw.events.retain(|e| match e {
            egui::Event::Key { key: egui::Key::Tab, pressed, modifiers, .. } => {
                if *pressed {
                    if let Some(c) = psxcore::keymap::Chord::new(modifiers.ctrl || modifiers.command, modifiers.shift, modifiers.alt, "Tab") {
                        chords.push(c);
                    }
                }
                false
            }
            _ => true,
        });
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

        self.menu_bar(ctx);
        self.status_bar(ctx);
        self.ribbon(ctx);
        if matches!(self.ed.mode, Mode::Pose | Mode::Animate | Mode::Render) {
            self.timeline_panel(ctx);
        }
        self.left_panel(ctx);
        self.right_panel(ctx);
        if self.ed.mode == Mode::Texture || (self.ed.mode == Mode::Animate && self.anim_tool != AnimTool::Pose) {
            self.texture_panel(ctx);
        }
        self.tool_strip(ctx);
        egui::CentralPanel::default().frame(compat::frame(BG0(), 0.0)).show(ctx, |ui| self.viewport_ui(ui));

        self.dialogs(ctx);
        self.radial_ui(ctx);
        self.discord_tick(ctx);
        self.run_pending(ctx);
        self.toasts_ui(ctx, dt);
        self.sync_look(ctx);
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
/// `--no-splash` skips the splash screen; `--splash 1-6|default|halloween|christmas|newyear|valentine|winter` and `--date MM-DD` preview a season.
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
    fn raw_input_hook(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        if let Some(a) = self.app.as_mut() {
            eframe::App::raw_input_hook(a, ctx, raw);
        }
    }

    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        if self.app.is_none() {
            let loading = self.boot.poll();
            let leave = match self.splash.as_mut() {
                Some(sp) => matches!(sp.ui(ctx, &mut self.boot, !loading), splash::State::Done),
                None => {
                    egui::CentralPanel::default().frame(compat::frame(BG0(), 0.0)).show(ctx, |ui| {
                        ui.centered_and_justified(|ui| ui.label(egui::RichText::new(self.boot.label).color(DIM())));
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
    let (settings, problems) = commands_ui::load_settings();
    // The splash is always shown; `--no-splash` / `JANK_NO_SPLASH` exist only for automated tests.
    let with_splash = !cli.no_splash;
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64 / 1000);
    let boot = startup::Boot::spawn(season, seed, cli.path);
    compat::run("Jank!", MAIN_SIZE, MAIN_MIN, with_splash.then_some(splash::SPLASH_SIZE), move |ctx| {
        theme::apply(ctx);
        Box::new(Shell { boot, splash: with_splash.then(splash::Splash::new), app: None, settings: Some(settings), problems })
    })
}
