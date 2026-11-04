//! Reference images were produced by PIL (an independent decoder). Lossless formats must match
//! bit for bit; JPEG must match within the IDCT/rounding tolerance (libjpeg uses an integer IDCT).
use psxcore::decode::image::decode_image;

fn check(name: &str, bytes: &[u8], expected: &[u8], tol: i32) {
    let img = decode_image(name, bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(img.rgba.len(), expected.len(), "{name}: size mismatch");
    let maxd = img.rgba.iter().zip(expected).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
    assert!(maxd <= tol, "{name}: maximum deviation {maxd} exceeds tolerance {tol}");
}
macro_rules! fx {
    ($n:literal, $tol:expr) => {
        check($n, include_bytes!(concat!("fixtures/", $n)), include_bytes!(concat!("fixtures/", $n, ".expected")), $tol)
    };
}
#[test]
fn lossless_formats_match_pil_exactly() {
    for_each_lossless();
}
fn for_each_lossless() {
    fx!("rgba.png", 0);
    fx!("pal200.png", 0); // palette + tRNS
    fx!("gray16.png", 0); // 16-bit
    fx!("adam7.png", 0); // interlaced
    fx!("mono.png", 0); // 1-bit
    fx!("a8.bmp", 0);
    fx!("a24.bmp", 0);
    fx!("a32.tga", 0); // RLE + alpha
}
#[test]
fn jpeg_matches_libjpeg_closely() {
    fx!("j444.jpg", 3);
    fx!("j420.jpg", 4); // chroma subsampling: bilinear chroma upsampling, comparable to libjpeg's "fancy upsampling"
    fx!("jgray.jpg", 2);
}
