//! Connections: serial port, TCP client, TCP server (one client at a time).
//! A background thread reads and posts events; sends go through a shared writer.

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
    PeerConnected(String),
    PeerDisconnected(String),
    /// A queued send failed (sends happen on a background thread).
    SendError(String),
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
    TcpClient { host: String, port: u16 },
    TcpListen { bind: String, port: u16 },
}

impl LinkSettings {
    pub fn kind(&self) -> &'static str {
        match self {
            LinkSettings::Serial { .. } => "serial",
            LinkSettings::TcpClient { .. } => "tcp",
            LinkSettings::TcpListen { .. } => "listen",
        }
    }

    pub fn describe(&self) -> String {
        match self {
            LinkSettings::Serial { port, baud, data_bits, parity, stop_bits, flow } => {
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
        }
    }
}

enum Writer {
    Serial(Box<dyn serialport::SerialPort>),
    Tcp(TcpStream),
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Writer::Serial(p) => p.write(buf),
            Writer::Tcp(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Writer::Serial(p) => p.flush(),
            Writer::Tcp(s) => s.flush(),
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
}

impl Link {
    pub fn open(settings: &LinkSettings, tx: Sender<LinkEvent>) -> Result<Link, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let writer: SharedWriter = Arc::new(Mutex::new(None));
        let err_tx = tx.clone();
        let handle = match settings {
            LinkSettings::Serial { port, baud, data_bits, parity, stop_bits, flow } => {
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
                let mut reader = sp.try_clone().map_err(|e| format!("Could not clone {port}: {e}"))?;
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
                                let _ = tx.send(LinkEvent::Closed(format!("Serial port error: {e}")));
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
                                let _ = tx.send(LinkEvent::Closed("Connection closed by peer.".into()));
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
        })
    }

    /// Queue bytes to send. Fails straight away if there's nothing to send to.
    pub fn send(&self, data: &[u8]) -> Result<(), String> {
        if self.writer.lock().unwrap().is_none() {
            return Err("no client connected yet".into());
        }
        match &self.out {
            Some(out) => out.send(data.to_vec()).map_err(|_| "connection is closing".to_string()),
            None => Err("connection is closing".into()),
        }
    }

    pub fn set_pacing(&self, ms: u64) {
        self.pacing_ms.store(ms, Ordering::Relaxed);
    }

    /// Hold the serial line in break for `dur` (Cisco/HP password recovery, ROMMON).
    pub fn send_break(&self, dur: Duration) -> Result<(), String> {
        let guard = self.writer.lock().unwrap();
        match guard.as_ref() {
            Some(Writer::Serial(p)) => {
                p.set_break().map_err(|e| e.to_string())?;
                thread::sleep(dur);
                p.clear_break().map_err(|e| e.to_string())
            }
            Some(Writer::Tcp(_)) => Err("break only works on a serial port".into()),
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
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Write one queued message, either all at once or one byte at a time with a delay.
fn write_paced(writer: &SharedWriter, data: &[u8], pace: &AtomicU64, stop: &AtomicBool) -> Result<(), String> {
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

fn listen_loop(listener: TcpListener, writer: SharedWriter, stop: Arc<AtomicBool>, tx: Sender<LinkEvent>) {
    let mut client: Option<TcpStream> = None;
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        if let Some(s) = client.as_mut() {
            match s.read(&mut buf) {
                Ok(0) => {
                    client = None;
                    *writer.lock().unwrap() = None;
                    let _ = tx.send(LinkEvent::PeerDisconnected("Client disconnected. Waiting for the next one…".into()));
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
                    let _ = tx.send(LinkEvent::PeerDisconnected(format!("Client connection lost ({e}). Waiting for the next one…")));
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
                Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(50)),
                Err(e) if is_transient(&e) => {}
                Err(e) => {
                    let _ = tx.send(LinkEvent::Closed(format!("Listener failed: {e}")));
                    break;
                }
            }
        }
    }
}

fn is_transient(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted)
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
        ErrorKind::ConnectionRefused => "\nNothing is listening on that port, or a firewall is rejecting it.",
        ErrorKind::TimedOut => "\nNo answer within 5 s. Check the IP address, network and firewall.",
        _ => "",
    };
    Err(format!("Failed to connect to {host}:{port}: {e}{hint}"))
}

fn serial_error(port: &str, e: &serialport::Error) -> String {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    let hint = if lower.contains("access is denied") || lower.contains("permission") {
        "\nThe port is probably open in another program (PuTTY, another RawTap, a vendor tool)."
    } else if lower.contains("cannot find") || lower.contains("not found") || matches!(e.kind, serialport::ErrorKind::NoDevice) {
        "\nThat port doesn't exist right now. Check the cable / USB adapter, or pick one from the detected list."
    } else {
        ""
    };
    format!("Failed to open serial port {port}: {msg}{hint}")
}

fn listen_error(bind: &str, port: u16, e: &std::io::Error) -> String {
    let hint = match e.kind() {
        ErrorKind::AddrInUse => "\nSomething else is already listening on that port.",
        ErrorKind::AddrNotAvailable => "\nThat bind address isn't on this PC. Use 0.0.0.0 for all interfaces.",
        ErrorKind::PermissionDenied => "\nWindows refused the port (reserved range or policy). Try another port.",
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
            let ev = rx.recv_timeout(deadline - Instant::now()).expect("timed out waiting for event");
            if f(&ev) {
                return ev;
            }
        }
    }

    #[test]
    fn tcp_client_and_server_talk() {
        // Pick a free port.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let (stx, srx) = mpsc::channel();
        let server = Link::open(&LinkSettings::TcpListen { bind: "127.0.0.1".into(), port }, stx).unwrap();
        let (ctx, crx) = mpsc::channel();
        let client = Link::open(&LinkSettings::TcpClient { host: "127.0.0.1".into(), port }, ctx).unwrap();

        wait_for(&srx, |e| matches!(e, LinkEvent::PeerConnected(_)));
        client.send(b"\x02PING\x03").unwrap();
        let LinkEvent::Data(d) = wait_for(&srx, |e| matches!(e, LinkEvent::Data(_))) else { unreachable!() };
        assert_eq!(d, b"\x02PING\x03");

        server.send(b"PONG\r\n").unwrap();
        let LinkEvent::Data(d) = wait_for(&crx, |e| matches!(e, LinkEvent::Data(_))) else { unreachable!() };
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
        assert!(t0.elapsed() >= Duration::from_millis(90), "pacing not applied");

        drop(server);
        wait_for(&crx, |e| matches!(e, LinkEvent::Closed(_)));
    }
}
