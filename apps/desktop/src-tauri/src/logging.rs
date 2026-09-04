//! Local file logging (spec section 16): `%APPDATA%\FluxChunk\logs\`, one
//! file per calendar day, purged both by age (7 days) and total size
//! (50MB) at every launch. Default level is info (error/warn/info write;
//! debug doesn't) until "Verbose logging" is turned on in Settings --
//! debug-level *is* that verbose gate, a direct 1:1 mapping rather than a
//! separate flag to keep in sync.
//!
//! Vault secrets and full request/response bodies are never logged by
//! default -- only method/host/status/timing at normal levels. Even in
//! verbose mode, callers must only ever pass the *visible* (pre-vault,
//! `{{vault:...}}` still unresolved) request content, matching exactly
//! what the UI itself shows -- never the fully-resolved wire content,
//! which is the same boundary `fluxchunk_engine::vars` enforces for
//! secrets never reaching the UI or scripts.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use chrono::Local;
use url::Url;

const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static VERBOSE: AtomicBool = AtomicBool::new(false);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Creates the log directory if needed, purges anything past the
/// age/size caps, and remembers the directory for subsequent log calls.
/// Safe to call once at startup; a failure here (e.g. an unwritable
/// `%APPDATA%`) is surfaced to the caller rather than silently
/// swallowed, since it usually means something is wrong with the whole
/// app-data location, not just logging.
pub fn init(log_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(log_dir).map_err(|e| format!("couldn't create log directory: {e}"))?;
    purge_old(log_dir);
    let _ = LOG_DIR.set(log_dir.to_path_buf());
    Ok(())
}

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

fn purge_old(log_dir: &Path) {
    let Ok(entries) = fs::read_dir(log_dir) else { return };
    let now = SystemTime::now();

    let mut files: Vec<(PathBuf, SystemTime, u64)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("log"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let modified = meta.modified().ok()?;
            Some((e.path(), modified, meta.len()))
        })
        .collect();

    // Age-based purge first.
    files.retain(|(path, modified, _)| {
        let age = now.duration_since(*modified).unwrap_or_default();
        if age > MAX_AGE {
            let _ = fs::remove_file(path);
            false
        } else {
            true
        }
    });

    // Then size-based: oldest-first until the remainder is under the cap.
    files.sort_by_key(|(_, modified, _)| *modified);
    let mut total: u64 = files.iter().map(|(_, _, len)| len).sum();
    for (path, _, len) in &files {
        if total <= MAX_TOTAL_BYTES {
            break;
        }
        if fs::remove_file(path).is_ok() {
            total = total.saturating_sub(*len);
        }
    }
}

fn file_path_for(now: chrono::DateTime<Local>) -> Option<PathBuf> {
    let dir = LOG_DIR.get()?;
    Some(dir.join(format!("fluxchunk-{}.log", now.format("%Y-%m-%d"))))
}

/// Local time, not UTC -- this is a single-user desktop app whose logs
/// exist for the user (or a bug report) to read; matching their own
/// wall clock, both for which day's file a request landed in and for
/// each line's timestamp, is what "check the log for the request I just
/// sent" actually expects.
fn write_line(level: &str, msg: &str) {
    let now = Local::now();
    let Some(path) = file_path_for(now) else { return };
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "[{}] [{level}] {msg}", now.format("%H:%M:%S"));
    }
}

pub fn info(msg: impl AsRef<str>) {
    write_line("INFO", msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    write_line("WARN", msg.as_ref());
}

pub fn error(msg: impl AsRef<str>) {
    write_line("ERROR", msg.as_ref());
}

/// No-ops unless verbose logging is on -- see the module doc for why
/// debug level and the verbose setting are the same gate.
pub fn debug(msg: impl AsRef<str>) {
    if is_verbose() {
        write_line("DEBUG", msg.as_ref());
    }
}

fn host_only(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| match u.port() {
            Some(port) => format!("{h}:{port}"),
            None => h.to_string(),
        }))
        .unwrap_or_else(|| "unknown-host".to_string())
}

/// Spec section 16: "only method/host/status/timing at normal levels" --
/// deliberately never the path, query string, headers, or body, all of
/// which can carry request-specific/sensitive content.
pub fn log_request_sent(method: &str, url: &str, status: u16, elapsed_ms: u128) {
    info(format!("{method} {} -> {status} ({elapsed_ms}ms)", host_only(url)));
}

pub fn log_request_failed(method: &str, url: &str, kind: &str, message: &str) {
    warn(format!("{method} {} failed [{kind}]: {message}", host_only(url)));
}

/// Only ever called with the *visible* (pre-vault) headers/body -- see
/// the module doc. No-ops when verbose logging is off.
pub fn log_verbose_request(headers: &str, body: Option<&str>) {
    debug(format!("request headers: {headers}"));
    if let Some(b) = body {
        debug(format!("request body: {b}"));
    }
}
