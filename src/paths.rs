//! Single owner of RawTap's on-disk locations (App Standard sections 3 and 5).
//! Nothing else calls `dirs::`.
//!
//! Everything RawTap writes lives under one folder, `~/.config/rawtap`, on
//! Linux, Windows and macOS alike (Linux honours `XDG_CONFIG_HOME`):
//! `settings.json` (your macros and options), `state.json` (what RawTap
//! remembers between runs), `debug/` (session and update logs), and the
//! traffic logs, captures and extracted images.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const APP_DIR_NAME: &str = "rawtap";
pub const SETTINGS_FILE: &str = "settings.json";
pub const STATE_FILE: &str = "state.json";

/// Versions up to 0.6.0 kept their files next to the executable (or in the
/// current folder) under these names.
const LEGACY_SETTINGS_FILE: &str = "rawtap_config.json";
const LEGACY_STATE_FILE: &str = "rawtap_state.json";

/// `~/.config/rawtap`, not created.
pub fn config_root() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    let base = dirs::config_dir().ok_or("Could not resolve the config directory")?;
    #[cfg(not(target_os = "linux"))]
    let base = dirs::home_dir()
        .ok_or("Could not resolve the home directory")?
        .join(".config");
    Ok(base.join(APP_DIR_NAME))
}

/// `~/.local/bin`, where install.sh puts the AppImage (Linux only).
#[cfg(not(windows))]
pub fn user_local_bin() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".local").join("bin"))
}

/// `~/.ssh`, where ssh looks for its keys (Windows: `%USERPROFILE%\\.ssh`).
pub fn ssh_dir() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("Could not resolve the home directory")?
        .join(".ssh"))
}

/// Where RawTap keeps its files. Relative folders in `settings.json` (logs,
/// captures, extracted images) are resolved against `base`.
pub struct Paths {
    pub base: PathBuf,
    pub settings_file: PathBuf,
    pub state_file: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Paths, String> {
        Ok(Paths::at(config_root()?))
    }

    pub fn at(base: PathBuf) -> Paths {
        Paths {
            settings_file: base.join(SETTINGS_FILE),
            state_file: base.join(STATE_FILE),
            base,
        }
    }

    /// `debug/`: the session, last-session and update logs.
    pub fn debug_dir(&self) -> PathBuf {
        self.base.join("debug")
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

/// What the one-time move from the 0.6.0 locations did, for the session log
/// and (once) the user.
#[derive(Debug, PartialEq, Eq)]
pub enum Migration {
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    /// The new file already exists; the old one is left alone.
    KeptNew {
        legacy: PathBuf,
        current: PathBuf,
    },
    Failed {
        from: PathBuf,
        to: PathBuf,
        error: String,
    },
}

impl Migration {
    pub fn describe(&self) -> String {
        match self {
            Migration::Moved { from, to } => {
                format!("Moved {} to {}", from.display(), to.display())
            }
            Migration::KeptNew { legacy, current } => format!(
                "Found {} from an older RawTap, but {} already exists, so the old file was left where it is",
                legacy.display(),
                current.display()
            ),
            Migration::Failed { from, to, error } => format!(
                "Could not move {} to {}: {error}",
                from.display(),
                to.display()
            ),
        }
    }
}

/// The folders 0.6.0 looked in: next to the executable first, then the
/// current folder, whichever held `rawtap_config.json`.
fn legacy_dirs() -> Vec<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let cwd = std::env::current_dir().ok();
    let mut dirs: Vec<PathBuf> = [exe_dir, cwd].into_iter().flatten().collect();
    dirs.dedup();
    dirs
}

/// Moves `rawtap_config.json` and `rawtap_state.json` from where 0.6.0 kept
/// them into `~/.config/rawtap`, once, before the session log opens. Logs and
/// captures stay where they were written.
pub fn migrate_legacy_files(paths: &Paths) -> Vec<Migration> {
    let Some(legacy_dir) = legacy_dirs()
        .into_iter()
        .find(|dir| dir.join(LEGACY_SETTINGS_FILE).is_file() && *dir != paths.base)
    else {
        return Vec::new();
    };
    [
        (LEGACY_SETTINGS_FILE, &paths.settings_file),
        (LEGACY_STATE_FILE, &paths.state_file),
    ]
    .into_iter()
    .filter_map(|(name, target)| migrate_file(&legacy_dir.join(name), target))
    .collect()
}

fn migrate_file(legacy: &Path, target: &Path) -> Option<Migration> {
    if !legacy.is_file() {
        return None;
    }
    if target.exists() {
        return Some(Migration::KeptNew {
            legacy: legacy.to_path_buf(),
            current: target.to_path_buf(),
        });
    }
    let failed = |error: io::Error| Migration::Failed {
        from: legacy.to_path_buf(),
        to: target.to_path_buf(),
        error: error.to_string(),
    };
    if let Some(parent) = target.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Some(failed(error));
    }
    // Rename first; across drives (or out of a read-only folder) copy instead.
    if fs::rename(legacy, target).is_err() {
        if let Err(error) = fs::copy(legacy, target) {
            return Some(failed(error));
        }
        if let Err(error) = fs::remove_file(legacy) {
            log::warn!(
                "Copied {} to {} but could not remove the old file: {error}",
                legacy.display(),
                target.display()
            );
        }
    }
    Some(Migration::Moved {
        from: legacy.to_path_buf(),
        to: target.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rawtap_paths_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn config_root_is_the_rawtap_folder_under_dot_config() {
        let root = config_root().unwrap();
        assert!(root.ends_with(APP_DIR_NAME));
        #[cfg(not(target_os = "linux"))]
        assert!(root.parent().unwrap().ends_with(".config"));
    }

    #[test]
    fn paths_name_the_standard_files() {
        let paths = Paths::at(PathBuf::from("base"));
        assert_eq!(paths.settings_file, Path::new("base").join("settings.json"));
        assert_eq!(paths.state_file, Path::new("base").join("state.json"));
        assert_eq!(paths.debug_dir(), Path::new("base").join("debug"));
    }

    #[test]
    fn legacy_file_moves_into_a_missing_folder() {
        let dir = temp_dir("move");
        let legacy = dir.join("old").join(LEGACY_SETTINGS_FILE);
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "{}").unwrap();
        let target = dir.join("new").join(SETTINGS_FILE);
        assert!(matches!(
            migrate_file(&legacy, &target),
            Some(Migration::Moved { .. })
        ));
        assert!(!legacy.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "{}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn existing_new_file_is_never_overwritten() {
        let dir = temp_dir("keep");
        let legacy = dir.join(LEGACY_STATE_FILE);
        let target = dir.join(STATE_FILE);
        fs::write(&legacy, "old").unwrap();
        fs::write(&target, "new").unwrap();
        assert!(matches!(
            migrate_file(&legacy, &target),
            Some(Migration::KeptNew { .. })
        ));
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(fs::read_to_string(&legacy).unwrap(), "old");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nothing_to_move_reports_nothing() {
        let dir = temp_dir("none");
        assert_eq!(
            migrate_file(&dir.join(LEGACY_SETTINGS_FILE), &dir.join(SETTINGS_FILE)),
            None
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
