//! `setun-relay`: a gateway for network cards that are somewhere else.
//!
//! The browser build's `TNIC` has no host to reach, so the page opens a
//! WebSocket here and passes Ethernet frames, one per binary message, both
//! ways. Each connection gets its own gateway (`setun_net::Network`, the one
//! behind `setun --net`: DHCP-less 10.0.2.15, DNS at 10.0.2.3, a NAT to the
//! world), in public-only mode: the relay's own host and private networks
//! are refused, so a visitor's machine reaches the internet and nothing
//! else.
//!
//!     setun-relay [ADDR:PORT]        default 127.0.0.1:8027

use setun_net::Network;
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

/// Connections served at once; more are turned away.
const MAX_CLIENTS: usize = 27;
/// A connection whose card sends nothing for this long is closed.
const IDLE: Duration = Duration::from_secs(30 * 60);
/// Ethernet frames, without the FCS: header to the largest MTU-sized one.
const FRAME_MIN: usize = 14;
const FRAME_MAX: usize = 1514;

fn main() {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8027".into());
    if addr == "--help" || addr == "-h" {
        println!("usage: setun-relay [ADDR:PORT]   (default 127.0.0.1:8027)");
        return;
    }
    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| {
        eprintln!("setun-relay: cannot listen on {addr}: {e}");
        std::process::exit(1)
    });
    eprintln!("setun-relay: listening on {addr}");
    let clients = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let clients = clients.clone();
        if clients.fetch_add(1, Ordering::SeqCst) >= MAX_CLIENTS {
            clients.fetch_sub(1, Ordering::SeqCst);
            drop(stream);
            continue;
        }
        std::thread::spawn(move || {
            let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
            match serve(stream) {
                Ok(n) => eprintln!("setun-relay: {peer} done, {n} frames"),
                Err(e) => eprintln!("setun-relay: {peer}: {e}"),
            }
            clients.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// One card: frames in, the gateway, frames out, until either side stops.
fn serve(stream: TcpStream) -> Result<u64, String> {
    stream.set_nodelay(true).ok();
    let mut ws = tungstenite::accept(stream).map_err(|e| format!("handshake: {e}"))?;
    ws.get_mut().set_read_timeout(Some(Duration::from_millis(2))).map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    let mut net = Network::new(0);
    net.set_public_only(true);
    let mut last_heard = Instant::now();
    let mut frames = 0u64;
    loop {
        let mut from_card = Vec::new();
        loop {
            match ws.read() {
                Ok(Message::Binary(b)) => {
                    last_heard = Instant::now();
                    if (FRAME_MIN..=FRAME_MAX).contains(&b.len()) {
                        from_card.push(b.to_vec());
                    }
                }
                Ok(Message::Close(_)) => return Ok(frames),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => break,
                Err(tungstenite::Error::ConnectionClosed) | Err(tungstenite::Error::AlreadyClosed) => return Ok(frames),
                Err(e) => return Err(e.to_string()),
            }
            if from_card.len() >= 64 {
                break;
            }
        }
        frames += from_card.len() as u64;
        let to_card = net.exchange(t0.elapsed().as_micros() as i64, from_card);
        send(&mut ws, to_card)?;
        if last_heard.elapsed() > IDLE {
            let _ = ws.close(None);
            return Ok(frames);
        }
    }
}

fn send(ws: &mut WebSocket<TcpStream>, frames: Vec<Vec<u8>>) -> Result<(), String> {
    if frames.is_empty() {
        return Ok(());
    }
    for f in frames {
        ws.write(Message::Binary(f.into())).map_err(|e| e.to_string())?;
    }
    ws.flush().map_err(|e| e.to_string())
}
