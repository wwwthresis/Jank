//! Export on a background thread: the interface never freezes, and there is progress reporting and cancellation.
//! The file is written under a temporary `*.part` name and renamed only after success,
//! so an error or a cancellation never leaves a broken file on disk.
use crate::encode::avi::AviCodec;
use crate::encode::sinks::{AviSink, GifSink, PngSeqSink, VideoSink};
use crate::export::{bake_video, check_size, render_photo, ImageFormat};
use crate::project::Project;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

#[derive(Clone, Debug, PartialEq)]
pub enum VideoKind {
    /// AVI with Motion-JPEG frames, quality 1..=100.
    AviQuality(u8),
    /// AVI with Motion-JPEG frames aimed at a target bitrate in bits per second.
    AviBitrate(u32),
    /// Uncompressed AVI.
    AviRaw,
    Gif { loops: u16 },
    /// A folder of PNG frames named `<name>_00001.png` and so on.
    PngSequence { alpha: bool },
}
impl VideoKind {
    pub fn extension(&self) -> &'static str {
        match self {
            VideoKind::AviQuality(_) | VideoKind::AviBitrate(_) | VideoKind::AviRaw => "avi",
            VideoKind::Gif { .. } => "gif",
            VideoKind::PngSequence { .. } => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A single frame of the animation (a playback tick).
    Photo { format: ImageFormat, tick: u32 },
    Video(VideoKind),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportSpec {
    pub target: Target,
    pub width: u32,
    pub height: u32,
    /// Output path (for a frame sequence this is the folder itself).
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportReport {
    pub path: PathBuf,
    pub frames: u32,
    pub bytes: u64,
    pub cancelled: bool,
}

pub struct ExportJob {
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<ExportReport, String>>>,
    /// A video (true) or a single picture.
    pub video: bool,
}

fn part_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

fn dir_size(p: &Path) -> u64 {
    std::fs::read_dir(p).map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()).unwrap_or(0)
}

fn run(project: &Project, anim_index: usize, spec: &ExportSpec, progress: &AtomicU32, cancel: &AtomicBool) -> Result<ExportReport, String> {
    let anim = project.animations.get(anim_index).ok_or("animation not found")?;
    if anim.frames.is_empty() {
        return Err("The animation has no frames".into());
    }
    check_size(spec.width, spec.height).map_err(|e| e.to_string())?;
    if let Some(dir) = spec.path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create folder \"{}\": {e}", dir.display()))?;
    }
    let io = |e: std::io::Error| format!("could not write \"{}\": {e}", spec.path.display());
    match &spec.target {
        Target::Photo { format, tick } => {
            let bytes = render_photo(project, anim, *tick, spec.width, spec.height, *format).map_err(io)?;
            let part = part_path(&spec.path);
            std::fs::write(&part, &bytes).map_err(io)?;
            std::fs::rename(&part, &spec.path).map_err(io)?;
            progress.store(1000, Ordering::Relaxed);
            Ok(ExportReport { path: spec.path.clone(), frames: 1, bytes: bytes.len() as u64, cancelled: false })
        }
        Target::Video(kind) => {
            let (w, h) = (spec.width, spec.height);
            let (num, den) = (anim.fps_num.max(1), anim.fps_den.max(1));
            let is_dir = matches!(kind, VideoKind::PngSequence { .. });
            let out_path = if is_dir { spec.path.clone() } else { part_path(&spec.path) };
            let mut sink: Box<dyn VideoSink> = match kind {
                VideoKind::AviQuality(q) => Box::new(AviSink::create(&out_path, w, h, num, den, AviCodec::MjpegQuality((*q).clamp(1, 100))).map_err(io)?),
                VideoKind::AviBitrate(b) => Box::new(AviSink::create(&out_path, w, h, num, den, AviCodec::MjpegBitrate((*b).max(1000))).map_err(io)?),
                VideoKind::AviRaw => Box::new(AviSink::create(&out_path, w, h, num, den, AviCodec::Raw).map_err(io)?),
                VideoKind::Gif { loops } => Box::new(GifSink::create(&out_path, w, h, num as f64 / den as f64, *loops).map_err(io)?),
                VideoKind::PngSequence { alpha } => {
                    let prefix = spec.path.file_name().and_then(|n| n.to_str()).unwrap_or("frame").to_string();
                    Box::new(PngSeqSink::create(&spec.path, &prefix, w, h, *alpha).map_err(io)?)
                }
            };
            let mut cb = |done: u32, total: u32| {
                progress.store((done as u64 * 1000 / total.max(1) as u64) as u32, Ordering::Relaxed);
                !cancel.load(Ordering::Relaxed)
            };
            let res = bake_video(project, anim, w, h, sink.as_mut(), &mut cb);
            let frames = match res {
                Ok(n) => n,
                Err(e) => {
                    drop(sink);
                    if !is_dir {
                        let _ = std::fs::remove_file(&out_path);
                    }
                    return Err(format!("error while writing the video: {e}"));
                }
            };
            sink.finish().map_err(io)?;
            if cancel.load(Ordering::Relaxed) && frames < anim.total_ticks() {
                if !is_dir {
                    let _ = std::fs::remove_file(&out_path);
                }
                return Ok(ExportReport { path: spec.path.clone(), frames, bytes: 0, cancelled: true });
            }
            let bytes = if is_dir {
                dir_size(&spec.path)
            } else {
                std::fs::rename(&out_path, &spec.path).map_err(io)?;
                std::fs::metadata(&spec.path).map(|m| m.len()).unwrap_or(0)
            };
            progress.store(1000, Ordering::Relaxed);
            Ok(ExportReport { path: spec.path.clone(), frames, bytes, cancelled: false })
        }
    }
}

impl ExportJob {
    /// Starts exporting animation `anim_index`. The project is copied, so editing can continue undisturbed.
    pub fn start(project: &Project, anim_index: usize, spec: ExportSpec) -> Result<ExportJob, String> {
        if project.animations.get(anim_index).is_none() {
            return Err("animation not found".into());
        }
        check_size(spec.width, spec.height).map_err(|e| e.to_string())?;
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c, proj) = (progress.clone(), cancel.clone(), project.clone());
        let video = matches!(spec.target, Target::Video(_));
        let handle = std::thread::Builder::new()
            .name("jank-export".into())
            .spawn(move || {
                // a panic inside an encoder must not take the interface down: turn it into a readable error
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&proj, anim_index, &spec, &p, &c))).unwrap_or_else(|_| Err("internal export error".into()))
            })
            .map_err(|e| format!("could not start the export: {e}"))?;
        Ok(ExportJob { progress, cancel, handle: Some(handle), video })
    }
    /// 0.0..=1.0
    pub fn progress(&self) -> f32 {
        self.progress.load(Ordering::Relaxed) as f32 / 1000.0
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().map_or(true, |h| h.is_finished())
    }
    /// Takes the result if the thread has finished. A second call returns `None`.
    pub fn take_result(&mut self) -> Option<Result<ExportReport, String>> {
        if !self.is_finished() {
            return None;
        }
        let h = self.handle.take()?;
        Some(h.join().unwrap_or_else(|_| Err("the export thread crashed".into())))
    }
    /// Waits for completion (for tests and command-line runs).
    pub fn wait(mut self) -> Result<ExportReport, String> {
        match self.handle.take() {
            Some(h) => h.join().unwrap_or_else(|_| Err("the export thread crashed".into())),
            None => Err("the result was already taken".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Editor, Mode, Primitive};

    fn project() -> Project {
        let mut ed = Editor::new();
        ed.add_primitive(Primitive::Cube);
        ed.set_mode(Mode::Animate);
        ed.add_frame();
        ed.set_fps(12, 1);
        let mut p = ed.project;
        p.scene.style.width = 64;
        p.scene.style.height = 48;
        p
    }
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jank_job_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn spec(path: PathBuf, target: Target, w: u32, h: u32) -> ExportSpec {
        ExportSpec { target, width: w, height: h, path }
    }

    #[test]
    fn photo_video_gif_and_sequence_are_written_and_valid() {
        let d = tmp("ok");
        let p = project();
        let r = ExportJob::start(&p, 0, spec(d.join("a.png"), Target::Photo { format: ImageFormat::Png, tick: 0 }, 200, 150)).unwrap().wait().unwrap();
        let png = std::fs::read(&r.path).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let img = crate::decode::image::decode_image("a.png", &png).unwrap();
        assert_eq!((img.w, img.h), (200, 150));

        let r = ExportJob::start(&p, 0, spec(d.join("v.avi"), Target::Video(VideoKind::AviQuality(80)), 128, 96)).unwrap().wait().unwrap();
        assert_eq!(r.frames, 2);
        let avi = std::fs::read(&r.path).unwrap();
        assert_eq!(&avi[..4], b"RIFF");
        assert_eq!(&avi[8..12], b"AVI ");
        assert!(r.bytes > 0 && !d.join("v.avi.part").exists());

        let r = ExportJob::start(&p, 0, spec(d.join("g.gif"), Target::Video(VideoKind::Gif { loops: 0 }), 64, 48)).unwrap().wait().unwrap();
        assert_eq!(&std::fs::read(&r.path).unwrap()[..6], b"GIF89a");

        let r = ExportJob::start(&p, 0, spec(d.join("seq"), Target::Video(VideoKind::PngSequence { alpha: false }), 64, 48)).unwrap().wait().unwrap();
        assert_eq!(r.frames, 2);
        assert!(d.join("seq/seq_00001.png").exists() && d.join("seq/seq_00002.png").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cancel_leaves_no_files() {
        let d = tmp("cancel");
        let mut p = project();
        p.animations[0].frames[0].hold = 100_000; // a very long video
        let job = ExportJob::start(&p, 0, spec(d.join("long.avi"), Target::Video(VideoKind::AviQuality(50)), 64, 48)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        job.cancel();
        let r = job.wait().unwrap();
        assert!(r.cancelled);
        assert!(!d.join("long.avi").exists() && !d.join("long.avi.part").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn bad_requests_fail_early_and_cleanly() {
        let d = tmp("bad");
        let p = project();
        assert!(ExportJob::start(&p, 5, spec(d.join("x.png"), Target::Photo { format: ImageFormat::Png, tick: 0 }, 10, 10)).is_err());
        assert!(ExportJob::start(&p, 0, spec(d.join("x.png"), Target::Photo { format: ImageFormat::Png, tick: 0 }, 0, 10)).is_err());
        assert!(ExportJob::start(&p, 0, spec(d.join("x.png"), Target::Photo { format: ImageFormat::Png, tick: 0 }, 100_000, 10)).is_err());
        // writing to a place that does not exist and cannot be created gives a readable error and no panic
        let r = ExportJob::start(&p, 0, spec(PathBuf::from("/proc/jank_nope/x.png"), Target::Photo { format: ImageFormat::Png, tick: 0 }, 10, 10)).unwrap().wait();
        assert!(r.is_err(), "{r:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn progress_reaches_one_and_result_taken_once() {
        let d = tmp("prog");
        let p = project();
        let mut job = ExportJob::start(&p, 0, spec(d.join("g.gif"), Target::Video(VideoKind::Gif { loops: 1 }), 32, 24)).unwrap();
        while !job.is_finished() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(job.progress(), 1.0);
        assert!(job.take_result().unwrap().is_ok());
        assert!(job.take_result().is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
