//! Unified video output interface. The application chooses the format; the core writes the frames.
//! Extension point: MP4/H.264/WebM can be added by implementing `VideoSink` in the application layer
//! (via a codec crate) without changing the core.
use super::avi::{AviCodec, AviEncoder, RateControl};
use super::gif::GifEncoder;
use super::png::encode_png;
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

pub trait VideoSink {
    fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()>;
    fn finish(self: Box<Self>) -> io::Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VideoFormat {
    AviMjpeg(RateControl),
    AviRaw,
    Gif { loops: u16 },
    /// A folder of frame_00001.png, frame_00002.png, ... (for editing in external tools).
    PngSequence,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoSpec {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub format: VideoFormat,
}
impl VideoSpec {
    pub fn fps(&self) -> f64 {
        self.fps_num as f64 / self.fps_den as f64
    }
    pub fn extension(&self) -> &'static str {
        match self.format {
            VideoFormat::AviMjpeg(_) | VideoFormat::AviRaw => "avi",
            VideoFormat::Gif { .. } => "gif",
            VideoFormat::PngSequence => "",
        }
    }
}

struct AviSink(AviEncoder<BufWriter<File>>);
impl VideoSink for AviSink {
    fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        self.0.add_frame(rgba)
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        self.0.finish().map(|_| ())
    }
}
struct GifSink(GifEncoder<BufWriter<File>>);
impl VideoSink for GifSink {
    fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        self.0.add_frame(rgba)
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        self.0.finish().map(|_| ())
    }
}
struct PngSeqSink {
    dir: PathBuf,
    w: u32,
    h: u32,
    n: u32,
}
impl VideoSink for PngSeqSink {
    fn add_frame(&mut self, rgba: &[u8]) -> io::Result<()> {
        self.n += 1;
        fs::write(self.dir.join(format!("frame_{:05}.png", self.n)), encode_png(self.w, self.h, rgba, false))
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        Ok(())
    }
}

/// Opens a frame sink. For `PngSequence`, `path` is a folder (created if missing); otherwise it is a file.
pub fn open_sink(spec: &VideoSpec, path: &Path) -> io::Result<Box<dyn VideoSink>> {
    if spec.width == 0 || spec.height == 0 || spec.fps_num == 0 || spec.fps_den == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "width, height and fps must be non-zero"));
    }
    Ok(match spec.format {
        VideoFormat::AviMjpeg(rc) => Box::new(AviSink(AviEncoder::new(BufWriter::new(File::create(path)?), spec.width, spec.height, spec.fps_num, spec.fps_den, AviCodec::Mjpeg(rc))?)),
        VideoFormat::AviRaw => Box::new(AviSink(AviEncoder::new(BufWriter::new(File::create(path)?), spec.width, spec.height, spec.fps_num, spec.fps_den, AviCodec::Raw)?)),
        VideoFormat::Gif { loops } => Box::new(GifSink(GifEncoder::new(BufWriter::new(File::create(path)?), spec.width, spec.height, spec.fps(), loops))),
        VideoFormat::PngSequence => {
            fs::create_dir_all(path)?;
            Box::new(PngSeqSink { dir: path.to_path_buf(), w: spec.width, h: spec.height, n: 0 })
        }
    })
}
