#!/usr/bin/env bash
set -euo pipefail

# RawTap uninstaller for Linux.
#   curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/rawtap/uninstall.sh | bash

BIN_NAME="rawtap"
APP_ID="info.brumley.rawtap"
USER_DATA_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/rawtap"

usage() {
  cat <<'EOF'
RawTap uninstaller

Usage:
  uninstall.sh [--purge-data] [--yes]

Removes the RawTap package (apt/dnf) or AppImage and its launcher. Leaves
~/.config/rawtap (settings, macros, logs and captures) and your serial-port
group membership in place.

Options:
  --purge-data  Also remove ~/.config/rawtap, including traffic logs and captures
  --yes         Skip the confirmation prompt
  -h, --help    Show this help message
EOF
}

log() { printf "[rawtap-uninstall] %s\n" "$*"; }
fail() { printf "[rawtap-uninstall] ERROR: %s\n" "$*" >&2; exit 1; }
optional_cmd() { command -v "$1" >/dev/null 2>&1; }
has_display() { [[ -n "${DISPLAY:-}" || -n "${WAYLAND_DISPLAY:-}" ]]; }

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

non_interactive=false
purge_data=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --purge-data) purge_data=true ;;
    --yes) non_interactive=true ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unknown option: $1" ;;
  esac
  shift
done
[[ -t 0 ]] || non_interactive=true

log "Preparing RawTap uninstall"
if [[ "$non_interactive" == false ]]; then
  printf "Proceed with uninstall? [y/N] "
  read -r answer
  [[ "${answer:-}" == "y" || "${answer:-}" == "Y" ]] || fail "uninstall cancelled"
fi

if optional_cmd pgrep && pgrep -x "$BIN_NAME" >/dev/null 2>&1; then
  log "Closing running RawTap"
  pkill -TERM -x "$BIN_NAME" >/dev/null 2>&1 || true
  sleep 1
  pkill -KILL -x "$BIN_NAME" >/dev/null 2>&1 || true
fi

# System package.
if optional_cmd rpm && rpm -q "$BIN_NAME" >/dev/null 2>&1; then
  log "Removing RPM package"
  run_as_root dnf remove -y "$BIN_NAME" 2>/dev/null || run_as_root rpm -e "$BIN_NAME"
fi
if optional_cmd dpkg && dpkg -s "$BIN_NAME" 2>/dev/null | grep -q "Status: install ok installed"; then
  log "Removing DEB package"
  run_as_root apt-get remove -y "$BIN_NAME" 2>/dev/null || run_as_root dpkg -r "$BIN_NAME"
fi

# User-local AppImage install.
for path in \
  "${HOME}/.local/bin/${BIN_NAME}" \
  "${HOME}/.local/share/applications/${APP_ID}.desktop" \
  "${HOME}/.local/share/icons/hicolor/256x256/apps/${BIN_NAME}.png"; do
  if [[ -e "$path" || -L "$path" ]]; then
    log "Removing ${path}"
    rm -f "$path"
  fi
done

if [[ "$purge_data" == true ]]; then
  if [[ -d "$USER_DATA_DIR" ]]; then
    log "Removing user data: ${USER_DATA_DIR}"
    rm -rf "$USER_DATA_DIR"
  fi
elif [[ -d "$USER_DATA_DIR" ]]; then
  log "User data left at ${USER_DATA_DIR} (re-run with --purge-data to remove it)"
fi

optional_cmd update-desktop-database && update-desktop-database "${HOME}/.local/share/applications" >/dev/null 2>&1 || true
if optional_cmd kbuildsycoca6; then
  kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
elif optional_cmd kbuildsycoca5; then
  kbuildsycoca5 --noincremental >/dev/null 2>&1 || true
fi

log "Uninstall complete"
