//! Discord Rich Presence: builds the activity from the editor state once a second and hands it
//! to the connection thread in `psxcore::discord`. Settings → Discord holds the switches.
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{dim, section, toggle_tip, Btn};
use crate::App;
use eframe::egui::{self, Context};
use psxcore::discord::{self, Activity, Doing, Info, Presence, Status};
use psxcore::editor::Mode;
use std::time::{Duration, Instant};

/// No input for this long counts as a break.
const IDLE_AFTER: Duration = Duration::from_secs(5 * 60);

pub struct Rpc {
    conn: Option<Presence>,
    last: Option<Activity>,
    checked: Instant,
    /// Path (or name) of the project the timer belongs to, and when it was opened.
    project_key: String,
    since: u64,
    last_input: Instant,
    app_id: Option<String>,
}

impl Default for Rpc {
    fn default() -> Self {
        Rpc { conn: None, last: None, checked: Instant::now() - Duration::from_secs(10), project_key: String::new(), since: discord::unix_now(), last_input: Instant::now(), app_id: discord::application_id() }
    }
}

impl Rpc {
    /// Closes the connection without blocking the interface (clearing takes a round trip).
    fn close(&mut self) {
        if let Some(p) = self.conn.take() {
            std::thread::spawn(move || p.stop());
        }
        self.last = None;
    }
}

impl App {
    /// Called every frame; does real work once a second.
    pub fn discord_tick(&mut self, ctx: &Context) {
        if ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            self.rpc.last_input = Instant::now();
        }
        let prefs = self.settings.discord;
        if !prefs.enabled || self.rpc.app_id.is_none() {
            self.rpc.close();
            return;
        }
        // keep ticking while idle so the "Taking a break" state can appear, without busy redraws
        ctx.request_repaint_after(Duration::from_secs(20));
        if self.rpc.checked.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.rpc.checked = Instant::now();
        if self.rpc.conn.is_none() {
            let id = self.rpc.app_id.clone().unwrap_or_default();
            self.rpc.conn = Some(Presence::start(id));
            self.rpc.last = None;
        }
        let key = self.ed.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| self.ed.project.name.clone());
        if key != self.rpc.project_key {
            self.rpc.project_key = key;
            self.rpc.since = discord::unix_now();
        }
        let info = Info {
            project: self.ed.project.name.clone(),
            show_project: prefs.show_project,
            show_activity: prefs.show_activity,
            doing: self.discord_doing(),
            idle: self.rpc.last_input.elapsed() > IDLE_AFTER,
            since: prefs.show_time.then_some(self.rpc.since),
            version: crate::appinfo::version_string().split('+').next().unwrap_or_default().to_string(),
            buttons: vec![],
        };
        let a = discord::activity(&info);
        if self.rpc.last.as_ref() != Some(&a) {
            if let Some(c) = &self.rpc.conn {
                c.set(Some(a.clone()));
            }
            self.rpc.last = Some(a);
        }
    }

    fn discord_doing(&self) -> Doing {
        if let Some(job) = &self.job {
            return Doing::Exporting { video: job.video, percent: (job.progress() * 100.0).round().clamp(0.0, 100.0) as u32 };
        }
        let p = &self.ed.project;
        match self.ed.mode {
            Mode::Model => match self.ed.edit_mesh() {
                Some(m) => Doing::EditingMesh { verts: m.verts.len(), faces: m.faces.len() },
                None => {
                    let tris = p.scene.objects.iter().filter_map(|o| p.assets.get(o.asset)).map(|a| a.model.triangle_count()).sum();
                    Doing::Modelling { objects: p.scene.objects.len(), tris }
                }
            },
            Mode::Texture => {
                let (width, height) = self.ed.active_canvas().and_then(|c| self.ed.canvas_texture(c)).map_or((0, 0), |t| (t.w, t.h));
                Doing::Painting { width, height, on_model: self.ed.project_paint }
            }
            Mode::Pose => Doing::Posing { bones: self.ed.selected_model().and_then(|m| m.skeleton.as_ref()).map_or(0, |s| s.bones.len()) },
            Mode::Animate => {
                let (frames, fps) = self.ed.animation().map_or((1, 12), |a| (a.frames.len(), a.fps().round().max(1.0) as u32));
                Doing::Animating { frame: self.ed.current_frame, frames, playing: self.ed.playing, fps }
            }
            Mode::Render => {
                let (width, height) = self.ed.render_size();
                Doing::Rendering { width, height }
            }
        }
    }

    pub fn settings_discord(&mut self, ui: &mut egui::Ui) {
        section(ui, "Discord activity", accent(), |ui| {
            dim(ui, "Shows \u{201c}Playing Jank!\u{201d} on your Discord profile with what you are working on. Jank! talks only to the Discord app on this computer; nothing goes to the internet from here.");
            let mut d = self.settings.discord;
            let mut changed = false;
            changed |= toggle_tip(ui, &mut d.enabled, "Show my activity on Discord", accent(), "Turn the Discord status on or off").changed();
            ui.add_enabled_ui(d.enabled, |ui| {
                changed |= toggle_tip(ui, &mut d.show_project, "Show the project name", accent(), "Off: Discord says \u{201c}Working on a project\u{201d} instead of the name").changed();
                changed |= toggle_tip(ui, &mut d.show_activity, "Show what I am doing", accent(), "The mode and a short detail, e.g. \u{201c}Editing polygons \u{00b7} 96 vertices\u{201d}. Off: \u{201c}Making low-poly art\u{201d}").changed();
                changed |= toggle_tip(ui, &mut d.show_time, "Show the elapsed time", accent(), "Time since the project was opened").changed();
            });
            if changed {
                self.settings.discord = d;
                self.touch_settings();
            }
        });
        section(ui, "Connection", accent(), |ui| {
            let (text, col) = if !self.settings.discord.enabled {
                ("Off".to_string(), DIM())
            } else if self.rpc.app_id.is_none() {
                ("This build has no Discord Application ID yet.".to_string(), WARN())
            } else {
                match self.rpc.conn.as_ref().map(|c| c.status()) {
                    Some(Status::Connected(u)) if !u.is_empty() => (format!("Connected as {u}"), accent()),
                    Some(Status::Connected(_)) => ("Connected".to_string(), accent()),
                    Some(Status::Rejected(why)) => (format!("Discord refused the status: {why}"), DANGER()),
                    Some(Status::Searching) | None => ("Waiting for the Discord app (start it and Jank! connects by itself)".to_string(), DIM()),
                    Some(Status::Stopped) => ("Stopped".to_string(), DIM()),
                }
            };
            ui.horizontal(|ui| {
                crate::widgets::icon_inline(ui, Icon::Presence, col);
                ui.label(egui::RichText::new(text).color(col));
            });
            if let Some(a) = self.rpc.last.as_ref().filter(|_| self.settings.discord.enabled) {
                preview_card(ui, a);
            }
            if Btn::new("Reconnect").icon(Icon::Loop).enabled(self.settings.discord.enabled && self.rpc.app_id.is_some()).tip("Look for the Discord app again now").show(ui).clicked() {
                self.rpc.close();
                self.rpc.checked = Instant::now() - Duration::from_secs(10);
            }
        });
    }
}

/// A small mock-up of the profile card, so the privacy switches can be judged at a glance.
/// Painted into a fixed rectangle so it never changes the size of the Settings window.
fn preview_card(ui: &mut egui::Ui, a: &Activity) {
    use crate::compat;
    use egui::{pos2, vec2, Align2};
    let w = ui.available_width().clamp(240.0, 380.0);
    let lines = if a.start.is_some() { 4.0 } else { 3.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(w, 22.0 + lines * 17.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, compat::cr(6.0), BG0());
    compat::stroke_rect(&p, rect, 6.0, compat::stroke(1.0, LINE()));
    let art = egui::Rect::from_min_size(rect.min + vec2(10.0, 11.0), vec2(56.0, 56.0));
    p.rect_filled(art, compat::cr(8.0), accent());
    crate::icons::draw(&p, Icon::Cube, art.center(), 32.0, BRIGHT());
    if !a.small_image.is_empty() {
        let c = art.right_bottom() - vec2(3.0, 3.0);
        p.circle_filled(c, 11.0, BG0());
        p.circle_filled(c, 9.0, BG3());
        crate::icons::draw(&p, small_icon(&a.small_image), c, 16.0, TEXT());
    }
    let x = art.max.x + 12.0;
    let max_chars = ((rect.max.x - x - 8.0) / 6.6).max(8.0) as usize;
    let fit = |s: &str| if s.chars().count() > max_chars { format!("{}\u{2026}", s.chars().take(max_chars - 1).collect::<String>()) } else { s.to_string() };
    let mut y = rect.min.y + 12.0;
    let mut line = |t: &str, size: f32, col| {
        compat::text(&p, pos2(x, y), Align2::LEFT_TOP, &fit(t), size, col);
        y += 17.0;
    };
    line("Jank!", 13.0, TEXT());
    line(&a.details, 12.0, TEXT());
    line(&a.state, 12.0, DIM());
    if let Some(t) = a.start {
        let secs = discord::unix_now().saturating_sub(t);
        line(&format!("{:02}:{:02}:{:02} elapsed", secs / 3600, secs / 60 % 60, secs % 60), 12.0, DIM());
    }
}

fn small_icon(key: &str) -> Icon {
    use discord::assets as k;
    match key {
        k::MODEL => Icon::Cube,
        k::EDIT => Icon::Vertex,
        k::TEXTURE => Icon::Brush,
        k::POSE => Icon::Bone,
        k::ANIMATE => Icon::Film,
        k::RENDER => Icon::Camera,
        k::EXPORT => Icon::Export,
        _ => Icon::Clock,
    }
}
