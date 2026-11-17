//! Session log file, same layout as the Python rawtap logs plus RX/TX direction and milliseconds.

use crate::codec::{hex_string, text_string};
use crate::monitor::{Entry, EntryKind};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Logger {
    file: File,
    pub path: PathBuf,
    /// The file's name without a chosen name: "rawtap_ssh_20261117_191025".
    base: String,
}

impl Logger {
    pub fn create(dir: &Path, kind: &str, details: &str) -> std::io::Result<Logger> {
        std::fs::create_dir_all(dir)?;
        let now = chrono::Local::now();
        let base = format!("rawtap_{kind}_{}", now.format("%Y%m%d_%H%M%S"));
        let path = dir.join(format!("{base}.log"));
        let mut file = File::create(&path)?;
        write!(
            file,
            "RawTap Log File\nStart Time: {}\nConnection Type: {}\nConnection Details: {details}\n\n",
            now.format("%Y-%m-%d %H:%M:%S"),
            kind.to_uppercase()
        )?;
        Ok(Logger { file, path, base })
    }

    /// Gives the log a name, kept after its date and time (so logs still
    /// sort by time): "rawtap_ssh_20261117_191025_core-switch.log". The
    /// file stays open; renaming an open file works on Linux and Windows.
    pub fn rename(&mut self, name: &str) -> std::io::Result<()> {
        let path = self
            .path
            .with_file_name(format!("{}_{name}.log", self.base));
        if path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} already exists", path.display()),
            ));
        }
        std::fs::rename(&self.path, &path)?;
        self.path = path;
        Ok(())
    }

    pub fn entry(&mut self, e: &Entry) -> std::io::Result<()> {
        self.file.write_all(format_entry(e).as_bytes())
    }
}

/// One entry in log-file form (also used when copying from the raw log pane).
pub fn format_entry(e: &Entry) -> String {
    let t = e.time.format("%H:%M:%S%.3f");
    match &e.kind {
        EntryKind::Rx(d) | EntryKind::Tx(d) => {
            let dir = if matches!(e.kind, EntryKind::Rx(_)) {
                "RX"
            } else {
                "TX"
            };
            format!(
                "[{t}] {dir} HEX:  {}\n[{t}] {dir} TEXT: {}\n",
                hex_string(d),
                text_string(d)
            )
        }
        EntryKind::TxHidden => format!("[{t}] TX HEX:  <hidden>\n[{t}] TX TEXT: <hidden>\n"),
        EntryKind::Info(m) | EntryKind::Good(m) => format!("[{t}] -- {m}\n"),
        EntryKind::Error(m) => format!("[{t}] !! {m}\n"),
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        if let Err(error) = writeln!(
            self.file,
            "\nLog closed: {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        ) {
            log::warn!("Could not close {}: {error}", self.path.display());
        }
    }
}
