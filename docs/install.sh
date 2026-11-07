#!/usr/bin/env bash
set -euo pipefail

# RawTap one-line installer for Linux.
#   curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/rawtap/install.sh | bash
# The in-app updater re-runs this same script, so keep its options stable.
# Follows the Release and Update Standard; see docs/RELEASE.md.

REPO="jackbrumley/rawtap"
BIN_NAME="rawtap"
# App ID (RELEASE_STANDARD.md "App Identity"): the launcher is named
# <APP_ID>.desktop. Never change it.
APP_ID="info.brumley.rawtap"
INSTALL_BASE_URL="https://brumley.info/rawtap"

# GitHub API request headers. RAWTAP_GITHUB_TOKEN is optional: anonymous
# requests are limited to 60 an hour per IP address.
GITHUB_API_HEADERS=(-H "Accept: application/vnd.github+json")
if [[ -n "${RAWTAP_GITHUB_TOKEN:-}" ]]; then
  GITHUB_API_HEADERS+=(-H "Authorization: Bearer ${RAWTAP_GITHUB_TOKEN}")
fi

usage() {
  cat <<EOF
RawTap installer

Usage:
  install.sh [--appimage] [--system] [--version <tag>] [--yes] [--relaunch|--no-relaunch] [--insecure-skip-verify]

Default: system-wide install via apt/dnf (asks for your password). Falls back to
the AppImage if no supported package manager or root access is available.
Either way, your user is added to the serial-port group (dialout, or uucp on
Arch) so RawTap can open serial ports.

Options:
  --appimage              Install the AppImage to ~/.local/bin/rawtap
  --system                Force a system-wide install via apt/dnf
  --version <tag>         Install a specific release tag (e.g. v0.7.0)
  --yes                   Skip the confirmation prompt
  --relaunch              Start RawTap afterwards even if RAWTAP_NO_RELAUNCH is set
  --no-relaunch           Do not start RawTap after installing
  --insecure-skip-verify  Skip checksum verification (not recommended)
  -h, --help              Show this help message

Environment:
  RAWTAP_GITHUB_TOKEN     GitHub token for API requests (avoids the anonymous rate limit)
  RAWTAP_NO_RELAUNCH      Set to 1 to not start RawTap after installing

Examples:
  curl --proto '=https' --tlsv1.2 -sSf ${INSTALL_BASE_URL}/install.sh | bash
  curl --proto '=https' --tlsv1.2 -sSf ${INSTALL_BASE_URL}/install.sh | bash -s -- --appimage
EOF
}

LOG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/rawtap/debug"
mkdir -p "$LOG_DIR" 2>/dev/null || true
LOG_FILE="${LOG_DIR}/update.log"

log() {
  printf "[rawtap] %s\n" "$*"
  printf "[%s] [rawtap] %s\n" "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE" 2>/dev/null || true
}

warn() {
  printf "[rawtap] WARNING: %s\n" "$*" >&2
  printf "[%s] [rawtap] WARNING: %s\n" "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE" 2>/dev/null || true
}

fail() {
  printf "[rawtap] ERROR: %b\n" "$*" >&2
  printf "[%s] [rawtap] ERROR: %b\n" "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >> "$LOG_FILE" 2>/dev/null || true
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

optional_cmd() {
  command -v "$1" >/dev/null 2>&1
}

has_display() {
  [[ -n "${DISPLAY:-}" || -n "${WAYLAND_DISPLAY:-}" ]]
}

run_as_root() {
  if [[ "$EUID" -eq 0 ]]; then
    "$@"
  elif optional_cmd sudo && sudo -n true 2>/dev/null; then
    sudo "$@"
  elif [[ -t 0 ]] && optional_cmd sudo; then
    sudo "$@"
  elif optional_cmd pkexec && has_display; then
    pkexec "$@"
  elif optional_cmd sudo; then
    sudo "$@"
  else
    fail "root permissions required but neither sudo nor pkexec is available"
  fi
}

# Returns 0 when root commands can run (already root, cached sudo, an
# interactive sudo prompt, or a graphical pkexec prompt).
acquire_root() {
  if [[ "$EUID" -eq 0 ]]; then
    return 0
  fi
  if optional_cmd sudo && sudo -n true 2>/dev/null; then
    return 0
  fi
  if [[ -t 0 ]]; then
    optional_cmd sudo || return 1
    sudo -v
  elif optional_cmd pkexec && has_display; then
    return 0
  elif optional_cmd sudo; then
    sudo -n true 2>/dev/null || return 1
  else
    return 1
  fi
}

stop_running_process() {
  if optional_cmd pgrep && pgrep -x "$BIN_NAME" >/dev/null 2>&1; then
    log "Closing running RawTap"
    if optional_cmd pkill; then
      pkill -TERM -x "$BIN_NAME" >/dev/null 2>&1 || true
      sleep 1
      pkill -KILL -x "$BIN_NAME" >/dev/null 2>&1 || true
    fi
  fi
}

resolve_arch() {
  case "$(uname -m)" in
    x86_64|amd64) printf "x64" ;;
    aarch64|arm64) fail "ARM64 builds of RawTap are not available yet; only x64 (x86_64) is supported." ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
  esac
}

detect_package_manager() {
  if optional_cmd dnf; then
    printf "dnf"
  elif optional_cmd apt-get; then
    printf "apt"
  else
    printf "none"
  fi
}

fetch_latest_release_tag() {
  local tag
  tag="$(curl -fsL --retry 3 --connect-timeout 10 "${GITHUB_API_HEADERS[@]}" \
    "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -o '"tag_name": *"[^"]*"' | head -n1 | sed 's/.*"\([^"]*\)"$/\1/')" || true
  [[ -n "$tag" ]] || fail "could not determine the latest release from GitHub (you may be rate limited; try again in a few minutes)"
  printf "%s" "$tag"
}

# GitHub computes a SHA-256 digest for every uploaded release asset and
# exposes it through the releases API; that is the source of truth.
verify_checksum() {
  local file="$1" asset_name="$2"
  local release_json expected actual
  release_json="$(curl -fsL --retry 3 --connect-timeout 10 "${GITHUB_API_HEADERS[@]}" \
    "https://api.github.com/repos/${REPO}/releases/tags/${release_tag}")" \
    || fail "could not fetch the checksum for ${asset_name} from GitHub. Try again later, or re-run with --insecure-skip-verify."
  if optional_cmd python3; then
    expected="$(printf "%s" "$release_json" | python3 -c '
import json, sys
name = sys.argv[1]
for asset in json.load(sys.stdin).get("assets", []):
    if asset.get("name") == name:
        print(asset.get("digest") or "")
' "$asset_name" 2>/dev/null)" || true
  else
    # No JSON parser: each asset lists "digest" shortly before its
    # "browser_download_url", with only flat fields in between.
    expected="$(printf "%s" "$release_json" | tr -d '\n' \
      | grep -o "\"digest\": *\"sha256:[0-9a-f]*\"[^{}]*\"browser_download_url\": *\"[^\"]*/${asset_name//./\\.}\"" \
      | grep -o 'sha256:[0-9a-f]*' | head -n1)" || true
  fi
  expected="${expected#sha256:}"
  [[ -n "$expected" ]] || fail "GitHub did not provide a checksum for ${asset_name}. Re-run with --insecure-skip-verify to bypass verification."
  actual="$(sha256sum "$file" | awk '{print $1}')"
  [[ "$expected" == "$actual" ]] || fail "checksum mismatch (expected $expected, got $actual)"
  log "Checksum verified"
}

# Serial ports belong to the dialout group (uucp on Arch). Without it RawTap
# runs but can't open a serial port. Membership takes effect at the next login.
serial_group() {
  local group
  for group in dialout uucp; do
    if getent group "$group" >/dev/null 2>&1; then
      printf "%s" "$group"
      return 0
    fi
  done
  return 1
}

add_user_to_serial_group() {
  local group user
  user="${SUDO_USER:-$(id -un)}"
  if ! group="$(serial_group)"; then
    warn "no dialout or uucp group found; check your distribution's serial port permissions."
    return 0
  fi
  # /etc/group, not `id`: a membership added earlier only shows in `id` after a new login.
  if getent group "$group" | cut -d: -f4 | tr ',' '\n' | grep -qx "$user"; then
    log "${user} can already use serial ports (${group} group)"
    return 0
  fi
  log "Adding ${user} to the ${group} group so RawTap can open serial ports (needs your password)"
  if ! acquire_root || ! run_as_root usermod -aG "$group" "$user"; then
    warn "could not add ${user} to the ${group} group, so serial ports may be refused."
    warn "Fix it with: sudo usermod -aG ${group} ${user}   (then log out and back in)"
    return 0
  fi
  log "Added ${user} to ${group}. Log out and back in for it to take effect."
}

remove_user_appimage() {
  local user_binary="${HOME}/.local/bin/${BIN_NAME}"
  local user_desktop="${HOME}/.local/share/applications/${APP_ID}.desktop"
  local user_icon="${HOME}/.local/share/icons/hicolor/256x256/apps/${BIN_NAME}.png"
  for path in "$user_binary" "$user_desktop" "$user_icon"; do
    if [[ -e "$path" || -L "$path" ]]; then
      log "Removing previous user install: ${path}"
      rm -f "$path"
    fi
  done
}

install_system=false
install_appimage=false
non_interactive=false
skip_verify=false
relaunch=true
if [[ "${RAWTAP_NO_RELAUNCH:-}" == "1" || "${RAWTAP_NO_RELAUNCH:-}" == "true" ]]; then
  relaunch=false
fi
version=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --system) install_system=true ;;
    --appimage) install_appimage=true ;;
    --relaunch) relaunch=true ;;
    --no-relaunch) relaunch=false ;;
    --version)
      [[ $# -ge 2 ]] || fail "--version requires a value"
      version="$2"
      shift
      ;;
    --yes) non_interactive=true ;;
    --insecure-skip-verify) skip_verify=true ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unknown option: $1" ;;
  esac
  shift
done

[[ "$(uname -s)" == "Linux" ]] || fail "this installer is for Linux. On Windows, see ${INSTALL_BASE_URL}/"

need_cmd curl
need_cmd sha256sum

# Piped into bash (curl | bash), stdin is the script, so prompts can't be answered.
if [[ ! -t 0 ]]; then
  non_interactive=true
fi

arch="$(resolve_arch)"
pm="$(detect_package_manager)"

if [[ "$install_appimage" == true ]]; then
  install_system=false
elif [[ "$install_system" == true ]]; then
  [[ "$pm" != "none" ]] || fail "no supported package manager found (dnf/apt). Use --appimage instead."
  log "System install requested; root access required"
  acquire_root || fail "could not get root access for a system install"
elif [[ "$pm" != "none" ]]; then
  log "Package manager '${pm}' detected; asking for root access"
  if acquire_root; then
    install_system=true
  elif [[ -x "/usr/bin/${BIN_NAME}" ]]; then
    fail "root access is required to update the existing system install in /usr/bin"
  else
    log "Root access unavailable; installing the AppImage instead"
  fi
else
  log "No supported package manager found; installing the AppImage"
fi

if [[ "$install_system" == true ]]; then
  if [[ "$pm" == "dnf" ]]; then package_ext="rpm"; else package_ext="deb"; fi
else
  package_ext="AppImage"
fi

if [[ -n "$version" ]]; then
  release_tag="$version"
else
  release_tag="$(fetch_latest_release_tag)"
fi

asset_name="${BIN_NAME}-${release_tag#v}-linux-${arch}.${package_ext}"
asset_url="https://github.com/${REPO}/releases/download/${release_tag}/${asset_name}"

log "Release: ${release_tag}"
if [[ "$install_system" == true ]]; then
  log "Install mode: system-wide (${pm}, .${package_ext})"
else
  log "Install mode: user (AppImage in ~/.local/bin)"
fi

if [[ "$non_interactive" == false ]]; then
  printf "Proceed with installation? [y/N] "
  read -r answer
  [[ "${answer:-}" == "y" || "${answer:-}" == "Y" ]] || fail "installation cancelled"
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
package_path="${tmp_dir}/${asset_name}"

log "Downloading ${asset_url}"
curl -fL --retry 3 --connect-timeout 15 -o "$package_path" "$asset_url" \
  || fail "download failed: ${asset_name} was not found in release ${release_tag}."

if [[ "$skip_verify" == true ]]; then
  warn "checksum verification disabled"
else
  verify_checksum "$package_path" "$asset_name"
fi

stop_running_process

if [[ "$install_system" == true ]]; then
  # A user-local AppImage would shadow the system package, so remove it.
  remove_user_appimage
  log "Installing with ${pm}"
  if [[ "$pm" == "dnf" ]]; then
    run_as_root dnf install -y "$package_path"
  else
    # A stale package list names dependency versions the mirror has since
    # removed (404), so refresh it first. An unrelated broken repository can
    # make the refresh fail; the install is still worth trying then.
    # One root call, so a graphical pkexec prompt (the in-app updater) asks
    # for the password once.
    # shellcheck disable=SC2016 # $1 expands in the root shell.
    run_as_root bash -c '
      apt-get update \
        || echo "[rawtap] WARNING: apt-get update failed; installing with the existing package list" >&2
      apt-get install -y "$1"
    ' bash "$package_path"
  fi
  log "System install complete"
else
  install_dir="${HOME}/.local/bin"
  desktop_dir="${HOME}/.local/share/applications"
  icon_dir="${HOME}/.local/share/icons/hicolor/256x256/apps"
  install -d "$install_dir" "$desktop_dir" "$icon_dir"

  appimage_path="${install_dir}/${BIN_NAME}"
  install -m 0755 "$package_path" "$appimage_path"
  log "AppImage installed at ${appimage_path}"

  # Pull the app icon out of the AppImage for the launcher.
  icon_src="${tmp_dir}/squashfs-root/usr/share/icons/hicolor/256x256/apps/${BIN_NAME}.png"
  if (cd "$tmp_dir" && "$appimage_path" --appimage-extract 'usr/share/icons/*' >/dev/null 2>&1) \
    && [[ -f "$icon_src" ]]; then
    cp "$icon_src" "${icon_dir}/${BIN_NAME}.png"
  else
    warn "could not extract the app icon from the AppImage; the launcher will use a generic icon"
  fi

  # The same entry as packaging/info.brumley.rawtap.desktop, pointing at the AppImage.
  cat > "${desktop_dir}/${APP_ID}.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=RawTap
Comment=See and send raw serial and TCP data, in hex and text
Exec=${appimage_path}
Icon=${BIN_NAME}
Terminal=true
Categories=Development;Utility;
Keywords=serial;tcp;console;hex;terminal;switch;
StartupNotify=false
EOF
  log "Desktop launcher created"

  if [[ ":$PATH:" != *":${install_dir}:"* ]]; then
    log "Note: ${install_dir} is not in your PATH. Launch RawTap from your app menu, or add it to PATH."
  fi
fi

add_user_to_serial_group

optional_cmd update-desktop-database && update-desktop-database "${HOME}/.local/share/applications" >/dev/null 2>&1 || true
optional_cmd gtk-update-icon-cache && gtk-update-icon-cache -f -t -q "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
if optional_cmd kbuildsycoca6; then
  kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
elif optional_cmd kbuildsycoca5; then
  kbuildsycoca5 --noincremental >/dev/null 2>&1 || true
fi

log "Installation complete. Run 'rawtap' in a terminal, or open RawTap from your application menu."

# RawTap is a terminal app, so it starts in this terminal. Piped into bash
# (curl | bash), stdin is the script, so its keys come from /dev/tty; without
# a terminal (CI, a detached updater) there is nothing to start it in.
if [[ "$relaunch" == true ]] && { : </dev/tty >/dev/tty; } 2>/dev/null; then
  if [[ "$install_system" == true ]]; then
    launch_path="/usr/bin/${BIN_NAME}"
  else
    launch_path="${appimage_path}"
  fi
  if [[ -x "$launch_path" ]]; then
    log "Starting RawTap"
    exec "$launch_path" </dev/tty >/dev/tty 2>&1
  fi
fi
