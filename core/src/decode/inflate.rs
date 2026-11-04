//! A complete inflate decoder per RFC 1951 (stored, fixed and dynamic Huffman blocks) with a hard output limit.
//! Needed to read PNG and glTF files from other programs, whose streams almost always use dynamic Huffman blocks.

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    acc: u32,
    n: u32,
}
impl<'a> Bits<'a> {
    fn need(&mut self, n: u32) -> Result<(), String> {
        while self.n < n {
            let b = *self.d.get(self.pos).ok_or("deflate: unexpected end of data")?;
            self.pos += 1;
            self.acc |= (b as u32) << self.n;
            self.n += 8;
        }
        Ok(())
    }
    fn bits(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.acc & ((1u32 << n) - 1);
        self.acc >>= n;
        self.n -= n;
        Ok(v)
    }
    fn align_byte(&mut self) {
        let drop = self.n % 8;
        self.acc >>= drop;
        self.n -= drop;
    }
}

/// A canonical Huffman code stored as two tables: the number of codes per length and the symbols in code order (the counting method described in RFC 1951).
struct Huff {
    count: [u16; 16],
    symbol: Vec<u16>,
}
impl Huff {
    fn new(lengths: &[u8]) -> Result<Huff, String> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        // check that the code is not over-subscribed
        let mut left: i32 = 1;
        for len in 1..16 {
            left <<= 1;
            left -= count[len] as i32;
            if left < 0 {
                return Err("deflate: over-subscribed Huffman code".into());
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        count[0] = 0;
        Ok(Huff { count, symbol })
    }
    fn decode(&self, b: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.bits(1)? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return self.symbol.get((index + (code - first)) as usize).copied().ok_or_else(|| "deflate: invalid code".to_string());
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("deflate: invalid Huffman code".into())
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// Decompresses a raw deflate stream. `max_out` is the upper bound of the result size
/// (protection against decompression bombs): exceeding it is an error instead of gigabytes of memory.
pub fn inflate(data: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
    let mut b = Bits { d: data, pos: 0, acc: 0, n: 0 };
    let mut out: Vec<u8> = Vec::with_capacity(max_out.min(1 << 20));
    let (fixed_lit, fixed_dist) = {
        let mut l = [0u8; 288];
        for (i, v) in l.iter_mut().enumerate() {
            *v = match i {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        (Huff::new(&l)?, Huff::new(&[5u8; 30])?)
    };
    loop {
        let last = b.bits(1)?;
        match b.bits(2)? {
            0 => {
                b.align_byte();
                let len = b.bits(16)? as usize;
                let nlen = b.bits(16)? as usize;
                if len != (!nlen & 0xFFFF) {
                    return Err("deflate: LEN and NLEN do not match".into());
                }
                if out.len() + len > max_out {
                    return Err("deflate: more data than declared".into());
                }
                // after align_byte the bit buffer may still hold whole bytes: use those first
                for _ in 0..len {
                    out.push(b.bits(8)? as u8);
                }
            }
            t @ (1 | 2) => {
                let dynamic;
                let (lit, dist): (&Huff, &Huff) = if t == 1 {
                    (&fixed_lit, &fixed_dist)
                } else {
                    let hlit = b.bits(5)? as usize + 257;
                    let hdist = b.bits(5)? as usize + 1;
                    let hclen = b.bits(4)? as usize + 4;
                    if hlit > 286 || hdist > 30 {
                        return Err("deflate: too many codes".into());
                    }
                    let mut cl = [0u8; 19];
                    for &o in CL_ORDER.iter().take(hclen) {
                        cl[o] = b.bits(3)? as u8;
                    }
                    let clh = Huff::new(&cl)?;
                    let mut lengths = vec![0u8; hlit + hdist];
                    let mut i = 0;
                    while i < hlit + hdist {
                        let sym = clh.decode(&mut b)?;
                        match sym {
                            0..=15 => {
                                lengths[i] = sym as u8;
                                i += 1;
                            }
                            16 | 17 | 18 => {
                                let (prev, rep) = match sym {
                                    16 => {
                                        if i == 0 {
                                            return Err("deflate: repeat without a previous length".into());
                                        }
                                        (lengths[i - 1], 3 + b.bits(2)? as usize)
                                    }
                                    17 => (0, 3 + b.bits(3)? as usize),
                                    _ => (0, 11 + b.bits(7)? as usize),
                                };
                                if i + rep > hlit + hdist {
                                    return Err("deflate: repeat runs past the table".into());
                                }
                                for _ in 0..rep {
                                    lengths[i] = prev;
                                    i += 1;
                                }
                            }
                            _ => return Err("deflate: invalid length symbol".into()),
                        }
                    }
                    if lengths[256] == 0 {
                        return Err("deflate: missing end-of-block code".into());
                    }
                    dynamic = (Huff::new(&lengths[..hlit])?, Huff::new(&lengths[hlit..])?);
                    (&dynamic.0, &dynamic.1)
                };
                loop {
                    let sym = lit.decode(&mut b)? as usize;
                    if sym < 256 {
                        if out.len() >= max_out {
                            return Err("deflate: more data than declared".into());
                        }
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let li = sym - 257;
                        if li >= 29 {
                            return Err("deflate: invalid length code".into());
                        }
                        let len = LEN_BASE[li] as usize + b.bits(LEN_EXTRA[li] as u32)? as usize;
                        let di = dist.decode(&mut b)? as usize;
                        if di >= 30 {
                            return Err("deflate: invalid distance code".into());
                        }
                        let d = DIST_BASE[di] as usize + b.bits(DIST_EXTRA[di] as u32)? as usize;
                        if d > out.len() {
                            return Err("deflate: reference before the start of the data".into());
                        }
                        if out.len() + len > max_out {
                            return Err("deflate: more data than declared".into());
                        }
                        for _ in 0..len {
                            let v = out[out.len() - d];
                            out.push(v);
                        }
                    }
                }
            }
            _ => return Err("deflate: invalid block type".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

/// The zlib wrapper (RFC 1950): a 2-byte header, deflate data and an Adler-32 checksum, which is verified.
pub fn zlib_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
    if data.len() < 6 {
        return Err("zlib: the stream is too short".into());
    }
    if data[0] & 0x0F != 8 || ((data[0] as u32) << 8 | data[1] as u32) % 31 != 0 {
        return Err("zlib: invalid header".into());
    }
    if data[1] & 0x20 != 0 {
        return Err("zlib: preset dictionaries are not supported".into());
    }
    let out = inflate(&data[2..data.len() - 4], max_out)?;
    let want = u32::from_be_bytes([data[data.len() - 4], data[data.len() - 3], data[data.len() - 2], data[data.len() - 1]]);
    if crate::encode::png::adler32(&out) != want {
        return Err("zlib: Adler-32 checksum mismatch".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::png::{adler32, zlib};

    const Z_TEXT_L9: &[u8] = &[120, 218, 237, 206, 203, 82, 131, 48, 24, 134, 97, 218, 132, 67, 160, 64, 57, 19, 78, 249, 75, 18, 216, 121, 45, 206, 216, 133, 91, 81, 172, 135, 218, 212, 214, 90, 235, 213, 203, 117, 56, 253, 182, 239, 204, 51, 223, 250, 101, 132, 207, 211, 235, 227, 59, 12, 7, 117, 222, 193, 179, 250, 129, 183, 211, 199, 254, 8, 234, 123, 60, 192, 215, 148, 183, 15, 191, 23, 120, 82, 155, 27, 184, 189, 187, 135, 189, 218, 94, 54, 106, 119, 132, 179, 26, 134, 237, 184, 130, 245, 85, 184, 10, 255, 78, 136, 9, 69, 184, 53, 50, 137, 192, 157, 235, 101, 129, 125, 157, 151, 72, 152, 158, 68, 66, 82, 228, 205, 185, 21, 22, 164, 53, 69, 196, 29, 67, 138, 69, 102, 112, 44, 80, 239, 178, 182, 140, 107, 89, 103, 145, 239, 248, 186, 136, 86, 44, 169, 194, 14, 155, 80, 216, 9, 97, 197, 28, 115, 17, 39, 105, 199, 100, 141, 245, 160, 193, 40, 18, 85, 152, 167, 179, 58, 181, 123, 147, 33, 55, 180, 124, 74, 153, 110, 87, 148, 7, 86, 201, 131, 34, 205, 61, 162, 59, 196, 243, 52, 38, 157, 101, 168, 145, 162, 205, 122, 17, 91, 208, 163, 154, 211, 105, 70, 67, 209, 2, 187, 149, 109, 38, 29, 50, 52, 65, 166, 207, 253, 12, 187, 125, 78, 150, 105, 151, 53, 127, 171, 234, 42, 200];
    const Z_STORED: &[u8] = &[120, 1, 1, 84, 0, 171, 255, 115, 116, 111, 114, 101, 100, 32, 98, 108, 111, 99, 107, 32, 100, 97, 116, 97, 32, 49, 50, 51, 52, 53, 54, 55, 56, 57, 48, 115, 116, 111, 114, 101, 100, 32, 98, 108, 111, 99, 107, 32, 100, 97, 116, 97, 32, 49, 50, 51, 52, 53, 54, 55, 56, 57, 48, 115, 116, 111, 114, 101, 100, 32, 98, 108, 111, 99, 107, 32, 100, 97, 116, 97, 32, 49, 50, 51, 52, 53, 54, 55, 56, 57, 48, 131, 252, 25, 234];
    const Z_EMPTY: &[u8] = &[120, 218, 3, 0, 0, 0, 0, 1];

    #[test]
    fn dynamic_huffman_from_real_zlib() {
        // a type-2 (dynamic) block produced by a real zlib at level 9; verified by the Adler-32 at the end of the stream
        let out = zlib_decompress(Z_TEXT_L9, 10_000).expect("dynamic block");
        assert_eq!(out.len(), 992);
        assert!(out.starts_with(b"The quick brown fox jumps over the lazy dog."));
        let trailer = u32::from_be_bytes(Z_TEXT_L9[Z_TEXT_L9.len() - 4..].try_into().unwrap());
        assert_eq!(adler32(&out), trailer);
    }
    #[test]
    fn stored_and_fixed_blocks() {
        let out = zlib_decompress(Z_STORED, 1000).unwrap();
        assert_eq!(out, b"stored block data 1234567890".repeat(3));
        assert_eq!(zlib_decompress(Z_EMPTY, 0).unwrap(), Vec::<u8>::new());
    }
    #[test]
    fn roundtrips_our_own_encoder() {
        let data: Vec<u8> = (0..30_000u32).map(|i| ((i * i / 13) % 251) as u8).collect();
        assert_eq!(zlib_decompress(&zlib(&data), data.len()).unwrap(), data);
    }
    #[test]
    fn bombs_truncation_and_corruption_are_errors_not_panics() {
        assert!(zlib_decompress(Z_TEXT_L9, 100).is_err(), "output limit");
        assert!(zlib_decompress(&Z_TEXT_L9[..Z_TEXT_L9.len() / 2], 10_000).is_err());
        let mut bad = Z_TEXT_L9.to_vec();
        bad[40] ^= 0xFF;
        assert!(zlib_decompress(&bad, 10_000).is_err());
        let mut sum = Z_TEXT_L9.to_vec();
        let n = sum.len();
        sum[n - 1] ^= 1;
        assert!(zlib_decompress(&sum, 10_000).is_err(), "Adler-32");
        // garbage must not panic
        for seed in 0..200u32 {
            let junk: Vec<u8> = (0..64).map(|i| (seed.wrapping_mul(2654435761).wrapping_add(i * 40503) >> 7) as u8).collect();
            let _ = inflate(&junk, 4096);
        }
    }
}
