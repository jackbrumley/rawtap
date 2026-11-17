//! Linux: gives RawTap started from the app menu a terminal window that the
//! taskbar shows as RawTap, with RawTap's name and icon.
//!
//! A launcher with `Terminal=true` opens the desktop's terminal under the
//! terminal's own window class, so the taskbar showed a terminal. Instead
//! the launcher starts `rawtap --own-window` with no terminal, and RawTap
//! opens itself in the desktop's terminal with the window class set to
//! RawTap's app ID. The launcher's `StartupWMClass` is that same ID, so the
//! taskbar matches the window to the launcher and its icon.
//!
//! The terminal is the desktop's choice (Cinnamon's or GNOME's setting),
//! then `x-terminal-emulator`, then the common ones. A terminal with no way
//! to set the class still runs RawTap, under the terminal's icon.

use crate::app::identity::APP_ID;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The size RawTap's own window opens at, as on Windows: wide enough for
/// the split view, tall enough for the start screen.
const COLUMNS: u16 = 140;
const ROWS: u16 = 42;

/// Set on the RawTap started in the new window, so it can never start
/// itself again.
const STARTED_ENV: &str = "RAWTAP_OWN_WINDOW";

/// Terminals RawTap knows how to start with its own class and size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    GnomeTerminal,
    Ghostty,
    Kitty,
    Alacritty,
    WezTerm,
    Foot,
    Xterm,
    /// Started with the usual `-e`, under its own class.
    Other,
}

/// Tried in order when the desktop names no terminal that's installed.
const COMMON: &[&str] = &[
    "gnome-terminal",
    "ghostty",
    "kitty",
    "alacritty",
    "wezterm",
    "foot",
    "konsole",
    "xfce4-terminal",
    "mate-terminal",
    "tilix",
    "xterm",
];

/// What `ensure` did.
pub enum Outcome {
    /// Carry on in this terminal.
    Stay,
    /// RawTap is starting in its own window; this process should exit.
    Started,
    /// No terminal could be started: say why.
    Failed(String),
}

/// Opens RawTap in a terminal window of its own, unless this is that
/// window already.
pub fn ensure() -> Outcome {
    if std::env::var_os(STARTED_ENV).is_some() {
        return Outcome::Stay;
    }
    match open_in_terminal() {
        Ok(()) => Outcome::Started,
        Err(error) => Outcome::Failed(error),
    }
}

/// Started from the app menu with no terminal to show it in: say why as a
/// desktop notification (and on stderr, for the desktop's own log).
pub fn report_without_terminal(error: &str) {
    eprintln!("{error}");
    if let Err(e) = Command::new("notify-send")
        .args([
            "--app-name=RawTap",
            "--icon=rawtap",
            "RawTap couldn't open",
            error,
        ])
        .status()
    {
        eprintln!("notify-send: {e}");
    }
}

fn open_in_terminal() -> Result<(), String> {
    let terminal = find_terminal().ok_or_else(|| {
        format!(
            "RawTap couldn't find a terminal to open in. Install one (such as gnome-terminal or xterm), or run rawtap from a terminal. Looked for: {}.",
            COMMON.join(", ")
        )
    })?;
    // An AppImage's own path is inside its mount; $APPIMAGE is the file.
    let exe = match std::env::var_os("APPIMAGE") {
        Some(path) => PathBuf::from(path),
        None => std::env::current_exe()
            .map_err(|e| format!("RawTap couldn't find its own program file: {e}"))?,
    };
    let mut command = vec![exe.to_string_lossy().into_owned()];
    command.extend(std::env::args().skip(1).filter(|a| a != "--own-window"));
    Command::new(&terminal)
        .args(terminal_args(kind_of(&terminal), &command))
        .env(STARTED_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("RawTap couldn't start {}: {e}", terminal.display()))
}

/// The desktop's terminal, then `x-terminal-emulator`, then a common one.
fn find_terminal() -> Option<PathBuf> {
    desktop_terminal()
        .and_then(|name| on_path(&name))
        .or_else(|| on_path("x-terminal-emulator"))
        .or_else(|| COMMON.iter().find_map(|name| on_path(name)))
}

/// The terminal set in Cinnamon's or GNOME's settings, which is what a
/// `Terminal=true` launcher opened.
fn desktop_terminal() -> Option<String> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let schema = if desktop.contains("Cinnamon") {
        "org.cinnamon.desktop.default-applications.terminal"
    } else if desktop.contains("GNOME") {
        "org.gnome.desktop.default-applications.terminal"
    } else {
        return None;
    };
    let output = Command::new("gsettings")
        .args(["get", schema, "exec"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_matches('\'')
        .to_string();
    (output.status.success() && !name.is_empty()).then_some(name)
}

fn on_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        return Path::new(name).is_file().then(|| PathBuf::from(name));
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Which terminal a program is, following links such as
/// `x-terminal-emulator` → `/usr/bin/ghostty`.
fn kind_of(program: &Path) -> Kind {
    let real = std::fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf());
    let name = real
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.trim_end_matches(".wrapper").trim_end_matches(".real") {
        "gnome-terminal" => Kind::GnomeTerminal,
        "ghostty" => Kind::Ghostty,
        "kitty" => Kind::Kitty,
        "alacritty" => Kind::Alacritty,
        "wezterm" | "wezterm-gui" => Kind::WezTerm,
        "foot" => Kind::Foot,
        "xterm" | "uxterm" => Kind::Xterm,
        _ => Kind::Other,
    }
}

/// The terminal's arguments that set RawTap's class and size, then run
/// `command`.
fn terminal_args(kind: Kind, command: &[String]) -> Vec<String> {
    let size = format!("{COLUMNS}x{ROWS}");
    let mut args: Vec<String> = match kind {
        Kind::GnomeTerminal => vec![
            format!("--class={APP_ID}"),
            format!("--geometry={size}"),
            "--".into(),
        ],
        Kind::Ghostty => vec![
            format!("--class={APP_ID}"),
            format!("--window-width={COLUMNS}"),
            format!("--window-height={ROWS}"),
            "-e".into(),
        ],
        Kind::Kitty => vec![
            "--class".into(),
            APP_ID.into(),
            "-o".into(),
            format!("initial_window_width={COLUMNS}c"),
            "-o".into(),
            format!("initial_window_height={ROWS}c"),
        ],
        Kind::Alacritty => vec![
            "--class".into(),
            APP_ID.into(),
            "-o".into(),
            format!("window.dimensions.columns={COLUMNS}"),
            "-o".into(),
            format!("window.dimensions.lines={ROWS}"),
            "-e".into(),
        ],
        Kind::WezTerm => vec!["start".into(), "--class".into(), APP_ID.into(), "--".into()],
        Kind::Foot => vec![
            format!("--app-id={APP_ID}"),
            format!("--window-size-chars={size}"),
        ],
        Kind::Xterm => vec![
            "-class".into(),
            APP_ID.into(),
            "-geometry".into(),
            size,
            "-e".into(),
        ],
        Kind::Other => vec!["-e".into()],
    };
    args.extend(command.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_terminal_gets_the_app_id_as_its_class_then_the_command() {
        let command = vec!["/usr/bin/rawtap".to_string()];
        for kind in [
            Kind::GnomeTerminal,
            Kind::Ghostty,
            Kind::Kitty,
            Kind::Alacritty,
            Kind::WezTerm,
            Kind::Foot,
            Kind::Xterm,
        ] {
            let args = terminal_args(kind, &command);
            assert!(
                args.iter().any(|a| a.contains(APP_ID)),
                "{kind:?}: {args:?}"
            );
            assert_eq!(args.last().unwrap(), "/usr/bin/rawtap", "{kind:?}");
        }
        assert_eq!(
            terminal_args(Kind::Other, &command),
            ["-e", "/usr/bin/rawtap"]
        );
    }

    #[test]
    fn terminals_are_known_by_their_real_name() {
        assert_eq!(kind_of(Path::new("/no/such/ghostty")), Kind::Ghostty);
        assert_eq!(
            kind_of(Path::new("/no/such/gnome-terminal.wrapper")),
            Kind::GnomeTerminal
        );
        assert_eq!(kind_of(Path::new("/no/such/konsole")), Kind::Other);
    }
}
