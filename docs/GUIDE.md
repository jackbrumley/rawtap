# RawTap Guide

How to use RawTap day to day. For installing, see the [README](../README.md).

## Where RawTap keeps its files

Everything lives in one folder, on Linux and Windows alike: `~/.config/rawtap` (Windows: `%USERPROFILE%\.config\rawtap`). On first run RawTap creates a starter `settings.json` there. RawTap 0.6.0 kept `rawtap_config.json` and `rawtap_state.json` next to the exe; the first run of a newer version moves them here (as `settings.json` and `state.json`). Logs and captures written by 0.6.0 stay where they were.

## Favourites

Save any connection (serial port, TCP client or server, Telnet, SSH) with a name, and it's listed on the start screen under **+ New connection**.

- **+ New connection** opens an empty connection panel (only the connection type is the one you used last). **Enter** connects without saving; **Ctrl+S** saves it as a favourite (with no name yet, it suggests one, such as `SSH admin@10.0.0.5:22`; press Ctrl+S again to keep it). **Esc** goes back to the list (twice if there are unsaved changes).
- **F1 → Save as favourite…** in a session saves the connection you're in.
- On the start screen, **↑↓** moves through the list without changing anything. On a favourite, **Enter** connects, **E** (or F2) opens it in the panel to change its name, connection or options, and **Del** removes it (after asking).
- A favourite opens in its connection's view (Connection options ▸ View, below), so a switch can always open in the terminal.
- From the command line: `rawtap open "Core switch"` (any case).

Favourites are kept in `state.json`. Passwords are never saved there.

## Passwords

RawTap never writes a password to its own files. Two ways to stop typing one:

- **SSH key login** (best for servers): log in once, then **F1 → Set up key login**. RawTap makes an SSH key if you have none (`~/.ssh/id_ed25519`, no passphrase; add one any time with `ssh-keygen -p`) and types a command into the server's shell that adds it to `~/.ssh/authorized_keys`. From then on ssh logs in with the key. For Linux and other Unix servers.
- **Remember password** (any connection, e.g. a switch's Telnet or console login): **F1 → Remember password…**, type it (hidden), Enter. It's kept in your operating system's keyring: GNOME Keyring or KWallet on Linux, Windows Credential Manager on Windows. RawTap sends it, hidden, at the first password prompt of each connection; if it's wrong, the next prompt is yours, and **F1 → Forget password** removes it. Only offered when a keyring is available (the session log says why when it isn't).

## Screen

```
┌ Hex · compact ───────────────┐┌ Terminal 94×29 ─────────── LIVE KEYS · Enter sends CR · F4 to leave ┐
│12:48:35 TX  1 20       │     ││Switch#show running-config                               │
│12:48:35 RX 63 08 08 …  │ <BS…││Building configuration...                                │
│12:48:35 RX 33 69 6E …  │ int…││interface GigabitEthernet1/0/1                           │
└──────────────────────────────┘└─────────────────────────────────────────────────────────┘
 F1  Menu    F3  View  Split     F4  Leave live keys    F8  Hex compact   F1 then Ctrl+Q quits
```

- **The view**: **Terminal** (the live terminal), **Hex** (the raw data in hex and text) or **Split** (both: side by side when the window is at least 130 columns wide, stacked otherwise). Switch with **F3** or click the current view in the footer (`F3 View Split`). RawTap remembers the last view you used and opens every connection in it, unless the connection panel's Connection options ▸ View gives that connection its own (Terminal, Split or Hex).
- **Live keys** shows on the terminal pane's title, with the line ending Enter sends; the send box is hidden then, so the terminal gets its rows. The header shows a red **● REC** while the session is recorded to a log; click it to open the session logs folder.
  - **Ctrl+←/→** changes the terminal width; **Ctrl+↑/↓** changes the Hex strip height.
- **Each connection remembers its own settings** (`serial:COM22`, `tcp:10.0.0.5:2101`...): hex format, line ending, HEX/ASCII, char delay, logging and pane sizes. A switch and a scanner feed can each keep their own setup.
- **The Hex pane groups bytes into one entry per line**, starting a new entry on a newline, a 30 ms gap, a change of direction, or every 1 KB. **F8** cycles the format: compact (one row each) → hex+text → hex → text.

## Sending

| Mode | How |
|------|-----|
| Line mode (default) | Type, Enter sends. **Tab** HEX↔ASCII. **↑↓** history. **F2** line ending (default **CR**: CRLF counts as two Enters on most switches). |
| HEX | `02 41 03`, `024103`, `0x02,0x41,0x03`, `02:41:03`. Sent exactly as typed. |
| ASCII | Text, then the line ending. Also takes `\r \n \t \0 \e \\ \xHH` and the display tokens `<STX> <CR> <0x1B>`, so a received TEXT line can be pasted back. `\<` sends a literal `<`. |
| Live keys (**F4**) | Every key goes straight to the device: Tab, `?`, space at `--More--`, Ctrl+C/Z, arrows. F-keys still work. Press F4 again to get back to line mode, where Ctrl+Q/R/N work. |
| Hidden send (**F6**) | Typed masked, then sent with the line ending. Logged only as `<hidden>` (not even its length) and never kept in history. Works from line mode or live keys. |

At a `Password:` / `Passcode:` / `Secret:` / `PIN:` prompt, what you type is never recorded: in live keys each key is logged as `<hidden>`, and in line mode the send box shows `PASSWORD · not recorded`, masks what you type, logs the send as `<hidden>` and keeps it out of history. A prompt worded differently isn't recognised, so use **F6** hidden send there.

## Menu (F1)

**F1** opens a menu in the middle of the screen. It lists its sections (Session, Send, Copy & paste, View, Connection options, Session log, Macros, Updates, Debug), each with a hint of what's inside, then *Keyboard shortcuts* and *Quit*.
- **↑↓** to move.
- **Enter** opens a section; **Esc** goes back to the sections.
- **←→** changes a setting straight away. That includes the on/off switches: live keys, raw log following/paused and the PgUp/PgDn scroll pane. The menu stays open.
- **Enter** runs an action.
- **Typing** searches every section at once (`delay`, `break`, `save`...); each result shows its section, such as *Connection options › Char delay*.
- **Esc** clears the search, then goes back, then closes the menu.

The **Session log** section is this session's recording: *Log to file*, *Name this session log…* (adds a name after the date and time in the file names), *Open session log*, *Open transcript*, *Open session logs folder* and *Copy session log path*.

The **Debug** section at the bottom has what a bug report needs: *Copy log* (RawTap's own debug log, not your sessions), *Open log folder*, *Copy system info* and the *Log level*.

A short description of the highlighted item is shown at the bottom. On screen, the footer shows only F1 Menu, F3 View, F4 Live keys, F8 Hex format (when the Hex pane shows), Ctrl+N New connection and Ctrl+Q Quit.

The connection panel has a **Connection options ▸** row (←→ or Enter to open it). It shows the line ending, char delay, send mode, view and *Log to file* remembered for the selected port or host, so you can set them before connecting. *Log to file* is also in **F1 → Session log**, and changing it there is remembered too. The view is **Last used** (whichever you used last) or always **Terminal**, **Split** or **Hex** for that connection. The line under the panel lists what Enter and the other keys do for the highlighted row.

## Mouse, copy and paste

These follow Windows Terminal conventions:

| Action | How |
|--------|-----|
| Scroll back | Mouse wheel over either pane scrolls that pane; nothing is sent to the device |
| Select | Drag in the terminal or raw log pane. Only that pane's text is selected, with no borders. To go beyond one screen, drag past the top or bottom edge (it auto-scrolls) or use the wheel while dragging. **Ctrl+click** extends an existing selection; scroll first if you need to. |
| Raw log copies | Always whole entries, in full log-file form (`[time] RX HEX: …` / `RX TEXT: …`), even if the screen shows the compact view. Hidden sends stay `<hidden>`. |
| Select all | **Alt+A** selects and copies the whole pane you're in: the terminal with its scrollback, or every raw log entry. In line mode **Ctrl+A** (or Ctrl+Shift+A) does too; in live keys Ctrl+A goes to the device (most terminals send Ctrl+Shift+A as Ctrl+A). Also **F1 → Select all**. |
| Copy | **Automatic:** a selection is copied as soon as you let go of the mouse (or select all), and stays highlighted. **Ctrl+C** or **right-click** on a selection copies it again and clears the highlight. Ctrl+C copies only while there's a selection; otherwise it does what it always does. GNOME Terminal keeps **Ctrl+Shift+C** for its own copy, which can't see RawTap's selection, so use Ctrl+C. |
| Paste | **Right-click** with nothing selected (your terminal's own paste works too). One line goes into the send box (in live keys, straight to the device). **Several lines ask for confirmation**, then go line by line with the line ending and the char delay. |
| Long output | F1 → *Copy all terminal output*, or F1 → Session log → *Open transcript* (Notepad on Windows, your default text app on Linux) |
| The console's own selection | **Shift+drag** |

**Ctrl+Q** quits, from the start screen, a session or the menu. In live keys every key goes to the device, but the F1 menu still works: **F1, then Ctrl+Q** quits (and **F1, then Ctrl+N** starts a new connection). Searching the menu for "exit" finds Quit too. Ctrl+C never quits; with a selection it copies. (In Windows Terminal, Shift+click is taken by the console's own selection, which is why extending uses Ctrl+click.)

## Shortcut keys

Everything below is also in the F1 menu. The shortcuts are for when you already know what you want.

| Key | Action |
|-----|--------|
| F1 | menu |
| F2 / F3 / F4 | line ending / view (Terminal, Split, Hex; also click it in the footer) / live keys |
| F5 | which pane PgUp/PgDn scroll (Split view) |
| F6 / F7 | hidden send / break (300 ms on serial, for password recovery and ROMMON; Telnet's break command on Telnet) |
| F8 / F9 / F10 | hex format / pause-follow the Hex pane / clear both panes (the files keep everything) |
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
| `logs/rawtap_<type>_<time>[_<name>].log` | Everything sent and received: timestamp, RX/TX, hex and text. Passwords and hidden sends show only `<hidden>`. The name is there once you name the session log. |
| `logs/rawtap_<type>_<time>[_<name>].txt` | Plain-text transcript of what the device printed, with escape codes and `--More--` cleaned out |
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
rawtap telnet -H 192.168.1.1 [-p 23]     Telnet login to a switch or router
rawtap ssh [user@]host [-p 22]           SSH login, through your system's ssh
rawtap open "Core switch"                open a favourite by name
rawtap serial -p /dev/ttyUSB0 -b 9600    (Linux)
rawtap ports                             list serial ports
rawtap update                            update to the latest release
rawtap --system-info                     version, OS and terminal, for bug reports
--no-log                                 don't write log/transcript files
```

## Image extraction

Same settings as the Python version (`image_extraction` in the config), plus:
- `file_extension` (default `bmp`)
- `max_payload_bytes` (default 16 MB), so a corrupt size field can't make it buffer forever.

## Limitations

- For a switch's Telnet login (port 23), choose **Telnet** as the connection: RawTap answers the Telnet negotiation (it lets the device echo, reports its window size and says it's an XTERM) and notes what was agreed in the raw log. **TCP client** stays raw: every byte, no negotiation, for terminal servers and devices.
- **SSH** runs your system's own `ssh` (OpenSSH; on Windows 10+ it's the optional "OpenSSH Client" feature) inside RawTap's terminal pane, so your keys, agent, `~/.ssh/config` and `known_hosts` work as usual and RawTap never handles credentials. Before starting ssh, RawTap checks that the server answers (at the address and port your `~/.ssh/config` gives it; a jump host is left to ssh), so an unreachable server fails straight away with the reason instead of a blank screen. It starts in live keys, so ssh's own questions (host key, password) can be answered; a password typed or pasted there is logged as `<hidden>`. The raw log shows what the remote prints, after decryption.
- Break works on serial ports and Telnet (not SSH).
- When the window is resized, existing terminal lines aren't re-wrapped (PuTTY doesn't re-wrap them either).

## Code layout

| File | Contents |
|------|----------|
| `src/codec.rs` | hex/text formatting and send parsing |
| `src/link.rs` | serial, TCP client and TCP server (a reader thread each), and break |
| `src/term.rs` | terminal emulation (vt100), live-key encoding, plain-text cleaner and transcript |
| `src/macros.rs` | macro keys and the step runner |
| `src/menu.rs` | the F1 menu: sections, items, descriptions, search |
| `src/clip.rs` | clipboard (in-memory during tests) |
| `src/monitor.rs` | monitor state: raw log grouping, sending, live keys, hidden send, macros |
| `src/monitor/view.rs`, `src/monitor/keys.rs` | panes, mouse, copy and paste / commands, the F1 menu and keys |
| `src/ui_monitor.rs` | drawing the monitor: layouts and header |
| `src/ui_monitor/panes.rs`, `src/ui_monitor/controls.rs`, `src/ui_monitor/menu_popup.rs` | drawing the raw log and terminal / the send box, footer and help / the F1 menu |
| `src/app.rs`, `src/ui.rs` | start screen and app wiring |
| `src/app/form.rs`, `src/app/setup.rs` | the start screen's keys and fields / its rows and connection settings |
| `src/app/favourites.rs`, `src/ui/favourites.rs` | favourites: saving, editing, removing, `rawtap open` / their rows and dialog |
| `src/app/connect.rs`, `src/app/update_prompt.rs` | connecting in the background / update checks and the Update Available dialog |
| `src/extract.rs`, `src/logger.rs`, `src/config.rs` | image extraction, traffic log file, settings/state |
| `src/paths.rs`, `src/json_store.rs` | where files live (and the move from 0.6.0's locations); atomic saves |
| `src/session_log.rs`, `src/system_info.rs` | RawTap's own session log and panic hook; system info for bug reports |
| `src/opener.rs`, `src/app/identity.rs` | opening files and folders; the app ID `info.brumley.rawtap` |
| `src/switch_sim.rs`, `src/screen_tests.rs` | end-to-end test against a fake switch / screens drawn in a test terminal |
