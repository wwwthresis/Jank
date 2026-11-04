use psxcore::encode::avi::{AviCodec, AviEncoder};
fn frame(w: u32, h: u32, f: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h { for x in 0..w {
        let cx = (f * 6 % w) as i32; let d = ((x as i32 - cx).pow(2) + (y as i32 - (h / 2) as i32).pow(2)) as f32;
        let ball = if d < 400.0 { 255 } else { 0 };
        px.extend([((x + f * 3) % 256) as u8, (y * 255 / h) as u8, ball, 255]);
    }}
    px
}
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let (w, h) = (320u32, 240u32);
    for (name, codec) in [("q.avi", AviCodec::MjpegQuality(85)), ("br.avi", AviCodec::MjpegBitrate(1_500_000)), ("raw.avi", AviCodec::Raw)] {
        let f = std::fs::File::create(format!("{dir}/{name}")).unwrap();
        let mut e = AviEncoder::new(f, w, h, 24000, 1001, codec).unwrap();
        for i in 0..24 { e.add_frame(&frame(w, h, i)).unwrap(); }
        println!("{name}: avg bitrate {:.0} bps", e.average_bitrate());
        e.finish().unwrap();
    }
}
