use psxcore::encode::gif::GifEncoder;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let (w, h) = (120u32, 80u32);
    let mut g = GifEncoder::new(std::fs::File::create(format!("{dir}/a.gif")).unwrap(), w, h, 12.0, 0);
    let mut seed = 12345u32;
    for f in 0..6u32 {
        let mut px = Vec::new();
        for y in 0..h { for x in 0..w {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = if f % 2 == 1 { (seed >> 24) as u8 } else { 0 }; // odd frames get noise (many colours, forces LZW table resets)
            px.extend([((x * 2 + f * 20) % 256) as u8 ^ noise, (y * 3) as u8, (f * 40) as u8 ^ (noise >> 1), 255]);
        }}
        if f == 0 { std::fs::write(format!("{dir}/gif_f0.raw"), &px).unwrap(); }
        g.add_frame(&px).unwrap();
    }
    g.finish().unwrap();
}
