//! The start screen's list and connection panel: a new connection saved
//! as a favourite, a second one, editing, discarding and removing.

use super::*;

fn ctrl(app: &mut App, c: char) {
    app.on_event(Event::Key(KeyEvent::new(
        KeyCode::Char(c),
        KeyModifiers::CONTROL,
    )));
}

/// The list's highlighted row (the panel is closed).
fn list_focus(app: &mut App) -> app::setup::ListRow {
    assert!(app.setup.panel.is_none(), "the panel is open");
    app.focused_list_row()
}

#[test]
fn favourites_new_connection_save_edit_and_remove() {
    use app::setup::ListRow;
    let dir = std::env::temp_dir().join(format!("rawtap_fav_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = paths::Paths::at(dir.clone());
    let (cfg, _) = config::load(&paths).unwrap();
    let saved = config::Saved {
        connection: config::Connection {
            mode: ConnMode::Ssh,
            ssh_host: "10.0.0.5".into(),
            ssh_user: "admin".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut app = App::new(cfg, paths, saved, Vec::new(), true);
    let mut t = Terminal::new(TestBackend::new(100, 34)).unwrap();

    // The list starts on "New connection"; there are no favourites yet.
    assert_eq!(list_focus(&mut app), ListRow::New);
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "empty list");
    assert!(screen_text(&t).contains("New connection"));
    assert!(screen_text(&t).contains("None yet."));

    // New connection opens an empty panel: only the type (SSH) is the last
    // one used, so it's never a copy of a favourite. Ctrl+S with no name
    // suggests one; Ctrl+S again saves it.
    key(&mut app, KeyCode::Enter);
    let panel = app.setup.panel.as_ref().unwrap();
    assert_eq!(panel.connection.mode, ConnMode::Ssh);
    assert_eq!(panel.connection.ssh_host, "");
    assert_eq!(panel.name, "");
    fill_host_and_port(&mut app, "10.0.0.5", "admin"); // host, user
    ctrl(&mut app, 's');
    assert_eq!(
        app.setup.panel.as_ref().unwrap().name,
        "SSH admin@10.0.0.5:22"
    );
    for _ in 0.."SSH admin@10.0.0.5:22".len() {
        key(&mut app, KeyCode::Backspace);
    }
    typ(&mut app, "Core switch");
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "new connection panel");
    ctrl(&mut app, 's');
    assert!(app.setup.panel.is_none());
    assert_eq!(app.saved.favourites[0].name, "Core switch");
    assert_eq!(list_focus(&mut app), ListRow::Favourite(0));

    // A second favourite: moving through the list changes nothing, and the
    // same connection again says which favourite already has it.
    key(&mut app, KeyCode::Up);
    assert_eq!(list_focus(&mut app), ListRow::New);
    key(&mut app, KeyCode::Enter);
    typ(&mut app, "Printer");
    fill_host_and_port(&mut app, "10.0.0.5", "admin");
    ctrl(&mut app, 's');
    assert!(
        app.setup
            .error
            .as_ref()
            .unwrap()
            .contains("already saved as \"Core switch\"")
    );
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::Up); // back on Connection
    key(&mut app, KeyCode::Right); // SSH -> Serial
    key(&mut app, KeyCode::Right); // -> TCP client
    key(&mut app, KeyCode::Down);
    typ(&mut app, "192.168.1.50");
    key(&mut app, KeyCode::Down);
    typ(&mut app, "9100");
    ctrl(&mut app, 's');
    assert_eq!(app.saved.favourites.len(), 2);
    assert_eq!(app.saved.favourites[1].name, "Printer");
    assert_eq!(list_focus(&mut app), ListRow::Favourite(1));
    // Saving doesn't change the last connection used.
    assert_eq!(app.saved.connection.mode, ConnMode::Ssh);
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "two favourites");
    let text = screen_text(&t);
    assert!(text.contains("Core switch"));
    assert!(text.contains("SSH admin@10.0.0.5:22"));
    assert!(text.contains("TCP 192.168.1.50:9100"));

    // E edits a favourite's connection, not just its name; a clashing name
    // is refused.
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::Char('e'));
    for _ in 0.."Core switch".len() {
        key(&mut app, KeyCode::Backspace);
    }
    typ(&mut app, "PRINTER");
    ctrl(&mut app, 's');
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    assert!(screen_text(&t).contains("There's already a favourite called"));
    // ←→, Home and Delete edit inside the name: "PRINTER" -> "Core switch".
    key(&mut app, KeyCode::Home);
    for _ in 0.."PRINTER".len() {
        key(&mut app, KeyCode::Delete);
    }
    typ(&mut app, "Core swtch");
    for _ in 0.."tch".len() {
        key(&mut app, KeyCode::Left);
    }
    typ(&mut app, "i");
    assert_eq!(app.setup.panel.as_ref().unwrap().name, "Core switch");
    key(&mut app, KeyCode::Down); // Connection
    key(&mut app, KeyCode::Down); // Host
    key(&mut app, KeyCode::Backspace);
    typ(&mut app, "6");
    ctrl(&mut app, 's');
    assert_eq!(app.saved.favourites[0].connection.ssh_host, "10.0.0.6");
    assert_eq!(app.saved.favourites.len(), 2);

    // Esc with unsaved changes needs a second Esc.
    key(&mut app, KeyCode::Char('e'));
    typ(&mut app, "!");
    key(&mut app, KeyCode::Esc);
    assert!(app.setup.panel.is_some());
    key(&mut app, KeyCode::Esc);
    assert!(app.setup.panel.is_none());
    assert_eq!(app.saved.favourites[0].name, "Core switch");

    // Del asks first; Esc keeps it, Enter removes it.
    key(&mut app, KeyCode::Delete);
    t.draw(|f| ui::draw(f, &mut app)).unwrap();
    dump(&t, "remove?");
    assert!(screen_text(&t).contains("Remove \"Core switch\""));
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.saved.favourites.len(), 2);
    key(&mut app, KeyCode::Delete);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.saved.favourites.len(), 1);

    // Saved in state.json, and `rawtap open` finds it by name.
    let (mut reloaded, _) = config::load_saved(&paths::Paths::at(dir.clone())).unwrap();
    assert_eq!(reloaded.favourites.len(), 1);
    app::favourites::open_by_name(&mut reloaded, "printer").unwrap();
    assert_eq!(reloaded.connection.tcp_port, "9100");
    let _ = std::fs::remove_dir_all(&dir);
}
