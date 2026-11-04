//! Animated GIF89a writer: median-cut palette, LZW, looping and accurate frame delays.
use std::collections::HashMap;
use std::io::{self, Write};

/// Quantizes an RGBA frame to a palette of at most `max` colours. When the frame already has few colours (typical for low-colour art) nothing is lost.
pub fn quantize(rgba: &[u8], max: usize) -> (Vec<[u8; 3]>, Vec<u8>) {
    let max = max.clamp(2, 256);
    let key = |p: &[u8]| ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32;
    let mut counts: HashMap<u32, u32> = HashMap::new();
    for p in rgba.chunks_exact(4) {
        *counts.entry(key(p)).or_insert(0) += 1;
    }
    let mut uniq: Vec<(u32, u32)> = counts.into_iter().collect();
    uniq.sort_unstable(); // deterministic result
    let rgb = |k: u32| [(k >> 16) as u8, (k >> 8) as u8, k as u8];

    let palette: Vec<[u8; 3]> = if uniq.len() <= max {
        uniq.iter().map(|&(k, _)| rgb(k)).collect()
    } else {
        let mut boxes: Vec<Vec<(u32, u32)>> = vec![uniq.clone()];
        while boxes.len() < max {
            // split the box with the widest channel range
            let mut best: Option<(usize, usize, i32)> = None;
            for (bi, b) in boxes.iter().enumerate() {
                if b.len() < 2 {
                    continue;
                }
                for ch in 0..3 {
                    let sh = 16 - 8 * ch;
                    let lo = b.iter().map(|&(k, _)| (k >> sh) & 0xFF).min().unwrap() as i32;
                    let hi = b.iter().map(|&(k, _)| (k >> sh) & 0xFF).max().unwrap() as i32;
                    if best.map_or(true, |x| hi - lo > x.2) {
                        best = Some((bi, ch, hi - lo));
                    }
                }
            }
            let Some((bi, ch, range)) = best else { break };
            if range == 0 {
                break;
            }
            let mut b = boxes.swap_remove(bi);
            let sh = 16 - 8 * ch as u32;
            b.sort_by_key(|&(k, _)| (k >> sh) & 0xFF);
            let total: u64 = b.iter().map(|&(_, c)| c as u64).sum();
            let (mut acc, mut cut) = (0u64, 1usize);
            for (i, &(_, c)) in b.iter().enumerate() {
                acc += c as u64;
                if acc * 2 >= total {
                    cut = (i + 1).clamp(1, b.len() - 1);
                    break;
                }
            }
            let hi = b.split_off(cut);
            boxes.push(b);
            boxes.push(hi);
        }
        boxes
            .iter()
            .map(|b| {
                let total: u64 = b.iter().map(|&(_, c)| c as u64).sum::<u64>().max(1);
                let mut s = [0u64; 3];
                for &(k, c) in b {
                    let r = rgb(k);
                    for i in 0..3 {
                        s[i] += r[i] as u64 * c as u64;
                    }
                }
                [(s[0] / total) as u8, (s[1] / total) as u8, (s[2] / total) as u8]
            })
            .collect()
    };
    // index of the nearest palette colour for each unique colour (cache)
    let mut map: HashMap<u32, u8> = HashMap::with_capacity(uniq.len());
    for &(k, _) in &uniq {
        let c = rgb(k);
        let idx = palette
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| (0..3).map(|i| (p[i] as i32 - c[i] as i32).pow(2)).sum::<i32>())
            .map(|(i, _)| i)
            .unwrap_or(0);
        map.insert(k, idx as u8);
    }
    let indices = rgba.chunks_exact(4).map(|p| map[&key(p)]).collect();
    (palette, indices)
}

fn lzw(indices: &[u8], min_code: u8) -> Vec<u8> {
    struct W {
        out: Vec<u8>,
        acc: u32,
        n: u32,
    }
    impl W {
        fn put(&mut self, code: u32, bits: u32) {
            self.acc |= code << self.n;
            self.n += bits;
            while self.n >= 8 {
                self.out.push(self.acc as u8);
                self.acc >>= 8;
                self.n -= 8;
            }
        }
    }
    let clear = 1u32 << min_code;
    let eoi = clear + 1;
    let mut w = W { out: vec![], acc: 0, n: 0 };
    let mut size = min_code as u32 + 1;
    let mut next = eoi + 1;
    let mut dict: HashMap<(u32, u8), u32> = HashMap::new();
    w.put(clear, size);
    let mut it = indices.iter();
    let Some(&first) = it.next() else {
        w.put(eoi, size);
        if w.n > 0 {
            w.out.push(w.acc as u8);
        }
        return w.out;
    };
    let mut prefix = first as u32;
    for &px in it {
        if let Some(&c) = dict.get(&(prefix, px)) {
            prefix = c;
            continue;
        }
        w.put(prefix, size);
        if next < 4096 {
            dict.insert((prefix, px), next);
            if next == (1 << size) && size < 12 {
                size += 1;
            }
            next += 1;
        } else {
            w.put(clear, size);
            dict.clear();
            next = eoi + 1;
            size = min_code as u32 + 1;
        }
        prefix = px as u32;
    }
    w.put(prefix, size);
    w.put(eoi, size);
    if w.n > 0 {
        w.out.push(w.acc as u8);
    }
    w.out
}

pub struct GifEncoder<W: Write> {
    out: W,
    w: u16,
    h: u16,
    started: bool,
    loops: u16,
    frame_no: u64,
    fps: f64,
}
impl<W: Write> GifEncoder<W> {
    /// `loops` = 0 means loop forever.
    pub fn new(out: W, w: u32, h: u32, fps: f64, loops: u16) -> Self {
        assert!(w > 0 && h > 0 && w < 65536 && h < 65536);
        Self { out, w: w as u16, h: h as u16, started: false, loops, frame_no: 0, fps: fps.max(0.01) }
    }

    fn header(&mut self) -> io::Result<()> {
        self.out.write_all(b"GIF89a")?;
        self.out.write_all(&self.w.to_le_bytes())?;
        self.out.write_all(&self.h.to_le_bytes())?;
        self.out.write_all(&[0x00, 0, 0])?; // no global palette: every frame carries its own
        // NETSCAPE 2.0 application extension: looping
        self.out.write_all(&[0x21, 0xFF, 0x0B])?;
        self.out.write_all(b"NETSCAPE2.0")?;
        self.out.write_all(&[3, 1])?;
        self.out.write_all(&self.loops.to_le_bytes())?;
        self.out.write_all(&[0])?;
        self.started = true;
        Ok(())
    }

    pub fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        assert_eq!(rgba.len(), self.w as usize * self.h as usize * 4, "frame size does not match");
        if !self.started {
            self.header()?;
        }
        // delay in hundredths of a second with error accumulation, so the average fps stays exact
        let t = |n: u64| (n as f64 * 100.0 / self.fps).round() as u64;
        let delay = (t(self.frame_no + 1) - t(self.frame_no)).clamp(2, 65535) as u16; // many viewers stretch delays below 2 cs
        self.frame_no += 1;

        let (pal, idx) = quantize(rgba, 256);
        let bits = (usize::BITS - (pal.len().max(2) - 1).leading_zeros()).clamp(1, 8) as u8;
        let table = 1usize << bits;

        // Graphic Control Extension: dispose=1 (leave in place) and the delay
        self.out.write_all(&[0x21, 0xF9, 4, 0x04])?;
        self.out.write_all(&delay.to_le_bytes())?;
        self.out.write_all(&[0, 0])?;
        // Image Descriptor with a local palette
        self.out.write_all(&[0x2C, 0, 0, 0, 0])?;
        self.out.write_all(&self.w.to_le_bytes())?;
        self.out.write_all(&self.h.to_le_bytes())?;
        self.out.write_all(&[0x80 | (bits - 1)])?;
        for i in 0..table {
            self.out.write_all(&pal.get(i).copied().unwrap_or([0, 0, 0]))?;
        }
        let min_code = bits.max(2);
        self.out.write_all(&[min_code])?;
        for block in lzw(&idx, min_code).chunks(255) {
            self.out.write_all(&[block.len() as u8])?;
            self.out.write_all(block)?;
        }
        self.out.write_all(&[0])
    }

    pub fn finish(mut self) -> io::Result<W> {
        if !self.started {
            self.header()?;
        }
        self.out.write_all(&[0x3B])?;
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn few_colors_are_lossless_palette() {
        let px: Vec<u8> = [[255u8, 0, 0, 255], [0, 255, 0, 255], [255, 0, 0, 255]].concat();
        let (pal, idx) = quantize(&px, 256);
        assert_eq!(pal.len(), 2);
        assert_eq!(idx[0], idx[2]);
        assert_ne!(idx[0], idx[1]);
    }
    #[test]
    fn many_colors_reduced_to_256() {
        let mut px = Vec::new();
        for i in 0..5000u32 {
            px.extend([(i % 251) as u8, ((i * 7) % 253) as u8, ((i * 13) % 255) as u8, 255]);
        }
        let (pal, idx) = quantize(&px, 256);
        assert!(pal.len() <= 256 && idx.len() == 5000);
    }
    #[test]
    fn gif_has_header_loop_and_trailer() {
        let mut g = GifEncoder::new(Vec::new(), 4, 4, 12.0, 0);
        g.add_frame(&vec![200u8; 4 * 4 * 4]).unwrap();
        g.add_frame(&vec![50u8; 4 * 4 * 4]).unwrap();
        let out = g.finish().unwrap();
        assert_eq!(&out[..6], b"GIF89a");
        assert_eq!(*out.last().unwrap(), 0x3B);
        assert!(out.windows(11).any(|w| w == b"NETSCAPE2.0"));
    }
}
