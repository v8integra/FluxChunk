//! Crash reporting (spec section 16): local-first, GitHub Issues as the
//! report destination, zero backend. A panic hook writes a redacted,
//! app/system-level-only report before the process exits -- version, OS,
//! stack trace, and a general action label like "sending request" --
//! explicitly never anything about the specific request in flight (no
//! URLs, headers, bodies, variables, response data), since request-level
//! issues are almost always environment-specific to the user and not
//! actionable by the maintainer anyway. `set_context` is the only way
//! that action label gets set, and it only ever accepts a static label,
//! never caller data, so there's no way for request content to leak in
//! through it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

static LAST_ACTION: OnceLock<Mutex<String>> = OnceLock::new();

/// Records what the app was doing, for the crash report's "last action"
/// line -- e.g. `set_context("sending request")`. Never pass request
/// data (URLs, headers, bodies, variables) here; only static labels.
pub fn set_context(action: &str) {
    let cell = LAST_ACTION.get_or_init(|| Mutex::new(String::new()));
    if let Ok(mut guard) = cell.lock() {
        *guard = action.to_string();
    }
}

fn context() -> String {
    LAST_ACTION
        .get()
        .and_then(|c| c.lock().ok().map(|g| g.clone()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no recorded action)".to_string())
}

#[derive(Serialize, Deserialize)]
struct PendingCrash {
    path: String,
    summary: String,
}

#[derive(Debug, Serialize)]
pub struct CrashInfo {
    pub path: String,
    pub summary: String,
}

/// Installs the panic hook. Chains the previous (default) hook so
/// existing behavior -- printing to stderr, etc. -- is unchanged; this
/// only adds the redacted-report side effect before that runs.
pub fn install_panic_hook(crash_dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = fs::create_dir_all(&crash_dir);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        let message = panic_message(info);
        let backtrace = std::backtrace::Backtrace::force_capture();

        let report = format!(
            "FluxChunk crash report\n\
             time: {now} (unix)\n\
             version: {}\n\
             os: {} ({})\n\
             last action: {}\n\
             panic: {message}\n\
             location: {location}\n\n\
             backtrace:\n{backtrace}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            context(),
        );

        let path = crash_dir.join(format!("crash-{now}.txt"));
        if fs::write(&path, &report).is_ok() {
            let pending = PendingCrash {
                path: path.to_string_lossy().into_owned(),
                summary: format!("{message} ({location})"),
            };
            if let Ok(json) = serde_json::to_string(&pending) {
                let _ = fs::write(crash_dir.join("pending.json"), json);
            }
        }

        previous(info);
    }));
}

fn panic_message(info: &std::panic::PanicHookInfo) -> String {
    if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "(non-string panic payload)".to_string()
    }
}

/// Reads and clears the pending-crash marker, if one exists -- call once
/// at launch. Clearing on read means the calm "closed unexpectedly"
/// notice appears exactly once per crash, not on every later launch;
/// the crash report file itself is left in place so "View details" /
/// "Report this issue" can still read it afterward.
pub fn take_pending_crash(crash_dir: &Path) -> Option<CrashInfo> {
    let marker = crash_dir.join("pending.json");
    let contents = fs::read_to_string(&marker).ok()?;
    let pending: PendingCrash = serde_json::from_str(&contents).ok()?;
    let _ = fs::remove_file(&marker);
    Some(CrashInfo { path: pending.path, summary: pending.summary })
}

pub fn read_report(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("couldn't read crash report: {e}"))
}
