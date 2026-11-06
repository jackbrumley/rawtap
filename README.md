# RawTap

See and send raw serial and TCP data, in hex and text. A console when you need one.

RawTap started as a small Python monitor (`rawtap.py`) and is now a single program for Linux and Windows that also works as a switch/console terminal.

- Watches a serial port, TCP connection or TCP listener in **hex and text**, and logs everything. It also pulls scanner images out of the stream, as before.
- **Live terminal pane** shows the device the way PuTTY does: `--More--` paging, backspaces and colours all work.
- **Sends** hex or ASCII a whole line at a time, or every keystroke live (Tab completion, `?`, Ctrl+C...).
- **Hidden sends** for passwords. Passwords typed at a `Password:` prompt never reach the log either.
- **Macros** that can wait for a prompt and save the output, e.g. one key to save a switch's running-config to a file.
- One small program (`rawtap` / `rawtap.exe`), no Python. Run it with no arguments for the start screen, or use it from the command line.

## Build

```bash
cargo build --release          # -> target/release/rawtap (rawtap.exe on Windows)
cargo test                     # unit tests + a scripted session against a fake Cisco-style switch
cargo test drive_a_switch -- --nocapture   # same, printing every screen it renders
```

On Linux, serial ports belong to the `dialout` group (`uucp` on Arch): `sudo usermod -aG dialout $USER`, then log out and back in.

## Where RawTap keeps its files

Everything lives in one folder, on Linux and Windows alike: `~/.config/rawtap` (Windows: `%USERPROFILE%\.config\rawtap`). On first run RawTap creates a starter `settings.json` there. RawTap 0.6.0 kept `rawtap_config.json` and `rawtap_state.json` next to the exe; the first run of a newer version moves them here (as `settings.json` and `state.json`). Logs and captures written by 0.6.0 stay where they were.

## Screen

```
┌ Raw log · compact ───────────┐┌ Terminal 94×26 ──────────────── LIVE KEYS · F4 to leave ┐
│12:48:35 TX  1 20       │     ││Switch#show running-config                               │
│12:48:35 RX 63 08 08 …  │ <BS…││Building configuration...                                │
│12:48:35 RX 33 69 6E …  │ int…││interface GigabitEthernet1/0/1                           │
└──────────────────────────────┘└─────────────────────────────────────────────────────────┘
┌  LIVE KEYS  Enter sends CR ─────────────────────────────────────────────────────────────┐
```

- **Split** is the default layout: the raw log on the left and the terminal on the right.
  - If the window is under 130 columns wide, the panes stack instead: the terminal on top and a small log strip above the send box.
  - **F3** cycles split → log only → terminal only.
  - **Ctrl+←/→** changes the terminal width; **Ctrl+↑/↓** changes the log strip height.
- **Each connection remembers its own settings** (`serial:COM22`, `tcp:10.0.0.5:2101`...): layout, log format, line ending, HEX/ASCII and pane sizes. A switch and a scanner feed can each keep their own setup.
- **The raw log groups bytes into one entry per line**, starting a new entry on a newline, a 30 ms gap, a change of direction, or every 1 KB. **F8** cycles the format: compact (one row each) → hex+text → hex → text.

## Sending

| Mode | How |
|------|-----|
| Line mode (default) | Type, Enter sends. **Tab** HEX↔ASCII. **↑↓** history. **F2** line ending (default **CR**: CRLF counts as two Enters on most switches). |
| HEX | `02 41 03`, `024103`, `0x02,0x41,0x03`, `02:41:03`. Sent exactly as typed. |
| ASCII | Text, then the line ending. Also takes `\r \n \t \0 \e \\ \xHH` and the display tokens `<STX> <CR> <0x1B>`, so a received TEXT line can be pasted back. `\<` sends a literal `<`. |
| Live keys (**F4**) | Every key goes straight to the device: Tab, `?`, space at `--More--`, Ctrl+C/Z, arrows. F-keys still work. Press F4 again to get back to line mode, where Ctrl+Q/R/N work. |
| Hidden send (**F6**) | Typed masked, then sent with the line ending. Logged only as `<hidden: N bytes>` and never kept in history. Works from line mode or live keys. |

In live keys, anything typed while the cursor is just after a `Password:` / `Passcode:` / `Secret:` / `PIN:` prompt is also logged as `<hidden>`.

## Menu (F1)

**F1** opens a menu in the middle of the screen. **Ctrl+P** also opens it in line mode; in live keys mode Ctrl+P goes to the device, which uses it as its own "previous command" key. The menu lists every action, macro and setting for this connection.
- **↑↓** to move.
- **←→** changes a setting straight away. That includes the on/off switches: live keys, raw log following/paused and the PgUp/PgDn scroll pane. The menu stays open.
- **Enter** runs an action.
- **Typing** filters the list (`delay`, `break`, `save`...).
- **Esc** closes it.

The **Debug** section at the bottom has what a bug report needs: *Copy log* (RawTap's own session log), *Open log folder*, *Copy system info* and the *Log level*.

A short description of the highlighted item is shown at the bottom. On screen, the footer shows only F1 Menu, F4 Live keys, Ctrl+N New connection and Ctrl+Q Quit.

The start screen has an **Options ▸** row (←→ or Enter to open it). It shows the remembered line ending, char delay, send mode and layout for the selected port or host, so you can set them before connecting. These are the same settings the F1 menu changes.

## Mouse, copy and paste

These follow Windows Terminal conventions:

| Action | How |
|--------|-----|
| Scroll back | Mouse wheel over either pane scrolls that pane; nothing is sent to the device |
| Select | Drag in the terminal or raw log pane. Only that pane's text is selected, with no borders. To go beyond one screen, drag past the top or bottom edge (it auto-scrolls) or use the wheel while dragging. **Ctrl+click** extends an existing selection; scroll first if you need to. |
| Raw log copies | Always whole entries, in full log-file form (`[time] RX HEX: …` / `RX TEXT: …`), even if the screen shows the compact view. Hidden sends stay `<hidden>`. |
| Copy | **Ctrl+Shift+C**, or right-click while something is selected. In line mode Ctrl+C also copies a selection; in live keys Ctrl+C goes to the device. |
| Paste | **Ctrl+Shift+V**, or right-click with nothing selected. One line goes into the send box (in live keys, straight to the device). **Several lines ask for confirmation**, then go line by line with the line ending and the char delay. |
| Long output | F1 → *Copy all terminal output*, or *Open transcript* (Notepad on Windows, your default text app on Linux) |
| The console's own selection | **Shift+drag** |

Ctrl+C never quits. Ctrl+Q quits in line mode, and **Ctrl+Shift+Q** quits from anywhere, including live keys. (In Windows Terminal, Shift+click is taken by the console's own selection, which is why extending uses Ctrl+click.)

## Shortcut keys

Everything below is also in the F1 menu. The shortcuts are for when you already know what you want.

| Key | Action |
|-----|--------|
| F1 / Ctrl+P | menu |
| F2 / F3 / F4 | line ending / layout / live keys |
| F5 | which pane PgUp/PgDn scroll (split layout) |
| F6 / F7 | hidden send / serial break (300 ms, for password recovery and ROMMON) |
| F8 / F9 / F10 | log format / pause-follow the log / clear both panes (the files keep everything) |
| PgUp/PgDn, Ctrl+Home/End | scroll the focused pane / jump to its top or bottom |
| Ctrl+T | char delay 0 / 2 / 5 / 10 / 20 / 50 ms between sent characters, for consoles that garble fast input (remembered per connection) |
| Ctrl+R / Ctrl+N / Ctrl+Q | reconnect / new connection / quit (line mode) |
| Esc | clear the send box, or cancel a running macro |

## Macros

Macros live in `settings.json`. They can use **Alt+0..9**, **Shift+F1..F12**, **F11** or **F12** (plain F1–F10 are built-in keys). A macro is either one `data` send or a list of `steps`. Within a step, things run in the order `delay_ms` → `send`/`send_hex` → `wait` (for this text) → `wait_idle_ms` (until the device goes quiet), with an optional `timeout_ms` (default 10 s). `capture: true` saves everything the device printed while the macro ran to `captures/<label>_<time>.txt`.

```json
{ "key": "Alt+9", "label": "Switch: save running-config", "capture": true,
  "steps": [
    { "send": "terminal length 0", "wait": "#" },
    { "send": "show running-config", "wait_idle_ms": 2000, "timeout_ms": 120000 }
  ] }
```

Don't put passwords in macros. Log in first (using F6), then run the macro.

## Files written

All paths are inside `~/.config/rawtap`. The `logs` and `captures` folders can be changed in `settings.json`.

| File | What |
|------|------|
| `logs/rawtap_<type>_<time>.log` | Everything sent and received: timestamp, RX/TX, hex and text. Hidden sends show only their length. |
| `logs/rawtap_<type>_<time>.txt` | Plain-text transcript of what the device printed, with escape codes and `--More--` cleaned out |
| `captures/*.txt` | Macro captures |
| `logs/extracted_images/*.bmp` | Image extraction (see below) |
| `settings.json` | Your macros, folders and image extraction settings (edit by hand, then restart RawTap) |
| `state.json` | Last connection, per-connection settings, line-mode history, log level |
| `debug/session.log`, `debug/last-session.log` | RawTap's own log for this run and the previous one, for bug reports (F1 → Debug) |

## Command line

```text
rawtap                                   start screen
rawtap serial -p COM3 -b 9600            (also --data-bits --parity --stop-bits --flow)
rawtap tcp -H 192.168.1.50 -p 2101       connect out
rawtap listen -p 9000 [--bind 0.0.0.0]   wait for a client
rawtap serial -p /dev/ttyUSB0 -b 9600    (Linux)
rawtap ports                             list serial ports
rawtap --system-info                     version, OS and terminal, for bug reports
--no-log                                 don't write log/transcript files
```

## Image extraction

Same settings as the Python version (`image_extraction` in the config), plus:
- `file_extension` (default `bmp`)
- `max_payload_bytes` (default 16 MB), so a corrupt size field can't make it buffer forever.

## Limitations

- Raw TCP only, no telnet negotiation. Connecting to a switch's port 23 shows the telnet handshake bytes and some option prompts won't work. A serial console or a raw TCP port (terminal server) is fine.
- Break works on serial ports only.
- When the window is resized, existing terminal lines aren't re-wrapped (PuTTY doesn't re-wrap them either).

## Code layout

| File | Contents |
|------|----------|
| `src/codec.rs` | hex/text formatting and send parsing |
| `src/link.rs` | serial, TCP client and TCP server (a reader thread each), and break |
| `src/term.rs` | terminal emulation (vt100), live-key encoding, plain-text cleaner and transcript |
| `src/macros.rs` | macro keys and the step runner |
| `src/menu.rs` | the F1 menu: items, descriptions, search |
| `src/clip.rs` | clipboard (in-memory during tests) |
| `src/monitor.rs` | monitor state: raw log grouping, sending, live keys, hidden send, macros |
| `src/monitor/view.rs`, `src/monitor/keys.rs` | panes, mouse, copy and paste / commands, the F1 menu and keys |
| `src/ui_monitor.rs` | drawing the monitor: layouts and header |
| `src/ui_monitor/panes.rs`, `src/ui_monitor/controls.rs` | drawing the raw log and terminal / the send box, footer, menu and help |
| `src/app.rs`, `src/ui.rs` | start screen and app wiring |
| `src/extract.rs`, `src/logger.rs`, `src/config.rs` | image extraction, traffic log file, settings/state |
| `src/paths.rs`, `src/json_store.rs` | where files live (and the move from 0.6.0's locations); atomic saves |
| `src/session_log.rs`, `src/system_info.rs` | RawTap's own session log and panic hook; system info for bug reports |
| `src/opener.rs`, `src/app/identity.rs` | opening files and folders; the app ID `info.brumley.rawtap` |
| `src/switch_sim.rs` | end-to-end test against a fake switch |
