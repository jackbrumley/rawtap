//! The session log (App Standard section 3): `~/.config/rawtap/debug/session.log`,
//! with the previous run kept as `last-session.log`. Lines are
//! `[<RFC 3339 time>] LEVEL: message`. This is RawTap's own diagnostic log;
//! the traffic logs in `logs/` are written by `logger.rs`.

use chrono::SecondsFormat;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const DEFAULT_LEVEL: LevelFilter = LevelFilter::Info;
/// The levels offered in F1 → Debug, quietest first.
pub const LEVELS: [LevelFilter; 5] = [
    LevelFilter::Error,
    LevelFilter::Warn,
    LevelFilter::Info,
    LevelFilter::Debug,
    LevelFilter::Trace,
];

struct SessionLog {
    file: Mutex<File>,
    path: PathBuf,
}

static SESSION_LOG: OnceLock<SessionLog> = OnceLock::new();

impl Log for SessionLog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // Other crates only get through at warn and above, so a debug log
        // stays about RawTap.
        let ours = metadata.target().starts_with(env!("CARGO_PKG_NAME"));
        metadata.level() <= log::max_level() && (ours || metadata.level() <= Level::Warn)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_line(record.level(), &record.args().to_string());
        if let Ok(mut file) = self.file.lock() {
            // Nowhere to report a failed write to the log itself: the TUI owns
            // the terminal.
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

pub fn format_line(level: Level, message: &str) -> String {
    let timestamp = chrono::Local::now().to_rfc3339_opts(SecondsFormat::Millis, false);
    format!("[{timestamp}] {level}: {message}\n")
}

/// Opens this run's session log in `debug_dir`, keeping the previous one as
/// `last-session.log`, and writes the header.
pub fn init(debug_dir: &Path, level: LevelFilter) -> Result<PathBuf, String> {
    fs::create_dir_all(debug_dir)
        .map_err(|error| format!("Could not create {}: {error}", debug_dir.display()))?;
    let path = debug_dir.join("session.log");
    if path.exists() {
        let last = debug_dir.join("last-session.log");
        fs::rename(&path, &last).map_err(|error| {
            format!(
                "Could not keep the previous session log as {}: {error}",
                last.display()
            )
        })?;
    }
    let mut file = File::create(&path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    let mut header = format_line(Level::Info, "SESSION START");
    for line in crate::system_info::SystemInfo::collect().summary().lines() {
        header.push_str(&format_line(Level::Info, line));
    }
    file.write_all(header.as_bytes())
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    let session_log = SESSION_LOG.get_or_init(|| SessionLog {
        file: Mutex::new(file),
        path: path.clone(),
    });
    log::set_logger(session_log)
        .map_err(|error| format!("Could not start the session log: {error}"))?;
    log::set_max_level(level);
    Ok(path)
}

/// This run's session log, once `init` has run.
pub fn path() -> Option<&'static Path> {
    SESSION_LOG.get().map(|log| log.path.as_path())
}

pub fn set_level(level: LevelFilter) {
    log::set_max_level(level);
    log::info!("Log level set to {}", level_name(level));
}

pub fn level_name(level: LevelFilter) -> &'static str {
    match level {
        LevelFilter::Off => "off",
        LevelFilter::Error => "error",
        LevelFilter::Warn => "warn",
        LevelFilter::Info => "info",
        LevelFilter::Debug => "debug",
        LevelFilter::Trace => "trace",
    }
}

pub fn parse_level(name: &str) -> Option<LevelFilter> {
    LEVELS
        .into_iter()
        .find(|level| level_name(*level).eq_ignore_ascii_case(name.trim()))
}

/// Installed first thing in `main`: puts the terminal back to normal, writes
/// the panic to the session log, then runs the default hook (which prints it).
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        crate::restore_terminal();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        log::error!("PANIC: {message} at {location} on thread {thread}");
        log::logger().flush();
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_is_rfc3339_level_message() {
        let line = format_line(Level::Info, "hello");
        assert!(line.starts_with('['));
        assert!(line.ends_with("] INFO: hello\n"), "{line}");
        let timestamp = &line[1..line.find(']').unwrap()];
        assert!(chrono::DateTime::parse_from_rfc3339(timestamp).is_ok());
        assert_eq!(timestamp.split('.').nth(1).unwrap().len(), 3 + 6); // .123+11:00
    }

    #[test]
    fn level_names_round_trip() {
        for level in LEVELS {
            assert_eq!(parse_level(level_name(level)), Some(level));
        }
        assert_eq!(parse_level("nonsense"), None);
    }
}
