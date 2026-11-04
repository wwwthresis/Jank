use psxcore::decode::image::decode_image;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| !n.ends_with(".rgba") && !n.ends_with(".txt")).collect();
    names.sort();
    for n in names {
        let bytes = std::fs::read(format!("{dir}/{n}")).unwrap();
        match decode_image(&n, &bytes) {
            Ok(img) => { std::fs::write(format!("{dir}/{n}.rgba"), &img.rgba).unwrap(); println!("{n} OK {} {}", img.w, img.h); }
            Err(e) => println!("{n} ERR {e}"),
        }
    }
}
