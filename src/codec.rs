//! Byte formatting (hex / control-char text) and parsing of what the user types to send.

use serde::{Deserialize, Serialize};

const CTRL: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR",
    "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM", "SUB", "ESC",
    "FS", "GS", "RS", "US",
];

/// Text form of one byte: printable ASCII as-is, control chars as `<CR>`, others as `<0xFF>`.
pub fn byte_text(b: u8) -> String {
    match b {
        0x00..=0x1F => format!("<{}>", CTRL[b as usize]),
        0x7F => "<DEL>".to_string(),
        0x20..=0x7E => (b as char).to_string(),
        _ => format!("<0x{b:02X}>"),
    }
}

pub fn hex_string(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

pub fn text_string(data: &[u8]) -> String {
    data.iter().map(|&b| byte_text(b)).collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputMode {
    Hex,
    #[default]
    Ascii,
}

impl InputMode {
    pub fn label(self) -> &'static str {
        match self {
            InputMode::Hex => "HEX",
            InputMode::Ascii => "ASCII",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    #[default]
    None,
    Cr,
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn bytes(self) -> &'static [u8] {
        match self {
            LineEnding::None => b"",
            LineEnding::Cr => b"\r",
            LineEnding::Lf => b"\n",
            LineEnding::Crlf => b"\r\n",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            LineEnding::None => "none",
            LineEnding::Cr => "CR",
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
        }
    }
}

/// Turn user input into bytes. HEX mode sends exactly the bytes typed; ASCII mode appends `ending`.
pub fn encode(input: &str, mode: InputMode, ending: LineEnding) -> Result<Vec<u8>, String> {
    match mode {
        InputMode::Hex => parse_hex(input),
        InputMode::Ascii => {
            let mut out = parse_ascii(input)?;
            out.extend_from_slice(ending.bytes());
            Ok(out)
        }
    }
}

/// Accepts `02 41 03`, `024103`, `0x02,0x41,0x03`, `02:41:03`, `02-41-03`, or a mix.
pub fn parse_hex(input: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let is_sep = |c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-');
    for raw in input.split(is_sep) {
        if raw.is_empty() {
            continue;
        }
        let tok = raw
            .strip_prefix("0x")
            .or_else(|| raw.strip_prefix("0X"))
            .unwrap_or(raw);
        if tok.is_empty() || !tok.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("'{raw}' is not hex"));
        }
        if tok.len() == 1 {
            out.push(u8::from_str_radix(tok, 16).unwrap());
            continue;
        }
        if tok.len() % 2 != 0 {
            return Err(format!("'{raw}' has an odd number of hex digits"));
        }
        for i in (0..tok.len()).step_by(2) {
            out.push(u8::from_str_radix(&tok[i..i + 2], 16).unwrap());
        }
    }
    Ok(out)
}

/// ASCII with escapes: `\r \n \t \0 \e \\ \< \xHH`, plus the same tokens the display
/// uses (`<CR>`, `<STX>`, `<0x1B>`...), so a received TEXT line can be pasted back and sent.
pub fn parse_ascii(input: &str) -> Result<Vec<u8>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            let Some(&n) = chars.get(i + 1) else {
                return Err("trailing \\ (use \\\\ for a backslash)".into());
            };
            i += 2;
            match n {
                'r' => out.push(0x0D),
                'n' => out.push(0x0A),
                't' => out.push(0x09),
                '0' => out.push(0x00),
                'e' => out.push(0x1B),
                '\\' => out.push(b'\\'),
                '<' => out.push(b'<'),
                'x' => {
                    let h: String = chars.iter().skip(i).take(2).collect();
                    if h.len() == 2 && h.chars().all(|c| c.is_ascii_hexdigit()) {
                        out.push(u8::from_str_radix(&h, 16).unwrap());
                        i += 2;
                    } else {
                        return Err("\\x needs two hex digits, e.g. \\x02".into());
                    }
                }
                other => {
                    return Err(format!("unknown escape \\{other} (use \\\\ for a backslash)"));
                }
            }
            continue;
        }
        if c == '<' {
            // Tokens are at most 4 chars inside the brackets (<NUL>, <0xFF>).
            if let Some(end) = chars[i + 1..].iter().take(5).position(|&ch| ch == '>') {
                let name: String = chars[i + 1..i + 1 + end].iter().collect();
                if let Some(b) = token_byte(&name) {
                    out.push(b);
                    i += end + 2;
                    continue;
                }
            }
        }
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        i += 1;
    }
    Ok(out)
}

fn token_byte(name: &str) -> Option<u8> {
    let up = name.to_ascii_uppercase();
    if let Some(p) = CTRL.iter().position(|n| *n == up) {
        return Some(p as u8);
    }
    if up == "DEL" {
        return Some(0x7F);
    }
    if let Some(h) = up.strip_prefix("0X") {
        if h.len() == 2 && h.chars().all(|c| c.is_ascii_hexdigit()) {
            return u8::from_str_radix(h, 16).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_formats() {
        assert_eq!(parse_hex("02 41 03").unwrap(), vec![2, 0x41, 3]);
        assert_eq!(parse_hex("024103").unwrap(), vec![2, 0x41, 3]);
        assert_eq!(parse_hex("0x02,0x41, 0x03").unwrap(), vec![2, 0x41, 3]);
        assert_eq!(parse_hex("02:41-03").unwrap(), vec![2, 0x41, 3]);
        assert_eq!(parse_hex("0xA f").unwrap(), vec![0x0A, 0x0F]);
        assert_eq!(parse_hex("").unwrap(), Vec::<u8>::new());
        assert!(parse_hex("0241 3G").is_err());
        assert!(parse_hex("024").is_err());
        assert!(parse_hex("0x").is_err());
    }

    #[test]
    fn ascii_escapes_and_tokens() {
        assert_eq!(parse_ascii("AB\\r\\n").unwrap(), b"AB\r\n");
        assert_eq!(parse_ascii("<STX>A01<ETX>").unwrap(), b"\x02A01\x03");
        assert_eq!(parse_ascii("<cr><lf>").unwrap(), b"\r\n");
        assert_eq!(parse_ascii("<0x1B>[0m").unwrap(), b"\x1B[0m");
        assert_eq!(parse_ascii("\\x02hi\\x03").unwrap(), b"\x02hi\x03");
        assert_eq!(parse_ascii("a<b>c").unwrap(), b"a<b>c");
        assert_eq!(parse_ascii("\\<CR>").unwrap(), b"<CR>");
        assert_eq!(parse_ascii("x < y").unwrap(), b"x < y");
        assert!(parse_ascii("bad\\q").is_err());
        assert!(parse_ascii("end\\").is_err());
        assert!(parse_ascii("\\x2").is_err());
    }

    #[test]
    fn display_round_trips_through_ascii_parser() {
        let data: Vec<u8> = (0u8..=255).collect();
        let shown = text_string(&data);
        // '<' and '\' are shown literally, so escape them before re-parsing.
        let escaped = shown.replace('\\', "\\\\");
        let parsed = parse_ascii(&escaped).unwrap();
        assert_eq!(parsed, data);
    }

    #[test]
    fn encode_line_endings() {
        assert_eq!(encode("OK", InputMode::Ascii, LineEnding::Crlf).unwrap(), b"OK\r\n");
        assert_eq!(encode("", InputMode::Ascii, LineEnding::Cr).unwrap(), b"\r");
        assert_eq!(encode("4F 4B", InputMode::Hex, LineEnding::Crlf).unwrap(), b"OK");
    }

    #[test]
    fn formatting() {
        assert_eq!(hex_string(b"Hi\r\n"), "48 69 0D 0A");
        assert_eq!(text_string(b"Hi\r\n\xFF"), "Hi<CR><LF><0xFF>");
    }
}
