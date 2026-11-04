//! PNG encoder in pure std Rust: row filters, LZ77 with fixed Huffman codes (deflate), zlib and CRC-32.

pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(0xFFFF_FFFF, data) ^ 0xFFFF_FFFF
}
pub fn crc32_update(mut c: u32, data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, e) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    });
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c
}
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

struct BitW {
    out: Vec<u8>,
    buf: u64,
    n: u32,
}
impl BitW {
    fn put(&mut self, v: u32, bits: u32) {
        self.buf |= (v as u64) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.buf as u8);
            self.buf >>= 8;
            self.n -= 8;
        }
    }
    /// Huffman codes are written most significant bit first.
    fn huff(&mut self, code: u32, bits: u32) {
        let mut r = 0u32;
        for i in 0..bits {
            if code & (1 << i) != 0 {
                r |= 1 << (bits - 1 - i);
            }
        }
        self.put(r, bits);
    }
    fn lit(&mut self, v: u32) {
        match v {
            0..=143 => self.huff(0x30 + v, 8),
            144..=255 => self.huff(0x190 + (v - 144), 9),
            256..=279 => self.huff(v - 256, 7),
            _ => self.huff(0xC0 + (v - 280), 8),
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.buf as u8);
        }
        self.out
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// A raw deflate stream: one final fixed-Huffman block with LZ77 matching (hash chains).
pub fn deflate(data: &[u8]) -> Vec<u8> {
    const WIN: usize = 32768;
    const HBITS: usize = 15;
    let mut w = BitW { out: Vec::with_capacity(data.len() / 2 + 16), buf: 0, n: 0 };
    w.put(1, 1); // BFINAL
    w.put(1, 2); // BTYPE = 01 (fixed)
    let mut head = vec![usize::MAX; 1 << HBITS];
    let mut prev = vec![usize::MAX; data.len().max(1)];
    let hash = |i: usize| -> usize { (((data[i] as usize) << 10) ^ ((data[i + 1] as usize) << 5) ^ data[i + 2] as usize) & ((1 << HBITS) - 1) };
    let mut i = 0;
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + 3 <= data.len() {
            let h = hash(i);
            let mut cand = head[h];
            let mut chain = 24;
            while cand != usize::MAX && i - cand <= WIN && chain > 0 {
                let max = (data.len() - i).min(258);
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == max {
                        break;
                    }
                }
                cand = prev[cand];
                chain -= 1;
            }
        }
        let step;
        if best_len >= 3 {
            let li = LEN_BASE.iter().rposition(|&b| b as usize <= best_len).unwrap();
            w.lit(257 + li as u32);
            w.put((best_len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
            let di = DIST_BASE.iter().rposition(|&b| b as usize <= best_dist).unwrap();
            w.huff(di as u32, 5);
            w.put((best_dist - DIST_BASE[di] as usize) as u32, DIST_EXTRA[di] as u32);
            step = best_len;
        } else {
            w.lit(data[i] as u32);
            step = 1;
        }
        for k in 0..step {
            let p = i + k;
            if p + 3 <= data.len() {
                let h = hash(p);
                prev[p] = head[h];
                head[h] = p;
            }
        }
        i += step;
    }
    w.lit(256);
    w.finish()
}

pub fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x9C];
    out.extend(deflate(data));
    out.extend(adler32(data).to_be_bytes());
    out
}

fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(ty);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

fn paeth(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
}

/// `rgba` holds w*h*4 bytes. `alpha=false` writes RGB, which is 25% smaller.
pub fn encode_png(w: u32, h: u32, rgba: &[u8], alpha: bool) -> Vec<u8> {
    assert_eq!(rgba.len(), (w as usize) * (h as usize) * 4, "buffer size does not match w*h*4");
    let bpp = if alpha { 4 } else { 3 };
    let row_len = w as usize * bpp;
    let mut rows: Vec<u8> = Vec::with_capacity((row_len + 1) * h as usize);
    let get_row = |y: usize| -> Vec<u8> {
        let src = &rgba[y * w as usize * 4..(y + 1) * w as usize * 4];
        if alpha { src.to_vec() } else { src.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect() }
    };
    let mut prev = vec![0u8; row_len];
    for y in 0..h as usize {
        let cur = get_row(y);
        let mut best: (u64, u8, Vec<u8>) = (u64::MAX, 0, vec![]);
        for ft in 0u8..5 {
            let mut f = Vec::with_capacity(row_len);
            for x in 0..row_len {
                let a = if x >= bpp { cur[x - bpp] as i32 } else { 0 };
                let b = prev[x] as i32;
                let c = if x >= bpp { prev[x - bpp] as i32 } else { 0 };
                let pred = match ft {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => (a + b) / 2,
                    _ => paeth(a, b, c),
                };
                f.push((cur[x] as i32 - pred) as u8);
            }
            let score: u64 = f.iter().map(|&v| (v as i8 as i32).unsigned_abs() as u64).sum();
            if score < best.0 {
                best = (score, ft, f);
            }
        }
        rows.push(best.1);
        rows.extend(best.2);
        prev = cur;
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, if alpha { 6 } else { 2 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&rows));
    chunk(&mut out, b"IEND", &[]);
    out
}

pub fn encode_png_pixels(w: u32, h: u32, px: &[[u8; 4]], alpha: bool) -> Vec<u8> {
    let flat: Vec<u8> = px.iter().flat_map(|p| *p).collect();
    encode_png(w, h, &flat, alpha)
}

/// A small inflate for streams written by our own `deflate` (stored and fixed-Huffman blocks).
/// Dynamic blocks are not supported here: the native .jank format only writes what this reads.
pub fn inflate(data: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
    struct R<'a> {
        d: &'a [u8],
        pos: usize,
        bit: u32,
    }
    impl R<'_> {
        fn bit(&mut self) -> Result<u32, String> {
            let byte = *self.d.get(self.pos).ok_or("deflate: unexpected end")?;
            let b = (byte >> self.bit) as u32 & 1;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
            Ok(b)
        }
        fn bits(&mut self, n: u32) -> Result<u32, String> {
            let mut v = 0;
            for i in 0..n {
                v |= self.bit()? << i;
            }
            Ok(v)
        }
        fn huff(&mut self, n: u32) -> Result<u32, String> {
            let mut v = 0;
            for _ in 0..n {
                v = (v << 1) | self.bit()?;
            }
            Ok(v)
        }
    }
    let mut r = R { d: data, pos: 0, bit: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = r.bit()?;
        match r.bits(2)? {
            0 => {
                if r.bit != 0 {
                    r.bit = 0;
                    r.pos += 1;
                }
                let h = r.d.get(r.pos..r.pos + 4).ok_or("deflate: stored header is cut off")?;
                let (len, nlen) = (u16::from_le_bytes([h[0], h[1]]), u16::from_le_bytes([h[2], h[3]]));
                if len != !nlen {
                    return Err("deflate: LEN and NLEN do not match".into());
                }
                r.pos += 4;
                let body = r.d.get(r.pos..r.pos + len as usize).ok_or("deflate: stored block is cut off")?;
                if out.len() + body.len() > max_out {
                    return Err("deflate: more data than declared".into());
                }
                out.extend(body);
                r.pos += len as usize;
            }
            1 => loop {
                let mut code = r.huff(7)?;
                let sym = if code <= 0b001_0111 {
                    256 + code
                } else {
                    code = (code << 1) | r.bit()?;
                    if (0x30..=0xBF).contains(&code) {
                        code - 0x30
                    } else if (0xC0..=0xC7).contains(&code) {
                        280 + (code - 0xC0)
                    } else {
                        code = (code << 1) | r.bit()?;
                        if !(0x190..=0x1FF).contains(&code) {
                            return Err("deflate: invalid code".into());
                        }
                        144 + (code - 0x190)
                    }
                };
                if sym < 256 {
                    if out.len() >= max_out {
                        return Err("deflate: more data than declared".into());
                    }
                    out.push(sym as u8);
                } else if sym == 256 {
                    break;
                } else {
                    let li = (sym - 257) as usize;
                    if li >= 29 {
                        return Err("deflate: invalid length code".into());
                    }
                    let len = LEN_BASE[li] as usize + r.bits(LEN_EXTRA[li] as u32)? as usize;
                    let di = r.huff(5)? as usize;
                    if di >= 30 {
                        return Err("deflate: invalid distance code".into());
                    }
                    let dist = DIST_BASE[di] as usize + r.bits(DIST_EXTRA[di] as u32)? as usize;
                    if dist > out.len() || out.len() + len > max_out {
                        return Err("deflate: reference outside the data".into());
                    }
                    for _ in 0..len {
                        let b = out[out.len() - dist];
                        out.push(b);
                    }
                }
            },
            _ => return Err("deflate: dynamic blocks are not supported".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_checksums() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }
    #[test]
    fn deflate_inflate_roundtrip() {
        let mut data = Vec::new();
        for i in 0..20000u32 {
            data.push(((i * i / 7) % 251) as u8);
            if i % 50 < 30 {
                data.push(7);
            }
        }
        let c = deflate(&data);
        assert!(c.len() < data.len());
        assert_eq!(inflate(&c, data.len()).unwrap(), data);
        assert!(inflate(&c[..c.len() / 2], data.len()).is_err(), "a truncated stream must fail");
        assert!(inflate(&c, 100).is_err(), "the output limit protects against inflation");
        assert_eq!(inflate(&deflate(&[]), 0).unwrap(), Vec::<u8>::new());
    }
    #[test]
    fn png_structure_and_compression() {
        let (w, h) = (64u32, 64u32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            px[i * 4..i * 4 + 4].copy_from_slice(&[(i % 64) as u8 * 4, 40, 90, 255]);
        }
        let png = encode_png(w, h, &px, true);
        assert_eq!(&png[1..4], b"PNG");
        assert!(png.len() < px.len() / 4, "a gradient should compress well: {} bytes", png.len());
    }
}
