//! Dialogs: the built-in file browser, the photo/video output panel, the About window, confirmations and toast notifications.
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{dim, section, slider_f, slider_u, toggle, Btn};
use crate::{compat, App, ToastKind};
use eframe::egui::{self, vec2, Align2, Context, ScrollArea, Ui};
use psxcore::editor::{Canvas, Editor};
use psxcore::export::ImageFormat;
use psxcore::filebrowser::{human_size, sanitize_file_name, unique_path, FileBrowser};
use psxcore::jobs::{ExportJob, ExportSpec, Target, VideoKind};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
pub enum Purpose {
    ImportModel,
    OpenProject,
    SaveProject,
    ImportTexture(Canvas),
    SaveTexture(Canvas),
    /// Load an image into a new scene layer (true = behind the 3D objects).
    ImportSceneLayer(bool),
    ExportFolder,
    /// A reference picture for the 3D view.
    AddReference,
}
#[derive(Clone, Copy, PartialEq)]
pub enum After {
    New,
    Open,
    Quit,
}
pub struct BrowserDlg {
    pub purpose: Purpose,
    pub fb: FileBrowser,
    pub name: String,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AboutTab {
    Overview,
    Keys,
    Licenses,
}

pub enum Dialog {
    Browser(Box<BrowserDlg>),
    About(AboutTab),
    ConfirmDiscard(After),
    Warnings(Vec<String>),
    Palette(crate::commands_ui::PaletteState),
    Settings(crate::commands_ui::SettingsState),
    Recover(psxcore::recovery::Snapshot),
    WhatsNew,
}

fn home_or_cwd() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_dir()).or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn browser(purpose: Purpose) -> Dialog {
    let (filter, name): (&[&str], &str) = match purpose {
        Purpose::ImportModel => (&["obj", "stl", "ply", "gltf", "glb"], ""),
        Purpose::OpenProject => (&["jank"], ""),
        Purpose::SaveProject => (&["jank"], "Project.jank"),
        Purpose::ImportTexture(_) => (&["png", "jpg", "jpeg", "bmp", "tga", "ppm"], ""),
        Purpose::SaveTexture(_) => (&["png"], "texture.png"),
        Purpose::ImportSceneLayer(_) => (&["png", "jpg", "jpeg", "bmp", "tga", "ppm"], ""),
        Purpose::ExportFolder => (&[], ""),
        Purpose::AddReference => (&["png", "jpg", "jpeg", "bmp", "tga", "ppm"], ""),
    };
    let fb = FileBrowser::new(&home_or_cwd()).with_filter(filter);
    Dialog::Browser(Box::new(BrowserDlg { purpose, fb, name: name.to_string() }))
}

// ------------------------------------------------------------------ output settings

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Photo,
    Video,
}
#[derive(Clone, Copy, PartialEq)]
pub enum PhotoFmt {
    Png,
    PngRgb,
    Jpeg,
    Bmp,
    Tga,
    Ppm,
}
#[derive(Clone, Copy, PartialEq)]
pub enum VideoFmt {
    AviQuality,
    AviBitrate,
    AviRaw,
    Gif,
    PngSeq,
}
pub struct ExportCfg {
    pub kind: Kind,
    pub photo: PhotoFmt,
    pub jpeg_q: u32,
    pub video: VideoFmt,
    pub avi_q: u32,
    pub kbps: u32,
    pub gif_loop: bool,
    pub w: u32,
    pub h: u32,
    pub lock: bool,
    pub name: String,
    pub dir: PathBuf,
    pub tick: u32,
}
impl Default for ExportCfg {
    fn default() -> Self {
        Self { kind: Kind::Video, photo: PhotoFmt::Png, jpeg_q: 92, video: VideoFmt::AviQuality, avi_q: 90, kbps: 8000, gif_loop: true, w: 0, h: 0, lock: true, name: "Jank".into(), dir: home_or_cwd(), tick: 0 }
    }
}

fn chip_row<T: Copy + PartialEq>(ui: &mut Ui, cur: &mut T, items: &[(T, &str, &str)]) -> bool {
    let mut ch = false;
    ui.horizontal_wrapped(|ui| {
        for (v, label, tip) in items {
            if Btn::new(label).accent(accent()).on(*cur == *v).tip(tip).size(vec2(22.0 + label.chars().count() as f32 * 7.6, 30.0)).show(ui).clicked() {
                *cur = *v;
                ch = true;
            }
        }
    });
    ch
}

impl App {
    pub fn export_panel(&mut self, ui: &mut Ui) {
        let (nw, nh) = (self.ed.project.scene.style.width, self.ed.project.scene.style.height);
        if self.export.w == 0 || self.export.h == 0 {
            self.export.w = nw * 3;
            self.export.h = nh * 3;
        }
        let (frames, total, fps) = self.ed.animation().map_or((0, 0, 12.0), |a| (a.frames.len(), a.total_ticks(), a.fps()));
        let busy = self.job.is_some();
        section(ui, "Output", accent(), |ui| {
            chip_row(ui, &mut self.export.kind, &[(Kind::Photo, "Photo", "Save a single frame as an image"), (Kind::Video, "Video / animation", "Save the whole animation")]);
        });
        section(ui, "Size", accent(), |ui| {
            ui.horizontal_wrapped(|ui| {
                for k in [1u32, 2, 3, 4, 6, 8] {
                    let on = self.export.w == nw * k && self.export.h == nh * k;
                    if Btn::new(&format!("×{k}")).accent(accent()).on(on).tip("Whole multiple of the render size: pixels stay perfectly square").size(vec2(46.0, 28.0)).show(ui).clicked() {
                        self.export.w = nw * k;
                        self.export.h = nh * k;
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                for (w, h, l) in [(1280u32, 720u32, "720p"), (1920, 1080, "1080p"), (2560, 1440, "1440p"), (3840, 2160, "4K")] {
                    if Btn::new(l).accent(accent()).on(self.export.w == w && self.export.h == h).size(vec2(60.0, 28.0)).show(ui).clicked() {
                        self.export.w = w;
                        self.export.h = h;
                    }
                }
            });
            ui.horizontal(|ui| {
                let (ow, oh) = (self.export.w, self.export.h);
                let mut w = self.export.w;
                let mut h = self.export.h;
                if ui.add(egui::DragValue::new(&mut w).speed(4.0).range(16..=16384)).changed() && self.export.lock && ow > 0 {
                    h = ((w as f64 * oh as f64 / ow as f64).round() as u32).clamp(16, 16384);
                } else if ui.add(egui::DragValue::new(&mut h).speed(4.0).range(16..=16384)).changed() && self.export.lock && oh > 0 {
                    w = ((h as f64 * ow as f64 / oh as f64).round() as u32).clamp(16, 16384);
                }
                self.export.w = w;
                self.export.h = h;
                toggle(ui, &mut self.export.lock, "keep ratio", accent());
            });
            if self.export.w as f32 / self.export.h.max(1) as f32 != nw as f32 / nh.max(1) as f32 {
                dim(ui, "The aspect ratio differs from the scene, so the frame will be stretched.");
            }
        });
        match self.export.kind {
            Kind::Photo => {
                section(ui, "Photo format", accent(), |ui| {
                    chip_row(ui, &mut self.export.photo, &[(PhotoFmt::Png, "PNG", "Lossless, keeps transparency"), (PhotoFmt::PngRgb, "PNG, no alpha", "Lossless, smaller file, no transparency"), (PhotoFmt::Jpeg, "JPEG", "Lossy, small files"), (PhotoFmt::Bmp, "BMP", "Uncompressed bitmap"), (PhotoFmt::Tga, "TGA", "Targa image, common in game pipelines"), (PhotoFmt::Ppm, "PPM", "Plain portable pixmap")]);
                    if self.export.photo == PhotoFmt::Jpeg {
                        slider_u(ui, "quality", &mut self.export.jpeg_q, 1..=100);
                    }
                    let mut t = self.export.tick.min(total.saturating_sub(1));
                    if total > 1 && slider_u(ui, "tick", &mut t, 0..=total - 1) {
                        self.export.tick = t;
                    }
                    if Btn::new("Use current frame").size(vec2(ui.available_width(), 28.0)).show(ui).clicked() {
                        self.export.tick = self.ed.frame_start_tick(self.ed.current_frame);
                    }
                });
            }
            Kind::Video => {
                section(ui, "Video format", accent(), |ui| {
                    chip_row(ui, &mut self.export.video, &[(VideoFmt::AviQuality, "AVI quality", "AVI with Motion-JPEG frames, set by quality"), (VideoFmt::AviBitrate, "AVI bitrate", "AVI with Motion-JPEG frames, aimed at a target bitrate"), (VideoFmt::AviRaw, "AVI raw", "Uncompressed AVI: huge files, no quality loss"), (VideoFmt::Gif, "GIF", "Looping animated GIF"), (VideoFmt::PngSeq, "PNG frames", "A folder of numbered PNG images for any video editor")]);
                    match self.export.video {
                        VideoFmt::AviQuality => {
                            slider_u(ui, "quality", &mut self.export.avi_q, 1..=100);
                        }
                        VideoFmt::AviBitrate => {
                            slider_u(ui, "kbit/s", &mut self.export.kbps, 200..=200_000);
                        }
                        VideoFmt::AviRaw => {
                            let mb = self.export.w as f64 * self.export.h as f64 * 3.0 * total as f64 / 1e6;
                            dim(ui, &format!("Estimated size: {mb:.0} MB"));
                        }
                        VideoFmt::Gif => {
                            toggle(ui, &mut self.export.gif_loop, "Loop forever", accent());
                        }
                        VideoFmt::PngSeq => {}
                    }
                    dim(ui, &format!("{frames} frames, {total} ticks at {fps:.0} fps = {:.1} s", total as f32 / fps.max(1.0)));
                    dim(ui, "Formats: AVI, GIF or PNG frames.");
                });
            }
        }
        section(ui, "File", accent(), |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.export.name).desired_width(150.0));
                dim(ui, &self.ext_label());
            });
            if Btn::new("Folder…").icon(Icon::Open).accent(accent()).size(vec2(ui.available_width(), 30.0)).show(ui).clicked() {
                self.dialog = Some(browser(Purpose::ExportFolder));
            }
            dim(ui, &self.export.dir.display().to_string());
        });
        ui.add_space(6.0);
        if let Some(job) = &self.job {
            ui.add(egui::ProgressBar::new(job.progress()).show_percentage());
            if Btn::new("Cancel").icon(Icon::Trash).accent(DANGER()).size(vec2(ui.available_width(), 34.0)).show(ui).clicked() {
                job.cancel();
            }
        } else if Btn::new("Save").icon(Icon::Export).accent(accent()).enabled(!busy && frames > 0).size(vec2(ui.available_width(), 42.0)).show(ui).clicked() {
            self.start_export();
        }
    }

    fn ext_label(&self) -> String {
        match self.export.kind {
            Kind::Photo => match self.export.photo {
                PhotoFmt::Png | PhotoFmt::PngRgb => ".png",
                PhotoFmt::Jpeg => ".jpg",
                PhotoFmt::Bmp => ".bmp",
                PhotoFmt::Tga => ".tga",
                PhotoFmt::Ppm => ".ppm",
            },
            Kind::Video => match self.export.video {
                VideoFmt::Gif => ".gif",
                VideoFmt::PngSeq => "/ (folder)",
                _ => ".avi",
            },
        }
        .to_string()
    }

    fn start_export(&mut self) {
        let c = &self.export;
        let stem = sanitize_file_name(&c.name);
        let (target, ext) = match c.kind {
            Kind::Photo => {
                let (fmt, ext) = match c.photo {
                    PhotoFmt::Png => (ImageFormat::Png, "png"),
                    PhotoFmt::PngRgb => (ImageFormat::PngRgb, "png"),
                    PhotoFmt::Jpeg => (ImageFormat::Jpeg(c.jpeg_q as u8), "jpg"),
                    PhotoFmt::Bmp => (ImageFormat::Bmp, "bmp"),
                    PhotoFmt::Tga => (ImageFormat::Tga, "tga"),
                    PhotoFmt::Ppm => (ImageFormat::Ppm, "ppm"),
                };
                (Target::Photo { format: fmt, tick: c.tick }, ext)
            }
            Kind::Video => match c.video {
                VideoFmt::AviQuality => (Target::Video(VideoKind::AviQuality(c.avi_q as u8)), "avi"),
                VideoFmt::AviBitrate => (Target::Video(VideoKind::AviBitrate(c.kbps * 1000)), "avi"),
                VideoFmt::AviRaw => (Target::Video(VideoKind::AviRaw), "avi"),
                VideoFmt::Gif => (Target::Video(VideoKind::Gif { loops: if c.gif_loop { 0 } else { 1 } }), "gif"),
                VideoFmt::PngSeq => (Target::Video(VideoKind::PngSequence { alpha: false }), ""),
            },
        };
        // Existing files are never overwritten: a numeric suffix such as " (2)" is added instead.
        let path = unique_path(&c.dir, &stem, ext);
        let spec = ExportSpec { target, width: c.w, height: c.h, path: path.clone() };
        match ExportJob::start(&self.ed.project, self.ed.current_anim, spec) {
            Ok(j) => {
                self.job = Some(j);
                self.info(format!("Saving: {}", path.display()));
            }
            Err(e) => self.error(e),
        }
    }

    // ------------------------------------------------------------------ windows

    pub fn dialogs(&mut self, ctx: &Context) {
        let Some(d) = self.dialog.take() else { return };
        // Esc closes a dialog (same as Cancel), except while a shortcut is being recorded or a recovery decision is pending
        let esc_closes = !matches!(&d, Dialog::Recover(_)) && !matches!(&d, Dialog::Settings(s) if s.capturing.is_some());
        if esc_closes && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Dialog::Palette(_) | Dialog::Settings(_) = &d {
                ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            }
            return;
        }
        self.dialog = match d {
            Dialog::Browser(b) => self.browser_dialog(ctx, b),
            Dialog::About(t) => self.about_dialog(ctx, t),
            Dialog::ConfirmDiscard(a) => self.confirm_dialog(ctx, a),
            Dialog::Warnings(w) => self.warnings_dialog(ctx, w),
            Dialog::Palette(p) => self.palette_dialog(ctx, p),
            Dialog::Settings(s) => self.settings_dialog(ctx, s),
            Dialog::Recover(r) => self.recover_dialog(ctx, r),
            Dialog::WhatsNew => self.whats_new_dialog(ctx),
        };
    }

    fn about_dialog(&mut self, ctx: &Context, mut tab: AboutTab) -> Option<Dialog> {
        let mut open = true;
        egui::Window::new("About Jank!").collapsible(false).resizable(true).default_size(vec2(560.0, 520.0)).default_pos(compat::centered_pos(ctx, vec2(560.0, 520.0))).show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.horizontal(|ui| {
                crate::widgets::logo(ui, 36.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(crate::appinfo::version_string()).size(15.0).strong());
                    dim(ui, &format!("{}  |  format v{}  |  {}", crate::appinfo::TAGLINE, psxcore::project::VERSION, crate::appinfo::platform()));
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (t, name, tip) in [
                    (AboutTab::Overview, "Overview", "What Jank! is and how it is versioned"),
                    (AboutTab::Keys, "Keys", "Keyboard and mouse reference"),
                    (AboutTab::Licenses, "Licenses", "Licences of the open-source libraries built into Jank!"),
                ] {
                    if Btn::new(name).on(tab == t).accent(accent()).tip(tip).size(vec2(110.0, 30.0)).show(ui).clicked() {
                        tab = t;
                    }
                }
            });
            ui.separator();
            match tab {
                AboutTab::Overview => {
                    dim(ui, "Modeling, texture painting, posing, frame-by-frame animation, and video and photo output in one program.");
                    ui.add_space(6.0);
                    section(ui, "Version", accent(), |ui| {
                        dim(ui, "MAJOR.MINOR.PATCH[-channel]+BUILD, for example 0.2.0-alpha+31027.");
                        dim(ui, "Channels are ordered alpha < beta < rc < stable. The build number grows with every build and is the number to quote in a bug report.");
                        dim(ui, "Project files carry their own format number; older files always open, newer ones are refused with a clear message.");
                    });
                    section(ui, "Support", WARN(), |ui| {
                        let report = crate::appinfo::support_report();
                        ui.label(egui::RichText::new(report.as_str()).monospace().size(12.5));
                        if Btn::new("Copy to clipboard").tip("Copy the version and platform text for a bug report").size(vec2(170.0, 30.0)).show(ui).clicked() {
                            ui.ctx().copy_text(report);
                            self.toast("Copied version information", crate::ToastKind::Info);
                        }
                    });
                    section(ui, "Author and contact", accent(), |ui| {
                        dim(ui, crate::appinfo::AUTHOR);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(crate::appinfo::CONTACT).size(13.5).color(accent())).on_hover_text("Questions, licences and security reports");
                            if Btn::new("Copy").tip("Copy the e-mail address").size(vec2(70.0, 24.0)).show(ui).clicked() {
                                ui.ctx().copy_text(crate::appinfo::CONTACT.to_string());
                                self.toast("Copied e-mail address", crate::ToastKind::Info);
                            }
                        });
                        ui.hyperlink_to(crate::appinfo::REPOSITORY, crate::appinfo::REPOSITORY);
                    });
                    dim(ui, crate::appinfo::COPYRIGHT);
                }
                AboutTab::Keys => {
                    dim(ui, "Your current shortcuts. Change them in Settings, tab Shortcuts. Press Ctrl+K to search every command.");
                    egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
                        for c in psxcore::commands::COMMANDS {
                            let l: Vec<String> = self.settings.keymap.chords(c.id).iter().map(|x| x.to_string()).collect();
                            if !l.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.allocate_ui(vec2(230.0, 22.0), |ui| {
                                        ui.set_min_size(vec2(230.0, 22.0));
                                        ui.label(egui::RichText::new(c.title).size(13.5))
                                    });
                                    ui.label(egui::RichText::new(l.join("   or   ")).size(13.5).color(accent()));
                                });
                            }
                        }
                        ui.add_space(6.0);
                        for l in ["Middle or right mouse: orbit the view", "Shift + drag: pan", "Mouse wheel: zoom", "Alt + left mouse: orbit"] {
                            dim(ui, l);
                        }
                    });
                }
                AboutTab::Licenses => {
                    dim(ui, "Jank! includes the open-source libraries below. Every one is used under a permissive licence; none is GPL or AGPL.");
                    egui::ScrollArea::vertical().max_height(340.0).auto_shrink([false, false]).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(crate::appinfo::THIRD_PARTY_LICENSES).monospace().size(11.5)).selectable(true));
                    });
                }
            }
            ui.add_space(6.0);
            if Btn::new("Close").size(vec2(ui.available_width(), 34.0)).tip("Close this window (Esc)").show(ui).clicked() {
                open = false;
            }
        });
        open.then_some(Dialog::About(tab))
    }

    fn warnings_dialog(&mut self, ctx: &Context, w: Vec<String>) -> Option<Dialog> {
        let mut open = true;
        egui::Window::new("Model loaded with warnings").collapsible(false).resizable(false).default_pos(compat::centered_pos(ctx, vec2(440.0, 220.0))).show(ctx, |ui| {
            ui.set_min_width(420.0);
            for l in &w {
                ui.colored_label(WARN(), l.as_str());
            }
            if Btn::new("OK").size(vec2(ui.available_width(), 34.0)).show(ui).clicked() {
                open = false;
            }
        });
        open.then_some(Dialog::Warnings(w))
    }

    fn confirm_dialog(&mut self, ctx: &Context, after: After) -> Option<Dialog> {
        let mut result = Some(Dialog::ConfirmDiscard(after));
        egui::Window::new("Unsaved changes").collapsible(false).resizable(false).default_pos(compat::centered_pos(ctx, vec2(380.0, 150.0))).show(ctx, |ui| {
            ui.set_min_width(380.0);
            dim(ui, "This project has changes that have not been saved.");
            ui.horizontal(|ui| {
                if Btn::new("Save").icon(Icon::Save).accent(accent()).show(ui).clicked() {
                    if self.ed.save().is_ok() {
                        result = None;
                        self.proceed(ctx, after);
                    } else {
                        result = Some(browser(Purpose::SaveProject));
                    }
                }
                if Btn::new("Discard").accent(DANGER()).show(ui).clicked() {
                    result = None;
                    self.proceed(ctx, after);
                }
                if Btn::new("Cancel").show(ui).clicked() {
                    result = None;
                }
            });
        });
        result
    }
    fn proceed(&mut self, ctx: &Context, after: After) {
        match after {
            After::New => self.ed.new_project("Untitled"),
            After::Open => self.dialog = Some(browser(Purpose::OpenProject)),
            After::Quit => {
                self.allow_close = true;
                compat::close(ctx);
            }
        }
    }

    fn browser_dialog(&mut self, ctx: &Context, mut b: Box<BrowserDlg>) -> Option<Dialog> {
        let mut close = false;
        let mut chosen: Option<PathBuf> = None;
        let title = match b.purpose {
            Purpose::ImportModel => "Import model",
            Purpose::OpenProject => "Open project",
            Purpose::SaveProject => "Save project",
            Purpose::ImportTexture(_) => "Load image into texture",
            Purpose::SaveTexture(_) => "Save texture",
            Purpose::ImportSceneLayer(_) => "Load image into a new layer",
            Purpose::ExportFolder => "Output folder",
            Purpose::AddReference => "Add reference picture",
        };
        let saving = matches!(b.purpose, Purpose::SaveProject | Purpose::SaveTexture(_));
        let folder = b.purpose == Purpose::ExportFolder;
        egui::Window::new(title).collapsible(false).resizable(true).default_size(vec2(760.0, 520.0)).default_pos(compat::centered_pos(ctx, vec2(760.0, 520.0))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if Btn::new("").icon(Icon::StepBack).tip("Go to the parent folder").size(vec2(34.0, 30.0)).show(ui).clicked() {
                    b.fb.up();
                }
                for (name, path) in b.fb.breadcrumbs() {
                    if ui.small_button(name).clicked() {
                        b.fb.set_dir(&path);
                        break;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut b.fb.query).hint_text("search").desired_width(180.0));
                toggle(ui, &mut b.fb.show_hidden, "hidden", accent());
            });
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_width(140.0);
                    for (name, path) in FileBrowser::places() {
                        if Btn::new(&name).accent(accent()).size(vec2(136.0, 30.0)).show(ui).clicked() {
                            b.fb.set_dir(&path);
                        }
                    }
                });
                ui.vertical(|ui| {
                    if let Some(e) = &b.fb.error {
                        ui.colored_label(DANGER(), e.as_str());
                    }
                    let mut go: Option<String> = None;
                    let mut pick: Option<String> = None;
                    ScrollArea::vertical().auto_shrink([false, false]).max_height(330.0).show(ui, |ui| {
                        for e in b.fb.visible() {
                            let label = if e.is_dir { format!("[dir]  {}", e.name) } else { format!("{}    {}", e.name, human_size(e.size)) };
                            let r = ui.selectable_label(b.name == e.name && !e.is_dir, label);
                            if e.is_dir && r.clicked() {
                                go = Some(e.name.clone());
                            } else if !e.is_dir {
                                if r.double_clicked() {
                                    go = Some(e.name.clone());
                                } else if r.clicked() {
                                    pick = Some(e.name.clone());
                                }
                            }
                        }
                    });
                    if let Some(n) = pick {
                        b.name = n;
                    }
                    if let Some(n) = go {
                        if let Some(p) = b.fb.activate(&n) {
                            if !saving {
                                chosen = Some(p);
                            } else {
                                b.name = n;
                            }
                        }
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if !folder {
                    ui.add(egui::TextEdit::singleline(&mut b.name).hint_text("file name").desired_width(300.0));
                }
                let target = b.fb.dir.join(&b.name);
                let exists = saving && target.exists();
                let label = if folder { "Select this folder" } else if saving { if exists { "Replace" } else { "Save" } } else { "Open" };
                let ok = folder || !b.name.trim().is_empty();
                if Btn::new(label).icon(if saving || folder { Icon::Save } else { Icon::Open }).accent(if exists { WARN() } else { accent() }).enabled(ok).show(ui).clicked() {
                    chosen = Some(if folder { b.fb.dir.clone() } else { target });
                }
                if Btn::new("Cancel").show(ui).clicked() {
                    close = true;
                }
            });
        });
        if let Some(path) = chosen {
            self.finish_browse(b.purpose, &path);
            return None;
        }
        (!close).then_some(Dialog::Browser(b))
    }

    fn finish_browse(&mut self, purpose: Purpose, path: &Path) {
        match purpose {
            Purpose::ImportModel => match self.ed.import_path(path) {
                Ok(w) if w.is_empty() => {}
                Ok(w) => self.dialog = Some(Dialog::Warnings(w)),
                Err(e) => self.error(e),
            },
            Purpose::OpenProject => match Editor::open(path) {
                Ok(ed) => {
                    self.ed = ed;
                    self.info("Project opened");
                }
                Err(e) => self.error(e),
            },
            Purpose::SaveProject => {
                let mut p = path.to_path_buf();
                if p.extension().and_then(|e| e.to_str()) != Some("jank") {
                    p.set_extension("jank");
                }
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    self.ed.project.name = stem.to_string();
                }
                match self.ed.save_as(&p) {
                    Ok(()) => self.info("Project saved"),
                    Err(e) => self.error(e),
                }
            }
            Purpose::ImportTexture(c) => match self.ed.import_image_into(c, path) {
                Ok(()) => self.info("Texture loaded"),
                Err(e) => self.error(e),
            },
            Purpose::ImportSceneLayer(behind) => match self.ed.import_image_as_scene_layer(path, behind) {
                Ok(_) => self.info("Image added as a layer"),
                Err(e) => self.error(e),
            },
            Purpose::SaveTexture(c) => {
                let mut p = path.to_path_buf();
                if p.extension().is_none() {
                    p.set_extension("png");
                }
                match self.ed.export_texture_png(c, &p) {
                    Ok(()) => self.info(format!("Texture saved: {}", p.display())),
                    Err(e) => self.error(e),
                }
            }
            Purpose::ExportFolder => self.export.dir = path.to_path_buf(),
            Purpose::AddReference => match self.ed.ref_add_path(path) {
                Ok(_) => self.info("Reference picture added (it is never rendered)"),
                Err(e) => self.error(e),
            },
        }
    }

    pub fn toasts_ui(&mut self, ctx: &Context, dt: f32) {
        for t in &mut self.toasts {
            t.age += dt;
        }
        self.toasts.retain(|t| t.age < if t.kind == ToastKind::Error { 9.0 } else { 4.5 });
        if !self.toasts.is_empty() {
            ctx.request_repaint();
        }
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("jank_toasts")).order(egui::Order::Tooltip).interactable(false).anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -16.0)).show(ctx, |ui| {
            ui.set_max_width(400.0);
            for t in &self.toasts {
                let col = if t.kind == ToastKind::Error { DANGER() } else { accent() };
                compat::frame(BG2(), 12.0).show(ui, |ui| {
                    ui.set_max_width(376.0);
                    ui.colored_label(col, t.text.as_str());
                });
                ui.add_space(6.0);
            }
        });
    }
}

#[allow(dead_code)]
fn _unused(_: f32) {
    let _ = slider_f;
}
