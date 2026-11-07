//! A program in a pseudo-terminal: SSH runs the system's `ssh` here, so
//! OpenSSH keeps doing what it does well (keys, passwords, known_hosts, the
//! agent) and RawTap never handles credentials. `portable-pty` gives a
//! pseudo-terminal on Linux (openpty) and Windows (ConPTY).
//!
//! Two threads besides the link's writer: a reader (output →
//! `LinkEvent::Data`) and a waiter. When the program exits, the waiter closes
//! the pseudo-terminal (so the reader sees end-of-file on every platform),
//! waits for the reader to pass on the last output, then reports the exit
//! as `LinkEvent::Closed`.

use super::LinkEvent;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

type SharedMaster = Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>;

pub struct PtySession {
    master: SharedMaster,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

/// What `spawn` hands back: the session, the program's input, and the
/// waiter thread, which ends once the program and the reader have.
pub struct Spawned {
    pub session: PtySession,
    pub input: Box<dyn Write + Send>,
    pub waiter: JoinHandle<()>,
}

/// Starts `program` with `args` in an 80×24 pseudo-terminal (the monitor
/// reports the pane's real size straight after).
pub fn spawn(program: &str, args: &[String], tx: Sender<LinkEvent>) -> Result<Spawned, String> {
    if !on_path(program) {
        return Err(missing_program_message(program));
    }
    let pair = native_pty_system()
        .openpty(PtySize::default())
        .map_err(|e| format!("Could not open a pseudo-terminal: {e}"))?;
    let mut command = CommandBuilder::new(program);
    command.args(args);
    // What RawTap's terminal pane emulates.
    command.env("TERM", "xterm-256color");
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| format!("Could not start {program}: {e}"))?;
    // Only the child keeps the terminal's other end, so its exit is seen.
    drop(pair.slave);
    let mut output = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Could not read from {program}: {e}"))?;
    let input = pair
        .master
        .take_writer()
        .map_err(|e| format!("Could not write to {program}: {e}"))?;
    let killer = child.clone_killer();
    let master: SharedMaster = Arc::new(Mutex::new(Some(pair.master)));

    let reader_tx = tx.clone();
    let reader = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        // End-of-file or an error means the program is gone; the waiter
        // reports how it ended.
        while let Ok(n) = output.read(&mut buf) {
            if n == 0 || reader_tx.send(LinkEvent::Data(buf[..n].to_vec())).is_err() {
                break;
            }
        }
    });

    let (waiter_master, name) = (master.clone(), program.to_string());
    let waiter = thread::spawn(move || {
        let ended = match child.wait() {
            Ok(status) if status.success() => {
                LinkEvent::Ended(format!("{name} ended (you logged out)."))
            }
            // 255 is ssh's own failure: no connection, refused host key, failed login.
            Ok(status) if status.exit_code() == 255 && name == "ssh" => LinkEvent::Closed(format!(
                "ssh ended: {status}. It couldn't connect or log in; its message is in the terminal pane."
            )),
            Ok(status) => LinkEvent::Closed(format!("{name} ended: {status}")),
            Err(e) => LinkEvent::Closed(format!("{name} ended: {e}")),
        };
        // Closing the pseudo-terminal ends the reader (ConPTY doesn't on its
        // own); its last output is posted before the exit.
        waiter_master.lock().unwrap().take();
        let _ = reader.join();
        let _ = tx.send(ended);
    });

    Ok(Spawned {
        session: PtySession { master, killer },
        input,
        waiter,
    })
}

impl PtySession {
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        match self.master.lock().unwrap().as_ref() {
            Some(master) => master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| format!("Could not resize the terminal: {e}")),
            // The program has ended; there's nothing left to resize.
            None => Ok(()),
        }
    }

    /// Ends the program if it's still running; the waiter thread then ends.
    pub fn kill(&mut self) {
        if let Err(e) = self.killer.kill() {
            // Usually it had already exited.
            log::debug!("Stopping the program: {e}");
        }
    }
}

/// Whether `program` can be found on PATH (with `.exe` on Windows).
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{program}.exe"), program.to_string()]
    } else {
        vec![program.to_string()]
    };
    std::env::split_paths(&path).any(|dir| names.iter().any(|name| dir.join(name).is_file()))
}

fn missing_program_message(program: &str) -> String {
    if program == "ssh" {
        if cfg!(windows) {
            "ssh wasn't found. Install the OpenSSH Client: Settings → System → Optional features → Add a feature → OpenSSH Client.".into()
        } else {
            "ssh wasn't found. Install the OpenSSH client, e.g. `sudo apt install openssh-client` or `sudo dnf install openssh-clients`.".into()
        }
    } else {
        format!("{program} wasn't found on PATH.")
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// A shell stands in for ssh: prompts, reads a line, answers, exits 3.
    #[test]
    fn a_program_runs_in_a_pseudo_terminal_and_its_exit_is_reported() {
        let (tx, rx) = mpsc::channel();
        let script = "printf READY; read line; echo GOT:$line; exit 3".to_string();
        let mut spawned = spawn("sh", &["-c".to_string(), script], tx).unwrap();
        let mut seen = String::new();
        let mut sent = false;
        let deadline = Instant::now() + Duration::from_secs(10);
        let closed = loop {
            assert!(
                Instant::now() < deadline,
                "timed out; output so far: {seen:?}"
            );
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(LinkEvent::Data(d)) => seen.push_str(&String::from_utf8_lossy(&d)),
                Ok(LinkEvent::Closed(m) | LinkEvent::Ended(m)) => break m,
                _ => {}
            }
            if seen.contains("READY") && !sent {
                spawned.input.write_all(b"hello\r").unwrap();
                spawned.input.flush().unwrap();
                sent = true;
            }
        };
        assert!(seen.contains("GOT:hello"), "{seen:?}");
        assert!(closed.contains("code 3"), "{closed}");
        spawned.session.kill();
        spawned.waiter.join().unwrap();
    }

    #[test]
    fn a_missing_program_says_how_to_install_it() {
        let (tx, _rx) = mpsc::channel();
        let Err(error) = spawn("rawtap-no-such-program", &[], tx) else {
            panic!("spawning a missing program should fail");
        };
        assert!(error.contains("wasn't found"), "{error}");
    }
}
