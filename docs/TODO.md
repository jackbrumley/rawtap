# RawTap TODO

Open work only; remove items when done. The target is the App Standard and
the Release and Update Standard (see `AGENTS.md`).

## Now

- **App basics (App Standard §3–5):**
  - `src/paths.rs` owns every location: `~/.config/rawtap` on every platform
    (Windows: `%USERPROFILE%\.config\rawtap`), honouring `XDG_CONFIG_HOME` on
    Linux. Config becomes `settings.json`, state `state.json`; traffic logs
    and captures go under it. Remove the next-to-the-exe lookup in
    `config.rs` and the repo's `rawtap_config.json` sample (keep the
    starter template in code).
  - Session log `debug/session.log` (previous run kept as
    `last-session.log`), standard header, `[<RFC 3339>] LEVEL: message`
    lines, log level in settings.
  - Panic hook: restore the terminal, then log the panic.
  - Atomic writes for settings and state; an unparseable file is renamed
    `.invalid-<timestamp>` and the user is told once. Today
    `config.rs` saves state with `let _ = std::fs::write(...)`.
  - F1 → Debug: Copy log, Open log folder, System info, log level; and
    `rawtap --system-info`.
- **Native Linux:**
  - "Open transcript in Notepad" (`monitor/keys.rs`) uses `notepad.exe` and
    ignores the spawn error: use `xdg-open` on Linux, report failures.
  - Help text says "COM port": show `/dev/ttyUSB0` / `/dev/ttyACM0` on Linux.
  - Serial "permission denied" on Linux explains the `dialout` group
    (`uucp` on Arch).
  - `link.rs` "Windows refused the port" message is Windows-only.
  - arboard `wayland-data-control` feature for native Wayland clipboard.
  - App identity: `src/app/identity.rs` with `APP_ID = "info.brumley.rawtap"`
    and its unit test; launcher `packaging/info.brumley.rawtap.desktop`
    (`Terminal=true`); a 256 px app icon.

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
