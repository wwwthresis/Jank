//! Interface themes as plain text files. A theme sets the colours of the workspace, optionally the colour of every single
//! part of the window (title bar, menus, buttons, timeline…), sizes such as corner radii and shadows, and pictures for the
//! background, the title bar, the panels and more. The GUI reads all of it at paint time, so switching is instant, and a
//! theme file that changes on disk is picked up while Jank runs.
//!
//! File format (`*.theme` in `<config dir>/themes/`), one `key = value` per line, `#` starts a comment:
//!
//! ```text
//! name = Night Shift
//! bg0 = #101014              # the 16 base colours
//! titlebar = #1a1030         # optional: one part of the window (falls back to a base colour)
//! radius = 6                 # sizes
//! background = night.png     # a picture behind the whole window …
//! background_mode = cover    # … cover | tile | stretch | center | fit
//! panel_opacity = 0.85       # panels let the background show through
//! ```
//!
//! Keys that are missing are taken from the built-in Graphite theme (extra colours from the base colour they name), so a
//! theme file can be tiny. Pictures are looked up next to the theme file and in its `images/` folder. [`Theme::template`]
//! writes a file that lists and explains every key.

use std::path::{Path, PathBuf};

/// The base colour slots of a theme, in file order.
pub const KEYS: [&str; 16] = ["bg0", "bg1", "bg2", "bg3", "line", "line_soft", "text", "dim", "hilite", "shade", "warn", "danger", "viewport", "axis_x", "axis_y", "axis_z"];

/// What the base colours are used for (for the template file).
const KEY_INFO: [&str; 16] = [
    "deepest surface: menu bar, ribbon, around the viewport",
    "panels",
    "controls (buttons, fields)",
    "hovered controls",
    "borders",
    "soft separators",
    "text",
    "dim text and captions",
    "top/left pixel bevel of buttons",
    "bottom/right pixel bevel of buttons",
    "warnings",
    "destructive actions (delete)",
    "background around the 3D picture",
    "X axis (red)",
    "Y axis (green)",
    "Z axis (blue)",
];

/// Extra colour slots for single parts of the window: `(key, falls back to, what it colours)`. The fallback is a base key,
/// another extra key or `accent`.
pub const EXTRA: [(&str, &str, &str); 20] = [
    ("titlebar", "bg0", "title bar and menu bar"),
    ("titlebar_text", "dim", "document title in the title bar"),
    ("ribbon", "bg0", "icon column at the far left"),
    ("toolstrip", "bg1", "tool column next to the viewport"),
    ("panel", "bg1", "side panels"),
    ("statusbar", "bg0", "status bar"),
    ("statusbar_text", "dim", "status bar text"),
    ("window", "bg1", "dialogs, menus and pop-ups"),
    ("window_border", "line", "border of dialogs, menus and pop-ups"),
    ("tooltip", "bg2", "tooltip background"),
    ("tooltip_text", "text", "tooltip text"),
    ("button", "bg2", "buttons"),
    ("button_hover", "bg3", "hovered buttons"),
    ("button_text", "text", "text on buttons"),
    ("input", "bg0", "text fields"),
    ("selection", "accent", "selected text and list items"),
    ("timeline", "bg1", "timeline background"),
    ("timeline_frame", "bg2", "frame cells in the timeline"),
    ("playhead", "accent", "current frame marker in the timeline"),
    ("link", "accent", "links"),
];

/// Sizes and switches: `(key, default, min, max, what it does)`.
pub const METRICS: [(&str, f32, f32, f32, &str); 11] = [
    ("radius", 3.0, 0.0, 16.0, "corner radius of buttons and fields, in points"),
    ("window_radius", 6.0, 0.0, 24.0, "corner radius of dialogs"),
    ("menu_radius", 4.0, 0.0, 16.0, "corner radius of menus and pop-ups"),
    ("border", 1.0, 0.0, 3.0, "width of control borders (0 = none)"),
    ("shadow", 1.0, 0.0, 2.0, "strength of dialog and menu shadows (0 = none)"),
    ("shadow_blur", 18.0, 0.0, 48.0, "softness of the shadows"),
    ("spacing", 1.0, 0.6, 1.8, "space between controls (1 = normal)"),
    ("panel_opacity", 1.0, 0.0, 1.0, "below 1 the background picture shows through the panels"),
    ("titlebar_height", 30.0, 24.0, 48.0, "height of Jank's own title bar"),
    ("scrollbar_width", 8.0, 4.0, 16.0, "width of scroll bars"),
    ("bevel", 1.0, 0.0, 1.0, "pixel bevel on buttons (0 = flat buttons)"),
];

/// Places a picture can be shown: `(slot, where)`.
pub const IMAGE_SLOTS: [(&str, &str); 9] = [
    ("background", "behind the whole window; shows through panels when panel_opacity is below 1"),
    ("titlebar", "the title bar"),
    ("ribbon", "the icon column at the far left"),
    ("panel", "the side panels"),
    ("statusbar", "the status bar"),
    ("viewport", "around the 3D picture (visible in Render mode and when the view is empty)"),
    ("welcome", "the welcome screen"),
    ("timeline", "behind the timeline"),
    ("logo", "replaces the Jank! wordmark in the title bar"),
];

/// How a picture fills its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFit {
    /// Fills the place, cropping what sticks out (keeps the proportions).
    Cover,
    /// Repeats at its own size.
    Tile,
    /// Fills the place exactly, distorting the proportions.
    Stretch,
    /// At its own size in the middle.
    Center,
    /// As large as fits entirely (keeps the proportions).
    Fit,
}
impl ImageFit {
    pub const ALL: [ImageFit; 5] = [ImageFit::Cover, ImageFit::Tile, ImageFit::Stretch, ImageFit::Center, ImageFit::Fit];
    pub fn id(self) -> &'static str {
        match self {
            ImageFit::Cover => "cover",
            ImageFit::Tile => "tile",
            ImageFit::Stretch => "stretch",
            ImageFit::Center => "center",
            ImageFit::Fit => "fit",
        }
    }
    pub fn parse(s: &str) -> Option<ImageFit> {
        let s = s.trim().to_lowercase();
        ImageFit::ALL.into_iter().find(|f| f.id() == s || (s == "centre" && *f == ImageFit::Center))
    }
    /// The part of the picture (`uv` 0..1) and of the place (`rect` min/max) to draw, for a picture of `img` pixels in a
    /// place of `area` points. Tile returns the picture's repeat count as the uv range (draw with a repeating texture).
    pub fn layout(self, img: [f32; 2], area: [f32; 2]) -> ([f32; 4], [f32; 4]) {
        let (iw, ih) = (img[0].max(1.0), img[1].max(1.0));
        let (aw, ah) = (area[0].max(1.0), area[1].max(1.0));
        match self {
            ImageFit::Stretch => ([0.0, 0.0, 1.0, 1.0], [0.0, 0.0, aw, ah]),
            ImageFit::Tile => ([0.0, 0.0, aw / iw, ah / ih], [0.0, 0.0, aw, ah]),
            ImageFit::Cover => {
                let k = (aw / iw).max(ah / ih);
                let (uw, uh) = (aw / (iw * k), ah / (ih * k));
                ([(1.0 - uw) * 0.5, (1.0 - uh) * 0.5, (1.0 + uw) * 0.5, (1.0 + uh) * 0.5], [0.0, 0.0, aw, ah])
            }
            ImageFit::Fit => {
                let k = (aw / iw).min(ah / ih);
                let (w, h) = (iw * k, ih * k);
                ([0.0, 0.0, 1.0, 1.0], [(aw - w) * 0.5, (ah - h) * 0.5, (aw + w) * 0.5, (ah + h) * 0.5])
            }
            ImageFit::Center => {
                let (x0, y0) = ((aw - iw) * 0.5, (ah - ih) * 0.5);
                // crop what does not fit
                let (cx, cy) = ((-x0).max(0.0) / iw, (-y0).max(0.0) / ih);
                ([cx, cy, 1.0 - cx, 1.0 - cy], [x0.max(0.0), y0.max(0.0), (x0 + iw).min(aw), (y0 + ih).min(ah)])
            }
        }
    }
}

/// A picture of a theme.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeImage {
    /// File name as written in the theme (relative to the theme file, or absolute).
    pub file: String,
    pub fit: ImageFit,
    /// 0..1.
    pub opacity: f32,
    /// Multiplies the picture's colours.
    pub tint: Option<[u8; 3]>,
}
impl Default for ThemeImage {
    fn default() -> Self {
        ThemeImage { file: String::new(), fit: ImageFit::Cover, opacity: 1.0, tint: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    /// Colours in the order of [`KEYS`].
    pub colors: [[u8; 3]; 16],
    /// Colours of single parts in the order of [`EXTRA`]; `None` = use the fallback.
    pub extra: [Option<[u8; 3]>; 20],
    /// Values in the order of [`METRICS`].
    pub metrics: [f32; 11],
    /// Pictures in the order of [`IMAGE_SLOTS`].
    pub images: [Option<ThemeImage>; 9],
    /// An accent that replaces the one chosen in Settings (optional).
    pub accent: Option<[u8; 3]>,
    /// Light themes get dark text on accent-coloured buttons and lighter shadows.
    pub light: bool,
    /// Where the theme was loaded from (`None` = built in).
    pub path: Option<PathBuf>,
}

const fn hex(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

const GRAPHITE: [[u8; 3]; 16] = [
    hex(0x131316),
    hex(0x1b1b1f),
    hex(0x26262b),
    hex(0x313137),
    hex(0x3a3a41),
    hex(0x27272c),
    hex(0xdcdce0),
    hex(0x8b8b94),
    hex(0x3d3d45),
    hex(0x0d0d10),
    hex(0xd6a74a),
    hex(0xd65f6b),
    hex(0x131316),
    hex(0xd06464),
    hex(0x84b06a),
    hex(0x6493d2),
];

fn default_metrics() -> [f32; 11] {
    METRICS.map(|m| m.1)
}

fn fmt_hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
fn fmt_num(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl Theme {
    /// A stable id for the settings file: the name in lower case with dashes.
    pub fn id(&self) -> String {
        self.name.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect()
    }
    /// A base or extra colour by key (extra colours resolved through their fallbacks; `accent` resolves to `accent`).
    pub fn color(&self, key: &str) -> [u8; 3] {
        self.color_with(key, self.accent.unwrap_or([0x8a, 0x7b, 0xf2]))
    }
    fn color_with(&self, key: &str, accent: [u8; 3]) -> [u8; 3] {
        let mut k = key;
        for _ in 0..8 {
            if k == "accent" {
                return accent;
            }
            if let Some(i) = KEYS.iter().position(|x| *x == k) {
                return self.colors[i];
            }
            match EXTRA.iter().position(|e| e.0 == k) {
                Some(i) => match self.extra[i] {
                    Some(c) => return c,
                    None => k = EXTRA[i].1,
                },
                None => break,
            }
        }
        [255, 0, 255]
    }
    /// Every extra colour with its fallback applied, in the order of [`EXTRA`].
    pub fn resolved_extra(&self, accent: [u8; 3]) -> [[u8; 3]; 20] {
        std::array::from_fn(|i| self.color_with(EXTRA[i].0, accent))
    }
    pub fn set_extra(&mut self, key: &str, c: [u8; 3]) {
        if let Some(i) = EXTRA.iter().position(|e| e.0 == key) {
            self.extra[i] = Some(c);
        }
    }
    pub fn set_metric(&mut self, key: &str, v: f32) {
        if let Some(i) = METRICS.iter().position(|m| m.0 == key) {
            self.metrics[i] = v.clamp(METRICS[i].2, METRICS[i].3);
        }
    }
    pub fn metric(&self, key: &str) -> f32 {
        METRICS.iter().position(|m| m.0 == key).map_or(0.0, |i| self.metrics[i])
    }
    pub fn image(&self, slot: &str) -> Option<&ThemeImage> {
        IMAGE_SLOTS.iter().position(|s| s.0 == slot).and_then(|i| self.images[i].as_ref())
    }
    /// Where the picture of a slot is on disk: next to the theme file, then in its `images/` folder (absolute names as
    /// they are). `None` when the slot has no picture or the file does not exist.
    pub fn image_path(&self, slot: &str) -> Option<PathBuf> {
        let img = self.image(slot)?;
        let f = Path::new(&img.file);
        if f.is_absolute() {
            return f.exists().then(|| f.to_path_buf());
        }
        let dir = self.path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).or_else(themes_dir)?;
        [dir.join(f), dir.join("images").join(f)].into_iter().find(|p| p.is_file())
    }
    /// Parses a theme file. Unknown keys and bad values are reported, the rest is used.
    pub fn parse(text: &str) -> (Theme, Vec<String>) {
        let mut t = builtin().remove(0);
        t.name = "Untitled theme".into();
        let mut problems = vec![];
        let mut light = None;
        let mut img: [ThemeImage; 9] = std::array::from_fn(|_| ThemeImage::default());
        let mut img_set = [false; 9];
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                problems.push(format!("line {}: missing '='", n + 1));
                continue;
            };
            let k = k.trim().to_lowercase();
            // strip a trailing comment after the value
            let v = v.trim();
            let v = match v.strip_prefix('#') {
                Some(rest) => format!("#{}", rest.split_whitespace().next().unwrap_or("")),
                None => v.split(" #").next().unwrap_or("").trim().to_string(),
            };
            let bad_colour = |problems: &mut Vec<String>| problems.push(format!("line {}: \"{v}\" is not a colour like #12ab34", n + 1));
            let bad_number = |problems: &mut Vec<String>| problems.push(format!("line {}: \"{v}\" is not a number", n + 1));
            // pictures: `background = file`, `<slot>_image = file`, `<slot>_mode`, `<slot>_opacity`, `<slot>_tint` (also
            // written `<slot>_image_mode` and so on). A size key wins: `panel_opacity` is the panels' own opacity, the
            // panel picture's is `panel_image_opacity`.
            let is_metric = METRICS.iter().any(|m| m.0 == k);
            let slot_key = IMAGE_SLOTS.iter().enumerate().find_map(|(i, (slot, _))| {
                if is_metric {
                    return None;
                }
                if k == "background" && *slot == "background" {
                    return Some((i, "image"));
                }
                let rest = k.strip_prefix(slot)?.strip_prefix('_')?;
                let rest = rest.strip_prefix("image_").unwrap_or(rest);
                ["image", "mode", "fit", "opacity", "tint"].into_iter().find(|x| *x == rest).map(|x| (i, x))
            });
            if let Some((i, what)) = slot_key {
                match what {
                    "image" => {
                        img[i].file = v.clone();
                        img_set[i] = !v.is_empty() && !v.eq_ignore_ascii_case("none");
                    }
                    "mode" | "fit" => match ImageFit::parse(&v) {
                        Some(f) => img[i].fit = f,
                        None => problems.push(format!("line {}: \"{v}\" is not one of cover, tile, stretch, center, fit", n + 1)),
                    },
                    "opacity" => match v.parse::<f32>() {
                        Ok(x) if x.is_finite() => img[i].opacity = x.clamp(0.0, 1.0),
                        _ => bad_number(&mut problems),
                    },
                    _ => match parse_hex(&v) {
                        Some(c) => img[i].tint = Some(c),
                        None => bad_colour(&mut problems),
                    },
                }
                continue;
            }
            match k.as_str() {
                "name" => t.name = v.chars().take(40).collect(),
                "light" => light = Some(matches!(v.to_lowercase().as_str(), "true" | "yes" | "1" | "on")),
                "accent" => match parse_hex(&v) {
                    Some(c) => t.accent = Some(c),
                    None => bad_colour(&mut problems),
                },
                _ => {
                    if let Some(i) = KEYS.iter().position(|x| *x == k) {
                        match parse_hex(&v) {
                            Some(c) => t.colors[i] = c,
                            None => bad_colour(&mut problems),
                        }
                    } else if let Some(i) = EXTRA.iter().position(|x| x.0 == k) {
                        match parse_hex(&v) {
                            Some(c) => t.extra[i] = Some(c),
                            None => bad_colour(&mut problems),
                        }
                    } else if let Some(i) = METRICS.iter().position(|x| x.0 == k) {
                        match v.parse::<f32>() {
                            Ok(x) if x.is_finite() => t.metrics[i] = x.clamp(METRICS[i].2, METRICS[i].3),
                            _ => bad_number(&mut problems),
                        }
                    } else {
                        problems.push(format!("line {}: unknown key \"{k}\"", n + 1));
                    }
                }
            }
        }
        for (i, im) in img.into_iter().enumerate() {
            t.images[i] = img_set[i].then_some(im);
        }
        // light or dark is guessed from the panel colour unless the file says so
        let bg = t.colors[1];
        t.light = light.unwrap_or((bg[0] as u32 * 3 + bg[1] as u32 * 6 + bg[2] as u32) / 10 > 128);
        (t, problems)
    }
    /// The theme as a short file that [`Theme::parse`] reads back (only what differs from the defaults besides colours).
    pub fn to_text(&self) -> String {
        let mut s = format!("# Jank! interface theme. Edit and save; missing keys use the Graphite theme.\nname = {}\nlight = {}\n", self.name, self.light);
        if let Some(a) = self.accent {
            s.push_str(&format!("accent = {}\n", fmt_hex(a)));
        }
        for (k, c) in KEYS.iter().zip(self.colors.iter()) {
            s.push_str(&format!("{k} = {}\n", fmt_hex(*c)));
        }
        for (e, c) in EXTRA.iter().zip(self.extra.iter()) {
            if let Some(c) = c {
                s.push_str(&format!("{} = {}\n", e.0, fmt_hex(*c)));
            }
        }
        for (m, v) in METRICS.iter().zip(self.metrics.iter()) {
            if (*v - m.1).abs() > 1e-4 {
                s.push_str(&format!("{} = {}\n", m.0, fmt_num(*v)));
            }
        }
        for (slot, im) in IMAGE_SLOTS.iter().zip(self.images.iter()) {
            if let Some(im) = im {
                s.push_str(&image_lines(slot.0, im));
            }
        }
        s
    }
    /// The theme as a complete, commented file: every key with an explanation, unset extras and pictures as comments to
    /// fill in. "Save a copy to edit" writes this.
    pub fn template(&self) -> String {
        let mut s = String::new();
        s.push_str("# Jank! interface theme\n#\n# Edit any value and save the file: Jank picks the change up while it runs.\n");
        s.push_str("# Colours are #rrggbb. Lines starting with # are comments; remove the # in front of a key to use it.\n");
        s.push_str("# Missing keys fall back to the Graphite theme (extra colours to the colour named in their comment).\n\n");
        s.push_str(&format!("name = {}\n", self.name));
        s.push_str(&format!("light = {}                  # true for light themes (dark text on bright surfaces)\n", self.light));
        match self.accent {
            Some(a) => s.push_str(&format!("accent = {}             # replaces the accent chosen in Settings\n", fmt_hex(a))),
            None => s.push_str("# accent = #8a7bf2           # replaces the accent chosen in Settings\n"),
        }
        s.push_str("\n# ---- base colours\n");
        for (i, k) in KEYS.iter().enumerate() {
            s.push_str(&format!("{:<16}= {}   # {}\n", k, fmt_hex(self.colors[i]), KEY_INFO[i]));
        }
        s.push_str("\n# ---- single parts of the window (each falls back to the colour after \"default:\")\n");
        for (i, e) in EXTRA.iter().enumerate() {
            match self.extra[i] {
                Some(c) => s.push_str(&format!("{:<16}= {}   # {} (default: {})\n", e.0, fmt_hex(c), e.2, e.1)),
                None => s.push_str(&format!("# {:<14}= {}   # {} (default: {})\n", e.0, fmt_hex(self.color(e.0)), e.2, e.1)),
            }
        }
        s.push_str("\n# ---- sizes\n");
        for (i, m) in METRICS.iter().enumerate() {
            let v = self.metrics[i];
            let line = format!("{:<16}= {:<8}# {} ({} to {}, default {})\n", m.0, fmt_num(v), m.4, fmt_num(m.2), fmt_num(m.3), fmt_num(m.1));
            if (v - m.1).abs() > 1e-4 {
                s.push_str(&line);
            } else {
                s.push_str(&format!("# {line}"));
            }
        }
        s.push_str("\n# ---- pictures: PNG, JPEG, BMP, TGA or PPM, next to this file or in an images/ folder beside it\n");
        s.push_str("# <place>_image = file   <place>_mode = cover | tile | stretch | center | fit   <place>_opacity = 0..1   <place>_tint = #rrggbb\n");
        s.push_str("# (the panel picture uses panel_image_mode, panel_image_opacity, panel_image_tint: panel_opacity is a size above)\n");
        for (i, (slot, info)) in IMAGE_SLOTS.iter().enumerate() {
            match &self.images[i] {
                Some(im) => {
                    s.push_str(&format!("# {slot}: {info}\n"));
                    s.push_str(&image_lines(slot, im));
                }
                None => s.push_str(&format!("# {slot}_image = picture.png   # {info}\n")),
            }
        }
        s
    }
}

fn image_lines(slot: &str, im: &ThemeImage) -> String {
    // `<slot>_opacity` would clash with a size key (`panel_opacity`): write `<slot>_image_opacity` there
    let pre = if METRICS.iter().any(|m| m.0 == format!("{slot}_opacity")) { format!("{slot}_image") } else { slot.to_string() };
    let mut s = format!("{slot}_image = {}\n{pre}_mode = {}\n", im.file, im.fit.id());
    if im.opacity < 1.0 {
        s.push_str(&format!("{pre}_opacity = {}\n", fmt_num(im.opacity)));
    }
    if let Some(t) = im.tint {
        s.push_str(&format!("{pre}_tint = {}\n", fmt_hex(t)));
    }
    s
}

fn parse_hex(v: &str) -> Option<[u8; 3]> {
    let h = v.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(hex(n))
}

fn theme(name: &str, colors: [[u8; 3]; 16], accent: Option<[u8; 3]>, light: bool) -> Theme {
    Theme { name: name.into(), colors, extra: [None; 20], metrics: default_metrics(), images: Default::default(), accent, light, path: None }
}

/// The themes built into the program. The first one is the default.
pub fn builtin() -> Vec<Theme> {
    let mut midnight = GRAPHITE;
    for (i, c) in [0x0e1018, 0x151824, 0x1f2333, 0x2a2f44, 0x343a52, 0x1e2130, 0xd8dcf0, 0x8389a6, 0x353b55, 0x08090f, 0xe0b050, 0xe0607a, 0x0e1018].iter().enumerate() {
        midnight[i] = hex(*c);
    }
    let mut paper = GRAPHITE;
    for (i, c) in [0xdcdad4, 0xeceae4, 0xf8f7f3, 0xffffff, 0xb8b5ac, 0xd6d3cb, 0x26252a, 0x6b6964, 0xffffff, 0xa9a69e, 0xa86f10, 0xb8343f, 0xc9c7c0].iter().enumerate() {
        paper[i] = hex(*c);
    }
    paper[13] = hex(0xc0392b);
    paper[14] = hex(0x4c8a2f);
    paper[15] = hex(0x2f63b0);
    let mut psx = GRAPHITE;
    for (i, c) in [0x1c1f26, 0x2a2e38, 0x3a404d, 0x4a5161, 0x596172, 0x323744, 0xe6e8ee, 0x9aa1b2, 0x5d6578, 0x111318, 0xe8c050, 0xe86a6a, 0x101216].iter().enumerate() {
        psx[i] = hex(*c);
    }
    let mut crt = GRAPHITE;
    for (i, c) in [0x0c0a06, 0x14110a, 0x201b10, 0x2c2516, 0x3d331c, 0x1d180e, 0xffc56b, 0xb08443, 0x3a301a, 0x050402, 0xffd27a, 0xff6a4a, 0x0c0a06].iter().enumerate() {
        crt[i] = hex(*c);
    }
    let mut krita = GRAPHITE;
    for (i, c) in [0x232323, 0x2f2f2f, 0x3b3b3b, 0x474747, 0x505050, 0x383838, 0xe8e8e8, 0x9a9a9a, 0x4a4a4a, 0x181818, 0xe0b040, 0xe05a5a, 0x2a2a2a].iter().enumerate() {
        krita[i] = hex(*c);
    }
    let mut neon = GRAPHITE;
    for (i, c) in [0x0d0a18, 0x151026, 0x221a3a, 0x2e2450, 0x3d3266, 0x241c3c, 0xeae4ff, 0x9a90c0, 0x3a3060, 0x07050e, 0xffc36b, 0xff5c8a, 0x0a0814].iter().enumerate() {
        neon[i] = hex(*c);
    }
    let mut neon = theme("Neon Night", neon, Some([0xff, 0x4f, 0xd8]), false);
    for (k, c) in [("titlebar", 0x1a0f33), ("titlebar_text", 0xc8a8ff), ("ribbon", 0x120c24), ("statusbar", 0x1a0f33), ("statusbar_text", 0xc8a8ff), ("timeline", 0x100b20), ("playhead", 0x4fe3ff), ("link", 0x4fe3ff), ("window_border", 0x5a3d99)] {
        neon.set_extra(k, hex(c));
    }
    for (k, v) in [("radius", 5.0), ("window_radius", 10.0), ("menu_radius", 7.0), ("shadow", 1.4), ("shadow_blur", 26.0), ("bevel", 0.0)] {
        neon.set_metric(k, v);
    }
    vec![
        theme("Graphite", GRAPHITE, None, false),
        theme("Midnight", midnight, None, false),
        theme("Studio Grey", krita, None, false),
        theme("PlayStation", psx, Some([0x5c, 0x9c, 0xf0]), false),
        theme("Amber CRT", crt, Some([0xff, 0xa0, 0x30]), false),
        theme("Paper", paper, Some([0x3d, 0x6f, 0xd8]), true),
        neon,
    ]
}

/// Folder with the user's theme files.
pub fn themes_dir() -> Option<PathBuf> {
    crate::settings::config_dir().map(|d| d.join("themes"))
}

/// Built-in themes followed by the ones found in `dir` (`*.theme`). A file whose name matches a
/// built-in theme replaces it. Problems are returned per file.
pub fn load_all(dir: Option<&Path>) -> (Vec<Theme>, Vec<String>) {
    let mut out = builtin();
    let mut problems = vec![];
    let Some(dir) = dir else { return (out, problems) };
    let Ok(rd) = std::fs::read_dir(dir) else { return (out, problems) };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("theme"))).collect();
    files.sort();
    for p in files.into_iter().take(64) {
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        if text.len() > 64 * 1024 {
            continue;
        }
        let (mut t, pr) = Theme::parse(&text);
        let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
        problems.extend(pr.into_iter().map(|m| format!("{fname}: {m}")));
        t.path = Some(p);
        match out.iter().position(|o| o.id() == t.id()) {
            Some(i) => out[i] = t,
            None => out.push(t),
        }
    }
    (out, problems)
}

/// Writes a theme into the themes folder (as `<id>.theme`) and returns the file path.
pub fn save(t: &Theme, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let p = dir.join(format!("{}.theme", t.id()));
    std::fs::write(&p, t.to_text()).map_err(|e| format!("Could not write {}: {e}", p.display()))?;
    Ok(p)
}

/// Writes a theme into the themes folder as a complete, commented file ([`Theme::template`]) and returns its path.
pub fn save_template(t: &Theme, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let p = dir.join(format!("{}.theme", t.id()));
    std::fs::write(&p, t.template()).map_err(|e| format!("Could not write {}: {e}", p.display()))?;
    Ok(p)
}

const SHRINE_THEME: &str = include_str!("../assets/themes/shrine-night.theme");
const SHRINE_PICTURE: &[u8] = include_bytes!("../assets/themes/shrine-night.png");
const BLUEPRINT_THEME: &str = include_str!("../assets/themes/blueprint.theme");

/// A 32×32 blueprint grid tile (drawn here, so it needs no file in the program).
fn blueprint_tile() -> Vec<u8> {
    let n = 32u32;
    let mut px = vec![[0x16u8, 0x3d, 0x6b, 255]; (n * n) as usize];
    for y in 0..n {
        for x in 0..n {
            let major = x == 0 || y == 0;
            let minor = x % 8 == 0 || y % 8 == 0;
            if major {
                px[(y * n + x) as usize] = [0x3f, 0x72, 0xa8, 255];
            } else if minor {
                px[(y * n + x) as usize] = [0x22, 0x4d, 0x7f, 255];
            }
        }
    }
    crate::encode::png::encode_png_pixels(n, n, &px, false)
}

/// Writes the example themes and their pictures into `dir` (pictures into `dir/images`). Returns the ids of the themes.
pub fn install_examples(dir: &Path) -> Result<Vec<String>, String> {
    let img = dir.join("images");
    std::fs::create_dir_all(&img).map_err(|e| format!("Could not create {}: {e}", img.display()))?;
    let write = |p: PathBuf, b: &[u8]| std::fs::write(&p, b).map_err(|e| format!("Could not write {}: {e}", p.display()));
    write(img.join("shrine-night.png"), SHRINE_PICTURE)?;
    write(img.join("blueprint-grid.png"), &blueprint_tile())?;
    let mut ids = vec![];
    for text in [SHRINE_THEME, BLUEPRINT_THEME] {
        let (t, _) = Theme::parse(text);
        write(dir.join(format!("{}.theme", t.id())), text.as_bytes())?;
        ids.push(t.id());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_unique_and_round_trip() {
        let b = builtin();
        let mut ids: Vec<String> = b.iter().map(|t| t.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), b.len());
        for t in &b {
            let (back, p) = Theme::parse(&t.to_text());
            assert!(p.is_empty(), "{p:?}");
            assert_eq!(back.colors, t.colors);
            assert_eq!(back.accent, t.accent);
            assert_eq!(back.name, t.name);
            assert_eq!(back.light, t.light);
        }
        assert!(b.iter().any(|t| t.light));
    }

    #[test]
    fn panel_opacity_is_a_size_not_the_panel_picture() {
        let (t, p) = Theme::parse("panel_opacity = 0.5\npanel_image = a.png\npanel_image_opacity = 0.3\nbackground = b.png\nbackground_opacity = 0.7\n");
        assert!(p.is_empty(), "{p:?}");
        assert!((t.metric("panel_opacity") - 0.5).abs() < 1e-6);
        assert!((t.image("panel").unwrap().opacity - 0.3).abs() < 1e-6);
        assert!((t.image("background").unwrap().opacity - 0.7).abs() < 1e-6);
        // both spellings of the picture keys, and the written file reads back the same
        let (u, _) = Theme::parse("background = b.png\nbackground_image_mode = tile\n");
        assert_eq!(u.image("background").unwrap().fit, ImageFit::Tile);
        for text in [t.to_text(), t.template()] {
            let (back, p) = Theme::parse(&text);
            assert!(p.is_empty(), "{p:?}");
            assert_eq!(back.metrics, t.metrics);
            assert_eq!(back.images, t.images);
        }
        // the example themes really make their panels see-through
        for text in [SHRINE_THEME, BLUEPRINT_THEME] {
            let (e, p) = Theme::parse(text);
            assert!(p.is_empty(), "{p:?}");
            assert!(e.metric("panel_opacity") < 1.0 && e.image("background").is_some());
        }
    }

    #[test]
    fn partial_files_fall_back_and_report_problems() {
        let (t, p) = Theme::parse("name = Tiny\nbg0 = #010203 # deep\ntext = banana\nwhat = #ffffff\n");
        assert_eq!(t.name, "Tiny");
        assert_eq!(t.color("bg0"), [1, 2, 3]);
        assert_eq!(t.color("text"), builtin()[0].color("text"));
        assert_eq!(p.len(), 2, "{p:?}");
    }

    #[test]
    fn files_in_the_folder_are_loaded_and_override_builtins() {
        let dir = std::env::temp_dir().join(format!("jank_themes_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = builtin()[1].clone();
        t.colors[0] = [9, 9, 9];
        save(&t, &dir).unwrap();
        std::fs::write(dir.join("mine.theme"), "name = Mine\nbg1 = #f0f0f0\n").unwrap();
        let (all, p) = load_all(Some(&dir));
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(all.len(), builtin().len() + 1);
        assert_eq!(all.iter().find(|x| x.id() == "midnight").unwrap().colors[0], [9, 9, 9]);
        let mine = all.iter().find(|x| x.name == "Mine").unwrap();
        assert!(mine.light, "a light panel colour makes a light theme");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extras_metrics_and_pictures_parse_fall_back_and_round_trip() {
        let text = "name = Rich\nbg0 = #101010\ntitlebar = #203040\nradius = 7\nshadow = 9\nspacing = x\nbackground = night.png\nbackground_mode = tile\nbackground_opacity = 0.5\npanel_image = wood.png # grain\npanel_tint = #ff8000\nstatusbar_mode = sideways\n";
        let (t, p) = Theme::parse(text);
        assert_eq!(p.len(), 2, "a bad number and a bad mode: {p:?}");
        assert_eq!(t.color("titlebar"), [0x20, 0x30, 0x40]);
        assert_eq!(t.color("ribbon"), [0x10, 0x10, 0x10], "an unset extra falls back to its base colour");
        assert_eq!(t.color("window"), t.color("bg1"));
        assert_eq!(t.resolved_extra([1, 2, 3])[EXTRA.iter().position(|e| e.0 == "playhead").unwrap()], [1, 2, 3], "accent fallbacks use the live accent");
        assert_eq!(t.metric("radius"), 7.0);
        assert_eq!(t.metric("shadow"), 2.0, "sizes are clamped");
        assert_eq!(t.metric("spacing"), 1.0, "a bad value keeps the default");
        let bg = t.image("background").unwrap();
        assert_eq!((bg.file.as_str(), bg.fit, bg.opacity), ("night.png", ImageFit::Tile, 0.5));
        let panel = t.image("panel").unwrap();
        assert_eq!((panel.file.as_str(), panel.tint), ("wood.png", Some([0xff, 0x80, 0x00])));
        assert!(t.image("statusbar").is_none(), "a mode without a picture adds no picture");
        for text in [t.to_text(), t.template()] {
            let (back, p) = Theme::parse(&text);
            assert!(p.is_empty(), "{p:?}\n{text}");
            assert_eq!(back.colors, t.colors);
            assert_eq!(back.extra, t.extra);
            assert_eq!(back.metrics, t.metrics);
            assert_eq!(back.images, t.images);
            assert_eq!(back.name, t.name);
        }
        // every builtin's template reads back to the same theme
        for b in builtin() {
            let (back, p) = Theme::parse(&b.template());
            assert!(p.is_empty(), "{p:?}");
            assert_eq!((back.colors, back.extra, back.metrics, back.accent, back.light), (b.colors, b.extra, b.metrics, b.accent, b.light));
        }
    }

    #[test]
    fn picture_layouts_cover_fit_and_tile() {
        let (uv, r) = ImageFit::Cover.layout([200.0, 100.0], [100.0, 100.0]);
        assert_eq!(r, [0.0, 0.0, 100.0, 100.0]);
        assert!((uv[0] - 0.25).abs() < 1e-6 && (uv[2] - 0.75).abs() < 1e-6 && uv[1] == 0.0 && uv[3] == 1.0);
        let (uv, r) = ImageFit::Fit.layout([200.0, 100.0], [100.0, 100.0]);
        assert_eq!(uv, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(r, [0.0, 25.0, 100.0, 75.0]);
        let (uv, _) = ImageFit::Tile.layout([50.0, 25.0], [100.0, 100.0]);
        assert_eq!(uv, [0.0, 0.0, 2.0, 4.0]);
        let (uv, r) = ImageFit::Center.layout([40.0, 40.0], [100.0, 20.0]);
        assert_eq!(r, [30.0, 0.0, 70.0, 20.0]);
        assert!((uv[1] - 0.25).abs() < 1e-6 && (uv[3] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn pictures_are_found_next_to_the_theme_and_in_images() {
        let dir = std::env::temp_dir().join(format!("jank_theme_pics_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("images")).unwrap();
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        std::fs::write(dir.join("images").join("b.png"), b"x").unwrap();
        std::fs::write(dir.join("pics.theme"), "name = Pics\nbackground = a.png\npanel_image = b.png\nlogo_image = missing.png\n").unwrap();
        let (all, _) = load_all(Some(&dir));
        let t = all.iter().find(|t| t.name == "Pics").unwrap();
        assert_eq!(t.image_path("background"), Some(dir.join("a.png")));
        assert_eq!(t.image_path("panel"), Some(dir.join("images").join("b.png")));
        assert_eq!(t.image_path("logo"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn example_themes_install_parse_cleanly_and_find_their_pictures() {
        for text in [SHRINE_THEME, BLUEPRINT_THEME] {
            let (_, p) = Theme::parse(text);
            assert!(p.is_empty(), "{p:?}");
        }
        let dir = std::env::temp_dir().join(format!("jank_theme_examples_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ids = install_examples(&dir).unwrap();
        assert_eq!(ids.len(), 2);
        let (all, p) = load_all(Some(&dir));
        assert!(p.is_empty(), "{p:?}");
        for id in &ids {
            let t = all.iter().find(|t| &t.id() == id).unwrap();
            for (slot, _) in IMAGE_SLOTS {
                if t.image(slot).is_some() {
                    let path = t.image_path(slot).unwrap_or_else(|| panic!("{id}: {slot} picture exists"));
                    let bytes = std::fs::read(path).unwrap();
                    assert!(crate::decode::image::decode_image("x.png", &bytes).is_ok());
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
