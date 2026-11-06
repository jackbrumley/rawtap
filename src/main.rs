mod app;
mod clip;
mod codec;
mod config;
mod extract;
mod link;
mod logger;
mod macros;
mod menu;
mod monitor;
mod term;
mod ui;
mod ui_monitor;
#[cfg(test)]
mod switch_sim;

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
}

#[derive(Subcommand)]
enum Cmd {
    /// Open a serial (COM) port
    Serial {
        /// COM port, e.g. COM3
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
    /// List serial ports and exit
    Ports,
}

pub fn set_title(t: &str) {
    let _ = ratatui::crossterm::execute!(std::io::stdout(), ratatui::crossterm::terminal::SetTitle(t));
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
        None | Some(Cmd::Ports) => false,
        Some(Cmd::Serial { port, baud, data_bits, parity, stop_bits, flow }) => {
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
                s.serial_port = p.to_uppercase();
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

fn main() {
    let cli = Cli::parse();

    if let Some(Cmd::Ports) = cli.cmd {
        let ports = app::scan_ports();
        if ports.is_empty() {
            println!("No serial ports found.");
        }
        for p in ports {
            println!("{:<8} {}", p.name, p.desc);
        }
        return;
    }

    let paths = config::Paths::discover();
    let (cfg, notes) = config::load(&paths);
    let mut saved = config::load_saved(&paths);
    let direct = apply_cli(&cli.cmd, &mut saved);

    let mut app = App::new(cfg, paths, saved, notes, cli.no_log);
    if direct {
        app.request_connect();
    }

    let mut terminal = ratatui::init();
    // RawTap handles the mouse itself (wheel scrolling, selection) so the console doesn't turn
    // the wheel into arrow keys for the device. Shift+drag still uses the console's selection.
    let _ = ratatui::crossterm::execute!(std::io::stdout(), ratatui::crossterm::event::EnableMouseCapture);
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = ratatui::crossterm::execute!(std::io::stdout(), ratatui::crossterm::event::DisableMouseCapture);
        prev_hook(info);
    }));
    set_title("RawTap");
    let result = run(&mut terminal, &mut app);
    app.shutdown();
    let _ = ratatui::crossterm::execute!(std::io::stdout(), ratatui::crossterm::event::DisableMouseCapture);
    ratatui::restore();
    set_title("");

    if let Err(e) = result {
        eprintln!("RawTap error: {e}");
        eprintln!("Press Enter to close.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

#[cfg(test)]
mod screen_tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
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
            let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
            println!("{}", row.trim_end());
        }
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
                if n == 0 { break; }
                s.write_all(&buf[..n]).unwrap();
            }
        });

        let dir = std::env::temp_dir().join(format!("rawtap_ui_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = config::Paths { base: dir.clone(), config_file: dir.join(config::CONFIG_NAME), state_file: dir.join(config::STATE_NAME) };
        let (cfg, notes) = config::load(&paths);
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
        app.shutdown();
        let logs = std::fs::read_dir(dir.join("logs")).unwrap().next().unwrap().unwrap().path();
        println!("===== log file\n{}", std::fs::read_to_string(logs).unwrap());
        println!("===== state\n{}", std::fs::read_to_string(dir.join(config::STATE_NAME)).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
