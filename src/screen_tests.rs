//! Screen tests: the start screen, the dialogs and the monitor drawn into a
//! test terminal and driven with keys and mouse events.

use super::*;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::io::{Read, Write};

mod favourites;
mod passwords;

fn key(app: &mut App, code: KeyCode) {
    app.on_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}
fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}
/// In a new connection's panel (TCP, Telnet or SSH): the host, then the
/// field under it.
fn fill_host_and_port(app: &mut App, host: &str, port: &str) {
    key(app, KeyCode::Down); // Connection
    key(app, KeyCode::Down); // Host
    typ(app, host);
    key(app, KeyCode::Down);
    typ(app, port);
}

fn dump(t: &Terminal<TestBackend>, title: &str) {
    let buf = t.backend().buffer();
    println!("===== {title}");
    for y in 0..buf.area.height {
        let row: String = (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        println!("{}", row.trim_end());
    }
}

fn screen_text(t: &Terminal<TestBackend>) -> String {
    let buf = t.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn update_dialog_shows_versions_and_buttons() {
    let dir = std::env::temp_dir().join(format!("rawtap_dialog_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = paths::Paths::at(dir.clone());
    let (cfg, notes) = config::load(&paths).unwrap();
    let mut app = App::new(cfg, paths, config::Saved::default(), notes, true);
    app.update_prompt = Some(app::update_prompt::UpdatePrompt::new(update::UpdateCheck {
        current_version: "0.7.1".into(),
        latest_version: "0.7.2".into(),
        update_available: true,
        release_url: "https://github.com/jackbrumley/rawtap/releases/tag/v0.7.2".into(),
    }));
    let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "update dialog");
    let screen = screen_text(&t);
    assert!(screen.contains("Update Available"));
    // Wrapped to the dialog's width, so check the pieces.
    assert!(screen.contains("Current: v0.7.1 -> Latest:"));
    assert!(screen.contains("v0.7.2."));
    for button in ["Later", "View Release Notes", "Update Now"] {
        assert!(screen.contains(button), "missing {button}");
    }
    // Keys go to the dialog, not the start screen behind it; Esc is Later.
    key(&mut app, KeyCode::Tab);
    assert!(app.update_prompt.is_some());
    key(&mut app, KeyCode::Esc);
    assert!(app.update_prompt.is_none());
    assert!(!app.quit);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_dialog_sits_over_the_start_screen_box_in_a_tall_window() {
    let dir = std::env::temp_dir().join(format!("rawtap_tall_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = paths::Paths::at(dir.clone());
    let (cfg, _) = config::load(&paths).unwrap();
    let saved = config::Saved {
        connection: config::Connection {
            mode: ConnMode::Tcp,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut app = App::new(cfg, paths, saved, Vec::new(), true);
    key(&mut app, KeyCode::Enter); // New connection: the taller box
    app.update_prompt = Some(app::update_prompt::UpdatePrompt::new(update::UpdateCheck {
        current_version: "0.7.2".into(),
        latest_version: "0.7.3".into(),
        update_available: true,
        release_url: "https://github.com/jackbrumley/rawtap/releases/tag/v0.7.3".into(),
    }));
    let mut t = Terminal::new(TestBackend::new(120, 70)).unwrap();
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    let screen = screen_text(&t);
    let row_of = |text: &str| screen.lines().position(|l| l.contains(text)).unwrap();
    let box_bottom = row_of("Ctrl+Q quit");
    let dialog_top = row_of("Update Available");
    assert!(
        dialog_top < box_bottom,
        "dialog (row {dialog_top}) should sit over the start screen box (bottom row {box_bottom})"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Enter shows a spinner (and clears the last error) while the connection
/// opens in the background; Esc cancels; a failure ends with the reason.
#[test]
fn connecting_shows_a_spinner_then_the_error() {
    let dir = std::env::temp_dir().join(format!("rawtap_connecting_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = paths::Paths::at(dir.clone());
    let (cfg, _) = config::load(&paths).unwrap();
    // A port nobody listens on: the attempt fails with "refused".
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let saved = config::Saved {
        connection: config::Connection {
            mode: ConnMode::Tcp,
            tcp_host: "127.0.0.1".into(),
            tcp_port: port.to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut app = App::new(cfg, paths, saved, Vec::new(), true);
    app.setup.error = Some("an earlier error".into());
    let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();

    key(&mut app, KeyCode::Enter); // New connection: TCP, the last type used
    fill_host_and_port(&mut app, "127.0.0.1", &port.to_string());
    key(&mut app, KeyCode::Enter); // connect
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    let screen = screen_text(&t);
    assert!(screen.contains("Connecting to TCP 127.0.0.1"), "{screen}");
    assert!(screen.contains("Esc cancels"));
    assert!(!screen.contains("an earlier error"));

    // Esc cancels; Enter tries again.
    key(&mut app, KeyCode::Esc);
    assert!(app.connecting.is_none() && !app.quit);
    assert_eq!(app.setup.error.as_deref(), Some("Cancelled."));
    key(&mut app, KeyCode::Enter);
    assert!(app.connecting.is_some() && app.setup.error.is_none());

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while app.connecting.is_some() && std::time::Instant::now() < deadline {
        app.tick();
        std::thread::sleep(Duration::from_millis(20));
    }
    let error = app.setup.error.clone().unwrap_or_default();
    assert!(error.contains("Failed to connect"), "{error}");
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    assert!(screen_text(&t).contains("to connect"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The footer's view switch: a click on the current view changes it, and
/// the last view used is where the next connection opens.
#[test]
fn the_view_switch_is_clickable_and_follows_you() {
    let dir = std::env::temp_dir().join(format!("rawtap_view_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            std::thread::sleep(Duration::from_secs(5));
            drop(stream);
        }
    });
    let connect = |view: config::Layout| {
        let paths = paths::Paths::at(dir.clone());
        let (cfg, _) = config::load(&paths).unwrap();
        let saved = config::Saved {
            connection: config::Connection {
                mode: ConnMode::Tcp,
                tcp_host: "127.0.0.1".into(),
                tcp_port: port.to_string(),
                ..Default::default()
            },
            view,
            ..Default::default()
        };
        let mut app = App::new(cfg, paths, saved, Vec::new(), true);
        app.request_connect();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app.monitor.is_none() && std::time::Instant::now() < deadline {
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        app
    };

    let mut app = connect(config::Layout::Split);
    let mut t = Terminal::new(TestBackend::new(120, 30)).unwrap();
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    let footer = screen_text(&t).lines().last().unwrap().to_string();
    // The key, what it does, then the current view; a click on the view
    // changes to the next one, as F3 does (Split -> Hex).
    assert!(footer.contains("F3  View  Split "), "{footer}");
    let hex = app.monitor.as_ref().unwrap().footer_view.unwrap();
    app.on_event(Event::Mouse(ratatui::crossterm::event::MouseEvent {
        kind: ratatui::crossterm::event::MouseEventKind::Down(
            ratatui::crossterm::event::MouseButton::Left,
        ),
        column: hex.x + 1,
        row: hex.y,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.monitor.as_ref().unwrap().layout, config::Layout::Hex);
    app.shutdown().unwrap();
    assert_eq!(app.saved.view, config::Layout::Hex);

    // A new connection opens in the last view used.
    let mut app = connect(app.saved.view);
    assert_eq!(app.monitor.as_ref().unwrap().layout, config::Layout::Hex);

    // F1 → "Save as favourite…" saves the session's connection.
    let action = app
        .monitor
        .as_mut()
        .unwrap()
        .run_cmd(menu::Cmd::SaveFavourite);
    assert!(matches!(action, monitor::Action::SaveFavourite));
    app.save_session_as_favourite();
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.saved.favourites.len(), 1);
    app.shutdown().unwrap();

    // The start screen's Options give a connection its own view, which
    // opens whatever was used last and doesn't become "last used" unless
    // the user changes it.
    let mut saved = app.saved.clone();
    saved.view = config::Layout::Split;
    let paths = paths::Paths::at(dir.clone());
    let (cfg, _) = config::load(&paths).unwrap();
    let mut app = App::new(cfg, paths, saved, Vec::new(), true);
    app.setup.focus = 0;
    key(&mut app, KeyCode::Enter); // New connection
    let (host, port) = (
        app.saved.connection.tcp_host.clone(),
        app.saved.connection.tcp_port.clone(),
    );
    fill_host_and_port(&mut app, &host, &port);
    let panel = app.setup.panel.as_mut().unwrap();
    panel.options_open = true;
    panel.focus = panel
        .fields()
        .iter()
        .position(|f| *f == app::setup::Field::OptLayout)
        .unwrap();
    for _ in 0..3 {
        key(&mut app, KeyCode::Right); // Last used -> Terminal -> Split -> Hex
    }
    let panel = app.setup.panel.as_ref().unwrap();
    assert_eq!(app.field_value(panel, app::setup::Field::OptLayout), "Hex");
    let mut t = Terminal::new(TestBackend::new(100, 34)).unwrap();
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "a connection's own view");
    key(&mut app, KeyCode::Enter); // connect
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while app.monitor.is_none() && std::time::Instant::now() < deadline {
        app.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.monitor.as_ref().unwrap().layout, config::Layout::Hex);
    app.shutdown().unwrap();
    assert_eq!(app.saved.view, config::Layout::Split);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn long_start_screen_notes_wrap_with_their_indent() {
    let dir = std::env::temp_dir().join(format!("rawtap_notes_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = paths::Paths::at(dir.clone());
    let (cfg, _) = config::load(&paths).unwrap();
    let note = "Update available: RawTap 0.7.1 → 0.7.2. Ctrl+U to update, or open F1 and choose Updates, or run rawtap update from a terminal.".to_string();
    // TCP mode: no serial port list to push the note off the screen.
    let saved = config::Saved {
        connection: config::Connection {
            mode: ConnMode::Tcp,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut app = App::new(cfg, paths, saved, vec![note], true);
    let mut t = Terminal::new(TestBackend::new(90, 40)).unwrap();
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    let screen = screen_text(&t);
    let rows: Vec<&str> = screen
        .lines()
        .skip_while(|l| !l.contains("Update available"))
        .take(2)
        .collect();
    assert_eq!(rows.len(), 2, "the note should wrap onto a second row");
    let indent = |row: &str| row.find(|c: char| c.is_alphanumeric()).unwrap();
    assert_eq!(indent(rows[0]), indent(rows[1]), "{rows:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn walkthrough() {
    // Echo server that also says hello.
    let srv = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = srv.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut s, _) = srv.accept().unwrap();
        s.write_all(b"\x02HELLO\x03\r\n").unwrap();
        let mut buf = [0u8; 256];
        while let Ok(n) = s.read(&mut buf) {
            if n == 0 {
                break;
            }
            s.write_all(&buf[..n]).unwrap();
        }
    });

    let dir = std::env::temp_dir().join(format!("rawtap_ui_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths = paths::Paths::at(dir.clone());
    let (cfg, notes) = config::load(&paths).unwrap();
    let saved = config::Saved::default();
    let mut app = App::new(cfg, paths, saved, notes, false);
    let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();

    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "start screen");

    key(&mut app, KeyCode::Enter); // New connection
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "new connection (serial)");
    key(&mut app, KeyCode::Down); // Name -> Connection
    key(&mut app, KeyCode::Right); // -> TCP client
    key(&mut app, KeyCode::Down);
    typ(&mut app, "127.0.0.1");
    key(&mut app, KeyCode::Down);
    typ(&mut app, &port.to_string());
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "setup (tcp filled)");

    key(&mut app, KeyCode::Enter);
    app.tick();
    std::thread::sleep(Duration::from_millis(300));
    app.tick();
    typ(&mut app, "<STX>PING<ETX>");
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "monitor, typing ascii");
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Tab);
    typ(&mut app, "02 41 30 31 03");
    key(&mut app, KeyCode::Enter);
    typ(&mut app, "02 zz");
    std::thread::sleep(Duration::from_millis(300));
    app.tick();
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "monitor after sends, bad hex pending");

    key(&mut app, KeyCode::F(1));
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "help");
    app.shutdown().unwrap();
    let logs = std::fs::read_dir(dir.join("logs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    println!("===== log file\n{}", std::fs::read_to_string(logs).unwrap());
    println!(
        "===== state\n{}",
        std::fs::read_to_string(dir.join(paths::STATE_FILE)).unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
