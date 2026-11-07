//! `settings.json` (your macros and options, edited by hand) and `state.json`
//! (what RawTap remembers between runs). Locations come from `paths.rs`;
//! reading and saving go through `json_store.rs`.

use crate::codec::{InputMode, LineEnding};
use crate::json_store::{self, Loaded};
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub log_directory: Option<String>,
    pub capture_directory: Option<String>,
    pub image_extraction: Option<ExtractConfig>,
    pub macros: Vec<Macro>,
}

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct ExtractConfig {
    pub enabled: bool,
    pub header_magic: String,
    pub header_length: usize,
    pub size_offset: usize,
    pub size_length: usize,
    pub size_endian: String,
    pub output_directory: String,
    pub filename_prefix: String,
    pub file_extension: String,
    pub max_payload_bytes: u64,
}

impl Default for ExtractConfig {
    // Same defaults as the original Python rawtap.py.
    fn default() -> Self {
        ExtractConfig {
            enabled: true,
            header_magic: "AB CD 12 34 CA FE BA BE".into(),
            header_length: 21,
            size_offset: 17,
            size_length: 4,
            size_endian: "little".into(),
            output_directory: "logs/extracted_images".into(),
            filename_prefix: "scan_image_".into(),
            file_extension: "bmp".into(),
            max_payload_bytes: 16 * 1024 * 1024,
        }
    }
}

/// A macro: either a single send (`data`) or a list of `steps`.
#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Macro {
    /// e.g. "Alt+1", "Shift+F5", "F11"
    pub key: String,
    pub label: String,
    pub mode: InputMode,
    pub data: Option<String>,
    /// For ASCII sends. Leave out to use the current line ending (F2).
    pub line_ending: Option<LineEnding>,
    pub steps: Vec<Step>,
    /// Save the device output seen while the macro runs to a text file.
    pub capture: bool,
}

/// One macro step. Any of these may be combined; they run in the order
/// delay → send → wait → wait_idle.
#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Step {
    pub delay_ms: Option<u64>,
    pub send: Option<String>,
    pub send_hex: Option<String>,
    pub line_ending: Option<LineEnding>,
    /// Wait until this text appears in the device output (after the last send).
    pub wait: Option<String>,
    /// Wait until the device has been quiet for this long.
    pub wait_idle_ms: Option<u64>,
    /// Give up on wait / wait_idle after this long (default 10 s).
    pub timeout_ms: Option<u64>,
}

const TEMPLATE: &str = r##"{
  "_readme": "RawTap settings. Edit and restart RawTap. Keys starting with _ are ignored.",
  "log_directory": "logs",
  "capture_directory": "captures",
  "macros": [
    { "key": "Alt+1", "label": "Hello", "mode": "ascii", "data": "HELLO" },
    { "key": "Alt+2", "label": "STX A ETX", "mode": "hex", "data": "02 41 03" },
    {
      "key": "Alt+9", "label": "Switch: save running-config", "capture": true,
      "steps": [
        { "send": "terminal length 0", "wait": "#" },
        { "send": "show running-config", "wait_idle_ms": 2000, "timeout_ms": 120000 }
      ]
    }
  ],
  "_macros_help": "key: Alt+0..9, Shift+F1..F12, F11, F12 (F1-F10 are built in). mode: ascii|hex. line_ending: none|cr|lf|crlf, default = current (F2). Steps: delay_ms, send, send_hex, wait, wait_idle_ms, timeout_ms. capture: true saves the output to capture_directory. Don't put passwords in macros - use F6.",
  "image_extraction": {
    "enabled": false,
    "header_magic": "AB CD 12 34 CA FE BA BE",
    "header_length": 21,
    "size_offset": 17,
    "size_length": 4,
    "size_endian": "little",
    "output_directory": "logs/extracted_images",
    "filename_prefix": "scan_image_",
    "file_extension": "bmp",
    "max_payload_bytes": 16777216
  }
}
"##;

/// Loads `settings.json`, writing the starter template when there isn't one.
/// Returns notes to show the user. `Err` means the file exists but can't be
/// read: stop rather than overwrite it.
pub fn load(paths: &Paths) -> Result<(Config, Vec<String>), String> {
    let mut notes = Vec::new();
    let config = match json_store::read::<Config>(&paths.settings_file)? {
        Loaded::Parsed(config) => config,
        Loaded::Missing => {
            write_template(paths, &mut notes);
            template_config()
        }
        Loaded::Quarantined { backup, error } => {
            let note = format!(
                "settings.json had an error ({error}), so it was renamed {} and RawTap started with the default settings.",
                backup.display()
            );
            log::warn!("{note}");
            notes.push(note);
            write_template(paths, &mut notes);
            template_config()
        }
    };
    Ok((config, notes))
}

/// The starter settings, so a first run has the example macros whether or
/// not the template could be written.
fn template_config() -> Config {
    serde_json::from_str(TEMPLATE).expect("the starter settings template is valid JSON")
}

fn write_template(paths: &Paths, notes: &mut Vec<String>) {
    match json_store::write_atomically(&paths.settings_file, TEMPLATE.as_bytes()) {
        Ok(()) => {
            log::info!("Created {}", paths.settings_file.display());
            notes.push(format!(
                "Created the starter settings file: {}",
                paths.settings_file.display()
            ));
        }
        Err(error) => {
            log::error!("{error}");
            notes.push(error);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnMode {
    #[default]
    Serial,
    Tcp,
    Listen,
}

impl ConnMode {
    pub fn label(self) -> &'static str {
        match self {
            ConnMode::Serial => "Serial port",
            ConnMode::Tcp => "TCP client (connect out)",
            ConnMode::Listen => "TCP server (listen)",
        }
    }
    pub fn cycle(self, dir: i32) -> Self {
        let all = [ConnMode::Serial, ConnMode::Tcp, ConnMode::Listen];
        let i = all.iter().position(|m| *m == self).unwrap() as i32;
        all[(i + dir).rem_euclid(all.len() as i32) as usize]
    }
}

/// Which panes the monitor shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Split,
    Log,
    Terminal,
}

impl Layout {
    pub fn label(self) -> &'static str {
        match self {
            Layout::Split => "split",
            Layout::Log => "log only",
            Layout::Terminal => "terminal only",
        }
    }
}

/// How the raw log shows each chunk.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// One row per chunk, truncated to fit.
    #[default]
    Compact,
    Both,
    Hex,
    Text,
}

impl LogFormat {
    pub fn label(self) -> &'static str {
        match self {
            LogFormat::Compact => "compact",
            LogFormat::Both => "hex+text",
            LogFormat::Hex => "hex",
            LogFormat::Text => "text",
        }
    }
}

/// View/send preferences remembered per connection (e.g. "serial:COM22").
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub layout: Layout,
    /// Log format when sharing the screen with the terminal.
    pub split_log: LogFormat,
    /// Log format in log-only layout.
    pub full_log: LogFormat,
    pub line_ending: LineEnding,
    pub input_mode: InputMode,
    /// Terminal pane width in side-by-side layout (0 = automatic).
    pub term_cols: u16,
    /// Log pane height in stacked layout (0 = automatic).
    pub log_rows: u16,
    /// Delay between characters when sending (Ctrl+T). Some device consoles garble
    /// input that arrives faster than a person types.
    pub char_delay_ms: u64,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            layout: Layout::Split,
            split_log: LogFormat::Compact,
            full_log: LogFormat::Both,
            line_ending: LineEnding::Cr,
            input_mode: InputMode::Ascii,
            term_cols: 0,
            log_rows: 0,
            char_delay_ms: 0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub mode: InputMode,
    pub text: String,
}

/// Everything remembered between runs.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub mode: ConnMode,
    pub serial_port: String,
    pub baud: String,
    pub data_bits: String,
    pub parity: String,
    pub stop_bits: String,
    pub flow: String,
    pub tcp_host: String,
    pub tcp_port: String,
    pub listen_bind: String,
    pub listen_port: String,
    pub logging: bool,
    /// Session log level (F1 → Debug): error, warn, info, debug or trace.
    pub log_level: String,
    /// When the update check last finished (F1 → Updates, "Last checked").
    pub last_update_check: Option<String>,
    pub history: Vec<HistoryItem>,
    pub profiles: BTreeMap<String, Profile>,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            mode: ConnMode::Serial,
            serial_port: String::new(),
            baud: "9600".into(),
            data_bits: "8".into(),
            parity: "None".into(),
            stop_bits: "1".into(),
            flow: "None".into(),
            tcp_host: String::new(),
            tcp_port: String::new(),
            listen_bind: "0.0.0.0".into(),
            listen_port: String::new(),
            logging: true,
            log_level: crate::session_log::level_name(crate::session_log::DEFAULT_LEVEL).into(),
            last_update_check: None,
            history: Vec::new(),
            profiles: BTreeMap::new(),
        }
    }
}

/// Loads `state.json`. Returns a note when it had to be set aside. `Err`
/// means the file exists but can't be read: stop rather than overwrite it.
pub fn load_saved(paths: &Paths) -> Result<(Saved, Option<String>), String> {
    Ok(match json_store::read::<Saved>(&paths.state_file)? {
        Loaded::Parsed(saved) => (saved, None),
        Loaded::Missing => (Saved::default(), None),
        Loaded::Quarantined { backup, error } => {
            let note = format!(
                "state.json had an error ({error}), so it was renamed {} and RawTap started without remembered connections and history.",
                backup.display()
            );
            log::warn!("{note}");
            (Saved::default(), Some(note))
        }
    })
}

pub fn store_saved(paths: &Paths, saved: &Saved) -> Result<(), String> {
    json_store::write_json(&paths.state_file, saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_template_parses_with_its_macros() {
        let config = template_config();
        assert!(config.macros.iter().any(|m| m.key == "Alt+9" && m.capture));
    }
}
