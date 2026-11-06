# RawTap TODO

Open work only; remove items when done. The target is the App Standard and
the Release and Update Standard (see `AGENTS.md`).

## Now

- **Installers:** `docs/install.sh`, `uninstall.sh`, `install.ps1`,
  `install-system.ps1`, `uninstall.ps1`, copied from Rusty G6. Post-install:
  add the user to the serial-port group. Then the release workflow's install
  test job (every install mode, `rawtap --version`, uninstall).
- **Install page:** `docs/index.html` with the shared app page layout, served
  at `https://brumley.info/rawtap`; a RawTap card on brumley.info (icon:
  `packaging/icons/rawtap-256.png`).

## Next

- **In-app update:** the Release Standard's "Terminal apps" flow
  (`rawtap update`, F1 → Updates, `--update-failed`).
- **First standard release:** v0.7.0, install-tested on Linux and Windows.
- **Windows exe icon and version info:** embed `rawtap.ico` and the version
  in `rawtap.exe` itself (shortcuts already use `rawtap.ico`).

## Ideas / Parking Lot

- macOS build.
- OSC 52 clipboard for RawTap running over SSH.
- Telnet option negotiation for port 23.
