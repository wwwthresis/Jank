//! Persistent user settings: interface scale, accessibility options, autosave, panel layout,
//! recent files and customised shortcuts. Stored as a small readable text file
//! (`settings.conf`) in the user's configuration directory.
//!
//! Loading never fails: a missing file gives the defaults, and bad lines are skipped and reported
//! so a hand-edited file cannot lock the user out of the application.

use crate::keymap::Keymap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Bump when a key changes meaning. Older files stay readable because unknown keys are ignored.
pub const SETTINGS_VERSION: u32 = 1;

pub const UI_SCALE_MIN: f32 = 0.75;
pub const UI_SCALE_MAX: f32 = 2.0;
pub const RECENT_LIMIT: usize = 10;

/// Widths and heights of the dockable panels, in logical points (before UI scaling).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelLayout {
    pub left_width: f32,
    pub right_width: f32,
    pub texture_width: f32,
    pub timeline_height: f32,
}

impl Default for PanelLayout {
    fn default() -> Self {
        PanelLayout { left_width: 252.0, right_width: 290.0, texture_width: 470.0, timeline_height: 178.0 }
    }
}

/// How sharp the main viewport is drawn relative to its size on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportQuality {
    /// Follows the measured frame time: drops to a lower scale when drawing is slow, rises again when it is fast.
    Auto,
    Full,
    ThreeQuarter,
    Half,
    Third,
}
impl ViewportQuality {
    pub const ALL: [ViewportQuality; 5] = [ViewportQuality::Auto, ViewportQuality::Full, ViewportQuality::ThreeQuarter, ViewportQuality::Half, ViewportQuality::Third];
    pub fn id(self) -> &'static str {
        match self {
            ViewportQuality::Auto => "auto",
            ViewportQuality::Full => "100",
            ViewportQuality::ThreeQuarter => "75",
            ViewportQuality::Half => "50",
            ViewportQuality::Third => "33",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ViewportQuality::Auto => "Auto",
            ViewportQuality::Full => "100%",
            ViewportQuality::ThreeQuarter => "75%",
            ViewportQuality::Half => "50%",
            ViewportQuality::Third => "33%",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.id() == s.trim().to_ascii_lowercase())
    }
    /// Fixed scale for the manual modes (`Auto` starts at 1.0).
    pub fn scale(self) -> f32 {
        match self {
            ViewportQuality::Auto | ViewportQuality::Full => 1.0,
            ViewportQuality::ThreeQuarter => 0.75,
            ViewportQuality::Half => 0.5,
            ViewportQuality::Third => 1.0 / 3.0,
        }
    }
}

/// Corner of the viewport where the render preview window sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}
impl Corner {
    pub fn id(self) -> &'static str {
        match self {
            Corner::BottomLeft => "bottom-left",
            Corner::BottomRight => "bottom-right",
            Corner::TopLeft => "top-left",
            Corner::TopRight => "top-right",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [Corner::BottomLeft, Corner::BottomRight, Corner::TopLeft, Corner::TopRight].into_iter().find(|c| c.id() == s.trim())
    }
}

/// Render preview window: a small view of the final picture in the chosen render resolution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewPrefs {
    pub visible: bool,
    pub corner: Corner,
    /// Width of the window on screen in logical points; the height follows the render aspect ratio.
    pub width: f32,
}
impl Default for PreviewPrefs {
    fn default() -> Self {
        PreviewPrefs { visible: true, corner: Corner::BottomLeft, width: 280.0 }
    }
}
pub const PREVIEW_WIDTH_MIN: f32 = 120.0;
pub const PREVIEW_WIDTH_MAX: f32 = 900.0;

/// A small set of named choices stored by a stable id (shared code for the option enums below).
pub trait Choice: Copy + PartialEq + Sized + 'static {
    const ALL: &'static [Self];
    fn id(self) -> &'static str;
    fn label(self) -> &'static str;
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        Self::ALL.iter().copied().find(|c| c.id() == s)
    }
}

macro_rules! choice {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident = $id:literal, $label:literal;)+ }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($(#[$vm])* $v,)+ }
        impl Choice for $name {
            const ALL: &'static [Self] = &[$($name::$v,)+];
            fn id(self) -> &'static str { match self { $($name::$v => $id,)+ } }
            fn label(self) -> &'static str { match self { $($name::$v => $label,)+ } }
        }
    };
}

choice!(
    /// The one interaction colour of the interface (selection, active tool, focus).
    Accent {
        Violet = "violet", "Violet";
        Blue = "blue", "Blue";
        Teal = "teal", "Teal";
        Green = "green", "Green";
        Amber = "amber", "Amber";
        Rose = "rose", "Rose";
    }
);
impl Accent {
    /// sRGB colour of the accent.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Accent::Violet => [0x8a, 0x7b, 0xf2],
            Accent::Blue => [0x5c, 0x9c, 0xf0],
            Accent::Teal => [0x3c, 0xb4, 0xa6],
            Accent::Green => [0x7d, 0xbf, 0x69],
            Accent::Amber => [0xdd, 0xa0, 0x3f],
            Accent::Rose => [0xe0, 0x68, 0x8a],
        }
    }
}

choice!(
    /// Spacing of the controls.
    Density {
        Compact = "compact", "Compact";
        Comfortable = "comfortable", "Comfortable";
    }
);

choice!(
    /// Size of the interface text (independent of the interface scale).
    TextSize {
        Small = "small", "Small";
        Normal = "normal", "Normal";
        Large = "large", "Large";
    }
);
impl TextSize {
    /// Body text size in points.
    pub fn points(self) -> f32 {
        match self {
            TextSize::Small => 12.0,
            TextSize::Normal => 13.0,
            TextSize::Large => 14.5,
        }
    }
}

choice!(
    /// Shadows in the editing viewport. The final render and the preview window always use the scene's own setting.
    ViewportShadows {
        /// Exactly like the final render.
        Render = "render", "Like render";
        /// The scene's shadows, but a shadow map of at most Medium quality.
        Medium = "medium", "Up to Medium";
        /// Soft round spots instead of cast shadows.
        Blob = "blob", "Blob";
        Off = "off", "Off";
    }
);

choice!(
    /// Composition guides drawn over the viewport in Render mode.
    Guides {
        Off = "off", "Off";
        Thirds = "thirds", "Thirds";
        Centre = "centre", "Centre";
        SafeArea = "safe", "Safe area";
        Golden = "golden", "Golden";
    }
);

pub const SPEED_MIN: f32 = 0.25;
pub const SPEED_MAX: f32 = 3.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Multiplier for the whole interface (1.0 = normal).
    pub ui_scale: f32,
    /// Turns off decorative motion (button tilt, animated toggles) for comfort and accessibility.
    pub reduce_motion: bool,
    /// Shows explanatory tooltips when hovering controls.
    pub show_tooltips: bool,
    pub autosave_enabled: bool,
    /// Minutes between automatic recovery snapshots, 1..=60.
    pub autosave_minutes: u32,
    pub layout: PanelLayout,
    pub viewport_quality: ViewportQuality,
    /// Worker threads for drawing; 0 = use every core (up to 8).
    pub render_threads: u32,
    /// Shows frame time, draw time and triangle count in the viewport corner.
    pub show_stats: bool,
    pub preview: PreviewPrefs,
    pub last_dir: Option<String>,
    pub recent_files: Vec<String>,
    pub keymap: Keymap,
    /// Interface accent colour.
    pub accent: Accent,
    pub density: Density,
    pub text_size: TextSize,
    /// The vertical tool strip at the left edge of the viewport.
    pub show_toolbar: bool,
    pub show_status_bar: bool,
    /// Frame rate and drawing time at the right end of the status bar.
    pub status_fps: bool,
    /// Shadows in the editing viewport only.
    pub viewport_shadows: ViewportShadows,
    /// Redraw the 3D view only when something in it may have changed (saves power and keeps the interface fast).
    pub lazy_redraw: bool,
    /// Multipliers for orbiting and wheel zoom, `SPEED_MIN..=SPEED_MAX`.
    pub orbit_speed: f32,
    pub zoom_speed: f32,
    pub invert_orbit_y: bool,
    /// Pixel grid over the texture canvas when zoomed in far enough.
    pub pixel_grid: bool,
    /// Strength of the onion-skin ghosts, 0.1..=1.
    pub onion_opacity: f32,
    pub guides: Guides,
    /// Id of the interface theme (see `themes`).
    pub theme: String,
    /// Build number whose "What's new" was last shown (0 = never).
    pub last_seen_build: u32,
    /// Smoothing of brush strokes (0 = off .. 1 = very smooth, the line trails behind the mouse).
    pub stabilizer: f32,
    /// Outline the object under the mouse in the viewport.
    pub hover_highlight: bool,
    /// Discord Rich Presence: show "Playing Jank!" and what you are doing on your Discord profile.
    pub discord: DiscordPrefs,
}

/// Privacy choices of the Discord activity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiscordPrefs {
    pub enabled: bool,
    /// Show the project name (off: "Working on a project").
    pub show_project: bool,
    /// Show the mode and what you are doing in it (off: "Making low-poly art").
    pub show_activity: bool,
    /// Show the time since the project was opened.
    pub show_time: bool,
}

impl Default for DiscordPrefs {
    fn default() -> Self {
        DiscordPrefs { enabled: true, show_project: false, show_activity: true, show_time: true }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ui_scale: 1.0,
            reduce_motion: false,
            show_tooltips: true,
            autosave_enabled: true,
            autosave_minutes: 3,
            layout: PanelLayout::default(),
            viewport_quality: ViewportQuality::Auto,
            render_threads: 0,
            show_stats: false,
            preview: PreviewPrefs::default(),
            last_dir: None,
            recent_files: vec![],
            keymap: Keymap::default(),
            accent: Accent::Violet,
            density: Density::Compact,
            text_size: TextSize::Normal,
            show_toolbar: true,
            show_status_bar: true,
            status_fps: false,
            viewport_shadows: ViewportShadows::Render,
            lazy_redraw: true,
            orbit_speed: 1.0,
            zoom_speed: 1.0,
            invert_orbit_y: false,
            pixel_grid: true,
            onion_opacity: 0.6,
            guides: Guides::Off,
            theme: "graphite".into(),
            last_seen_build: 0,
            stabilizer: 0.0,
            hover_highlight: true,
            discord: DiscordPrefs::default(),
        }
    }
}

/// Directory for configuration files. `JANK_CONFIG_DIR` overrides everything (used by tests and
/// portable installs), then `XDG_CONFIG_HOME`, then `~/.config`, then `%APPDATA%`.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("JANK_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(x).join("jank"));
    }
    if let Some(h) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(h).join(".config").join("jank"));
    }
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Jank"))
}

pub fn settings_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("settings.conf"))
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

impl Settings {
    /// Forces every value into its valid range.
    pub fn clamp(&mut self) {
        self.ui_scale = if self.ui_scale.is_finite() { self.ui_scale.clamp(UI_SCALE_MIN, UI_SCALE_MAX) } else { 1.0 };
        self.autosave_minutes = self.autosave_minutes.clamp(1, 60);
        let l = &mut self.layout;
        let fix = |v: &mut f32, lo: f32, hi: f32, d: f32| *v = if v.is_finite() { v.clamp(lo, hi) } else { d };
        fix(&mut l.left_width, 180.0, 600.0, 252.0);
        fix(&mut l.right_width, 200.0, 600.0, 290.0);
        fix(&mut l.texture_width, 260.0, 1200.0, 470.0);
        fix(&mut l.timeline_height, 120.0, 400.0, 178.0);
        self.stabilizer = if self.stabilizer.is_finite() { self.stabilizer.clamp(0.0, 1.0) } else { 0.0 };
        self.render_threads = self.render_threads.min(32);
        let w = self.preview.width;
        self.preview.width = if w.is_finite() { w.clamp(PREVIEW_WIDTH_MIN, PREVIEW_WIDTH_MAX) } else { PreviewPrefs::default().width };
        self.recent_files.truncate(RECENT_LIMIT);
        let speed = |v: f32| if v.is_finite() { v.clamp(SPEED_MIN, SPEED_MAX) } else { 1.0 };
        self.orbit_speed = speed(self.orbit_speed);
        self.zoom_speed = speed(self.zoom_speed);
        self.onion_opacity = if self.onion_opacity.is_finite() { self.onion_opacity.clamp(0.1, 1.0) } else { 0.6 };
    }

    /// Remembers a file at the front of the recent list (no duplicates).
    pub fn push_recent(&mut self, path: &Path) {
        let s = path.to_string_lossy().to_string();
        self.recent_files.retain(|p| p != &s);
        self.recent_files.insert(0, s);
        self.recent_files.truncate(RECENT_LIMIT);
        if let Some(parent) = path.parent() {
            self.last_dir = Some(parent.to_string_lossy().to_string());
        }
    }

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("# Jank! settings. Lines starting with # are ignored.\nsettings_version = {SETTINGS_VERSION}\n"));
        s.push_str(&format!("ui_scale = {:.2}\n", self.ui_scale));
        s.push_str(&format!("reduce_motion = {}\n", self.reduce_motion));
        s.push_str(&format!("show_tooltips = {}\n", self.show_tooltips));
        s.push_str(&format!("autosave = {}\n", self.autosave_enabled));
        s.push_str(&format!("autosave_minutes = {}\n", self.autosave_minutes));
        s.push_str(&format!("layout.left_width = {:.0}\n", self.layout.left_width));
        s.push_str(&format!("layout.right_width = {:.0}\n", self.layout.right_width));
        s.push_str(&format!("layout.texture_width = {:.0}\n", self.layout.texture_width));
        s.push_str(&format!("layout.timeline_height = {:.0}\n", self.layout.timeline_height));
        s.push_str(&format!("viewport_quality = {}\n", self.viewport_quality.id()));
        s.push_str(&format!("render_threads = {}\n", self.render_threads));
        s.push_str(&format!("show_stats = {}\n", self.show_stats));
        s.push_str(&format!("preview.visible = {}\n", self.preview.visible));
        s.push_str(&format!("preview.corner = {}\n", self.preview.corner.id()));
        s.push_str(&format!("preview.width = {:.0}\n", self.preview.width));
        s.push_str(&format!("accent = {}\n", self.accent.id()));
        s.push_str(&format!("density = {}\n", self.density.id()));
        s.push_str(&format!("text_size = {}\n", self.text_size.id()));
        s.push_str(&format!("show_toolbar = {}\n", self.show_toolbar));
        s.push_str(&format!("show_status_bar = {}\n", self.show_status_bar));
        s.push_str(&format!("status_fps = {}\n", self.status_fps));
        s.push_str(&format!("viewport_shadows = {}\n", self.viewport_shadows.id()));
        s.push_str(&format!("lazy_redraw = {}\n", self.lazy_redraw));
        s.push_str(&format!("orbit_speed = {:.2}\n", self.orbit_speed));
        s.push_str(&format!("zoom_speed = {:.2}\n", self.zoom_speed));
        s.push_str(&format!("invert_orbit_y = {}\n", self.invert_orbit_y));
        s.push_str(&format!("pixel_grid = {}\n", self.pixel_grid));
        s.push_str(&format!("onion_opacity = {:.2}\n", self.onion_opacity));
        s.push_str(&format!("guides = {}\n", self.guides.id()));
        s.push_str(&format!("theme = {}\n", self.theme));
        s.push_str(&format!("last_seen_build = {}\n", self.last_seen_build));
        s.push_str(&format!("stabilizer = {:.2}\n", self.stabilizer));
        s.push_str(&format!("hover_highlight = {}\n", self.hover_highlight));
        s.push_str(&format!("discord = {}\n", self.discord.enabled));
        s.push_str(&format!("discord_project = {}\n", self.discord.show_project));
        s.push_str(&format!("discord_activity = {}\n", self.discord.show_activity));
        s.push_str(&format!("discord_time = {}\n", self.discord.show_time));
        if let Some(d) = &self.last_dir {
            s.push_str(&format!("last_dir = {d}\n"));
        }
        for r in &self.recent_files {
            s.push_str(&format!("recent = {r}\n"));
        }
        s.push_str("\n[shortcuts]\n");
        s.push_str(&self.keymap.to_text());
        s
    }

    /// Parses a settings file. Problems are returned as readable messages; everything that could
    /// be understood is applied.
    pub fn from_text(text: &str) -> (Settings, Vec<String>) {
        let mut st = Settings::default();
        let mut problems = vec![];
        let mut shortcuts = String::new();
        let mut in_shortcuts = false;
        let mut recent = vec![];
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "[shortcuts]" {
                in_shortcuts = true;
                continue;
            }
            if line.starts_with('[') {
                in_shortcuts = false;
                continue;
            }
            if in_shortcuts {
                shortcuts.push_str(line);
                shortcuts.push('\n');
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                problems.push(format!("line {}: missing '='", n + 1));
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            let mut bad = || problems.push(format!("line {}: cannot read value of {k}", n + 1));
            match k {
                "settings_version" => {}
                "ui_scale" => match v.parse() {
                    Ok(x) => st.ui_scale = x,
                    Err(_) => bad(),
                },
                "reduce_motion" => parse_bool(v).map(|b| st.reduce_motion = b).unwrap_or_else(bad),
                "show_tooltips" => parse_bool(v).map(|b| st.show_tooltips = b).unwrap_or_else(bad),
                "autosave" => parse_bool(v).map(|b| st.autosave_enabled = b).unwrap_or_else(bad),
                "autosave_minutes" => v.parse().map(|x| st.autosave_minutes = x).unwrap_or_else(|_| bad()),
                "layout.left_width" => v.parse().map(|x| st.layout.left_width = x).unwrap_or_else(|_| bad()),
                "layout.right_width" => v.parse().map(|x| st.layout.right_width = x).unwrap_or_else(|_| bad()),
                "layout.texture_width" => v.parse().map(|x| st.layout.texture_width = x).unwrap_or_else(|_| bad()),
                "layout.timeline_height" => v.parse().map(|x| st.layout.timeline_height = x).unwrap_or_else(|_| bad()),
                "viewport_quality" => ViewportQuality::parse(v).map(|q| st.viewport_quality = q).ok_or(()).unwrap_or_else(|_| bad()),
                "render_threads" => v.parse().map(|x| st.render_threads = x).unwrap_or_else(|_| bad()),
                "show_stats" => parse_bool(v).map(|b| st.show_stats = b).unwrap_or_else(bad),
                "preview.visible" => parse_bool(v).map(|b| st.preview.visible = b).unwrap_or_else(bad),
                "preview.corner" => Corner::parse(v).map(|c| st.preview.corner = c).ok_or(()).unwrap_or_else(|_| bad()),
                "preview.width" => v.parse().map(|x| st.preview.width = x).unwrap_or_else(|_| bad()),
                "accent" => Accent::parse(v).map(|c| st.accent = c).ok_or(()).unwrap_or_else(|_| bad()),
                "density" => Density::parse(v).map(|c| st.density = c).ok_or(()).unwrap_or_else(|_| bad()),
                "text_size" => TextSize::parse(v).map(|c| st.text_size = c).ok_or(()).unwrap_or_else(|_| bad()),
                "show_toolbar" => parse_bool(v).map(|b| st.show_toolbar = b).unwrap_or_else(bad),
                "show_status_bar" => parse_bool(v).map(|b| st.show_status_bar = b).unwrap_or_else(bad),
                "status_fps" => parse_bool(v).map(|b| st.status_fps = b).unwrap_or_else(bad),
                "viewport_shadows" => ViewportShadows::parse(v).map(|c| st.viewport_shadows = c).ok_or(()).unwrap_or_else(|_| bad()),
                "lazy_redraw" => parse_bool(v).map(|b| st.lazy_redraw = b).unwrap_or_else(bad),
                "orbit_speed" => v.parse().map(|x| st.orbit_speed = x).unwrap_or_else(|_| bad()),
                "zoom_speed" => v.parse().map(|x| st.zoom_speed = x).unwrap_or_else(|_| bad()),
                "invert_orbit_y" => parse_bool(v).map(|b| st.invert_orbit_y = b).unwrap_or_else(bad),
                "pixel_grid" => parse_bool(v).map(|b| st.pixel_grid = b).unwrap_or_else(bad),
                "onion_opacity" => v.parse().map(|x| st.onion_opacity = x).unwrap_or_else(|_| bad()),
                "guides" => Guides::parse(v).map(|c| st.guides = c).ok_or(()).unwrap_or_else(|_| bad()),
                "theme" => st.theme = v.chars().take(48).collect(),
                "last_seen_build" => v.parse().map(|x| st.last_seen_build = x).unwrap_or_else(|_| bad()),
                "stabilizer" => v.parse().map(|x| st.stabilizer = x).unwrap_or_else(|_| bad()),
                "hover_highlight" => parse_bool(v).map(|b| st.hover_highlight = b).unwrap_or_else(bad),
                "discord" => parse_bool(v).map(|b| st.discord.enabled = b).unwrap_or_else(bad),
                "discord_project" => parse_bool(v).map(|b| st.discord.show_project = b).unwrap_or_else(bad),
                "discord_activity" => parse_bool(v).map(|b| st.discord.show_activity = b).unwrap_or_else(bad),
                "discord_time" => parse_bool(v).map(|b| st.discord.show_time = b).unwrap_or_else(bad),
                "last_dir" => st.last_dir = Some(v.to_string()),
                "recent" => recent.push(v.to_string()),
                _ => {} // written by a newer version: ignore silently
            }
        }
        st.recent_files = recent;
        problems.extend(st.keymap.apply_text(&shortcuts));
        st.clamp();
        (st, problems)
    }

    /// Reads the settings file. A missing or unreadable file yields defaults.
    pub fn load_from(path: &Path) -> (Settings, Vec<String>) {
        match fs::read(path) {
            Ok(bytes) => Settings::from_text(&String::from_utf8_lossy(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), vec![]),
            Err(e) => (Settings::default(), vec![format!("could not read {}: {e}", path.display())]),
        }
    }

    /// Writes through a temporary file and renames it, so a crash never leaves half a file.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("conf.part");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(self.to_text().as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandId;
    use crate::keymap::Chord;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jank-settings-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn defaults_survive_a_text_round_trip() {
        let s = Settings::default();
        let (back, problems) = Settings::from_text(&s.to_text());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(s, back);
    }

    #[test]
    fn customised_settings_round_trip() {
        let mut s = Settings::default();
        s.ui_scale = 1.25;
        s.reduce_motion = true;
        s.autosave_minutes = 10;
        s.layout.left_width = 300.0;
        s.push_recent(Path::new("/work/a.jank"));
        s.push_recent(Path::new("/work/b.jank"));
        s.keymap.set(CommandId::MirrorX, vec![Chord::parse("Alt+X").unwrap()]);
        s.accent = Accent::Teal;
        s.density = Density::Comfortable;
        s.text_size = TextSize::Large;
        s.show_toolbar = false;
        s.status_fps = true;
        s.viewport_shadows = ViewportShadows::Blob;
        s.lazy_redraw = false;
        s.orbit_speed = 1.5;
        s.zoom_speed = 0.75;
        s.invert_orbit_y = true;
        s.pixel_grid = false;
        s.onion_opacity = 0.35;
        s.guides = Guides::Thirds;
        s.discord = DiscordPrefs { enabled: false, show_project: true, show_activity: false, show_time: false };
        let (back, problems) = Settings::from_text(&s.to_text());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back.recent_files, vec!["/work/b.jank", "/work/a.jank"]);
        assert_eq!(back.last_dir.as_deref(), Some("/work"));
        assert_eq!(s, back);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let (s, _) = Settings::from_text("ui_scale = 99\nautosave_minutes = 0\nlayout.left_width = -5\n");
        assert_eq!(s.ui_scale, UI_SCALE_MAX);
        assert_eq!(s.autosave_minutes, 1);
        assert!(s.layout.left_width >= 180.0);
        let (s, _) = Settings::from_text("ui_scale = NaN\n");
        assert_eq!(s.ui_scale, 1.0);
        let (s, _) = Settings::from_text("orbit_speed = 40\nzoom_speed = 0\nonion_opacity = 7\n");
        assert_eq!((s.orbit_speed, s.zoom_speed, s.onion_opacity), (SPEED_MAX, SPEED_MIN, 1.0));
    }

    #[test]
    fn every_choice_round_trips_through_its_id() {
        fn check<C: Choice + std::fmt::Debug>() {
            for &c in C::ALL {
                assert_eq!(C::parse(c.id()), Some(c));
                assert_eq!(C::parse(&c.id().to_ascii_uppercase()), Some(c), "ids are case-insensitive");
                assert!(!c.label().is_empty());
            }
        }
        check::<Accent>();
        check::<Density>();
        check::<TextSize>();
        check::<ViewportShadows>();
        check::<Guides>();
        let (s, p) = Settings::from_text("accent = plaid\n");
        assert_eq!(s.accent, Accent::Violet);
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn garbage_is_reported_not_fatal() {
        let (s, p) = Settings::from_text("ui_scale = big\nreduce_motion = maybe\nfuture_option = 7\nnonsense\n[shortcuts]\nfile.new = Ctrl+Banana\n");
        assert_eq!(s.ui_scale, 1.0);
        assert_eq!(p.len(), 4, "{p:?}");
    }

    #[test]
    fn recent_list_is_unique_and_limited() {
        let mut s = Settings::default();
        for i in 0..20 {
            s.push_recent(Path::new(&format!("/p/{i}.jank")));
        }
        s.push_recent(Path::new("/p/15.jank"));
        assert_eq!(s.recent_files.len(), RECENT_LIMIT);
        assert_eq!(s.recent_files[0], "/p/15.jank");
        assert_eq!(s.recent_files.iter().filter(|p| *p == "/p/15.jank").count(), 1);
    }

    #[test]
    fn save_and_load_use_the_disk_atomically() {
        let dir = scratch("disk");
        let path = dir.join("nested").join("settings.conf");
        let mut s = Settings::default();
        s.ui_scale = 1.5;
        s.save_to(&path).unwrap();
        assert!(!path.with_extension("conf.part").exists(), "temporary file must be renamed away");
        let (back, problems) = Settings::load_from(&path);
        assert!(problems.is_empty());
        assert_eq!(back.ui_scale, 1.5);
        let (missing, p) = Settings::load_from(&dir.join("none.conf"));
        assert!(p.is_empty());
        assert_eq!(missing, Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn viewport_and_preview_settings_round_trip_and_clamp() {
        let mut a = Settings::default();
        a.viewport_quality = ViewportQuality::Half;
        a.render_threads = 3;
        a.show_stats = true;
        a.preview = PreviewPrefs { visible: false, corner: Corner::TopRight, width: 333.0 };
        let (b, problems) = Settings::from_text(&a.to_text());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(a, b);
        let (c, p) = Settings::from_text("preview.width = 99999\nviewport_quality = sideways\nrender_threads = 500\n");
        assert_eq!(c.preview.width, PREVIEW_WIDTH_MAX);
        assert_eq!(c.viewport_quality, ViewportQuality::Auto);
        assert_eq!(c.render_threads, 32);
        assert_eq!(p.len(), 1, "{p:?}");
    }
}
