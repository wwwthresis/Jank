//! Texture painting tools. The same functions serve the 2D texture editor,
//! painting directly on the 3D model, and the hand-drawn animation layer.
use crate::texture::{Rgba, Texture};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Brush (soft or pixel-perfect, see `Brush::pixel`).
    Brush,
    Eraser,
    /// Flood fill of a region of one colour (with a tolerance).
    Fill,
    Line,
    Rect,
    RectFilled,
    /// Eyedropper: paints nothing; the editor picks the colour instead.
    Picker,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub tool: Tool,
    pub color: Rgba,
    /// Radius of the soft brush, or the size of the square nib in pixels when `pixel` is set.
    pub size: f32,
    pub hardness: f32,
    /// Pixel-art mode: a crisp square without smoothing, what low-resolution game textures need.
    pub pixel: bool,
    pub tolerance: u32,
}
impl Default for Brush {
    fn default() -> Self {
        Self { tool: Tool::Brush, color: [230, 90, 70, 255], size: 1.0, hardness: 0.8, pixel: true, tolerance: 0 }
    }
}

fn stamp_pixel(t: &mut Texture, x: i32, y: i32, size: i32, c: Rgba, erase: bool) {
    let s = size.max(1);
    let (x0, y0) = (x - (s - 1) / 2, y - (s - 1) / 2);
    for yy in y0..y0 + s {
        for xx in x0..x0 + s {
            if let Some(i) = t.idx(xx, yy) {
                if erase {
                    t.px[i][3] = 0;
                } else {
                    t.px[i] = c;
                }
            }
        }
    }
}

fn pixel_line(t: &mut Texture, a: (i32, i32), b: (i32, i32), size: i32, c: Rgba, erase: bool) {
    let (mut x, mut y) = a;
    let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
    let (sx, sy) = (if a.0 < b.0 { 1 } else { -1 }, if a.1 < b.1 { 1 } else { -1 });
    let mut err = dx + dy;
    loop {
        stamp_pixel(t, x, y, size, c, erase);
        if (x, y) == b {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Applies the continuous tools (brush, eraser) along the segment `from` to `to`, in texture pixels.
/// See `apply_point` and `apply_shape` for the other tools.
pub fn apply_stroke(t: &mut Texture, b: &Brush, from: Option<(f32, f32)>, to: (f32, f32)) {
    let erase = b.tool == Tool::Eraser;
    if !matches!(b.tool, Tool::Brush | Tool::Eraser) {
        return;
    }
    let a = from.unwrap_or(to);
    if b.pixel {
        let sz = b.size.round().max(1.0) as i32;
        pixel_line(t, (a.0.floor() as i32, a.1.floor() as i32), (to.0.floor() as i32, to.1.floor() as i32), sz, b.color, erase);
    } else if erase {
        let len = ((to.0 - a.0).powi(2) + (to.1 - a.1).powi(2)).sqrt();
        let n = (len / (b.size / 3.0).max(0.5)).ceil().max(1.0) as i32;
        for i in 0..=n {
            let k = i as f32 / n as f32;
            t.erase(a.0 + (to.0 - a.0) * k, a.1 + (to.1 - a.1) * k, b.size);
        }
    } else {
        t.stroke(a, to, b.size, b.hardness, b.color);
    }
}

/// A single press: brush and eraser place a dot, fill floods a region.
pub fn apply_point(t: &mut Texture, b: &Brush, p: (f32, f32)) {
    match b.tool {
        Tool::Brush | Tool::Eraser => apply_stroke(t, b, None, p),
        Tool::Fill => t.flood_fill(p.0.floor() as i32, p.1.floor() as i32, b.color, b.tolerance),
        _ => {}
    }
}

/// A line or rectangle between two points (dragged with the mouse, committed on release).
pub fn apply_shape(t: &mut Texture, b: &Brush, a: (f32, f32), z: (f32, f32)) {
    let (x0, y0, x1, y1) = (a.0.floor() as i32, a.1.floor() as i32, z.0.floor() as i32, z.1.floor() as i32);
    match b.tool {
        Tool::Line => pixel_line(t, (x0, y0), (x1, y1), b.size.round().max(1.0) as i32, b.color, false),
        Tool::Rect => t.rect(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1), b.color, false),
        Tool::RectFilled => t.rect(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1), b.color, true),
        _ => {}
    }
}

/// The colour under a point (for the eyedropper). Transparent pixels give `None`.
pub fn pick_color(t: &Texture, p: (f32, f32)) -> Option<Rgba> {
    t.get(p.0.floor() as i32, p.1.floor() as i32).filter(|c| c[3] > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    const R: Rgba = [255, 0, 0, 255];
    fn tex() -> Texture {
        Texture::new("t", 16, 16, [0, 0, 0, 255])
    }
    #[test]
    fn pixel_brush_is_crisp_and_connected() {
        let mut t = tex();
        let b = Brush { color: R, size: 1.0, pixel: true, ..Default::default() };
        apply_stroke(&mut t, &b, Some((1.5, 1.5)), Some((10.5, 4.5)).unwrap());
        assert_eq!(t.get(1, 1), Some(R));
        assert_eq!(t.get(10, 4), Some(R));
        // only red or black pixels: no smoothing
        assert!(t.px.iter().all(|p| *p == R || *p == [0, 0, 0, 255]));
        // the line is connected: every red pixel has a red neighbour (8-connectivity)
        let red: Vec<(i32, i32)> = (0..16).flat_map(|y| (0..16).map(move |x| (x, y))).filter(|&(x, y)| t.get(x, y) == Some(R)).collect();
        for &(x, y) in &red {
            assert!(red.iter().any(|&(a, b)| (a, b) != (x, y) && (a - x).abs() <= 1 && (b - y).abs() <= 1));
        }
    }
    #[test]
    fn square_nib_size() {
        let mut t = tex();
        apply_point(&mut t, &Brush { color: R, size: 3.0, ..Default::default() }, (8.5, 8.5));
        assert_eq!(t.px.iter().filter(|p| **p == R).count(), 9);
    }
    #[test]
    fn edges_clip_without_panic() {
        let mut t = tex();
        let b = Brush { color: R, size: 5.0, ..Default::default() };
        apply_stroke(&mut t, &b, Some((-50.0, -50.0)), (500.0, 500.0));
        apply_stroke(&mut t, &Brush { pixel: false, size: 4.0, ..b }, Some((-9.0, 3.0)), (30.0, 3.0));
        assert!(t.get(8, 8).is_some());
    }
    #[test]
    fn eraser_clears_alpha_and_fill_respects_tolerance() {
        let mut t = tex();
        apply_point(&mut t, &Brush { tool: Tool::Eraser, size: 2.0, ..Default::default() }, (3.5, 3.5));
        assert_eq!(t.get(3, 3).unwrap()[3], 0);
        apply_point(&mut t, &Brush { tool: Tool::Fill, color: R, ..Default::default() }, (10.0, 10.0));
        assert_eq!(t.get(0, 0), Some(R));
        assert_eq!(t.get(3, 3).unwrap()[3], 0, "the fill must not leak into the erased area");
    }
    #[test]
    fn shapes_and_picker() {
        let mut t = tex();
        apply_shape(&mut t, &Brush { tool: Tool::RectFilled, color: R, ..Default::default() }, (9.0, 9.0), (2.0, 2.0));
        assert_eq!(t.px.iter().filter(|p| **p == R).count(), 8 * 8);
        assert_eq!(pick_color(&t, (3.2, 3.9)), Some(R));
        t.px[0][3] = 0;
        assert_eq!(pick_color(&t, (0.0, 0.0)), None);
        apply_shape(&mut t, &Brush { tool: Tool::Line, color: [0, 255, 0, 255], ..Default::default() }, (0.0, 15.0), (15.0, 15.0));
        assert_eq!(t.get(7, 15), Some([0, 255, 0, 255]));
    }
}
