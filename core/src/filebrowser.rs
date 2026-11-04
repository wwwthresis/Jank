//! In-app file browser (no system dialogs): it behaves the same on every desktop and in every
//! session, and it can be styled to match the rest of the interface.
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Name,
    Size,
    Modified,
}

pub struct FileBrowser {
    pub dir: PathBuf,
    entries: Vec<Entry>,
    /// Allowed file extensions, lowercase, without the dot. Empty means show all files. Folders are always visible.
    pub filter: Vec<String>,
    pub show_hidden: bool,
    pub sort: Sort,
    pub descending: bool,
    /// Name search (case-insensitive substring).
    pub query: String,
    pub error: Option<String>,
}

/// Natural-order comparison: `file2` sorts before `file10`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut ia, mut ib) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ia.peek().copied(), ib.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                let mut nb = String::new();
                while let Some(c) = ia.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ia.next();
                }
                while let Some(c) = ib.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    ib.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ia.next();
                ib.next();
            }
        }
    }
}

impl FileBrowser {
    pub fn new(start: &Path) -> Self {
        let mut b = Self { dir: start.to_path_buf(), entries: vec![], filter: vec![], show_hidden: false, sort: Sort::Name, descending: false, query: String::new(), error: None };
        b.refresh();
        b
    }
    pub fn with_filter(mut self, exts: &[&str]) -> Self {
        self.filter = exts.iter().map(|e| e.to_ascii_lowercase()).collect();
        self
    }
    /// Re-reads the current folder. An unreadable folder does not break the browser: the previous
    /// path is kept and an error message is stored in `error` for display.
    pub fn refresh(&mut self) {
        match std::fs::read_dir(&self.dir) {
            Ok(rd) => {
                self.error = None;
                self.entries = rd
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let md = e.metadata().ok();
                        // symbolic links to folders are treated as folders
                        let is_dir = e.path().is_dir();
                        Entry { name: e.file_name().to_string_lossy().into_owned(), path: e.path(), is_dir, size: md.as_ref().map_or(0, |m| m.len()), modified: md.and_then(|m| m.modified().ok()) }
                    })
                    .collect();
            }
            Err(e) => {
                self.entries.clear();
                self.error = Some(format!("Could not open \"{}\": {e}", self.dir.display()));
            }
        }
    }
    /// Navigates to a folder. Returns `false` (and stays where it is) if it cannot be opened.
    pub fn set_dir(&mut self, path: &Path) -> bool {
        if !path.is_dir() || std::fs::read_dir(path).is_err() {
            self.error = Some(format!("Could not open \"{}\"", path.display()));
            return false;
        }
        self.dir = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.query.clear();
        self.refresh();
        true
    }
    pub fn up(&mut self) -> bool {
        match self.dir.parent().map(|p| p.to_path_buf()) {
            Some(p) => self.set_dir(&p),
            None => false,
        }
    }
    /// Activates a row by name: for a folder, enters it and returns `None`; for a file, returns its path.
    pub fn activate(&mut self, name: &str) -> Option<PathBuf> {
        let e = self.entries.iter().find(|e| e.name == name)?.clone();
        if e.is_dir {
            self.set_dir(&e.path);
            None
        } else {
            Some(e.path)
        }
    }
    /// Breadcrumbs: ("/", /), ("home", /home), ...
    pub fn breadcrumbs(&self) -> Vec<(String, PathBuf)> {
        let mut out = vec![];
        let mut acc = PathBuf::new();
        for c in self.dir.components() {
            acc.push(c);
            let name = match c {
                std::path::Component::RootDir => "/".to_string(),
                other => other.as_os_str().to_string_lossy().into_owned(),
            };
            out.push((name, acc.clone()));
        }
        out
    }
    /// Quick-jump places: the home folder and the user's standard folders (when they exist), plus the root.
    pub fn places() -> Vec<(String, PathBuf)> {
        let mut out = vec![];
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_dir()) {
            out.push(("Home".to_string(), home.clone()));
            for (label, dir) in [("Desktop", "Desktop"), ("Documents", "Documents"), ("Downloads", "Downloads"), ("Pictures", "Pictures"), ("Videos", "Videos")] {
                let p = home.join(dir);
                if p.is_dir() {
                    out.push((label.to_string(), p));
                }
            }
        }
        out.push(("Root".to_string(), PathBuf::from("/")));
        out
    }
    fn matches_filter(&self, e: &Entry) -> bool {
        if e.is_dir || self.filter.is_empty() {
            return true;
        }
        Path::new(&e.name).extension().and_then(|x| x.to_str()).is_some_and(|x| self.filter.iter().any(|f| f.eq_ignore_ascii_case(x)))
    }
    /// Visible rows: folders first, then the chosen sort order; hidden entries and entries rejected by the filter are removed.
    pub fn visible(&self) -> Vec<&Entry> {
        let q = self.query.to_lowercase();
        let mut v: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| (self.show_hidden || !e.name.starts_with('.')) && self.matches_filter(e) && (q.is_empty() || e.name.to_lowercase().contains(&q)))
            .collect();
        v.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| {
                let o = match self.sort {
                    Sort::Name => natural_cmp(&a.name, &b.name),
                    Sort::Size => a.size.cmp(&b.size).then_with(|| natural_cmp(&a.name, &b.name)),
                    Sort::Modified => a.modified.cmp(&b.modified).then_with(|| natural_cmp(&a.name, &b.name)),
                };
                if self.descending {
                    o.reverse()
                } else {
                    o
                }
            })
        });
        v
    }
}

/// A file name that will not overwrite anything: `name.ext`, then `name (2).ext`, `name (3).ext`, ...
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let make = |n: u32| {
        let base = if n <= 1 { stem.to_string() } else { format!("{stem} ({n})") };
        if ext.is_empty() {
            dir.join(base)
        } else {
            dir.join(format!("{base}.{ext}"))
        }
    };
    (1..10_000).map(make).find(|p| !p.exists()).unwrap_or_else(|| make(10_000))
}

/// Replaces characters that are invalid in common file systems with `_` and avoids empty names
/// (falls back to "Untitled"). The result is trimmed, stripped of leading/trailing dots, and limited to 120 characters.
pub fn sanitize_file_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "Untitled".into()
    } else {
        s.chars().take(120).collect()
    }
}

/// Human-readable size, e.g. `1.5 MB` (units: B, KB, MB, GB; 1024-based).
pub fn human_size(bytes: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", U[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jank_fb_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    #[test]
    fn natural_order() {
        let mut v = vec!["file10", "file2", "File1", "file02a", "a", "B"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a", "B", "File1", "file2", "file02a", "file10"]);
    }
    #[test]
    fn lists_dirs_first_filters_hides_and_searches() {
        let d = tmp("list");
        std::fs::create_dir(d.join("zeta")).unwrap();
        std::fs::create_dir(d.join(".hidden_dir")).unwrap();
        for f in ["b.obj", "a.GLB", "c.txt", ".secret.obj", "img10.png", "img2.png"] {
            std::fs::write(d.join(f), vec![0u8; 10]).unwrap();
        }
        let mut fb = FileBrowser::new(&d).with_filter(&["obj", "glb", "png"]);
        let names: Vec<_> = fb.visible().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["zeta", "a.GLB", "b.obj", "img2.png", "img10.png"]);
        fb.show_hidden = true;
        assert_eq!(fb.visible().len(), 7);
        fb.show_hidden = false;
        fb.query = "IMG".into();
        assert_eq!(fb.visible().len(), 2);
        fb.query.clear();
        fb.descending = true;
        assert_eq!(fb.visible()[0].name, "zeta", "folders stay first even with reverse sorting");
        assert_eq!(fb.visible()[1].name, "img10.png");
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn navigation_and_errors() {
        let d = tmp("nav");
        std::fs::create_dir_all(d.join("sub/inner")).unwrap();
        std::fs::write(d.join("sub/m.obj"), b"v").unwrap();
        let mut fb = FileBrowser::new(&d);
        assert_eq!(fb.activate("sub"), None);
        assert!(fb.dir.ends_with("sub"));
        assert_eq!(fb.activate("m.obj").unwrap().file_name().unwrap(), "m.obj");
        assert!(fb.up() && fb.dir == std::fs::canonicalize(&d).unwrap());
        let before = fb.dir.clone();
        assert!(!fb.set_dir(&d.join("no such folder")));
        assert_eq!(fb.dir, before, "stay in place after a failure");
        assert!(fb.error.is_some());
        let bc = fb.breadcrumbs();
        assert_eq!(bc[0].0, "/");
        assert_eq!(bc.last().unwrap().1, fb.dir);
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn unique_names_and_sanitizing() {
        let d = tmp("uniq");
        assert_eq!(unique_path(&d, "scene", "mp4").file_name().unwrap(), "scene.mp4");
        std::fs::write(d.join("scene.mp4"), b"x").unwrap();
        std::fs::write(d.join("scene (2).mp4"), b"x").unwrap();
        assert_eq!(unique_path(&d, "scene", "mp4").file_name().unwrap(), "scene (3).mp4");
        assert_eq!(sanitize_file_name("a/b:c*?.png"), "a_b_c__.png");
        assert_eq!(sanitize_file_name("  ..  "), "Untitled");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(12), "12 B");
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn unreadable_start_dir_does_not_panic() {
        let fb = FileBrowser::new(Path::new("/definitely/not/here"));
        assert!(fb.error.is_some());
        assert!(fb.visible().is_empty());
    }
}
