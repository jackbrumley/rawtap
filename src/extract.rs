//! Pulls framed payloads (e.g. scanner BMP images) out of the byte stream.
//! Frame: [magic ... header_length bytes, with a size field at size_offset] [payload of that size].

use crate::config::{ExtractConfig, Paths};
use std::path::PathBuf;

pub enum ExtractEvent {
    Saved { path: PathBuf, len: usize },
    Failed(String),
    Skipped(String),
}

pub struct Extractor {
    enabled: bool,
    magic: Vec<u8>,
    header_len: usize,
    size_offset: usize,
    size_len: usize,
    big_endian: bool,
    max_payload: u64,
    output_dir: PathBuf,
    prefix: String,
    ext: String,
    buffer: Vec<u8>,
}

impl Extractor {
    pub fn disabled() -> Extractor {
        Extractor {
            enabled: false,
            magic: Vec::new(),
            header_len: 0,
            size_offset: 0,
            size_len: 0,
            big_endian: false,
            max_payload: 0,
            output_dir: PathBuf::new(),
            prefix: String::new(),
            ext: String::new(),
            buffer: Vec::new(),
        }
    }

    /// Returns the extractor plus a one-line description of its state for the UI.
    pub fn new(cfg: Option<&ExtractConfig>, paths: &Paths) -> (Extractor, String) {
        let Some(cfg) = cfg.filter(|c| c.enabled) else {
            return (Extractor::disabled(), "Image extraction: off".into());
        };
        let clean: String = cfg.header_magic.split_whitespace().collect();
        let magic = match crate::codec::parse_hex(&clean) {
            Ok(m) if !m.is_empty() => m,
            _ => {
                return (
                    Extractor::disabled(),
                    format!(
                        "Image extraction disabled: bad header_magic '{}'",
                        cfg.header_magic
                    ),
                );
            }
        };
        if !(1..=8).contains(&cfg.size_length)
            || cfg.header_length < cfg.size_offset + cfg.size_length
        {
            return (
                Extractor::disabled(),
                "Image extraction disabled: size field doesn't fit inside header_length".into(),
            );
        }
        let output_dir = paths.resolve(&cfg.output_directory);
        let note = format!(
            "Image extraction: on (magic {}, saving to {})",
            cfg.header_magic,
            output_dir.display()
        );
        let ext = cfg.file_extension.trim_start_matches('.').to_string();
        (
            Extractor {
                enabled: true,
                magic,
                header_len: cfg.header_length,
                size_offset: cfg.size_offset,
                size_len: cfg.size_length,
                big_endian: cfg.size_endian.eq_ignore_ascii_case("big"),
                max_payload: cfg.max_payload_bytes,
                output_dir,
                prefix: cfg.filename_prefix.clone(),
                ext: if ext.is_empty() { "bin".into() } else { ext },
                buffer: Vec::new(),
            },
            note,
        )
    }

    pub fn feed(&mut self, data: &[u8]) -> Vec<ExtractEvent> {
        let mut events = Vec::new();
        if !self.enabled {
            return events;
        }
        self.buffer.extend_from_slice(data);
        loop {
            let Some(idx) = find(&self.buffer, &self.magic) else {
                // Keep a possible partial magic at the end.
                let keep = self.magic.len() - 1;
                if self.buffer.len() > keep {
                    self.buffer.drain(..self.buffer.len() - keep);
                }
                break;
            };
            if idx > 0 {
                self.buffer.drain(..idx);
            }
            if self.buffer.len() < self.header_len {
                break;
            }
            let field = &self.buffer[self.size_offset..self.size_offset + self.size_len];
            let size = if self.big_endian {
                field.iter().fold(0u64, |a, &b| (a << 8) | b as u64)
            } else {
                field.iter().rev().fold(0u64, |a, &b| (a << 8) | b as u64)
            };
            if size > self.max_payload {
                events.push(ExtractEvent::Skipped(format!(
                    "Ignored frame header claiming {size} bytes (over max_payload_bytes)"
                )));
                self.buffer.drain(..1);
                continue;
            }
            let total = self.header_len + size as usize;
            if self.buffer.len() < total {
                break;
            }
            let payload = self.buffer[self.header_len..total].to_vec();
            events.push(self.save(&payload));
            self.buffer.drain(..total);
        }
        events
    }

    fn save(&self, payload: &[u8]) -> ExtractEvent {
        let ts = chrono::Local::now().format("%Y%m%d_%H%M%S_%3f");
        let path = self
            .output_dir
            .join(format!("{}{}.{}", self.prefix, ts, self.ext));
        let result =
            std::fs::create_dir_all(&self.output_dir).and_then(|_| std::fs::write(&path, payload));
        match result {
            Ok(()) => ExtractEvent::Saved {
                path,
                len: payload.len(),
            },
            Err(e) => ExtractEvent::Failed(format!("Error saving extracted image: {e}")),
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_extractor(dir: &std::path::Path) -> Extractor {
        let paths = Paths {
            base: dir.to_path_buf(),
            config_file: dir.join("c.json"),
            state_file: dir.join("s.json"),
        };
        let cfg = ExtractConfig {
            output_directory: dir.display().to_string(),
            ..Default::default()
        };
        Extractor::new(Some(&cfg), &paths).0
    }

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0xAB, 0xCD, 0x12, 0x34, 0xCA, 0xFE, 0xBA, 0xBE];
        f.extend_from_slice(&[0u8; 9]); // pad to size_offset 17
        f.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        f.extend_from_slice(payload);
        f
    }

    #[test]
    fn extracts_frame_split_across_reads() {
        let dir = std::env::temp_dir().join(format!("rawtap_test_{}", std::process::id()));
        let mut ex = test_extractor(&dir);
        let mut stream = b"noise".to_vec();
        stream.extend(frame(b"BMpayload"));
        stream.extend(b"tail");
        let mut saved = Vec::new();
        for chunk in stream.chunks(3) {
            for ev in ex.feed(chunk) {
                if let ExtractEvent::Saved { path, len } = ev {
                    saved.push((std::fs::read(&path).unwrap(), len));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].0, b"BMpayload");
    }
}
