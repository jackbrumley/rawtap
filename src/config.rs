//! Config file (rawtap_config.json), remembered session state (rawtap_state.json), and paths.

use crate::codec::{InputMode, LineEnding};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CONFIG_NAME: &str = "rawtap_config.json";
pub const STATE_NAME: &str = "rawtap_state.json";

/// Where RawTap keeps its files. `base` is the folder holding the config file (next to the exe
/// by default), and logs / extracted images / captures / state are written relative to it.
pub struct Paths {
    pub base: PathBuf,
    pub config_file: PathBuf,
    pub state_file: PathBuf,
}

impl Paths {
    pub fn discover() -> Paths {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let cwd = std::env::current_dir().ok();
        let base = [exe_dir.clone(), cwd]
            .into_iter()
            .flatten()
            .find(|d| d.join(CONFIG_NAME).is_file())
            .or(exe_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        Paths::at(base)
    }

    pub fn at(base: PathBuf) -> Paths {
        Paths {
            config_file: base.join(CONFIG_NAME),
            state_file: base.join(STATE_NAME),
            base,
        }
    }

    pub fn resolve(&self, p: &str) -> PathBuf {
        let p = Path::new(p);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base.join(p)
        }
    }
}

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
  "_readme": "RawTap config. Edit and restart RawTap. Keys starting with _ are ignored.",
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

/// Load the config, writing a template if there isn't one. Returns notes to show the user.
pub fn load(paths: &Paths) -> (Config, Vec<String>) {
    let mut notes = Vec::new();
    if !paths.config_file.is_file() {
        match std::fs::write(&paths.config_file, TEMPLATE) {
            Ok(()) => notes.push(format!(
                "Created starter config: {}",
                paths.config_file.display()
            )),
            Err(e) => notes.push(format!("No config file and couldn't create one: {e}")),
        }
    }
    let config = match std::fs::read_to_string(&paths.config_file) {
        Ok(text) => match serde_json::from_str::<Config>(&text) {
            Ok(c) => c,
            Err(e) => {
                notes.push(format!(
                    "Config error in {} ({e}). Using defaults.",
                    paths.config_file.display()
                ));
                Config::default()
            }
        },
        Err(_) => Config::default(),
    };
    (config, notes)
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
            ConnMode::Serial => "Serial (COM port)",
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
            history: Vec::new(),
            profiles: BTreeMap::new(),
        }
    }
}

pub fn load_saved(paths: &Paths) -> Saved {
    std::fs::read_to_string(&paths.state_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn store_saved(paths: &Paths, saved: &Saved) {
    if let Ok(text) = serde_json::to_string_pretty(saved) {
        let _ = std::fs::write(&paths.state_file, text);
    }
}
