//! Crash reports. A panic anywhere in the program writes a small text file to the state folder
//! (`crashes/crash-<unix time>.txt`) containing the version, platform, location and message, so a
//! user can attach it to a bug report. Recovery of the work itself is handled by `psxcore::recovery`.

use std::path::PathBuf;

pub fn crash_dir() -> Option<PathBuf> {
    psxcore::recovery::state_dir().map(|d| d.join("crashes"))
}

/// Installs the panic hook. The default hook (message on stderr) still runs afterwards.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = crash_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_else(|| "unknown location".into());
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "no message".to_string()
            };
            let text = format!("{}\npanic at {loc}\n{msg}\n\nbacktrace:\n{}\n", crate::appinfo::support_report(), std::backtrace::Backtrace::force_capture());
            let _ = std::fs::write(dir.join(format!("crash-{}.txt", psxcore::recovery::unix_now())), text);
        }
        previous(info);
    }));
}

/// Number of crash reports waiting in the folder (shown once at start so the user knows they exist).
pub fn pending_reports() -> usize {
    crash_dir().and_then(|d| std::fs::read_dir(d).ok()).map(|r| r.filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().starts_with("crash-")).count()).unwrap_or(0)
}
