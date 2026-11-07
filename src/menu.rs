//! The F1 / Ctrl+P pop-up menu: every action and per-connection setting in one searchable list,
//! with a description of the highlighted item.

use crate::codec::InputMode;
use crate::config::Layout;
use crate::link::LinkSettings;
use crate::monitor::Monitor;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    HiddenSend,
    Break,
    Macro(usize),
    ToggleLive,
    PauseLog,
    Clear,
    FocusPane,
    Shortcuts,
    Copy,
    Paste,
    CopyAll,
    OpenTranscript,
    CopySessionLog,
    OpenLogFolder,
    CopySystemInfo,
    CheckForUpdates,
    UpdateNow,
    Reconnect,
    NewConnection,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Setting {
    LineEnding,
    CharDelay,
    InputMode,
    Layout,
    LogFormat,
    Logging,
    LiveKeys,
    ScrollPane,
    LogFollow,
    LogLevel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Cmd(Cmd),
    Setting(Setting),
}

#[derive(Clone)]
pub struct Item {
    pub section: &'static str,
    pub label: String,
    /// Current value, for settings.
    pub value: Option<String>,
    /// Direct shortcut, shown on the right.
    pub key: String,
    pub desc: String,
    pub kind: Kind,
}

#[derive(Default)]
pub struct Menu {
    pub filter: String,
    /// Index into the visible (filtered) items.
    pub selected: usize,
}

pub const CHAR_DELAYS: [u64; 6] = [0, 2, 5, 10, 20, 50];

pub fn line_ending_desc() -> &'static str {
    "What's added when you press Enter. CR suits most switches and matches PuTTY. CR+LF counts as two Enters on many switches."
}
pub fn char_delay_desc() -> &'static str {
    "Pause between each character sent. If the device's echo comes back garbled, it can't keep up with a whole line at once: try 5–10 ms."
}
pub fn send_mode_desc() -> &'static str {
    "How the send box reads what you type. ASCII: text, plus \\r \\n \\xHH and <STX>-style tokens. HEX: bytes like 02 41 03, sent exactly."
}
pub fn layout_desc() -> &'static str {
    "Split shows the raw log and the live terminal together: side by side when the window is wide, stacked when it's narrow. Or show just one."
}

fn setting(
    section: &'static str,
    label: &str,
    value: String,
    key: &str,
    desc: &str,
    s: Setting,
) -> Item {
    Item {
        section,
        label: label.into(),
        value: Some(value),
        key: key.into(),
        desc: desc.into(),
        kind: Kind::Setting(s),
    }
}

fn cmd(section: &'static str, label: &str, key: &str, desc: String, c: Cmd) -> Item {
    Item {
        section,
        label: label.into(),
        value: None,
        key: key.into(),
        desc,
        kind: Kind::Cmd(c),
    }
}

/// All menu items for the current state.
pub fn items(m: &Monitor) -> Vec<Item> {
    let serial = matches!(m.settings, LinkSettings::Serial { .. });
    let conn = m.profile_key.clone();
    let settings_section: &'static str = "SETTINGS FOR THIS CONNECTION";
    let mut v = Vec::new();

    // Sending
    v.push(setting(
        "SEND",
        "Live keys",
        if m.live { "on".into() } else { "off".into() },
        "F4",
        "On: every key goes straight to the device as you press it, like PuTTY: Tab completion, ?, space at --More--, Ctrl+C, arrow keys. Off: type a line and Enter sends it.",
        Setting::LiveKeys,
    ));
    v.push(cmd(
        "SEND",
        "Hidden send (password)",
        "F6",
        "Type something without it being shown, logged or kept in history, then Enter sends it with the line ending. Use it for passwords.".into(),
        Cmd::HiddenSend,
    ));
    v.push(cmd(
        "SEND",
        if serial { "Send break" } else { "Send break (serial only)" },
        "F7",
        "Holds the serial line in 'break' for 300 ms. Used for password recovery or to drop a switch into ROMMON. Only works on serial ports.".into(),
        Cmd::Break,
    ));
    for (i, mac) in m.macros.iter().enumerate() {
        let d = &mac.def;
        let mut desc = match &d.data {
            Some(data) if d.steps.is_empty() => format!("Sends {:?} ({}).", data, d.mode.label()),
            _ => format!(
                "Runs a {}-step macro from rawtap_config.json.",
                d.steps.len() + d.data.is_some() as usize
            ),
        };
        if d.capture {
            desc.push_str(" Saves what the device prints to the captures folder.");
        }
        desc.push_str(" Esc cancels it while it runs.");
        v.push(cmd(
            "MACROS",
            &format!("Macro: {}", mac.label),
            &mac.key.label(),
            desc,
            Cmd::Macro(i),
        ));
    }

    // Clipboard
    v.push(cmd(
        "COPY & PASTE",
        "Copy selection",
        "Ctrl+Shift+C",
        "Copies what you selected with the mouse. Drag past the pane edge (or use the wheel) to select more than a screen; Ctrl+click extends. Raw log copies whole entries in full.".into(),
        Cmd::Copy,
    ));
    v.push(cmd(
        "COPY & PASTE",
        "Paste",
        "Ctrl+Shift+V",
        "Pastes the clipboard. One line goes into the send box (or straight to the device in live keys). Several lines ask first, then go line by line using the char delay.".into(),
        Cmd::Paste,
    ));
    v.push(cmd(
        "COPY & PASTE",
        "Copy all terminal output",
        "",
        "Copies everything the device has printed this session as plain text: handy for long show commands that scroll off the screen.".into(),
        Cmd::CopyAll,
    ));
    v.push(cmd(
        "COPY & PASTE",
        "Open transcript",
        "",
        "Opens this session's plain-text transcript (everything the device printed, cleaned up) in your text editor: Notepad on Windows, the desktop's default app on Linux.".into(),
        Cmd::OpenTranscript,
    ));

    // Settings remembered for this connection
    v.push(setting(
        settings_section,
        "Line ending",
        m.profile.line_ending.label().into(),
        "F2",
        line_ending_desc(),
        Setting::LineEnding,
    ));
    v.push(setting(
        settings_section,
        "Char delay",
        match m.profile.char_delay_ms {
            0 => "off".into(),
            ms => format!("{ms} ms"),
        },
        "Ctrl+T",
        char_delay_desc(),
        Setting::CharDelay,
    ));
    v.push(setting(
        settings_section,
        "Send mode",
        m.profile.input_mode.label().into(),
        "Tab",
        send_mode_desc(),
        Setting::InputMode,
    ));
    v.push(setting(
        settings_section,
        "Layout",
        m.profile.layout.label().into(),
        "F3",
        layout_desc(),
        Setting::Layout,
    ));
    v.push(setting(
        settings_section,
        "Raw log format",
        m.log_format().label().into(),
        "F8",
        "How the raw log pane shows data: compact (one row each), hex+text, hex only or text only. The log file always has both.",
        Setting::LogFormat,
    ));

    // View
    if m.profile.layout == Layout::Split {
        v.push(setting(
            "VIEW",
            "Scroll pane",
            if m.focus == crate::monitor::Pane::Log { "raw log".into() } else { "terminal".into() },
            "F5",
            "Which pane PgUp / PgDn scroll in the split layout. The mouse wheel always scrolls the pane under the pointer.",
            Setting::ScrollPane,
        ));
    }
    v.push(setting(
        "VIEW",
        "Raw log",
        if m.follow { "following".into() } else { "paused".into() },
        "F9",
        "Paused freezes the raw log pane so you can read it. Data is still received and saved to the log file.",
        Setting::LogFollow,
    ));
    v.push(cmd(
        "VIEW",
        "Clear screen",
        "F10",
        "Clears both panes on screen. The log and transcript files keep everything.".into(),
        Cmd::Clear,
    ));

    // Session
    v.push(setting(
        "SESSION",
        "Log to file",
        if m.logger.is_some() { "on".into() } else { "off".into() },
        "",
        "Writes a hex log and a plain-text transcript of this session to the logs folder. Turning it off closes the current files.",
        Setting::Logging,
    ));
    v.push(cmd(
        "SESSION",
        "Reconnect",
        "Ctrl+R",
        format!(
            "Closes and reopens {} with the same settings. Starts a new log file.",
            m.settings.describe()
        ),
        Cmd::Reconnect,
    ));
    v.push(cmd(
        "SESSION",
        "New connection…",
        "Ctrl+N",
        format!("Back to the start screen to pick a different port or host. Settings for {conn} are kept."),
        Cmd::NewConnection,
    ));
    v.push(cmd(
        "SESSION",
        "Keyboard shortcuts & send syntax",
        "",
        "Lists every shortcut key and the HEX / ASCII send syntax.".into(),
        Cmd::Shortcuts,
    ));
    v.push(cmd(
        "SESSION",
        "Quit",
        "Ctrl+Q",
        "Closes RawTap. Settings and history are saved. Ctrl+Shift+Q also quits from live keys."
            .into(),
        Cmd::Quit,
    ));

    // Updates (Release Standard section 7).
    let version = env!("CARGO_PKG_VERSION");
    let last_checked = crate::update::last_checked().unwrap_or_else(|| "never".into());
    if let Some(update) = crate::update::available() {
        v.push(cmd(
            "UPDATES",
            &format!("Update to {}", update.latest_version),
            "",
            format!(
                "Installs RawTap {} (you have {version}) with the same installer as the website, then starts it again. Release notes: {}",
                update.latest_version, update.release_url
            ),
            Cmd::UpdateNow,
        ));
    }
    let check_state = match crate::update::status() {
        crate::update::Status::Checking => " Checking now…".to_string(),
        crate::update::Status::Failed(error) => format!(" The last check failed: {error}"),
        crate::update::Status::NotChecked | crate::update::Status::Checked(_) => String::new(),
    };
    v.push(cmd(
        "UPDATES",
        "Check for updates",
        "",
        format!("You have RawTap {version}. Last checked: {last_checked}.{check_state}"),
        Cmd::CheckForUpdates,
    ));

    // Debug (App Standard section 4): what a bug report needs.
    v.push(cmd(
        "DEBUG",
        "Copy log",
        "",
        "Copies RawTap's own session log (not the device traffic) to the clipboard, for a bug report.".into(),
        Cmd::CopySessionLog,
    ));
    v.push(cmd(
        "DEBUG",
        "Open log folder",
        "",
        "Opens the folder with RawTap's session log, the previous session's log and the update log.".into(),
        Cmd::OpenLogFolder,
    ));
    v.push(cmd(
        "DEBUG",
        "Copy system info",
        "",
        "Copies RawTap's version, the OS, the desktop and the terminal to the clipboard. Bug reports ask for this and the log.".into(),
        Cmd::CopySystemInfo,
    ));
    v.push(setting(
        "DEBUG",
        "Log level",
        crate::session_log::level_name(log::max_level()).into(),
        "",
        "How much RawTap writes to its session log. Info is enough for most bug reports; debug and trace add detail.",
        Setting::LogLevel,
    ));
    v
}

/// Items matching the filter (all of them when it's empty).
pub fn visible(m: &Monitor, filter: &str) -> Vec<Item> {
    let all = items(m);
    let f = filter.trim().to_lowercase();
    if f.is_empty() {
        return all;
    }
    let words: Vec<&str> = f.split_whitespace().collect();
    let mut hits: Vec<(bool, Item)> = all
        .into_iter()
        .filter(|i| {
            let hay = format!("{} {} {} {}", i.label, i.section, i.desc, i.key).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .map(|i| {
            let in_name = words.iter().all(|w| i.label.to_lowercase().contains(w));
            (in_name, i)
        })
        .collect();
    // Items whose name matches come before ones that only match in their description.
    hits.sort_by_key(|(in_name, _)| !*in_name);
    hits.into_iter().map(|(_, i)| i).collect()
}

/// Step through a list of values, wrapping, in either direction.
pub fn step<T: PartialEq + Copy>(all: &[T], cur: T, dir: i32) -> T {
    let n = all.len() as i32;
    match all.iter().position(|v| *v == cur) {
        Some(i) => all[((i as i32 + dir).rem_euclid(n)) as usize],
        None => all[0],
    }
}

pub fn input_modes() -> [InputMode; 2] {
    [InputMode::Ascii, InputMode::Hex]
}
