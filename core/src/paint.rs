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
    /// Pushes colours along the stroke like a wet finger.
    Smudge,
    /// Softens details under the brush.
    Blur,
    /// Lightens (dodge) the pixels under the brush.
    Dodge,
    /// Darkens (burn) the pixels under the brush.
    Burn,
    /// A linear gradient from the main to the second colour (dragged like a line; dithered in pixel mode).
    Gradient,
}

impl Tool {
    /// Tools that paint continuously while the mouse moves.
    pub fn is_continuous(self) -> bool {
        matches!(self, Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Blur | Tool::Dodge | Tool::Burn)
    }
    /// Tools that are dragged out from a start point and previewed until release.
    pub fn is_shape(self) -> bool {
        matches!(self, Tool::Line | Tool::Rect | Tool::RectFilled | Tool::Gradient)
    }
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
    /// The second (background) colour: the end of gradients; X swaps it with `color`.
    pub bg: Rgba,
    /// Mirror every stroke across the vertical middle of the picture.
    pub symmetry: bool,
    /// Strength of smudge, blur, dodge and burn (0..1).
    pub strength: f32,
}
impl Default for Brush {
    fn default() -> Self {
        Self { tool: Tool::Brush, color: [230, 90, 70, 255], size: 1.0, hardness: 0.8, pixel: true, tolerance: 0, bg: [24, 20, 37, 255], symmetry: false, strength: 0.5 }
    }
}
impl Brush {
    /// Swaps the main and the second colour.
    pub fn swap_colors(&mut self) {
        std::mem::swap(&mut self.color, &mut self.bg);
    }
}

/// Weight of a round soft dab at distance `d` (0..1 of the radius).
fn falloff(d: f32, hardness: f32) -> f32 {
    if d > 1.0 {
        return 0.0;
    }
    let h = hardness.clamp(0.0, 0.99);
    if d <= h {
        1.0
    } else {
        let t = 1.0 - (d - h) / (1.0 - h);
        t * t * (3.0 - 2.0 * t)
    }
}

/// One dab of the filter tools (smudge, blur, dodge, burn) at `p`; `dir` is the stroke movement for smudge.
fn filter_dab(t: &mut Texture, b: &Brush, p: (f32, f32), dir: (f32, f32)) {
    let r = (b.size * 0.5).max(0.5);
    let (x0, x1) = ((p.0 - r).floor() as i32, (p.0 + r).ceil() as i32);
    let (y0, y1) = ((p.1 - r).floor() as i32, (p.1 + r).ceil() as i32);
    let src = t.clone();
    let k0 = b.strength.clamp(0.0, 1.0);
    let get = |x: i32, y: i32| src.get(x.clamp(0, src.w as i32 - 1), y.clamp(0, src.h as i32 - 1)).unwrap_or([0, 0, 0, 0]);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let Some(i) = t.idx(x, y) else { continue };
            let d = (((x as f32 + 0.5) - p.0).powi(2) + ((y as f32 + 0.5) - p.1).powi(2)).sqrt() / r;
            let w = if b.pixel { if d <= 1.0 { 1.0 } else { 0.0 } } else { falloff(d, b.hardness) } * k0;
            if w <= 0.0 {
                continue;
            }
            let o = src.px[i];
            let target: [f32; 4] = match b.tool {
                Tool::Blur => {
                    let mut acc = [0f32; 4];
                    for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (0, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                        let c = get(x + dx, y + dy);
                        for ch in 0..4 {
                            acc[ch] += c[ch] as f32 / 9.0;
                        }
                    }
                    acc
                }
                Tool::Smudge => {
                    let c = get((x as f32 - dir.0).round() as i32, (y as f32 - dir.1).round() as i32);
                    [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32]
                }
                Tool::Dodge => [o[0] as f32 + (255.0 - o[0] as f32) * 0.35, o[1] as f32 + (255.0 - o[1] as f32) * 0.35, o[2] as f32 + (255.0 - o[2] as f32) * 0.35, o[3] as f32],
                Tool::Burn => [o[0] as f32 * 0.7, o[1] as f32 * 0.7, o[2] as f32 * 0.7, o[3] as f32],
                _ => return,
            };
            let d = &mut t.px[i];
            for ch in 0..4 {
                d[ch] = (o[ch] as f32 + (target[ch] - o[ch] as f32) * w).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// A linear gradient from `b.color` at `a` to `b.bg` at `z` over the whole picture.
/// In pixel mode it is dithered with a 4×4 ordered pattern using only the two colours.
fn gradient(t: &mut Texture, b: &Brush, a: (f32, f32), z: (f32, f32)) {
    const BAYER: [f32; 16] = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];
    let (dx, dy) = (z.0 - a.0, z.1 - a.1);
    let l2 = (dx * dx + dy * dy).max(1e-6);
    for y in 0..t.h as i32 {
        for x in 0..t.w as i32 {
            let k = ((((x as f32 + 0.5) - a.0) * dx + ((y as f32 + 0.5) - a.1) * dy) / l2).clamp(0.0, 1.0);
            let i = (y as u32 * t.w + x as u32) as usize;
            let c = if b.pixel {
                let th = (BAYER[((y & 3) * 4 + (x & 3)) as usize] + 0.5) / 16.0;
                if k < th { b.color } else { b.bg }
            } else {
                let m = |p: u8, q: u8| (p as f32 + (q as f32 - p as f32) * k).round() as u8;
                [m(b.color[0], b.bg[0]), m(b.color[1], b.bg[1]), m(b.color[2], b.bg[2]), m(b.color[3], b.bg[3])]
            };
            // the colours are painted over what is there with their own alpha
            let a = c[3] as f32 / 255.0;
            let d = &mut t.px[i];
            for ch in 0..3 {
                d[ch] = (d[ch] as f32 * (1.0 - a) + c[ch] as f32 * a).round() as u8;
            }
            d[3] = (d[3] as f32 + (255.0 - d[3] as f32) * a).round() as u8;
        }
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
    stroke_once(t, b, from, to);
    if b.symmetry {
        let w = t.w as f32;
        stroke_once(t, b, from.map(|p| (w - p.0, p.1)), (w - to.0, to.1));
    }
}

fn stroke_once(t: &mut Texture, b: &Brush, from: Option<(f32, f32)>, to: (f32, f32)) {
    let erase = b.tool == Tool::Eraser;
    if matches!(b.tool, Tool::Smudge | Tool::Blur | Tool::Dodge | Tool::Burn) {
        let a = from.unwrap_or(to);
        let len = ((to.0 - a.0).powi(2) + (to.1 - a.1).powi(2)).sqrt();
        let step = (b.size * 0.25).max(0.75);
        let n = (len / step).ceil().max(1.0) as i32;
        let dir = if len > 1e-4 { ((to.0 - a.0) / len * step.max(1.0), (to.1 - a.1) / len * step.max(1.0)) } else { (0.0, 0.0) };
        for i in 1..=n {
            let k = i as f32 / n as f32;
            filter_dab(t, b, (a.0 + (to.0 - a.0) * k, a.1 + (to.1 - a.1) * k), dir);
        }
        return;
    }
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
        Tool::Brush | Tool::Eraser | Tool::Blur | Tool::Dodge | Tool::Burn => apply_stroke(t, b, None, p),
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
        Tool::Gradient => gradient(t, b, a, z),
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
    fn filters_gradient_and_symmetry() {
        // blur softens a hard edge
        let mut t = tex();
        for y in 0..16 {
            for x in 8..16 {
                t.px[y * 16 + x] = [255, 255, 255, 255];
            }
        }
        let b = Brush { tool: Tool::Blur, size: 6.0, pixel: false, strength: 1.0, ..Default::default() };
        apply_stroke(&mut t, &b, None, (8.0, 8.0));
        let v = t.get(8, 8).unwrap()[0];
        assert!(v > 0 && v < 255, "edge pixel is between black and white: {v}");
        // dodge lightens, burn darkens
        let mut d = Texture::new("t", 8, 8, [100, 100, 100, 255]);
        apply_point(&mut d, &Brush { tool: Tool::Dodge, size: 4.0, strength: 1.0, ..Default::default() }, (4.0, 4.0));
        assert!(d.get(4, 4).unwrap()[0] > 100);
        apply_point(&mut d, &Brush { tool: Tool::Burn, size: 4.0, strength: 1.0, ..Default::default() }, (1.0, 7.0));
        assert!(d.get(0, 7).unwrap()[0] < 100 || d.get(1, 7).unwrap()[0] < 100);
        // smudge drags the white into the black along the stroke
        let mut s = tex();
        for y in 0..16 {
            for x in 0..4 {
                s.px[y * 16 + x] = [255, 255, 255, 255];
            }
        }
        apply_stroke(&mut s, &Brush { tool: Tool::Smudge, size: 4.0, pixel: true, strength: 1.0, ..Default::default() }, Some((2.0, 8.0)), (8.0, 8.0));
        assert!(s.get(5, 8).unwrap()[0] > 0);
        // a dithered gradient uses only the two colours and goes from one to the other
        let mut g = tex();
        let gb = Brush { tool: Tool::Gradient, color: R, bg: [0, 0, 255, 255], pixel: true, ..Default::default() };
        apply_shape(&mut g, &gb, (1.0, 0.0), (15.0, 0.0));
        assert!(g.px.iter().all(|p| *p == R || *p == [0, 0, 255, 255]));
        assert_eq!(g.get(0, 0), Some(R));
        assert_eq!(g.get(15, 3), Some([0, 0, 255, 255]));
        // symmetry paints the mirrored side too
        let mut m = tex();
        apply_point(&mut m, &Brush { color: R, size: 1.0, symmetry: true, ..Default::default() }, (2.5, 5.5));
        assert_eq!(m.get(2, 5), Some(R));
        assert_eq!(m.get(13, 5), Some(R));
        let mut bb = Brush::default();
        let (c, g2) = (bb.color, bb.bg);
        bb.swap_colors();
        assert_eq!((bb.color, bb.bg), (g2, c));
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
