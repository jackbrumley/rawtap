# RawTap TODO

Open work only; remove items when done. The target is the App Standard and
the Release and Update Standard (see `AGENTS.md`).

## Now

- **Hand-test the installs and the update:** every install mode on Linux
  and Windows, and the in-app update from v0.7.0 to v0.7.1 (Ctrl+U).

## Next

- **SSH connection (v0.9.0)**, a new `ConnMode::Ssh` (host, user, port):
  run the system's `ssh` (OpenSSH; built into Windows 10+) in a
  pseudo-terminal with the `portable-pty` crate (openpty on Linux, ConPTY on
  Windows), as a new `Writer::Pty` with a reader thread like the others,
  and `Link::resize` (added for Telnet in v0.8.0) resizing the PTY. `ssh`
  owns keys, passwords, `known_hosts` and the agent; RawTap never sees
  credentials, and its Password-prompt masking keeps typed passwords out of
  the logs. Starts in live keys; ssh's exit becomes `LinkEvent::Closed` with
  its status; a missing `ssh` is a clear error. Profile key
  `ssh:user@host:port`; CLI `rawtap ssh [user@]host [-p port]`. The raw log
  shows what the remote prints (after decryption), not SSH packets. Tested
  with a shell in a pseudo-terminal standing in for `ssh`.

## Ideas / Parking Lot

- macOS build.
- OSC 52 clipboard for RawTap running over SSH.
