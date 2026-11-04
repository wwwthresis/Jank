//! Pixel selections for the texture editor: rectangle, colour-based "wand", whole-island masks,
//! combining and inverting, copy and paste of the selected pixels, and a helper that keeps
//! painting inside the selection.

use crate::texture::{Rgba, Texture};

/// A mask over a `w` x `h` canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub w: u32,
    pub h: u32,
    mask: Vec<bool>,
}

/// How a new selection combines with the existing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    Replace,
    Add,
    Subtract,
}

/// Selected pixels cut out of a texture, ready to be pasted elsewhere.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub w: u32,
    pub h: u32,
    pub px: Vec<Rgba>, // alpha 0 where nothing was selected
}

impl Selection {
    pub fn empty(w: u32, h: u32) -> Selection {
        Selection { w, h, mask: vec![false; (w * h) as usize] }
    }
    pub fn all(w: u32, h: u32) -> Selection {
        Selection { w, h, mask: vec![true; (w * h) as usize] }
    }
    /// Rectangle with corners given in any order; clipped to the canvas.
    pub fn rect(w: u32, h: u32, a: (i32, i32), b: (i32, i32)) -> Selection {
        let mut s = Selection::empty(w, h);
        let (x0, x1) = (a.0.min(b.0).max(0), a.0.max(b.0).min(w as i32 - 1));
        let (y0, y1) = (a.1.min(b.1).max(0), a.1.max(b.1).min(h as i32 - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                s.set(x, y, true);
            }
        }
        s
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h && self.mask[(y as u32 * self.w + x as u32) as usize]
    }
    pub fn set(&mut self, x: i32, y: i32, on: bool) {
        if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h {
            self.mask[(y as u32 * self.w + x as u32) as usize] = on;
        }
    }
    pub fn count(&self) -> usize {
        self.mask.iter().filter(|m| **m).count()
    }
    pub fn is_empty(&self) -> bool {
        !self.mask.iter().any(|m| *m)
    }
    pub fn invert(&mut self) {
        self.mask.iter_mut().for_each(|m| *m = !*m);
    }
    /// Smallest rectangle (x0, y0, x1, y1), inclusive, around the selected pixels.
    pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let mut r: Option<(u32, u32, u32, u32)> = None;
        for y in 0..self.h {
            for x in 0..self.w {
                if self.mask[(y * self.w + x) as usize] {
                    r = Some(match r {
                        None => (x, y, x, y),
                        Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
                    });
                }
            }
        }
        r
    }

    /// Combines `other` (same size) into `self`.
    pub fn combine(&mut self, other: &Selection, how: Combine) {
        if other.w != self.w || other.h != self.h {
            return;
        }
        for (m, o) in self.mask.iter_mut().zip(&other.mask) {
            *m = match how {
                Combine::Replace => *o,
                Combine::Add => *m || *o,
                Combine::Subtract => *m && !*o,
            };
        }
    }

    /// Selects pixels whose colour is within `tolerance` (0..=255 per channel) of the colour at `(x, y)`.
    /// With `contiguous` only pixels connected to the clicked one (4-neighbourhood) are taken.
    pub fn wand(t: &Texture, x: i32, y: i32, tolerance: u8, contiguous: bool) -> Selection {
        let mut s = Selection::empty(t.w, t.h);
        if x < 0 || y < 0 || x as u32 >= t.w || y as u32 >= t.h {
            return s;
        }
        let seed = t.px[(y as u32 * t.w + x as u32) as usize];
        let near = |p: Rgba| (0..4).all(|i| (p[i] as i32 - seed[i] as i32).unsigned_abs() <= tolerance as u32);
        if !contiguous {
            for (i, p) in t.px.iter().enumerate() {
                s.mask[i] = near(*p);
            }
            return s;
        }
        let mut stack = vec![(x, y)];
        while let Some((cx, cy)) = stack.pop() {
            if cx < 0 || cy < 0 || cx as u32 >= t.w || cy as u32 >= t.h || s.contains(cx, cy) {
                continue;
            }
            if !near(t.px[(cy as u32 * t.w + cx as u32) as usize]) {
                continue;
            }
            s.set(cx, cy, true);
            stack.extend([(cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)]);
        }
        s
    }

    /// Fills a triangle given in canvas pixel coordinates (used to turn UV islands into masks).
    pub fn add_triangle(&mut self, p: [(f32, f32); 3]) {
        let min_x = p.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let max_x = p.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil().min(self.w as f32 - 1.0) as i32;
        let min_y = p.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let max_y = p.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil().min(self.h as f32 - 1.0) as i32;
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area.abs() < 1e-9 {
            return;
        }
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                // a pixel is taken when its centre lies inside, or the triangle touches the pixel (so thin slivers survive)
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((p[1].0 - cx) * (p[2].1 - cy) - (p[2].0 - cx) * (p[1].1 - cy)) / area;
                let w1 = ((p[2].0 - cx) * (p[0].1 - cy) - (p[0].0 - cx) * (p[2].1 - cy)) / area;
                let w2 = 1.0 - w0 - w1;
                let eps = -0.5 / (self.w.max(self.h) as f32).max(1.0);
                if w0 >= eps && w1 >= eps && w2 >= eps {
                    self.set(x, y, true);
                }
            }
        }
    }

    /// Cuts the selected pixels out of `t` (the texture itself is not changed).
    pub fn copy_from(&self, t: &Texture) -> Option<Clip> {
        let (x0, y0, x1, y1) = self.bounds()?;
        let (cw, ch) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut px = vec![[0u8; 4]; (cw * ch) as usize];
        for y in 0..ch {
            for x in 0..cw {
                let (sx, sy) = (x0 + x, y0 + y);
                if self.mask[(sy * self.w + sx) as usize] && sx < t.w && sy < t.h {
                    px[(y * cw + x) as usize] = t.px[(sy * t.w + sx) as usize];
                }
            }
        }
        Some(Clip { w: cw, h: ch, px })
    }

    /// Sets every selected pixel to `c`.
    pub fn fill(&self, t: &mut Texture, c: Rgba) {
        for (i, m) in self.mask.iter().enumerate() {
            if *m && i < t.px.len() {
                t.px[i] = c;
            }
        }
    }

    /// Undoes any change outside the selection: pixels of `after` that are not selected are put back
    /// from `before`. Used after a brush stroke so painting stays inside the selection.
    pub fn restrict(&self, before: &Texture, after: &mut Texture) {
        if before.w != after.w || before.h != after.h || before.w != self.w || before.h != self.h {
            return;
        }
        for (i, m) in self.mask.iter().enumerate() {
            if !*m {
                after.px[i] = before.px[i];
            }
        }
    }

    /// Edges between selected and unselected pixels, as `(x0, y0, x1, y1)` segments in pixel-corner
    /// coordinates. The GUI draws them as the animated outline.
    pub fn outline(&self) -> Vec<(u32, u32, u32, u32)> {
        let mut e = vec![];
        let on = |x: i32, y: i32| self.contains(x, y);
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                if !on(x, y) {
                    continue;
                }
                let (ux, uy) = (x as u32, y as u32);
                if !on(x, y - 1) {
                    e.push((ux, uy, ux + 1, uy));
                }
                if !on(x, y + 1) {
                    e.push((ux, uy + 1, ux + 1, uy + 1));
                }
                if !on(x - 1, y) {
                    e.push((ux, uy, ux, uy + 1));
                }
                if !on(x + 1, y) {
                    e.push((ux + 1, uy, ux + 1, uy + 1));
                }
            }
        }
        e
    }
}

impl Clip {
    /// Draws the clip onto `t` with its top-left corner at `(x, y)`; transparent clip pixels leave `t` alone.
    pub fn paste_into(&self, t: &mut Texture, x: i32, y: i32) {
        for cy in 0..self.h as i32 {
            for cx in 0..self.w as i32 {
                let p = self.px[(cy as u32 * self.w + cx as u32) as usize];
                let (tx, ty) = (x + cx, y + cy);
                if p[3] > 0 && tx >= 0 && ty >= 0 && (tx as u32) < t.w && (ty as u32) < t.h {
                    t.px[(ty as u32 * t.w + tx as u32) as usize] = p;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_halves() -> Texture {
        let mut t = Texture::new("t", 8, 4, [255, 0, 0, 255]);
        for y in 0..4 {
            for x in 4..8 {
                t.px[y * 8 + x] = [0, 0, 255, 255];
            }
        }
        t
    }

    #[test]
    fn rect_is_clipped_and_order_independent() {
        let s = Selection::rect(8, 4, (6, 3), (-5, 1));
        assert_eq!(s.bounds(), Some((0, 1, 6, 3)));
        assert_eq!(s.count(), 7 * 3);
        assert!(s.contains(0, 1) && !s.contains(7, 1) && !s.contains(0, 0));
    }

    #[test]
    fn wand_stays_inside_the_clicked_region() {
        let t = two_halves();
        let s = Selection::wand(&t, 1, 1, 0, true);
        assert_eq!(s.count(), 16);
        assert!(s.contains(3, 3) && !s.contains(4, 3));
        assert!(Selection::wand(&t, 99, 0, 0, true).is_empty());
    }

    #[test]
    fn contiguous_off_takes_every_matching_pixel() {
        let mut t = two_halves();
        t.px[0] = [0, 255, 0, 255];
        t.px[7] = [255, 0, 0, 255]; // a red pixel inside the blue half, not connected to the red half
        let c = Selection::wand(&t, 1, 1, 0, true);
        let g = Selection::wand(&t, 1, 1, 0, false);
        assert!(!c.contains(7, 0));
        assert!(g.contains(7, 0));
    }

    #[test]
    fn tolerance_widens_the_wand() {
        let mut t = Texture::new("t", 3, 1, [100, 100, 100, 255]);
        t.px[1] = [110, 100, 100, 255];
        t.px[2] = [200, 100, 100, 255];
        assert_eq!(Selection::wand(&t, 0, 0, 0, true).count(), 1);
        assert_eq!(Selection::wand(&t, 0, 0, 10, true).count(), 2);
        assert_eq!(Selection::wand(&t, 0, 0, 255, true).count(), 3);
    }

    #[test]
    fn combine_and_invert() {
        let a = Selection::rect(8, 4, (0, 0), (3, 3));
        let b = Selection::rect(8, 4, (2, 0), (5, 3));
        let mut s = a.clone();
        s.combine(&b, Combine::Add);
        assert_eq!(s.count(), 24);
        let mut s = a.clone();
        s.combine(&b, Combine::Subtract);
        assert_eq!(s.count(), 8);
        let mut s = a.clone();
        s.combine(&b, Combine::Replace);
        assert_eq!(s, b);
        let mut inv = a.clone();
        inv.invert();
        assert_eq!(inv.count(), 32 - a.count());
        let mut other_size = a.clone();
        other_size.combine(&Selection::all(2, 2), Combine::Add);
        assert_eq!(other_size, a, "a mask of a different size is ignored");
    }

    #[test]
    fn copy_and_paste_move_pixels() {
        let t = two_halves();
        let s = Selection::rect(8, 4, (3, 1), (4, 2));
        let clip = s.copy_from(&t).unwrap();
        assert_eq!((clip.w, clip.h), (2, 2));
        assert_eq!(clip.px[0], [255, 0, 0, 255]);
        assert_eq!(clip.px[1], [0, 0, 255, 255]);
        let mut dst = Texture::new("d", 4, 4, [9, 9, 9, 255]);
        clip.paste_into(&mut dst, 3, 3); // partly outside: must not panic
        assert_eq!(dst.px[3 * 4 + 3], [255, 0, 0, 255]);
        assert_eq!(dst.px[0], [9, 9, 9, 255]);
        assert!(Selection::empty(8, 4).copy_from(&t).is_none());
    }

    #[test]
    fn restrict_keeps_painting_inside_the_selection() {
        let before = two_halves();
        let mut after = before.clone();
        after.px.iter_mut().for_each(|p| *p = [1, 2, 3, 255]); // a stroke that painted everything
        let s = Selection::rect(8, 4, (0, 0), (1, 1));
        s.restrict(&before, &mut after);
        assert_eq!(after.px[0], [1, 2, 3, 255]);
        assert_eq!(after.px[7], before.px[7]);
        let changed = after.px.iter().zip(&before.px).filter(|(a, b)| a != b).count();
        assert_eq!(changed, 4);
    }

    #[test]
    fn fill_only_touches_selected_pixels() {
        let mut t = two_halves();
        let s = Selection::rect(8, 4, (0, 0), (0, 3));
        s.fill(&mut t, [5, 5, 5, 255]);
        assert_eq!(t.px.iter().filter(|p| **p == [5, 5, 5, 255]).count(), 4);
    }

    #[test]
    fn triangle_mask_covers_its_area() {
        let mut s = Selection::empty(10, 10);
        s.add_triangle([(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]);
        assert!(s.contains(1, 1) && s.contains(2, 5));
        assert!(!s.contains(9, 9) && !s.contains(8, 8));
        let n = s.count();
        assert!((45..=65).contains(&n), "a right triangle of legs 10 covers about 50 pixels, got {n}");
    }

    #[test]
    fn outline_of_a_single_pixel_is_four_edges() {
        let mut s = Selection::empty(4, 4);
        s.set(1, 1, true);
        assert_eq!(s.outline().len(), 4);
        let r = Selection::rect(4, 4, (0, 0), (3, 3));
        assert_eq!(r.outline().len(), 16);
    }
}
