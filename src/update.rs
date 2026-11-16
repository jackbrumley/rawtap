//! In-app updates, per the Release and Update Standard
//! (https://github.com/jackbrumley/jackbrumley.github.io/blob/main/RELEASE_STANDARD.md),
//! section 7, "Terminal apps": the check reads GitHub's latest release, and
//! the website owns the update. RawTap only downloads `docs/update.sh` or
//! `docs/update.ps1` from the install site and runs it; that script runs the
//! same installer a new user runs and starts RawTap again, so a fix to the
//! update reaches every installed version without a release.
//!
//! - Linux: RawTap leaves the TUI and replaces itself with the updater in the
//!   same terminal, so its output is visible; update.sh starts the new
//!   version in that terminal.
//! - Windows: a running exe can't be replaced, so the updater runs in its own
//!   console window and RawTap exits; update.ps1 starts the new version.
//! - On failure the installed version starts again with `--update-failed`.
//! - The hidden `--update` flag runs the update without a check; the release
//!   install test uses it, with `RAWTAP_UPDATE_BASE_URL` set to the tag's
//!   `docs/`.

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

/// Where the update scripts are downloaded from. `RAWTAP_UPDATE_BASE_URL`
/// replaces it (https only); only the release install test sets it, to run
/// the tag's own scripts.
const UPDATE_BASE_URL: &str = INSTALL_SITE;
const UPDATE_BASE_URL_OVERRIDE_VARIABLE: &str = "RAWTAP_UPDATE_BASE_URL";

/// The first line `docs/update.sh` and `docs/update.ps1` print.
#[cfg(test)]
const UPDATE_STARTED_MARKER: &str = "[rawtap-update] started";

/// Where to send the user when the updater can't even be downloaded.
const RELEASES_PAGE: &str = "https://github.com/jackbrumley/rawtap/releases/latest";

/// `debug/update.log`: the update scripts and the installers write it.
fn update_log_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::paths::config_root()?
        .join("debug")
        .join("update.log"))
}

/// The message `--update-failed` shows.
pub fn update_failed_message() -> String {
    let log = update_log_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "~/.config/rawtap/debug/update.log".to_string());
    format!(
        "RawTap could not install the update, so this version is still installed. Update log: {log}"
    )
}

/// Runs the update (Release Standard section 7: the website owns the
/// update). RawTap only downloads `<site>/update.sh` or `update.ps1` and runs
/// it; everything else, from the installer options to starting RawTap again,
/// is in that script, so a fix there reaches every installed version.
///
/// Call it with the terminal back to normal (no TUI). On Linux this only
/// returns on failure (the process becomes the updater, in this terminal);
/// on Windows it returns once the updater's window is started, and RawTap
/// should then exit so the installer can replace rawtap.exe.
pub fn run_update() -> Result<(), String> {
    let base_url = parse_update_base_url(std::env::var(UPDATE_BASE_URL_OVERRIDE_VARIABLE).ok())?;
    let relaunch = relaunch_target()
        .ok_or("Could not find RawTap's own executable, to start it again after the update")?;
    let log = update_log_path()?;
    log::info!("Starting the updater from {base_url}");
    log::logger().flush();
    platform::start_updater(&base_url, &relaunch, &log)
}

/// The default base URL, or the override, which must be https and loses any
/// trailing slash.
fn parse_update_base_url(override_url: Option<String>) -> Result<String, String> {
    let Some(override_url) = override_url else {
        return Ok(UPDATE_BASE_URL.to_string());
    };
    let trimmed = override_url.trim().trim_end_matches('/');
    if !trimmed.starts_with("https://") {
        return Err(format!(
            "{UPDATE_BASE_URL_OVERRIDE_VARIABLE} must be an https:// URL, got '{override_url}'"
        ));
    }
    Ok(trimmed.to_string())
}

/// The executable to start again after the update: the AppImage file itself
/// when running as one (its mounted executable disappears on exit).
fn relaunch_target() -> Option<std::path::PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Some(std::path::PathBuf::from(appimage));
    }
    std::env::current_exe().ok()
}

/// Wraps `value` in single quotes for PowerShell, which escapes an embedded
/// quote by doubling it.
#[cfg(any(windows, test))]
fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(not(windows))]
mod platform {
    use std::os::unix::process::CommandExt;
    use std::path::Path;

    /// Downloads update.sh and replaces itself with it, in this terminal. The
    /// URL and paths are positional arguments, never shell code. A failed
    /// download says so (on screen and in the update log) and starts this
    /// version again with `--update-failed`, so RawTap never closes into
    /// nothing.
    const LAUNCHER: &str = r#"if ! updater="$(curl --proto '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 10 "$1/update.sh")"; then
  message="[rawtap-update] Could not download $1/update.sh, so nothing was changed. Download the new version from $4 instead."
  printf '%s\n' "$message"
  mkdir -p "$(dirname "$3")" 2>/dev/null && printf '[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$message" > "$3"
  [ "${RAWTAP_NO_RELAUNCH:-}" = 1 ] && exit 1
  exec "$2" --update-failed
fi
exec bash -c "$updater" rawtap-update --base-url "$1" --relaunch "$2"
"#;

    pub fn start_updater(base_url: &str, relaunch: &Path, log: &Path) -> Result<(), String> {
        let error = std::process::Command::new("bash")
            .arg("-c")
            .arg(LAUNCHER)
            .arg("rawtap-update")
            .arg(base_url)
            .arg(relaunch)
            .arg(log)
            .arg(super::RELEASES_PAGE)
            .exec();
        Err(format!("Could not start the updater: {error}"))
    }
}

#[cfg(windows)]
mod platform {
    use super::powershell_single_quote;
    use std::path::Path;

    /// Starts PowerShell in a console window of its own, so the update's
    /// output stays visible after RawTap exits. It downloads update.ps1 and
    /// runs it as a script block. A failed download says so (in the window
    /// and the update log), starts this version again with `--update-failed`
    /// and keeps the window open, so RawTap never closes into nothing.
    pub fn start_updater(base_url: &str, relaunch: &Path, log: &Path) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

        let script = format!(
            r#"[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
try {{
  $updater = Invoke-RestMethod -Uri {script_url} -ErrorAction Stop
}} catch {{
  $message = '[rawtap-update] Could not download ' + {script_url} + ', so nothing was changed. Download the new version from ' + {releases} + " instead. ($_)"
  Write-Output $message
  try {{
    New-Item -ItemType Directory -Force -Path (Split-Path {log}) | Out-Null
    Set-Content -LiteralPath {log} -Value $message
  }} catch {{}}
  if ($env:RAWTAP_NO_RELAUNCH -ne '1') {{
    Start-Process -FilePath {relaunch} -ArgumentList '--update-failed'
    Read-Host 'Press Enter to close'
  }}
  exit 1
}}
& ([scriptblock]::Create($updater)) -BaseUrl {base_url} -Relaunch {relaunch}
"#,
            script_url = powershell_single_quote(&format!("{base_url}/update.ps1")),
            releases = powershell_single_quote(super::RELEASES_PAGE),
            log = powershell_single_quote(&log.to_string_lossy()),
            base_url = powershell_single_quote(base_url),
            relaunch = powershell_single_quote(&relaunch.to_string_lossy()),
        );
        std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            // Windows PowerShell builds its own module path when none is set.
            // An inherited one (RawTap started from Git Bash, say, with
            // PowerShell 7's module folders on it) makes it load PowerShell
            // 7's modules, which fail, so install.ps1 can't find Get-FileHash.
            .env_remove("PSModulePath")
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Could not start the updater: {error}"))
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
    fn update_base_url_defaults_to_the_website() {
        assert_eq!(
            parse_update_base_url(None).unwrap(),
            "https://brumley.info/rawtap"
        );
    }

    #[test]
    fn update_base_url_override_must_be_https() {
        assert_eq!(
            parse_update_base_url(Some(
                "https://raw.githubusercontent.com/jackbrumley/rawtap/v1.2.3/docs/".to_string()
            ))
            .unwrap(),
            "https://raw.githubusercontent.com/jackbrumley/rawtap/v1.2.3/docs"
        );
        assert!(parse_update_base_url(Some("http://example.com".to_string())).is_err());
    }

    /// The scripts' first output is the started line: their contract with
    /// every shipped version and with the release install test.
    #[test]
    fn update_scripts_print_the_started_marker_first() {
        let first_output = |script: &str, command: &str| {
            script
                .lines()
                .find(|line| line.starts_with(command))
                .map(str::to_string)
        };
        assert_eq!(
            first_output(include_str!("../docs/update.ps1"), "Say "),
            Some(format!("Say \"{UPDATE_STARTED_MARKER}\""))
        );
        assert_eq!(
            first_output(include_str!("../docs/update.sh"), "say "),
            Some(format!("say \"{UPDATE_STARTED_MARKER}\""))
        );
    }
}
