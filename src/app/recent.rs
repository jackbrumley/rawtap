//! Recent connections: the last few connections opened, newest first,
//! listed on the start screen under the favourites, so one you used lately
//! is a keypress (or a click) away. They live in state.json.

use super::App;
use super::setup::{Panel, connection_profile_key};
use crate::config::{Connection, Saved};

/// How many recent connections are kept.
pub const RECENT_MAX: usize = 5;

/// Puts the connection just opened first. The same port or host is kept
/// once, with the settings it was last opened with.
pub fn remember(saved: &mut Saved, connection: &Connection) {
    let key = connection_profile_key(connection);
    saved.recent.retain(|c| connection_profile_key(c) != key);
    saved.recent.insert(0, connection.clone());
    saved.recent.truncate(RECENT_MAX);
}

/// The recent connections the start screen lists: those that aren't
/// already a favourite (which is listed above them).
pub fn visible(saved: &Saved) -> Vec<usize> {
    (0..saved.recent.len())
        .filter(|&i| {
            let key = connection_profile_key(&saved.recent[i]);
            !saved
                .favourites
                .iter()
                .any(|f| connection_profile_key(&f.connection) == key)
        })
        .collect()
}

impl App {
    /// Enter (or a click) on a recent connection: connect to it.
    pub(super) fn connect_recent(&mut self, i: usize) {
        if let Some(c) = self.saved.recent.get(i) {
            self.saved.connection = c.clone();
            self.request_connect();
        }
    }

    /// E or F2 on a recent connection: the connection panel, filled in, to
    /// change it or save it as a favourite (Ctrl+S).
    pub(super) fn edit_recent(&mut self, i: usize) {
        if let Some(c) = self.saved.recent.get(i) {
            self.setup.error = None;
            self.setup.panel = Some(Panel::new(None, String::new(), c.clone()));
        }
    }

    /// Del on a recent connection: drops it from the list.
    pub(super) fn forget_recent(&mut self, i: usize) {
        if i < self.saved.recent.len() {
            self.saved.recent.remove(i);
            self.store_state_reporting();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConnMode, Favourite};

    fn ssh(host: &str) -> Connection {
        Connection {
            mode: ConnMode::Ssh,
            ssh_host: host.into(),
            ..Default::default()
        }
    }

    #[test]
    fn newest_first_once_each_and_at_most_five() {
        let mut saved = Saved::default();
        for host in ["a", "b", "c", "d", "e", "f"] {
            remember(&mut saved, &ssh(host));
        }
        remember(&mut saved, &ssh("d"));
        let hosts: Vec<&str> = saved.recent.iter().map(|c| c.ssh_host.as_str()).collect();
        assert_eq!(hosts, ["d", "f", "e", "c", "b"]);
    }

    #[test]
    fn a_favourite_isnt_listed_again_as_recent() {
        let mut saved = Saved::default();
        remember(&mut saved, &ssh("a"));
        remember(&mut saved, &ssh("b"));
        saved.favourites.push(Favourite {
            name: "B".into(),
            connection: ssh("b"),
        });
        assert_eq!(visible(&saved), [1]);
    }
}
