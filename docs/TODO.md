# RawTap TODO

Open work only; remove items when done. The target is the App Standard and
the Release and Update Standard (see `AGENTS.md`).

## Now

- **App icon:** a 256 px `rawtap` icon for the launcher, the install page
  and the brumley.info card (needed by the release pipeline).

## Next

- **Release pipeline:** `scripts/release-assets.mjs` (version from
  `Cargo.toml`), prepare / notes / package / verify / promote,
  `packaging/linux/Containerfile` (Ubuntu 22.04), `release.yml`, and
  `docs/RELEASE.md`. Assets: `rawtap-X.Y.Z-linux-x64.{deb,rpm,AppImage}`,
  `rawtap-X.Y.Z-windows-x64-setup.exe`, `rawtap-X.Y.Z-windows-x64.msi`.
- **Installers:** `docs/install.sh`, `uninstall.sh`, `install.ps1`,
  `install-system.ps1`, `uninstall.ps1`, copied from Rusty G6. Post-install:
  add the user to the serial-port group. Windows installs put `rawtap` on
  PATH.
- **Install page:** `docs/index.html` with the shared app page layout, served
  at `https://brumley.info/rawtap`; a RawTap card on brumley.info.
- **In-app update:** the Release Standard's "Terminal apps" flow
  (`rawtap update`, F1 → Updates, `--update-failed`).
- **First standard release:** v0.7.0, install-tested on Linux and Windows.

## Ideas / Parking Lot

- macOS build.
- OSC 52 clipboard for RawTap running over SSH.
- Telnet option negotiation for port 23.
