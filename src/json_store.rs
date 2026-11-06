//! Reading and writing RawTap's JSON files (App Standard section 5).
//!
//! - Writes are atomic: a temp file in the same folder, flushed to disk, then
//!   renamed over the original, so a crash mid-save never leaves a cut-off
//!   file.
//! - A file that can't be parsed is renamed `<file>.invalid-<timestamp>` and
//!   the caller starts from defaults and tells the user once.
//! - A file that exists but can't be read (permissions, I/O) is an error the
//!   caller must stop on, so RawTap never overwrites a file it couldn't read.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

pub enum Loaded<T> {
    Missing,
    Parsed(T),
    /// The file didn't parse and was set aside at `backup`.
    Quarantined {
        backup: PathBuf,
        error: String,
    },
}

/// Reads and parses `path`. `Err` means the file exists but couldn't be read
/// or set aside: stop rather than overwrite it.
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<Loaded<T>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Loaded::Missing),
        Err(error) => return Err(format!("Could not read {}: {error}", path.display())),
    };
    match serde_json::from_str(&text) {
        Ok(value) => Ok(Loaded::Parsed(value)),
        Err(parse_error) => {
            let backup = quarantine(path)?;
            Ok(Loaded::Quarantined {
                backup,
                error: parse_error.to_string(),
            })
        }
    }
}

/// Renames an unparseable file to `<file>.invalid-<timestamp>`.
fn quarantine(path: &Path) -> Result<PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".invalid-{stamp}"));
    let backup = path.with_file_name(name);
    fs::rename(path, &backup).map_err(|error| {
        format!(
            "{} could not be parsed and could not be set aside as {}: {error}",
            path.display(),
            backup.display()
        )
    })?;
    Ok(backup)
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("Could not encode {}: {error}", path.display()))?;
    write_atomically(path, text.as_bytes())
}

/// Writes `contents` to `path` through a temp file in the same folder.
pub fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    let fail = |error: std::io::Error| format!("Could not save {}: {error}", path.display());
    let dir = path
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", path.display()))?;
    fs::create_dir_all(dir).map_err(fail)?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(path.file_name().unwrap_or_default());
    temp_name.push(format!(".tmp-{}", std::process::id()));
    let temp = dir.join(temp_name);
    let result = File::create(&temp)
        .and_then(|mut file| {
            file.write_all(contents)?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&temp, path));
    if let Err(error) = result {
        // The temp file is only ours; the original is untouched.
        if let Err(cleanup) = fs::remove_file(&temp)
            && cleanup.kind() != ErrorKind::NotFound
        {
            log::warn!("Could not remove {}: {cleanup}", temp.display());
        }
        return Err(fail(error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Sample {
        name: String,
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rawtap_store_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_then_read_round_trips_and_leaves_no_temp_file() {
        let dir = temp_dir("round");
        let path = dir.join("state.json");
        let value = Sample { name: "x".into() };
        write_json(&path, &value).unwrap();
        assert!(matches!(read::<Sample>(&path).unwrap(), Loaded::Parsed(v) if v == value));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_file_is_missing() {
        let dir = temp_dir("missing");
        assert!(matches!(
            read::<Sample>(&dir.join("none.json")).unwrap(),
            Loaded::Missing
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unparseable_file_is_set_aside() {
        let dir = temp_dir("bad");
        let path = dir.join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        let Loaded::Quarantined { backup, .. } = read::<Sample>(&path).unwrap() else {
            panic!("expected the file to be quarantined");
        };
        assert!(!path.exists());
        assert!(
            backup
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("settings.json.invalid-")
        );
        assert_eq!(fs::read_to_string(&backup).unwrap(), "{ not json");
        fs::remove_dir_all(&dir).unwrap();
    }
}
