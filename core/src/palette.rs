//! Colour palettes for pixel painting: a few built-in sets, extraction from a texture, remapping a
//! texture onto a palette, and a tiny text format (`.jankpal`) for saving and sharing palettes.
//!
//! File format (UTF-8 text):
//!
//! ```text
//! JANKPAL 1
//! name = Earth tones
//! #3b2f2f
//! #7a5c3e
//! ```
//!
//! Lines starting with `//` are comments. A file made only of `#rrggbb` lines (no header) is also
//! accepted, so palettes copied from a plain colour list can be imported.

use crate::texture::{Rgba, Texture};
use std::collections::HashMap;

pub const MAX_COLORS: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<Rgba>,
}

fn hex(c: &Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn parse_hex(s: &str) -> Option<Rgba> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

impl Palette {
    pub fn new(name: &str, colors: Vec<Rgba>) -> Palette {
        let mut p = Palette { name: name.to_string(), colors };
        p.colors.truncate(MAX_COLORS);
        p
    }

    /// Adds a colour unless it is already present. Returns its index, or `None` when the palette is full.
    pub fn add(&mut self, c: Rgba) -> Option<usize> {
        let c = [c[0], c[1], c[2], 255];
        if let Some(i) = self.colors.iter().position(|x| *x == c) {
            return Some(i);
        }
        if self.colors.len() >= MAX_COLORS {
            return None;
        }
        self.colors.push(c);
        Some(self.colors.len() - 1)
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.colors.len() {
            self.colors.remove(index);
            true
        } else {
            false
        }
    }

    /// Index of the palette colour closest to `c` (squared RGB distance).
    pub fn nearest(&self, c: Rgba) -> Option<usize> {
        self.colors.iter().enumerate().min_by_key(|(_, p)| dist2(**p, c)).map(|(i, _)| i)
    }

    /// Most frequent opaque colours of a texture, up to `max`.
    pub fn from_texture(t: &Texture, name: &str, max: usize) -> Palette {
        let mut count: HashMap<[u8; 3], usize> = HashMap::new();
        for p in t.px.iter().filter(|p| p[3] > 0) {
            *count.entry([p[0], p[1], p[2]]).or_default() += 1;
        }
        let mut v: Vec<([u8; 3], usize)> = count.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Palette::new(name, v.into_iter().take(max.min(MAX_COLORS)).map(|(c, _)| [c[0], c[1], c[2], 255]).collect())
    }

    /// Replaces every opaque pixel with the nearest palette colour ("palette lock"). Returns how many pixels changed.
    pub fn remap(&self, t: &mut Texture) -> usize {
        if self.colors.is_empty() {
            return 0;
        }
        let mut cache: HashMap<Rgba, Rgba> = HashMap::new();
        let mut changed = 0;
        for p in t.px.iter_mut().filter(|p| p[3] > 0) {
            let key = *p;
            let to = *cache.entry(key).or_insert_with(|| self.colors[self.nearest(key).unwrap_or(0)]);
            let new = [to[0], to[1], to[2], key[3]];
            if new != *p {
                *p = new;
                changed += 1;
            }
        }
        changed
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("JANKPAL 1\nname = {}\n", self.name);
        for c in &self.colors {
            s.push_str(&hex(c));
            s.push('\n');
        }
        s
    }

    /// Reads the text format. Bad lines are reported in the error text; nothing is guessed.
    pub fn from_text(text: &str, fallback_name: &str) -> Result<Palette, String> {
        let mut name = fallback_name.to_string();
        let mut colors = vec![];
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            if line.starts_with("JANKPAL") {
                let v = line.trim_start_matches("JANKPAL").trim();
                if v != "1" {
                    return Err(format!("Palette format version \"{v}\" is not supported"));
                }
                continue;
            }
            if let Some(v) = line.strip_prefix("name") {
                if let Some(v) = v.trim_start().strip_prefix('=') {
                    name = v.trim().to_string();
                    continue;
                }
            }
            match parse_hex(line) {
                Some(c) => colors.push(c),
                None => return Err(format!("Line {}: \"{}\" is not a colour like #aabbcc", n + 1, line)),
            }
        }
        if colors.is_empty() {
            return Err("The palette file has no colours".to_string());
        }
        if colors.len() > MAX_COLORS {
            return Err(format!("A palette can hold at most {MAX_COLORS} colours"));
        }
        Ok(Palette { name, colors })
    }
}

fn dist2(a: Rgba, b: Rgba) -> i32 {
    let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
    d(a[0], b[0]) + d(a[1], b[1]) + d(a[2], b[2])
}

/// The palettes shipped with Jank!. All colour values are original to this project.
pub fn builtin() -> Vec<Palette> {
    let h = |s: &str| parse_hex(s).expect("built-in colour");
    let list = |name: &str, v: &[&str]| Palette::new(name, v.iter().map(|s| h(s)).collect());
    vec![
        list("Plain 16", &["#000000", "#1d1d2b", "#5b3a4a", "#9c4a3c", "#d97a3a", "#f0c35a", "#e9e29c", "#ffffff", "#2f6b3f", "#6fae4a", "#2c4f8f", "#4a8fd9", "#8fd9e6", "#7a4aa6", "#d96aa0", "#8a8a96"]),
        list("Earth", &["#1b1410", "#3b2a1d", "#5e4430", "#85613f", "#a98456", "#cfae78", "#e8d4a2", "#f6ecd2", "#2d3a1e", "#4f6130", "#7d8a4a", "#a8b36a"]),
        list("Neon night", &["#0b0b1a", "#1c1442", "#3a1d7a", "#6b2bd9", "#ff2d95", "#ff7ac6", "#00e5ff", "#7affd9", "#d4ff3a", "#ffd23a", "#ff6a3a", "#f4f4ff"]),
        list("Greys", &["#000000", "#222222", "#444444", "#666666", "#888888", "#aaaaaa", "#cccccc", "#ffffff"]),
        list("Skin and hair", &["#3a2418", "#5c3a26", "#8a5a3a", "#b57a52", "#d9a075", "#f0c29e", "#fbe0c8", "#1a1a1a", "#4a3020", "#a8782a", "#e0c070", "#c8c8c8"]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_and_distinct() {
        let b = builtin();
        assert!(b.len() >= 4);
        for p in &b {
            assert!(!p.colors.is_empty() && p.colors.len() <= MAX_COLORS, "{}", p.name);
            let mut c = p.colors.clone();
            c.sort();
            c.dedup();
            assert_eq!(c.len(), p.colors.len(), "{} repeats a colour", p.name);
        }
    }

    #[test]
    fn text_round_trip() {
        let p = Palette::new("Test", vec![[1, 2, 3, 255], [255, 128, 0, 255]]);
        let back = Palette::from_text(&p.to_text(), "x").unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn plain_colour_lists_import_and_comments_are_skipped() {
        let p = Palette::from_text("// my colours\n#ff0000\n\n#00ff00\n", "Imported").unwrap();
        assert_eq!(p.name, "Imported");
        assert_eq!(p.colors, vec![[255, 0, 0, 255], [0, 255, 0, 255]]);
    }

    #[test]
    fn bad_files_give_clear_errors() {
        assert!(Palette::from_text("", "x").unwrap_err().contains("no colours"));
        assert!(Palette::from_text("#ff0000\nbanana\n", "x").unwrap_err().contains("Line 2"));
        assert!(Palette::from_text("JANKPAL 9\n#ffffff\n", "x").unwrap_err().contains("not supported"));
        assert!(Palette::from_text("#ff00\n", "x").is_err());
    }

    #[test]
    fn add_remove_and_nearest() {
        let mut p = Palette::new("p", vec![]);
        assert_eq!(p.add([10, 10, 10, 7]), Some(0));
        assert_eq!(p.add([10, 10, 10, 255]), Some(0), "duplicates are not added twice");
        assert_eq!(p.add([250, 0, 0, 255]), Some(1));
        assert_eq!(p.nearest([240, 10, 10, 255]), Some(1));
        assert!(p.remove(0));
        assert!(!p.remove(5));
        assert_eq!(Palette::new("e", vec![]).nearest([0, 0, 0, 255]), None);
    }

    #[test]
    fn extraction_orders_by_frequency_and_remap_locks_to_the_palette() {
        let mut t = Texture::new("t", 4, 1, [200, 0, 0, 255]);
        t.px[3] = [0, 0, 200, 255];
        t.px[2] = [0, 0, 0, 0]; // transparent pixels are ignored
        let p = Palette::from_texture(&t, "x", 8);
        assert_eq!(p.colors, vec![[200, 0, 0, 255], [0, 0, 200, 255]]);
        let pal = Palette::new("two", vec![[255, 0, 0, 255], [0, 0, 255, 255]]);
        let changed = pal.remap(&mut t);
        assert_eq!(changed, 3);
        assert_eq!(t.px[0], [255, 0, 0, 255]);
        assert_eq!(t.px[3], [0, 0, 255, 255]);
        assert_eq!(t.px[2], [0, 0, 0, 0]);
    }
}

/// An indexed-colour picture: every pixel stores the index of a palette colour (255 = transparent),
/// like the indexed mode of pixel-art editors. The texture keeps the resulting RGBA picture; the
/// indices make it possible to recolour every pixel of one colour at once by editing the palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Indexed {
    pub palette: Palette,
    pub idx: Vec<u8>,
}

/// Index value of a transparent pixel.
pub const TRANSPARENT: u8 = 255;

impl Indexed {
    /// Converts a picture to the palette (at most 255 colours are used).
    pub fn from_texture(t: &mut Texture, mut palette: Palette) -> Indexed {
        palette.colors.truncate(255);
        if palette.colors.is_empty() {
            palette.colors.push([0, 0, 0, 255]);
        }
        let mut ix = Indexed { palette, idx: vec![TRANSPARENT; t.px.len()] };
        ix.sync_from(t);
        ix
    }
    /// Re-reads the picture after it was painted on: every pixel takes the nearest palette colour
    /// (pixels with alpha below 128 become transparent). Returns how many pixels changed colour.
    pub fn sync_from(&mut self, t: &mut Texture) -> usize {
        if self.idx.len() != t.px.len() {
            self.idx = vec![TRANSPARENT; t.px.len()];
        }
        let mut cache: HashMap<Rgba, u8> = HashMap::new();
        let mut changed = 0;
        for (i, p) in t.px.iter_mut().enumerate() {
            let k = if p[3] < 128 {
                TRANSPARENT
            } else {
                *cache.entry(*p).or_insert_with(|| self.palette.nearest([p[0], p[1], p[2], 255]).unwrap_or(0) as u8)
            };
            self.idx[i] = k;
            let new = if k == TRANSPARENT { [0, 0, 0, 0] } else { self.palette.colors[k as usize] };
            let new = [new[0], new[1], new[2], if k == TRANSPARENT { 0 } else { 255 }];
            if new != *p {
                *p = new;
                changed += 1;
            }
        }
        changed
    }
    /// Writes the palette colours of all indices into the picture.
    pub fn apply(&self, t: &mut Texture) {
        for (p, &k) in t.px.iter_mut().zip(&self.idx) {
            *p = if k == TRANSPARENT { [0, 0, 0, 0] } else { let c = self.palette.colors.get(k as usize).copied().unwrap_or([0, 0, 0, 255]); [c[0], c[1], c[2], 255] };
        }
    }
    /// Changes one palette entry: every pixel that uses it changes with it.
    pub fn set_color(&mut self, i: usize, c: Rgba, t: &mut Texture) -> bool {
        let Some(slot) = self.palette.colors.get_mut(i) else { return false };
        *slot = [c[0], c[1], c[2], 255];
        self.apply(t);
        true
    }
    /// Appends a colour (up to 255). Returns its index.
    pub fn add_color(&mut self, c: Rgba) -> Option<usize> {
        if self.palette.colors.len() >= 255 {
            return None;
        }
        self.palette.colors.push([c[0], c[1], c[2], 255]);
        Some(self.palette.colors.len() - 1)
    }
    /// Pixels of `from` take index `to` (merging two colours). The unused entry stays in the palette.
    pub fn replace_index(&mut self, from: u8, to: u8, t: &mut Texture) -> usize {
        let mut n = 0;
        for k in &mut self.idx {
            if *k == from {
                *k = to;
                n += 1;
            }
        }
        self.apply(t);
        n
    }
    /// How many pixels use each palette entry.
    pub fn usage(&self) -> Vec<usize> {
        let mut u = vec![0; self.palette.colors.len()];
        for &k in &self.idx {
            if let Some(c) = u.get_mut(k as usize) {
                *c += 1;
            }
        }
        u
    }
}

#[cfg(test)]
mod indexed_tests {
    use super::*;
    #[test]
    fn indexed_recolours_by_palette_entry() {
        let mut t = Texture::new("t", 4, 1, [0, 0, 0, 0]);
        t.px = vec![[250, 0, 0, 255], [0, 0, 250, 255], [255, 10, 5, 255], [0, 0, 0, 0]];
        let pal = Palette::new("p", vec![[255, 0, 0, 255], [0, 0, 255, 255]]);
        let mut ix = Indexed::from_texture(&mut t, pal);
        assert_eq!(ix.idx, vec![0, 1, 0, TRANSPARENT]);
        assert_eq!(t.px[2], [255, 0, 0, 255]);
        ix.set_color(0, [0, 255, 0, 255], &mut t);
        assert_eq!(t.px[0], [0, 255, 0, 255]);
        assert_eq!(t.px[2], [0, 255, 0, 255]);
        assert_eq!(t.px[1], [0, 0, 255, 255]);
        assert_eq!(ix.usage(), vec![2, 1]);
        // painting an off-palette colour snaps to the nearest entry
        t.px[1] = [10, 240, 10, 255];
        assert_eq!(ix.sync_from(&mut t), 1);
        assert_eq!(ix.idx[1], 0);
    }
}
