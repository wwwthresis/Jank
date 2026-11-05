//! Everything that turns the command catalogue into behaviour: running commands, keyboard
//! shortcuts from the user's keymap, the command palette, the settings window, periodic
//! autosave and the crash-recovery prompt.

use crate::dialogs::{self, AboutTab, Dialog, Purpose};
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{dim, section, toggle_tip, Btn};
use crate::{compat, App, ToastKind};
use eframe::egui::{self, vec2, Align2, Color32, Context, Key, Modifiers, Sense};
use psxcore::commands::{self, Category, CommandId, COMMANDS};
use psxcore::editor::{Mode, Primitive};
use psxcore::recovery::{self, Snapshot};
use psxcore::settings::{self, Settings, UI_SCALE_MAX, UI_SCALE_MIN};
use std::time::{Duration, Instant};

/// State of the open command palette.
#[derive(Default)]
pub struct PaletteState {
    pub query: String,
    pub selected: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsTab {
    Appearance,
    Interface,
    Viewport,
    Canvas,
    Files,
    Discord,
    Shortcuts,
}

/// State of the open settings window.
pub struct SettingsState {
    pub tab: SettingsTab,
    /// The command whose shortcut is being recorded, if any.
    pub capturing: Option<CommandId>,
    pub filter: String,
    /// Last result of a shortcut change, shown under the list.
    pub note: String,
    /// Scale chosen with the slider; applied by the Apply button so the slider does not move under the mouse.
    pub scale_edit: f32,
    /// Appearance generation the window was laid out with (see [`Housekeeping::look_epoch`]).
    pub look_epoch: u32,
    /// Frames left in which the window is put back in the centre after an appearance change.
    pub recenter: u8,
    /// Where the window is; it is pinned there each frame (egui would otherwise throw a fixed-size
    /// window to a screen edge when its contents are re-measured) and only moves by dragging.
    pub pos: Option<egui::Pos2>,
}

impl SettingsState {
    pub fn new(scale: f32) -> Self {
        SettingsState { tab: SettingsTab::Appearance, capturing: None, filter: String::new(), note: String::new(), scale_edit: scale, look_epoch: 0, recenter: 0, pos: None }
    }
}

/// Persistent-state bookkeeping that lives inside [`App`].
pub struct Housekeeping {
    pub settings_dirty: bool,
    pub settings_changed: Instant,
    pub last_autosave: Instant,
    pub snapshot_written: bool,
    pub autosave_error_shown: bool,
    /// The appearance last pushed to egui; re-applied only when the settings differ.
    pub look: Option<crate::theme::Look>,
    /// Counts appearance changes, so open windows can react to new metrics.
    pub look_epoch: u32,
}

impl Housekeeping {
    pub fn new() -> Self {
        Housekeeping { settings_dirty: false, settings_changed: Instant::now(), last_autosave: Instant::now(), snapshot_written: false, autosave_error_shown: false, look: None, look_epoch: 0 }
    }
}

/// Seconds between snapshots. `JANK_AUTOSAVE_SECONDS` overrides the setting (used by automated tests).
fn autosave_interval(s: &Settings) -> Duration {
    if let Some(v) = std::env::var("JANK_AUTOSAVE_SECONDS").ok().and_then(|v| v.parse::<u64>().ok()) {
        return Duration::from_secs(v.max(1));
    }
    Duration::from_secs(s.autosave_minutes as u64 * 60)
}

/// Loads the settings file and returns the problems found, to be shown as a warning.
pub fn load_settings() -> (Settings, Vec<String>) {
    match settings::settings_path() {
        Some(p) => Settings::load_from(&p),
        None => (Settings::default(), vec![]),
    }
}

impl App {
    // ------------------------------------------------------------------ commands

    /// Text for a tooltip that names the current shortcut of a command, e.g. "Save project (Ctrl+S)".
    pub fn tip_for(&self, id: CommandId, text: &str) -> String {
        let l = self.settings.keymap.label(id);
        if l.is_empty() {
            text.to_string()
        } else {
            format!("{text} ({l})")
        }
    }

    fn need_selection(&mut self, ok: bool) {
        if !ok {
            self.info("Nothing changed: select a model in the scene first");
        }
    }

    /// Runs one command. Every button, menu entry, palette hit and shortcut ends up here.
    pub fn run_command(&mut self, ctx: &Context, id: CommandId) {
        use CommandId::*;
        if self.run_mesh_command(id) {
            return;
        }
        match id {
            NewProject => self.new_project(),
            OpenProject => self.dialog = Some(dialogs::browser(Purpose::OpenProject)),
            OpenStarter => self.open_starter(),
            SaveProject => self.save(),
            SaveProjectAs => self.dialog = Some(dialogs::browser(Purpose::SaveProject)),
            ImportModel => self.dialog = Some(dialogs::browser(Purpose::ImportModel)),
            ExportOutput => self.ed.set_mode(Mode::Render),
            Quit => compat::close(ctx),
            Undo => {
                self.ed.undo();
            }
            Redo => {
                self.ed.redo();
            }
            DeleteSelected => self.delete_selected(),
            ModeModel => self.ed.set_mode(Mode::Model),
            ModeTexture => self.ed.set_mode(Mode::Texture),
            ModePose => self.ed.set_mode(Mode::Pose),
            ModeAnimate => self.ed.set_mode(Mode::Animate),
            ModeRender => self.ed.set_mode(Mode::Render),
            AddCube => {
                self.ed.add_primitive(Primitive::Cube);
            }
            AddButterfly => {
                self.ed.add_primitive(Primitive::Butterfly);
            }
            Subdivide => {
                let ok = self.ed.subdivide_selected();
                self.need_selection(ok);
            }
            MirrorX => {
                let ok = self.ed.mirror_x_selected();
                self.need_selection(ok);
            }
            WeldVertices => {
                let ok = self.ed.weld_selected();
                self.need_selection(ok);
            }
            FlatShade => {
                let ok = self.ed.flat_shade_selected();
                self.need_selection(ok);
            }
            AutoUv => {
                let ok = self.ed.unwrap_selected();
                self.need_selection(ok);
            }
            TogglePlay => self.ed.toggle_play(),
            PrevFrame => self.ed.step_frame(-1),
            NextFrame => self.ed.step_frame(1),
            AddFrame => {
                self.ed.add_frame();
            }
            DuplicateFrame => {
                self.ed.duplicate_frame();
            }
            FrameAll => self.ed.frame_all(),
            TogglePreview => {
                self.settings.preview.visible = !self.settings.preview.visible;
                self.touch_settings();
            }
            ToggleStats => {
                self.settings.show_stats = !self.settings.show_stats;
                self.touch_settings();
            }
            GizmoSelect => self.gizmo.mode = None,
            GizmoMove => self.gizmo.mode = Some(psxcore::gizmo::GizmoMode::Translate),
            GizmoRotate => self.gizmo.mode = Some(psxcore::gizmo::GizmoMode::Rotate),
            GizmoScale => self.gizmo.mode = Some(psxcore::gizmo::GizmoMode::Scale),
            GizmoToggleSpace => self.gizmo.local = !self.gizmo.local,
            GizmoToggleSnap => self.gizmo.snap = !self.gizmo.snap,
            CommandPalette => self.dialog = Some(Dialog::Palette(PaletteState::default())),
            OpenSettings => self.dialog = Some(Dialog::Settings(SettingsState::new(self.settings.ui_scale))),
            OpenAbout => self.dialog = Some(Dialog::About(AboutTab::Overview)),
            OpenLicenses => self.dialog = Some(Dialog::About(AboutTab::Licenses)),
            WhatsNew => self.dialog = Some(Dialog::WhatsNew),
            RadialMenu => self.radial_open(ctx),
            EditModeToggle => {
                let was = self.ed.edit_active();
                let now = self.ed.edit_toggle();
                self.edui.tool = crate::editui::EditTool::Select;
                self.edui.boxing = None;
                self.edui.press = None;
                if !was && !now {
                    let s = self.ed.status.clone();
                    self.info(if s.is_empty() { "Select an object to edit its mesh".to_string() } else { s });
                }
            }
            BooleanUnion => self.run_boolean(psxcore::editmesh::BoolOp::Union),
            BooleanDifference => self.run_boolean(psxcore::editmesh::BoolOp::Difference),
            BooleanIntersect => self.run_boolean(psxcore::editmesh::BoolOp::Intersect),
            AddReference => self.dialog = Some(dialogs::browser(Purpose::AddReference)),
            ToggleIk => {
                self.ed.ik_chain = if self.ed.ik_chain > 0 { 0 } else { self.ik_len.max(1) };
                self.info(if self.ed.ik_chain > 0 { format!("IK on: dragging a joint bends {} bone(s) above it", self.ed.ik_chain) } else { "IK off".to_string() });
            }
            ToolBrush | ToolEraser | ToolFill | ToolGradient | ToolPicker | ToolSmudge | ToolBlur | SwapColors | BrushSmaller | BrushBigger | ToggleSymmetry | ToggleProjectionPaint => self.run_paint_command(id),
            // Edit-mode commands were handled by `run_mesh_command` above
            _ => {}
        }
    }

    /// Keyboard shortcuts, read from the user's keymap. Nothing fires while a dialog is open or
    /// while a text field has the keyboard.
    /// Delete key: removes what the user most likely means, depending on the mode and on what is under the mouse.
    pub fn delete_selected(&mut self) {
        match self.ed.mode {
            Mode::Texture => {
                let target = self.ed.selection.as_ref().and(self.ed.selection_canvas);
                match target {
                    Some(c) if self.ed.selection_delete(c) => {}
                    _ => self.info("Nothing selected: select pixels first"),
                }
            }
            Mode::Model => {
                if self.ed.edit_active() {
                    self.ed.edit_delete_selected();
                } else if !self.ed.selected_faces.is_empty() {
                    self.ed.delete_selected_faces();
                } else if let Some(id) = self.ed.selected_object {
                    self.ed.delete_object(id);
                } else {
                    self.info("Nothing selected");
                }
            }
            Mode::Pose => self.info("Bones belong to the model and cannot be deleted here"),
            Mode::Animate => {
                let frames = self.ed.animation().map_or(0, |a| a.frames.len());
                if self.hover_timeline && frames > 1 {
                    self.ed.delete_frame();
                } else if let Some(id) = self.ed.selected_object {
                    self.ed.delete_object(id);
                } else {
                    self.info("Nothing selected");
                }
            }
            Mode::Render => {}
        }
    }

    pub fn shortcuts(&mut self, ctx: &Context) {
        if self.dialog.is_some() || ctx.wants_keyboard_input() || self.radial.is_some() {
            self.tab_chords.clear();
            return;
        }
        // a running Edit-mode tool owns the keyboard (axis locks, typed values, Enter / Esc)
        if self.edit_modal_keys(ctx) {
            return;
        }
        use psxcore::commands::Context as Cx;
        let mut active = vec![Cx::Global];
        if self.ed.edit_active() {
            active.push(Cx::MeshEdit);
        }
        if self.painting() {
            active.push(Cx::Paint);
        }
        let mut chords = std::mem::take(&mut self.tab_chords);
        chords.extend(compat::pressed_chords(ctx));
        for chord in chords {
            for id in self.settings.keymap.commands_in(&chord, &active) {
                self.run_command(ctx, id);
            }
        }
    }

    // ------------------------------------------------------------------ settings and housekeeping

    /// Marks the settings as changed; they are written to disk a moment later.
    pub fn touch_settings(&mut self) {
        self.settings.clamp();
        self.hk.settings_dirty = true;
        self.hk.settings_changed = Instant::now();
    }

    pub fn save_settings(&mut self) {
        self.hk.settings_dirty = false;
        if let Some(p) = settings::settings_path() {
            if let Err(e) = self.settings.save_to(&p) {
                self.error(format!("Could not save settings: {e}"));
            }
        }
    }

    /// Records a panel size; called each frame by the panel code.
    pub fn track_width(&mut self, which: crate::Panel, w: f32) {
        let l = &mut self.settings.layout;
        let slot = match which {
            crate::Panel::Left => &mut l.left_width,
            crate::Panel::Right => &mut l.right_width,
            crate::Panel::Texture => &mut l.texture_width,
            crate::Panel::Timeline => &mut l.timeline_height,
        };
        if (*slot - w).abs() > 1.5 && !self.layout_reset {
            *slot = w;
            self.touch_settings();
        }
    }

    /// Pushes the appearance settings (accent, density, text size) to egui when they changed.
    /// Called after the frame's UI, so a whole frame is always laid out with one set of metrics.
    pub fn sync_look(&mut self, ctx: &Context) {
        let t = self.current_theme();
        let look = crate::theme::Look { accent: t.accent.unwrap_or(self.settings.accent.rgb()), density: self.settings.density, text: self.settings.text_size, colors: t.colors, light: t.light };
        if self.hk.look != Some(look) {
            crate::theme::apply_look(ctx, look);
            self.hk.look = Some(look);
            self.hk.look_epoch = self.hk.look_epoch.wrapping_add(1);
            ctx.request_repaint();
        }
    }

    /// Per-frame work: preferences, UI scale, delayed settings save, autosave.
    pub fn housekeeping(&mut self, ctx: &Context) {
        crate::widgets::set_prefs(self.settings.reduce_motion, self.settings.show_tooltips);
        compat::set_scale(ctx, self.settings.ui_scale);
        if self.hk.look.is_none() {
            self.sync_look(ctx);
        }
        if self.hk.settings_dirty && self.hk.settings_changed.elapsed() > Duration::from_millis(1500) && !ctx.input(|i| i.pointer.any_down()) {
            self.save_settings();
        }
        if self.hk.settings_dirty {
            ctx.request_repaint_after(Duration::from_millis(1600));
        }
        let Some(dir) = recovery::recovery_dir() else { return };
        // The recovery prompt owns the snapshot until the user decides.
        if matches!(self.dialog, Some(Dialog::Recover(_))) {
            return;
        }
        if !self.ed.dirty {
            if self.hk.snapshot_written {
                recovery::clear(&dir);
                self.hk.snapshot_written = false;
            }
            self.hk.last_autosave = Instant::now();
            return;
        }
        if self.settings.autosave_enabled && self.hk.last_autosave.elapsed() >= autosave_interval(&self.settings) && !ctx.input(|i| i.pointer.any_down()) {
            self.hk.last_autosave = Instant::now();
            match recovery::write_snapshot(&dir, &self.ed) {
                Ok(()) => self.hk.snapshot_written = true,
                Err(e) => {
                    if !self.hk.autosave_error_shown {
                        self.hk.autosave_error_shown = true;
                        self.error(format!("Autosave failed: {e}"));
                    }
                }
            }
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    /// Offers a leftover snapshot from a session that did not end normally.
    pub fn check_recovery_at_start(&mut self) {
        if let Some(dir) = recovery::recovery_dir() {
            if let Some(s) = recovery::find_snapshot(&dir) {
                self.dialog = Some(Dialog::Recover(s));
            }
        }
    }

    /// Clean shutdown: remove the snapshot and flush the settings.
    pub fn shutdown(&mut self) {
        if let Some(dir) = recovery::recovery_dir() {
            if !matches!(self.dialog, Some(Dialog::Recover(_))) {
                recovery::clear(&dir);
            }
        }
        self.save_settings();
    }

    pub fn recover_dialog(&mut self, ctx: &Context, s: Snapshot) -> Option<Dialog> {
        let mut result = Some(Dialog::Recover(s.clone()));
        egui::Window::new("Recover unsaved work").collapsible(false).resizable(false).default_pos(compat::centered_pos(ctx, vec2(420.0, 170.0))).show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.horizontal(|ui| {
                crate::widgets::icon_inline(ui, Icon::Warn, WARN());
                ui.label(egui::RichText::new("Jank! did not close properly last time.").size(15.0).color(WARN()));
            });
            let what = s.original.as_ref().map(|p| format!("\"{}\"", p.display())).unwrap_or_else(|| "an unsaved project".to_string());
            dim(ui, &format!("A snapshot of {what} was saved {}. You can restore it now.", recovery::age_text(s.unix_time, recovery::unix_now())));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if Btn::new("Recover").icon(Icon::Open).accent(accent()).tip("Open the snapshot as an unsaved project").show(ui).clicked() {
                    match recovery::restore(&s) {
                        Ok(ed) => {
                            self.ed = ed;
                            self.hk.snapshot_written = true;
                            self.info("Recovered the autosaved project. Save it to keep it");
                        }
                        Err(e) => self.error(e),
                    }
                    result = None;
                }
                if Btn::new("Discard").accent(DANGER()).tip("Delete the snapshot permanently").show(ui).clicked() {
                    if let Some(dir) = recovery::recovery_dir() {
                        recovery::clear(&dir);
                    }
                    result = None;
                }
            });
        });
        result
    }

    // ------------------------------------------------------------------ command palette

    pub fn palette_dialog(&mut self, ctx: &Context, mut st: PaletteState) -> Option<Dialog> {
        let hits = commands::search(&st.query);
        let n = hits.len();
        let (up, down, enter) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Enter)));
        if n > 0 {
            if down {
                st.selected = (st.selected + 1) % n;
            }
            if up {
                st.selected = (st.selected + n - 1) % n;
            }
            st.selected = st.selected.min(n - 1);
        } else {
            st.selected = 0;
        }
        let mut run: Option<CommandId> = None;
        let mut close = false;
        if enter {
            run = hits.get(st.selected).map(|h| h.id);
            close = true;
        }
        let km = self.settings.keymap.clone();
        let moved = ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        egui::Window::new("command_palette").title_bar(false).resizable(false).collapsible(false).anchor(Align2::CENTER_TOP, vec2(0.0, 80.0)).show(ctx, |ui| {
            ui.set_width(600.0);
            ui.horizontal(|ui| {
                crate::widgets::icon_inline(ui, Icon::Search, accent());
                let r = ui.add(egui::TextEdit::singleline(&mut st.query).hint_text("Type a command, for example \"save\" or \"mirror\"").desired_width(f32::INFINITY).font(egui::FontId::proportional(16.0)));
                r.request_focus();
                if r.changed() {
                    st.selected = 0;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                if hits.is_empty() {
                    dim(ui, "No command matches. Try a shorter word.");
                }
                for (i, h) in hits.iter().enumerate() {
                    let info = h.id.info();
                    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
                    if r.hovered() && moved {
                        st.selected = i;
                    }
                    let sel = st.selected == i;
                    if sel {
                        ui.painter().rect_filled(rect, compat::cr(7.0), BG3());
                        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + vec2(0.0, 6.0), vec2(3.0, rect.height() - 12.0)), compat::cr(2.0), accent());
                        if up || down {
                            ui.scroll_to_rect(rect, None);
                        }
                    }
                    compat::text(ui.painter(), rect.min + vec2(14.0, 14.0), Align2::LEFT_CENTER, info.title, 15.0, if sel { TEXT() } else { mix(TEXT(), DIM(), 0.25) });
                    compat::text(ui.painter(), rect.min + vec2(14.0, 31.0), Align2::LEFT_CENTER, info.description, 11.5, DIM());
                    let label = km.label(h.id);
                    compat::text(ui.painter(), rect.right_center() + vec2(-12.0, -9.0), Align2::RIGHT_CENTER, info.category.title(), 11.5, DIM());
                    if !label.is_empty() {
                        compat::text(ui.painter(), rect.right_center() + vec2(-12.0, 9.0), Align2::RIGHT_CENTER, &label, 12.5, accent());
                    }
                    if r.clicked() {
                        run = Some(h.id);
                        close = true;
                    }
                }
            });
            ui.separator();
            dim(ui, "Up and Down choose, Enter runs, Esc closes");
        });
        if let Some(id) = run {
            self.pending = Some(id);
        }
        if close {
            None
        } else {
            Some(Dialog::Palette(st))
        }
    }

    // ------------------------------------------------------------------ settings window

    pub fn settings_dialog(&mut self, ctx: &Context, mut st: SettingsState) -> Option<Dialog> {
        if let Some(id) = st.capturing {
            for chord in compat::pressed_chords(ctx) {
                if chord.key == "Escape" && chord.is_bare() {
                    st.capturing = None;
                    st.note = "Cancelled".to_string();
                    break;
                }
                let stolen = self.settings.keymap.assign_exclusive(id, chord.clone());
                st.note = if stolen.is_empty() {
                    format!("{} is now {}", id.title(), chord)
                } else {
                    let names: Vec<&str> = stolen.iter().map(|c| c.title()).collect();
                    format!("{} is now {}. The shortcut was taken from: {}", id.title(), chord, names.join(", "))
                };
                st.capturing = None;
                self.touch_settings();
                break;
            }
        }
        let mut open = true;
        // New spacing or text sizes make egui re-measure the window, which can throw it to a screen
        // edge; for a few frames after such a change it is put back in the centre.
        if st.look_epoch != self.hk.look_epoch {
            st.look_epoch = self.hk.look_epoch;
            st.recenter = 3;
        }
        let size = vec2(760.0, 560.0);
        let mut win = egui::Window::new("Settings").collapsible(false).resizable(false).fixed_size(vec2(740.0, 520.0)).default_pos(compat::centered_pos(ctx, size));
        if st.recenter > 0 {
            st.recenter -= 1;
            st.pos = None;
            ctx.request_repaint();
        }
        let pos = st.pos.unwrap_or_else(|| compat::centered_pos(ctx, size));
        win = win.current_pos(pos);
        let shown = win.show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                // Obsidian-style navigation column
                ui.allocate_ui_with_layout(vec2(168.0, 470.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(egui::RichText::new("OPTIONS").size(10.5).color(DIM()).family(egui::FontFamily::Name("strong".into())));
                    ui.add_space(2.0);
                    for (t, icon, name, tip) in [
                        (SettingsTab::Appearance, Icon::Palette, "Appearance", "Accent colour, density, text size, toolbar and status bar"),
                        (SettingsTab::Interface, Icon::Monitor, "Interface", "Scale, comfort and accessibility"),
                        (SettingsTab::Viewport, Icon::Cube, "Viewport", "Drawing quality, shadows, navigation and the render preview window"),
                        (SettingsTab::Canvas, Icon::Onion, "Canvas & animation", "Pixel grid of the texture editor and onion-skin strength"),
                        (SettingsTab::Files, Icon::Folder, "Files", "Autosave, recovery and recent files"),
                        (SettingsTab::Discord, Icon::Presence, "Discord", "Your activity on Discord: on or off, project name and details"),
                        (SettingsTab::Shortcuts, Icon::Keyboard, "Hotkeys", "Change the keyboard shortcut of any command"),
                    ] {
                        if crate::widgets::nav_item(ui, icon, name, st.tab == t, tip).clicked() {
                            st.tab = t;
                        }
                    }
                    // the build sits at the foot of the column (fixed offset: the window has a fixed size)
                    ui.add_space(470.0 - 24.0 - 6.0 * 29.0 - 22.0);
                    ui.label(egui::RichText::new(format!("Jank! {}", crate::appinfo::version_string())).size(11.0).color(alpha(DIM(), 170)));
                });
                let (r, _) = ui.allocate_exact_size(vec2(1.0, 470.0), egui::Sense::hover());
                ui.painter().rect_filled(r, compat::cr(0.0), LINE_SOFT());
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().id_salt("settings-body").max_height(430.0).auto_shrink([false, false]).show(ui, |ui| match st.tab {
                        SettingsTab::Appearance => self.settings_appearance(ui),
                        SettingsTab::Interface => self.settings_interface(ui, &mut st),
                        SettingsTab::Viewport => self.settings_viewport(ui),
                        SettingsTab::Canvas => self.settings_canvas(ui),
                        SettingsTab::Files => self.settings_files(ui),
                        SettingsTab::Discord => self.settings_discord(ui),
                        SettingsTab::Shortcuts => self.settings_shortcuts(ui, &mut st),
                    });
                    ui.add_space(6.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Btn::new("Done").accent(accent()).on(true).size(vec2(110.0, 32.0)).tip("Close the settings (Esc). Changes are already saved").show(ui).clicked() {
                            open = false;
                        }
                        dim(ui, "Changes apply at once and are saved automatically.");
                    });
                });
            });
        });
        if let Some(r) = shown {
            let delta = if r.response.dragged() { r.response.drag_delta() } else { vec2(0.0, 0.0) };
            let screen = compat::screen_rect(ctx);
            let p = pos + delta;
            // keep the title bar reachable
            st.pos = Some(egui::pos2(p.x.clamp(screen.min.x - size.x + 80.0, screen.max.x - 80.0), p.y.clamp(screen.min.y, screen.max.y - 40.0)));
        }
        open.then_some(Dialog::Settings(st))
    }

    /// One row of buttons for a [`settings::Choice`]; returns true when the value changed.
    fn choice_row<T: settings::Choice>(&mut self, ui: &mut egui::Ui, v: &mut T, tip: &str) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for &c in T::ALL {
                if Btn::new(c.label()).accent(accent()).on(*v == c).tip(tip).show(ui).clicked() && *v != c {
                    *v = c;
                    changed = true;
                }
            }
        });
        if changed {
            self.touch_settings();
        }
        changed
    }

    fn settings_appearance(&mut self, ui: &mut egui::Ui) {
        use psxcore::settings::{Accent, Choice};
        self.themes_section(ui);
        section(ui, "Accent colour", accent(), |ui| {
            dim(ui, "The one colour that marks the active mode, tool, tab and selection.");
            ui.horizontal(|ui| {
                for a in Accent::ALL {
                    let [r, g, b] = a.rgb();
                    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), egui::Sense::click());
                    let p = ui.painter();
                    let on = self.settings.accent == *a;
                    p.rect_filled(rect.shrink(4.0), compat::cr(RADIUS + 1.0), Color32::from_rgb(r, g, b));
                    if on || resp.hovered() {
                        compat::stroke_rect(p, rect.shrink(1.0), RADIUS + 3.0, compat::stroke(if on { 2.0 } else { 1.0 }, if on { BRIGHT() } else { DIM() }));
                    }
                    if crate::widgets::with_tip(resp, a.label()).clicked() {
                        self.settings.accent = *a;
                        self.touch_settings();
                    }
                }
                ui.add_space(6.0);
                dim(ui, self.settings.accent.label());
            });
        });
        section(ui, "Density and text", accent(), |ui| {
            dim(ui, "Compact fits more controls on small screens; Comfortable adds breathing room. Text size is independent of the interface scale.");
            let mut d = self.settings.density;
            if self.choice_row(ui, &mut d, "Spacing of the controls") {
                self.settings.density = d;
            }
            let mut t = self.settings.text_size;
            if self.choice_row(ui, &mut t, "Size of the interface text") {
                self.settings.text_size = t;
            }
        });
        section(ui, "Workspace", accent(), |ui| {
            let mut v = self.settings.show_toolbar;
            if toggle_tip(ui, &mut v, "Tool strip next to the viewport", accent(), "The column of pixel-icon tools between the panels and the 3D view").clicked() {
                self.settings.show_toolbar = v;
                self.touch_settings();
            }
            let mut v = self.settings.show_status_bar;
            if toggle_tip(ui, &mut v, "Status bar", accent(), "The line at the bottom with messages, the selection and the build").clicked() {
                self.settings.show_status_bar = v;
                self.touch_settings();
            }
            let mut v = self.settings.status_fps;
            if toggle_tip(ui, &mut v, "Frame rate in the status bar", accent(), "Frames per second and the viewport drawing time").clicked() {
                self.settings.status_fps = v;
                self.touch_settings();
            }
        });
    }

    fn settings_canvas(&mut self, ui: &mut egui::Ui) {
        section(ui, "Texture editor", accent(), |ui| {
            let mut v = self.settings.pixel_grid;
            if toggle_tip(ui, &mut v, "Pixel grid when zoomed in", accent(), "Thin lines between texture pixels from 800% zoom").clicked() {
                self.settings.pixel_grid = v;
                self.touch_settings();
            }
        });
        section(ui, "Onion skin", accent(), |ui| {
            dim(ui, "Strength of the ghosts of the previous (warm) and next (green) frame in Animate mode.");
            let mut o = self.settings.onion_opacity;
            if crate::widgets::slider_f_tip(ui, "strength", &mut o, 0.1..=1.0, "How visible the neighbouring frames are") {
                self.settings.onion_opacity = o;
                self.touch_settings();
            }
        });
        section(ui, "Composition guides", accent(), |ui| {
            dim(ui, "Lines over the view in Render mode to help framing the shot. Also on the tool strip.");
            let mut g = self.settings.guides;
            if self.choice_row(ui, &mut g, "Composition guide") {
                self.settings.guides = g;
            }
        });
    }

    fn settings_interface(&mut self, ui: &mut egui::Ui, st: &mut SettingsState) {
        section(ui, "Interface scale", accent(), |ui| {
            dim(ui, "Makes every control larger or smaller. Choose a preset or drag the slider and press Apply. Large scales need a large window or screen.");
            ui.horizontal(|ui| {
                for (label, v) in [("75%", 0.75f32), ("100%", 1.0), ("125%", 1.25), ("150%", 1.5), ("175%", 1.75), ("200%", 2.0)] {
                    let on = (self.settings.ui_scale - v).abs() < 0.01;
                    if Btn::new(label).on(on).accent(accent()).size(vec2(64.0, 30.0)).tip("Set the interface scale").show(ui).clicked() {
                        self.settings.ui_scale = v;
                        st.scale_edit = v;
                        self.touch_settings();
                    }
                }
            });
            ui.horizontal(|ui| {
                crate::widgets::with_tip(ui.add(egui::Slider::new(&mut st.scale_edit, UI_SCALE_MIN..=UI_SCALE_MAX).fixed_decimals(2).text("scale")), "Drag to choose a scale, then press Apply");
                if Btn::new("Apply").accent(accent()).size(vec2(80.0, 30.0)).tip("Use the scale chosen with the slider").show(ui).clicked() {
                    self.settings.ui_scale = st.scale_edit;
                    self.touch_settings();
                }
            });
        });
        section(ui, "Comfort and accessibility", accent(), |ui| {
            let mut v = self.settings.show_tooltips;
            if toggle_tip(ui, &mut v, "Show tooltips", accent(), "Explain a control when the pointer rests on it").clicked() {
                self.settings.show_tooltips = v;
                self.touch_settings();
            }
            let mut v = self.settings.reduce_motion;
            if toggle_tip(ui, &mut v, "Reduce motion", accent(), "Turn off hover fades and shorten animated switches").clicked() {
                self.settings.reduce_motion = v;
                self.touch_settings();
            }
        });
        section(ui, "Panel layout", WARN(), |ui| {
            dim(ui, "Panel sizes are remembered between sessions. Drag a panel edge to resize it.");
            if Btn::new("Reset panel layout").tip("Return every panel to its original size").show(ui).clicked() {
                self.settings.layout = settings::PanelLayout::default();
                self.layout_reset = true;
                self.touch_settings();
            }
        });
    }

    fn settings_viewport(&mut self, ui: &mut egui::Ui) {
        use psxcore::settings::{Corner, ViewportQuality, PREVIEW_WIDTH_MAX, PREVIEW_WIDTH_MIN};
        section(ui, "Highlight", accent(), |ui| {
            let mut v = self.settings.hover_highlight;
            if toggle_tip(ui, &mut v, "Outline the object under the mouse", accent(), "Shows which object a click will select").changed() {
                self.settings.hover_highlight = v;
                self.touch_settings();
            }
        });
        section(ui, "Viewport sharpness", accent(), |ui| {
            dim(ui, "How sharp the 3D view is drawn compared with its size on screen. Auto lowers sharpness only while the frame rate would drop, and raises it back when there is time to spare. The final render is never affected.");
            ui.horizontal_wrapped(|ui| {
                for q in ViewportQuality::ALL {
                    if Btn::new(q.label()).accent(accent()).on(self.settings.viewport_quality == q).size(vec2(64.0, 30.0)).tip("Viewport sharpness").show(ui).clicked() {
                        self.settings.viewport_quality = q;
                        self.touch_settings();
                    }
                }
            });
        });
        section(ui, "Viewport shadows", accent(), |ui| {
            dim(ui, "Shadows in the editing view only. Lower settings keep the view smooth on slow computers; the final render, the export and the preview window always use the scene's own shadows.");
            let mut v = self.settings.viewport_shadows;
            if self.choice_row(ui, &mut v, "Shadow quality of the editing view") {
                self.settings.viewport_shadows = v;
            }
            let mut v = self.settings.lazy_redraw;
            if toggle_tip(ui, &mut v, "Redraw the view only when something changes", accent(), "Saves processor time while you hover panels or read tooltips. Turn off to draw every frame").clicked() {
                self.settings.lazy_redraw = v;
                self.touch_settings();
            }
        });
        section(ui, "Navigation", accent(), |ui| {
            dim(ui, "Orbit with the middle or right button (or Alt + left), pan with Shift, zoom with the wheel.");
            let mut o = self.settings.orbit_speed;
            if crate::widgets::slider_f_tip(ui, "orbit speed", &mut o, settings::SPEED_MIN..=settings::SPEED_MAX, "How fast the view turns while dragging") {
                self.settings.orbit_speed = o;
                self.touch_settings();
            }
            let mut z = self.settings.zoom_speed;
            if crate::widgets::slider_f_tip(ui, "zoom speed", &mut z, settings::SPEED_MIN..=settings::SPEED_MAX, "How far one wheel step zooms") {
                self.settings.zoom_speed = z;
                self.touch_settings();
            }
            let mut v = self.settings.invert_orbit_y;
            if toggle_tip(ui, &mut v, "Invert vertical orbit", accent(), "Dragging up tilts the view down instead of up").clicked() {
                self.settings.invert_orbit_y = v;
                self.touch_settings();
            }
        });
        section(ui, "Processor use", accent(), |ui| {
            dim(ui, "Number of threads the software renderer may use. 0 means automatic (all cores but one). Lower it on a weak or busy computer.");
            let mut t = self.settings.render_threads;
            if crate::widgets::slider_u_tip(ui, "render threads (0 = auto)", &mut t, 0..=32, "How many threads draw the 3D view") {
                self.settings.render_threads = t;
                self.touch_settings();
            }
            let mut v = self.settings.show_stats;
            if toggle_tip(ui, &mut v, "Show frame statistics (F3)", accent(), "Frame rate, drawing time and viewport size in a corner of the 3D view").clicked() {
                self.settings.show_stats = v;
                self.touch_settings();
            }
        });
        section(ui, "Render preview window", accent(), |ui| {
            dim(ui, "A small window with the real render at the chosen resolution. Drag its header to move it between corners; drag the corner grip to resize.");
            let mut pv = self.settings.preview;
            let mut ch = toggle_tip(ui, &mut pv.visible, "Show the preview window (Shift+P)", accent(), "Show or hide the preview").clicked();
            ui.horizontal_wrapped(|ui| {
                for (c, l) in [(Corner::TopLeft, "Top left"), (Corner::TopRight, "Top right"), (Corner::BottomLeft, "Bottom left"), (Corner::BottomRight, "Bottom right")] {
                    if Btn::new(l).accent(accent()).on(pv.corner == c).tip("Corner of the viewport").show(ui).clicked() {
                        pv.corner = c;
                        ch = true;
                    }
                }
            });
            ch |= crate::widgets::slider_f(ui, "window width", &mut pv.width, PREVIEW_WIDTH_MIN..=PREVIEW_WIDTH_MAX);
            if ch {
                self.settings.preview = pv;
                self.touch_settings();
            }
        });
    }

    fn settings_files(&mut self, ui: &mut egui::Ui) {
        section(ui, "Autosave and recovery", accent(), |ui| {
            dim(ui, "While a project has unsaved changes, Jank! keeps a snapshot. After a crash or power loss it offers to restore it at the next start.");
            let mut v = self.settings.autosave_enabled;
            if toggle_tip(ui, &mut v, "Autosave snapshots", accent(), "Periodically store a recovery snapshot of unsaved work").clicked() {
                self.settings.autosave_enabled = v;
                self.touch_settings();
            }
            let mut m = self.settings.autosave_minutes;
            if crate::widgets::slider_u_tip(ui, "minutes between snapshots", &mut m, 1..=60, "How often a snapshot is written") {
                self.settings.autosave_minutes = m;
                self.touch_settings();
            }
            if let Some(d) = recovery::recovery_dir() {
                dim(ui, &format!("Snapshot folder: {}", d.display()));
            }
        });
        section(ui, "Where settings are stored", accent(), |ui| {
            match settings::settings_path() {
                Some(p) => dim(ui, &format!("{}", p.display())),
                None => dim(ui, "No configuration folder was found; settings last only for this session."),
            };
        });
        section(ui, "Recent files", WARN(), |ui| {
            dim(ui, &format!("{} remembered", self.settings.recent_files.len()));
            if Btn::new("Clear recent files").enabled(!self.settings.recent_files.is_empty()).tip("Forget the list of recently opened files").show(ui).clicked() {
                self.settings.recent_files.clear();
                self.touch_settings();
            }
        });
    }

    fn settings_shortcuts(&mut self, ui: &mut egui::Ui, st: &mut SettingsState) {
        ui.horizontal(|ui| {
            crate::widgets::icon_inline(ui, Icon::Search, DIM());
            ui.add(egui::TextEdit::singleline(&mut st.filter).hint_text("Filter commands").desired_width(260.0));
            if Btn::new("Reset all").accent(DANGER()).tip("Restore every shortcut to its default").show(ui).clicked() {
                self.settings.keymap.reset_all();
                st.note = "All shortcuts restored".to_string();
                self.touch_settings();
            }
        });
        let conflicts = self.settings.keymap.conflicts();
        if !conflicts.is_empty() {
            ui.colored_label(DANGER(), format!("{} shortcut(s) are used by more than one command.", conflicts.len()));
        }
        if !st.note.is_empty() {
            dim(ui, &st.note.clone());
        }
        for cat in Category::ALL {
            let rows: Vec<CommandId> = COMMANDS.iter().filter(|c| c.category == cat && (st.filter.trim().is_empty() || commands::fuzzy_score(&st.filter, c.title).is_some())).map(|c| c.id).collect();
            if rows.is_empty() {
                continue;
            }
            section(ui, cat.title(), accent(), |ui| {
                for id in rows {
                    ui.horizontal(|ui| {
                        ui.allocate_ui(vec2(210.0, 28.0), |ui| {
                            ui.set_min_size(vec2(210.0, 28.0));
                            ui.label(egui::RichText::new(id.title()).size(14.0)).on_hover_text(id.info().description);
                        });
                        let capturing = st.capturing == Some(id);
                        let chords = self.settings.keymap.chords(id);
                        let text = if capturing {
                            "Press a key combination (Esc cancels)".to_string()
                        } else if chords.is_empty() {
                            "not assigned".to_string()
                        } else {
                            chords.iter().map(|c| c.to_string()).collect::<Vec<_>>().join("   or   ")
                        };
                        let col = if capturing { WARN() } else if chords.is_empty() { DIM() } else { accent() };
                        ui.allocate_ui(vec2(250.0, 28.0), |ui| {
                            ui.set_min_size(vec2(250.0, 28.0));
                            ui.label(egui::RichText::new(text).size(13.5).color(col));
                        });
                        if Btn::new("Change").size(vec2(74.0, 28.0)).tip("Record a new shortcut for this command").show(ui).clicked() {
                            st.capturing = Some(id);
                            st.note.clear();
                        }
                        if Btn::new("Clear").size(vec2(60.0, 28.0)).tip("Remove the shortcut of this command").show(ui).clicked() {
                            self.settings.keymap.set(id, vec![]);
                            self.touch_settings();
                        }
                        if !self.settings.keymap.is_default(id) && Btn::new("Reset").size(vec2(60.0, 28.0)).tip("Restore the default shortcut").show(ui).clicked() {
                            self.settings.keymap.reset(id);
                            self.touch_settings();
                        }
                    });
                }
            });
        }
    }

    /// Runs the command chosen in the palette (after the dialog code has finished borrowing).
    pub fn run_pending(&mut self, ctx: &Context) {
        if let Some(id) = self.pending.take() {
            self.run_command(ctx, id);
        }
    }

    /// Shows messages about a settings file that could not be read completely.
    pub fn report_settings_problems(&mut self, problems: Vec<String>) {
        if !problems.is_empty() {
            let n = problems.len();
            self.toast(format!("{n} line(s) of the settings file were ignored: {}", problems[0]), ToastKind::Error);
        }
    }
}
