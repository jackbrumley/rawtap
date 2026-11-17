//! Passwords and the session log: a remembered password is sent once and
//! never logged; a typed one at a password prompt is never recorded; the
//! session log shows ● REC and can be named.

use super::*;
use std::io::{Read, Write};

/// A device that asks for a username, then a password, then (as if the
/// password were wrong) a password again. Returns everything it received.
fn login_device() -> (u16, std::sync::mpsc::Receiver<Vec<u8>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_millis(1500)))
            .unwrap();
        let mut got = Vec::new();
        let read_line = |s: &mut std::net::TcpStream, got: &mut Vec<u8>| {
            let mut b = [0u8; 1];
            while let Ok(1) = s.read(&mut b) {
                got.push(b[0]);
                if b[0] == b'\r' {
                    return;
                }
            }
        };
        // A session that closes early just ends the conversation.
        for prompt in [
            &b"Username: "[..],
            b"\r\nPassword: ",
            b"\r\n% Login invalid\r\nPassword: ",
        ] {
            if s.write_all(prompt).is_err() {
                break;
            }
            read_line(&mut s, &mut got);
        }
        let _ = tx.send(got);
    });
    (port, rx)
}

fn pump(app: &mut App, ms: u64) {
    let end = std::time::Instant::now() + Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        app.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_remembered_password_is_sent_once_at_the_first_prompt_and_never_logged() {
    secrets::use_test_store();
    let (port, received) = login_device();
    let account = format!("tcp:127.0.0.1:{port}");
    secrets::remember(&account, "s3cret-pw").unwrap();

    let dir = std::env::temp_dir().join(format!("rawtap_pw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = paths::Paths::at(dir.clone());
    let (cfg, _) = config::load(&paths).unwrap();
    let saved = config::Saved {
        connection: config::Connection {
            mode: ConnMode::Tcp,
            tcp_host: "127.0.0.1".into(),
            tcp_port: port.to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut app = App::new(cfg, paths, saved, Vec::new(), false);
    app.request_connect();
    pump(&mut app, 400);
    assert!(app.monitor.is_some(), "{:?}", app.setup.error);
    assert_eq!(app.monitor.as_ref().unwrap().password_stored(), Some(true));
    typ(&mut app, "admin");
    key(&mut app, KeyCode::Enter);
    pump(&mut app, 600);
    // The second Password: prompt is the user's: nothing more is sent.
    let got = received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(String::from_utf8_lossy(&got), "admin\rs3cret-pw\r");

    let m = app.monitor.as_ref().unwrap();
    let logged: Vec<u8> = m
        .entries
        .iter()
        .flat_map(|e| match &e.kind {
            monitor::EntryKind::Tx(d) | monitor::EntryKind::Rx(d) => d.clone(),
            _ => Vec::new(),
        })
        .collect();
    assert!(!String::from_utf8_lossy(&logged).contains("s3cret"));
    assert!(
        m.entries
            .iter()
            .any(|e| matches!(e.kind, monitor::EntryKind::TxHidden))
    );
    app.shutdown().unwrap();
    for file in std::fs::read_dir(dir.join("logs")).unwrap() {
        let text = std::fs::read_to_string(file.unwrap().path()).unwrap();
        assert!(!text.contains("s3cret"), "{text}");
    }

    // Forget, then Remember through the hidden box.
    let mut app = App::new(
        config::load(&paths::Paths::at(dir.clone())).unwrap().0,
        paths::Paths::at(dir.clone()),
        app.saved.clone(),
        Vec::new(),
        true,
    );
    let (port2, _received) = login_device();
    app.saved.connection.tcp_port = port2.to_string();
    app.request_connect();
    pump(&mut app, 300);
    let m = app.monitor.as_mut().unwrap();
    assert_eq!(m.password_stored(), Some(false));
    m.run_cmd(menu::Cmd::RememberPassword);
    typ(&mut app, "n3w-pw");
    let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    assert!(screen_text(&t).contains("REMEMBER PASSWORD"));
    assert!(!screen_text(&t).contains("n3w-pw"));
    key(&mut app, KeyCode::Enter);
    pump(&mut app, 200);
    let account2 = format!("tcp:127.0.0.1:{port2}");
    assert_eq!(secrets::read(&account2).unwrap().as_deref(), Some("n3w-pw"));
    assert_eq!(app.monitor.as_ref().unwrap().password_stored(), Some(true));
    app.monitor
        .as_mut()
        .unwrap()
        .run_cmd(menu::Cmd::ForgetPassword);
    pump(&mut app, 200);
    assert_eq!(secrets::read(&account2).unwrap(), None);
    app.shutdown().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// In line mode at a password prompt the send box hides what's typed, the
/// send is logged as <hidden> (not even its length) and kept out of
/// history. The header shows ● REC while logging, and the log can be named.
#[test]
fn a_typed_password_is_never_recorded_and_the_log_can_be_named() {
    secrets::use_test_store();
    let (port, received) = login_device();
    let dir = std::env::temp_dir().join(format!("rawtap_typedpw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = paths::Paths::at(dir.clone());
    let (cfg, _) = config::load(&paths).unwrap();
    let saved = config::Saved {
        connection: config::Connection {
            mode: ConnMode::Tcp,
            tcp_host: "127.0.0.1".into(),
            tcp_port: port.to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut app = App::new(cfg, paths, saved, Vec::new(), false);
    app.request_connect();
    pump(&mut app, 400);
    let mut t = Terminal::new(TestBackend::new(120, 30)).unwrap();
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    assert!(screen_text(&t).contains("● REC"), "no recording badge");

    typ(&mut app, "admin");
    key(&mut app, KeyCode::Enter);
    pump(&mut app, 300);
    typ(&mut app, "hunter22");
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "typing at a password prompt");
    let screen = screen_text(&t);
    assert!(screen.contains("PASSWORD · not recorded"), "{screen}");
    assert!(screen.contains("••••••••") && !screen.contains("hunter22"));
    key(&mut app, KeyCode::Enter);
    pump(&mut app, 300);
    let got = received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(String::from_utf8_lossy(&got).contains("hunter22\r"));
    let m = app.monitor.as_ref().unwrap();
    assert!(
        m.entries
            .iter()
            .any(|e| matches!(e.kind, monitor::EntryKind::TxHidden))
    );
    assert!(!m.history.iter().any(|h| h.text.contains("hunter22")));

    // F1 → Name this session log renames the log and its transcript.
    let old = m.logger.as_ref().unwrap().path.clone();
    app.monitor
        .as_mut()
        .unwrap()
        .run_cmd(menu::Cmd::NameSessionLog);
    typ(&mut app, "core switch");
    key(&mut app, KeyCode::Enter);
    let m = app.monitor.as_mut().unwrap();
    let new = m.logger.as_ref().unwrap().path.clone();
    let stem = old.file_stem().unwrap().to_string_lossy().to_string();
    assert_eq!(
        new.file_name().unwrap().to_string_lossy(),
        format!("{stem}_core-switch.log")
    );
    assert!(new.exists() && !old.exists());
    assert!(new.with_extension("txt").exists());
    m.run_cmd(menu::Cmd::CopySessionLogPath);
    assert_eq!(clip::get().unwrap(), new.display().to_string());

    app.shutdown().unwrap();
    for file in std::fs::read_dir(dir.join("logs")).unwrap() {
        let text = std::fs::read_to_string(file.unwrap().path()).unwrap();
        assert!(!text.contains("hunter22"), "{text}");
        assert!(!text.contains("8 bytes"), "the length leaked: {text}");
    }
    let state = std::fs::read_to_string(dir.join("state.json")).unwrap();
    assert!(!state.contains("hunter22"), "password in history");
    let _ = std::fs::remove_dir_all(&dir);
}
