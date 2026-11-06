//! Session log file, same layout as the Python rawtap logs plus RX/TX direction and milliseconds.

use crate::codec::{hex_string, text_string};
use crate::monitor::{Entry, EntryKind};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Logger {
    file: File,
    pub path: PathBuf,
}

impl Logger {
    pub fn create(dir: &Path, kind: &str, details: &str) -> std::io::Result<Logger> {
        std::fs::create_dir_all(dir)?;
        let now = chrono::Local::now();
        let path = dir.join(format!("rawtap_{kind}_{}.log", now.format("%Y%m%d_%H%M%S")));
        let mut file = File::create(&path)?;
        write!(
            file,
            "RawTap Log File\nStart Time: {}\nConnection Type: {}\nConnection Details: {details}\n\n",
            now.format("%Y-%m-%d %H:%M:%S"),
            kind.to_uppercase()
        )?;
        Ok(Logger { file, path })
    }

    pub fn entry(&mut self, e: &Entry) {
        let _ = self.file.write_all(format_entry(e).as_bytes());
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
        EntryKind::TxHidden(n) => {
            format!("[{t}] TX HEX:  <hidden: {n} bytes>\n[{t}] TX TEXT: <hidden>\n")
        }
        EntryKind::Info(m) | EntryKind::Good(m) => format!("[{t}] -- {m}\n"),
        EntryKind::Error(m) => format!("[{t}] !! {m}\n"),
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        let _ = writeln!(
            self.file,
            "\nLog closed: {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );
    }
}
