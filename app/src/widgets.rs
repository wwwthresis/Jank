//! Custom widgets: Aseprite-style bevelled buttons and tool cells, Obsidian-style ribbon buttons,
//! tabs and collapsible section headers, toggles, sliders, XYZ drag fields and colour swatches.
//! Everything is painted directly, so it matches the app theme.
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
        Self { label, icon: None, accent: accent(), selected: false, size: None, tip: "", enabled: true }
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
        let down = self.enabled && resp.is_pointer_button_down_on();
        bevel(&painter, rect, self.accent, hov, self.selected, down);
        let ink = if !self.enabled {
            alpha(DIM, 120)
        } else if self.selected {
            mix(TEXT, Color32::WHITE, 0.5)
        } else {
            mix(mix(DIM, TEXT, 0.75), Color32::WHITE, 0.25 * hov)
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

/// Paints an Aseprite-style raised plate: dark 1 px outline, a 1 px highlight along the top and a
/// shade along the bottom. Pressed or selected plates are sunk (the bevel is inverted) and tinted.
pub fn bevel(p: &egui::Painter, rect: egui::Rect, tint: Color32, hov: f32, selected: bool, down: bool) {
    let sunk = selected || down;
    let mut fill = mix(BG2, BG3, hov);
    if selected {
        fill = mix(BG2, tint, 0.32 + 0.08 * hov);
    }
    if down {
        fill = mix(fill, BG0, 0.3);
    }
    let edge = if selected { mix(tint, BG0, 0.15) } else { SHADE };
    p.rect_filled(rect, compat::cr(RADIUS - 1.0), fill);
    let inner = rect.shrink(1.0);
    let (top, bottom) = if sunk { (alpha(SHADE, 150), alpha(Color32::WHITE, 18)) } else { (mix(HILITE, Color32::WHITE, 0.08 * hov), alpha(SHADE, 170)) };
    p.line_segment([pos2(inner.min.x + 1.0, inner.min.y + 0.5), pos2(inner.max.x - 1.0, inner.min.y + 0.5)], compat::stroke(1.0, top));
    p.line_segment([pos2(inner.min.x + 1.0, inner.max.y - 0.5), pos2(inner.max.x - 1.0, inner.max.y - 0.5)], compat::stroke(1.0, bottom));
    compat::stroke_rect(p, rect, RADIUS - 1.0, compat::stroke(1.0, edge));
}

/// A square tool cell for the Aseprite-style tool strip: pixel icon, sunk accent plate when active.
pub fn tool_cell(ui: &mut Ui, icon: Icon, active: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 26.0), Sense::click());
    let hov = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), if reduce_motion() { 0.0 } else { 0.08 });
    let p = ui.painter();
    if active || hov > 0.0 || resp.is_pointer_button_down_on() {
        bevel(p, rect, accent(), hov, active, resp.is_pointer_button_down_on());
    }
    let ink = if active { Color32::WHITE } else { mix(DIM, TEXT, 0.5 + 0.5 * hov) };
    icons::draw(p, icon, rect.center(), 16.0, ink);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    with_tip(resp, tip)
}

/// An Obsidian-style ribbon button: borderless, a soft rounded hover plate and an accent bar on the
/// left edge of the active entry.
pub fn ribbon_button(ui: &mut Ui, icon: Icon, active: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let hov = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), if reduce_motion() { 0.0 } else { 0.08 });
    let p = ui.painter();
    let plate = egui::Rect::from_center_size(rect.center(), vec2(30.0, 30.0));
    if active {
        p.rect_filled(plate, compat::cr(RADIUS + 1.0), alpha(accent(), 46));
        p.rect_filled(egui::Rect::from_min_size(pos2(rect.min.x, plate.min.y + 5.0), vec2(2.0, plate.height() - 10.0)), compat::cr(1.0), accent());
    } else if hov > 0.0 {
        p.rect_filled(plate, compat::cr(RADIUS + 1.0), alpha(BG3, (hov * 255.0) as u8));
    }
    let ink = if active { mix(accent(), Color32::WHITE, 0.35) } else { mix(mix(DIM, TEXT, 0.3), TEXT, hov) };
    icons::draw(p, icon, rect.center(), 16.0, ink);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    with_tip(resp, tip)
}

/// A flat tab with an accent underline when active (used for the mode switcher and dialog tabs).
/// `width` of 0 sizes the tab to its content; when the label does not fit, only the icon is drawn.
pub fn tab(ui: &mut Ui, label: &str, icon: Option<Icon>, active: bool, tip: &str, width: f32) -> Response {
    let natural = 20.0 + label.chars().count() as f32 * 7.2 + if icon.is_some() { 22.0 } else { 0.0 };
    let w = if width > 0.0 { width } else { natural };
    let label = if w + 2.0 < natural && icon.is_some() { "" } else { label };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    let p = ui.painter();
    let plate = rect.shrink2(vec2(0.0, 2.0));
    if active {
        p.rect_filled(plate, compat::cr(RADIUS + 1.0), BG3);
        p.rect_filled(egui::Rect::from_min_size(pos2(plate.min.x + 6.0, rect.max.y - 2.0), vec2(plate.width() - 12.0, 2.0)), compat::cr(1.0), accent());
    } else if resp.hovered() {
        p.rect_filled(plate, compat::cr(RADIUS + 1.0), BG2);
    }
    let ink = if active { TEXT } else if resp.hovered() { mix(DIM, TEXT, 0.6) } else { DIM };
    let text_w = if label.is_empty() { 0.0 } else { label.chars().count() as f32 * 7.2 };
    let content = text_w + if icon.is_some() { if label.is_empty() { 16.0 } else { 22.0 } } else { 0.0 };
    let mut x = rect.center().x - content * 0.5;
    if let Some(i) = icon {
        icons::draw(p, i, pos2(x + 8.0, rect.center().y), 16.0, if active { mix(accent(), Color32::WHITE, 0.3) } else { ink });
        x += 22.0;
    }
    if !label.is_empty() {
        compat::text(p, pos2(x, rect.center().y), Align2::LEFT_CENTER, label, 13.0, ink);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    with_tip(resp, tip)
}

/// Collapsible panel section (Obsidian-style): a small pixel chevron, an uppercase title and a hairline
/// rule. Clicking the header folds the section; the state is remembered per title for the session
/// (a trailing count such as "Faces (3)" does not change the identity). The accent argument is kept
/// for call-site compatibility; headers are neutral by design. Returns `None` while folded.
pub fn section<R>(ui: &mut Ui, title: &str, _accent: Color32, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    ui.add_space(6.0);
    let key = title.split(" (").next().unwrap_or(title).trim();
    let id = ui.id().with(("section", key));
    let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::click());
    if resp.clicked() {
        open = !open;
        ui.ctx().data_mut(|d| d.insert_temp(id, open));
    }
    let p = ui.painter();
    let ink = if resp.hovered() { TEXT } else { DIM };
    // chevron: a 5 px pixel triangle, pointing down when open
    let (cx, cy) = (rect.min.x + 4.0, rect.center().y);
    let tri = if open { vec![pos2(cx - 3.0, cy - 1.5), pos2(cx + 3.0, cy - 1.5), pos2(cx, cy + 2.0)] } else { vec![pos2(cx - 1.5, cy - 3.0), pos2(cx + 2.0, cy), pos2(cx - 1.5, cy + 3.0)] };
    p.add(egui::Shape::convex_polygon(tri, ink, egui::Stroke::NONE));
    let font = egui::FontId::new(10.5, egui::FontFamily::Name("strong".into()));
    let galley = p.layout_no_wrap(title.to_uppercase(), font, ink);
    let w = galley.size().x;
    let gy = rect.center().y - galley.size().y * 0.5;
    p.galley(pos2(rect.min.x + 12.0, gy), galley, ink);
    p.line_segment([pos2(rect.min.x + w + 20.0, rect.center().y), pos2(rect.max.x, rect.center().y)], compat::stroke(1.0, LINE_SOFT));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if !open {
        return None;
    }
    ui.add_space(2.0);
    Some(ui.vertical(body).inner)
}

/// An entry of an Obsidian-style vertical navigation list (the settings window).
pub fn nav_item(ui: &mut Ui, icon: Icon, label: &str, active: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, compat::cr(RADIUS + 1.0), BG3);
    } else if resp.hovered() {
        p.rect_filled(rect, compat::cr(RADIUS + 1.0), BG2);
    }
    let ink = if active { TEXT } else { mix(DIM, TEXT, if resp.hovered() { 0.7 } else { 0.25 }) };
    icons::draw(p, icon, pos2(rect.min.x + 14.0, rect.center().y), 16.0, if active { mix(accent(), Color32::WHITE, 0.3) } else { ink });
    compat::text(p, pos2(rect.min.x + 30.0, rect.center().y), Align2::LEFT_CENTER, label, 13.0, ink);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    with_tip(resp, tip)
}

/// What was clicked in a [`tree_row`].
pub struct RowResp {
    pub row: Response,
    pub eye: bool,
}

/// An Obsidian file-explorer row: icon, left-aligned name, an eye toggle on the right (shown on hover
/// or while hidden). The selected row gets an accent-tinted plate.
/// `eye` is `Some(visible)` for rows that can be hidden, `None` for rows without the toggle.
pub fn tree_row(ui: &mut Ui, icon: Icon, name: &str, selected: bool, eye: Option<bool>, tip: &str) -> RowResp {
    let visible = eye.unwrap_or(true);
    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let p = ui.painter();
    let eye_rect = egui::Rect::from_center_size(pos2(rect.max.x - 14.0, rect.center().y), vec2(22.0, 20.0));
    let over_eye = eye.is_some() && row.hovered() && ui.ctx().pointer_hover_pos().is_some_and(|q| eye_rect.contains(q));
    if selected {
        p.rect_filled(rect, compat::cr(RADIUS + 1.0), alpha(accent(), 56));
    } else if row.hovered() {
        p.rect_filled(rect, compat::cr(RADIUS + 1.0), BG2);
    }
    let ink = if !visible { alpha(DIM, 150) } else if selected { Color32::WHITE } else { TEXT };
    icons::draw(p, icon, pos2(rect.min.x + 14.0, rect.center().y), 16.0, if selected { mix(accent(), Color32::WHITE, 0.45) } else { alpha(DIM, if visible { 255 } else { 120 }) });
    let clip = p.with_clip_rect(egui::Rect::from_min_max(rect.min, pos2(eye_rect.min.x - 2.0, rect.max.y)));
    compat::text(&clip, pos2(rect.min.x + 28.0, rect.center().y), Align2::LEFT_CENTER, name, 13.0, ink);
    if eye.is_some() && (row.hovered() || !visible) {
        if over_eye {
            p.rect_filled(eye_rect, compat::cr(RADIUS), BG3);
        }
        icons::draw(p, if visible { Icon::Eye } else { Icon::EyeOff }, eye_rect.center(), 16.0, if over_eye { TEXT } else { DIM });
    }
    let eye = row.clicked() && over_eye;
    if row.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    RowResp { row: with_tip(row, if over_eye { "Show or hide" } else { tip }), eye }
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
    p.rect_filled(track, compat::cr(th * 0.5), mix(BG0, accent, 0.85 * t));
    compat::stroke_rect(p, track, th * 0.5, compat::stroke(1.0, mix(LINE, accent, t)));
    let knob = egui::Rect::from_min_size(pos2(track.min.x + 2.0 + t * (tw - th), track.min.y + 2.0), vec2(th - 4.0, th - 4.0));
    p.rect_filled(knob, compat::cr(th * 0.5), mix(DIM, Color32::WHITE, t));
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
    p.rect_filled(rect, compat::cr(0.0), color);
    let edge = if selected { Color32::WHITE } else if resp.hovered() { TEXT } else { SHADE };
    compat::stroke_rect(p, rect, 0.0, compat::stroke(1.0, edge));
    if selected {
        compat::stroke_rect(p, rect.expand(1.0), 0.0, compat::stroke(1.0, SHADE));
        compat::stroke_rect(p, rect.shrink(1.0), 0.0, compat::stroke(1.0, SHADE));
    }
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
    let bang = p.layout_no_wrap("!".to_string(), font, accent());
    let (w1, w2, h) = (name.size().x, bang.size().x, name.size().y);
    let (rect, _) = ui.allocate_exact_size(vec2(w1 + w2 + 2.0, h.max(size)), Sense::hover());
    let y = rect.center().y - h * 0.5;
    p.galley(pos2(rect.min.x, y), name, TEXT);
    p.galley(pos2(rect.min.x + w1 + 1.0, y), bang, accent());
}
