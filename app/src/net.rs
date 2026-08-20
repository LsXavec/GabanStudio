//! THE SESSION (research/PSD-session-room.md, gate passed 2026-08-18):
//! host a file room over direct IP; joining needs the room's API key AND a
//! fresh TOTP code from the HOST's authenticator (Authy manual-entry
//! compatible, RFC 6238). Inside the room: presence — every artist's
//! cursor, name, and live wet stroke — plus the host document mirrored to
//! guests (join snapshot + change-driven refresh).
//!
//! LAWS (the room's NEVER-DO, enforced here):
//! - One truth: guests render the HOST's document; refresh is a full
//!   snapshot, never a merge.
//! - Auth gates: challenge-response HMAC over (nonce ‖ code) — the key
//!   never crosses the wire; a verified code is BURNED for its window.
//! - Disconnect discards; nothing half-done is ever committed.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

// ---------------------------------------------------------------------------
// SESSION WIRE LOG (2026-08-19, diagnosis instrument): every significant
// session event lands in %APPDATA%/AnimStudio/session_log.txt with a
// timestamp — sizes, counts, stalls — so a lag report reads as data,
// not deduction. Truncated at every app start; cheap enough to stay on.
// ---------------------------------------------------------------------------

static SLOG: Mutex<Option<(std::fs::File, std::time::Instant)>> = Mutex::new(None);

/// Open (truncate) the log. Called once at app start.
pub fn slog_init() {
    if let Some(base) = std::env::var_os("APPDATA") {
        let dir = std::path::PathBuf::from(base).join("AnimStudio");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(f) = std::fs::File::create(dir.join("session_log.txt")) {
            *SLOG.lock().unwrap() = Some((f, std::time::Instant::now()));
        }
    }
    slog(format!(
        "session log · build v{} · {}",
        env!("CARGO_PKG_VERSION"),
        std::env::var("COMPUTERNAME").unwrap_or_default()
    ));
}

pub fn slog(msg: impl AsRef<str>) {
    if let Some((f, t0)) = SLOG.lock().unwrap().as_mut() {
        use std::io::Write as _;
        let _ = writeln!(f, "[{:9.3}s] {}", t0.elapsed().as_secs_f64(), msg.as_ref());
    }
}

// ---------------------------------------------------------------------------
// Base32 (RFC 4648, no padding) — for keys and Authy manual entry.
// ---------------------------------------------------------------------------

const B32: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn base32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for &b in data {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b' ' || c == b'-' {
            continue;
        }
        let v = B32.iter().position(|&b| b == c.to_ascii_uppercase())? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 255) as u8);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// TOTP (RFC 6238, SHA-1, 30s step, 6 digits) — Authy manual entry.
// ---------------------------------------------------------------------------

fn hotp(secret: &[u8], counter: u64) -> u32 {
    let mut mac = HmacSha1::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(&counter.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let off = (h[19] & 0x0f) as usize;
    let bin = ((h[off] as u32 & 0x7f) << 24)
        | ((h[off + 1] as u32) << 16)
        | ((h[off + 2] as u32) << 8)
        | (h[off + 3] as u32);
    bin % 1_000_000
}

pub fn totp_at(secret: &[u8], unix_time: u64) -> u32 {
    hotp(secret, unix_time / 30)
}

/// The code the HOST's authenticator is showing right now, and the seconds
/// left in its window — so the host can read it out without alt-tabbing
/// (and can prove enrollment matched).
pub fn current_code(secret_b32: &str) -> Option<(String, u64)> {
    let secret = base32_decode(secret_b32)?;
    if secret.is_empty() {
        return None;
    }
    let now = now_unix();
    Some((format!("{:06}", totp_at(&secret, now)), 30 - (now % 30)))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Verify a 6-digit code against the secret with a ±1-step window; a
/// verified (window, code) pair is BURNED — replay refused (NEVER-DO 3).
#[allow(dead_code)] // dormant with the 2FA flow (owner 2026-08-19)
fn verify_totp(secret: &[u8], code: &str, burned: &mut HashSet<(u64, u32)>) -> bool {
    let Ok(c) = code.trim().parse::<u32>() else {
        return false;
    };
    let step = now_unix() / 30;
    for w in [step.wrapping_sub(1), step, step + 1] {
        if hotp(secret, w) == c {
            if burned.contains(&(w, c)) {
                return false;
            }
            burned.insert((w, c));
            // Old burns age out (keep the set tiny).
            burned.retain(|(bw, _)| step.saturating_sub(*bw) < 10);
            return true;
        }
    }
    false
}

pub fn generate_key() -> String {
    let bytes: [u8; 20] = rand::random();
    base32_encode(&bytes)
}

// ---------------------------------------------------------------------------
// Wire: length-prefixed frames — kind 0 = JSON message, kind 1 = raw
// document snapshot bytes.
// ---------------------------------------------------------------------------

const FRAME_JSON: u8 = 0;
const FRAME_DOC: u8 = 1;
/// SESSION MIRROR: a serde_json Vec<Command> batch (host → guests).
const FRAME_CMDS: u8 = 2;
/// Snapshots of real projects can be large; everything else is small.
const MAX_FRAME: u32 = 256 * 1024 * 1024;

fn write_frame(s: &mut TcpStream, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    s.write_all(&(payload.len() as u32).to_be_bytes())?;
    s.write_all(&[kind])?;
    s.write_all(payload)
}

fn read_frame(s: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len);
    if len > MAX_FRAME {
        return Err(std::io::Error::other("frame too large"));
    }
    let mut kind = [0u8; 1];
    s.read_exact(&mut kind)?;
    let mut buf = vec![0u8; len as usize];
    s.read_exact(&mut buf)?;
    Ok((kind[0], buf))
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum Msg {
    /// Server → client on connect: prove you hold the key + a fresh code.
    Nonce {
        nonce: String,
    },
    /// Client → server: hex HMAC-SHA1(key, nonce ‖ code). The key itself
    /// never crosses the wire.
    Auth {
        username: String,
        code: String,
        mac: String,
        /// The guest's build (serde-default: old builds send none).
        #[serde(default)]
        version: String,
    },
    Welcome {
        peer_id: u64,
        peers: Vec<(u64, String)>,
    },
    Refused {
        why: String,
    },
    PeerJoined {
        id: u64,
        name: String,
        #[serde(default)]
        version: String,
    },
    PeerLeft {
        id: u64,
    },
    /// V2 (research/PSD-session-v2.md): a guest's finished stroke, as the
    /// tile patch their GPU already computed, addressed BY IDENTITY —
    /// drawing id + LAYER NAME (NEVER-DO 2). The HOST applies it through
    /// its own guarded path; the guest never commits locally.
    EditTiles {
        author: String,
        drawing: u64,
        layer_name: String,
        /// (tile x, tile y, RGBA16F texels) — after-state only; the host
        /// derives the before-state from its own authoritative document.
        tiles: Vec<(i32, i32, Vec<u16>)>,
    },
    /// The host telling one author their edit could not land, and why.
    EditRefused {
        author: String,
        why: String,
    },
    /// A guest's mirror slipped (a batch would not apply): please send
    /// a fresh snapshot.
    ResyncRequest {},
    /// A guest asking the host to undo/redo THEIR last step.
    UndoRequest {
        author: String,
        redo: bool,
    },
    /// Presence: where an artist is and what their pen is doing.
    Presence {
        id: u64,
        frame: u32,
        cursor: Option<[f32; 2]>,
        pen_down: bool,
        wet: Vec<[f32; 3]>,
    },
}

/// SESSION PERF 3: what a writer thread carries. Raw frames go as-is;
/// Json is serialized ON THE WRITER (EditTiles' tile payloads are the
/// expensive ones — never on the UI thread).
enum Out {
    Raw(u8, Vec<u8>),
    Json(Box<Msg>),
    /// Serialized ON THE WRITER (tile payloads are the expensive part).
    Cmds(Vec<anim_core::command::Command>),
}

/// The writer: drains the queue into the socket; dies with either end.
fn spawn_writer(mut stream: TcpStream, rx: Receiver<Out>) {
    std::thread::spawn(move || {
        for out in rx {
            let t = std::time::Instant::now();
            let (ok, kind, len) = match out {
                Out::Raw(kind, payload) => (
                    write_frame(&mut stream, kind, &payload).is_ok(),
                    kind,
                    payload.len(),
                ),
                Out::Json(msg) => match serde_json::to_vec(&*msg) {
                    Ok(bytes) => (
                        write_frame(&mut stream, FRAME_JSON, &bytes).is_ok(),
                        FRAME_JSON,
                        bytes.len(),
                    ),
                    Err(_) => (true, FRAME_JSON, 0),
                },
                Out::Cmds(batch) => match serde_json::to_vec(&batch) {
                    Ok(bytes) => (
                        write_frame(&mut stream, FRAME_CMDS, &bytes).is_ok(),
                        FRAME_CMDS,
                        bytes.len(),
                    ),
                    Err(_) => (true, FRAME_CMDS, 0),
                },
            };
            let ms = t.elapsed().as_millis();
            if len > 64 * 1024 || kind != FRAME_JSON || ms > 100 {
                slog(format!(
                    "OUT kind={kind} bytes={len} write={ms}ms ok={ok}"
                ));
            }
            if !ok {
                slog("writer: socket closed");
                return;
            }
        }
    });
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn auth_mac(key: &str, nonce: &str, code: &str) -> String {
    let key_bytes = base32_decode(key).unwrap_or_default();
    let mut mac = HmacSha1::new_from_slice(&key_bytes).expect("any length");
    mac.update(nonce.as_bytes());
    mac.update(code.as_bytes());
    hex(&mac.finalize().into_bytes())
}

// ---------------------------------------------------------------------------
// Events surfaced to the UI thread (both roles).
// ---------------------------------------------------------------------------

pub enum NetEvent {
    Status(String),
    PeerJoined {
        id: u64,
        name: String,
        version: String,
    },
    PeerLeft {
        id: u64,
    },
    Presence {
        id: u64,
        frame: u32,
        cursor: Option<[f32; 2]>,
        pen_down: bool,
        wet: Vec<[f32; 3]>,
    },
    /// Host side: a guest's finished stroke, to apply locally.
    EditTiles {
        author: String,
        drawing: u64,
        layer_name: String,
        tiles: Vec<(i32, i32, Vec<u16>)>,
    },
    /// Host side: a guest asking to undo/redo their own last step.
    UndoRequest {
        author: String,
        redo: bool,
    },
    /// Guest side: the host refused this artist's edit, with why.
    EditRefused(String),
    /// Guest side: one applied batch from the host's engine.
    Commands(Vec<anim_core::command::Command>),
    /// Host side: a guest's mirror slipped — send a fresh snapshot.
    ResyncNeeded,
    /// Guest side: a fresh authoritative document from the host.
    Snapshot(Vec<u8>),
    /// The connection or room ended.
    Ended(String),
}

/// What the UI pushes out each frame.
pub struct PresenceOut {
    pub frame: u32,
    pub cursor: Option<[f32; 2]>,
    pub pen_down: bool,
    pub wet: Vec<[f32; 3]>,
}

// ---------------------------------------------------------------------------
// HOST
// ---------------------------------------------------------------------------

struct ClientHandle {
    name: String,
    /// SESSION PERF 3: the outbound queue; a writer thread owns the
    /// socket. A stalled peer stalls its own queue, never the UI.
    tx: std::sync::mpsc::Sender<Out>,
}

pub struct Host {
    pub events: Receiver<NetEvent>,
    clients: Arc<Mutex<HashMap<u64, ClientHandle>>>,
    stop: Arc<Mutex<bool>>,
    pub port: u16,
}

impl Host {
    pub fn start(
        port: u16,
        key: String,
        totp_secret: String,
        room_name: String,
        open: bool,
    ) -> std::io::Result<Host> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let (tx, rx) = channel();
        let clients: Arc<Mutex<HashMap<u64, ClientHandle>>> = Arc::new(Mutex::new(HashMap::new()));
        let stop = Arc::new(Mutex::new(false));
        spawn_beacon(room_name, port, open, stop.clone());
        let burned: Arc<Mutex<HashSet<(u64, u32)>>> = Arc::new(Mutex::new(HashSet::new()));
        {
            let clients = clients.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut next_id: u64 = 1;
                loop {
                    if *stop.lock().unwrap() {
                        return;
                    }
                    match listener.accept() {
                        Ok((stream, addr)) => {
                            let id = next_id;
                            next_id += 1;
                            let key = key.clone();
                            let secret = totp_secret.clone();
                            let tx = tx.clone();
                            let clients = clients.clone();
                            let burned = burned.clone();
                            std::thread::spawn(move || {
                                let mut stream = stream;
                                // WINDOWS: a stream accepted from a
                                // NON-BLOCKING listener inherits that mode —
                                // the handshake read would return WouldBlock
                                // and drop the connection. Clear it first.
                                stream.set_nonblocking(false).ok();
                                stream
                                    .set_read_timeout(Some(std::time::Duration::from_secs(20)))
                                    .ok();
                                let nonce_bytes: [u8; 16] = rand::random();
                                let nonce = hex(&nonce_bytes);
                                let hello = serde_json::to_vec(&Msg::Nonce {
                                    nonce: nonce.clone(),
                                })
                                .unwrap();
                                if write_frame(&mut stream, FRAME_JSON, &hello).is_err() {
                                    return;
                                }
                                let Ok((FRAME_JSON, buf)) = read_frame(&mut stream) else {
                                    return;
                                };
                                let Ok(Msg::Auth {
                                    username,
                                    code,
                                    mac,
                                    version,
                                }) = serde_json::from_slice::<Msg>(&buf)
                                else {
                                    return;
                                };
                                // OWNER AMENDMENT 2026-08-19: TOTP is
                                // dormant — the KEY is the gate. The mac
                                // still covers whatever code string the
                                // guest sent (older builds type one), so
                                // the wire shape is unchanged and replays
                                // still die on the nonce.
                                let _ = (&secret, &burned);
                                // LAN-open rooms (testing phase): no key
                                // check at all — the amendment's trade.
                                let mac_ok =
                                    open || mac == auth_mac(&key, &nonce, &code);
                                if !mac_ok {
                                    let msg = serde_json::to_vec(&Msg::Refused {
                                        why: "the room key was refused".into(),
                                    })
                                    .unwrap();
                                    let _ = write_frame(&mut stream, FRAME_JSON, &msg);
                                    return;
                                }
                                stream.set_read_timeout(None).ok();
                                // Welcome + register.
                                let peers: Vec<(u64, String)> = clients
                                    .lock()
                                    .unwrap()
                                    .iter()
                                    .map(|(pid, c)| (*pid, c.name.clone()))
                                    .collect();
                                let welcome =
                                    serde_json::to_vec(&Msg::Welcome { peer_id: id, peers })
                                        .unwrap();
                                if write_frame(&mut stream, FRAME_JSON, &welcome).is_err() {
                                    return;
                                }
                                let reader = match stream.try_clone() {
                                    Ok(r) => r,
                                    Err(_) => return,
                                };
                                let (out_tx, out_rx) = channel();
                                spawn_writer(stream, out_rx);
                                clients.lock().unwrap().insert(
                                    id,
                                    ClientHandle {
                                        name: username.clone(),
                                        tx: out_tx,
                                    },
                                );
                                // Announce to everyone else + the UI.
                                let joined = serde_json::to_vec(&Msg::PeerJoined {
                                    id,
                                    name: username.clone(),
                                    version: version.clone(),
                                })
                                .unwrap();
                                broadcast_except(&clients, id, FRAME_JSON, &joined);
                                let _ = tx.send(NetEvent::PeerJoined {
                                    id,
                                    name: username.clone(),
                                    version,
                                });
                                let _ = tx
                                    .send(NetEvent::Status(format!("{username} joined ({addr})")));
                                // Read loop: relay presence.
                                let mut reader = reader;
                                loop {
                                    match read_frame(&mut reader) {
                                        Ok((FRAME_JSON, buf)) => {
                                            match serde_json::from_slice::<Msg>(&buf) {
                                                Ok(Msg::EditTiles {
                                                    author,
                                                    drawing,
                                                    layer_name,
                                                    tiles,
                                                }) => {
                                                    let _ = tx.send(NetEvent::EditTiles {
                                                        author,
                                                        drawing,
                                                        layer_name,
                                                        tiles,
                                                    });
                                                    continue;
                                                }
                                                Ok(Msg::ResyncRequest {}) => {
                                                    let _ = tx.send(NetEvent::ResyncNeeded);
                                                    continue;
                                                }
                                                Ok(Msg::UndoRequest { author, redo }) => {
                                                    let _ = tx.send(NetEvent::UndoRequest {
                                                        author,
                                                        redo,
                                                    });
                                                    continue;
                                                }
                                                _ => {}
                                            }
                                            if let Ok(Msg::Presence {
                                                frame,
                                                cursor,
                                                pen_down,
                                                wet,
                                                ..
                                            }) = serde_json::from_slice::<Msg>(&buf)
                                            {
                                                let rebroadcast =
                                                    serde_json::to_vec(&Msg::Presence {
                                                        id,
                                                        frame,
                                                        cursor,
                                                        pen_down,
                                                        wet: wet.clone(),
                                                    })
                                                    .unwrap();
                                                broadcast_except(
                                                    &clients,
                                                    id,
                                                    FRAME_JSON,
                                                    &rebroadcast,
                                                );
                                                let _ = tx.send(NetEvent::Presence {
                                                    id,
                                                    frame,
                                                    cursor,
                                                    pen_down,
                                                    wet,
                                                });
                                            }
                                        }
                                        Ok(_) => {}
                                        Err(_) => break,
                                    }
                                }
                                clients.lock().unwrap().remove(&id);
                                let left = serde_json::to_vec(&Msg::PeerLeft { id }).unwrap();
                                broadcast_except(&clients, id, FRAME_JSON, &left);
                                let _ = tx.send(NetEvent::PeerLeft { id });
                            });
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(120));
                        }
                        Err(_) => return,
                    }
                }
            });
        }
        Ok(Host {
            events: rx,
            clients,
            stop,
            port,
        })
    }

    /// The host's own presence, to every guest.
    pub fn send_presence(&self, p: &PresenceOut) {
        let msg = serde_json::to_vec(&Msg::Presence {
            id: 0,
            frame: p.frame,
            cursor: p.cursor,
            pen_down: p.pen_down,
            wet: p.wet.clone(),
        })
        .unwrap();
        broadcast_except(&self.clients, u64::MAX, FRAME_JSON, &msg);
    }

    /// Tell every guest an edit was refused (the author filters by name —
    /// small rooms, and a refusal is never secret from collaborators).
    pub fn send_refusal(&self, author: &str, why: &str) {
        let msg = serde_json::to_vec(&Msg::EditRefused {
            author: author.to_string(),
            why: why.to_string(),
        })
        .unwrap();
        broadcast_except(&self.clients, u64::MAX, FRAME_JSON, &msg);
    }

    /// SESSION MIRROR: one applied batch, to every guest.
    pub fn send_commands(&self, batch: Vec<anim_core::command::Command>) {
        let mut dead = Vec::new();
        {
            let mut map = self.clients.lock().unwrap();
            for (id, c) in map.iter_mut() {
                if c.tx.send(Out::Cmds(batch.clone())).is_err() {
                    dead.push(*id);
                }
            }
            for id in &dead {
                map.remove(id);
            }
        }
    }

    /// The authoritative document, to every guest (join + refresh).
    pub fn send_snapshot(&self, doc: &[u8]) {
        broadcast_except(&self.clients, u64::MAX, FRAME_DOC, doc);
    }

    #[allow(dead_code)]
    // status surface for a later pass
    pub fn peer_count(&self) -> usize {
        self.clients.lock().unwrap().len()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        *self.stop.lock().unwrap() = true;
        self.clients.lock().unwrap().clear();
    }
}

fn broadcast_except(
    clients: &Arc<Mutex<HashMap<u64, ClientHandle>>>,
    except: u64,
    kind: u8,
    payload: &[u8],
) {
    // SESSION PERF 3: enqueue only — the writer threads own the sockets.
    let mut dead = Vec::new();
    {
        let mut map = clients.lock().unwrap();
        for (id, c) in map.iter_mut() {
            if *id == except {
                continue;
            }
            if c.tx.send(Out::Raw(kind, payload.to_vec())).is_err() {
                dead.push(*id);
            }
        }
        for id in &dead {
            map.remove(id);
        }
    }
}

// ---------------------------------------------------------------------------
// CLIENT (guest)
// ---------------------------------------------------------------------------

pub struct Client {
    pub events: Receiver<NetEvent>,
    tx: std::sync::mpsc::Sender<Out>,
    pub peer_id: u64,
}

impl Client {
    /// Connect + authenticate. `code` is the 6-digit number read from the
    /// HOST's authenticator. Blocking, with timeouts — called on click.
    pub fn connect(addr: &str, key: &str, code: &str, username: &str) -> Result<Client, String> {
        let addr = if addr.contains(':') {
            addr.to_string()
        } else {
            format!("{addr}:41100")
        };
        let sock_addr: std::net::SocketAddr = addr
            .parse()
            .map_err(|_| format!("bad address '{addr}' (use host:port)"))?;
        let mut stream = TcpStream::connect_timeout(&sock_addr, std::time::Duration::from_secs(6))
            .map_err(|e| format!("no answer from {addr}: {e}"))?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .ok();
        let (k, buf) = read_frame(&mut stream).map_err(|e| format!("handshake: {e}"))?;
        let Msg::Nonce { nonce } = (k == FRAME_JSON)
            .then(|| serde_json::from_slice::<Msg>(&buf).ok())
            .flatten()
            .ok_or("handshake: bad hello")?
        else {
            return Err("handshake: bad hello".into());
        };
        let auth = serde_json::to_vec(&Msg::Auth {
            username: username.to_string(),
            code: code.trim().to_string(),
            mac: auth_mac(key, &nonce, code.trim()),
            version: crate::update::CURRENT_VERSION.to_string(),
        })
        .unwrap();
        write_frame(&mut stream, FRAME_JSON, &auth).map_err(|e| e.to_string())?;
        let (k, buf) = read_frame(&mut stream).map_err(|e| format!("auth: {e}"))?;
        let peer_id = match (k == FRAME_JSON)
            .then(|| serde_json::from_slice::<Msg>(&buf).ok())
            .flatten()
        {
            Some(Msg::Welcome { peer_id, peers }) => {
                let _ = peers;
                peer_id
            }
            Some(Msg::Refused { why }) => return Err(why),
            _ => return Err("auth: unexpected reply".into()),
        };
        stream.set_read_timeout(None).ok();
        let (tx, rx) = channel();
        let reader_stream = stream.try_clone().map_err(|e| e.to_string())?;
        let (out_tx, out_rx) = channel();
        spawn_writer(stream, out_rx);
        let mut reader = reader_stream;
        std::thread::spawn(move || {
            loop {
                match read_frame(&mut reader) {
                    Ok((FRAME_DOC, bytes)) => {
                        slog(format!("IN snapshot bytes={}", bytes.len()));
                        let _ = tx.send(NetEvent::Snapshot(bytes));
                    }
                    Ok((FRAME_CMDS, bytes)) => {
                        // Deserialized HERE, on the reader thread.
                        slog(format!("IN cmds bytes={}", bytes.len()));
                        if let Ok(batch) = serde_json::from_slice::<Vec<anim_core::command::Command>>(&bytes) {
                            let _ = tx.send(NetEvent::Commands(batch));
                        } else {
                            slog("IN cmds: PARSE FAILED");
                        }
                    }
                    Ok((FRAME_JSON, buf)) => match serde_json::from_slice::<Msg>(&buf) {
                        Ok(Msg::Presence {
                            id,
                            frame,
                            cursor,
                            pen_down,
                            wet,
                        }) => {
                            let _ = tx.send(NetEvent::Presence {
                                id,
                                frame,
                                cursor,
                                pen_down,
                                wet,
                            });
                        }
                        Ok(Msg::PeerJoined { id, name, version }) => {
                            let _ =
                                tx.send(NetEvent::PeerJoined { id, name, version });
                        }
                        Ok(Msg::PeerLeft { id }) => {
                            let _ = tx.send(NetEvent::PeerLeft { id });
                        }
                        Ok(Msg::EditRefused { why, .. }) => {
                            let _ = tx.send(NetEvent::EditRefused(why));
                        }
                        _ => {}
                    },
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send(NetEvent::Ended(format!("room closed: {e}")));
                        return;
                    }
                }
            }
        });
        Ok(Client {
            events: rx,
            tx: out_tx,
            peer_id,
        })
    }

    /// Send a finished stroke to the host for application (v2).
    pub fn send_edit(
        &mut self,
        author: &str,
        drawing: u64,
        layer_name: &str,
        tiles: Vec<(i32, i32, Vec<u16>)>,
    ) {
        // SESSION PERF 3: the tile payload's JSON is the expensive part —
        // it serializes on the WRITER thread, never here.
        let _ = self.tx.send(Out::Json(Box::new(Msg::EditTiles {
            author: author.to_string(),
            drawing,
            layer_name: layer_name.to_string(),
            tiles,
        })));
    }

    /// Tell the host our mirror slipped.
    pub fn send_resync_request(&mut self) {
        let _ = self.tx.send(Out::Json(Box::new(Msg::ResyncRequest {})));
    }

    /// Ask the host to undo/redo THIS artist's last step (v2).
    pub fn send_undo(&mut self, author: &str, redo: bool) {
        let _ = self.tx.send(Out::Json(Box::new(Msg::UndoRequest {
            author: author.to_string(),
            redo,
        })));
    }
    pub fn send_presence(&mut self, p: &PresenceOut) {
        let _ = self.tx.send(Out::Json(Box::new(Msg::Presence {
            id: self.peer_id,
            frame: p.frame,
            cursor: p.cursor,
            pen_down: p.pen_down,
            wet: p.wet.clone(),
        })));
    }
}

/// A peer as the canvas draws them.
#[derive(Clone)]
pub struct PeerView {
    pub name: String,
    /// The peer's build, for the room lamp's cross-check.
    pub version: String,
    pub frame: u32,
    pub cursor: Option<[f32; 2]>,
    pub pen_down: bool,
    pub wet: Vec<[f32; 3]>,
}

// ---------------------------------------------------------------------------
// LAN DISCOVERY (owner amendment 2026-08-19): a hosting app broadcasts a
// UDP beacon every 2s; every app listens and lists rooms on this network
// for one-click joining. Testing-phase trade, recorded in the room.
// ---------------------------------------------------------------------------

const DISCOVERY_PORT: u16 = 41101;

/// A room seen on this network.
#[derive(Clone)]
pub struct Beacon {
    pub name: String,
    pub addr: String,
    pub version: String,
    pub open: bool,
    pub last: std::time::Instant,
}

/// The always-on listener: rooms currently visible on this network.
pub struct Discovery {
    pub rooms: Arc<Mutex<HashMap<String, Beacon>>>,
}

pub fn spawn_discovery() -> Discovery {
    let rooms: Arc<Mutex<HashMap<String, Beacon>>> = Arc::new(Mutex::new(HashMap::new()));
    let r = rooms.clone();
    std::thread::spawn(move || {
        let Ok(sock) = std::net::UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) else {
            // A second instance on this machine holds the port — the
            // host side still beacons; only listing is lost here.
            slog("discovery: listen port busy (another instance?)");
            return;
        };
        let mut buf = [0u8; 512];
        loop {
            let Ok((n, src)) = sock.recv_from(&mut buf) else {
                continue;
            };
            let Ok(j) = serde_json::from_slice::<serde_json::Value>(&buf[..n]) else {
                continue;
            };
            if j["gaban"].as_u64() != Some(1) {
                continue;
            }
            let port = j["port"].as_u64().unwrap_or(41100) as u16;
            let addr = format!("{}:{port}", src.ip());
            let b = Beacon {
                name: j["name"].as_str().unwrap_or("room").to_string(),
                addr: addr.clone(),
                version: j["ver"].as_str().unwrap_or("?").to_string(),
                open: j["open"].as_bool().unwrap_or(false),
                last: std::time::Instant::now(),
            };
            let mut map = r.lock().unwrap();
            map.insert(addr, b);
            // Expire rooms that stopped beaconing.
            map.retain(|_, b| b.last.elapsed().as_secs() < 8);
        }
    });
    Discovery { rooms }
}

/// The host's beacon, alive while the room is.
fn spawn_beacon(name: String, port: u16, open: bool, stop: Arc<Mutex<bool>>) {
    std::thread::spawn(move || {
        let Ok(sock) = std::net::UdpSocket::bind(("0.0.0.0", 0)) else {
            return;
        };
        let _ = sock.set_broadcast(true);
        let msg = serde_json::json!({
            "gaban": 1,
            "name": name,
            "port": port,
            "ver": crate::update::CURRENT_VERSION,
            "open": open,
        })
        .to_string();
        loop {
            if *stop.lock().unwrap() {
                return;
            }
            let _ = sock.send_to(
                msg.as_bytes(),
                (std::net::Ipv4Addr::BROADCAST, DISCOVERY_PORT),
            );
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

/// The App's session state.
pub enum Session {
    Idle,
    Hosting(Host),
    Joined(Client),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_roundtrips() {
        for data in [b"hello".to_vec(), vec![0u8; 7], (0u8..=255).collect()] {
            assert_eq!(base32_decode(&base32_encode(&data)).unwrap(), data);
        }
    }

    #[test]
    fn totp_matches_rfc6238_vectors() {
        // RFC 6238 SHA-1 vectors, truncated to 6 digits.
        let secret = b"12345678901234567890";
        assert_eq!(totp_at(secret, 59), 287082 % 1_000_000);
        assert_eq!(totp_at(secret, 1111111109), 81804);
        assert_eq!(totp_at(secret, 1234567890), 5924);
    }

    #[test]
    fn burned_codes_refuse_replay() {
        let secret = b"12345678901234567890";
        let code = format!("{:06}", totp_at(secret, now_unix()));
        let mut burned = HashSet::new();
        assert!(verify_totp(secret, &code, &mut burned));
        assert!(
            !verify_totp(secret, &code, &mut burned),
            "replay must refuse"
        );
    }

    #[test]
    fn loopback_room_authenticates_and_relays() {
        let key = generate_key();
        let secret_raw: [u8; 20] = rand::random();
        let secret = base32_encode(&secret_raw);
        let host = Host::start(0, key.clone(), secret.clone(), "test room".into(), false)
            .expect("bind");
        let code = format!("{:06}", totp_at(&secret_raw, now_unix()));
        let addr = format!("127.0.0.1:{}", host.port);
        let mut guest = Client::connect(&addr, &key, &code, "guest-a").expect("join with key+code");
        // OWNER AMENDMENT 2026-08-19: the KEY is the gate. A wrong key
        // refuses; the code no longer gates (older builds may send one).
        assert!(
            Client::connect(&addr, &generate_key(), "000000", "bad").is_err(),
            "a wrong key must refuse"
        );
        let mut second =
            Client::connect(&addr, &key, "", "codeless").expect("key alone joins");
        second.send_presence(&PresenceOut {
            frame: 0,
            cursor: None,
            pen_down: false,
            wet: vec![],
        });
        // Host presence reaches the guest.
        std::thread::sleep(std::time::Duration::from_millis(150));
        host.send_presence(&PresenceOut {
            frame: 7,
            cursor: Some([10.0, 20.0]),
            pen_down: false,
            wet: vec![],
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut got = false;
        while std::time::Instant::now() < deadline {
            if let Ok(ev) = guest.events.try_recv() {
                if let NetEvent::Presence {
                    id: 0, frame: 7, ..
                } = ev
                {
                    got = true;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(got, "host presence must reach the guest");
        // Guest presence reaches the host's UI events.
        guest.send_presence(&PresenceOut {
            frame: 3,
            cursor: Some([1.0, 2.0]),
            pen_down: true,
            wet: vec![[1.0, 2.0, 3.0]],
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut got = false;
        while std::time::Instant::now() < deadline {
            if let Ok(NetEvent::Presence {
                frame: 3,
                pen_down: true,
                ..
            }) = host.events.try_recv()
            {
                got = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(got, "guest presence must reach the host");
    }
}
