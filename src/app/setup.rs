//! The start screen: its fields, the detected serial ports, and turning
//! what's filled in into connection settings and a per-connection profile key.

use crate::config::{ConnMode, Saved};
use crate::link::LinkSettings;
use serialport::{DataBits, FlowControl, Parity, StopBits};
use std::time::Instant;

pub const BAUDS: &[&str] = &[
    "300", "1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200", "230400", "460800",
    "921600",
];
pub const DATA_BITS: &[&str] = &["5", "6", "7", "8"];
pub const PARITIES: &[&str] = &["None", "Odd", "Even"];
pub const STOP_BITS: &[&str] = &["1", "2"];
pub const FLOWS: &[&str] = &["None", "Software", "Hardware"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Mode,
    SerialPort,
    Baud,
    DataBits,
    Parity,
    StopBits,
    Flow,
    Host,
    TcpPort,
    Bind,
    ListenPort,
    TelnetHost,
    TelnetPort,
    Logging,
    Options,
    OptLineEnding,
    OptCharDelay,
    OptSendMode,
    OptLayout,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::Mode => "Connection",
            Field::SerialPort => "Serial port",
            Field::Baud => "Baud rate",
            Field::DataBits => "Data bits",
            Field::Parity => "Parity",
            Field::StopBits => "Stop bits",
            Field::Flow => "Flow control",
            Field::Host => "Host / IP",
            Field::TcpPort => "TCP port",
            Field::Bind => "Listen on",
            Field::ListenPort => "TCP port",
            Field::TelnetHost => "Host / IP",
            Field::TelnetPort => "Telnet port",
            Field::Logging => "Log to file",
            Field::Options => "Options",
            Field::OptLineEnding => "  Line ending",
            Field::OptCharDelay => "  Char delay",
            Field::OptSendMode => "  Send mode",
            Field::OptLayout => "  Layout",
        }
    }

    /// Shown under the form for the highlighted field.
    pub fn description(self) -> &'static str {
        match self {
            Field::Mode => {
                "Serial: a serial port (USB adapter or console cable). TCP client: raw TCP out to an IP and port. TCP server: wait for a device to connect to this computer. Telnet: a switch or router's Telnet login."
            }
            Field::SerialPort => {
                "The serial port to open. ←→ cycles through the ports detected on this computer, or type a name such as COM3 (Windows) or /dev/ttyUSB0 (Linux)."
            }
            Field::Baud => {
                "Speed of the serial line; it must match the device. Switch consoles are usually 9600 or 115200. ←→ for common speeds, or type one."
            }
            Field::DataBits | Field::Parity | Field::StopBits => {
                "Almost always 8 data bits, no parity, 1 stop bit (\"8N1\"). Only change these if the device's manual says so."
            }
            Field::Flow => {
                "Usually None. Hardware uses the RTS/CTS wires; Software uses XON/XOFF characters."
            }
            Field::Host => {
                "Host name or IP address of the device or terminal server. Raw TCP: for a Telnet login (port 23), choose Telnet as the connection."
            }
            Field::TcpPort => {
                "Port number on that host, e.g. a terminal server's raw TCP port for the device."
            }
            Field::Bind => {
                "Which address on this computer to listen on. 0.0.0.0 means all network adapters."
            }
            Field::ListenPort => {
                "Port devices will connect to on this computer. A firewall may need to allow it."
            }
            Field::TelnetHost => "Host name or IP address of the switch, router or server.",
            Field::TelnetPort => {
                "Usually 23. RawTap answers the Telnet negotiation, reports its window size and says it's an XTERM."
            }
            Field::Logging => {
                "Write a hex log and a plain-text transcript of the session to the logs folder."
            }
            Field::Options => {
                "Settings remembered for this port or host. ←→ or Enter to show / hide them. They can also be changed any time from the F1 menu."
            }
            Field::OptLineEnding => crate::menu::line_ending_desc(),
            Field::OptCharDelay => crate::menu::char_delay_desc(),
            Field::OptSendMode => crate::menu::send_mode_desc(),
            Field::OptLayout => crate::menu::layout_desc(),
        }
    }
    /// Text fields accept typing; the rest are cycled with ←/→.
    pub fn is_text(self) -> bool {
        matches!(
            self,
            Field::SerialPort
                | Field::Baud
                | Field::Host
                | Field::TcpPort
                | Field::Bind
                | Field::ListenPort
                | Field::TelnetHost
                | Field::TelnetPort
        )
    }
}

pub struct PortInfo {
    pub name: String,
    pub desc: String,
}

pub struct Setup {
    pub focus: usize,
    pub ports: Vec<PortInfo>,
    pub error: Option<String>,
    pub connecting: bool,
    pub options_open: bool,
    pub(super) last_scan: Option<Instant>,
}

impl Setup {
    pub fn fields(mode: ConnMode, options_open: bool) -> Vec<Field> {
        let mut f = vec![Field::Mode];
        match mode {
            ConnMode::Serial => f.extend([
                Field::SerialPort,
                Field::Baud,
                Field::DataBits,
                Field::Parity,
                Field::StopBits,
                Field::Flow,
            ]),
            ConnMode::Tcp => f.extend([Field::Host, Field::TcpPort]),
            ConnMode::Listen => f.extend([Field::Bind, Field::ListenPort]),
            ConnMode::Telnet => f.extend([Field::TelnetHost, Field::TelnetPort]),
        }
        f.push(Field::Logging);
        f.push(Field::Options);
        if options_open {
            f.extend([
                Field::OptLineEnding,
                Field::OptCharDelay,
                Field::OptSendMode,
                Field::OptLayout,
            ]);
        }
        f
    }
}

pub fn scan_ports() -> Vec<PortInfo> {
    let mut ports: Vec<PortInfo> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let desc = match p.port_type {
                serialport::SerialPortType::UsbPort(u) => {
                    let name = u
                        .product
                        .or(u.manufacturer)
                        .unwrap_or_else(|| "USB serial".into());
                    format!("{name} (USB {:04X}:{:04X})", u.vid, u.pid)
                }
                serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                serialport::SerialPortType::PciPort => "Built-in / PCI".into(),
                serialport::SerialPortType::Unknown => String::new(),
            };
            PortInfo {
                name: p.port_name,
                desc,
            }
        })
        .collect();
    let num = |n: &str| {
        n.trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    };
    ports.sort_by(|a, b| num(&a.name).cmp(&num(&b.name)).then(a.name.cmp(&b.name)));
    ports
}

pub(super) fn cycle_in(list: &[&str], current: &str, dir: i32) -> String {
    let i = list.iter().position(|v| v.eq_ignore_ascii_case(current));
    let n = list.len() as i32;
    let next = match i {
        Some(i) => (i as i32 + dir).rem_euclid(n),
        None => {
            if dir > 0 {
                0
            } else {
                n - 1
            }
        }
    };
    list[next as usize].to_string()
}

/// The profile key for what's currently typed in the start screen (matches `profile_key`).
pub fn form_profile_key(s: &Saved) -> String {
    let port = |p: &str| {
        p.trim()
            .parse::<u16>()
            .map(|n| n.to_string())
            .unwrap_or_else(|_| p.trim().to_string())
    };
    match s.mode {
        ConnMode::Serial => format!("serial:{}", serial_key(s.serial_port.trim())),
        ConnMode::Tcp => format!("tcp:{}:{}", s.tcp_host.trim(), port(&s.tcp_port)),
        ConnMode::Listen => format!("listen:{}", port(&s.listen_port)),
        ConnMode::Telnet => format!("telnet:{}:{}", s.telnet_host.trim(), port(&s.telnet_port)),
    }
}

/// Key for remembering view settings per connection, e.g. "serial:COM22".
pub fn profile_key(s: &LinkSettings) -> String {
    match s {
        LinkSettings::Serial { port, .. } => format!("serial:{}", serial_key(port)),
        LinkSettings::TcpClient { host, port } => format!("tcp:{host}:{port}"),
        LinkSettings::TcpListen { port, .. } => format!("listen:{port}"),
        LinkSettings::Telnet { host, port } => format!("telnet:{host}:{port}"),
    }
}

/// Windows port names are case-insensitive (com3 is COM3); Linux device paths
/// are case-sensitive and kept as they are.
fn serial_key(port: &str) -> String {
    if cfg!(windows) {
        port.to_uppercase()
    } else {
        port.to_string()
    }
}

pub fn settings_from_saved(s: &Saved) -> Result<LinkSettings, String> {
    let port_num = |v: &str, what: &str| -> Result<u16, String> {
        match v.trim().parse::<u16>() {
            Ok(p) if p > 0 => Ok(p),
            _ => Err(format!("{what} must be a number from 1 to 65535")),
        }
    };
    match s.mode {
        ConnMode::Serial => {
            let port = s.serial_port.trim().to_string();
            if port.is_empty() {
                return Err("Pick a serial port (←/→ cycles through the detected ones).".into());
            }
            let baud = match s.baud.trim().parse::<u32>() {
                Ok(b) if b > 0 => b,
                _ => return Err("Baud rate must be a number, e.g. 9600".into()),
            };
            let data_bits = match s.data_bits.as_str() {
                "5" => DataBits::Five,
                "6" => DataBits::Six,
                "7" => DataBits::Seven,
                _ => DataBits::Eight,
            };
            let parity = match s.parity.to_ascii_lowercase().as_str() {
                "odd" | "o" => Parity::Odd,
                "even" | "e" => Parity::Even,
                _ => Parity::None,
            };
            let stop_bits = if s.stop_bits == "2" {
                StopBits::Two
            } else {
                StopBits::One
            };
            let flow = match s.flow.to_ascii_lowercase().as_str() {
                "software" | "xonxoff" => FlowControl::Software,
                "hardware" | "rtscts" => FlowControl::Hardware,
                _ => FlowControl::None,
            };
            Ok(LinkSettings::Serial {
                port,
                baud,
                data_bits,
                parity,
                stop_bits,
                flow,
            })
        }
        ConnMode::Tcp => {
            let host = s.tcp_host.trim().to_string();
            if host.is_empty() {
                return Err("Enter a host name or IP address.".into());
            }
            Ok(LinkSettings::TcpClient {
                host,
                port: port_num(&s.tcp_port, "TCP port")?,
            })
        }
        ConnMode::Listen => {
            let bind = match s.listen_bind.trim() {
                "" => "0.0.0.0".to_string(),
                b => b.to_string(),
            };
            Ok(LinkSettings::TcpListen {
                bind,
                port: port_num(&s.listen_port, "TCP port")?,
            })
        }
        ConnMode::Telnet => {
            let host = s.telnet_host.trim().to_string();
            if host.is_empty() {
                return Err("Enter a host name or IP address.".into());
            }
            Ok(LinkSettings::Telnet {
                host,
                port: port_num(&s.telnet_port, "Telnet port")?,
            })
        }
    }
}
