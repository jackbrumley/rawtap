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
#[cfg(test)]
mod screen_tests;
mod secrets;
mod session_log;
mod ssh_key;
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
    /// SSH to a host with your system's ssh
    Ssh {
        /// [user@]host, or a Host alias from ~/.ssh/config
        destination: Option<String>,
        /// SSH port (default: last used, else 22)
        #[arg(short, long)]
        port: Option<u16>,
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
    /// Open a favourite from the start screen, by name
    Open {
        /// The favourite's name (any case), e.g. "Core switch"
        name: String,
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
        Some(Cmd::Open { name }) => {
            if let Err(error) = app::favourites::open_by_name(s, name) {
                // Typed in a terminal, so no "Press Enter": say why and stop.
                log::warn!("{error}");
                eprintln!("{error}");
                std::process::exit(2);
            }
            true
        }
        Some(Cmd::Serial {
            port,
            baud,
            data_bits,
            parity,
            stop_bits,
            flow,
        }) => {
            s.connection.mode = ConnMode::Serial;
            if let Some(b) = baud {
                s.connection.baud = b.to_string();
            }
            if let Some(v) = data_bits {
                s.connection.data_bits = v.clone();
            }
            if let Some(v) = parity {
                s.connection.parity = capitalise(v);
            }
            if let Some(v) = stop_bits {
                s.connection.stop_bits = v.clone();
            }
            if let Some(v) = flow {
                s.connection.flow = capitalise(v);
            }
            if let Some(p) = port {
                // Windows port names are case-insensitive and shown as COM3;
                // Linux device paths are case-sensitive.
                s.connection.serial_port = if cfg!(windows) {
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
            s.connection.mode = ConnMode::Tcp;
            if let Some(h) = host {
                s.connection.tcp_host = h.clone();
            }
            if let Some(p) = port {
                s.connection.tcp_port = p.to_string();
            }
            host.is_some() && port.is_some()
        }
        Some(Cmd::Ssh { destination, port }) => {
            s.connection.mode = ConnMode::Ssh;
            if let Some(d) = destination {
                match d.rsplit_once('@') {
                    Some((user, host)) => {
                        s.connection.ssh_user = user.to_string();
                        s.connection.ssh_host = host.to_string();
                    }
                    None => {
                        s.connection.ssh_user.clear();
                        s.connection.ssh_host = d.clone();
                    }
                }
            }
            if let Some(p) = port {
                s.connection.ssh_port = p.to_string();
            }
            destination.is_some()
        }
        Some(Cmd::Telnet { host, port }) => {
            s.connection.mode = ConnMode::Telnet;
            if let Some(h) = host {
                s.connection.telnet_host = h.clone();
            }
            if let Some(p) = port {
                s.connection.telnet_port = p.to_string();
            }
            host.is_some()
        }
        Some(Cmd::Listen { port, bind }) => {
            s.connection.mode = ConnMode::Listen;
            if let Some(b) = bind {
                s.connection.listen_bind = b.clone();
            }
            if let Some(p) = port {
                s.connection.listen_port = p.to_string();
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
    secrets::init();

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
