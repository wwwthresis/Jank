//! Visual system of Jank!: an Obsidian-like graphite workspace (deep flat surfaces, hairline
//! separators, one accent colour) dressed with Aseprite-style pixel details (16×16 pixel icons,
//! bevelled controls with a one-pixel highlight and shade, crisp square-ish corners).
//!
//! Rules that every widget follows:
//! * Surfaces step from `BG0` (deepest: ribbon, viewport surround) through `BG1` (panels) and `BG2`
//!   (controls) to `BG3` (hover). `HILITE` and `SHADE` are only used for the 1 px bevel of controls.
//! * [`accent()`] marks selection, the active tool or tab and focus, and nothing else. The user picks
//!   it in Settings → Appearance; it is read at paint time, so a change applies immediately.
//! * `WARN` and `DANGER` are used only for warnings and destructive actions.
//! * `AXIS_X/Y/Z` are used only for the three coordinate axes (gizmos and XYZ fields).
//! * Corners are lightly rounded (`RADIUS`); there are no glows or gradients.

use crate::compat::cr;
use eframe::egui::{self, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use psxcore::settings::{Density, TextSize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// The colours of the active theme (see `psxcore::themes`), packed RGB in the order of
/// `psxcore::themes::KEYS`. Read at paint time, so switching themes applies immediately.
static PALETTE: [AtomicU32; 16] = [
    AtomicU32::new(0x131316),
    AtomicU32::new(0x1b1b1f),
    AtomicU32::new(0x26262b),
    AtomicU32::new(0x313137),
    AtomicU32::new(0x3a3a41),
    AtomicU32::new(0x27272c),
    AtomicU32::new(0xdcdce0),
    AtomicU32::new(0x8b8b94),
    AtomicU32::new(0x3d3d45),
    AtomicU32::new(0x0d0d10),
    AtomicU32::new(0xd6a74a),
    AtomicU32::new(0xd65f6b),
    AtomicU32::new(0x131316),
    AtomicU32::new(0xd06464),
    AtomicU32::new(0x84b06a),
    AtomicU32::new(0x6493d2),
];
static LIGHT: AtomicU32 = AtomicU32::new(0);

fn slot(i: usize) -> Color32 {
    let v = PALETTE[i].load(Ordering::Relaxed);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
macro_rules! colour {
    ($($(#[$m:meta])* $name:ident = $i:expr;)*) => {
        $( $(#[$m])* #[allow(non_snake_case)] #[inline] pub fn $name() -> Color32 { slot($i) } )*
    };
}
colour! {
    /// Deepest surface: ribbon, menu bar, viewport surround.
    BG0 = 0;
    /// Panels.
    BG1 = 1;
    /// Controls.
    BG2 = 2;
    /// Hovered controls.
    BG3 = 3;
    LINE = 4;
    LINE_SOFT = 5;
    TEXT = 6;
    DIM = 7;
    /// Top/left bevel highlight of a raised control (Aseprite-style pixel bevel).
    HILITE = 8;
    /// Bottom/right bevel shade of a raised control.
    SHADE = 9;
    WARN = 10;
    DANGER = 11;
    /// Background around the 3D picture.
    VIEWPORT = 12;
    AXIS_X = 13;
    AXIS_Y = 14;
    AXIS_Z = 15;
}
/// True for light themes (dark text on bright surfaces).
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed) != 0
}
/// The strongest ink for text and icons on selected or accent-tinted surfaces:
/// white on dark themes, near-black on light ones.
#[allow(non_snake_case)]
pub fn BRIGHT() -> Color32 {
    if is_light() {
        Color32::from_rgb(0x12, 0x14, 0x1a)
    } else {
        Color32::WHITE
    }
}
/// Onion-skin ghosts: previous frames are warm, next frames are green (functional colours like the axis colours).
pub const ONION_PREV: Color32 = Color32::from_rgb(0xd9, 0x7a, 0x5c);
pub const ONION_NEXT: Color32 = Color32::from_rgb(0x5f, 0xb4, 0x82);

/// Corner radius for controls and windows, in points.
pub const RADIUS: f32 = 3.0;

/// The accent colour as packed RGB; violet (Obsidian's default) until the settings are applied.
static ACCENT_RGB: AtomicU32 = AtomicU32::new(0x8a7bf2);

/// The single interaction colour chosen in Settings → Appearance.
pub fn accent() -> Color32 {
    let v = ACCENT_RGB.load(Ordering::Relaxed);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}
pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// Installs the Inter typeface (SIL Open Font License 1.1, see `assets/fonts/Inter-LICENSE-OFL.txt`).
/// The built-in egui fonts stay behind it as a fallback for symbols and emoji.
fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("inter".into(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf"))));
    fonts.font_data.insert("inter-semibold".into(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf"))));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    fonts.families.insert(FontFamily::Name("strong".into()), vec!["inter-semibold".into(), "inter".into()]);
    ctx.set_fonts(fonts);
}

/// The appearance choices that [`apply_look`] turns into egui visuals and spacing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    pub accent: [u8; 3],
    pub density: Density,
    pub text: TextSize,
    /// Theme colours in the order of `psxcore::themes::KEYS`.
    pub colors: [[u8; 3]; 16],
    pub light: bool,
}
impl Default for Look {
    fn default() -> Self {
        let t = &psxcore::themes::builtin()[0];
        Self { accent: [0x8a, 0x7b, 0xf2], density: Density::Compact, text: TextSize::Normal, colors: t.colors, light: false }
    }
}

/// Installs the fonts and the default look; called once at start-up.
pub fn apply(ctx: &Context) {
    install_fonts(ctx);
    apply_look(ctx, Look::default());
}

/// Applies colours, spacing and text sizes. Cheap enough to call whenever a setting changes.
pub fn apply_look(ctx: &Context, look: Look) {
    for (i, c) in look.colors.iter().enumerate() {
        PALETTE[i].store(((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32, Ordering::Relaxed);
    }
    LIGHT.store(look.light as u32, Ordering::Relaxed);
    let [r, g, b] = look.accent;
    ACCENT_RGB.store(((r as u32) << 16) | ((g as u32) << 8) | b as u32, Ordering::Relaxed);
    let acc = accent();
    let roomy = look.density == Density::Comfortable;
    let body = look.text.points();
    let mut v = if look.light { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.override_text_color = Some(TEXT());
    v.panel_fill = BG1();
    v.window_fill = BG1();
    v.extreme_bg_color = BG0();
    v.faint_bg_color = BG2();
    v.code_bg_color = BG0();
    v.hyperlink_color = acc;
    v.window_stroke = crate::compat::stroke(1.0, LINE());
    v.window_corner_radius = cr(RADIUS + 3.0);
    v.menu_corner_radius = cr(RADIUS + 1.0);
    v.window_shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(if look.light { 40 } else { 110 }) };
    v.popup_shadow = egui::Shadow { offset: [0, 3], blur: 10, spread: 0, color: Color32::from_black_alpha(if look.light { 35 } else { 100 }) };
    v.selection.bg_fill = alpha(acc, 80);
    v.selection.stroke = crate::compat::stroke(1.0, acc);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.5 };
    v.striped = false;
    v.text_cursor.stroke = crate::compat::stroke(2.0, acc);
    for (w, fill, stroke) in [
        (&mut v.widgets.noninteractive, BG1(), LINE_SOFT()),
        (&mut v.widgets.inactive, BG2(), LINE()),
        (&mut v.widgets.hovered, BG3(), mix(LINE(), acc, 0.45)),
        (&mut v.widgets.active, BG3(), acc),
        (&mut v.widgets.open, BG3(), LINE()),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = cr(RADIUS);
        w.bg_stroke = crate::compat::stroke(1.0, stroke);
        w.fg_stroke = crate::compat::stroke(1.0, TEXT());
        w.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        // UI text must not be selectable or copyable
        s.interaction.selectable_labels = false;
        s.spacing.item_spacing = if roomy { egui::vec2(8.0, 6.0) } else { egui::vec2(6.0, 4.0) };
        s.spacing.button_padding = if roomy { egui::vec2(10.0, 5.0) } else { egui::vec2(8.0, 3.0) };
        s.spacing.slider_width = 120.0;
        s.spacing.interact_size = if roomy { egui::vec2(28.0, 26.0) } else { egui::vec2(26.0, 22.0) };
        s.spacing.menu_margin = egui::Margin::same(5);
        s.spacing.window_margin = egui::Margin::same(if roomy { 12 } else { 10 });
        s.spacing.tooltip_width = 320.0;
        // thin, always-visible scrollbars that take their own space instead of floating over content
        s.spacing.scroll = egui::style::ScrollStyle::solid();
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.scroll.handle_min_length = 24.0;
        s.animation_time = 0.08;
        s.text_styles.insert(TextStyle::Body, FontId::proportional(body));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(body));
        s.text_styles.insert(TextStyle::Small, FontId::proportional((body - 2.0).max(10.0)));
        s.text_styles.insert(TextStyle::Heading, FontId::new(body + 2.0, FontFamily::Name("strong".into())));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(body - 1.0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accent_round_trips_through_the_packed_value() {
        ACCENT_RGB.store(0x12ab34, Ordering::Relaxed);
        assert_eq!(accent(), Color32::from_rgb(0x12, 0xab, 0x34));
        ACCENT_RGB.store(0x8a7bf2, Ordering::Relaxed);
    }
}
