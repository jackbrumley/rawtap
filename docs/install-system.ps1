# RawTap system-wide (MSI) install for Windows. Run from PowerShell as Administrator:
#   irm https://brumley.info/rawtap/install-system.ps1 | iex
$env:RAWTAP_SYSTEM = "1"
irm https://brumley.info/rawtap/install.ps1 | iex
