<#
.SYNOPSIS
  Adds or removes RawTap's folder in the current user's PATH.
.DESCRIPTION
  Run by the per-user installer (setup.exe) and its uninstaller. Reads and
  writes HKCU\Environment\Path as REG_EXPAND_SZ without expanding it, so
  entries like %USERPROFILE%\bin survive, and works on a PATH of any length
  (NSIS's own string functions stop at 1024 characters). Then tells running
  programs that the environment changed.
.EXAMPLE
  path-helper.ps1 -Action Add -Directory "$env:LOCALAPPDATA\rawtap"
#>
param(
  [Parameter(Mandatory = $true)][ValidateSet("Add", "Remove")][string]$Action,
  [Parameter(Mandatory = $true)][string]$Directory
)

$ErrorActionPreference = "Stop"

$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
if (-not $key) { throw "Could not open HKCU\Environment" }
try {
  $current = [string]$key.GetValue("Path", "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
  $trimmed = $Directory.TrimEnd("\")
  $entries = @($current -split ";" | Where-Object { $_ -and ($_.TrimEnd("\") -ine $trimmed) })
  if ($Action -eq "Add") { $entries += $trimmed }
  $updated = $entries -join ";"
  if ($updated -ne $current) {
    $key.SetValue("Path", $updated, [Microsoft.Win32.RegistryValueKind]::ExpandString)
  }
} finally {
  $key.Close()
}

# WM_SETTINGCHANGE "Environment", so new terminals see the change.
Add-Type -Namespace RawTap -Name Native -MemberDefinition @"
[DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);
"@
$result = [UIntPtr]::Zero
[void][RawTap.Native]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, "Environment", 2, 5000, [ref]$result)
Write-Host "RawTap PATH entry: $Action $Directory"
