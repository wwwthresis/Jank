//! Discord Rich Presence ("Playing Jank!" with what you are doing) over Discord's local IPC
//! socket. No SDK, no network access and no token: the desktop Discord client listens on a local
//! socket (`discord-ipc-0..9` in the runtime/temp folder, a named pipe on Windows), and a program
//! only needs the public *Application ID* of its Discord application to set its activity.
//!
//! Protocol: frames of `u32 opcode | u32 length | JSON` (little-endian). Opcode 0 is the
//! handshake (`{"v":1,"client_id":"…"}`), opcode 1 carries commands (`SET_ACTIVITY`) and their
//! replies, 2 closes, 3/4 are ping/pong.
//!
//! [`Presence`] runs the connection on its own thread, reconnects when Discord starts later, sends
//! at most one update every few seconds (Discord allows 5 per 20 s) and clears the activity when
//! it is dropped.
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Application ID of the "Jank!" application in the Discord Developer Portal (public, not a
/// secret); `JANK_DISCORD_CLIENT_ID` (at build time or at run time) overrides it.
pub const APPLICATION_ID: &str = "1567497552780075190";

/// The button on the profile card (other people see it; Discord hides it from yourself).
pub const GITHUB_BUTTON: (&str, &str) = ("GitHub", "https://github.com/wwwthresis/Jank/");

/// Art asset keys uploaded to the Discord application (Rich Presence → Art Assets).
pub mod assets {
    pub const LOGO: &str = "jank_logo";
    pub const MODEL: &str = "mode_model";
    pub const EDIT: &str = "mode_edit";
    pub const TEXTURE: &str = "mode_texture";
    pub const POSE: &str = "mode_pose";
    pub const ANIMATE: &str = "mode_animate";
    pub const RENDER: &str = "mode_render";
    pub const EXPORT: &str = "mode_export";
    pub const IDLE: &str = "mode_idle";
    pub const ALL: [&str; 9] = [LOGO, MODEL, EDIT, TEXTURE, POSE, ANIMATE, RENDER, EXPORT, IDLE];
}

/// The Application ID to use: the run-time variable, then the build-time one, then
/// [`APPLICATION_ID`]. Only a string of digits (a Discord snowflake) counts.
pub fn application_id() -> Option<String> {
    let valid = |s: &str| (17..=20).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    let run = std::env::var("JANK_DISCORD_CLIENT_ID").ok();
    let found = [run.as_deref(), option_env!("JANK_DISCORD_CLIENT_ID"), Some(APPLICATION_ID)].into_iter().flatten().map(str::trim).find(|s| valid(s)).map(str::to_string);
    found
}

/// What Discord shows on the profile card.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Activity {
    /// First line under the application name.
    pub details: String,
    /// Second line.
    pub state: String,
    pub large_image: String,
    pub large_text: String,
    pub small_image: String,
    pub small_text: String,
    /// Unix time in seconds; Discord counts up "00:42 elapsed" from it.
    pub start: Option<u64>,
    /// Up to two (label, https URL) buttons; shown to other people only.
    pub buttons: Vec<(String, String)>,
}

/// Discord rejects text fields shorter than 2 or longer than 128 bytes.
fn field(s: &str) -> String {
    let mut s = s.trim().to_string();
    if s.len() > 128 {
        let mut cut = 127 - "…".len() + 1;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    if s.chars().count() == 1 {
        s.push(' ');
    }
    s
}

/// JSON string literal.
pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

impl Activity {
    pub fn to_json(&self) -> String {
        let mut parts = vec![];
        for (k, v) in [("details", &self.details), ("state", &self.state)] {
            let v = field(v);
            if !v.is_empty() {
                parts.push(format!("{}:{}", quote(k), quote(&v)));
            }
        }
        if let Some(t) = self.start {
            parts.push(format!("\"timestamps\":{{\"start\":{t}}}"));
        }
        let mut a = vec![];
        for (k, v) in [("large_image", &self.large_image), ("large_text", &self.large_text), ("small_image", &self.small_image), ("small_text", &self.small_text)] {
            let v = if k.ends_with("_text") { field(v) } else { v.trim().to_string() };
            if !v.is_empty() {
                a.push(format!("{}:{}", quote(k), quote(&v)));
            }
        }
        if !a.is_empty() {
            parts.push(format!("\"assets\":{{{}}}", a.join(",")));
        }
        let b: Vec<String> = self
            .buttons
            .iter()
            .filter(|(l, u)| !l.trim().is_empty() && (u.starts_with("https://") || u.starts_with("http://")))
            .take(2)
            .map(|(l, u)| {
                let mut l = l.trim().to_string();
                if l.len() > 32 {
                    let mut c = 32;
                    while !l.is_char_boundary(c) {
                        c -= 1;
                    }
                    l.truncate(c);
                }
                format!("{{\"label\":{},\"url\":{}}}", quote(&l), quote(u))
            })
            .collect();
        if !b.is_empty() {
            parts.push(format!("\"buttons\":[{}]", b.join(",")));
        }
        parts.push("\"instance\":false".into());
        format!("{{{}}}", parts.join(","))
    }
}

pub fn handshake_json(client_id: &str) -> String {
    format!("{{\"v\":1,\"client_id\":{}}}", quote(client_id))
}

/// `SET_ACTIVITY` command; `None` clears the activity.
pub fn set_activity_json(pid: u32, activity: Option<&Activity>, nonce: u64) -> String {
    let act = activity.map_or("null".to_string(), Activity::to_json);
    format!("{{\"cmd\":\"SET_ACTIVITY\",\"args\":{{\"pid\":{pid},\"activity\":{act}}},\"nonce\":\"{nonce}\"}}")
}

pub const OP_HANDSHAKE: u32 = 0;
pub const OP_FRAME: u32 = 1;
pub const OP_CLOSE: u32 = 2;
pub const OP_PING: u32 = 3;
pub const OP_PONG: u32 = 4;

pub fn encode_frame(op: u32, payload: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + payload.len());
    v.extend_from_slice(&op.to_le_bytes());
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    v.extend_from_slice(payload.as_bytes());
    v
}

pub fn read_frame<R: Read + ?Sized>(r: &mut R) -> io::Result<(u32, String)> {
    let mut h = [0u8; 8];
    r.read_exact(&mut h)?;
    let op = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
    let len = u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as usize;
    if len > 1 << 20 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut b = vec![0u8; len];
    r.read_exact(&mut b)?;
    Ok((op, String::from_utf8_lossy(&b).into_owned()))
}

/// Where Discord (and Discord running as Flatpak or Snap, and Vesktop) listens.
pub fn socket_candidates() -> Vec<PathBuf> {
    let mut out = vec![];
    if cfg!(windows) {
        for i in 0..10 {
            out.push(PathBuf::from(format!(r"\\.\pipe\discord-ipc-{i}")));
        }
        return out;
    }
    let mut dirs: Vec<PathBuf> = vec![];
    for var in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            let d = PathBuf::from(v);
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
    }
    dirs.push(PathBuf::from("/tmp"));
    for d in dirs {
        for sub in ["", "app/com.discordapp.Discord", "app/com.discordapp.DiscordCanary", "snap.discord", "snap.discord-canary", ".flatpak/dev.vencord.Vesktop/xdg-run", "app/dev.vencord.Vesktop"] {
            for i in 0..10 {
                out.push(d.join(sub).join(format!("discord-ipc-{i}")));
            }
        }
    }
    out
}

/// Connection state, for the Settings page.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Looking for the Discord app (not running, or not installed).
    Searching,
    /// Connected; the Discord user name when Discord told it.
    Connected(String),
    /// Discord refused the handshake or an update (usually a wrong Application ID).
    Rejected(String),
    Stopped,
}

trait Conn: Read + Write + Send {}
impl<T: Read + Write + Send> Conn for T {}

fn open(path: &PathBuf) -> io::Result<Box<dyn Conn>> {
    #[cfg(unix)]
    {
        let s = std::os::unix::net::UnixStream::connect(path)?;
        s.set_read_timeout(Some(Duration::from_secs(4)))?;
        s.set_write_timeout(Some(Duration::from_secs(4)))?;
        Ok(Box::new(s))
    }
    #[cfg(not(unix))]
    {
        let f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Box::new(f))
    }
}

/// Reads frames until the reply to a command (answering pings on the way).
fn read_reply(c: &mut dyn Conn) -> io::Result<(u32, String)> {
    loop {
        let (op, body) = read_frame(c)?;
        match op {
            OP_PING => {
                c.write_all(&encode_frame(OP_PONG, &body))?;
            }
            OP_CLOSE => return Err(io::Error::new(io::ErrorKind::ConnectionAborted, body)),
            _ => return Ok((op, body)),
        }
    }
}

/// Error text in a Discord reply (`"evt":"ERROR"` or a close frame), if any.
fn reply_error(body: &str) -> Option<String> {
    let j = crate::json::Json::parse(body).ok()?;
    if j.get("evt").and_then(|e| e.as_str()) == Some("ERROR") {
        return Some(j.get("data").and_then(|d| d.get("message")).and_then(|m| m.as_str()).unwrap_or("error").to_string());
    }
    None
}

fn user_name(ready: &str) -> String {
    let Ok(j) = crate::json::Json::parse(ready) else { return String::new() };
    let u = j.get("data").and_then(|d| d.get("user"));
    let g = u.and_then(|u| u.get("global_name")).and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    g.or_else(|| u.and_then(|u| u.get("username")).and_then(|v| v.as_str())).unwrap_or("").to_string()
}

enum Msg {
    Set(Option<Activity>),
    Stop,
}

/// A running Rich Presence connection.
pub struct Presence {
    tx: Sender<Msg>,
    status: Arc<Mutex<Status>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Timing of the connection thread (shorter in tests).
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// Minimum time between two activity updates.
    pub min_update: Duration,
    /// Time between two attempts to find Discord.
    pub retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { min_update: Duration::from_secs(4), retry: Duration::from_secs(10) }
    }
}

impl Presence {
    pub fn start(client_id: String) -> Presence {
        Self::start_with(client_id, socket_candidates(), Timing::default())
    }

    pub fn start_with(client_id: String, candidates: Vec<PathBuf>, timing: Timing) -> Presence {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::Searching));
        let st = status.clone();
        let thread = std::thread::Builder::new().name("discord-rpc".into()).spawn(move || run(client_id, candidates, timing, rx, st)).ok();
        Presence { tx, status, thread }
    }

    /// The activity to show (`None` clears it). Only the newest one is sent.
    pub fn set(&self, activity: Option<Activity>) {
        let _ = self.tx.send(Msg::Set(activity));
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or(Status::Stopped)
    }

    /// Clears the activity and closes the connection; waits at most about a second.
    pub fn stop(mut self) {
        self.shutdown(Duration::from_millis(1200));
    }

    fn shutdown(&mut self, wait: Duration) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.take() {
            let until = Instant::now() + wait;
            while !t.is_finished() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            if t.is_finished() {
                let _ = t.join();
            }
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.shutdown(Duration::from_millis(600));
    }
}

fn run(client_id: String, candidates: Vec<PathBuf>, timing: Timing, rx: Receiver<Msg>, status: Arc<Mutex<Status>>) {
    let set_status = |s: Status| {
        if let Ok(mut g) = status.lock() {
            *g = s;
        }
    };
    let pid = std::process::id();
    let mut conn: Option<Box<dyn Conn>> = None;
    let mut wanted: Option<Activity> = None;
    // what Discord shows now (None = nothing sent yet or cleared)
    let mut shown: Option<Option<Activity>> = None;
    let mut last_send: Option<Instant> = None;
    let mut next_try = Instant::now();
    let mut nonce = 0u64;
    let mut rejected = false;
    loop {
        // wait for news, but wake up for retries and for rate-limited updates
        let msg = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Msg::Stop),
        };
        let mut stop = false;
        for m in msg.into_iter().chain(rx.try_iter()) {
            match m {
                Msg::Set(a) => wanted = a,
                Msg::Stop => stop = true,
            }
        }
        if stop {
            if let Some(c) = conn.as_mut() {
                nonce += 1;
                let _ = c.write_all(&encode_frame(OP_FRAME, &set_activity_json(pid, None, nonce)));
                let _ = c.flush();
                let _ = read_reply(c.as_mut());
                let _ = c.write_all(&encode_frame(OP_CLOSE, "{}"));
            }
            set_status(Status::Stopped);
            return;
        }
        if conn.is_none() && !rejected && Instant::now() >= next_try {
            next_try = Instant::now() + timing.retry;
            for path in &candidates {
                let Ok(mut c) = open(path) else { continue };
                if c.write_all(&encode_frame(OP_HANDSHAKE, &handshake_json(&client_id))).is_err() {
                    continue;
                }
                match read_reply(c.as_mut()) {
                    Ok((OP_FRAME, body)) if reply_error(&body).is_none() => {
                        set_status(Status::Connected(user_name(&body)));
                        conn = Some(c);
                        shown = None;
                        break;
                    }
                    Ok((_, body)) => {
                        let why = reply_error(&body).unwrap_or(body);
                        set_status(Status::Rejected(why));
                        rejected = true;
                        break;
                    }
                    Err(e) if e.kind() == io::ErrorKind::ConnectionAborted => {
                        // Discord closes the socket right away for an unknown Application ID
                        let body = e.to_string();
                        let why = crate::json::Json::parse(&body).ok().and_then(|j| j.get("message").and_then(|m| m.as_str()).map(str::to_string)).unwrap_or_else(|| "Discord closed the connection".into());
                        set_status(Status::Rejected(why));
                        rejected = true;
                        break;
                    }
                    Err(_) => continue,
                }
            }
            if conn.is_none() && !rejected {
                set_status(Status::Searching);
            }
        }
        let Some(c) = conn.as_mut() else { continue };
        if shown.as_ref() == Some(&wanted) {
            continue;
        }
        if last_send.is_some_and(|t| t.elapsed() < timing.min_update) {
            continue;
        }
        nonce += 1;
        let frame = encode_frame(OP_FRAME, &set_activity_json(pid, wanted.as_ref(), nonce));
        last_send = Some(Instant::now());
        let ok = c.write_all(&frame).and_then(|_| c.flush()).and_then(|_| read_reply(c.as_mut()));
        match ok {
            Ok((_, body)) => {
                if let Some(e) = reply_error(&body) {
                    // keep the connection; do not resend the same rejected activity
                    set_status(Status::Rejected(e));
                } else if let Ok(g) = status.lock() {
                    if !matches!(*g, Status::Connected(_)) {
                        drop(g);
                        set_status(Status::Connected(String::new()));
                    }
                }
                shown = Some(wanted.clone());
            }
            Err(_) => {
                // Discord was closed: look for it again soon
                conn = None;
                shown = None;
                set_status(Status::Searching);
                next_try = Instant::now() + timing.retry.min(Duration::from_secs(3));
            }
        }
    }
}

// ------------------------------------------------------------------ what Jank! shows

/// What the user is doing, in the words of the profile card.
#[derive(Clone, Debug, PartialEq)]
pub enum Doing {
    Modelling { objects: usize, tris: usize },
    EditingMesh { verts: usize, faces: usize },
    Painting { width: u32, height: u32, on_model: bool },
    Posing { bones: usize },
    Animating { frame: usize, frames: usize, playing: bool, fps: u32 },
    Rendering { width: u32, height: u32 },
    Exporting { video: bool, percent: u32 },
}

/// Everything the activity is made from, with the privacy choices of Settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub project: String,
    pub show_project: bool,
    pub show_activity: bool,
    pub doing: Doing,
    /// No input for a while.
    pub idle: bool,
    /// Unix seconds since the project was opened (shown as elapsed time), if wanted.
    pub since: Option<u64>,
    /// e.g. `0.2.0-alpha`.
    pub version: String,
    pub buttons: Vec<(String, String)>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    let s = n.to_string();
    // thousands separators: 12,345
    let mut g = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            g.push(',');
        }
        g.push(ch);
    }
    format!("{g} {}", if n == 1 { one } else { many })
}

/// Turns the editor state into the profile card.
pub fn activity(i: &Info) -> Activity {
    let name = i.project.trim();
    let details = if !i.show_project {
        "Working on a project".to_string()
    } else if name.is_empty() || name.eq_ignore_ascii_case("untitled") {
        "Starting a new project".to_string()
    } else {
        format!("Working on \u{201c}{name}\u{201d}")
    };
    let (state, small, small_text) = if i.idle {
        ("Taking a break".to_string(), assets::IDLE, "Idle")
    } else if !i.show_activity {
        ("Making low-poly art".to_string(), "", "")
    } else {
        match &i.doing {
            Doing::Modelling { objects, tris } => (format!("Modelling \u{00b7} {}, {}", plural(*objects, "object", "objects"), plural(*tris, "triangle", "triangles")), assets::MODEL, "Model mode"),
            Doing::EditingMesh { verts, faces } => (format!("Editing polygons \u{00b7} {}, {}", plural(*verts, "vertex", "vertices"), plural(*faces, "face", "faces")), assets::EDIT, "Edit mode"),
            Doing::Painting { width, height, on_model } => (format!("{} \u{00b7} {width}\u{00d7}{height} px", if *on_model { "Painting on the model" } else { "Painting a texture" }), assets::TEXTURE, "Texture mode"),
            Doing::Posing { bones } => (format!("Posing \u{00b7} {}", plural(*bones, "bone", "bones")), assets::POSE, "Pose mode"),
            Doing::Animating { frames, playing: true, fps, .. } => (format!("Watching the animation \u{00b7} {} at {fps} fps", plural(*frames, "frame", "frames")), assets::ANIMATE, "Animate mode"),
            Doing::Animating { frame, frames, .. } => (format!("Animating \u{00b7} frame {} of {frames}", frame + 1), assets::ANIMATE, "Animate mode"),
            Doing::Rendering { width, height } => (format!("Setting up the render \u{00b7} {width}\u{00d7}{height}"), assets::RENDER, "Render mode"),
            Doing::Exporting { video, percent } => (format!("Exporting a {} \u{00b7} {percent}%", if *video { "video" } else { "picture" }), assets::EXPORT, "Exporting"),
        }
    };
    Activity {
        details,
        state,
        large_image: assets::LOGO.into(),
        large_text: format!("Jank! {} \u{2014} low-poly PSX studio", i.version),
        small_image: small.into(),
        small_text: small_text.into(),
        start: i.since,
        buttons: i.buttons.clone(),
    }
}

/// Current Unix time in seconds.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(doing: Doing) -> Info {
        Info { project: "Butterfly demo".into(), show_project: true, show_activity: true, doing, idle: false, since: Some(1_700_000_000), version: "0.2.0-alpha".into(), buttons: vec![] }
    }

    #[test]
    fn frames_round_trip() {
        let f = encode_frame(OP_FRAME, "{\"a\":1}");
        assert_eq!(&f[..4], &1u32.to_le_bytes());
        assert_eq!(&f[4..8], &7u32.to_le_bytes());
        let (op, body) = read_frame(&mut &f[..]).unwrap();
        assert_eq!((op, body.as_str()), (1, "{\"a\":1}"));
        let mut big = encode_frame(1, "");
        big[4..8].copy_from_slice(&(2u32 << 20).to_le_bytes());
        assert!(read_frame(&mut &big[..]).is_err());
    }

    #[test]
    fn activity_json_is_valid_and_respects_limits() {
        let mut a = activity(&info(Doing::Modelling { objects: 12, tris: 12345 }));
        a.details = format!("Working on \u{201c}{}\u{201d}", "é".repeat(100));
        a.buttons = vec![("Get Jank!".into(), "https://example.org".into()), ("bad".into(), "ftp://x".into())];
        let j = set_activity_json(42, Some(&a), 7);
        let p = crate::json::Json::parse(&j).expect("valid JSON");
        assert_eq!(p.get("cmd").and_then(|c| c.as_str()), Some("SET_ACTIVITY"));
        let act = p.get("args").and_then(|a| a.get("activity")).unwrap();
        assert!(act.get("details").and_then(|d| d.as_str()).unwrap().len() <= 128);
        assert_eq!(act.get("state").and_then(|d| d.as_str()), Some("Modelling \u{00b7} 12 objects, 12,345 triangles"));
        assert_eq!(act.get("timestamps").and_then(|t| t.get("start")).and_then(|s| s.as_f64()), Some(1_700_000_000.0));
        assert_eq!(act.get("assets").and_then(|t| t.get("small_image")).and_then(|s| s.as_str()), Some(assets::MODEL));
        assert_eq!(act.get("buttons").map(|b| b.len()), Some(1));
        let clear = set_activity_json(42, None, 8);
        assert!(crate::json::Json::parse(&clear).is_ok() && clear.contains("\"activity\":null"));
        assert_eq!(quote("a\"b\\c\n\u{1}"), "\"a\\\"b\\\\c\\n\\u0001\"");
    }

    #[test]
    fn privacy_choices_hide_the_project_and_the_details() {
        let mut i = info(Doing::EditingMesh { verts: 1, faces: 76 });
        let a = activity(&i);
        assert_eq!(a.details, "Working on \u{201c}Butterfly demo\u{201d}");
        assert_eq!(a.state, "Editing polygons \u{00b7} 1 vertex, 76 faces");
        assert_eq!(a.small_image, assets::EDIT);
        i.buttons = vec![(GITHUB_BUTTON.0.into(), GITHUB_BUTTON.1.into())];
        assert!(activity(&i).to_json().contains("\"buttons\":[{\"label\":\"GitHub\",\"url\":\"https://github.com/wwwthresis/Jank/\"}]"));
        i.show_project = false;
        i.show_activity = false;
        let a = activity(&i);
        assert!(!a.details.contains("Butterfly") && !a.state.contains("76"));
        assert!(a.small_image.is_empty());
        i.idle = true;
        assert_eq!(activity(&i).small_image, assets::IDLE);
        i.show_project = true;
        i.project = "Untitled".into();
        assert_eq!(activity(&i).details, "Starting a new project");
        let anim = activity(&info(Doing::Animating { frame: 2, frames: 12, playing: false, fps: 12 }));
        assert_eq!(anim.state, "Animating \u{00b7} frame 3 of 12");
    }

    #[test]
    fn asset_keys_are_valid_discord_keys() {
        for k in assets::ALL {
            assert!(k.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'), "{k}");
        }
    }

    #[test]
    fn application_id_must_be_a_snowflake() {
        // the built-in id is either empty (not configured yet) or a valid snowflake
        assert!(APPLICATION_ID.is_empty() || APPLICATION_ID.bytes().all(|b| b.is_ascii_digit()));
        assert!((17..=20).contains(&APPLICATION_ID.len()));
        assert!(GITHUB_BUTTON.1.starts_with("https://") && GITHUB_BUTTON.0.len() <= 32);
    }

    #[cfg(unix)]
    #[test]
    fn talks_to_a_fake_discord() {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!("jank-rpc-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("discord-ipc-0");
        let _ = std::fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock).unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let (op, hs) = read_frame(&mut s).unwrap();
            assert_eq!(op, OP_HANDSHAKE);
            assert!(hs.contains("\"client_id\":\"123456789012345678\""));
            s.write_all(&encode_frame(OP_FRAME, r#"{"cmd":"DISPATCH","evt":"READY","data":{"v":1,"user":{"username":"nik","global_name":"Nikolay"}}}"#)).unwrap();
            let mut got = vec![];
            loop {
                let Ok((op, body)) = read_frame(&mut s) else { break };
                if op == OP_CLOSE {
                    break;
                }
                s.write_all(&encode_frame(OP_FRAME, r#"{"cmd":"SET_ACTIVITY","data":{},"evt":null,"nonce":"1"}"#)).unwrap();
                got.push(body);
            }
            got
        });
        let p = Presence::start_with("123456789012345678".into(), vec![dir.join("discord-ipc-9"), sock.clone()], Timing { min_update: Duration::from_millis(50), retry: Duration::from_millis(50) });
        p.set(Some(activity(&info(Doing::Posing { bones: 5 }))));
        let until = Instant::now() + Duration::from_secs(5);
        while !matches!(p.status(), Status::Connected(_)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(p.status(), Status::Connected("Nikolay".into()));
        std::thread::sleep(Duration::from_millis(400));
        p.stop();
        let got = server.join().unwrap();
        assert!(got.len() >= 2, "{got:?}");
        assert!(got[0].contains("Posing \u{00b7} 5 bones") && got[0].contains("\"pid\":"));
        assert!(got.last().unwrap().contains("\"activity\":null"), "the activity is cleared at the end");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn searching_when_discord_is_not_running() {
        let p = Presence::start_with("123456789012345678".into(), vec![std::env::temp_dir().join("jank-no-discord-here/discord-ipc-0")], Timing::default());
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(p.status(), Status::Searching);
        p.stop();
    }
}
