//! Opening a file or folder in the desktop's own app: the transcript and the
//! log folder. Windows uses Notepad for text files and Explorer for folders;
//! Linux uses `xdg-open`, which needs a desktop session.

use std::path::Path;
use std::process::{Command, Stdio};

/// Starts the viewer and returns once it's running; the viewer outlives
/// nothing in RawTap, so it isn't waited for.
pub fn open(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{} doesn't exist", path.display()));
    }
    let mut command = viewer(path)?;
    command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| log::info!("Opened {}", path.display()))
        .map_err(|error| format!("Could not open {}: {error}", path.display()))
}

#[cfg(windows)]
fn viewer(path: &Path) -> Result<Command, String> {
    Ok(Command::new(if path.is_dir() {
        "explorer.exe"
    } else {
        "notepad.exe"
    }))
}

#[cfg(not(windows))]
fn viewer(path: &Path) -> Result<Command, String> {
    // Over SSH or on a bare console there is no desktop to open it in.
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err(format!(
            "No desktop session to open it in. It's at {}",
            path.display()
        ));
    }
    Ok(Command::new("xdg-open"))
}
