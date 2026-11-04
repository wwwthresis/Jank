//! JPEG decoder (ITU-T T.81): baseline and extended sequential Huffman, 1 or 3 components, any chroma subsampling,
//! restart intervals. Progressive and arithmetic-coded JPEGs are rejected with a clear error.
use super::image::{Image, MAX_PIXELS};

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52,
    45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

#[derive(Clone, Default)]
struct Huff {
    maxcode: [i32; 18],
    valptr: [i32; 17],
    mincode: [i32; 17],
    vals: Vec<u8>,
    present: bool,
}
impl Huff {
    fn build(bits: &[u8; 16], vals: &[u8]) -> Result<Huff, String> {
        let total: usize = bits.iter().map(|&b| b as usize).sum();
        if total > vals.len() || total > 256 {
            return Err("JPEG: the Huffman table is damaged".into());
        }
        let mut h = Huff { vals: vals[..total].to_vec(), present: true, ..Default::default() };
        let (mut code, mut k) = (0i32, 0i32);
        for l in 1..=16usize {
            h.valptr[l] = k;
            h.mincode[l] = code;
            code += bits[l - 1] as i32;
            k += bits[l - 1] as i32;
            h.maxcode[l] = if bits[l - 1] > 0 { code - 1 } else { -1 };
            if code > (1 << l) {
                return Err("JPEG: over-subscribed Huffman table".into());
            }
            code <<= 1;
        }
        h.maxcode[17] = i32::MAX;
        Ok(h)
    }
}

struct BitReader<'a> {
    d: &'a [u8],
    pos: usize,
    acc: u32,
    n: u32,
    hit_marker: bool,
}
impl<'a> BitReader<'a> {
    fn new(d: &'a [u8], pos: usize) -> Self {
        Self { d, pos, acc: 0, n: 0, hit_marker: false }
    }
    fn fill(&mut self) {
        while self.n <= 24 {
            let b = if self.hit_marker || self.pos >= self.d.len() {
                0
            } else {
                let b = self.d[self.pos];
                if b == 0xFF {
                    match self.d.get(self.pos + 1) {
                        Some(0) => {
                            self.pos += 2;
                            0xFF
                        }
                        _ => {
                            self.hit_marker = true; // marker reached: the entropy-coded data ended, so pad with zero bits
                            0
                        }
                    }
                } else {
                    self.pos += 1;
                    b
                }
            };
            self.acc |= (b as u32) << (24 - self.n);
            self.n += 8;
        }
    }
    fn bit(&mut self) -> u32 {
        if self.n == 0 {
            self.fill();
        }
        let b = self.acc >> 31;
        self.acc <<= 1;
        self.n -= 1;
        b
    }
    fn bits(&mut self, n: u32) -> i32 {
        let mut v = 0i32;
        for _ in 0..n {
            v = (v << 1) | self.bit() as i32;
        }
        v
    }
    fn decode(&mut self, h: &Huff) -> Result<u8, String> {
        if !h.present {
            return Err("JPEG: a Huffman table is used before it was defined".into());
        }
        let mut code = 0i32;
        for l in 1..=16usize {
            code = (code << 1) | self.bit() as i32;
            if h.maxcode[l] >= 0 && code <= h.maxcode[l] {
                let idx = (h.valptr[l] + code - h.mincode[l]) as usize;
                return h.vals.get(idx).copied().ok_or_else(|| "JPEG: invalid code".to_string());
            }
        }
        Err("JPEG: invalid Huffman code".into())
    }
    /// Aligns to a byte boundary and skips an RSTn marker.
    fn restart(&mut self) {
        self.acc = 0;
        self.n = 0;
        // search forward from the current position for 0xFF 0xD0..0xD7
        while self.pos + 1 < self.d.len() {
            if self.d[self.pos] == 0xFF && (0xD0..=0xD7).contains(&self.d[self.pos + 1]) {
                self.pos += 2;
                self.hit_marker = false;
                return;
            }
            if self.d[self.pos] == 0xFF && self.d[self.pos + 1] != 0 && self.d[self.pos + 1] != 0xFF {
                break; // another marker: the entropy-coded data ended
            }
            self.pos += 1;
        }
        self.hit_marker = true;
    }
}

fn extend(v: i32, t: u32) -> i32 {
    if t == 0 {
        0
    } else if v < (1 << (t - 1)) {
        v - (1 << t) + 1
    } else {
        v
    }
}

struct Comp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    td: usize,
    ta: usize,
    pw: usize, // plane width (a multiple of 8*h)
    ph: usize,
    plane: Vec<u8>,
}

pub fn decode_jpeg(d: &[u8]) -> Result<Image, String> {
    if d.len() < 4 || d[0] != 0xFF || d[1] != 0xD8 {
        return Err("JPEG: missing SOI marker".into());
    }
    let mut qt = [[0u16; 64]; 4];
    let mut dc: [Huff; 4] = Default::default();
    let mut ac: [Huff; 4] = Default::default();
    let mut comps: Vec<Comp> = vec![];
    let (mut w, mut h) = (0usize, 0usize);
    let (mut hmax, mut vmax) = (1usize, 1usize);
    let mut restart_interval = 0usize;
    let mut adobe_transform: Option<u8> = None;
    let mut got_scan = false;
    let mut pos = 2;

    // cosine table for the inverse DCT
    let mut cos = [[0f32; 8]; 8];
    for (x, row) in cos.iter_mut().enumerate() {
        for (u, c) in row.iter_mut().enumerate() {
            *c = (((2 * x + 1) as f32) * u as f32 * std::f32::consts::PI / 16.0).cos() * if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 } * 0.5;
        }
    }

    loop {
        // next marker
        while pos < d.len() && d[pos] != 0xFF {
            pos += 1;
        }
        while pos < d.len() && d[pos] == 0xFF {
            pos += 1;
        }
        let Some(&m) = d.get(pos) else { break };
        pos += 1;
        match m {
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            0xD9 => break,
            _ => {}
        }
        let len = d.get(pos..pos + 2).map(|s| u16::from_be_bytes([s[0], s[1]]) as usize).ok_or("JPEG: the file is cut off")?;
        let seg = d.get(pos + 2..pos + len).ok_or("JPEG: a segment is cut off")?;
        match m {
            0xDB => {
                let mut s = seg;
                while !s.is_empty() {
                    let (pq, tq) = (s[0] >> 4, (s[0] & 15) as usize);
                    if tq > 3 {
                        return Err("JPEG: invalid quantization table index".into());
                    }
                    let n = if pq == 0 { 64 } else { 128 };
                    let body = s.get(1..1 + n).ok_or("JPEG: DQT segment is cut off")?;
                    for i in 0..64 {
                        qt[tq][ZIGZAG[i]] = if pq == 0 { body[i] as u16 } else { u16::from_be_bytes([body[i * 2], body[i * 2 + 1]]) };
                    }
                    s = &s[1 + n..];
                }
            }
            0xC4 => {
                let mut s = seg;
                while s.len() >= 17 {
                    let (class, id) = (s[0] >> 4, (s[0] & 15) as usize);
                    if id > 3 || class > 1 {
                        return Err("JPEG: invalid Huffman table id".into());
                    }
                    let mut bits = [0u8; 16];
                    bits.copy_from_slice(&s[1..17]);
                    let total: usize = bits.iter().map(|&b| b as usize).sum();
                    let vals = s.get(17..17 + total).ok_or("JPEG: DHT segment is cut off")?;
                    let t = Huff::build(&bits, vals)?;
                    if class == 0 { dc[id] = t } else { ac[id] = t }
                    s = &s[17 + total..];
                }
            }
            0xC0 | 0xC1 => {
                if seg.len() < 6 || seg[0] != 8 {
                    return Err("JPEG: only 8 bits per channel are supported".into());
                }
                h = u16::from_be_bytes([seg[1], seg[2]]) as usize;
                w = u16::from_be_bytes([seg[3], seg[4]]) as usize;
                let n = seg[5] as usize;
                if w == 0 || h == 0 || (w as u64 * h as u64) > MAX_PIXELS {
                    return Err("JPEG: invalid image size".into());
                }
                if n != 1 && n != 3 {
                    return Err(format!("JPEG: {n} components are not supported (1 or 3 are needed; CMYK is not supported)"));
                }
                if seg.len() < 6 + n * 3 {
                    return Err("JPEG: SOF segment is cut off".into());
                }
                comps.clear();
                for i in 0..n {
                    let c = &seg[6 + i * 3..9 + i * 3];
                    let (ch, cv) = ((c[1] >> 4) as usize, (c[1] & 15) as usize);
                    if !(1..=4).contains(&ch) || !(1..=4).contains(&cv) || (c[2] as usize) > 3 {
                        return Err("JPEG: invalid component parameters".into());
                    }
                    comps.push(Comp { id: c[0], h: ch, v: cv, tq: c[2] as usize, td: 0, ta: 0, pw: 0, ph: 0, plane: vec![] });
                }
                hmax = comps.iter().map(|c| c.h).max().unwrap();
                vmax = comps.iter().map(|c| c.v).max().unwrap();
                let (mw, mh) = (8 * hmax, 8 * vmax);
                let (mcux, mcuy) = ((w + mw - 1) / mw, (h + mh - 1) / mh);
                for c in &mut comps {
                    c.pw = mcux * c.h * 8;
                    c.ph = mcuy * c.v * 8;
                    c.plane = vec![0u8; c.pw * c.ph];
                }
            }
            0xC2 => return Err("Progressive JPEG is not supported: re-save the picture as a baseline (standard) JPEG".into()),
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return Err("JPEG: this compression type (lossless or arithmetic) is not supported".into()),
            0xDD => {
                restart_interval = seg.get(..2).map(|s| u16::from_be_bytes([s[0], s[1]]) as usize).ok_or("JPEG: DRI segment is cut off")?;
            }
            0xEE => {
                if seg.len() >= 12 && &seg[..5] == b"Adobe" {
                    adobe_transform = Some(seg[11]);
                }
            }
            0xDA => {
                if comps.is_empty() {
                    return Err("JPEG: SOS comes before SOF".into());
                }
                let ns = *seg.first().ok_or("JPEG: SOS segment is cut off")? as usize;
                if ns == 0 || ns > comps.len() || seg.len() < 1 + ns * 2 + 3 {
                    return Err("JPEG: invalid SOS".into());
                }
                let mut scan: Vec<usize> = vec![];
                for i in 0..ns {
                    let (cid, t) = (seg[1 + i * 2], seg[2 + i * 2]);
                    let ci = comps.iter().position(|c| c.id == cid).ok_or("JPEG: SOS refers to an unknown component")?;
                    comps[ci].td = (t >> 4) as usize & 3;
                    comps[ci].ta = (t & 15) as usize & 3;
                    scan.push(ci);
                }
                let mut br = BitReader::new(d, pos + len);
                let mut pred = vec![0i32; comps.len()];
                let decode_block = |br: &mut BitReader, c: &mut Comp, ci: usize, bx: usize, by: usize, pred: &mut Vec<i32>| -> Result<(), String> {
                    let mut coef = [0f32; 64];
                    let q = &qt[c.tq];
                    let t = br.decode(&dc[c.td])? as u32;
                    if t > 11 {
                        return Err("JPEG: invalid DC category".into());
                    }
                    pred[ci] += extend(br.bits(t), t);
                    coef[0] = (pred[ci] * q[0] as i32) as f32;
                    let mut k = 1;
                    while k < 64 {
                        let rs = br.decode(&ac[c.ta])?;
                        let (r, s) = ((rs >> 4) as usize, (rs & 15) as u32);
                        if s == 0 {
                            if r == 15 {
                                k += 16;
                                continue;
                            }
                            break;
                        }
                        k += r;
                        if k > 63 {
                            return Err("JPEG: coefficients run past the end of the block".into());
                        }
                        let v = extend(br.bits(s), s);
                        coef[ZIGZAG[k]] = (v * q[ZIGZAG[k]] as i32) as f32;
                        k += 1;
                    }
                    // inverse DCT: rows first, then columns
                    let mut tmp = [0f32; 64];
                    for u in 0..8 {
                        for y in 0..8 {
                            let mut s = 0.0;
                            for v in 0..8 {
                                s += coef[v * 8 + u] * cos[y][v];
                            }
                            tmp[y * 8 + u] = s;
                        }
                    }
                    for y in 0..8 {
                        for x in 0..8 {
                            let mut s = 0.0;
                            for u in 0..8 {
                                s += tmp[y * 8 + u] * cos[x][u];
                            }
                            let px = (s + 128.0).round().clamp(0.0, 255.0) as u8;
                            let (ox, oy) = (bx * 8 + x, by * 8 + y);
                            if ox < c.pw && oy < c.ph {
                                c.plane[oy * c.pw + ox] = px;
                            }
                        }
                    }
                    Ok(())
                };
                let mut count = 0usize;
                let maybe_restart = |br: &mut BitReader, pred: &mut Vec<i32>, count: &mut usize| {
                    *count += 1;
                    if restart_interval > 0 && *count % restart_interval == 0 {
                        br.restart();
                        pred.iter_mut().for_each(|p| *p = 0);
                    }
                };
                if ns == 1 {
                    let ci = scan[0];
                    let (cw, chh) = ((w * comps[ci].h + hmax - 1) / hmax, (h * comps[ci].v + vmax - 1) / vmax);
                    let (bw, bh) = ((cw + 7) / 8, (chh + 7) / 8);
                    for by in 0..bh {
                        for bx in 0..bw {
                            decode_block(&mut br, &mut comps[ci], ci, bx, by, &mut pred)?;
                            maybe_restart(&mut br, &mut pred, &mut count);
                        }
                    }
                } else {
                    let (mcux, mcuy) = ((w + 8 * hmax - 1) / (8 * hmax), (h + 8 * vmax - 1) / (8 * vmax));
                    for my in 0..mcuy {
                        for mx in 0..mcux {
                            for &ci in &scan {
                                let (ch, cv) = (comps[ci].h, comps[ci].v);
                                for vy in 0..cv {
                                    for hx in 0..ch {
                                        decode_block(&mut br, &mut comps[ci], ci, mx * ch + hx, my * cv + vy, &mut pred)?;
                                    }
                                }
                            }
                            maybe_restart(&mut br, &mut pred, &mut count);
                        }
                    }
                }
                got_scan = true;
                pos = br.pos;
                continue;
            }
            _ => {}
        }
        pos += len;
    }
    if !got_scan || comps.is_empty() {
        return Err("JPEG: no image data".into());
    }

    let mut rgba = vec![255u8; w * h * 4];
    let rgb_coded = adobe_transform == Some(0) || (comps.len() == 3 && comps[0].id == b'R' && comps[1].id == b'G' && comps[2].id == b'B');
    // Bilinear chroma upsampling with centred samples; for a factor of 2 this is exactly
    // the triangle filter with weights 3/4 + 1/4 (and 9/16, 3/16, 3/16, 1/16 in 2D); edges are clamped.
    let (cws, chs): (Vec<usize>, Vec<usize>) = comps.iter().map(|c| ((w * c.h + hmax - 1) / hmax, (h * c.v + vmax - 1) / vmax)).unzip();
    for y in 0..h {
        for x in 0..w {
            let s = |c: &Comp| -> f32 {
                if c.h == hmax && c.v == vmax {
                    return c.plane[y * c.pw + x] as f32;
                }
                let ci = comps.iter().position(|k| std::ptr::eq(k, c)).unwrap_or(0);
                let fx = ((x as f32 + 0.5) * c.h as f32 / hmax as f32 - 0.5).clamp(0.0, (cws[ci] - 1) as f32);
                let fy = ((y as f32 + 0.5) * c.v as f32 / vmax as f32 - 0.5).clamp(0.0, (chs[ci] - 1) as f32);
                let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
                let (x1, y1) = ((x0 + 1).min(cws[ci] - 1), (y0 + 1).min(chs[ci] - 1));
                let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                let p = |xx: usize, yy: usize| c.plane[yy * c.pw + xx] as f32;
                (p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx) * (1.0 - ty) + (p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx) * ty
            };
            let o = (y * w + x) * 4;
            if comps.len() == 1 {
                let g = s(&comps[0]) as u8;
                rgba[o..o + 3].copy_from_slice(&[g, g, g]);
            } else {
                let (a, b, c) = (s(&comps[0]), s(&comps[1]), s(&comps[2]));
                let (r, g, bl) = if rgb_coded { (a, b, c) } else { (a + 1.402 * (c - 128.0), a - 0.344_136 * (b - 128.0) - 0.714_136 * (c - 128.0), a + 1.772 * (b - 128.0)) };
                rgba[o] = r.round().clamp(0.0, 255.0) as u8;
                rgba[o + 1] = g.round().clamp(0.0, 255.0) as u8;
                rgba[o + 2] = bl.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(Image { w: w as u32, h: h as u32, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::jpeg::encode_jpeg;

    fn smooth(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend([(x * 255 / w) as u8, (y * 255 / h) as u8, ((x + y) * 255 / (w + h)) as u8, 255]);
            }
        }
        v
    }
    fn psnr(a: &[u8], b: &[u8]) -> f64 {
        let mse: f64 = a.chunks(4).zip(b.chunks(4)).map(|(p, q)| (0..3).map(|i| (p[i] as f64 - q[i] as f64).powi(2)).sum::<f64>() / 3.0).sum::<f64>() / (a.len() / 4) as f64;
        if mse == 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
    }
    #[test]
    fn own_encoder_roundtrip_high_quality() {
        let px = smooth(45, 29); // not a multiple of 8
        let img = decode_jpeg(&encode_jpeg(45, 29, &px, 95)).unwrap();
        assert_eq!((img.w, img.h), (45, 29));
        assert!(psnr(&px, &img.rgba) > 38.0, "{}", psnr(&px, &img.rgba));
    }
    #[test]
    fn garbage_and_truncation_are_errors() {
        assert!(decode_jpeg(b"").is_err());
        assert!(decode_jpeg(&[0xFF, 0xD8, 0xFF, 0xD9]).is_err());
        let good = encode_jpeg(16, 16, &smooth(16, 16), 80);
        for cut in [4, 30, good.len() / 2] {
            let _ = decode_jpeg(&good[..cut]); // must not panic
        }
        for seed in 0..100u32 {
            let mut j = good.clone();
            let i = 2 + (seed as usize * 7919) % (j.len() - 2);
            j[i] ^= (seed as u8).wrapping_mul(37) | 1;
            let _ = decode_jpeg(&j);
        }
    }
}
