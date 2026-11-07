//! Opening a connection on a background thread, so the screen keeps drawing
//! (a spinner, Esc to cancel) while a slow host times out. Each attempt has
//! its own event channel, so a cancelled or late attempt can never leak
//! events into the next connection; a cancelled attempt's link is dropped,
//! and closed, as soon as it finishes opening.

use super::App;
use crate::link::{Link, LinkEvent, LinkSettings};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

pub struct Connecting {
    pub settings: LinkSettings,
    pub started: Instant,
    /// Ctrl+R on the monitor (reattach there), not the start screen.
    pub reconnect: bool,
    result: Receiver<Result<Link, String>>,
    /// The new connection's events: they become the app's once it opens.
    events: Receiver<LinkEvent>,
}

/// The spinner frame for an attempt that started at `started`.
pub fn spinner(started: Instant) -> char {
    const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    FRAMES[(started.elapsed().as_millis() / 80) as usize % FRAMES.len()]
}

impl App {
    pub(super) fn start_connect(&mut self, settings: LinkSettings, reconnect: bool) {
        log::info!("Connecting to {}", settings.describe());
        let (events_tx, events) = mpsc::channel();
        let (result_tx, result) = mpsc::channel();
        let open = settings.clone();
        std::thread::spawn(move || {
            // After a cancel nobody is waiting; the link is then dropped and closed.
            let _ = result_tx.send(Link::open(&open, events_tx));
        });
        self.connecting = Some(Connecting {
            settings,
            started: Instant::now(),
            reconnect,
            result,
            events,
        });
    }

    pub(super) fn poll_connect(&mut self) {
        let Some(attempt) = &self.connecting else {
            return;
        };
        let outcome = match attempt.result.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The connection attempt stopped unexpectedly.".into())
            }
        };
        let Some(attempt) = self.connecting.take() else {
            return;
        };
        match outcome {
            Ok(link) => {
                self.rx = attempt.events;
                if attempt.reconnect {
                    self.reattach(&attempt.settings, link);
                } else {
                    self.open_monitor(attempt.settings, link);
                }
            }
            Err(error) => {
                log::warn!(
                    "Could not connect to {}: {error}",
                    attempt.settings.describe()
                );
                match self.monitor.as_mut().filter(|_| attempt.reconnect) {
                    Some(m) => m.report_error(error),
                    None => self.setup.error = Some(error),
                }
            }
        }
    }

    /// Esc while connecting: stop waiting. The attempt closes itself when it
    /// finishes.
    pub(super) fn cancel_connect(&mut self) {
        if let Some(attempt) = self.connecting.take() {
            log::info!("Cancelled connecting to {}", attempt.settings.describe());
            match self.monitor.as_mut().filter(|_| attempt.reconnect) {
                Some(m) => m.report_error("Reconnect cancelled.".into()),
                None => self.setup.error = Some("Cancelled.".into()),
            }
        }
    }
}
