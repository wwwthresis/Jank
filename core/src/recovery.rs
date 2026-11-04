//! Autosave and crash recovery.
//!
//! While a project has unsaved changes the application writes a snapshot to the recovery folder
//! at regular intervals. A normal exit, a successful save or "New project" removes the snapshot.
//! If a snapshot is still there at the next start and the process that wrote it is gone, the
//! previous session ended abnormally and the user is offered the snapshot.
//!
//! Layout of the folder (`<state dir>/recovery/`):
//!
//! * `autosave.jank` - the snapshot in the normal project format
//! * `autosave.meta` - three lines: writer process id, unix time, original project path (may be empty)

use crate::editor::Editor;
use crate::project::Project;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Folder for state that is not configuration: `JANK_STATE_DIR`, `XDG_STATE_HOME`, `~/.local/state`.
pub fn state_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("JANK_STATE_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(x) = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(x).join("jank"));
    }
    if let Some(h) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(h).join(".local").join("state").join("jank"));
    }
    std::env::var_os("LOCALAPPDATA").map(|a| PathBuf::from(a).join("Jank"))
}

pub fn recovery_dir() -> Option<PathBuf> {
    state_dir().map(|d| d.join("recovery"))
}

/// What a leftover snapshot says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub file: PathBuf,
    pub writer_pid: u32,
    pub unix_time: u64,
    /// Where the project lived on disk, if it had been saved before.
    pub original: Option<PathBuf>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// True when a process with this id is running. On systems without `/proc` the answer is `false`,
/// which errs on the side of offering recovery.
pub fn process_alive(pid: u32) -> bool {
    pid == std::process::id() || Path::new(&format!("/proc/{pid}")).exists()
}

/// Writes a snapshot of the editor's project. The editor itself is not modified (its path and
/// "unsaved" flag stay as they are).
pub fn write_snapshot(dir: &Path, ed: &Editor) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let file = dir.join("autosave.jank");
    ed.project.save(&file)?;
    let meta = format!("{}\n{}\n{}\n", std::process::id(), now(), ed.path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default());
    let tmp = dir.join("autosave.meta.part");
    fs::write(&tmp, meta)?;
    fs::rename(&tmp, dir.join("autosave.meta"))
}

/// Removes the snapshot (after a save, a clean exit, or when the user declines recovery).
pub fn clear(dir: &Path) {
    let _ = fs::remove_file(dir.join("autosave.jank"));
    let _ = fs::remove_file(dir.join("autosave.meta"));
    let _ = fs::remove_file(dir.join("autosave.meta.part"));
}

/// Looks for a snapshot left by a session that is no longer running.
pub fn find_snapshot(dir: &Path) -> Option<Snapshot> {
    let file = dir.join("autosave.jank");
    if !file.is_file() {
        return None;
    }
    let meta = fs::read_to_string(dir.join("autosave.meta")).unwrap_or_default();
    let mut it = meta.lines();
    let writer_pid = it.next().and_then(|l| l.trim().parse().ok()).unwrap_or(0);
    let unix_time = it.next().and_then(|l| l.trim().parse().ok()).unwrap_or(0);
    let original = it.next().map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from);
    if writer_pid != 0 && process_alive(writer_pid) {
        return None; // another running instance owns it
    }
    Some(Snapshot { file, writer_pid, unix_time, original })
}

/// Loads a snapshot into a new editor marked as having unsaved changes. The original path is
/// restored so "Save" overwrites the real project, not the snapshot.
pub fn restore(s: &Snapshot) -> Result<Editor, String> {
    let project = Project::load(&s.file).map_err(|e| format!("The recovery file is damaged: {e}"))?;
    let mut ed = Editor::new();
    ed.project = project;
    ed.path = s.original.clone();
    ed.selected_object = ed.project.scene.objects.first().map(|o| o.id);
    ed.frame_all();
    ed.dirty = true;
    Ok(ed)
}

/// Human description such as "3 minutes ago" for the recovery prompt.
pub fn age_text(unix_time: u64, now_unix: u64) -> String {
    let d = now_unix.saturating_sub(unix_time);
    match d {
        0..=59 => "less than a minute ago".to_string(),
        60..=3599 => {
            let m = d / 60;
            format!("{m} minute{} ago", if m == 1 { "" } else { "s" })
        }
        3600..=86399 => {
            let h = d / 3600;
            format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
        }
        _ => {
            let n = d / 86400;
            format!("{n} day{} ago", if n == 1 { "" } else { "s" })
        }
    }
}

pub fn unix_now() -> u64 {
    now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Primitive;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jank-recovery-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn editor_with_cube() -> Editor {
        let mut ed = Editor::new();
        ed.add_primitive(Primitive::Cube);
        ed
    }

    #[test]
    fn snapshot_of_a_live_session_is_not_offered() {
        let dir = scratch("live");
        let mut ed = editor_with_cube();
        ed.path = Some(PathBuf::from("/work/thing.jank"));
        write_snapshot(&dir, &ed).unwrap();
        assert!(dir.join("autosave.jank").is_file());
        assert!(find_snapshot(&dir).is_none(), "our own pid is alive, so another instance owns nothing here");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_of_a_dead_session_is_offered_and_restores_the_work() {
        let dir = scratch("dead");
        let mut ed = editor_with_cube();
        ed.path = Some(PathBuf::from("/work/thing.jank"));
        write_snapshot(&dir, &ed).unwrap();
        // pretend the writer was a process that no longer exists
        let meta = fs::read_to_string(dir.join("autosave.meta")).unwrap();
        let rest: Vec<&str> = meta.lines().skip(1).collect();
        fs::write(dir.join("autosave.meta"), format!("4000000000\n{}\n", rest.join("\n"))).unwrap();
        let snap = find_snapshot(&dir).expect("a dead writer's snapshot must be offered");
        assert_eq!(snap.original.as_deref(), Some(Path::new("/work/thing.jank")));
        let back = restore(&snap).unwrap();
        assert_eq!(back.project.scene.objects.len(), 1);
        assert!(back.dirty);
        assert_eq!(back.path.as_deref(), Some(Path::new("/work/thing.jank")));
        clear(&dir);
        assert!(find_snapshot(&dir).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writing_a_snapshot_leaves_the_editor_untouched() {
        let dir = scratch("untouched");
        let mut ed = editor_with_cube();
        ed.dirty = true;
        write_snapshot(&dir, &ed).unwrap();
        assert!(ed.dirty);
        assert!(ed.path.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn damaged_snapshot_gives_an_error_not_a_panic() {
        let dir = scratch("bad");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("autosave.jank"), b"JANKnot really").unwrap();
        fs::write(dir.join("autosave.meta"), "4000000000\n1\n\n").unwrap();
        let snap = find_snapshot(&dir).unwrap();
        assert!(snap.original.is_none());
        assert!(restore(&snap).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn age_text_is_readable() {
        assert_eq!(age_text(100, 130), "less than a minute ago");
        assert_eq!(age_text(0, 60), "1 minute ago");
        assert_eq!(age_text(0, 7200), "2 hours ago");
        assert_eq!(age_text(0, 86400 * 3), "3 days ago");
    }
}
