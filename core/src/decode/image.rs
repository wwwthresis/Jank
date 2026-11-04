//! Image decoders in pure std Rust: PNG (all colour types, bit depths 1-16, Adam7), BMP, TGA and PPM.
//! The result is always RGBA8. Every failure is an `Err(String)` with a readable message; malformed input never panics.
use super::inflate::zlib_decompress;
use crate::encode::png::crc32;

#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Upper limit on the pixel count: protection against decompression-bomb files.
pub const MAX_PIXELS: u64 = 1 << 27; // about 134 megapixels

fn check_dims(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("the image has zero size".into());
    }
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(format!("the image is too large: {w}×{h}"));
    }
    Ok(())
}

/// Detects the format from its signature; TGA has no signature, so it is detected from the extension in `name`.
pub fn decode_image(name: &str, bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        decode_png(bytes)
    } else if bytes.starts_with(b"BM") {
        decode_bmp(bytes)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        super::jpeg::decode_jpeg(bytes)
    } else if bytes.starts_with(b"P6") || bytes.starts_with(b"P3") {
        decode_ppm(bytes)
    } else if name.to_ascii_lowercase().ends_with(".tga") {
        decode_tga(bytes)
    } else {
        Err("Unknown image format (supported: PNG, JPEG, BMP, TGA, PPM)".into())
    }
}

// ------------------------------------------------------------------ PNG

fn paeth(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
}

/// Reverses the row filters of one pass. `data` holds (1 + stride) * rows bytes.
fn unfilter(data: &[u8], stride: usize, rows: usize, bpp: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; stride * rows];
    for y in 0..rows {
        let src = data.get(y * (stride + 1)..(y + 1) * (stride + 1)).ok_or("PNG: fewer data than rows")?;
        let ft = src[0];
        let line = &src[1..];
        for x in 0..stride {
            let a = if x >= bpp { out[y * stride + x - bpp] as i32 } else { 0 };
            let b = if y > 0 { out[(y - 1) * stride + x] as i32 } else { 0 };
            let c = if y > 0 && x >= bpp { out[(y - 1) * stride + x - bpp] as i32 } else { 0 };
            let pred = match ft {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => paeth(a, b, c),
                f => return Err(format!("PNG: unknown filter {f}")),
            };
            out[y * stride + x] = (line[x] as i32 + pred) as u8;
        }
    }
    Ok(out)
}

pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() < 8 || bytes[..8] != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Err("PNG: invalid signature".into());
    }
    let (mut w, mut h, mut depth, mut ctype, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let (mut palette, mut trns): (Vec<[u8; 3]>, Vec<u8>) = (vec![], vec![]);
    let mut idat: Vec<u8> = vec![];
    let mut pos = 8;
    let mut seen_ihdr = false;
    loop {
        let hdr = bytes.get(pos..pos + 8).ok_or("PNG: the file is cut off")?;
        let len = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
        let ty = &hdr[4..8];
        let data = bytes.get(pos + 8..pos + 8 + len).ok_or("PNG: a chunk is cut off")?;
        let crc_at = pos + 8 + len;
        let crc = bytes.get(crc_at..crc_at + 4).ok_or("PNG: missing CRC")?;
        let mut chunk = ty.to_vec();
        chunk.extend(data);
        if crc32(&chunk) != u32::from_be_bytes([crc[0], crc[1], crc[2], crc[3]]) {
            return Err(format!("PNG: CRC of chunk {} does not match", String::from_utf8_lossy(ty)));
        }
        match ty {
            b"IHDR" => {
                if data.len() < 13 {
                    return Err("PNG: IHDR is too short".into());
                }
                w = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                h = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                depth = data[8];
                ctype = data[9];
                interlace = data[12];
                seen_ihdr = true;
                check_dims(w, h)?;
                let ok = match ctype {
                    0 => [1, 2, 4, 8, 16].contains(&depth),
                    2 | 4 | 6 => [8, 16].contains(&depth),
                    3 => [1, 2, 4, 8].contains(&depth),
                    _ => false,
                };
                if !ok {
                    return Err(format!("PNG: unsupported combination of colour type {ctype} and bit depth {depth}"));
                }
            }
            b"PLTE" => palette = data.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            b"tRNS" => trns = data.to_vec(),
            b"IDAT" => idat.extend(data),
            b"IEND" => break,
            _ => {}
        }
        pos = crc_at + 4;
    }
    if !seen_ihdr {
        return Err("PNG: missing IHDR".into());
    }
    let channels = match ctype {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    let bits_pp = channels * depth as usize;
    let bpp = (bits_pp + 7) / 8;
    let max_raw = (w as usize * h as usize * bits_pp / 8 + h as usize * 8 + 64) * if interlace == 1 { 2 } else { 1 };
    let raw = zlib_decompress(&idat, max_raw)?;

    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    let max_v = ((1u32 << depth) - 1) as f32;
    // one pixel of a row converted to RGBA
    let pixel = |row: &[u8], x: usize| -> [u8; 4] {
        let sample = |idx: usize| -> u16 {
            // idx is the sample number within the row
            match depth {
                8 => row[idx] as u16,
                16 => u16::from_be_bytes([row[idx * 2], row[idx * 2 + 1]]),
                d => {
                    let per = 8 / d as usize;
                    let byte = row[idx / per];
                    let shift = 8 - d as usize * (idx % per + 1);
                    ((byte >> shift) & ((1u8 << d) - 1)) as u16
                }
            }
        };
        let to8 = |v: u16| -> u8 {
            if depth == 8 { v as u8 } else if depth == 16 { (v >> 8) as u8 } else { (v as f32 / max_v * 255.0).round() as u8 }
        };
        match ctype {
            0 => {
                let v = sample(x);
                let a = if trns.len() >= 2 && u16::from_be_bytes([trns[0], trns[1]]) == v { 0 } else { 255 };
                let g = to8(v);
                [g, g, g, a]
            }
            2 => {
                let (r, g, b) = (sample(x * 3), sample(x * 3 + 1), sample(x * 3 + 2));
                let a = if trns.len() >= 6 && u16::from_be_bytes([trns[0], trns[1]]) == r && u16::from_be_bytes([trns[2], trns[3]]) == g && u16::from_be_bytes([trns[4], trns[5]]) == b { 0 } else { 255 };
                [to8(r), to8(g), to8(b), a]
            }
            3 => {
                let i = sample(x) as usize;
                let c = palette.get(i).copied().unwrap_or([0, 0, 0]);
                [c[0], c[1], c[2], trns.get(i).copied().unwrap_or(255)]
            }
            4 => {
                let g = to8(sample(x * 2));
                [g, g, g, to8(sample(x * 2 + 1))]
            }
            _ => [to8(sample(x * 4)), to8(sample(x * 4 + 1)), to8(sample(x * 4 + 2)), to8(sample(x * 4 + 3))],
        }
    };

    if interlace == 0 {
        let stride = (w as usize * bits_pp + 7) / 8;
        let img = unfilter(&raw, stride, h as usize, bpp)?;
        for y in 0..h as usize {
            let row = &img[y * stride..(y + 1) * stride];
            for x in 0..w as usize {
                let o = (y * w as usize + x) * 4;
                rgba[o..o + 4].copy_from_slice(&pixel(row, x));
            }
        }
    } else if interlace == 1 {
        const PASSES: [(usize, usize, usize, usize); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
        let mut off = 0;
        for &(xs, ys, dx, dy) in &PASSES {
            let pw = (w as usize + dx - 1 - xs) / dx;
            let ph = (h as usize + dy - 1 - ys) / dy;
            if pw == 0 || ph == 0 {
                continue;
            }
            let stride = (pw * bits_pp + 7) / 8;
            let size = (stride + 1) * ph;
            let part = raw.get(off..off + size).ok_or("PNG: fewer data than the Adam7 passes need")?;
            off += size;
            let img = unfilter(part, stride, ph, bpp)?;
            for py in 0..ph {
                let row = &img[py * stride..(py + 1) * stride];
                for px in 0..pw {
                    let (x, y) = (xs + px * dx, ys + py * dy);
                    let o = (y * w as usize + x) * 4;
                    rgba[o..o + 4].copy_from_slice(&pixel(row, px));
                }
            }
        }
    } else {
        return Err("PNG: unknown interlace method".into());
    }
    Ok(Image { w, h, rgba })
}

// ------------------------------------------------------------------ BMP

pub fn decode_bmp(b: &[u8]) -> Result<Image, String> {
    let rd32 = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or("BMP: the file is cut off".to_string());
    let rd16 = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or("BMP: the file is cut off".to_string());
    let data_off = rd32(10)? as usize;
    let hdr = rd32(14)? as usize;
    if hdr < 40 {
        return Err("BMP: only BITMAPINFOHEADER and newer headers are supported".into());
    }
    let w = rd32(18)? as i32;
    let hraw = rd32(22)? as i32;
    let bits = rd16(28)?;
    let comp = rd32(30)?;
    let ncol = rd32(46)? as usize;
    if w <= 0 || hraw == 0 {
        return Err("BMP: invalid size".into());
    }
    let (w, h, top_down) = (w as u32, hraw.unsigned_abs(), hraw < 0);
    check_dims(w, h)?;
    if comp != 0 && !(comp == 3 && bits == 32) {
        return Err(format!("BMP: compression {comp} is not supported"));
    }
    let pal_off = 14 + hdr;
    let ncolors = if ncol == 0 && bits <= 8 { 1usize << bits } else { ncol };
    let palette: Vec<[u8; 3]> = (0..ncolors).filter_map(|i| b.get(pal_off + i * 4..pal_off + i * 4 + 3).map(|c| [c[2], c[1], c[0]])).collect();
    let stride = (((w as usize * bits as usize) + 31) / 32) * 4;
    let mut rgba = vec![255u8; w as usize * h as usize * 4];
    for y in 0..h as usize {
        let src_y = if top_down { y } else { h as usize - 1 - y };
        let row = b.get(data_off + src_y * stride..data_off + src_y * stride + stride).ok_or("BMP: fewer pixel data than needed")?;
        for x in 0..w as usize {
            let o = (y * w as usize + x) * 4;
            let px: [u8; 4] = match bits {
                24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255],
                32 => [row[x * 4 + 2], row[x * 4 + 1], row[x * 4], if comp == 3 { row[x * 4 + 3] } else { 255 }],
                8 => {
                    let c = palette.get(row[x] as usize).copied().unwrap_or([0; 3]);
                    [c[0], c[1], c[2], 255]
                }
                4 => {
                    let v = (row[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 15;
                    let c = palette.get(v as usize).copied().unwrap_or([0; 3]);
                    [c[0], c[1], c[2], 255]
                }
                1 => {
                    let v = (row[x / 8] >> (7 - x % 8)) & 1;
                    let c = palette.get(v as usize).copied().unwrap_or([0; 3]);
                    [c[0], c[1], c[2], 255]
                }
                other => return Err(format!("BMP: {other} bits per pixel are not supported")),
            };
            rgba[o..o + 4].copy_from_slice(&px);
        }
    }
    Ok(Image { w, h, rgba })
}

// ------------------------------------------------------------------ TGA

pub fn decode_tga(b: &[u8]) -> Result<Image, String> {
    if b.len() < 18 {
        return Err("TGA: the file is too short".into());
    }
    let (id_len, cmap_type, ty) = (b[0] as usize, b[1], b[2]);
    let cmap_len = u16::from_le_bytes([b[5], b[6]]) as usize;
    let cmap_bits = b[7] as usize;
    let (w, h) = (u16::from_le_bytes([b[12], b[13]]) as u32, u16::from_le_bytes([b[14], b[15]]) as u32);
    let (bpp, desc) = (b[16] as usize, b[17]);
    check_dims(w, h)?;
    let rle = matches!(ty, 9 | 10 | 11);
    let kind = ty & 7; // 1 = palette, 2 = true colour, 3 = grey
    if !matches!(kind, 1 | 2 | 3) {
        return Err(format!("TGA: type {ty} is not supported"));
    }
    let mut pos = 18 + id_len;
    let mut palette: Vec<[u8; 4]> = vec![];
    if cmap_type == 1 {
        let ent = (cmap_bits + 7) / 8;
        for i in 0..cmap_len {
            let e = b.get(pos + i * ent..pos + (i + 1) * ent).ok_or("TGA: the palette is cut off")?;
            palette.push(match ent {
                3 => [e[2], e[1], e[0], 255],
                4 => [e[2], e[1], e[0], e[3]],
                _ => return Err("TGA: this palette format is not supported".into()),
            });
        }
        pos += cmap_len * ent;
    }
    let bytes_pp = (bpp + 7) / 8;
    if bytes_pp == 0 || bytes_pp > 4 {
        return Err("TGA: unsupported bit depth".into());
    }
    let n = w as usize * h as usize;
    let mut raw: Vec<u8> = Vec::with_capacity(n * bytes_pp);
    if rle {
        while raw.len() < n * bytes_pp {
            let head = *b.get(pos).ok_or("TGA: the data is cut off")?;
            pos += 1;
            let cnt = (head & 0x7F) as usize + 1;
            if head & 0x80 != 0 {
                let px = b.get(pos..pos + bytes_pp).ok_or("TGA: the data is cut off")?;
                pos += bytes_pp;
                for _ in 0..cnt {
                    raw.extend(px);
                }
            } else {
                let px = b.get(pos..pos + cnt * bytes_pp).ok_or("TGA: the data is cut off")?;
                pos += cnt * bytes_pp;
                raw.extend(px);
            }
        }
        raw.truncate(n * bytes_pp);
    } else {
        raw.extend(b.get(pos..pos + n * bytes_pp).ok_or("TGA: fewer pixel data than needed")?);
    }
    let mut rgba = vec![0u8; n * 4];
    for i in 0..n {
        let p = &raw[i * bytes_pp..(i + 1) * bytes_pp];
        let px: [u8; 4] = match (kind, bpp) {
            (1, 8) => palette.get(p[0] as usize).copied().unwrap_or([0, 0, 0, 255]),
            (3, 8) => [p[0], p[0], p[0], 255],
            (2, 24) => [p[2], p[1], p[0], 255],
            (2, 32) => [p[2], p[1], p[0], p[3]],
            (2, 16) => {
                let v = u16::from_le_bytes([p[0], p[1]]);
                let e = |s: u16| (((v >> s) & 31) * 255 / 31) as u8;
                [e(10), e(5), e(0), if v & 0x8000 != 0 || desc & 0x0F == 0 { 255 } else { 0 }]
            }
            _ => return Err(format!("TGA: the combination of type {ty} and {bpp} bits is not supported")),
        };
        // row order: descriptor bit 5 set means the image starts at the top
        let (x, y) = (i as u32 % w, i as u32 / w);
        let y = if desc & 0x20 != 0 { y } else { h - 1 - y };
        let x = if desc & 0x10 != 0 { w - 1 - x } else { x };
        let o = (y * w + x) as usize * 4;
        rgba[o..o + 4].copy_from_slice(&px);
    }
    Ok(Image { w, h, rgba })
}

// ------------------------------------------------------------------ PPM

pub fn decode_ppm(b: &[u8]) -> Result<Image, String> {
    let binary = b.starts_with(b"P6");
    // header: tokens separated by whitespace or comments
    let mut pos = 2;
    let tok = |pos: &mut usize| -> Result<u32, String> {
        loop {
            while *pos < b.len() && b[*pos].is_ascii_whitespace() {
                *pos += 1;
            }
            if *pos < b.len() && b[*pos] == b'#' {
                while *pos < b.len() && b[*pos] != b'\n' {
                    *pos += 1;
                }
            } else {
                break;
            }
        }
        let s = *pos;
        while *pos < b.len() && b[*pos].is_ascii_digit() {
            *pos += 1;
        }
        std::str::from_utf8(&b[s..*pos]).ok().and_then(|t| t.parse().ok()).ok_or_else(|| "PPM: invalid header".to_string())
    };
    let (w, h, maxv) = (tok(&mut pos)?, tok(&mut pos)?, tok(&mut pos)?);
    check_dims(w, h)?;
    if maxv == 0 || maxv > 255 {
        return Err("PPM: only maxval up to 255 is supported".into());
    }
    let n = w as usize * h as usize;
    let mut rgba = Vec::with_capacity(n * 4);
    if binary {
        pos += 1; // the single whitespace character after maxval
        let d = b.get(pos..pos + n * 3).ok_or("PPM: fewer data than needed")?;
        for p in d.chunks_exact(3) {
            rgba.extend([(p[0] as u32 * 255 / maxv) as u8, (p[1] as u32 * 255 / maxv) as u8, (p[2] as u32 * 255 / maxv) as u8, 255]);
        }
    } else {
        for _ in 0..n {
            let (r, g, bl) = (tok(&mut pos)?, tok(&mut pos)?, tok(&mut pos)?);
            rgba.extend([(r * 255 / maxv) as u8, (g * 255 / maxv) as u8, (bl * 255 / maxv) as u8, 255]);
        }
    }
    Ok(Image { w, h, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::{png::encode_png, simple::*};

    fn sample(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend([(x * 17) as u8, (y * 29) as u8, (x * y) as u8, if (x + y) % 5 == 0 { 100 } else { 255 }]);
            }
        }
        v
    }
    #[test]
    fn own_encoders_roundtrip() {
        let px = sample(19, 11);
        assert_eq!(decode_image("a.png", &encode_png(19, 11, &px, true)).unwrap().rgba, px);
        let rgb: Vec<u8> = px.chunks(4).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        assert_eq!(decode_image("a.png", &encode_png(19, 11, &rgb, false)).unwrap().rgba, rgb);
        assert_eq!(decode_image("a.bmp", &encode_bmp(19, 11, &px)).unwrap().rgba, rgb);
        assert_eq!(decode_image("a.tga", &encode_tga(19, 11, &px)).unwrap().rgba, px);
        assert_eq!(decode_image("a.ppm", &encode_ppm(19, 11, &px)).unwrap().rgba, rgb);
    }
    #[test]
    fn hostile_inputs_do_not_panic() {
        assert!(decode_image("x", b"").is_err());
        assert!(decode_image("x.png", &[0x89, b'P', b'N', b'G', 13, 10, 26, 10]).is_err());
        let good = encode_png(8, 8, &sample(8, 8), true);
        for cut in [9, 20, good.len() / 2, good.len() - 1] {
            assert!(decode_png(&good[..cut]).is_err());
        }
        let mut bad = good.clone();
        bad[20] ^= 0xFF;
        assert!(decode_png(&bad).is_err(), "CRC");
        // declared huge dimensions
        let mut huge = encode_png(2, 2, &sample(2, 2), true);
        huge[16..20].copy_from_slice(&0x7FFF_FFFFu32.to_be_bytes());
        assert!(decode_png(&huge).is_err());
        assert!(decode_tga(&[0; 18]).is_err());
        assert!(decode_bmp(b"BM\0\0\0\0\0\0\0\0\0\0\0\0").is_err());
    }
}
