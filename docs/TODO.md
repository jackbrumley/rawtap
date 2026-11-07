# RawTap TODO

Open work only; remove items when done. The target is the App Standard and
the Release and Update Standard (see `AGENTS.md`).

## Now

- **Hand-test the installs and the update:** every install mode on Linux
  and Windows, and the in-app update from v0.7.0 to v0.7.1 (Ctrl+U).

## Next

- **Passwords:** never in RawTap's own files. "Set up key login" for SSH
  (ssh-keygen, then copy the key over); an opt-in "Remember password" in
  the OS keyring (GNOME Keyring / KWallet, Windows Credential Manager), given
  to ssh through SSH_ASKPASS and sent hidden at the first password prompt on
  Telnet and serial; "Forget password". Not offered without a keyring.

## Ideas / Parking Lot

- macOS build.
- OSC 52 clipboard for RawTap running over SSH.
