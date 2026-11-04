//! Real start-up work, done on a background thread while the splash screen shows true progress.
//!
//! Every step below is something the program needs before the first editor frame. The splash
//! shows a short introduction first and then follows these steps (see `splash.rs`); the loading itself is never delayed.
use psxcore::editor::Editor;
use psxcore::raster::Renderer;
use psxcore::recovery::{self, Snapshot};
use psxcore::splash::Season;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// Everything the loader prepared; moved into the `App` on the UI thread.
pub struct Loaded {
    pub editor: Editor,
    pub rend: Renderer,
    /// Name of the project given on the command line, when it opened successfully.
    pub opened: Option<String>,
    /// A command-line path that is not a `.jank` project (imported by the UI thread) or an open error.
    pub import_path: Option<String>,
    pub error: Option<String>,
    pub snapshot: Option<Snapshot>,
    pub crash_reports: usize,
}

pub struct SplashImage {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

enum Msg {
    Image(SplashImage),
    Progress(f32, &'static str),
    Done(Box<Loaded>),
}

pub struct Boot {
    rx: Receiver<Msg>,
    pub progress: f32,
    pub label: &'static str,
    pub image: Option<SplashImage>,
    pub done: Option<Box<Loaded>>,
    /// Wall-clock seconds the real loading took (shown in the About page / logs).
    started: std::time::Instant,
    pub took: Option<f32>,
}

/// The ordered start-up steps (used for the progress fraction).
const STEPS: f32 = 6.0;

impl Boot {
    pub fn spawn(season: Season, seed: u64, path: Option<String>) -> Boot {
        let (tx, rx) = channel();
        let file = season.file_name(seed);
        std::thread::Builder::new()
            .name("jank-startup".into())
            .spawn(move || {
                let step = |n: f32, label: &'static str| {
                    let _ = tx.send(Msg::Progress(n / STEPS, label));
                };
                step(0.0, "Decoding splash picture");
                if let Some(bytes) = crate::splash::bytes(file) {
                    if let Ok(img) = psxcore::decode::image::decode_image(file, bytes) {
                        let _ = tx.send(Msg::Image(SplashImage { w: img.w as usize, h: img.h as usize, rgba: img.rgba }));
                    }
                }
                step(1.0, "Preparing the editor");
                let mut editor = Editor::new();
                step(2.0, "Preparing the renderer");
                let mut rend = Renderer::new(320, 240);
                // one real frame: touches every buffer once, so the first editor frame is not the slow one
                editor.render_viewport(&mut rend);
                step(3.0, "Opening project");
                let (mut opened, mut import_path, mut error) = (None, None, None);
                if let Some(p) = path {
                    let pp = std::path::Path::new(&p);
                    if pp.extension().and_then(|e| e.to_str()).map_or(false, |e| e.eq_ignore_ascii_case("jank")) {
                        match Editor::open(pp) {
                            Ok(ed) => {
                                opened = Some(ed.project.name.clone());
                                editor = ed;
                            }
                            Err(e) => error = Some(e),
                        }
                    } else {
                        import_path = Some(p);
                    }
                }
                step(4.0, "Looking for unsaved work");
                let snapshot = if opened.is_none() && import_path.is_none() { recovery::recovery_dir().and_then(|d| recovery::find_snapshot(&d)) } else { None };
                step(5.0, "Checking crash reports");
                let crash_reports = crate::crash::pending_reports();
                let _ = tx.send(Msg::Progress(1.0, "Ready"));
                let _ = tx.send(Msg::Done(Box::new(Loaded { editor, rend, opened, import_path, error, snapshot, crash_reports })));
            })
            .expect("spawn start-up thread");
        Boot { rx, progress: 0.0, label: "Starting", image: None, done: None, started: std::time::Instant::now(), took: None }
    }

    /// Takes everything the thread has sent so far. Returns true while loading is still running.
    pub fn poll(&mut self) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(Msg::Image(i)) => self.image = Some(i),
                Ok(Msg::Progress(p, l)) => {
                    self.progress = p;
                    self.label = l;
                }
                Ok(Msg::Done(l)) => {
                    self.done = Some(l);
                    self.took = Some(self.started.elapsed().as_secs_f32());
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.done.is_none() {
                        // the loader died (panic): start with an empty editor instead of hanging
                        self.done = Some(Box::new(Loaded { editor: Editor::new(), rend: Renderer::new(320, 240), opened: None, import_path: None, error: Some("Start-up loader stopped unexpectedly".into()), snapshot: None, crash_reports: 0 }));
                    }
                    break;
                }
            }
        }
        self.done.is_none()
    }
}
