//! Update checks and the "Update Available" dialog (Release Standard section
//! 7, as in Voquill): current → latest with **Later**, **View Release Notes**
//! and **Update Now**; a check the user asked for that finds nothing new says
//! "RawTap is Up to Date" with **Close**.
//!
//! The dialog opens by itself once a run, as soon as nothing else needs the
//! keyboard: never over live keys, a hidden send, a paste confirmation, a
//! running macro or the F1 menu, so keys meant for the device never land in
//! it.

use super::{App, Screen};
use crate::update::UpdateCheck;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromptButton {
    Later,
    ReleaseNotes,
    UpdateNow,
}

impl PromptButton {
    pub fn label(self, update_available: bool) -> &'static str {
        match self {
            PromptButton::Later if update_available => "Later",
            PromptButton::Later => "Close",
            PromptButton::ReleaseNotes => "View Release Notes",
            PromptButton::UpdateNow => "Update Now",
        }
    }
}

pub struct UpdatePrompt {
    pub check: UpdateCheck,
    pub selected: PromptButton,
    /// A failure from the last button (e.g. no browser), shown in the dialog.
    pub error: Option<String>,
}

impl UpdatePrompt {
    pub(crate) fn new(check: UpdateCheck) -> UpdatePrompt {
        let selected = if check.update_available {
            PromptButton::UpdateNow
        } else {
            PromptButton::Later
        };
        UpdatePrompt {
            check,
            selected,
            error: None,
        }
    }

    /// The buttons, left to right.
    pub fn buttons(&self) -> &'static [PromptButton] {
        if self.check.update_available {
            &[
                PromptButton::Later,
                PromptButton::ReleaseNotes,
                PromptButton::UpdateNow,
            ]
        } else {
            &[PromptButton::Later]
        }
    }

    fn move_selection(&mut self, step: isize) {
        let buttons = self.buttons();
        let current = buttons
            .iter()
            .position(|b| *b == self.selected)
            .unwrap_or(0) as isize;
        let next = (current + step).rem_euclid(buttons.len() as isize) as usize;
        self.selected = buttons[next];
    }
}

impl App {
    /// Starts a background update check. `manual`: the user asked, so the
    /// dialog opens whatever it finds.
    pub fn start_update_check(&mut self, manual: bool) {
        if self.update_check.is_none() {
            self.update_check = Some((crate::update::spawn_check(), manual));
        }
    }

    pub(super) fn poll_update_check(&mut self) {
        let Some((rx, manual)) = &self.update_check else {
            return;
        };
        let manual = *manual;
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("The update check stopped".into()),
        };
        self.update_check = None;
        self.saved.last_update_check = crate::update::last_checked();
        match result {
            Ok(check) => {
                if check.update_available {
                    // Stays on the start screen after "Later".
                    self.notes.push(format!(
                        "Update available: RawTap {} → {}. Ctrl+U to update.",
                        check.current_version, check.latest_version
                    ));
                }
                if manual || (check.update_available && !self.prompt_shown) {
                    self.pending_prompt = Some(check);
                }
            }
            Err(error) if manual => match self.monitor.as_mut() {
                Some(m) => m.report_error(error),
                None => self.setup.error = Some(error),
            },
            // A silent start-up check only logs its failure (Release Standard section 7).
            Err(_) => {}
        }
    }

    /// Nothing else is using the keyboard, so the dialog can't swallow keys
    /// meant for the device or another prompt.
    fn can_show_prompt(&self) -> bool {
        match self.screen {
            Screen::Setup => true,
            Screen::Monitor => self.monitor.as_ref().is_some_and(|m| {
                !m.live
                    && m.menu.is_none()
                    && m.secret.is_none()
                    && m.paste_confirm.is_none()
                    && m.run.is_none()
                    && !m.show_help
            }),
        }
    }

    pub(super) fn show_pending_prompt(&mut self) {
        if self.update_prompt.is_some() || !self.can_show_prompt() {
            return;
        }
        let Some(check) = self.pending_prompt.take() else {
            return;
        };
        log::info!(
            "Showing the update dialog: {} → {} (available: {})",
            check.current_version,
            check.latest_version,
            check.update_available
        );
        self.prompt_shown = true;
        self.update_prompt = Some(UpdatePrompt::new(check));
    }

    /// Keys while the dialog is open: ←/→ or Tab choose, Enter presses,
    /// Esc is Later / Close. Nothing reaches the screen behind it.
    pub(super) fn update_prompt_key(&mut self, k: KeyEvent) {
        let Some(prompt) = self.update_prompt.as_mut() else {
            return;
        };
        match k.code {
            KeyCode::Left | KeyCode::BackTab => prompt.move_selection(-1),
            KeyCode::Right | KeyCode::Tab => prompt.move_selection(1),
            KeyCode::Esc => self.update_prompt = None,
            KeyCode::Enter => match prompt.selected {
                PromptButton::Later => self.update_prompt = None,
                PromptButton::ReleaseNotes => {
                    let url = prompt.check.release_url.clone();
                    prompt.error = crate::opener::open_url(&url).err();
                }
                PromptButton::UpdateNow => {
                    self.update_prompt = None;
                    self.request_update();
                }
            },
            _ => {}
        }
    }

    /// Ctrl+U / F1 → Update now / Update Now: update when a newer version is
    /// known, otherwise check first.
    pub(super) fn request_update(&mut self) {
        if crate::update::available().is_some() {
            self.update_requested = true;
            self.quit = true;
        } else {
            self.start_update_check(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(update_available: bool) -> UpdateCheck {
        UpdateCheck {
            current_version: "0.7.1".into(),
            latest_version: "0.7.2".into(),
            update_available,
            release_url: "https://github.com/jackbrumley/rawtap/releases/tag/v0.7.2".into(),
        }
    }

    #[test]
    fn update_now_is_selected_and_choices_wrap() {
        let mut prompt = UpdatePrompt::new(check(true));
        assert_eq!(prompt.selected, PromptButton::UpdateNow);
        prompt.move_selection(1);
        assert_eq!(prompt.selected, PromptButton::Later);
        prompt.move_selection(-1);
        prompt.move_selection(-1);
        assert_eq!(prompt.selected, PromptButton::ReleaseNotes);
    }

    #[test]
    fn up_to_date_dialog_only_closes() {
        let prompt = UpdatePrompt::new(check(false));
        assert_eq!(prompt.buttons(), &[PromptButton::Later]);
        assert_eq!(PromptButton::Later.label(false), "Close");
    }
}
