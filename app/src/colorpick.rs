//! Colour picking: an HSV wheel (hue ring with a saturation/value square inside) and colour conversions.
use crate::theme::*;
use eframe::egui::{self, pos2, vec2, Color32, Id, Pos2, Sense, Shape, Ui};
use std::f32::consts::TAU;

pub fn rgb_to_hsv(c: [u8; 3]) -> [f32; 3] {
    let (r, g, b) = (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d <= 1e-6 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h, if mx <= 1e-6 { 0.0 } else { d / mx }, mx]
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let (i, f) = (h.floor(), h - h.floor());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let f = |x: f32| (x * 255.0).round().clamp(0.0, 255.0) as u8;
    [f(r), f(g), f(b)]
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

#[derive(Clone, Copy, PartialEq)]
enum Zone {
    Ring,
    Square,
}

/// Draws the wheel; returns true while the user changes the colour. `hsv` is edited in place.
pub fn wheel(ui: &mut Ui, hsv: &mut [f32; 3], size: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    let id = Id::new("color_wheel_zone");
    let center = rect.center();
    let outer = size * 0.5 - 2.0;
    let inner = outer - (size * 0.09).max(12.0);
    let half = inner * 0.70; // half side of the square, kept inside the ring
    let sq = egui::Rect::from_center_size(center, vec2(half * 2.0, half * 2.0));

    let mut changed = false;
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.drag_started() || resp.clicked() {
            let d = p - center;
            let r = d.length();
            let zone = if r >= inner - 4.0 { Some(Zone::Ring) } else if sq.expand(6.0).contains(p) { Some(Zone::Square) } else { None };
            ui.memory_mut(|m| match zone {
                Some(z) => m.data.insert_temp(id, z as u8),
                None => m.data.remove::<u8>(id),
            });
        }
        let zone = ui.memory(|m| m.data.get_temp::<u8>(id));
        match zone {
            Some(z) if z == Zone::Ring as u8 => {
                let d = p - center;
                hsv[0] = (d.y.atan2(d.x) / TAU).rem_euclid(1.0);
                changed = true;
            }
            Some(_) => {
                hsv[1] = ((p.x - sq.min.x) / sq.width()).clamp(0.0, 1.0);
                hsv[2] = 1.0 - ((p.y - sq.min.y) / sq.height()).clamp(0.0, 1.0);
                changed = true;
            }
            None => {}
        }
    }

    let painter = ui.painter_at(rect.expand(2.0));
    // hue ring
    let n = 120;
    let mut mesh = egui::epaint::Mesh::default();
    for i in 0..=n {
        let a = i as f32 / n as f32 * TAU;
        let col = c32(hsv_to_rgb(i as f32 / n as f32, 1.0, 1.0));
        let dir = vec2(a.cos(), a.sin());
        mesh.colored_vertex(center + dir * inner, col);
        mesh.colored_vertex(center + dir * outer, col);
    }
    for i in 0..n as u32 {
        let k = i * 2;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    painter.add(Shape::mesh(mesh));
    // saturation / value square
    let hue = c32(hsv_to_rgb(hsv[0], 1.0, 1.0));
    let mut m = egui::epaint::Mesh::default();
    m.colored_vertex(sq.left_top(), Color32::WHITE);
    m.colored_vertex(sq.right_top(), hue);
    m.colored_vertex(sq.right_bottom(), Color32::BLACK);
    m.colored_vertex(sq.left_bottom(), Color32::BLACK);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(m));
    crate::compat::stroke_rect(&painter, sq, 0.0, crate::compat::stroke(1.0, LINE()));
    // markers
    let a = hsv[0] * TAU;
    let mid = (inner + outer) * 0.5;
    let hp: Pos2 = center + vec2(a.cos(), a.sin()) * mid;
    painter.circle_stroke(hp, (outer - inner) * 0.5 - 1.0, crate::compat::stroke(2.0, Color32::WHITE));
    painter.circle_stroke(hp, (outer - inner) * 0.5 + 0.5, crate::compat::stroke(1.0, Color32::BLACK));
    let sp = pos2(sq.min.x + hsv[1] * sq.width(), sq.min.y + (1.0 - hsv[2]) * sq.height());
    painter.circle_stroke(sp, 5.5, crate::compat::stroke(2.0, Color32::WHITE));
    painter.circle_stroke(sp, 6.8, crate::compat::stroke(1.0, Color32::BLACK));
    changed
}
