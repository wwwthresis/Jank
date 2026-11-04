//! Simple image formats: BMP (24-bit), TGA (RLE) and PPM, plus the RGB to YCbCr colour conversion helper.

pub fn encode_bmp(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let row = ((w as usize * 3 + 3) / 4) * 4;
    let size = 54 + row * h as usize;
    let mut o = Vec::with_capacity(size);
    o.extend(b"BM");
    o.extend((size as u32).to_le_bytes());
    o.extend([0u8; 4]);
    o.extend(54u32.to_le_bytes());
    o.extend(40u32.to_le_bytes());
    o.extend((w as i32).to_le_bytes());
    o.extend((h as i32).to_le_bytes());
    o.extend(1u16.to_le_bytes());
    o.extend(24u16.to_le_bytes());
    o.extend([0u8; 24]);
    for y in (0..h as usize).rev() {
        let start = o.len();
        for x in 0..w as usize {
            let p = &rgba[(y * w as usize + x) * 4..];
            o.extend([p[2], p[1], p[0]]);
        }
        while o.len() - start < row {
            o.push(0);
        }
    }
    o
}

/// 32-bit TGA with RLE compression (image type 10), a format widely used for game assets.
pub fn encode_tga(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut o = vec![0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    o.extend((w as u16).to_le_bytes());
    o.extend((h as u16).to_le_bytes());
    o.extend([32, 0x28]); // 32 bits per pixel, 8 alpha bits, origin at top-left
    for y in 0..h as usize {
        let row: Vec<[u8; 4]> = (0..w as usize).map(|x| {
            let p = &rgba[(y * w as usize + x) * 4..];
            [p[2], p[1], p[0], p[3]]
        }).collect();
        let mut i = 0;
        while i < row.len() {
            let mut run = 1;
            while i + run < row.len() && run < 128 && row[i + run] == row[i] {
                run += 1;
            }
            if run >= 2 {
                o.push(0x80 | (run as u8 - 1));
                o.extend(row[i]);
                i += run;
            } else {
                let start = i;
                let mut n = 1;
                while i + n < row.len() && n < 128 && !(i + n + 1 < row.len() && row[i + n] == row[i + n + 1]) {
                    n += 1;
                }
                o.push(n as u8 - 1);
                for p in &row[start..start + n] {
                    o.extend(*p);
                }
                i += n;
            }
        }
    }
    o
}

pub fn encode_ppm(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut o = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in rgba.chunks_exact(4) {
        o.extend([p[0], p[1], p[2]]);
    }
    o
}

/// RGB to YCbCr (ITU-R BT.601, full range), for JPEG and Y4M output.
pub fn rgb_to_ycbcr(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32, g as f32, b as f32);
    (0.299 * r + 0.587 * g + 0.114 * b, -0.168_736 * r - 0.331_264 * g + 0.5 * b + 128.0, 0.5 * r - 0.418_688 * g - 0.081_312 * b + 128.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes() {
        let px = vec![255u8; 5 * 3 * 4];
        assert_eq!(encode_bmp(5, 3, &px).len(), 54 + 16 * 3);
        assert_eq!(encode_ppm(5, 3, &px).len(), "P6\n5 3\n255\n".len() + 45);
        assert!(encode_tga(5, 3, &px).len() < 18 + 5 * 3 * 5);
    }
}
