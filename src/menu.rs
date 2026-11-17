//! The F1 pop-up menu: every action and per-connection setting in one searchable list,
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
    SelectAll,
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
    SaveFavourite,
    RememberPassword,
    ForgetPassword,
    SetUpKeyLogin,
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

/// The menu's sections, in the order its top level lists them.
pub mod section {
    pub const SESSION: &str = "Session";
    pub const SEND: &str = "Send";
    pub const COPY: &str = "Copy & paste";
    pub const VIEW: &str = "View";
    pub const CONNECTION: &str = "This connection";
    pub const MACROS: &str = "Macros";
    /// Updates and Debug come last, as in every app (App Standard).
    pub const UPDATES: &str = "Updates";
    pub const DEBUG: &str = "Debug";
    pub const ALL: [&str; 8] = [
        SESSION, SEND, COPY, VIEW, CONNECTION, MACROS, UPDATES, DEBUG,
    ];
    /// Keyboard shortcuts: on the top level, not in a section of its own.
    pub const HELP: &str = "Help";
}

/// On the top level under the sections, so they're always one step away.
const PINNED: [Cmd; 2] = [Cmd::Shortcuts, Cmd::Quit];

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
    /// The section that's open; `None` is the top level.
    pub section: Option<&'static str>,
    /// Index into the visible rows.
    pub selected: usize,
}

/// A row of the menu.
#[derive(Clone)]
pub enum Row {
    /// A section on the top level, with a hint of what's in it.
    Section {
        name: &'static str,
        hint: String,
    },
    Item(Item),
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
    "Terminal shows just the live terminal; Hex just the raw data (hex and text); Split both, side by side when the window is wide. Also F3, or click the switch in the footer. RawTap opens each connection in the view you used last, unless its start screen Options give it its own."
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
    let mut v = Vec::new();

    // Sending
    v.push(setting(
        section::SEND,
        "Live keys",
        if m.live { "on".into() } else { "off".into() },
        "F4",
        "On: every key goes straight to the device as you press it, like PuTTY: Tab completion, ?, space at --More--, Ctrl+C, arrow keys. Off: type a line and Enter sends it.",
        Setting::LiveKeys,
    ));
    v.push(cmd(
        section::SEND,
        "Hidden send (password)",
        "F6",
        "Type something without it being shown, logged or kept in history, then Enter sends it with the line ending. Use it for passwords.".into(),
        Cmd::HiddenSend,
    ));
    match m.password_stored() {
        Some(false) => v.push(cmd(
            section::SEND,
            "Remember password…",
            "",
            format!(
                "Keeps a password for {} in {} (never in RawTap's files). RawTap then sends it, hidden, at the first password prompt of each connection.",
                m.settings.describe(),
                crate::secrets::store_name()
            ),
            Cmd::RememberPassword,
        )),
        Some(true) => v.push(cmd(
            section::SEND,
            "Forget password",
            "",
            format!(
                "Removes the password remembered for {} from {}.",
                m.settings.describe(),
                crate::secrets::store_name()
            ),
            Cmd::ForgetPassword,
        )),
        None => {}
    }
    if matches!(m.settings, crate::link::LinkSettings::Ssh { .. }) {
        v.push(cmd(
            section::SEND,
            "Set up key login",
            "",
            "Once you're logged in to a Linux or Unix server: adds your SSH key to ~/.ssh/authorized_keys there (making a key first if you have none), so the next login needs no password.".into(),
            Cmd::SetUpKeyLogin,
        ));
    }
    v.push(cmd(
        section::SEND,
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
            section::MACROS,
            &format!("Macro: {}", mac.label),
            &mac.key.label(),
            desc,
            Cmd::Macro(i),
        ));
    }

    // Clipboard
    v.push(cmd(
        section::COPY,
        "Copy selection",
        "Ctrl+C / right-click",
        "A selection is copied as soon as you make it (let go of the mouse), and stays highlighted. Drag past the pane edge (or use the wheel) to select more than a screen; Ctrl+click extends. Raw log copies whole entries in full. Ctrl+C or right-click on a selection copies it again and clears the highlight; with nothing selected Ctrl+C goes to the device in live keys.".into(),
        Cmd::Copy,
    ));
    v.push(cmd(
        section::COPY,
        "Select all",
        "Alt+A / Ctrl+A",
        "Selects everything in the pane you're in (F5 switches): the whole terminal with its scrollback, or every raw log entry. It's copied straight away. Alt+A works everywhere; Ctrl+A only in line mode, because in live keys it goes to the device.".into(),
        Cmd::SelectAll,
    ));
    v.push(cmd(
        section::COPY,
        "Paste",
        "Right-click",
        "Pastes the clipboard. One line goes into the send box (or straight to the device in live keys). Several lines ask first, then go line by line using the char delay.".into(),
        Cmd::Paste,
    ));
    v.push(cmd(
        section::COPY,
        "Copy all terminal output",
        "",
        "Copies everything the device has printed this session as plain text: handy for long show commands that scroll off the screen.".into(),
        Cmd::CopyAll,
    ));
    v.push(cmd(
        section::COPY,
        "Open transcript",
        "",
        "Opens this session's plain-text transcript (everything the device printed, cleaned up) in your text editor: Notepad on Windows, the desktop's default app on Linux.".into(),
        Cmd::OpenTranscript,
    ));

    // Settings remembered for this connection
    v.push(setting(
        section::CONNECTION,
        "Line ending",
        m.profile.line_ending.label().into(),
        "F2",
        line_ending_desc(),
        Setting::LineEnding,
    ));
    v.push(setting(
        section::CONNECTION,
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
        section::CONNECTION,
        "Send mode",
        m.profile.input_mode.label().into(),
        "Tab",
        send_mode_desc(),
        Setting::InputMode,
    ));
    v.push(setting(
        section::VIEW,
        "View",
        m.layout.label().into(),
        "F3",
        layout_desc(),
        Setting::Layout,
    ));
    v.push(setting(
        section::VIEW,
        "Hex format",
        m.log_format().label().into(),
        "F8",
        "How the Hex pane shows data: compact (one row each), hex+text, hex only or text only. The log file always has both.",
        Setting::LogFormat,
    ));

    // View
    if m.layout == Layout::Split {
        v.push(setting(
            section::VIEW,
            "Scroll pane",
            if m.focus == crate::monitor::Pane::Log { "Hex".into() } else { "Terminal".into() },
            "F5",
            "Which pane PgUp / PgDn scroll in the Split view. The mouse wheel always scrolls the pane under the pointer.",
            Setting::ScrollPane,
        ));
    }
    v.push(setting(
        section::VIEW,
        "Hex pane",
        if m.follow { "following".into() } else { "paused".into() },
        "F9",
        "Paused freezes the Hex pane so you can read it. Data is still received and saved to the log file.",
        Setting::LogFollow,
    ));
    v.push(cmd(
        section::VIEW,
        "Clear screen",
        "F10",
        "Clears both panes on screen. The log and transcript files keep everything.".into(),
        Cmd::Clear,
    ));

    // Session
    v.push(setting(
        section::CONNECTION,
        "Log to file",
        if m.logger.is_some() { "on".into() } else { "off".into() },
        "",
        "Writes a hex log and a plain-text transcript of this session to the logs folder. Turning it off closes the current files.",
        Setting::Logging,
    ));
    v.push(cmd(
        section::SESSION,
        "Reconnect",
        "Ctrl+R",
        format!(
            "Closes and reopens {} with the same settings. Starts a new log file.",
            m.settings.describe()
        ),
        Cmd::Reconnect,
    ));
    v.push(cmd(
        section::SESSION,
        "New connection…",
        "Ctrl+N",
        format!("Back to the start screen to pick a different port or host. Settings for {conn} are kept."),
        Cmd::NewConnection,
    ));
    v.push(cmd(
        section::SESSION,
        "Save as favourite…",
        "",
        format!(
            "Names {} and lists it at the top of the start screen.",
            m.settings.describe()
        ),
        Cmd::SaveFavourite,
    ));
    v.push(cmd(
        section::HELP,
        "Keyboard shortcuts & send syntax",
        "",
        "Lists every shortcut key and the HEX / ASCII send syntax.".into(),
        Cmd::Shortcuts,
    ));
    v.push(cmd(
        section::SESSION,
        "Quit",
        "Ctrl+Q",
        "Exits (closes) RawTap. Settings and history are saved. In live keys Ctrl+Q goes to the device, but in this menu it always quits: F1, then Ctrl+Q."
            .into(),
        Cmd::Quit,
    ));

    // Updates (Release Standard section 7).
    let version = env!("CARGO_PKG_VERSION");
    let last_checked = crate::update::last_checked().unwrap_or_else(|| "never".into());
    if let Some(update) = crate::update::available() {
        v.push(cmd(
            section::UPDATES,
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
        section::UPDATES,
        "Check for updates",
        "",
        format!("You have RawTap {version}. Last checked: {last_checked}.{check_state}"),
        Cmd::CheckForUpdates,
    ));

    // Debug (App Standard section 4): what a bug report needs.
    v.push(cmd(
        section::DEBUG,
        "Copy log",
        "",
        "Copies RawTap's own session log (not the device traffic) to the clipboard, for a bug report.".into(),
        Cmd::CopySessionLog,
    ));
    v.push(cmd(
        section::DEBUG,
        "Open log folder",
        "",
        "Opens the folder with RawTap's session log, the previous session's log and the update log.".into(),
        Cmd::OpenLogFolder,
    ));
    v.push(cmd(
        section::DEBUG,
        "Copy system info",
        "",
        "Copies RawTap's version, the OS, the desktop and the terminal to the clipboard. Bug reports ask for this and the log.".into(),
        Cmd::CopySystemInfo,
    ));
    v.push(setting(
        section::DEBUG,
        "Log level",
        crate::session_log::level_name(log::max_level()).into(),
        "",
        "How much RawTap writes to its session log. Info is enough for most bug reports; debug and trace add detail.",
        Setting::LogLevel,
    ));
    v
}

/// The rows the menu shows. The top level lists the sections, then Quit;
/// an open section lists its items; typing searches every item, wherever
/// it is.
pub fn visible(m: &Monitor, menu: &Menu) -> Vec<Row> {
    let all = items(m);
    let f = menu.filter.trim().to_lowercase();
    if !f.is_empty() {
        return search(all, &f).into_iter().map(Row::Item).collect();
    }
    if let Some(open) = menu.section {
        return all
            .into_iter()
            .filter(|i| i.section == open)
            .map(Row::Item)
            .collect();
    }
    let mut rows: Vec<Row> = section::ALL
        .iter()
        .filter_map(|&name| {
            let labels: Vec<&str> = all
                .iter()
                .filter(|i| i.section == name)
                .map(|i| i.label.trim_end_matches('…'))
                .collect();
            (!labels.is_empty()).then(|| Row::Section {
                name,
                hint: labels.join(", "),
            })
        })
        .collect();
    rows.extend(PINNED.iter().filter_map(|&pinned| {
        all.iter()
            .find(|i| i.kind == Kind::Cmd(pinned))
            .cloned()
            .map(Row::Item)
    }));
    rows
}

/// Where a section sits on the top level, so going back highlights it.
pub fn section_row(m: &Monitor, name: &str) -> usize {
    visible(m, &Menu::default())
        .iter()
        .position(|r| matches!(r, Row::Section { name: n, .. } if *n == name))
        .unwrap_or(0)
}

/// Items matching every word of `filter`; the ones whose name matches come
/// before the ones that only match in their description.
fn search(all: Vec<Item>, filter: &str) -> Vec<Item> {
    let words: Vec<&str> = filter.split_whitespace().collect();
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
