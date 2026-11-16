# RawTap's in-app updater for Windows.
#
# `rawtap update`, Ctrl+U and F1 -> Update now (from 0.13.0 on) download the
# current copy of this script and run it in a console window of its own, so
# this file owns the whole update. A fix made here reaches every installed
# version on its next update, with no new release. RawTap only starts
# PowerShell, which downloads this script and runs it, and then exits so the
# installer can replace rawtap.exe.
#
# Contract with the app; keep it stable, every shipped version relies on it:
#   - Parameters: -BaseUrl (where this script was downloaded from, without a
#     trailing slash) and -Relaunch (the installed rawtap.exe). Unknown
#     arguments are ignored.
#   - The first output line is exactly "[rawtap-update] started".
#   - Output goes to this window; this script's own lines also go to
#     debug\update.log, which it empties first (install.ps1 adds its own).
#   - The last line is "[rawtap-update] finished: updated" or
#     "[rawtap-update] finished: failed". Updated: the new version starts in
#     its own window. Failed: the installed version starts with
#     --update-failed, which tells the user and names the update log, and
#     this window stays open until Enter. RAWTAP_NO_RELAUNCH=1 does neither
#     (the release install test).
# A plain (not advanced) script, so unknown arguments land in $args instead
# of failing.
param(
  [string]$BaseUrl = "",
  [string]$Relaunch = ""
)

$LogFile = Join-Path $env:USERPROFILE ".config\rawtap\debug\update.log"
try {
  New-Item -ItemType Directory -Force -Path (Split-Path $LogFile) | Out-Null
  Set-Content -LiteralPath $LogFile -Value $null -ErrorAction Stop
} catch {
  $LogFile = $null
}

function Say($Message) {
  Write-Output $Message
  if ($LogFile) { Add-Content -LiteralPath $LogFile -Value "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] $Message" -ErrorAction SilentlyContinue }
}

Say "[rawtap-update] started"
Say "[rawtap-update] $(Get-Date -Format o) from $BaseUrl"

$Unattended = $env:RAWTAP_NO_RELAUNCH -eq "1"

# Through the classic console host, so RawTap opens in its own window rather
# than as a Windows Terminal tab.
function Start-RawTap([string[]]$Arguments) {
  $exe = $Relaunch
  if (-not ($exe -and (Test-Path -LiteralPath $exe))) {
    $exe = Join-Path $env:LOCALAPPDATA "rawtap\rawtap.exe"
  }
  if (-not (Test-Path -LiteralPath $exe)) {
    Say "[rawtap-update] No installed RawTap found to start"
    return
  }
  $conhost = Join-Path $env:SystemRoot "System32\conhost.exe"
  Start-Process -FilePath $conhost -ArgumentList (@("`"$exe`"") + $Arguments)
}

if (-not $BaseUrl) {
  Say "[rawtap-update] ERROR: -BaseUrl is required"
  $succeeded = $false
} else {
  # install.ps1 runs from a file rather than through iex, so its `exit 1` on
  # failure ends that script, not this one, and sets $global:LASTEXITCODE.
  $installer = Join-Path $env:TEMP 'rawtap-update-install.ps1'
  try {
    Invoke-RestMethod -Uri "$BaseUrl/install.ps1" -OutFile $installer -ErrorAction Stop
    $global:LASTEXITCODE = 0
    & $installer -NoRelaunch
    $succeeded = $global:LASTEXITCODE -eq 0
  } catch {
    Say "[rawtap-update] ERROR: $_"
    $succeeded = $false
  }
  Remove-Item -LiteralPath $installer -ErrorAction SilentlyContinue
}

if ($succeeded) {
  Say "[rawtap-update] finished: updated"
  if (-not $Unattended) { Start-RawTap @() }
  exit 0
}

Say "[rawtap-update] finished: failed"
if (-not $Unattended) {
  Start-RawTap @('--update-failed')
  Read-Host 'Press Enter to close'
}
exit 1
