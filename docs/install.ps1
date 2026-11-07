<#
.SYNOPSIS
  RawTap installer for Windows.
.DESCRIPTION
  Downloads the latest RawTap release from GitHub, verifies its checksum and
  installs it: the per-user setup.exe by default (no admin, %LOCALAPPDATA%\rawtap),
  or the MSI with -System (system-wide, admin, Program Files\rawtap). Both put
  rawtap on PATH. The in-app updater re-runs this script.
.EXAMPLE
  irm https://brumley.info/rawtap/install.ps1 | iex
#>

param(
  [string]$Version = "",
  [switch]$System,
  [switch]$InsecureSkipVerify,
  [switch]$NoRelaunch
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"  # Invoke-WebRequest is far slower with the progress bar.

$Repo = "jackbrumley/rawtap"
$AppName = "rawtap"
$TempDir = Join-Path $env:TEMP "rawtap-install"

$GitHubApiHeaders = @{ "Accept" = "application/vnd.github+json" }
if ($env:RAWTAP_GITHUB_TOKEN) {
  $GitHubApiHeaders["Authorization"] = "Bearer $env:RAWTAP_GITHUB_TOKEN"
}

# RawTap keeps its files in ~/.config/rawtap on every platform.
$ConfigDir = Join-Path $env:USERPROFILE ".config\rawtap"

$LogFile = $null
try {
  $DebugDir = Join-Path $ConfigDir "debug"
  New-Item -ItemType Directory -Force -Path $DebugDir | Out-Null
  $LogFile = Join-Path $DebugDir "update.log"
} catch {}

function Log($Message) {
  Write-Host "[rawtap] $Message"
  if ($LogFile) { Add-Content -Path $LogFile -Value "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] [rawtap] $Message" -ErrorAction SilentlyContinue }
}

function Fail($Message) {
  Write-Host "[rawtap] ERROR: $Message" -ForegroundColor Red
  if ($LogFile) { Add-Content -Path $LogFile -Value "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] [rawtap] ERROR: $Message" -ErrorAction SilentlyContinue }
  exit 1
}

# Removes any existing RawTap install (setup.exe or MSI) so switching between
# the two never leaves both installed.
function Uninstall-Existing {
  $RegistryPaths = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*"
  )
  foreach ($path in $RegistryPaths) {
    $entries = Get-ItemProperty -Path $path -ErrorAction SilentlyContinue | Where-Object {
      $_.DisplayName -and $_.DisplayName -like "RawTap*"
    }
    foreach ($entry in $entries) {
      Log "Removing previous version: $($entry.DisplayName) $($entry.DisplayVersion)"
      try {
        if ($entry.PSChildName -match "^\{[0-9A-Fa-f\-]+\}$") {
          Start-Process -FilePath "msiexec.exe" -ArgumentList "/x `"$($entry.PSChildName)`" /qb /norestart" -Wait | Out-Null
        } elseif ($entry.QuietUninstallString) {
          Start-Process -FilePath "cmd.exe" -ArgumentList "/c `"$($entry.QuietUninstallString)`"" -Wait | Out-Null
        } elseif ($entry.UninstallString) {
          $exe = $entry.UninstallString.Trim('"')
          if (Test-Path $exe) {
            Start-Process -FilePath $exe -ArgumentList "/S" -Wait | Out-Null
          } else {
            Start-Process -FilePath "cmd.exe" -ArgumentList "/c `"$($entry.UninstallString) /S`"" -Wait | Out-Null
          }
        }
      } catch {
        Log "WARNING: could not remove $($entry.DisplayName): $_"
      }
    }
  }
}

if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
  Fail "Only 64-bit x64 Windows is supported (this PC is $env:PROCESSOR_ARCHITECTURE)."
}

if ($System -or $env:RAWTAP_SYSTEM -eq "1") {
  $IsAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  if (-not $IsAdmin) { Fail "A system-wide install needs PowerShell opened as Administrator." }
  $InstallType = "msi"
} else {
  $InstallType = "exe"
}

if ([string]::IsNullOrEmpty($Version)) {
  try {
    $ReleaseTag = (Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers $GitHubApiHeaders).tag_name
  } catch {
    Fail "Could not get the latest release from GitHub (you may be rate limited; try again in a few minutes): $_"
  }
} else {
  $ReleaseTag = $Version
}
$VersionNumber = $ReleaseTag.TrimStart('v')

if ($InstallType -eq "msi") {
  $AssetName = "$AppName-$VersionNumber-windows-x64.msi"
} else {
  $AssetName = "$AppName-$VersionNumber-windows-x64-setup.exe"
}

Log "Release: $ReleaseTag"
Log "Package: $AssetName"

try {
  $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/tags/$ReleaseTag" -Headers $GitHubApiHeaders
} catch {
  Fail "Could not read release $ReleaseTag from GitHub: $_"
}
$Asset = $Release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1
if (-not $Asset) { Fail "$AssetName was not found in release $ReleaseTag." }

if (Test-Path $TempDir) { Remove-Item -Recurse -Force $TempDir }
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
$InstallerPath = Join-Path $TempDir $AssetName

Log "Downloading $($Asset.browser_download_url)"
try {
  Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $InstallerPath -UseBasicParsing
} catch {
  Fail "Download failed: $_"
}

if ($InsecureSkipVerify) {
  Log "WARNING: checksum verification disabled"
} else {
  $Digest = [string]$Asset.digest
  if (-not $Digest.StartsWith("sha256:")) {
    Fail "GitHub did not provide a checksum for $AssetName. Re-run with -InsecureSkipVerify to bypass verification."
  }
  $Expected = $Digest.Substring(7).ToLower()
  $Actual = (Get-FileHash -Path $InstallerPath -Algorithm SHA256).Hash.ToLower()
  if ($Expected -ne $Actual) { Fail "Checksum mismatch (expected $Expected, got $Actual)" }
  Log "Checksum verified"
}

if (Get-Process -Name $AppName -ErrorAction SilentlyContinue) {
  Log "Closing running RawTap"
  Stop-Process -Name $AppName -Force -ErrorAction SilentlyContinue
  Start-Sleep -Seconds 1
}

Uninstall-Existing

Log "Running installer"
if ($InstallType -eq "msi") {
  $process = Start-Process -FilePath "msiexec.exe" -ArgumentList "/i `"$InstallerPath`" /qb /norestart" -Wait -PassThru
  if ($process.ExitCode -ne 0 -and $process.ExitCode -ne 3010) { Fail "MSI installer failed with exit code $($process.ExitCode)" }
} else {
  $process = Start-Process -FilePath $InstallerPath -ArgumentList "/S" -Wait -PassThru
  if ($process.ExitCode -ne 0) { Fail "Installer failed with exit code $($process.ExitCode)" }
}

Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
Log "Installation complete. Open a new terminal and run 'rawtap', or open RawTap from the Start Menu."

# RawTap is a terminal app: started this way it opens in its own console window.
if (-not $NoRelaunch -and $env:RAWTAP_NO_RELAUNCH -ne "1") {
  $candidates = @(
    (Join-Path $env:LOCALAPPDATA "rawtap\rawtap.exe"),
    (Join-Path $env:ProgramFiles "rawtap\rawtap.exe")
  )
  $exePath = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
  if ($exePath) {
    Log "Starting RawTap"
    Start-Process -FilePath $exePath
  }
}
