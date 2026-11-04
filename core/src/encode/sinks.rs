//! Frame sinks: a unified video output interface. A new format (MP4, WebM) is added by
//! implementing `VideoSink` in the application layer; the core and the render pipeline stay unchanged.
use super::avi::{AviCodec, AviEncoder};
use super::gif::GifEncoder;
use super::png::encode_png;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

pub trait VideoSink {
    /// `rgba` must hold exactly w*h*4 bytes.
    fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()>;
    fn finish(self: Box<Self>) -> io::Result<()>;
}

pub struct AviSink(AviEncoder<BufWriter<File>>);
impl AviSink {
    pub fn create(path: &Path, w: u32, h: u32, fps_num: u32, fps_den: u32, codec: AviCodec) -> io::Result<Self> {
        Ok(Self(AviEncoder::new(BufWriter::new(File::create(path)?), w, h, fps_num, fps_den, codec)?))
    }
}
impl VideoSink for AviSink {
    fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        self.0.add_frame(rgba)
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        self.0.finish().map(|_| ())
    }
}

pub struct GifSink(GifEncoder<BufWriter<File>>);
impl GifSink {
    pub fn create(path: &Path, w: u32, h: u32, fps: f64, loops: u16) -> io::Result<Self> {
        Ok(Self(GifEncoder::new(BufWriter::new(File::create(path)?), w, h, fps, loops)))
    }
}
impl VideoSink for GifSink {
    fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        self.0.add_frame(rgba)
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        self.0.finish().map(|_| ())
    }
}

/// PNG sequence: `<prefix>_00001.png`, ... for assembly in any external editor or engine.
pub struct PngSeqSink {
    dir: PathBuf,
    prefix: String,
    w: u32,
    h: u32,
    n: u32,
    alpha: bool,
}
impl PngSeqSink {
    pub fn create(dir: &Path, prefix: &str, w: u32, h: u32, alpha: bool) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Self { dir: dir.to_path_buf(), prefix: prefix.into(), w, h, n: 0, alpha })
    }
}
impl VideoSink for PngSeqSink {
    fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        self.n += 1;
        std::fs::write(self.dir.join(format!("{}_{:05}.png", self.prefix, self.n)), encode_png(self.w, self.h, rgba, self.alpha))
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        Ok(())
    }
}

/// Collects frames in memory, for tests and previews.
#[derive(Default)]
pub struct MemorySink {
    pub frames: Vec<Vec<u8>>,
}
impl VideoSink for MemorySink {
    fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        self.frames.push(rgba.to_vec());
        Ok(())
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        Ok(())
    }
}
