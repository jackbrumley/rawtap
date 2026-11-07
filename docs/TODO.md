# RawTap TODO

Open work only; remove items when done. The target is the App Standard and
the Release and Update Standard (see `AGENTS.md`).

## Now

- **Hand-test the installs and the update:** every install mode on Linux
  and Windows, and the in-app update from v0.7.0 to v0.7.1 (Ctrl+U).

## Next

- **SSH and Telnet connections (v0.8.0)**, built in this order:
  1. **Terminal size reporting:** `Link::resize(rows, cols)`, called when the
     terminal pane's size changes (`term.rs` `resize`); serial and raw TCP
     ignore it. Both new link types need it.
  2. **Telnet** (a "Telnet" option on TCP client, port 23): handle IAC
     negotiation in the link reader. Accept ECHO and SUPPRESS-GO-AHEAD,
     answer NAWS with the pane size and TERMINAL-TYPE with `xterm`, refuse
     the rest; escape 0xFF in what we send. The raw log keeps every byte,
     IAC included; the terminal pane gets the data without them. Pure Rust,
     no new crate; tested against a fake Telnet switch like `switch_sim.rs`.
     Removes the README's "no telnet negotiation" limitation.
  3. **SSH** (a new `ConnMode::Ssh`: host, user, port): run the system's
     `ssh` (OpenSSH; built into Windows 10+) in a pseudo-terminal with the
     `portable-pty` crate (openpty on Linux, ConPTY on Windows), as a new
     `Writer::Pty` with a reader thread like serial and TCP. `ssh` owns
     keys, passwords, `known_hosts` and the agent; RawTap never sees
     credentials, and its Password-prompt masking keeps typed passwords out
     of the logs. Starts in live keys; ssh's exit becomes `LinkEvent::Closed`
     with its status; a missing `ssh` is a clear error. Profile key
     `ssh:user@host:port`; CLI `rawtap ssh [user@]host [-p port]`. The raw
     log shows what the remote prints (after decryption), not SSH packets.
     Tested with a shell in a pseudo-terminal standing in for `ssh`.


## Ideas / Parking Lot

- macOS build.
- OSC 52 clipboard for RawTap running over SSH.
- Telnet option negotiation for port 23.
