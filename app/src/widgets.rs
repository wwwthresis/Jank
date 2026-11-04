//! Custom widgets: "plate" buttons that tilt and lift slightly on hover, toggles, section headers,
//! sliders, XYZ drag fields and color swatches. Everything is painted directly, so it matches the app theme.
//! Every interactive helper accepts a tooltip (`Btn::tip`, or the `*_tip` function variants).
use crate::compat;
use crate::icons::{self, Icon};
use crate::theme::*;
use eframe::egui::{self, pos2, vec2, Align2, Color32, DragValue, Response, Sense, Slider, Ui, Vec2};
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, Ordering};

static REDUCE_MOTION: AtomicBool = AtomicBool::new(false);
static TOOLTIPS: AtomicBool = AtomicBool::new(true);

/// Applies the accessibility preferences from the settings. Called every frame; cheap.
pub fn set_prefs(reduce_motion: bool, tooltips: bool) {
    REDUCE_MOTION.store(reduce_motion, Ordering::Relaxed);
    TOOLTIPS.store(tooltips, Ordering::Relaxed);
}
fn reduce_motion() -> bool {
    REDUCE_MOTION.load(Ordering::Relaxed)
}
fn tooltips_on() -> bool {
    TOOLTIPS.load(Ordering::Relaxed)
}

pub struct Btn<'a> {
    label: &'a str,
    icon: Option<Icon>,
    accent: Color32,
    selected: bool,
    size: Option<Vec2>,
    tip: &'a str,
    enabled: bool,
}
impl<'a> Btn<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, icon: None, accent: ACCENT, selected: false, size: None, tip: "", enabled: true }
    }
    pub fn icon(mut self, i: Icon) -> Self {
        self.icon = Some(i);
        self
    }
    pub fn accent(mut self, c: Color32) -> Self {
        self.accent = c;
        self
    }
    pub fn on(mut self, v: bool) -> Self {
        self.selected = v;
        self
    }
    pub fn size(mut self, s: Vec2) -> Self {
        self.size = Some(s);
        self
    }
    pub fn tip(mut self, t: &'a str) -> Self {
        self.tip = t;
        self
    }
    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }
    pub fn show(self, ui: &mut Ui) -> Response {
        let chars = self.label.chars().count() as f32;
        let default = if self.label.is_empty() { vec2(28.0, 28.0) } else { vec2(18.0 + chars * 7.0 + if self.icon.is_some() { 22.0 } else { 0.0 }, 28.0) };
        let mut size = self.size.unwrap_or(default);
        // compact controls: callers pass nominal sizes, the height is trimmed to the dense layout
        size.y = (size.y * 0.8).round().max(20.0);
        if self.label.is_empty() && self.size.is_some() {
            size.x = (size.x * 0.85).round().max(22.0);
        }
        let (rect, resp) = ui.allocate_exact_size(size, if self.enabled { Sense::click() } else { Sense::hover() });
        let ctx = ui.ctx().clone();
        let painter = ui.painter().clone();
        let hov = if self.enabled { ctx.animate_bool_with_time(resp.id, resp.hovered(), if reduce_motion() { 0.0 } else { 0.08 }) } else { 0.0 };
        let sel = if self.selected { 1.0 } else { 0.0 };
        let down = self.enabled && resp.is_pointer_button_down_on();
        let tint = self.accent;
        let mut fill = mix(BG2, BG3, hov);
        fill = mix(fill, tint, 0.20 * sel);
        if down {
            fill = mix(fill, BG0, 0.35);
        }
        let edge = if self.selected { tint } else { mix(LINE, tint, 0.5 * hov) };
        painter.rect_filled(rect, compat::cr(RADIUS), fill);
        compat::stroke_rect(&painter, rect, RADIUS, compat::stroke(1.0, edge));
        let ink = if !self.enabled {
            alpha(DIM, 120)
        } else if self.selected {
            mix(TEXT, tint, 0.55)
        } else {
            mix(TEXT, Color32::WHITE, 0.15 * hov)
        };
        let c = rect.center();
        match (self.icon, self.label.is_empty()) {
            (Some(i), true) => icons::draw(&painter, i, c, 16.0, ink),
            (Some(i), false) => {
                icons::draw(&painter, i, pos2(rect.min.x + 14.0, c.y), 15.0, ink);
                compat::text(&painter, pos2(c.x + 8.0, c.y), Align2::CENTER_CENTER, self.label, 13.0, ink);
            }
            (None, _) => {
                compat::text(&painter, c, Align2::CENTER_CENTER, self.label, 13.0, ink);
            }
        }
        if self.enabled && resp.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        with_tip(resp, self.tip)
    }
}

/// A flat tab with an accent underline when active (used for the mode switcher and dialog tabs).
pub fn tab(ui: &mut Ui, label: &str, icon: Option<Icon>, active: bool, tip: &str) -> Response {
    let w = 22.0 + label.chars().count() as f32 * 7.4 + if icon.is_some() { 20.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, compat::cr(0.0), BG2);
        p.rect_filled(egui::Rect::from_min_size(pos2(rect.min.x, rect.max.y - 2.0), vec2(rect.width(), 2.0)), compat::cr(0.0), ACCENT);
    } else if resp.hovered() {
        p.rect_filled(rect, compat::cr(0.0), BG2);
    }
    let ink = if active { TEXT } else if resp.hovered() { mix(DIM, TEXT, 0.6) } else { DIM };
    let mut x = rect.min.x + 11.0;
    if let Some(i) = icon {
        icons::draw(p, i, pos2(x + 7.0, rect.center().y), 15.0, if active { ACCENT } else { ink });
        x += 20.0;
    }
    compat::text(p, pos2(x, rect.center().y), Align2::LEFT_CENTER, label, 13.0, ink);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    with_tip(resp, tip)
}

/// Panel section header: small uppercase title and a hairline rule. The accent argument is kept for
/// call-site compatibility; headers are neutral by design.
pub fn section<R>(ui: &mut Ui, title: &str, _accent: Color32, body: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
    let p = ui.painter();
    let font = egui::FontId::new(10.5, egui::FontFamily::Name("strong".into()));
    let galley = p.layout_no_wrap(title.to_uppercase(), font, DIM);
    let w = galley.size().x;
    p.galley(rect.min + vec2(0.0, 1.0), galley, DIM);
    p.line_segment([pos2(rect.min.x + w + 8.0, rect.center().y + 1.0), pos2(rect.max.x, rect.center().y + 1.0)], compat::stroke(1.0, LINE_SOFT));
    ui.add_space(2.0);
    ui.vertical(body).inner
}

/// Small dimmed hint or caption text.
pub fn dim(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.5).color(DIM));
}

/// Toggle switch with a smoothly animated knob (no tooltip; see [`toggle_tip`]).
#[allow(dead_code)]
pub fn toggle(ui: &mut Ui, value: &mut bool, label: &str, accent: Color32) -> Response {
    toggle_tip(ui, value, label, accent, "")
}

/// Toggle switch with a tooltip (an empty `tip` shows none).
pub fn toggle_tip(ui: &mut Ui, value: &mut bool, label: &str, accent: Color32, tip: &str) -> Response {
    let (tw, th) = (28.0, 14.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(tw + 10.0 + label.chars().count() as f32 * 7.2), 20.0), Sense::click());
    if resp.clicked() {
        *value = !*value;
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *value, if reduce_motion() { 0.0 } else { 0.08 });
    let track = egui::Rect::from_min_size(rect.min + vec2(0.0, 3.0), vec2(tw, th));
    let p = ui.painter();
    p.rect_filled(track, compat::cr(RADIUS), mix(BG0, accent, 0.55 * t));
    compat::stroke_rect(p, track, RADIUS, compat::stroke(1.0, mix(LINE, accent, t)));
    let knob = egui::Rect::from_min_size(pos2(track.min.x + 2.0 + t * (tw - th), track.min.y + 2.0), vec2(th - 4.0, th - 4.0));
    p.rect_filled(knob, compat::cr(2.0), mix(DIM, Color32::WHITE, t));
    compat::text(p, rect.min + vec2(tw + 8.0, 11.0), Align2::LEFT_CENTER, label, 13.0, TEXT);
    with_tip(resp, tip)
}

/// Attaches a hover tooltip to a response unless the text is empty.
pub fn with_tip(resp: Response, tip: &str) -> Response {
    if tip.is_empty() || !tooltips_on() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// Float slider with a caption (no tooltip; see [`slider_f_tip`]). Returns true if the value changed.
#[allow(dead_code)]
pub fn slider_f(ui: &mut Ui, label: &str, v: &mut f32, range: RangeInclusive<f32>) -> bool {
    slider_f_tip(ui, label, v, range, "")
}
/// Float slider with a caption and a tooltip. Returns true if the value changed.
pub fn slider_f_tip(ui: &mut Ui, label: &str, v: &mut f32, range: RangeInclusive<f32>, tip: &str) -> bool {
    ui.horizontal(|ui| with_tip(ui.add(Slider::new(v, range).text(label)), tip).changed()).inner
}
/// Integer slider with a caption (no tooltip; see [`slider_u_tip`]). Returns true if the value changed.
#[allow(dead_code)]
pub fn slider_u(ui: &mut Ui, label: &str, v: &mut u32, range: RangeInclusive<u32>) -> bool {
    slider_u_tip(ui, label, v, range, "")
}
/// Integer slider with a caption and a tooltip. Returns true if the value changed.
pub fn slider_u_tip(ui: &mut Ui, label: &str, v: &mut u32, range: RangeInclusive<u32>, tip: &str) -> bool {
    ui.horizontal(|ui| with_tip(ui.add(Slider::new(v, range).text(label)), tip).changed()).inner
}

/// Three X/Y/Z drag fields (no tooltip; see [`drag3_tip`]).
#[allow(dead_code)]
pub fn drag3(ui: &mut Ui, label: &str, v: &mut [f32; 3], speed: f32) -> bool {
    drag3_tip(ui, label, v, speed, "")
}
/// Three X/Y/Z drag fields with a tooltip on each field. Returns true if any value changed.
pub fn drag3_tip(ui: &mut Ui, label: &str, v: &mut [f32; 3], speed: f32, tip: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(12.5).color(DIM));
        for (i, c) in [AXIS_X, AXIS_Y, AXIS_Z].into_iter().enumerate() {
            let name = ["X", "Y", "Z"][i];
            ui.label(egui::RichText::new(name).size(12.5).color(c));
            let axis_tip = if tip.is_empty() { String::new() } else { format!("{tip} ({name})") };
            changed |= with_tip(ui.add(DragValue::new(&mut v[i]).speed(speed)), &axis_tip).changed();
        }
    });
    changed
}

/// Square color swatch; hover tooltip is added by the caller via `Response::on_hover_text`.
pub fn swatch(ui: &mut Ui, color: Color32, selected: bool, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, compat::cr(1.0), color);
    let edge = if selected { ACCENT } else if resp.hovered() { TEXT } else { LINE };
    compat::stroke_rect(p, rect.expand(if selected { 1.0 } else { 0.0 }), 1.0, compat::stroke(if selected { 2.0 } else { 1.0 }, edge));
    resp
}

/// A small icon drawn in the flow of a horizontal layout.
pub fn icon_inline(ui: &mut Ui, icon: Icon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
    icons::draw(ui.painter(), icon, rect.center(), 18.0, color);
}

/// The "Jank!" wordmark: a semi-bold name with the exclamation mark in the accent colour.
pub fn logo(ui: &mut Ui, size: f32) {
    let font = egui::FontId::new(size * 0.8, egui::FontFamily::Name("strong".into()));
    let p = ui.painter().clone();
    let name = p.layout_no_wrap("Jank".to_string(), font.clone(), TEXT);
    let bang = p.layout_no_wrap("!".to_string(), font, ACCENT);
    let (w1, w2, h) = (name.size().x, bang.size().x, name.size().y);
    let (rect, _) = ui.allocate_exact_size(vec2(w1 + w2 + 2.0, h.max(size)), Sense::hover());
    let y = rect.center().y - h * 0.5;
    p.galley(pos2(rect.min.x, y), name, TEXT);
    p.galley(pos2(rect.min.x + w1 + 1.0, y), bang, ACCENT);
}
