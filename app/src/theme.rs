//! Visual system of Jank!: a restrained graphite palette with one functional accent colour, compact
//! rectangular controls and the Inter typeface.
//!
//! Rules that every widget follows:
//! * Surfaces step from `BG0` (deepest: viewport surround) through `BG1` (panels) and `BG2`
//!   (controls) to `BG3` (hover).
//! * `ACCENT` marks selection, the active tool or tab and focus, and nothing else.
//! * `WARN` and `DANGER` are used only for warnings and destructive actions.
//! * `AXIS_X/Y/Z` are used only for the three coordinate axes (gizmos and XYZ fields).
//! * Corners are square or lightly rounded (`RADIUS`); there are no glows, shadows or gradients.

use crate::compat::cr;
use eframe::egui::{self, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use std::sync::Arc;

pub const BG0: Color32 = Color32::from_rgb(0x17, 0x18, 0x1b);
pub const BG1: Color32 = Color32::from_rgb(0x1f, 0x20, 0x24);
pub const BG2: Color32 = Color32::from_rgb(0x28, 0x29, 0x2e);
pub const BG3: Color32 = Color32::from_rgb(0x33, 0x34, 0x3a);
pub const LINE: Color32 = Color32::from_rgb(0x3b, 0x3c, 0x43);
pub const LINE_SOFT: Color32 = Color32::from_rgb(0x2b, 0x2c, 0x31);
pub const TEXT: Color32 = Color32::from_rgb(0xd2, 0xd3, 0xd7);
pub const DIM: Color32 = Color32::from_rgb(0x8a, 0x8b, 0x93);
/// The single interaction colour.
pub const ACCENT: Color32 = Color32::from_rgb(0x6f, 0x93, 0xc9);
pub const WARN: Color32 = Color32::from_rgb(0xc9, 0xa2, 0x4e);
pub const DANGER: Color32 = Color32::from_rgb(0xc7, 0x5b, 0x63);
/// Onion-skin ghosts: previous frames are warm, next frames are green (functional colours like the axis colours).
pub const ONION_PREV: Color32 = Color32::from_rgb(0xd9, 0x7a, 0x5c);
pub const ONION_NEXT: Color32 = Color32::from_rgb(0x5f, 0xb4, 0x82);
pub const AXIS_X: Color32 = Color32::from_rgb(0xc4, 0x62, 0x62);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x7f, 0xa8, 0x68);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x62, 0x8f, 0xc8);

/// Corner radius for controls and windows, in points.
pub const RADIUS: f32 = 3.0;

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

pub fn apply(ctx: &Context) {
    install_fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG1;
    v.window_fill = BG1;
    v.extreme_bg_color = BG0;
    v.faint_bg_color = BG2;
    v.code_bg_color = BG0;
    v.hyperlink_color = ACCENT;
    v.window_stroke = crate::compat::stroke(1.0, LINE);
    v.window_corner_radius = cr(RADIUS + 1.0);
    v.menu_corner_radius = cr(RADIUS);
    v.window_shadow = egui::Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(90) };
    v.popup_shadow = v.window_shadow;
    v.selection.bg_fill = alpha(ACCENT, 90);
    v.selection.stroke = crate::compat::stroke(1.0, ACCENT);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.5 };
    v.striped = false;
    for (w, fill, stroke) in [
        (&mut v.widgets.noninteractive, BG1, LINE_SOFT),
        (&mut v.widgets.inactive, BG2, LINE),
        (&mut v.widgets.hovered, BG3, mix(LINE, ACCENT, 0.5)),
        (&mut v.widgets.active, BG3, ACCENT),
        (&mut v.widgets.open, BG3, LINE),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = cr(RADIUS);
        w.bg_stroke = crate::compat::stroke(1.0, stroke);
        w.fg_stroke = crate::compat::stroke(1.0, TEXT);
        w.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        // UI text must not be selectable or copyable
        s.interaction.selectable_labels = false;
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.slider_width = 120.0;
        s.spacing.interact_size = egui::vec2(26.0, 22.0);
        s.spacing.menu_margin = egui::Margin::same(4);
        s.spacing.window_margin = egui::Margin::same(8);
        s.spacing.tooltip_width = 320.0;
        // thin, always-visible scrollbars that take their own space instead of floating over content
        s.spacing.scroll = egui::style::ScrollStyle::solid();
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.scroll.handle_min_length = 24.0;
        s.animation_time = 0.08;
        s.text_styles.insert(TextStyle::Body, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(13.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(11.0));
        s.text_styles.insert(TextStyle::Heading, FontId::new(15.0, FontFamily::Name("strong".into())));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
    });
}
