//! Telnet (RFC 854) for switch and router consoles on TCP port 23.
//!
//! Incoming bytes go through `Telnet::receive`, which separates the device's
//! data from the protocol: negotiation is answered (`replies`, sent straight
//! back) and described for the raw log (`notes`), and only the data reaches
//! the terminal pane and transcript.
//!
//! The device leads the negotiation; RawTap only answers:
//! - the device may echo (ECHO), suppress go-ahead (SGA) and send binary;
//! - RawTap will suppress go-ahead, report its window size (NAWS) and say its
//!   terminal type is XTERM (TTYPE);
//! - everything else is refused, once.

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const BRK: u8 = 243;

const BINARY: u8 = 0;
const ECHO: u8 = 1;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;

const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;
const TERMINAL_TYPE: &[u8] = b"XTERM";

/// The Telnet "break" command, sent by F7 on a Telnet connection.
pub const BREAK: [u8; 2] = [IAC, BRK];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ParseState {
    Data,
    Iac,
    Option(u8),
    Sub,
    SubIac,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Received {
    /// The device's own data, with the protocol taken out.
    pub data: Vec<u8>,
    /// Answers to send straight back.
    pub replies: Vec<u8>,
    /// What was agreed, for the raw log.
    pub notes: Vec<String>,
}

pub struct Telnet {
    state: ParseState,
    sub: Vec<u8>,
    /// A CR was the last data byte: a NUL straight after it is padding.
    after_cr: bool,
    /// Options the device has turned on for itself (it WILL), by option code.
    device_on: [bool; 256],
    /// Options RawTap has turned on (it said WILL), by option code.
    ours_on: [bool; 256],
    /// Options already refused, so a repeated request isn't answered again.
    refused: [bool; 256],
    cols: u16,
    rows: u16,
}

impl Telnet {
    pub fn new() -> Telnet {
        Telnet {
            state: ParseState::Data,
            sub: Vec::new(),
            after_cr: false,
            device_on: [false; 256],
            ours_on: [false; 256],
            refused: [false; 256],
            cols: 80,
            rows: 24,
        }
    }

    pub fn receive(&mut self, input: &[u8]) -> Received {
        let mut out = Received::default();
        for &byte in input {
            match self.state {
                ParseState::Data => {
                    if byte == IAC {
                        self.state = ParseState::Iac;
                    } else if !(byte == 0 && self.after_cr) {
                        out.data.push(byte);
                    }
                    self.after_cr = byte == b'\r';
                }
                ParseState::Iac => {
                    self.state = match byte {
                        IAC => {
                            out.data.push(IAC);
                            self.after_cr = false;
                            ParseState::Data
                        }
                        WILL | WONT | DO | DONT => ParseState::Option(byte),
                        SB => {
                            self.sub.clear();
                            ParseState::Sub
                        }
                        // NOP, GA, data mark and the rest carry nothing to show.
                        _ => ParseState::Data,
                    };
                }
                ParseState::Option(command) => {
                    self.negotiate(command, byte, &mut out);
                    self.state = ParseState::Data;
                }
                ParseState::Sub => {
                    if byte == IAC {
                        self.state = ParseState::SubIac;
                    } else {
                        self.sub.push(byte);
                    }
                }
                ParseState::SubIac => {
                    if byte == SE {
                        self.subnegotiation(&mut out);
                        self.state = ParseState::Data;
                    } else {
                        // IAC IAC inside a subnegotiation is a literal 255.
                        self.sub.push(byte);
                        self.state = ParseState::Sub;
                    }
                }
            }
        }
        out
    }

    fn negotiate(&mut self, command: u8, option: u8, out: &mut Received) {
        let index = option as usize;
        match command {
            WILL if matches!(option, ECHO | SGA | BINARY) => {
                if !self.device_on[index] {
                    self.device_on[index] = true;
                    out.replies.extend([IAC, DO, option]);
                    out.notes
                        .push(format!("Telnet: the device will {}", describe(option)));
                }
            }
            WILL => self.refuse(DONT, option, "the device offered", out),
            WONT => {
                if self.device_on[index] {
                    self.device_on[index] = false;
                    out.replies.extend([IAC, DONT, option]);
                    out.notes
                        .push(format!("Telnet: the device won't {}", describe(option)));
                }
            }
            DO if matches!(option, SGA | NAWS | TTYPE) => {
                if !self.ours_on[index] {
                    self.ours_on[index] = true;
                    out.replies.extend([IAC, WILL, option]);
                    if option == NAWS {
                        out.replies.extend(self.naws());
                        out.notes.push(format!(
                            "Telnet: reporting the window size ({}×{})",
                            self.cols, self.rows
                        ));
                    } else {
                        out.notes
                            .push(format!("Telnet: RawTap will {}", describe(option)));
                    }
                }
            }
            DO => self.refuse(WONT, option, "the device asked RawTap to", out),
            DONT if self.ours_on[index] => {
                self.ours_on[index] = false;
                out.replies.extend([IAC, WONT, option]);
            }
            _ => {}
        }
    }

    fn refuse(&mut self, answer: u8, option: u8, request: &str, out: &mut Received) {
        let index = option as usize;
        if !self.refused[index] {
            self.refused[index] = true;
            out.replies.extend([IAC, answer, option]);
            out.notes.push(format!(
                "Telnet: declined, {request} {} (option {option})",
                option_name(option)
            ));
        }
    }

    fn subnegotiation(&mut self, out: &mut Received) {
        if self.sub.as_slice() == [TTYPE, TTYPE_SEND] && self.ours_on[TTYPE as usize] {
            out.replies.extend([IAC, SB, TTYPE, TTYPE_IS]);
            out.replies.extend(TERMINAL_TYPE);
            out.replies.extend([IAC, SE]);
        }
    }

    /// The window size message: IAC SB NAWS width height IAC SE, with any
    /// 255 byte doubled.
    fn naws(&self) -> Vec<u8> {
        let mut message = vec![IAC, SB, NAWS];
        for byte in self
            .cols
            .to_be_bytes()
            .into_iter()
            .chain(self.rows.to_be_bytes())
        {
            message.push(byte);
            if byte == IAC {
                message.push(IAC);
            }
        }
        message.extend([IAC, SE]);
        message
    }

    /// The terminal pane changed size. Returns the window size message to
    /// send when the device asked for it.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
        if (cols, rows) == (self.cols, self.rows) {
            return None;
        }
        self.cols = cols;
        self.rows = rows;
        self.ours_on[NAWS as usize].then(|| self.naws())
    }
}

/// Doubles every 255 byte in data being sent, so the device doesn't read it
/// as a command.
pub fn escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for &byte in data {
        out.push(byte);
        if byte == IAC {
            out.push(IAC);
        }
    }
    out
}

/// Names for options a device commonly asks about.
fn option_name(option: u8) -> &'static str {
    match option {
        BINARY => "send binary",
        ECHO => "echo",
        SGA => "suppress go-ahead",
        5 => "send status",
        TTYPE => "send a terminal type",
        NAWS => "report a window size",
        32 => "report a terminal speed",
        33 => "use remote flow control",
        34 => "use line mode",
        35 => "send an X display location",
        36 | 39 => "send environment variables",
        _ => "use an option",
    }
}

fn describe(option: u8) -> &'static str {
    match option {
        ECHO => "echo what you type",
        SGA => "suppress go-ahead",
        BINARY => "send binary",
        TTYPE => "send its terminal type",
        _ => "use an option",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_data_passes_through() {
        let mut t = Telnet::new();
        let out = t.receive(b"Username: ");
        assert_eq!(out.data, b"Username: ");
        assert!(out.replies.is_empty() && out.notes.is_empty());
    }

    #[test]
    fn a_cisco_style_greeting_is_answered_and_removed() {
        let mut t = Telnet::new();
        let mut input = vec![IAC, WILL, ECHO, IAC, WILL, SGA, IAC, DO, NAWS];
        input.extend(b"\r\nUser Access Verification\r\n");
        let out = t.receive(&input);
        assert_eq!(out.data, b"\r\nUser Access Verification\r\n");
        let mut expected = vec![IAC, DO, ECHO, IAC, DO, SGA, IAC, WILL, NAWS];
        expected.extend([IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE]);
        assert_eq!(out.replies, expected);
        assert_eq!(out.notes.len(), 3);
    }

    #[test]
    fn requests_are_answered_once() {
        let mut t = Telnet::new();
        assert!(!t.receive(&[IAC, WILL, ECHO]).replies.is_empty());
        assert!(t.receive(&[IAC, WILL, ECHO]).replies.is_empty());
        assert_eq!(t.receive(&[IAC, DO, 34]).replies, [IAC, WONT, 34]);
        assert!(t.receive(&[IAC, DO, 34]).replies.is_empty());
    }

    #[test]
    fn terminal_type_is_sent_when_asked() {
        let mut t = Telnet::new();
        t.receive(&[IAC, DO, TTYPE]);
        let out = t.receive(&[IAC, SB, TTYPE, TTYPE_SEND, IAC, SE]);
        let mut expected = vec![IAC, SB, TTYPE, TTYPE_IS];
        expected.extend(b"XTERM");
        expected.extend([IAC, SE]);
        assert_eq!(out.replies, expected);
    }

    #[test]
    fn doubled_iac_is_one_data_byte_and_cr_nul_is_cr() {
        let mut t = Telnet::new();
        assert_eq!(t.receive(&[b'a', IAC, IAC, b'b']).data, [b'a', IAC, b'b']);
        assert_eq!(t.receive(b"x\r\0y").data, b"x\ry");
        // Split across reads too.
        assert_eq!(t.receive(&[b'z', IAC]).data, b"z");
        assert_eq!(t.receive(&[IAC]).data, [IAC]);
    }

    #[test]
    fn window_size_is_sent_after_a_resize_only_when_asked_for() {
        let mut t = Telnet::new();
        assert_eq!(t.resize(100, 30), None);
        t.receive(&[IAC, DO, NAWS]);
        assert_eq!(
            t.resize(120, 40).unwrap(),
            [IAC, SB, NAWS, 0, 120, 0, 40, IAC, SE]
        );
        assert_eq!(t.resize(120, 40), None);
    }

    #[test]
    fn outgoing_255_is_doubled() {
        assert_eq!(escape(&[1, IAC, 2]), [1, IAC, IAC, 2]);
    }
}
