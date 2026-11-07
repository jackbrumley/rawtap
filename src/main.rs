mod app;
mod clip;
mod codec;
mod config;
mod extract;
mod json_store;
mod link;
mod logger;
mod macros;
mod menu;
mod monitor;
mod opener;
mod paths;
mod session_log;
#[cfg(test)]
mod switch_sim;
mod system_info;
mod term;
mod ui;
mod ui_monitor;
mod update;

use app::App;
use clap::{Parser, Subcommand};
use config::ConnMode;
use ratatui::crossterm::event;
use std::time::Duration;

/// RawTap: watch and talk to serial ports and TCP connections in hex and ASCII.
/// Run with no arguments (or double-click) for the start screen.
#[derive(Parser)]
#[command(name = "rawtap", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// Don't write a log file for this session
    #[arg(long, global = true)]
    no_log: bool,
    /// Print the details a bug report needs (version, OS, terminal) and exit
    #[arg(long)]
    system_info: bool,
    /// Set by the updater when an update failed: say so on the start screen
    #[arg(long, hide = true)]
    update_failed: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open a serial port
    Serial {
        /// Serial port, e.g. COM3 (Windows) or /dev/ttyUSB0 (Linux)
        #[arg(short, long)]
        port: Option<String>,
        /// Baud rate (default: last used, else 9600)
        #[arg(short, long)]
        baud: Option<u32>,
        /// 5, 6, 7 or 8
        #[arg(long)]
        data_bits: Option<String>,
        /// none, odd or even
        #[arg(long)]
        parity: Option<String>,
        /// 1 or 2
        #[arg(long)]
        stop_bits: Option<String>,
        /// none, software or hardware
        #[arg(long)]
        flow: Option<String>,
    },
    /// Connect out to a TCP host
    Tcp {
        /// Host name or IP
        #[arg(short = 'H', long)]
        host: Option<String>,
        /// TCP port
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Listen for a TCP client to connect in
    Listen {
        /// TCP port
        #[arg(short, long)]
        port: Option<u16>,
        /// Address to listen on
        #[arg(long)]
        bind: Option<String>,
    },
    /// Telnet to a switch, router or server
    Telnet {
        /// Host name or IP
        #[arg(short = 'H', long)]
        host: Option<String>,
        /// Telnet port (default: last used, else 23)
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// List serial ports and exit
    Ports,
    /// Update RawTap to the latest release (re-runs the installer)
    Update,
}

pub fn set_title(t: &str) {
    let _ =
        ratatui::crossterm::execute!(std::io::stdout(), ratatui::crossterm::terminal::SetTitle(t));
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// Fold command-line options into the saved settings. Returns true if there's enough to connect.
fn apply_cli(cmd: &Option<Cmd>, s: &mut config::Saved) -> bool {
    match cmd {
        None | Some(Cmd::Ports) | Some(Cmd::Update) => false,
        Some(Cmd::Serial {
            port,
            baud,
            data_bits,
            parity,
            stop_bits,
            flow,
        }) => {
            s.mode = ConnMode::Serial;
            if let Some(b) = baud {
                s.baud = b.to_string();
            }
            if let Some(v) = data_bits {
                s.data_bits = v.clone();
            }
            if let Some(v) = parity {
                s.parity = capitalise(v);
            }
            if let Some(v) = stop_bits {
                s.stop_bits = v.clone();
            }
            if let Some(v) = flow {
                s.flow = capitalise(v);
            }
            if let Some(p) = port {
                // Windows port names are case-insensitive and shown as COM3;
                // Linux device paths are case-sensitive.
                s.serial_port = if cfg!(windows) {
                    p.to_uppercase()
                } else {
                    p.clone()
                };
                true
            } else {
                false
            }
        }
        Some(Cmd::Tcp { host, port }) => {
            s.mode = ConnMode::Tcp;
            if let Some(h) = host {
                s.tcp_host = h.clone();
            }
            if let Some(p) = port {
                s.tcp_port = p.to_string();
            }
            host.is_some() && port.is_some()
        }
        Some(Cmd::Telnet { host, port }) => {
            s.mode = ConnMode::Telnet;
            if let Some(h) = host {
                s.telnet_host = h.clone();
            }
            if let Some(p) = port {
                s.telnet_port = p.to_string();
            }
            host.is_some()
        }
        Some(Cmd::Listen { port, bind }) => {
            s.mode = ConnMode::Listen;
            if let Some(b) = bind {
                s.listen_bind = b.clone();
            }
            if let Some(p) = port {
                s.listen_port = p.to_string();
            }
            port.is_some()
        }
    }
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    while !app.quit {
        terminal.draw(|f| ui::draw(f, app))?;
        if event::poll(Duration::from_millis(30))? {
            let mut batch = vec![event::read()?];
            while batch.len() < 100_000 && event::poll(Duration::ZERO)? {
                batch.push(event::read()?);
            }
            app.on_events(batch);
        }
        app.tick();
    }
    Ok(())
}

/// Puts the terminal back to normal: mouse capture off, raw mode and the
/// alternate screen left. Safe to call when the TUI never started.
pub fn restore_terminal() {
    // Best effort on the way out; there's nowhere left to report a failure.
    let _ = ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::DisableMouseCapture
    );
    ratatui::restore();
}

/// Before the TUI starts: print why RawTap can't run and wait, so a console
/// opened by double-clicking doesn't vanish before it can be read.
fn fail_before_tui(message: &str) -> ! {
    log::error!("{message}");
    eprintln!("RawTap could not start: {message}");
    wait_for_enter();
    std::process::exit(1);
}

fn wait_for_enter() {
    eprintln!("Press Enter to close.");
    // Only a pause before exiting; a closed stdin just ends it sooner.
    let _ = std::io::stdin().read_line(&mut String::new());
}

/// Everything before the TUI: the one-time move from 0.6.0's locations, the
/// session log, then state and settings. `Err` stops RawTap without touching
/// a file it couldn't read.
fn start(cli: &Cli) -> Result<(App, bool), String> {
    let paths = paths::Paths::discover()?;
    let migrations = paths::migrate_legacy_files(&paths);
    session_log::init(&paths.debug_dir(), session_log::DEFAULT_LEVEL)?;
    log::info!("Config folder: {}", paths.base.display());

    let mut notes = Vec::new();
    for migration in &migrations {
        match migration {
            paths::Migration::Moved { .. } => {
                log::info!("{}", migration.describe());
                notes.push(migration.describe());
            }
            // Only logged: it would repeat on every start.
            paths::Migration::KeptNew { .. } => log::warn!("{}", migration.describe()),
            paths::Migration::Failed { .. } => {
                log::error!("{}", migration.describe());
                notes.push(migration.describe());
            }
        }
    }

    let (mut saved, state_note) = config::load_saved(&paths)?;
    notes.extend(state_note);
    match session_log::parse_level(&saved.log_level) {
        Some(level) => session_log::set_level(level),
        None => {
            let note = format!(
                "Unknown log level '{}' in state.json; using {}.",
                saved.log_level,
                session_log::level_name(session_log::DEFAULT_LEVEL)
            );
            log::warn!("{note}");
            notes.push(note);
        }
    }
    let (cfg, config_notes) = config::load(&paths)?;
    notes.extend(config_notes);
    if cli.update_failed {
        let message = update::update_failed_message();
        log::warn!("{message}");
        notes.push(message);
    }
    update::set_last_checked(saved.last_update_check.clone());
    let direct = apply_cli(&cli.cmd, &mut saved);
    Ok((App::new(cfg, paths, saved, notes, cli.no_log), direct))
}

/// `rawtap update`: check, then update if there's something newer.
fn update_command() {
    match update::check() {
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
        Ok(check) if !check.update_available => {
            println!("RawTap {} is the latest version.", check.current_version);
        }
        Ok(check) => {
            println!(
                "Updating RawTap {} → {}…",
                check.current_version, check.latest_version
            );
            run_update();
        }
    }
}

/// Runs the updater once the terminal is back to normal. On Linux it
/// replaces this process; on Windows it opens its own window.
fn run_update() {
    match update::run_update() {
        Ok(()) => {
            println!("The update continues in a new window; RawTap starts again when it's done.")
        }
        Err(error) => {
            eprintln!("{error}");
            wait_for_enter();
            std::process::exit(1);
        }
    }
}

fn main() {
    session_log::install_panic_hook();
    let cli = Cli::parse();

    if cli.system_info {
        println!("{}", system_info::SystemInfo::collect().summary());
        return;
    }

    if let Some(Cmd::Update) = cli.cmd {
        update_command();
        return;
    }

    if let Some(Cmd::Ports) = cli.cmd {
        let ports = app::setup::scan_ports();
        if ports.is_empty() {
            println!("No serial ports found.");
        }
        for p in ports {
            println!("{:<8} {}", p.name, p.desc);
        }
        return;
    }

    let (mut app, direct) = start(&cli).unwrap_or_else(|error| fail_before_tui(&error));
    if direct {
        app.request_connect();
    }
    // Silent: a failure is only logged (Release Standard section 7).
    app.start_update_check(false);

    let mut terminal = ratatui::init();
    // RawTap handles the mouse itself (wheel scrolling, selection) so the console doesn't turn
    // the wheel into arrow keys for the device. Shift+drag still uses the console's selection.
    if let Err(error) = ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::EnableMouseCapture
    ) {
        log::warn!("Mouse capture unavailable: {error}");
    }
    set_title("RawTap");
    let result = run(&mut terminal, &mut app);
    let saved = app.shutdown();
    restore_terminal();
    set_title("");

    let mut failed = false;
    if let Err(error) = saved {
        eprintln!("RawTap could not save its state: {error}");
        failed = true;
    }
    if let Err(error) = result {
        log::error!("RawTap error: {error}");
        eprintln!("RawTap error: {error}");
        failed = true;
    }
    log::info!("SESSION END");
    if failed {
        wait_for_enter();
    } else if app.update_requested {
        if let Some(update) = update::available() {
            println!(
                "Updating RawTap {} → {}.",
                update.current_version, update.latest_version
            );
        }
        // Voquill's wording (Release Standard section 7).
        println!("Starting update... RawTap will restart shortly.");
        run_update();
    }
}

#[cfg(test)]
mod screen_tests {
    use super::*;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};
    use std::io::{Read, Write};

    fn key(app: &mut App, code: KeyCode) {
        app.on_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    fn typ(app: &mut App, s: &str) {
        for c in s.chars() {
            key(app, KeyCode::Char(c));
        }
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
            mode: ConnMode::Tcp,
            ..Default::default()
        };
        let mut app = App::new(cfg, paths, saved, Vec::new(), true);
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
        let box_bottom = row_of("Esc quit");
        let dialog_top = row_of("Update Available");
        assert!(
            dialog_top < box_bottom,
            "dialog (row {dialog_top}) should sit over the start screen box (bottom row {box_bottom})"
        );
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
            mode: ConnMode::Tcp,
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
        dump(&t, "setup (serial)");

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
}
