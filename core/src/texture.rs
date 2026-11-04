//! An RGBA8 texture and its editing operations (brushes, fill, palette, retro colour filters, undo).

pub type Rgba = [u8; 4];

#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub name: String,
    pub w: u32,
    pub h: u32,
    pub px: Vec<Rgba>,
}

impl Texture {
    pub fn new(name: &str, w: u32, h: u32, fill: Rgba) -> Self {
        Self { name: name.into(), w: w.max(1), h: h.max(1), px: vec![fill; (w.max(1) * h.max(1)) as usize] }
    }
    /// A checkerboard placeholder for models without a texture.
    pub fn checker(name: &str, w: u32, h: u32, cell: u32, a: Rgba, b: Rgba) -> Self {
        let mut t = Self::new(name, w, h, a);
        for y in 0..t.h {
            for x in 0..t.w {
                if ((x / cell) + (y / cell)) % 2 == 1 {
                    t.px[(y * t.w + x) as usize] = b;
                }
            }
        }
        t
    }
    #[inline]
    pub fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            None
        } else {
            Some(y as usize * self.w as usize + x as usize)
        }
    }
    pub fn get(&self, x: i32, y: i32) -> Option<Rgba> {
        self.idx(x, y).map(|i| self.px[i])
    }
    pub fn set(&mut self, x: i32, y: i32, c: Rgba) {
        if let Some(i) = self.idx(x, y) {
            self.px[i] = c;
        }
    }
    /// Samples with wrap-around at integer coordinates.
    #[inline]
    pub fn sample_wrap(&self, x: i32, y: i32) -> Rgba {
        let xx = x.rem_euclid(self.w as i32) as usize;
        let yy = y.rem_euclid(self.h as i32) as usize;
        self.px[yy * self.w as usize + xx]
    }
    /// Nearest-neighbour sampling by UV (the PS1 did not filter textures). v = 0 is the top of the image.
    #[inline]
    pub fn sample_uv(&self, u: f32, v: f32) -> Rgba {
        self.sample_wrap((u * self.w as f32).floor() as i32, (v * self.h as f32).floor() as i32)
    }

    pub fn to_rgba_bytes(&self) -> Vec<u8> {
        self.px.iter().flat_map(|p| p.iter().copied()).collect()
    }
    pub fn from_rgba_bytes(name: &str, w: u32, h: u32, bytes: &[u8]) -> Option<Self> {
        if bytes.len() != (w as usize) * (h as usize) * 4 || w == 0 || h == 0 {
            return None;
        }
        Some(Self { name: name.into(), w, h, px: bytes.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect() })
    }

    // ---------- painting ----------

    /// Blends colour `c` into the pixel with strength `k` (0..1), using the source alpha.
    pub fn blend(&mut self, x: i32, y: i32, c: Rgba, k: f32) {
        if let Some(i) = self.idx(x, y) {
            let a = (c[3] as f32 / 255.0) * k.clamp(0.0, 1.0);
            let d = &mut self.px[i];
            for ch in 0..3 {
                d[ch] = (d[ch] as f32 * (1.0 - a) + c[ch] as f32 * a).round() as u8;
            }
            d[3] = (d[3] as f32 + (255.0 - d[3] as f32) * a).round() as u8;
        }
    }

    /// A round brush. `hardness` 1.0 gives a crisp pixel-art edge, lower values a soft edge.
    pub fn brush(&mut self, cx: f32, cy: f32, radius: f32, hardness: f32, c: Rgba) {
        let r = radius.max(0.5);
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = (((x as f32 + 0.5) - cx).powi(2) + ((y as f32 + 0.5) - cy).powi(2)).sqrt();
                if d > r {
                    continue;
                }
                let k = if hardness >= 0.999 {
                    1.0
                } else {
                    let t = (d / r - hardness) / (1.0 - hardness).max(1e-3);
                    1.0 - t.clamp(0.0, 1.0)
                };
                self.blend(x, y, c, k);
            }
        }
    }
    /// A stroke between two points (stamping with a step of at most radius / 3).
    pub fn stroke(&mut self, a: (f32, f32), b: (f32, f32), radius: f32, hardness: f32, c: Rgba) {
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let step = (radius / 3.0).max(0.5);
        let n = (len / step).ceil().max(1.0) as i32;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.brush(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, radius, hardness, c);
        }
    }
    /// Eraser: reduces alpha.
    pub fn erase(&mut self, cx: f32, cy: f32, radius: f32) {
        let r = radius.max(0.5);
        for y in (cy - r).floor() as i32..=(cy + r).ceil() as i32 {
            for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
                if (((x as f32 + 0.5) - cx).powi(2) + ((y as f32 + 0.5) - cy).powi(2)).sqrt() <= r {
                    if let Some(i) = self.idx(x, y) {
                        self.px[i][3] = 0;
                    }
                }
            }
        }
    }
    /// A one-pixel line drawn with the integer line algorithm (for pixel art).
    pub fn line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, c: Rgba) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let mut err = dx + dy;
        loop {
            self.set(x0, y0, c);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }
    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba, filled: bool) {
        let (xa, xb, ya, yb) = (x0.min(x1), x0.max(x1), y0.min(y1), y0.max(y1));
        for y in ya..=yb {
            for x in xa..=xb {
                if filled || x == xa || x == xb || y == ya || y == yb {
                    self.set(x, y, c);
                }
            }
        }
    }
    /// Flood fill (4-connected) with a tolerance measured as RGBA distance.
    pub fn flood_fill(&mut self, x: i32, y: i32, c: Rgba, tol: u32) {
        let Some(target) = self.get(x, y) else { return };
        if target == c {
            return;
        }
        let close = |p: Rgba| -> bool {
            let d: u32 = (0..4).map(|i| (p[i] as i32 - target[i] as i32).unsigned_abs()).sum();
            d <= tol
        };
        let mut seen = vec![false; self.px.len()];
        let mut stack = vec![(x, y)];
        while let Some((px, py)) = stack.pop() {
            let Some(i) = self.idx(px, py) else { continue };
            if seen[i] || !close(self.px[i]) {
                continue;
            }
            seen[i] = true;
            self.px[i] = c;
            stack.extend([(px + 1, py), (px - 1, py), (px, py + 1), (px, py - 1)]);
        }
    }
    /// Replaces a colour everywhere (with a tolerance).
    pub fn replace_color(&mut self, from: Rgba, to: Rgba, tol: u32) -> usize {
        let mut n = 0;
        for p in &mut self.px {
            let d: u32 = (0..4).map(|i| (p[i] as i32 - from[i] as i32).unsigned_abs()).sum();
            if d <= tol {
                *p = to;
                n += 1;
            }
        }
        n
    }

    // ---------- transformations ----------
    pub fn flip_h(&mut self) {
        for y in 0..self.h as usize {
            self.px[y * self.w as usize..(y + 1) * self.w as usize].reverse();
        }
    }
    pub fn flip_v(&mut self) {
        let w = self.w as usize;
        for y in 0..self.h as usize / 2 {
            for x in 0..w {
                self.px.swap(y * w + x, (self.h as usize - 1 - y) * w + x);
            }
        }
    }
    /// Cyclic shift (handy for seamless textures).
    pub fn offset_wrap(&mut self, dx: i32, dy: i32) {
        let src = self.px.clone();
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                self.px[(y as u32 * self.w + x as u32) as usize] = {
                    let sx = (x - dx).rem_euclid(self.w as i32) as usize;
                    let sy = (y - dy).rem_euclid(self.h as i32) as usize;
                    src[sy * self.w as usize + sx]
                };
            }
        }
    }
    /// Nearest-neighbour resize (no smoothing, so pixels stay crisp).
    pub fn resized(&self, nw: u32, nh: u32) -> Texture {
        let (nw, nh) = (nw.max(1), nh.max(1));
        let mut o = Texture::new(&self.name, nw, nh, [0; 4]);
        for y in 0..nh {
            for x in 0..nw {
                let sx = (x as u64 * self.w as u64 / nw as u64) as i32;
                let sy = (y as u64 * self.h as u64 / nh as u64) as i32;
                o.px[(y * nw + x) as usize] = self.px[sy as usize * self.w as usize + sx as usize];
            }
        }
        o
    }

    // ---------- retro colour filters ----------

    /// Quantizes to 15-bit colour as on the PS1 (5 bits per channel).
    pub fn quantize_15bit(&mut self) {
        for p in &mut self.px {
            for ch in 0..3 {
                let v = p[ch] >> 3;
                p[ch] = (v << 3) | (v >> 2);
            }
        }
    }
    /// Ordered 4x4 Bayer dithering followed by quantization to `bits` bits per channel.
    pub fn dither_quantize(&mut self, bits: u32) {
        const B: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
        let bits = bits.clamp(1, 8);
        let levels = (1u32 << bits) - 1;
        let step = 255.0 / levels as f32;
        for y in 0..self.h as usize {
            for x in 0..self.w as usize {
                let t = (B[y & 3][x & 3] as f32 / 16.0 - 0.5) * step;
                let p = &mut self.px[y * self.w as usize + x];
                for ch in 0..3 {
                    let v = (p[ch] as f32 + t).clamp(0.0, 255.0);
                    p[ch] = ((v / step).round() * step).clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    /// Reduces the palette to `n` colours (median cut). Returns the palette.
    pub fn reduce_palette(&mut self, n: usize) -> Vec<Rgba> {
        let n = n.clamp(2, 256);
        let mut buckets: Vec<Vec<Rgba>> = vec![self.px.iter().copied().filter(|p| p[3] > 0).collect()];
        if buckets[0].is_empty() {
            return vec![];
        }
        while buckets.len() < n {
            // split the bucket that is widest along one channel
            let (bi, ch, range) = buckets
                .iter()
                .enumerate()
                .filter(|(_, b)| b.len() > 1)
                .map(|(i, b)| {
                    let (mut best, mut br) = (0, 0);
                    for ch in 0..3 {
                        let lo = b.iter().map(|p| p[ch]).min().unwrap();
                        let hi = b.iter().map(|p| p[ch]).max().unwrap();
                        if (hi - lo) as i32 >= br {
                            br = (hi - lo) as i32;
                            best = ch;
                        }
                    }
                    (i, best, br)
                })
                .max_by_key(|x| x.2)
                .unwrap_or((0, 0, 0));
            if range == 0 {
                break;
            }
            let mut b = buckets.swap_remove(bi);
            b.sort_by_key(|p| p[ch]);
            let hi = b.split_off(b.len() / 2);
            buckets.push(b);
            buckets.push(hi);
        }
        let pal: Vec<Rgba> = buckets
            .iter()
            .map(|b| {
                let mut s = [0u64; 3];
                for p in b {
                    for ch in 0..3 {
                        s[ch] += p[ch] as u64;
                    }
                }
                let l = b.len() as u64;
                [(s[0] / l) as u8, (s[1] / l) as u8, (s[2] / l) as u8, 255]
            })
            .collect();
        for p in &mut self.px {
            if p[3] == 0 {
                continue;
            }
            let best = pal
                .iter()
                .min_by_key(|q| (0..3).map(|i| (q[i] as i32 - p[i] as i32).pow(2)).sum::<i32>())
                .unwrap();
            *p = [best[0], best[1], best[2], p[3]];
        }
        pal
    }
    /// Shifts hue, saturation and brightness. `hue` is in degrees.
    pub fn adjust_hsv(&mut self, hue: f32, sat: f32, val: f32) {
        for p in &mut self.px {
            let (h, s, v) = rgb_to_hsv(p[0], p[1], p[2]);
            let (r, g, b) = hsv_to_rgb((h + hue).rem_euclid(360.0), (s * sat).clamp(0.0, 1.0), (v * val).clamp(0.0, 1.0));
            p[0] = r;
            p[1] = g;
            p[2] = b;
        }
    }
}

pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if mx == 0.0 { 0.0 } else { d / mx }, mx)
}
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (((r + m) * 255.0).round() as u8, ((g + m) * 255.0).round() as u8, ((b + m) * 255.0).round() as u8)
}

/// Undo/redo through snapshots (textures are small, 64 to 256 px, so this is simpler and more robust).
#[derive(Default)]
pub struct History {
    undo: Vec<Texture>,
    redo: Vec<Texture>,
}
impl History {
    const LIMIT: usize = 64;
    /// Call BEFORE a change (at the start of a stroke).
    pub fn push(&mut self, t: &Texture) {
        self.undo.push(t.clone());
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn undo(&mut self, cur: &mut Texture) -> bool {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(cur, prev));
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self, cur: &mut Texture) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(cur, next));
            true
        } else {
            false
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const R: Rgba = [255, 0, 0, 255];
    #[test]
    fn fill_and_history() {
        let mut t = Texture::new("t", 8, 8, [0, 0, 0, 255]);
        let mut h = History::default();
        h.push(&t);
        t.flood_fill(3, 3, R, 0);
        assert!(t.px.iter().all(|p| *p == R));
        assert!(h.undo(&mut t));
        assert_eq!(t.get(0, 0), Some([0, 0, 0, 255]));
        assert!(h.redo(&mut t));
        assert_eq!(t.get(7, 7), Some(R));
    }
    #[test]
    fn fill_respects_borders() {
        let mut t = Texture::new("t", 6, 6, [0, 0, 0, 255]);
        t.line(3, 0, 3, 5, [255; 4]);
        t.flood_fill(0, 0, R, 0);
        assert_eq!(t.get(2, 2), Some(R));
        assert_eq!(t.get(4, 2), Some([0, 0, 0, 255]));
    }
    #[test]
    fn palette_reduces() {
        let mut t = Texture::new("t", 16, 16, [0, 0, 0, 255]);
        for y in 0..16 {
            for x in 0..16 {
                t.set(x, y, [(x * 16) as u8, (y * 16) as u8, 128, 255]);
            }
        }
        let pal = t.reduce_palette(8);
        assert!(pal.len() <= 8);
        let mut uniq: Vec<Rgba> = t.px.clone();
        uniq.sort();
        uniq.dedup();
        assert!(uniq.len() <= 8);
    }
    #[test]
    fn quantize_15() {
        let mut t = Texture::new("t", 1, 1, [255, 7, 128, 255]);
        t.quantize_15bit();
        assert_eq!(t.px[0][0], 255);
        assert_eq!(t.px[0][1], 0);
    }
    #[test]
    fn hsv_roundtrip() {
        let (h, s, v) = rgb_to_hsv(200, 100, 50);
        let (r, g, b) = hsv_to_rgb(h, s, v);
        assert!((r as i32 - 200).abs() <= 1 && (g as i32 - 100).abs() <= 1 && (b as i32 - 50).abs() <= 1);
    }
}
