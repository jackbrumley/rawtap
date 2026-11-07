//! Connections: serial port, TCP client, TCP server (one client at a time),
//! Telnet, SSH (the system's ssh in a pseudo-terminal). A background thread
//! reads and posts events; sends go through a shared writer.

mod pty;
mod telnet;

use serialport::{DataBits, FlowControl, Parity, StopBits};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const READ_TIMEOUT: Duration = Duration::from_millis(100);

pub enum LinkEvent {
    Data(Vec<u8>),
    /// The connection is gone for good (serial error, peer closed TCP client socket).
    Closed(String),
    /// The session ended normally (e.g. you logged out of SSH).
    Ended(String),
    PeerConnected(String),
    PeerDisconnected(String),
    /// A queued send failed (sends happen on a background thread).
    SendError(String),
    /// Something the raw log should say about the connection itself, e.g.
    /// what Telnet agreed with the device.
    Info(String),
}

#[derive(Clone, Debug)]
pub enum LinkSettings {
    Serial {
        port: String,
        baud: u32,
        data_bits: DataBits,
        parity: Parity,
        stop_bits: StopBits,
        flow: FlowControl,
    },
    TcpClient {
        host: String,
        port: u16,
    },
    TcpListen {
        bind: String,
        port: u16,
    },
    Telnet {
        host: String,
        port: u16,
    },
    Ssh {
        host: String,
        /// Empty: ssh's own default (your login name or ~/.ssh/config).
        user: String,
        port: u16,
    },
}

impl LinkSettings {
    pub fn kind(&self) -> &'static str {
        match self {
            LinkSettings::Serial { .. } => "serial",
            LinkSettings::TcpClient { .. } => "tcp",
            LinkSettings::TcpListen { .. } => "listen",
            LinkSettings::Telnet { .. } => "telnet",
            LinkSettings::Ssh { .. } => "ssh",
        }
    }

    pub fn describe(&self) -> String {
        match self {
            LinkSettings::Serial {
                port,
                baud,
                data_bits,
                parity,
                stop_bits,
                flow,
            } => {
                let d = match data_bits {
                    DataBits::Five => '5',
                    DataBits::Six => '6',
                    DataBits::Seven => '7',
                    DataBits::Eight => '8',
                };
                let p = match parity {
                    Parity::None => 'N',
                    Parity::Odd => 'O',
                    Parity::Even => 'E',
                };
                let s = match stop_bits {
                    StopBits::One => '1',
                    StopBits::Two => '2',
                };
                let f = match flow {
                    FlowControl::None => "",
                    FlowControl::Software => " XON/XOFF",
                    FlowControl::Hardware => " RTS/CTS",
                };
                format!("{port} @ {baud} {d}{p}{s}{f}")
            }
            LinkSettings::TcpClient { host, port } => format!("TCP {host}:{port}"),
            LinkSettings::TcpListen { bind, port } => format!("TCP server {bind}:{port}"),
            LinkSettings::Telnet { host, port } => format!("Telnet {host}:{port}"),
            LinkSettings::Ssh { host, user, port } => match user.as_str() {
                "" => format!("SSH {host}:{port}"),
                user => format!("SSH {user}@{host}:{port}"),
            },
        }
    }
}

enum Writer {
    Serial(Box<dyn serialport::SerialPort>),
    Tcp(TcpStream),
    /// A program's input in a pseudo-terminal (SSH).
    Pty(Box<dyn Write + Send>),
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Writer::Serial(p) => p.write(buf),
            Writer::Tcp(s) => s.write(buf),
            Writer::Pty(w) => w.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Writer::Serial(p) => p.flush(),
            Writer::Tcp(s) => s.flush(),
            Writer::Pty(w) => w.flush(),
        }
    }
}

type SharedWriter = Arc<Mutex<Option<Writer>>>;

pub struct Link {
    writer: SharedWriter,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    /// Sends are queued to a writer thread so a paced send never freezes the screen.
    out: Option<Sender<Vec<u8>>>,
    out_handle: Option<JoinHandle<()>>,
    /// Delay between characters, in ms (0 = send at full speed).
    pacing_ms: Arc<AtomicU64>,
    /// The Telnet session, on a Telnet connection.
    telnet: Option<Arc<Mutex<telnet::Telnet>>>,
    /// The pseudo-terminal, on an SSH connection.
    pty: Option<pty::PtySession>,
}

impl Link {
    pub fn open(settings: &LinkSettings, tx: Sender<LinkEvent>) -> Result<Link, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let writer: SharedWriter = Arc::new(Mutex::new(None));
        let err_tx = tx.clone();
        let mut telnet_session = None;
        let mut pty_session = None;
        let handle = match settings {
            LinkSettings::Serial {
                port,
                baud,
                data_bits,
                parity,
                stop_bits,
                flow,
            } => {
                let mut sp = serialport::new(port, *baud)
                    .data_bits(*data_bits)
                    .parity(*parity)
                    .stop_bits(*stop_bits)
                    .flow_control(*flow)
                    .timeout(READ_TIMEOUT)
                    .open()
                    .map_err(|e| serial_error(port, &e))?;
                // Match PuTTY/pyserial: assert DTR and RTS on open (many devices need DTR).
                let _ = sp.write_data_terminal_ready(true);
                if *flow != FlowControl::Hardware {
                    let _ = sp.write_request_to_send(true);
                }
                let mut reader = sp
                    .try_clone()
                    .map_err(|e| format!("Could not clone {port}: {e}"))?;
                *writer.lock().unwrap() = Some(Writer::Serial(sp));
                let stop2 = stop.clone();
                thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    while !stop2.load(Ordering::Relaxed) {
                        match reader.read(&mut buf) {
                            Ok(0) => {}
                            Ok(n) => {
                                if tx.send(LinkEvent::Data(buf[..n].to_vec())).is_err() {
                                    break;
                                }
                            }
                            Err(e) if is_transient(&e) => {}
                            Err(e) => {
                                let _ =
                                    tx.send(LinkEvent::Closed(format!("Serial port error: {e}")));
                                break;
                            }
                        }
                    }
                })
            }
            LinkSettings::TcpClient { host, port } => {
                let stream = connect_tcp(host, *port)?;
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                let mut reader = stream.try_clone().map_err(|e| e.to_string())?;
                *writer.lock().unwrap() = Some(Writer::Tcp(stream));
                let stop2 = stop.clone();
                thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    while !stop2.load(Ordering::Relaxed) {
                        match reader.read(&mut buf) {
                            Ok(0) => {
                                let _ =
                                    tx.send(LinkEvent::Closed("Connection closed by peer.".into()));
                                break;
                            }
                            Ok(n) => {
                                if tx.send(LinkEvent::Data(buf[..n].to_vec())).is_err() {
                                    break;
                                }
                            }
                            Err(e) if is_transient(&e) => {}
                            Err(e) => {
                                let _ = tx.send(LinkEvent::Closed(format!("Connection lost: {e}")));
                                break;
                            }
                        }
                    }
                })
            }
            LinkSettings::Telnet { host, port } => {
                let stream = connect_tcp(host, *port)?;
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                let reader = stream.try_clone().map_err(|e| e.to_string())?;
                *writer.lock().unwrap() = Some(Writer::Tcp(stream));
                let session = Arc::new(Mutex::new(telnet::Telnet::new()));
                telnet_session = Some(session.clone());
                let (stop2, writer2) = (stop.clone(), writer.clone());
                thread::spawn(move || telnet_read_loop(reader, session, writer2, stop2, tx))
            }
            LinkSettings::Ssh { host, user, port } => {
                let mut args = vec!["-p".to_string(), port.to_string()];
                if !user.is_empty() {
                    args.extend(["-l".to_string(), user.clone()]);
                }
                args.push(host.clone());
                let spawned = pty::spawn("ssh", &args, tx)?;
                *writer.lock().unwrap() = Some(Writer::Pty(spawned.input));
                pty_session = Some(spawned.session);
                spawned.waiter
            }
            LinkSettings::TcpListen { bind, port } => {
                let listener = TcpListener::bind((bind.as_str(), *port))
                    .map_err(|e| listen_error(bind, *port, &e))?;
                listener.set_nonblocking(true).map_err(|e| e.to_string())?;
                let stop2 = stop.clone();
                let writer2 = writer.clone();
                thread::spawn(move || listen_loop(listener, writer2, stop2, tx))
            }
        };
        let pacing_ms = Arc::new(AtomicU64::new(0));
        let (out, out_rx) = mpsc::channel::<Vec<u8>>();
        let (w2, stop3, pace2) = (writer.clone(), stop.clone(), pacing_ms.clone());
        let out_handle = thread::spawn(move || {
            while let Ok(data) = out_rx.recv() {
                if let Err(e) = write_paced(&w2, &data, &pace2, &stop3) {
                    let _ = err_tx.send(LinkEvent::SendError(e));
                }
            }
        });
        Ok(Link {
            writer,
            stop,
            handle: Some(handle),
            out: Some(out),
            out_handle: Some(out_handle),
            pacing_ms,
            telnet: telnet_session,
            pty: pty_session,
        })
    }

    /// Queue bytes to send. Fails straight away if there's nothing to send to.
    pub fn send(&self, data: &[u8]) -> Result<(), String> {
        if self.writer.lock().unwrap().is_none() {
            return Err("no client connected yet".into());
        }
        // On Telnet a 255 byte would start a command: double it.
        let data = match self.telnet {
            Some(_) => telnet::escape(data),
            None => data.to_vec(),
        };
        match &self.out {
            Some(out) => out
                .send(data)
                .map_err(|_| "connection is closing".to_string()),
            None => Err("connection is closing".into()),
        }
    }

    pub fn set_pacing(&self, ms: u64) {
        self.pacing_ms.store(ms, Ordering::Relaxed);
    }

    /// Hold the serial line in break for `dur` (Cisco/HP password recovery,
    /// ROMMON); on Telnet, send Telnet's break command.
    pub fn send_break(&self, dur: Duration) -> Result<(), String> {
        let mut guard = self.writer.lock().unwrap();
        match guard.as_mut() {
            Some(Writer::Serial(p)) => {
                p.set_break().map_err(|e| e.to_string())?;
                thread::sleep(dur);
                p.clear_break().map_err(|e| e.to_string())
            }
            Some(w @ Writer::Tcp(_)) if self.telnet.is_some() => write_all(w, &telnet::BREAK),
            Some(Writer::Tcp(_) | Writer::Pty(_)) => {
                Err("break only works on a serial port or Telnet".into())
            }
            None => Err("not connected".into()),
        }
    }

    /// The terminal pane is now `cols`×`rows`. SSH resizes its pseudo-terminal
    /// (ssh passes it on); Telnet reports it when the device asked (NAWS);
    /// serial and raw TCP have no window size.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        if let Some(pty) = &self.pty {
            return pty.resize(cols, rows);
        }
        let Some(session) = &self.telnet else {
            return Ok(());
        };
        let Some(message) = session.lock().unwrap().resize(cols, rows) else {
            return Ok(());
        };
        let mut guard = self.writer.lock().unwrap();
        match guard.as_mut() {
            Some(w) => write_all(w, &message),
            None => Err("not connected".into()),
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.out = None; // ends the writer thread once its queue is empty
        if let Some(h) = self.out_handle.take() {
            let _ = h.join();
        }
        if let Ok(mut w) = self.writer.lock() {
            *w = None;
        }
        // The SSH waiter thread ends once ssh has.
        if let Some(pty) = self.pty.as_mut() {
            pty.kill();
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Write one queued message, either all at once or one byte at a time with a delay.
fn write_paced(
    writer: &SharedWriter,
    data: &[u8],
    pace: &AtomicU64,
    stop: &AtomicBool,
) -> Result<(), String> {
    let ms = pace.load(Ordering::Relaxed);
    if ms == 0 {
        let mut g = writer.lock().unwrap();
        let w = g.as_mut().ok_or("not connected")?;
        return write_all(w, data);
    }
    for (i, b) in data.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return Err(format!("stopped with {} bytes unsent", data.len() - i));
        }
        {
            let mut g = writer.lock().unwrap();
            let w = g.as_mut().ok_or("not connected")?;
            write_all(w, std::slice::from_ref(b))?;
        }
        if i + 1 < data.len() {
            thread::sleep(Duration::from_millis(ms));
        }
    }
    Ok(())
}

/// Write loop that tolerates the short serial timeouts on slow baud rates.
fn write_all(w: &mut Writer, data: &[u8]) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut rest = data;
    while !rest.is_empty() {
        match w.write(rest) {
            Ok(0) => {}
            Ok(n) => rest = &rest[n..],
            Err(e) if is_transient(&e) => {}
            Err(e) => return Err(e.to_string()),
        }
        if !rest.is_empty() && Instant::now() > deadline {
            return Err(format!("timed out with {} bytes unsent", rest.len()));
        }
    }
    w.flush().map_err(|e| e.to_string())
}

fn listen_loop(
    listener: TcpListener,
    writer: SharedWriter,
    stop: Arc<AtomicBool>,
    tx: Sender<LinkEvent>,
) {
    let mut client: Option<TcpStream> = None;
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        if let Some(s) = client.as_mut() {
            match s.read(&mut buf) {
                Ok(0) => {
                    client = None;
                    *writer.lock().unwrap() = None;
                    let _ = tx.send(LinkEvent::PeerDisconnected(
                        "Client disconnected. Waiting for the next one…".into(),
                    ));
                }
                Ok(n) => {
                    if tx.send(LinkEvent::Data(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(e) if is_transient(&e) => {}
                Err(e) => {
                    client = None;
                    *writer.lock().unwrap() = None;
                    let _ = tx.send(LinkEvent::PeerDisconnected(format!(
                        "Client connection lost ({e}). Waiting for the next one…"
                    )));
                }
            }
        } else {
            match listener.accept() {
                Ok((s, addr)) => {
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_read_timeout(Some(READ_TIMEOUT));
                    let _ = s.set_nodelay(true);
                    if let Ok(w) = s.try_clone() {
                        *writer.lock().unwrap() = Some(Writer::Tcp(w));
                    }
                    client = Some(s);
                    let _ = tx.send(LinkEvent::PeerConnected(addr.to_string()));
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50))
                }
                Err(e) if is_transient(&e) => {}
                Err(e) => {
                    let _ = tx.send(LinkEvent::Closed(format!("Listener failed: {e}")));
                    break;
                }
            }
        }
    }
}

/// Reads a Telnet connection: answers negotiation straight away, posts the
/// device's data, and posts what was agreed for the raw log.
fn telnet_read_loop(
    mut reader: TcpStream,
    session: Arc<Mutex<telnet::Telnet>>,
    writer: SharedWriter,
    stop: Arc<AtomicBool>,
    tx: Sender<LinkEvent>,
) {
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        let n = match reader.read(&mut buf) {
            Ok(0) => {
                let _ = tx.send(LinkEvent::Closed("Connection closed by peer.".into()));
                break;
            }
            Ok(n) => n,
            Err(e) if is_transient(&e) => continue,
            Err(e) => {
                let _ = tx.send(LinkEvent::Closed(format!("Connection lost: {e}")));
                break;
            }
        };
        let received = session.lock().unwrap().receive(&buf[..n]);
        if !received.replies.is_empty() {
            let result = match writer.lock().unwrap().as_mut() {
                Some(w) => write_all(w, &received.replies),
                None => Err("not connected".into()),
            };
            if let Err(e) = result {
                let _ = tx.send(LinkEvent::SendError(format!("Telnet reply failed: {e}")));
            }
        }
        for note in received.notes {
            if tx.send(LinkEvent::Info(note)).is_err() {
                return;
            }
        }
        if !received.data.is_empty() && tx.send(LinkEvent::Data(received.data)).is_err() {
            break;
        }
    }
}

fn is_transient(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
    )
}

fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, String> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("Could not resolve '{host}': {e}\nCheck the host name / IP address."))?
        .collect();
    let mut last_err = None;
    for a in &addrs {
        match TcpStream::connect_timeout(a, Duration::from_secs(5)) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = Some(e),
        }
    }
    let Some(e) = last_err else {
        return Err(format!("'{host}' resolved to no addresses"));
    };
    let hint = match e.kind() {
        ErrorKind::ConnectionRefused => {
            "\nNothing is listening on that port, or a firewall is rejecting it."
        }
        ErrorKind::TimedOut => {
            "\nNo answer within 5 s. Check the IP address, network and firewall."
        }
        _ => "",
    };
    Err(format!("Failed to connect to {host}:{port}: {e}{hint}"))
}

fn serial_error(port: &str, e: &serialport::Error) -> String {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    let hint = if lower.contains("access is denied") || lower.contains("permission") {
        serial_permission_hint()
    } else if lower.contains("cannot find")
        || lower.contains("not found")
        || matches!(e.kind, serialport::ErrorKind::NoDevice)
    {
        "\nThat port doesn't exist right now. Check the cable / USB adapter, or pick one from the detected list."
    } else {
        ""
    };
    format!("Failed to open serial port {port}: {msg}{hint}")
}

#[cfg(windows)]
fn serial_permission_hint() -> &'static str {
    "\nThe port is probably open in another program (PuTTY, another RawTap, a vendor tool)."
}

/// On Linux, serial ports belong to the `dialout` group (`uucp` on Arch).
#[cfg(not(windows))]
fn serial_permission_hint() -> &'static str {
    "\nYour user may not be allowed to use serial ports: add it to the dialout group (uucp on Arch), e.g. `sudo usermod -aG dialout $USER`, then log out and back in. Or the port is open in another program."
}

#[cfg(windows)]
fn listen_permission_hint() -> &'static str {
    "\nWindows refused the port (reserved range or policy). Try another port."
}

#[cfg(not(windows))]
fn listen_permission_hint() -> &'static str {
    "\nPorts below 1024 need root on Linux. Try a port above 1024."
}

fn listen_error(bind: &str, port: u16, e: &std::io::Error) -> String {
    let hint = match e.kind() {
        ErrorKind::AddrInUse => "\nSomething else is already listening on that port.",
        ErrorKind::AddrNotAvailable => {
            "\nThat bind address isn't on this computer. Use 0.0.0.0 for all interfaces."
        }
        ErrorKind::PermissionDenied => listen_permission_hint(),
        _ => "",
    };
    format!("Failed to listen on {bind}:{port}: {e}{hint}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn wait_for<F: Fn(&LinkEvent) -> bool>(rx: &mpsc::Receiver<LinkEvent>, f: F) -> LinkEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let ev = rx
                .recv_timeout(deadline - Instant::now())
                .expect("timed out waiting for event");
            if f(&ev) {
                return ev;
            }
        }
    }

    /// A fake Telnet switch: negotiates like a Cisco, greets, then reports
    /// everything the client sends back on `seen`.
    #[test]
    fn telnet_negotiates_and_passes_clean_data() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_tx, seen_rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            // IAC WILL ECHO, IAC WILL SGA, IAC DO NAWS, then a prompt.
            sock.write_all(&[255, 251, 1, 255, 251, 3, 255, 253, 31])
                .unwrap();
            sock.write_all(b"\r\nUsername: ").unwrap();
            sock.set_read_timeout(Some(Duration::from_millis(200)))
                .unwrap();
            let mut buf = [0u8; 256];
            loop {
                match sock.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if seen_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) if is_transient(&e) => {}
                    Err(_) => break,
                }
            }
        });

        let (tx, rx) = mpsc::channel();
        let link = Link::open(
            &LinkSettings::Telnet {
                host: "127.0.0.1".into(),
                port,
            },
            tx,
        )
        .unwrap();

        // What was agreed is noted for the raw log (before the data), and the
        // device's data arrives without the negotiation.
        wait_for(
            &rx,
            |e| matches!(e, LinkEvent::Info(m) if m.contains("window size")),
        );
        let LinkEvent::Data(data) = wait_for(&rx, |e| matches!(e, LinkEvent::Data(_))) else {
            unreachable!()
        };
        assert_eq!(data, b"\r\nUsername: ");

        // Answers: DO ECHO, DO SGA, WILL NAWS, then the window size (80×24
        // until the monitor reports the pane's size).
        let mut replies = Vec::new();
        let wanted = [
            255, 253, 1, 255, 253, 3, 255, 251, 31, 255, 250, 31, 0, 80, 0, 24, 255, 240,
        ];
        let deadline = Instant::now() + Duration::from_secs(5);
        while replies.len() < wanted.len() && Instant::now() < deadline {
            if let Ok(chunk) = seen_rx.recv_timeout(Duration::from_millis(200)) {
                replies.extend(chunk);
            }
        }
        assert_eq!(replies, wanted);

        // A resize is reported, a 255 byte is doubled, and F7 sends IAC BRK.
        // (A break goes out straight away, ahead of anything still queued, as
        // on serial, so each is checked in turn.)
        let receive = |count: usize| {
            let mut got = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(5);
            while got.len() < count && Instant::now() < deadline {
                if let Ok(chunk) = seen_rx.recv_timeout(Duration::from_millis(200)) {
                    got.extend(chunk);
                }
            }
            got
        };
        link.resize(120, 40).unwrap();
        assert_eq!(receive(9), [255, 250, 31, 0, 120, 0, 40, 255, 240]);
        link.send(&[b'a', 255]).unwrap();
        assert_eq!(receive(3), [b'a', 255, 255]);
        link.send_break(Duration::ZERO).unwrap();
        assert_eq!(receive(2), [255, 243]);
    }

    #[test]
    fn tcp_client_and_server_talk() {
        // Pick a free port.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (stx, srx) = mpsc::channel();
        let server = Link::open(
            &LinkSettings::TcpListen {
                bind: "127.0.0.1".into(),
                port,
            },
            stx,
        )
        .unwrap();
        let (ctx, crx) = mpsc::channel();
        let client = Link::open(
            &LinkSettings::TcpClient {
                host: "127.0.0.1".into(),
                port,
            },
            ctx,
        )
        .unwrap();

        wait_for(&srx, |e| matches!(e, LinkEvent::PeerConnected(_)));
        client.send(b"\x02PING\x03").unwrap();
        let LinkEvent::Data(d) = wait_for(&srx, |e| matches!(e, LinkEvent::Data(_))) else {
            unreachable!()
        };
        assert_eq!(d, b"\x02PING\x03");

        server.send(b"PONG\r\n").unwrap();
        let LinkEvent::Data(d) = wait_for(&crx, |e| matches!(e, LinkEvent::Data(_))) else {
            unreachable!()
        };
        assert_eq!(d, b"PONG\r\n");

        // Paced send: same bytes, arriving spread out over time.
        client.set_pacing(20);
        let t0 = Instant::now();
        client.send(b"admin\r").unwrap();
        let mut got = Vec::new();
        while got.len() < 6 {
            if let LinkEvent::Data(d) = wait_for(&srx, |e| matches!(e, LinkEvent::Data(_))) {
                got.extend(d);
            }
        }
        assert_eq!(got, b"admin\r");
        assert!(
            t0.elapsed() >= Duration::from_millis(90),
            "pacing not applied"
        );

        drop(server);
        wait_for(&crx, |e| matches!(e, LinkEvent::Closed(_)));
    }
}
