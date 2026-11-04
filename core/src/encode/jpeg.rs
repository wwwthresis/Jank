//! Baseline JPEG encoder (4:4:4, the standard Huffman tables from Annex K of ITU-T T.81).
//! 4:4:4 is chosen on purpose: pixel art does not tolerate chroma averaging.
//! `encode_jpeg_for_size` searches for a quality that fits a target size, which is how bitrate control works.
use super::simple::rgb_to_ycbcr;

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52,
    45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
const QL: [u8; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113, 92,
    49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const QC: [u8; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99, 47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];
const DC_L_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_C_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_VALS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_L_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_L_VALS: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
const AC_C_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_C_VALS: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

/// Code and length of every symbol from the canonical construction (Annex C).
fn build_codes(bits: &[u8; 16], vals: &[u8]) -> [(u16, u8); 256] {
    let mut t = [(0u16, 0u8); 256];
    let (mut code, mut k) = (0u16, 0usize);
    for len in 1..=16u8 {
        for _ in 0..bits[len as usize - 1] {
            t[vals[k] as usize] = (code, len);
            code += 1;
            k += 1;
        }
        code <<= 1;
    }
    t
}

struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}
impl Bits {
    fn put(&mut self, code: u32, len: u32) {
        if len == 0 {
            return;
        }
        self.acc = (self.acc << len) | (code & ((1 << len) - 1));
        self.n += len;
        while self.n >= 8 {
            let b = (self.acc >> (self.n - 8)) as u8;
            self.out.push(b);
            if b == 0xFF {
                self.out.push(0); // byte stuffing
            }
            self.n -= 8;
        }
    }
    fn flush(&mut self) {
        if self.n > 0 {
            let pad = 8 - self.n;
            self.put((1 << pad) - 1, pad);
        }
    }
}

fn scaled_table(base: &[u8; 64], quality: u8) -> [u8; 64] {
    let q = quality.clamp(1, 100) as i32;
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    let mut t = [0u8; 64];
    for i in 0..64 {
        t[i] = ((base[i] as i32 * scale + 50) / 100).clamp(1, 255) as u8;
    }
    t
}

fn fdct(block: &mut [f32; 64], cos: &[[f32; 8]; 8]) {
    let mut tmp = [0f32; 64];
    for y in 0..8 {
        for u in 0..8 {
            let mut s = 0.0;
            for x in 0..8 {
                s += block[y * 8 + x] * cos[u][x];
            }
            tmp[y * 8 + u] = s * if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 } * 0.5;
        }
    }
    for u in 0..8 {
        for v in 0..8 {
            let mut s = 0.0;
            for y in 0..8 {
                s += tmp[y * 8 + u] * cos[v][y];
            }
            block[v * 8 + u] = s * if v == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 } * 0.5;
        }
    }
}

fn category(v: i32) -> u32 {
    32 - (v.unsigned_abs()).leading_zeros()
}

pub fn encode_jpeg(w: u32, h: u32, rgba: &[u8], quality: u8) -> Vec<u8> {
    assert_eq!(rgba.len(), (w as usize) * (h as usize) * 4);
    assert!(w > 0 && h > 0 && w < 65536 && h < 65536);
    let (ql, qc) = (scaled_table(&QL, quality), scaled_table(&QC, quality));
    let mut o: Vec<u8> = vec![0xFF, 0xD8];
    let dqt = |o: &mut Vec<u8>, id: u8, t: &[u8; 64]| {
        o.extend([0xFF, 0xDB, 0, 67, id]);
        for &z in &ZIGZAG {
            o.push(t[z]);
        }
    };
    dqt(&mut o, 0, &ql);
    dqt(&mut o, 1, &qc);
    o.extend([0xFF, 0xC0, 0, 17, 8]);
    o.extend((h as u16).to_be_bytes());
    o.extend((w as u16).to_be_bytes());
    o.extend([3, 1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1]);
    let dht = |o: &mut Vec<u8>, class_id: u8, bits: &[u8; 16], vals: &[u8]| {
        o.extend([0xFF, 0xC4]);
        o.extend((19 + vals.len() as u16).to_be_bytes());
        o.push(class_id);
        o.extend(bits);
        o.extend(vals);
    };
    dht(&mut o, 0x00, &DC_L_BITS, &DC_VALS);
    dht(&mut o, 0x10, &AC_L_BITS, &AC_L_VALS);
    dht(&mut o, 0x01, &DC_C_BITS, &DC_VALS);
    dht(&mut o, 0x11, &AC_C_BITS, &AC_C_VALS);
    o.extend([0xFF, 0xDA, 0, 12, 3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);

    let dc_t = [build_codes(&DC_L_BITS, &DC_VALS), build_codes(&DC_C_BITS, &DC_VALS)];
    let ac_t = [build_codes(&AC_L_BITS, &AC_L_VALS), build_codes(&AC_C_BITS, &AC_C_VALS)];
    let mut cos = [[0f32; 8]; 8];
    for (u, row) in cos.iter_mut().enumerate() {
        for (x, c) in row.iter_mut().enumerate() {
            *c = (((2 * x + 1) as f32) * u as f32 * std::f32::consts::PI / 16.0).cos();
        }
    }

    // Y, Cb and Cr planes
    let (wu, hu) = (w as usize, h as usize);
    let mut planes = vec![vec![0f32; wu * hu]; 3];
    for i in 0..wu * hu {
        let p = &rgba[i * 4..i * 4 + 4];
        let (y, cb, cr) = rgb_to_ycbcr(p[0], p[1], p[2]);
        planes[0][i] = y;
        planes[1][i] = cb;
        planes[2][i] = cr;
    }
    let mut bw = Bits { out: Vec::with_capacity(wu * hu / 4), acc: 0, n: 0 };
    let mut pred = [0i32; 3];
    for by in (0..hu).step_by(8) {
        for bx in (0..wu).step_by(8) {
            for c in 0..3 {
                let mut blk = [0f32; 64];
                for y in 0..8 {
                    for x in 0..8 {
                        let (sx, sy) = ((bx + x).min(wu - 1), (by + y).min(hu - 1)); // edge replication
                        blk[y * 8 + x] = planes[c][sy * wu + sx] - 128.0;
                    }
                }
                fdct(&mut blk, &cos);
                let (q, t) = if c == 0 { (&ql, 0) } else { (&qc, 1) };
                let mut z = [0i32; 64];
                for k in 0..64 {
                    z[k] = (blk[ZIGZAG[k]] / q[ZIGZAG[k]] as f32).round() as i32;
                }
                // DC
                let diff = z[0] - pred[c];
                pred[c] = z[0];
                let cat = category(diff);
                let (code, len) = dc_t[t][cat as usize];
                bw.put(code as u32, len as u32);
                if cat > 0 {
                    let bitsv = if diff < 0 { diff - 1 } else { diff } as u32;
                    bw.put(bitsv, cat);
                }
                // AC
                let mut run = 0;
                for k in 1..64 {
                    if z[k] == 0 {
                        run += 1;
                        continue;
                    }
                    while run > 15 {
                        let (code, len) = ac_t[t][0xF0];
                        bw.put(code as u32, len as u32);
                        run -= 16;
                    }
                    let cat = category(z[k]);
                    let (code, len) = ac_t[t][(run << 4) as usize | cat as usize];
                    bw.put(code as u32, len as u32);
                    let bitsv = if z[k] < 0 { z[k] - 1 } else { z[k] } as u32;
                    bw.put(bitsv, cat);
                    run = 0;
                }
                if run > 0 {
                    let (code, len) = ac_t[t][0x00];
                    bw.put(code as u32, len as u32);
                }
            }
        }
    }
    bw.flush();
    o.extend(bw.out);
    o.extend([0xFF, 0xD9]);
    o
}

/// Finds a quality in 1..=100 that makes the file as large as possible while staying within `target_bytes`.
/// If even quality 1 does not fit, that result is returned (the honest minimum).
pub fn encode_jpeg_for_size(w: u32, h: u32, rgba: &[u8], target_bytes: usize) -> (Vec<u8>, u8) {
    let (mut lo, mut hi) = (1u8, 100u8);
    let mut best = (encode_jpeg(w, h, rgba, 1), 1u8);
    if best.0.len() > target_bytes {
        return best;
    }
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let j = encode_jpeg(w, h, rgba, mid);
        if j.len() <= target_bytes {
            best = (j, mid);
            lo = mid + 1;
        } else {
            if mid == 1 {
                break;
            }
            hi = mid - 1;
        }
    }
    best
}

/// The same, but starts from a hint (the previous frame's quality): usually 2-3 encodes are enough instead of 7.
pub fn encode_jpeg_for_size_hint(w: u32, h: u32, rgba: &[u8], target_bytes: usize, hint: u8) -> (Vec<u8>, u8) {
    let hint = hint.clamp(1, 100);
    let first = encode_jpeg(w, h, rgba, hint);
    if first.len() <= target_bytes {
        let mut best = (first, hint);
        for _ in 0..4 {
            if best.1 >= 100 {
                return best;
            }
            let n = encode_jpeg(w, h, rgba, best.1 + 1);
            if n.len() <= target_bytes {
                best = (n, best.1 + 1);
            } else {
                return best;
            }
        }
        return encode_jpeg_for_size(w, h, rgba, target_bytes);
    }
    let mut q = hint;
    for _ in 0..4 {
        if q <= 1 {
            return (encode_jpeg(w, h, rgba, 1), 1);
        }
        q -= 1;
        let n = encode_jpeg(w, h, rgba, q);
        if n.len() <= target_bytes {
            return (n, q);
        }
    }
    encode_jpeg_for_size(w, h, rgba, target_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn gradient(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend([(x * 255 / w) as u8, (y * 255 / h) as u8, ((x + y) * 255 / (w + h)) as u8, 255]);
            }
        }
        v
    }
    #[test]
    fn jpeg_markers_and_quality_monotonic() {
        let px = gradient(50, 37); // not a multiple of 8: exercises edge replication
        let lo = encode_jpeg(50, 37, &px, 20);
        let hi = encode_jpeg(50, 37, &px, 95);
        assert_eq!(&lo[..2], &[0xFF, 0xD8]);
        assert_eq!(&lo[lo.len() - 2..], &[0xFF, 0xD9]);
        assert!(hi.len() > lo.len());
    }
    #[test]
    fn rate_control_hits_target() {
        let px = gradient(160, 90);
        let big = encode_jpeg(160, 90, &px, 100).len();
        let target = big / 3;
        let (j, q) = encode_jpeg_for_size(160, 90, &px, target);
        assert!(j.len() <= target && q >= 1 && q < 100, "len={} q={q}", j.len());
    }
}
