//! In-app updates, per the Release and Update Standard
//! (https://github.com/jackbrumley/jackbrumley.github.io/blob/main/RELEASE_STANDARD.md),
//! "Terminal apps": the check reads GitHub's latest release, and updating
//! re-runs the same one-line installer a new user runs.
//!
//! - Linux: RawTap leaves the TUI and replaces itself with the installer in
//!   the same terminal, so its output is visible; install.sh starts the new
//!   version in that terminal. On failure the installed version starts again
//!   with `--update-failed`.
//! - Windows: a running exe can't be replaced, so the installer runs in its
//!   own console window and RawTap exits; install.ps1 starts the new version.

use serde::Deserialize;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const GITHUB_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/jackbrumley/rawtap/releases/latest";
const INSTALL_SITE: &str = "https://brumley.info/rawtap";
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: String,
}

#[derive(Clone, Debug, Default)]
pub enum Status {
    #[default]
    NotChecked,
    Checking,
    Checked(UpdateCheck),
    Failed(String),
}

/// The latest check's result, for the F1 menu and the start screen. One
/// owner: only `set_status` writes it.
static STATUS: Mutex<Status> = Mutex::new(Status::NotChecked);

/// When a check last finished, as shown in F1 → Updates ("Last checked").
/// Kept in state.json between runs.
static LAST_CHECKED: Mutex<Option<String>> = Mutex::new(None);

pub fn last_checked() -> Option<String> {
    LAST_CHECKED.lock().ok().and_then(|t| t.clone())
}

pub fn set_last_checked(time: Option<String>) {
    if let Ok(mut current) = LAST_CHECKED.lock() {
        *current = time;
    }
}

pub fn status() -> Status {
    STATUS.lock().map(|s| s.clone()).unwrap_or_default()
}

fn set_status(status: Status) {
    if let Ok(mut current) = STATUS.lock() {
        *current = status;
    }
}

/// The newer version, when the last check found one.
pub fn available() -> Option<UpdateCheck> {
    match status() {
        Status::Checked(check) if check.update_available => Some(check),
        _ => None,
    }
}

#[derive(Deserialize)]
struct GitHubLatestRelease {
    tag_name: String,
    html_url: String,
}

/// Asks GitHub for the latest release. Blocking; `spawn_check` runs it off
/// the UI thread.
pub fn check() -> Result<UpdateCheck, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    log::info!("Checking updates from {GITHUB_LATEST_RELEASE_API_URL}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(CHECK_TIMEOUT))
        .build()
        .into();
    let release: GitHubLatestRelease = agent
        .get(GITHUB_LATEST_RELEASE_API_URL)
        .header("User-Agent", "rawtap-update-checker")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| match error {
            ureq::Error::StatusCode(404) => {
                "No RawTap release is published on GitHub yet".to_string()
            }
            error => format!("Could not reach GitHub to check for updates: {error}"),
        })?
        .body_mut()
        .read_json()
        .map_err(|error| format!("Could not read GitHub's latest release: {error}"))?;
    let latest_version = normalize_version(&release.tag_name)
        .ok_or_else(|| format!("Invalid latest release tag: {}", release.tag_name))?;
    let current = parse_version(&current_version)
        .ok_or_else(|| format!("Invalid current version: {current_version}"))?;
    let latest = parse_version(&latest_version)
        .ok_or_else(|| format!("Invalid latest version: {latest_version}"))?;
    let update_available = latest > current;
    log::info!(
        "Update check: current={current_version} latest={latest_version} available={update_available}"
    );
    Ok(UpdateCheck {
        current_version,
        latest_version,
        update_available,
        release_url: release.html_url,
    })
}

/// Checks in a background thread. The result is stored for the menu and
/// also sent on the returned channel, for the app to react to.
pub fn spawn_check() -> Receiver<Result<UpdateCheck, String>> {
    set_status(Status::Checking);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = check();
        set_last_checked(Some(
            chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        ));
        set_status(match &result {
            Ok(check) => Status::Checked(check.clone()),
            Err(error) => {
                log::warn!("{error}");
                Status::Failed(error.clone())
            }
        });
        // The app may have quit meanwhile; then nobody needs the result.
        let _ = tx.send(result);
    });
    rx
}

/// The message `--update-failed` shows.
pub fn update_failed_message() -> String {
    let log = crate::paths::config_root()
        .map(|root| root.join("debug").join("update.log").display().to_string())
        .unwrap_or_else(|_| "~/.config/rawtap/debug/update.log".to_string());
    format!(
        "RawTap could not install the update, so this version is still installed. Update log: {log}"
    )
}

/// Runs the update. Call it with the terminal back to normal (no TUI).
/// On Linux this only returns on failure (the process becomes the
/// installer); on Windows it returns once the installer window is started,
/// and RawTap should then exit.
pub fn run_update() -> Result<(), String> {
    log::info!("Starting the update");
    platform::run_update()
}

/// The executable to start again with `--update-failed`: the AppImage file
/// itself when running as one (its mounted executable disappears on exit).
fn relaunch_target() -> Option<std::path::PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Some(std::path::PathBuf::from(appimage));
    }
    std::env::current_exe().ok()
}

/// Wraps `value` in single quotes for bash, which takes everything inside
/// literally (an embedded quote closes, escapes and reopens).
#[cfg(any(not(windows), test))]
fn bash_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Wraps `value` in single quotes for PowerShell, which escapes an embedded
/// quote by doubling it.
#[cfg(any(windows, test))]
fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The install command that replaces this installation: an AppImage install
/// (`$APPIMAGE` set, or the binary under `~/.local/bin`) stays an AppImage
/// install.
#[cfg(any(not(windows), test))]
fn unix_update_command(is_appimage: bool) -> String {
    let base = format!("curl --proto '=https' --tlsv1.2 -sSf {INSTALL_SITE}/install.sh | bash");
    if is_appimage {
        format!("{base} -s -- --appimage")
    } else {
        base
    }
}

/// Runs the installer; if it fails, starts the installed version again with
/// `--update-failed` so the user is told. `pipefail` makes a failed download
/// fail the update rather than piping an empty script into bash.
#[cfg(any(not(windows), test))]
fn unix_update_script(is_appimage: bool, relaunch: Option<&std::path::Path>) -> String {
    let relaunch = match relaunch {
        Some(path) => {
            let path = bash_single_quote(&path.to_string_lossy());
            format!("if [ -x {path} ]; then exec {path} --update-failed; fi")
        }
        None => "echo \"[rawtap] No installed version found to start again\"".to_string(),
    };
    format!(
        r#"set -o pipefail
{install}
status=$?
if [ "$status" -ne 0 ]; then
  echo "[rawtap] Update failed (exit $status); starting the installed version again"
  {relaunch}
fi
"#,
        install = unix_update_command(is_appimage),
    )
}

/// Windows counterpart of `unix_update_script`. install.ps1 runs from a file
/// rather than through `iex`, so its `exit 1` on failure ends the script, not
/// this updater, and sets `$LASTEXITCODE`.
#[cfg(any(windows, test))]
fn windows_update_script(relaunch: Option<&std::path::Path>) -> String {
    let relaunch = match relaunch {
        Some(path) => {
            let path = powershell_single_quote(&path.to_string_lossy());
            format!(
                "if (Test-Path -LiteralPath {path}) {{ Start-Process -FilePath {path} -ArgumentList '--update-failed' }}"
            )
        }
        None => "Write-Output '[rawtap] No installed version found to start again'".to_string(),
    };
    format!(
        r#"$installer = Join-Path $env:TEMP 'rawtap-update-install.ps1'
try {{
  Invoke-RestMethod -Uri '{INSTALL_SITE}/install.ps1' -OutFile $installer -ErrorAction Stop
  $LASTEXITCODE = 0
  & $installer
  $succeeded = $LASTEXITCODE -eq 0
}} catch {{
  Write-Output "[rawtap] ERROR: $_"
  $succeeded = $false
}}
if (-not $succeeded) {{
  Write-Output '[rawtap] Update failed; starting the installed version again'
  {relaunch}
  Read-Host 'Press Enter to close'
}}
"#
    )
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    fn is_appimage() -> bool {
        if std::env::var_os("APPIMAGE").is_some() {
            return true;
        }
        matches!(
            (std::env::current_exe(), crate::paths::user_local_bin()),
            (Ok(exe), Some(dir)) if exe.starts_with(&dir)
        )
    }

    /// Replaces this process with the update script, in this terminal.
    pub fn run_update() -> Result<(), String> {
        use std::os::unix::process::CommandExt;
        let script = unix_update_script(is_appimage(), relaunch_target().as_deref());
        log::info!("Running the update:\n{script}");
        log::logger().flush();
        let error = std::process::Command::new("bash")
            .arg("-c")
            .arg(script)
            .exec();
        Err(format!("Could not start the update: {error}"))
    }
}

#[cfg(windows)]
mod platform {
    use super::*;

    /// Starts the update in its own console window; RawTap then exits so the
    /// installer can replace rawtap.exe.
    pub fn run_update() -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let script = windows_update_script(relaunch_target().as_deref());
        log::info!("Running the update:\n{script}");
        std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Could not start the update: {error}"))
    }
}

fn normalize_version(raw: &str) -> Option<String> {
    let parsed = parse_version(raw.trim().trim_start_matches('v'))?;
    Some(format!("{}.{}.{}", parsed.0, parsed.1, parsed.2))
}

/// Plain `MAJOR.MINOR.PATCH` (Release Standard section 2).
fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut segments = value.trim().split('.');
    let major = segments.next()?.parse::<u64>().ok()?;
    let minor = segments.next()?.parse::<u64>().ok()?;
    let patch = segments.next()?.parse::<u64>().ok()?;
    if segments.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn bash_quoting_keeps_paths_literal() {
        assert_eq!(bash_single_quote("/usr/bin/rawtap"), "'/usr/bin/rawtap'");
        assert_eq!(
            bash_single_quote("/home/o'brien/Raw Tap $HOME.AppImage"),
            r"'/home/o'\''brien/Raw Tap $HOME.AppImage'"
        );
    }

    #[test]
    fn powershell_quoting_doubles_embedded_quotes() {
        assert_eq!(
            powershell_single_quote(r"C:\Users\o'brien\AppData\Local\rawtap\rawtap.exe"),
            r"'C:\Users\o''brien\AppData\Local\rawtap\rawtap.exe'"
        );
    }

    #[test]
    fn versions_normalize_from_tags() {
        assert_eq!(normalize_version("v0.7.0").as_deref(), Some("0.7.0"));
        assert_eq!(normalize_version("1.2"), None);
        assert_eq!(normalize_version("v1.2.3-beta.1"), None);
        assert!(parse_version("0.10.0") > parse_version("0.9.9"));
    }

    #[test]
    fn unix_update_script_fails_on_a_failed_download_and_relaunches() {
        let script = unix_update_script(false, Some(Path::new("/usr/bin/rawtap")));
        assert!(script.starts_with("set -o pipefail\n"));
        assert!(script.contains("https://brumley.info/rawtap/install.sh | bash\n"));
        assert!(script.contains("exec '/usr/bin/rawtap' --update-failed"));
    }

    #[test]
    fn appimage_install_stays_an_appimage_install() {
        let script = unix_update_script(true, None);
        assert!(script.contains("install.sh | bash -s -- --appimage"));
    }

    #[test]
    fn windows_update_script_runs_the_installer_from_a_file_and_relaunches() {
        let script = windows_update_script(Some(Path::new(r"C:\rawtap\rawtap.exe")));
        assert!(script.contains("https://brumley.info/rawtap/install.ps1"));
        assert!(script.contains("& $installer"));
        assert!(script.contains("--update-failed"));
    }
}
