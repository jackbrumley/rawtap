#!/usr/bin/env bash
# RawTap's in-app updater for Linux.
#
# `rawtap update`, Ctrl+U and F1 → Update now (from 0.13.0 on) download the
# current copy of this script and run it in RawTap's own terminal, so this file
# owns the whole update. A fix made here reaches every installed version on its
# next update, with no new release. RawTap only replaces itself with bash,
# which downloads this script and runs it.
#
# Contract with the app; keep it stable, every shipped version relies on it:
#   - Arguments: --base-url <where this script was downloaded from, without a
#     trailing slash> and --relaunch <the installed binary or AppImage>.
#     Unknown arguments are ignored.
#   - The first output line is exactly "[rawtap-update] started".
#   - Output goes to the terminal; this script's own lines also go to
#     debug/update.log, which it empties first (install.sh adds its own).
#   - The last line is "[rawtap-update] finished: updated" or
#     "[rawtap-update] finished: failed". Updated: the new version starts in
#     this terminal. Failed: the installed version starts with
#     --update-failed, which tells the user and names the update log.
#     RAWTAP_NO_RELAUNCH=1 starts neither (the release install test).

log_file="${XDG_CONFIG_HOME:-${HOME}/.config}/rawtap/debug/update.log"
mkdir -p "$(dirname "$log_file")" 2>/dev/null && : > "$log_file" 2>/dev/null

say() {
  printf '%s\n' "$1"
  printf '[%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$1" >> "$log_file" 2>/dev/null
}

say "[rawtap-update] started"

base_url=""
relaunch=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --base-url) base_url="${2:-}"; shift 2 ;;
    --relaunch) relaunch="${2:-}"; shift 2 ;;
    *) say "[rawtap-update] Ignoring unknown argument: $1"; shift ;;
  esac
done
say "[rawtap-update] $(date -u +%Y-%m-%dT%H:%M:%SZ) from ${base_url}"

# Starts RawTap in this terminal, replacing this script. Without a terminal
# (or with RAWTAP_NO_RELAUNCH=1) there is nothing to start it in.
start_rawtap() {
  if [[ "${RAWTAP_NO_RELAUNCH:-}" == "1" ]]; then
    return
  fi
  if [[ -n "$relaunch" && -x "$relaunch" ]] && { : </dev/tty >/dev/tty; } 2>/dev/null; then
    exec "$relaunch" "$@" </dev/tty >/dev/tty 2>&1
  fi
  say "[rawtap-update] Run 'rawtap' to start RawTap."
}

finish_failed() {
  say "[rawtap-update] finished: failed"
  start_rawtap --update-failed
  exit 1
}

if [[ -z "$base_url" ]]; then
  say "[rawtap-update] ERROR: --base-url is required"
  finish_failed
fi

# The user asked for the update, so install.sh doesn't ask again; it still
# asks for a password in this terminal when it needs one. This script starts
# the new version, not install.sh, so the finished line comes first. AppImage
# installs stay AppImage installs rather than switching to a package.
install_args=(--yes --no-relaunch)
if [[ -n "${APPIMAGE:-}" || "$relaunch" == "${HOME}/.local/bin/"* ]]; then
  install_args+=(--appimage)
fi

installer="$(mktemp "${TMPDIR:-/tmp}/rawtap-update-install.XXXXXX")"
if ! curl --proto '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 10 "${base_url}/install.sh" -o "$installer"; then
  rm -f "$installer"
  say "[rawtap-update] ERROR: could not download ${base_url}/install.sh"
  finish_failed
fi

bash "$installer" "${install_args[@]}"
status=$?
rm -f "$installer"
if [[ "$status" -ne 0 ]]; then
  say "[rawtap-update] Installer exited with status ${status}"
  finish_failed
fi

say "[rawtap-update] finished: updated"
start_rawtap
exit 0
