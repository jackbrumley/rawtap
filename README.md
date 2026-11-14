<p align="center">
  <img src="packaging/icons/rawtap-128.png" alt="" width="96" height="96" />
</p>

<h1 align="center">RawTap</h1>

<p align="center">See and send raw serial and TCP data, in hex and text, on Linux and Windows.<br />A switch console when you need one.</p>

<p align="center">
  <a href="https://github.com/jackbrumley/rawtap/releases/latest"><img src="https://img.shields.io/github/v/release/jackbrumley/rawtap?label=release&color=d97706" alt="Latest release" /></a>
  <img src="https://img.shields.io/badge/platform-Linux%20%7C%20Windows-d97706" alt="Linux and Windows" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-d97706" alt="AGPL-3.0" /></a>
</p>

<p align="center">
  <a href="https://brumley.info/rawtap/">Website</a> ·
  <a href="https://github.com/jackbrumley/rawtap/releases/latest">Downloads</a> ·
  <a href="docs/GUIDE.md">Guide</a> ·
  <a href="https://github.com/jackbrumley/rawtap/issues">Report an issue</a>
</p>

## Install

**Linux** (.deb or .rpm)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/rawtap/install.sh | bash
```

**Linux** (AppImage, any distro)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/rawtap/install.sh | bash -s -- --appimage
```

**Windows** (PowerShell, just for you)

```powershell
irm https://brumley.info/rawtap/install.ps1 | iex
```

**Windows** (PowerShell as Administrator, every user)

```powershell
irm https://brumley.info/rawtap/install-system.ps1 | iex
```

Prefer a package? The [latest release](https://github.com/jackbrumley/rawtap/releases/latest) has `.deb`, `.rpm`, `.AppImage`, `.exe` and `.msi` files. Every install puts `rawtap` on your PATH, and RawTap checks for a new version each time it starts.

## What it does

RawTap is a serial and TCP monitor and console for Linux and Windows. It was built for day-to-day work with barcode scanners on conveyor belts, printers and scales, over TCP (as client or server) and serial, where you need to see exactly what a device sends. Unlike PuTTY, it shows every byte in hex and text, carriage returns and line feeds included, sends raw hex, and opens straight back on the connection you used last.

- Shows every byte as hex and text, with carriage returns, line feeds and other control characters visible
- Sends text or raw hex, a line at a time or key by key
- Serial ports, TCP client and server, Telnet and SSH, all in one tool
- Remembers your last connection and each device's settings, so it opens ready to go
- A live terminal for switch and router consoles: paging, colours and Tab completion work like PuTTY, and passwords stay out of the logs
- Logs everything to a file, and macros can wait for a prompt and save the output

## Serial ports on Linux

Serial ports belong to the `dialout` group (`uucp` on Arch). The Linux installers add you to it. If you build from source, run `sudo usermod -aG dialout $USER`, then log out and back in.

## Build from source

```bash
cargo build --release   # -> target/release/rawtap (rawtap.exe on Windows)
cargo test              # unit tests and a scripted session against a fake switch
```

## License

[GNU AGPLv3](LICENSE). Use it, learn from it and build on it freely; if you improve it, share the changes back.

---

<p align="center">
  RawTap is one of the free apps by <a href="https://brumley.info">Jack Brumley</a>:<br />
  <a href="https://voquill.org">Voquill</a> · <a href="https://brumley.info/rusty-g6/">Rusty G6</a> · <a href="https://somedrive.org/">SomeDrive</a> · <a href="https://yambuck.com/">Yambuck</a> · <b>RawTap</b> · <a href="https://bmuster.com/">BMuster</a>
</p>
