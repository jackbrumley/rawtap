# RawTap system-wide (MSI) install for Windows. Run from PowerShell as Administrator:
#   irm https://brumley.info/rawtap/install-system.ps1 | iex
# Older Windows 10 PCs offer only TLS 1.0 by default, which GitHub refuses.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$env:RAWTAP_SYSTEM = "1"
irm https://brumley.info/rawtap/install.ps1 | iex
