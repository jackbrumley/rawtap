//! End-to-end test: drive RawTap against a fake Cisco-style switch console over TCP.
//! `cargo test drive_a_switch -- --nocapture` prints the rendered screens and the log files.

use crate::app::App;
use crate::config::{self, ConnMode};
use crate::ui;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

#[derive(PartialEq)]
enum St {
    User,
    Pass,
    Cli,
    More(usize),
}

/// A tiny Cisco-like console: echoes per character, Username/Password, --More-- paging,
/// Tab completion and '?' help. CR and LF are each an Enter, like the real thing.
fn fake_switch(mut s: std::net::TcpStream) {
    let config: Vec<String> = (1..=24)
        .map(|i| format!("interface GigabitEthernet1/0/{i}\r\n description port {i}"))
        .collect();
    fn send(s: &mut std::net::TcpStream, b: &[u8]) {
        let _ = s.write_all(b);
    }
    send(&mut s, b"\r\nUser Access Verification\r\n\r\nUsername: ");
    let mut st = St::User;
    let mut line = String::new();
    let mut paging = true;
    let mut buf = [0u8; 64];
    loop {
        let n = match s.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        for &b in &buf[..n] {
            if let St::More(i) = st {
                if b == b' ' {
                    send(&mut s, &[0x08; 10]);
                    send(&mut s, b"          ");
                    send(&mut s, &[0x08; 10]);
                    let end = (i + 8).min(config.len());
                    for l in &config[i..end] {
                        send(&mut s, format!("{l}\r\n").as_bytes());
                    }
                    if end < config.len() {
                        send(&mut s, b" --More-- ");
                        st = St::More(end);
                    } else {
                        send(&mut s, b"end\r\nSwitch#");
                        st = St::Cli;
                    }
                }
                continue;
            }
            match b {
                b'\r' | b'\n' => {
                    let cmd = std::mem::take(&mut line);
                    match st {
                        St::User => {
                            send(&mut s, b"\r\nPassword: ");
                            st = St::Pass;
                        }
                        St::Pass => {
                            if cmd == "secret" {
                                send(&mut s, b"\r\n\r\nSwitch#");
                                st = St::Cli;
                            } else {
                                send(&mut s, b"\r\n% Authentication failed\r\n\r\nUsername: ");
                                st = St::User;
                            }
                        }
                        St::Cli => {
                            send(&mut s, b"\r\n");
                            match cmd.trim() {
                                "" => {}
                                "terminal length 0" => paging = false,
                                "enable" => {
                                    send(&mut s, b"Password: ");
                                    st = St::Pass;
                                    continue;
                                }
                                "show running-config" | "show run" => {
                                    send(&mut s, b"Building configuration...\r\n\r\nhostname Switch\r\n");
                                    let end = if paging { 8 } else { config.len() };
                                    for l in &config[..end] {
                                        send(&mut s, format!("{l}\r\n").as_bytes());
                                    }
                                    if paging {
                                        send(&mut s, b" --More-- ");
                                        st = St::More(8);
                                        continue;
                                    }
                                    send(&mut s, b"end\r\n");
                                }
                                other => send(&mut s, format!("% Invalid input: {other}\r\n").as_bytes()),
                            }
                            send(&mut s, b"Switch#");
                        }
                        St::More(_) => {}
                    }
                }
                b'\t' if st == St::Cli => {
                    if line == "sh" {
                        line.push_str("ow ");
                        send(&mut s, b"ow ");
                    }
                }
                b'?' if st == St::Cli => {
                    let help = format!(
                        "?\r\n  show      Show running system information\r\n  terminal  Set terminal line parameters\r\n\r\nSwitch#{line}"
                    );
                    send(&mut s, help.as_bytes());
                }
                0x7F | 0x08 => {
                    if line.pop().is_some() {
                        send(&mut s, b"\x08 \x08");
                    }
                }
                0x20..=0x7E => {
                    line.push(b as char);
                    if st != St::Pass {
                        send(&mut s, &[b]);
                    }
                }
                _ => {}
            }
        }
    }
}

fn mouse(app: &mut App, kind: MouseEventKind, x: u16, y: u16) {
    app.on_event(Event::Mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE }));
}

fn key(app: &mut App, code: KeyCode, m: KeyModifiers) {
    app.on_event(Event::Key(KeyEvent::new(code, m)));
}

fn pump(app: &mut App, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    loop {
        app.tick();
        if Instant::now() >= end {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Type at human-ish speed so echoes interleave with keystrokes, as on a real console.
fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c), KeyModifiers::NONE);
        pump(app, 15);
    }
}

fn render(app: &mut App, w: u16, h: u16) {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, app)).unwrap();
}

fn dump(app: &mut App, w: u16, h: u16, title: &str) {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::draw(f, app)).unwrap();
    let buf = t.backend().buffer();
    println!("===== {title} ({w}x{h})");
    for y in 0..buf.area.height {
        let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
        println!("{}", row.trim_end());
    }
}

#[test]
fn drive_a_switch() {
    let srv = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = srv.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (s, _) = srv.accept().unwrap();
        fake_switch(s);
    });

    let dir = std::env::temp_dir().join(format!("rawtap_sw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = config::Paths::at(dir.clone());
    let (cfg, notes) = config::load(&paths);
    let saved = config::Saved {
        mode: ConnMode::Tcp,
        tcp_host: "127.0.0.1".into(),
        tcp_port: port.to_string(),
        ..Default::default()
    };
    let mut app = App::new(cfg, paths, saved, notes, false);
    let none = KeyModifiers::NONE;

    // Start screen: open Options (5th row for TCP), set char delay to 2 ms before connecting.
    for _ in 0..4 {
        key(&mut app, KeyCode::Down, none);
    }
    key(&mut app, KeyCode::Enter, none); // on Options: expands instead of connecting
    key(&mut app, KeyCode::Down, none);
    key(&mut app, KeyCode::Down, none); // Char delay
    key(&mut app, KeyCode::Right, none);
    dump(&mut app, 100, 40, "start screen with Options open, on Char delay");
    for _ in 0..3 {
        key(&mut app, KeyCode::Up, none);
    }
    dump(&mut app, 100, 40, "start screen, description follows the cursor (Logging)");
    key(&mut app, KeyCode::Enter, none);
    pump(&mut app, 200);
    assert_eq!(app.monitor.as_ref().unwrap().profile.char_delay_ms, 2, "start-screen option not applied");
    // Back to full speed for the rest of the run.
    key(&mut app, KeyCode::Char('t'), KeyModifiers::CONTROL);
    for _ in 0..4 {
        key(&mut app, KeyCode::Char('t'), KeyModifiers::CONTROL);
    }
    assert_eq!(app.monitor.as_ref().unwrap().profile.char_delay_ms, 0);

    // Line mode login: username, then the password via F6 hidden send.
    typ(&mut app, "admin");
    key(&mut app, KeyCode::Enter, none);
    pump(&mut app, 100);
    key(&mut app, KeyCode::F(6), none);
    for c in "secret".chars() {
        key(&mut app, KeyCode::Char(c), none);
    }
    dump(&mut app, 160, 30, "hidden send being typed");
    key(&mut app, KeyCode::Enter, none);
    pump(&mut app, 150);

    // Live keys: Tab completion, '?', paging with space.
    key(&mut app, KeyCode::F(4), none);
    typ(&mut app, "sh");
    key(&mut app, KeyCode::Tab, none);
    pump(&mut app, 50);
    typ(&mut app, "?");
    pump(&mut app, 50);
    typ(&mut app, "running-config");
    key(&mut app, KeyCode::Enter, none);
    pump(&mut app, 150);
    dump(&mut app, 160, 34, "live: show run paused at --More--");
    typ(&mut app, " ");
    pump(&mut app, 50);
    typ(&mut app, " ");
    pump(&mut app, 150);
    dump(&mut app, 160, 34, "live: after paging");
    dump(&mut app, 100, 34, "same, narrow window (stacked)");

    // A Password: prompt while in live keys: what's typed there must not be logged.
    typ(&mut app, "enable");
    key(&mut app, KeyCode::Enter, none);
    pump(&mut app, 50);
    typ(&mut app, "secret");
    key(&mut app, KeyCode::Enter, none);
    pump(&mut app, 100);
    dump(&mut app, 100, 34, "after live-mode enable password");

    // ---- Mouse: wheel scrolls the terminal and sends nothing to the device.
    dump(&mut app, 160, 34, "before mouse");
    let tx_before = app.monitor.as_ref().unwrap().tx_bytes;
    let t = app.monitor.as_ref().unwrap().term_inner;
    for _ in 0..2 {
        mouse(&mut app, MouseEventKind::ScrollUp, t.x + 5, t.y + 5);
    }
    assert_eq!(app.monitor.as_ref().unwrap().term.screen().scrollback(), 6, "wheel didn't scroll the terminal");
    assert_eq!(app.monitor.as_ref().unwrap().tx_bytes, tx_before, "wheel sent something to the device");
    dump(&mut app, 160, 34, "terminal scrolled back 6 lines with the wheel");
    for _ in 0..2 {
        mouse(&mut app, MouseEventKind::ScrollDown, t.x + 5, t.y + 5);
    }
    // ---- Drag-select two terminal lines and copy with Ctrl+Shift+C.
    dump(&mut app, 160, 34, "back at the bottom");
    let screen = app.monitor.as_ref().unwrap().term.screen().contents();
    let rows: Vec<&str> = screen.lines().collect();
    let r = rows.iter().position(|l| l.starts_with("interface GigabitEthernet1/0/20")).expect("row") as u16;
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), t.x, t.y + r);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), t.x + 40, t.y + r + 1);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), t.x + 40, t.y + r + 1);
    dump(&mut app, 160, 34, "two lines selected in the terminal pane (highlight not visible in text dump)");
    {
        // The selection is highlighted, and only inside the terminal pane.
        let mut term = Terminal::new(TestBackend::new(160, 34)).unwrap();
        term.draw(|f| ui::draw(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        assert_eq!(buf[(t.x + 3, t.y + r)].bg, ratatui::style::Color::LightBlue, "selection not highlighted");
        assert_eq!(buf[(t.x + 3, t.y + r + 1)].bg, ratatui::style::Color::LightBlue);
        assert_ne!(buf[(t.x - 2, t.y + r)].bg, ratatui::style::Color::LightBlue, "highlight leaked outside the pane");
    }
    key(&mut app, KeyCode::Char('C'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    let copied = crate::clip::get().unwrap();
    println!("===== copied from terminal pane:\n{copied}\n=====");
    assert_eq!(copied, "interface GigabitEthernet1/0/20\n description port 20");
    assert_eq!(app.monitor.as_ref().unwrap().tx_bytes, tx_before, "copy sent something to the device");

    // ---- Select more than a screen: drag from the last config line up past the top edge.
    render(&mut app, 160, 34);
    let screen = app.monitor.as_ref().unwrap().term.screen().contents();
    let r24 = screen.lines().position(|l| l.starts_with("interface GigabitEthernet1/0/24")).expect("row") as u16;
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), t.x + 60, t.y + r24);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), t.x + 10, 0); // above the pane
    for _ in 0..40 {
        pump(&mut app, 30);
        render(&mut app, 160, 34); // the draw follows the mouse with the selection
    }
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), t.x + 10, 0);
    assert!(app.monitor.as_ref().unwrap().term.screen().scrollback() > 0, "dragging past the edge didn't auto-scroll");
    key(&mut app, KeyCode::Char('C'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    let long = crate::clip::get().unwrap();
    let n = long.lines().count();
    println!("===== drag-past-edge copy: {n} lines, first 3 / last 2:");
    for l in long.lines().take(3) { println!("{l}"); }
    for l in long.lines().rev().take(2).collect::<Vec<_>>().into_iter().rev() { println!("{l}"); }
    assert!(n > 27, "copied only {n} lines: not more than one screen");
    assert!(long.contains("hostname Switch"), "selection didn't reach the scrolled-off lines");
    assert!(long.ends_with("interface GigabitEthernet1/0/24"));
    key(&mut app, KeyCode::End, KeyModifiers::CONTROL); // back to live view

    // ---- Ctrl+click extends a selection across a scroll.
    render(&mut app, 160, 34);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), t.x + 60, t.y + r24);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), t.x, t.y + r24);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), t.x, t.y + r24);
    for _ in 0..4 {
        mouse(&mut app, MouseEventKind::ScrollUp, t.x + 5, t.y + 5);
    }
    render(&mut app, 160, 34);
    app.on_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left), column: t.x, row: t.y, modifiers: KeyModifiers::CONTROL,
    }));
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), t.x, t.y);
    key(&mut app, KeyCode::Char('C'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    let ext = crate::clip::get().unwrap();
    assert!(ext.lines().count() >= 12, "Ctrl+click didn't extend: {} lines", ext.lines().count());
    assert!(ext.ends_with("interface GigabitEthernet1/0/24"));
    key(&mut app, KeyCode::End, KeyModifiers::CONTROL);

    // ---- Raw log: drag over two compact rows, copy whole entries in full.
    render(&mut app, 160, 34);
    let li = app.monitor.as_ref().unwrap().log_inner;
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), li.x + 3, li.y + li.height - 4);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), li.x + 3, li.y + li.height - 3);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), li.x + 3, li.y + li.height - 3);
    {
        let mut term = Terminal::new(TestBackend::new(160, 34)).unwrap();
        term.draw(|f| ui::draw(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        // Whole rows are highlighted, right to the pane edge.
        assert_eq!(buf[(li.x + li.width - 1, li.y + li.height - 4)].bg, ratatui::style::Color::LightBlue);
    }
    key(&mut app, KeyCode::Char('C'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    let logcopy = crate::clip::get().unwrap();
    println!("===== copied from raw log (compact view on screen):");
    println!("{logcopy}=====");
    assert!(logcopy.contains(" HEX:  ") && logcopy.contains(" TEXT: "), "raw log copy isn't log-file format");
    assert!(!logcopy.contains('…'), "raw log copy is truncated");

    // ---- Paste: Windows Terminal types a paste in as a burst of keys. Multi-line asks first.
    key(&mut app, KeyCode::F(4), none); // line mode
    let burst: Vec<Event> = "show clock\nshow version\n"
        .chars()
        .map(|c| {
            let code = if c == '\n' { KeyCode::Enter } else { KeyCode::Char(c) };
            Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
        })
        .collect();
    app.on_events(burst);
    pump(&mut app, 100);
    assert!(app.monitor.as_ref().unwrap().paste_confirm.is_some(), "multi-line paste didn't ask");
    assert_eq!(app.monitor.as_ref().unwrap().tx_bytes, tx_before, "paste sent before confirming");
    dump(&mut app, 120, 30, "multi-line paste confirmation (background dimmed)");
    {
        // Everything behind the pop-up is greyed out.
        let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
        term.draw(|f| ui::draw(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        assert_eq!(buf[(1, 0)].fg, ratatui::style::Color::DarkGray, "header not dimmed behind the pop-up");
        assert_eq!(buf[(1, 0)].bg, ratatui::style::Color::Reset);
    }
    key(&mut app, KeyCode::Enter, none);
    pump(&mut app, 150);
    // Single line via Ctrl+Shift+V goes into the send box, not straight out.
    crate::clip::set("show interfaces status").unwrap();
    key(&mut app, KeyCode::Char('V'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    assert_eq!(app.monitor.as_ref().unwrap().input_text(), "show interfaces status");
    key(&mut app, KeyCode::Esc, none);
    // Ctrl+C alone no longer quits in line mode.
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(!app.quit, "Ctrl+C quit the app");

    // The capture macro runs show run with paging off.
    key(&mut app, KeyCode::Char('9'), KeyModifiers::ALT);
    pump(&mut app, 2600);
    key(&mut app, KeyCode::F(7), none); // break on TCP: should refuse politely
    pump(&mut app, 50);
    dump(&mut app, 100, 34, "after capture macro + break attempt");

    // The F1 menu: browse, then search for "delay" and change it with →.
    key(&mut app, KeyCode::F(1), none);
    key(&mut app, KeyCode::Down, none);
    key(&mut app, KeyCode::Down, none);
    dump(&mut app, 120, 44, "F1 menu, third item selected");
    for c in "delay".chars() {
        key(&mut app, KeyCode::Char(c), none);
    }
    key(&mut app, KeyCode::Right, none);
    key(&mut app, KeyCode::Right, none);
    dump(&mut app, 120, 44, "F1 menu filtered to 'delay', changed to 5 ms");
    assert_eq!(app.monitor.as_ref().unwrap().profile.char_delay_ms, 5);
    key(&mut app, KeyCode::Esc, none); // clears the search
    // On/off switches change with ←→ too, and the menu stays open.
    for (word, check) in [
        ("live keys", (|m: &crate::monitor::Monitor| m.live) as fn(&crate::monitor::Monitor) -> bool),
        ("freezes", |m| !m.follow),
        ("scroll pane", |m| m.focus == crate::monitor::Pane::Log),
    ] {
        for c in word.chars() {
            key(&mut app, KeyCode::Char(c), none);
        }
        key(&mut app, KeyCode::Right, none);
        let m = app.monitor.as_ref().unwrap();
        assert!(check(m), "→ didn't flip '{word}'");
        assert!(m.menu.is_some(), "menu closed after changing '{word}'");
        key(&mut app, KeyCode::Left, none);
        assert!(!check(app.monitor.as_ref().unwrap()), "← didn't flip '{word}' back");
        key(&mut app, KeyCode::Esc, none);
    }
    key(&mut app, KeyCode::Esc, none); // closes
    assert!(app.monitor.as_ref().unwrap().menu.is_none());
    dump(&mut app, 120, 30, "menu closed: calm footer + status");

    // Awkward window sizes must not panic, with and without the menu open.
    for menu_open in [false, true] {
        if menu_open {
            key(&mut app, KeyCode::F(1), none);
        }
        for (w, h) in [(20, 8), (40, 10), (60, 12), (129, 20), (130, 20), (200, 60)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| ui::draw(f, &mut app)).unwrap();
        }
    }
    dump(&mut app, 90, 24, "menu in a small window (list scrolls, description stays)");
    for c in "shortcut".chars() {
        key(&mut app, KeyCode::Char(c), none);
    }
    key(&mut app, KeyCode::Enter, none);
    dump(&mut app, 100, 42, "keyboard shortcuts (from the menu)");
    key(&mut app, KeyCode::Esc, none);
    for _ in 0..3 {
        key(&mut app, KeyCode::F(3), none);
        for (w, h) in [(20, 8), (60, 12), (160, 40)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| ui::draw(f, &mut app)).unwrap();
        }
    }

    // Ctrl+Shift+Q quits even from live keys.
    key(&mut app, KeyCode::F(4), none);
    assert!(app.monitor.as_ref().unwrap().live);
    key(&mut app, KeyCode::Char('Q'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    assert!(app.quit, "Ctrl+Shift+Q didn't quit from live keys");

    app.shutdown();
    let logs = dir.join("logs");
    let mut files: Vec<_> = std::fs::read_dir(&logs).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    let mut all_text = String::new();
    for p in &files {
        let t = std::fs::read_to_string(p).unwrap();
        let lines: Vec<&str> = t.lines().collect();
        println!("===== {} ({} lines), first 45:", p.file_name().unwrap().to_string_lossy(), lines.len());
        for l in lines.iter().take(45) {
            println!("{l}");
        }
        all_text.push_str(&t);
    }
    let cap = std::fs::read_dir(dir.join("captures")).unwrap().next().unwrap().unwrap().path();
    let cap_text = std::fs::read_to_string(&cap).unwrap();
    println!("===== capture {} (head / tail)", cap.file_name().unwrap().to_string_lossy());
    let cl: Vec<&str> = cap_text.lines().collect();
    for l in cl.iter().take(6) {
        println!("{l}");
    }
    println!("...");
    for l in cl.iter().rev().take(3).rev() {
        println!("{l}");
    }
    let state = std::fs::read_to_string(dir.join(config::STATE_NAME)).unwrap();

    assert!(!all_text.contains("secret"), "password leaked into a log/transcript");
    assert!(!state.contains("secret"), "password leaked into state");
    assert!(all_text.contains("<hidden: 7 bytes>"), "F6 send not logged as hidden");
    assert!(all_text.matches("<hidden:").count() >= 2, "live-mode password not hidden");
    assert!(cap_text.contains("description port 24") && !cap_text.contains("--More--"));
    assert!(state.contains("\"tcp:127.0.0.1:"));
    assert!(all_text.contains("TX TEXT: show clock<CR>") && all_text.contains("TX TEXT: show version<CR>"), "pasted lines not sent");
    let _ = std::fs::remove_dir_all(&dir);
}
