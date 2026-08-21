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

/// STAGE 0 (PSD-multiplayer-rescope): every log line is NUMBERED, and a
/// guest in a session buffers its lines for shipping to the host — the
/// ordered remote debug channel ("the users computer gives you what you
/// need for the debug").
static SLOG_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SLOG_SHIP: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// STAGE 0: running wire totals for the STATS heartbeat.
static TX_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static RX_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn wire_totals() -> (u64, u64) {
    (
        TX_BYTES.load(std::sync::atomic::Ordering::Relaxed),
        RX_BYTES.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// STAGE 0: arm/disarm the ship buffer (guests arm it on join, disarm
/// when the room ends). Arming resets the buffer.
pub fn slog_ship_arm(on: bool) {
    *SLOG_SHIP.lock().unwrap() = if on { Some(Vec::new()) } else { None };
}

/// STAGE 0: take everything buffered since the last flush.
pub fn slog_ship_drain() -> Vec<String> {
    match SLOG_SHIP.lock().unwrap().as_mut() {
        Some(buf) => std::mem::take(buf),
        None => Vec::new(),
    }
}

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
    let seq = SLOG_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut shipped = None;
    if let Some((f, t0)) = SLOG.lock().unwrap().as_mut() {
        use std::io::Write as _;
        let line = format!(
            "[#{seq:<5} {:9.3}s] {}",
            t0.elapsed().as_secs_f64(),
            msg.as_ref()
        );
        let _ = writeln!(f, "{line}");
        shipped = Some(line);
    }
    if let Some(line) = shipped
        && let Some(buf) = SLOG_SHIP.lock().unwrap().as_mut()
    {
        // Bounded: a flood keeps the OLDEST lines of the window (order
        // is the instrument) and marks the drop once.
        if buf.len() < 800 {
            buf.push(line);
        } else if buf.len() == 800 {
            buf.push("… ship buffer full — later lines dropped this window".into());
        }
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
/// STAGE 1: prefixed with the 8-byte LE sequence stamp.
const FRAME_CMDS: u8 = 2;
/// STAGE 1 (PSD-multiplayer-rescope): a stroke frame — payload[0] is
/// the subkind; dab batches are raw Pod bytes (48 B/dab), never JSON.
const FRAME_STROKE: u8 = 3;
const SUB_BEGIN: u8 = 0;
const SUB_DABS: u8 = 1;
const SUB_END: u8 = 2;
const SUB_RES: u8 = 3;
const SUB_HASHES: u8 = 4;
/// v0.2.2: audit repairs, binary + deflated — the first live test
/// showed cross-GPU blending drifts on EVERY stroke, so this path runs
/// constantly and JSON texels (~130KB/tile) were the remaining lag.
const SUB_REPAIR: u8 = 5;
/// Snapshots of real projects can be large; everything else is small.
const MAX_FRAME: u32 = 256 * 1024 * 1024;

fn write_frame(s: &mut TcpStream, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    TX_BYTES.fetch_add(
        5 + payload.len() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
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
    RX_BYTES.fetch_add(5 + len as u64, std::sync::atomic::Ordering::Relaxed);
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
        /// v0.2.4 (owner): the HOST's build — a joining guest compares
        /// and updates RIGHT AWAY, never "15 minutes until a press".
        #[serde(default)]
        version: String,
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
    /// STAGE 1: the snapshot that follows on this writer is the host's
    /// document as of this sequence stamp.
    SnapshotMeta {
        as_of_seq: u64,
    },
    /// STAGE 1 (audit): a guest found drifted tiles — wants exact texels.
    RepairRequest {
        drawing: u64,
        layer_name: String,
        coords: Vec<(i32, i32)>,
    },
    /// STAGE 1 (audit): authoritative texels for drifted tiles (host →
    /// one guest). Empty texels = the tile is gone.
    RepairTiles {
        drawing: u64,
        layer_name: String,
        tiles: Vec<(i32, i32, Vec<u16>)>,
    },
    /// A guest asking the host to undo/redo THEIR last step.
    UndoRequest {
        author: String,
        redo: bool,
    },
    /// STAGE 3: the host's sequenced verdict — every peer replays the
    /// same per-artist undo/redo on its identical history.
    Undone {
        author: String,
        redo: bool,
        seq: u64,
    },
    /// STAGE 4: the host's current arrangement (a serialized Workspace)
    /// — LOADABLE from Settings ▸ Layout on every guest, never forced.
    HostLayout {
        json: String,
    },
    /// v0.2.5 (owner: "after you join all the hosts Usable brushes work
    /// alongside your own installed ones"): the host's preset list,
    /// keys rewritten to content hashes whose images already rode the
    /// session res cache. Session-scoped guest-side, never saved.
    BrushBank {
        json: String,
    },
    /// STAGE 0 (PSD-multiplayer-rescope): a guest's numbered wire-log
    /// lines, shipped through the join — the ordered debug channel.
    DebugLog {
        lines: Vec<String>,
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

// ---------------------------------------------------------------------------
// STAGE 1: the stroke wire. Begin/End/Hashes are small JSON after the
// tag byte; dab batches are raw Pod bytes; resource images deflate.
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct StrokeBeginWire {
    pub author: String,
    pub stroke_id: u64,
    pub drawing: u64,
    pub layer_name: String,
    /// 0 = ink, 1 = erase, 2 = alpha-lock.
    pub mode: u8,
    pub opacity: f32,
    /// (content hash, w, h, GIH frames)
    pub tip: Option<(u64, u32, u32, u32)>,
    /// (content hash, w, h, scale, strength)
    pub grain: Option<(u64, u32, u32, f32, f32)>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct StrokeEndWire {
    pub author: String,
    pub stroke_id: u64,
    /// Host → peers: the global order stamp. Guest → host: 0.
    pub seq: u64,
    /// false = the stroke never landed (peers drop it; the seq is a
    /// sequenced no-op so the lane stays in step).
    pub ok: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct StrokeHashesWire {
    pub stroke_id: u64,
    pub seq: u64,
    pub drawing: u64,
    pub layer_name: String,
    /// The committed after-state per touched tile: (x, y, hash; 0 = gone).
    pub tiles: Vec<(i32, i32, u64)>,
}

/// STAGE 3: one command batch on the wire — who produced it, and (host
/// → peers) its place in the one order. Guests send seq 0; the host
/// stamps origin from the join name, never the payload.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CmdBatchWire {
    pub seq: u64,
    pub origin: String,
    pub cmds: Vec<anim_core::command::Command>,
}

pub enum StrokeFrame {
    Begin(StrokeBeginWire),
    Dabs {
        stroke_id: u64,
        dabs: Vec<crate::paint::Dab>,
    },
    End(StrokeEndWire),
    /// A content-addressed RGBA image (tip atlas / grain paper).
    Res {
        hash: u64,
        w: u32,
        h: u32,
        rgba: Vec<u8>,
    },
    Hashes(StrokeHashesWire),
    /// v0.2.2: authoritative texels for drifted tiles (empty = gone).
    Repair {
        drawing: u64,
        layer_name: String,
        tiles: Vec<(i32, i32, Vec<u16>)>,
    },
}

/// v0.2.2: header + one deflated texel blob (u16 LE, TILE_LEN each for
/// every non-empty tile, in entry order).
#[derive(serde::Serialize, serde::Deserialize)]
struct RepairHeader {
    drawing: u64,
    layer_name: String,
    /// (x, y, present) — absent tiles clear.
    entries: Vec<(i32, i32, bool)>,
}

pub fn enc_repair(drawing: u64, layer_name: &str, tiles: &[(i32, i32, Vec<u16>)]) -> Vec<u8> {
    use flate2::{Compression, write::DeflateEncoder};
    use std::io::Write as _;
    let header = RepairHeader {
        drawing,
        layer_name: layer_name.to_string(),
        entries: tiles.iter().map(|(x, y, t)| (*x, *y, !t.is_empty())).collect(),
    };
    let hj = serde_json::to_vec(&header).unwrap();
    let mut v = vec![SUB_REPAIR];
    v.extend((hj.len() as u32).to_le_bytes());
    v.extend(hj);
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::fast());
    for (_, _, t) in tiles.iter().filter(|(_, _, t)| !t.is_empty()) {
        let _ = enc.write_all(bytemuck::cast_slice(t));
    }
    v.extend(enc.finish().unwrap_or_default());
    v
}

fn dec_repair(rest: &[u8]) -> Option<StrokeFrame> {
    use std::io::Read as _;
    if rest.len() < 4 {
        return None;
    }
    let hl = u32::from_le_bytes(rest[..4].try_into().ok()?) as usize;
    let header: RepairHeader = serde_json::from_slice(rest.get(4..4 + hl)?).ok()?;
    let mut texels = Vec::new();
    let mut dec = flate2::read::DeflateDecoder::new(rest.get(4 + hl..)?);
    dec.read_to_end(&mut texels).ok()?;
    let tile_bytes = anim_core::raster::TILE_LEN * 2;
    let present = header.entries.iter().filter(|(_, _, p)| *p).count();
    if texels.len() != present * tile_bytes {
        return None;
    }
    let mut off = 0usize;
    let mut tiles = Vec::with_capacity(header.entries.len());
    for (x, y, p) in header.entries {
        if p {
            let t: Vec<u16> = bytemuck::pod_collect_to_vec(&texels[off..off + tile_bytes]);
            off += tile_bytes;
            tiles.push((x, y, t));
        } else {
            tiles.push((x, y, Vec::new()));
        }
    }
    Some(StrokeFrame::Repair {
        drawing: header.drawing,
        layer_name: header.layer_name,
        tiles,
    })
}

pub fn enc_begin(b: &StrokeBeginWire) -> Vec<u8> {
    let mut v = vec![SUB_BEGIN];
    v.extend(serde_json::to_vec(b).unwrap());
    v
}

pub fn enc_dabs(stroke_id: u64, dabs: &[crate::paint::Dab]) -> Vec<u8> {
    let mut v =
        Vec::with_capacity(9 + std::mem::size_of_val(dabs));
    v.push(SUB_DABS);
    v.extend(stroke_id.to_le_bytes());
    v.extend_from_slice(bytemuck::cast_slice(dabs));
    v
}

pub fn enc_end(e: &StrokeEndWire) -> Vec<u8> {
    let mut v = vec![SUB_END];
    v.extend(serde_json::to_vec(e).unwrap());
    v
}

pub fn enc_res(hash: u64, w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    use flate2::{Compression, write::DeflateEncoder};
    use std::io::Write as _;
    let mut v = vec![SUB_RES];
    v.extend(hash.to_le_bytes());
    v.extend(w.to_le_bytes());
    v.extend(h.to_le_bytes());
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::fast());
    let _ = enc.write_all(rgba);
    v.extend(enc.finish().unwrap_or_else(|_| rgba.to_vec()));
    v
}

pub fn enc_hashes(hw: &StrokeHashesWire) -> Vec<u8> {
    let mut v = vec![SUB_HASHES];
    v.extend(serde_json::to_vec(hw).unwrap());
    v
}

pub fn dec_stroke(payload: &[u8]) -> Option<StrokeFrame> {
    let (&sub, rest) = payload.split_first()?;
    match sub {
        SUB_BEGIN => serde_json::from_slice(rest).ok().map(StrokeFrame::Begin),
        SUB_DABS => {
            if rest.len() < 8 {
                return None;
            }
            let stroke_id = u64::from_le_bytes(rest[..8].try_into().ok()?);
            let body = &rest[8..];
            if body.len() % std::mem::size_of::<crate::paint::Dab>() != 0 {
                return None;
            }
            Some(StrokeFrame::Dabs {
                stroke_id,
                dabs: bytemuck::pod_collect_to_vec(body),
            })
        }
        SUB_END => serde_json::from_slice(rest).ok().map(StrokeFrame::End),
        SUB_RES => {
            if rest.len() < 16 {
                return None;
            }
            let hash = u64::from_le_bytes(rest[..8].try_into().ok()?);
            let w = u32::from_le_bytes(rest[8..12].try_into().ok()?);
            let h = u32::from_le_bytes(rest[12..16].try_into().ok()?);
            use std::io::Read as _;
            let mut rgba = Vec::new();
            let mut dec = flate2::read::DeflateDecoder::new(&rest[16..]);
            dec.read_to_end(&mut rgba).ok()?;
            if rgba.len() != (w as usize) * (h as usize) * 4 {
                return None;
            }
            Some(StrokeFrame::Res { hash, w, h, rgba })
        }
        SUB_HASHES => serde_json::from_slice(rest).ok().map(StrokeFrame::Hashes),
        SUB_REPAIR => dec_repair(rest),
        _ => None,
    }
}

/// SESSION PERF 3: what a writer thread carries. Raw frames go as-is;
/// Json is serialized ON THE WRITER (EditTiles' tile payloads are the
/// expensive ones — never on the UI thread).
enum Out {
    Raw(u8, Vec<u8>),
    Json(Box<Msg>),
    /// Serialized ON THE WRITER (tile payloads are the expensive part).
    Cmds(Box<CmdBatchWire>),
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
                Out::Cmds(mut batch) => {
                    // v0.2.1 (audit, LAW 1): PaintTiles BEFORE-texels never
                    // ride the wire — every peer holds the identical
                    // document and rebuilds them locally (the same trick
                    // apply_remote_tiles always used). Then the whole
                    // batch deflates: fills/clears carry f16 texels that
                    // JSON bloats ~10x and deflate crushes back.
                    for cmd in &mut batch.cmds {
                        if let anim_core::command::Command::PaintTiles { diff, .. } = cmd {
                            for (_, before, _) in diff.iter_mut() {
                                *before = None;
                            }
                        }
                    }
                    match serde_json::to_vec(&*batch) {
                        Ok(json) => {
                            use flate2::{Compression, write::DeflateEncoder};
                            use std::io::Write as _;
                            let raw = json.len();
                            let mut enc =
                                DeflateEncoder::new(Vec::new(), Compression::fast());
                            let _ = enc.write_all(&json);
                            let bytes = enc.finish().unwrap_or(json);
                            if raw > 64 * 1024 {
                                slog(format!(
                                    "OUT cmds raw={raw} deflated={} seq={}",
                                    bytes.len(),
                                    batch.seq
                                ));
                            }
                            (
                                write_frame(&mut stream, FRAME_CMDS, &bytes).is_ok(),
                                FRAME_CMDS,
                                bytes.len(),
                            )
                        }
                        Err(_) => (true, FRAME_CMDS, 0),
                    }
                }
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
    /// LEGACY (pre-rescope builds only): a guest's finished stroke as
    /// tiles. Stage 1 refuses it with an update hint — the payload is
    /// never read again.
    EditTiles {
        author: String,
        #[allow(dead_code)]
        drawing: u64,
        #[allow(dead_code)]
        layer_name: String,
        #[allow(dead_code)]
        tiles: Vec<(i32, i32, Vec<u16>)>,
    },
    /// Host side: a guest asking to undo/redo their own last step.
    UndoRequest {
        author: String,
        redo: bool,
    },
    /// Guest side: the host refused this artist's edit, with why.
    EditRefused(String),
    /// Guest side: one sequenced batch from the host.
    Commands(CmdBatchWire),
    /// Host side: a guest's predicted command batch (origin = join name).
    GuestCmds {
        origin: String,
        cmds: Vec<anim_core::command::Command>,
    },
    /// Guest side: a sequenced per-artist undo/redo to replay.
    Undone {
        author: String,
        redo: bool,
        seq: u64,
    },
    /// Host side: a guest's mirror slipped — send THAT GUEST a fresh
    /// snapshot (v0.2.2: requester-only; a room-wide 26MB broadcast per
    /// resync was the first live test's biggest lag).
    ResyncNeeded {
        client: u64,
    },
    /// Host side: a stroke frame from one guest (author = join name).
    GuestStroke {
        client: u64,
        author: String,
        frame: StrokeFrame,
    },
    /// Guest side: a stroke frame from the host's broadcast.
    Stroke(StrokeFrame),
    /// Guest side: the next snapshot is the doc as of this seq.
    SnapshotMeta(u64),
    /// Host side: a guest's audit found drift — send exact tiles back.
    RepairRequest {
        client: u64,
        drawing: u64,
        layer_name: String,
        coords: Vec<(i32, i32)>,
    },
    /// Guest side: authoritative texels for drifted tiles.
    RepairTiles {
        drawing: u64,
        layer_name: String,
        tiles: Vec<(i32, i32, Vec<u16>)>,
    },
    /// Guest side: the host's loadable layout (Settings ▸ Layout).
    HostLayout(String),
    /// Guest side: the host's usable brushes (v0.2.5).
    BrushBank(String),
    /// Host side: a guest's numbered log lines (Stage 0 debug channel).
    DebugLog {
        from: String,
        lines: Vec<String>,
    },
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
        host_name: String,
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
                            let host_name = host_name.clone();
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
                                // v0.2.1 (audit): the join NAME is this
                                // artist's wire identity (echo-skip,
                                // per-artist undo, refusal routing) — a
                                // duplicate would silently corrupt all
                                // three. Refuse it at the door.
                                let taken = username.trim().is_empty()
                                    || username == host_name
                                    || clients
                                        .lock()
                                        .unwrap()
                                        .values()
                                        .any(|c| c.name == username);
                                if taken {
                                    let msg = serde_json::to_vec(&Msg::Refused {
                                        why: format!(
                                            "the artist name '{username}' is already in \
                                             this room — change yours in Settings › Session"
                                        ),
                                    })
                                    .unwrap();
                                    let _ = write_frame(&mut stream, FRAME_JSON, &msg);
                                    slog(format!("join refused: duplicate name '{username}'"));
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
                                let welcome = serde_json::to_vec(&Msg::Welcome {
                                    peer_id: id,
                                    peers,
                                    version: crate::update::CURRENT_VERSION.to_string(),
                                })
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
                                                    let _ = tx.send(NetEvent::ResyncNeeded {
                                                        client: id,
                                                    });
                                                    continue;
                                                }
                                                Ok(Msg::UndoRequest { author, redo }) => {
                                                    let _ = tx.send(NetEvent::UndoRequest {
                                                        author,
                                                        redo,
                                                    });
                                                    continue;
                                                }
                                                Ok(Msg::DebugLog { lines }) => {
                                                    // STAGE 0: numbered guest
                                                    // lines for the host's
                                                    // remote log file.
                                                    let _ = tx.send(NetEvent::DebugLog {
                                                        from: username.clone(),
                                                        lines,
                                                    });
                                                    continue;
                                                }
                                                Ok(Msg::RepairRequest {
                                                    drawing,
                                                    layer_name,
                                                    coords,
                                                }) => {
                                                    let _ = tx.send(NetEvent::RepairRequest {
                                                        client: id,
                                                        drawing,
                                                        layer_name,
                                                        coords,
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
                                        Ok((FRAME_CMDS, buf)) => {
                                            // STAGE 3: a guest's predicted
                                            // command batch — origin is the
                                            // JOIN name, never the payload.
                                            // v0.2.1: deflated on the wire.
                                            let json = {
                                                use std::io::Read as _;
                                                let mut out = Vec::new();
                                                let mut dec =
                                                    flate2::read::DeflateDecoder::new(
                                                        buf.as_slice(),
                                                    );
                                                match dec.read_to_end(&mut out) {
                                                    Ok(_) => out,
                                                    Err(_) => buf,
                                                }
                                            };
                                            if let Ok(b) =
                                                serde_json::from_slice::<CmdBatchWire>(&json)
                                            {
                                                let _ = tx.send(NetEvent::GuestCmds {
                                                    origin: username.clone(),
                                                    cmds: b.cmds,
                                                });
                                            } else {
                                                slog("IN guest cmds: PARSE FAILED");
                                            }
                                        }
                                        Ok((FRAME_STROKE, buf)) => {
                                            // STAGE 1: Begin (author
                                            // stamped from the join name),
                                            // Dabs and Res relay LIVE to
                                            // the other guests; End stays
                                            // here — the host replays and
                                            // sequences it.
                                            match dec_stroke(&buf) {
                                                Some(StrokeFrame::Begin(mut b)) => {
                                                    b.author = username.clone();
                                                    broadcast_except(
                                                        &clients,
                                                        id,
                                                        FRAME_STROKE,
                                                        &enc_begin(&b),
                                                    );
                                                    let _ = tx.send(NetEvent::GuestStroke {
                                                        client: id,
                                                        author: username.clone(),
                                                        frame: StrokeFrame::Begin(b),
                                                    });
                                                }
                                                Some(
                                                    frame @ (StrokeFrame::Dabs { .. }
                                                    | StrokeFrame::Res { .. }),
                                                ) => {
                                                    broadcast_except(
                                                        &clients,
                                                        id,
                                                        FRAME_STROKE,
                                                        &buf,
                                                    );
                                                    let _ = tx.send(NetEvent::GuestStroke {
                                                        client: id,
                                                        author: username.clone(),
                                                        frame,
                                                    });
                                                }
                                                Some(frame) => {
                                                    let _ = tx.send(NetEvent::GuestStroke {
                                                        client: id,
                                                        author: username.clone(),
                                                        frame,
                                                    });
                                                }
                                                None => {
                                                    slog("IN stroke frame: PARSE FAILED");
                                                }
                                            }
                                        }
                                        Ok(_) => {}
                                        Err(e) => {
                                            // v0.2.1 (audit): disconnects
                                            // were invisible in the log.
                                            slog(format!(
                                                "peer '{username}' (client {id}) \
                                                 disconnected: {e}"
                                            ));
                                            break;
                                        }
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

    /// v0.2.1 (audit): a refusal goes to the REFUSED ARTIST's connection
    /// only — broadcasting it made every guest flash the error and
    /// request a resync (a room-wide snapshot per refusal).
    pub fn send_refusal(&self, author: &str, why: &str) {
        let msg = serde_json::to_vec(&Msg::EditRefused {
            author: author.to_string(),
            why: why.to_string(),
        })
        .unwrap();
        let mut map = self.clients.lock().unwrap();
        for c in map.values_mut() {
            if c.name == author {
                let _ = c.tx.send(Out::Raw(FRAME_JSON, msg.clone()));
            }
        }
    }

    /// One sequenced batch (with its origin) to every guest.
    pub fn send_commands(&self, batch: CmdBatchWire) {
        let mut dead = Vec::new();
        {
            let mut map = self.clients.lock().unwrap();
            for (id, c) in map.iter_mut() {
                if c.tx.send(Out::Cmds(Box::new(batch.clone()))).is_err() {
                    dead.push(*id);
                }
            }
            for id in &dead {
                map.remove(id);
            }
        }
    }

    /// STAGE 3: a sequenced per-artist undo/redo, to every guest.
    pub fn send_undone(&self, author: &str, redo: bool, seq: u64) {
        let msg = serde_json::to_vec(&Msg::Undone {
            author: author.to_string(),
            redo,
            seq,
        })
        .unwrap();
        broadcast_except(&self.clients, u64::MAX, FRAME_JSON, &msg);
    }

    /// STAGE 1: one stroke frame (Begin/Dabs/End/Res/Hashes) to every
    /// guest.
    pub fn send_stroke_frame(&self, payload: Vec<u8>) {
        broadcast_except(&self.clients, u64::MAX, FRAME_STROKE, &payload);
    }

    /// STAGE 1 (audit): exact texels to ONE guest whose tiles drifted —
    /// v0.2.2: binary + deflated (this path runs per stroke when the
    /// GPUs disagree; JSON texels were the remaining lag).
    pub fn send_repair(
        &self,
        client: u64,
        drawing: u64,
        layer_name: &str,
        tiles: Vec<(i32, i32, Vec<u16>)>,
    ) {
        let payload = enc_repair(drawing, layer_name, &tiles);
        slog(format!(
            "REPAIR out client={client} tiles={} bytes={}",
            tiles.len(),
            payload.len()
        ));
        if let Some(c) = self.clients.lock().unwrap().get_mut(&client) {
            let _ = c.tx.send(Out::Raw(FRAME_STROKE, payload));
        }
    }

    /// v0.2.2: one stroke frame to ONE guest (join-time resource and
    /// layout refreshes are the joiner's business, not the room's).
    pub fn send_stroke_frame_to(&self, client: u64, payload: Vec<u8>) {
        if let Some(c) = self.clients.lock().unwrap().get_mut(&client) {
            let _ = c.tx.send(Out::Raw(FRAME_STROKE, payload));
        }
    }

    /// v0.2.2: the host's layout, to one joiner.
    pub fn send_layout_to(&self, client: u64, json: String) {
        if let Some(c) = self.clients.lock().unwrap().get_mut(&client) {
            let _ = c.tx.send(Out::Json(Box::new(Msg::HostLayout { json })));
        }
    }

    /// v0.2.5: the host's usable brushes, to one joiner (after the res
    /// images on the same writer — order is the contract).
    pub fn send_bank_to(&self, client: u64, json: String) {
        if let Some(c) = self.clients.lock().unwrap().get_mut(&client) {
            let _ = c.tx.send(Out::Json(Box::new(Msg::BrushBank { json })));
        }
    }

    /// The authoritative document — v0.2.2: to ONE requester, preceded
    /// by its sequence stamp on the same writer. Snapshots never go to
    /// the whole room again.
    pub fn send_snapshot_to(&self, client: u64, as_of_seq: u64, doc: &[u8]) {
        if let Some(c) = self.clients.lock().unwrap().get_mut(&client) {
            let meta = c
                .tx
                .send(Out::Json(Box::new(Msg::SnapshotMeta { as_of_seq })));
            if meta.is_ok() {
                let _ = c.tx.send(Out::Raw(FRAME_DOC, doc.to_vec()));
            }
        }
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
    /// v0.2.4: the host's build, from the Welcome ("" = pre-0.2.4 host).
    pub host_version: String,
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
        let (peer_id, host_version) = match (k == FRAME_JSON)
            .then(|| serde_json::from_slice::<Msg>(&buf).ok())
            .flatten()
        {
            Some(Msg::Welcome {
                peer_id,
                peers,
                version,
            }) => {
                let _ = peers;
                (peer_id, version)
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
                        // v0.2.1: the wire form is deflated JSON.
                        slog(format!("IN cmds bytes={}", bytes.len()));
                        let json = {
                            use std::io::Read as _;
                            let mut out = Vec::new();
                            let mut dec =
                                flate2::read::DeflateDecoder::new(bytes.as_slice());
                            match dec.read_to_end(&mut out) {
                                Ok(_) => out,
                                Err(_) => bytes, // pre-0.2.1 peer sent raw
                            }
                        };
                        if let Ok(batch) = serde_json::from_slice::<CmdBatchWire>(&json) {
                            let _ = tx.send(NetEvent::Commands(batch));
                        } else {
                            slog("IN cmds: PARSE FAILED");
                        }
                    }
                    Ok((FRAME_STROKE, bytes)) => {
                        // STAGE 1: the host's stroke broadcast.
                        match dec_stroke(&bytes) {
                            Some(frame) => {
                                let _ = tx.send(NetEvent::Stroke(frame));
                            }
                            None => slog("IN stroke frame: PARSE FAILED"),
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
                        Ok(Msg::SnapshotMeta { as_of_seq }) => {
                            let _ = tx.send(NetEvent::SnapshotMeta(as_of_seq));
                        }
                        Ok(Msg::Undone { author, redo, seq }) => {
                            let _ = tx.send(NetEvent::Undone { author, redo, seq });
                        }
                        Ok(Msg::HostLayout { json }) => {
                            let _ = tx.send(NetEvent::HostLayout(json));
                        }
                        Ok(Msg::BrushBank { json }) => {
                            let _ = tx.send(NetEvent::BrushBank(json));
                        }
                        Ok(Msg::RepairTiles {
                            drawing,
                            layer_name,
                            tiles,
                        }) => {
                            let _ = tx.send(NetEvent::RepairTiles {
                                drawing,
                                layer_name,
                                tiles,
                            });
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
            host_version,
        })
    }

    /// Tell the host our mirror slipped.
    pub fn send_resync_request(&mut self) {
        let _ = self.tx.send(Out::Json(Box::new(Msg::ResyncRequest {})));
    }

    /// STAGE 0: ship this machine's buffered log lines to the host.
    pub fn send_debug(&mut self, lines: Vec<String>) {
        let _ = self.tx.send(Out::Json(Box::new(Msg::DebugLog { lines })));
    }

    /// STAGE 1: one stroke frame (Begin/Dabs/End/Res) to the host.
    pub fn send_stroke_frame(&mut self, payload: Vec<u8>) {
        let _ = self.tx.send(Out::Raw(FRAME_STROKE, payload));
    }

    /// STAGE 3: this guest's predicted command batch, to the host.
    pub fn send_cmds(&mut self, origin: &str, cmds: Vec<anim_core::command::Command>) {
        let _ = self.tx.send(Out::Cmds(Box::new(CmdBatchWire {
            seq: 0,
            origin: origin.to_string(),
            cmds,
        })));
    }

    /// STAGE 1 (audit): ask the host for exact texels of drifted tiles.
    pub fn send_repair_request(
        &mut self,
        drawing: u64,
        layer_name: &str,
        coords: Vec<(i32, i32)>,
    ) {
        let _ = self.tx.send(Out::Json(Box::new(Msg::RepairRequest {
            drawing,
            layer_name: layer_name.to_string(),
            coords,
        })));
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
        let host = Host::start(
            0,
            key.clone(),
            secret.clone(),
            "test room".into(),
            false,
            "the-host".into(),
        )
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
        // STAGE 0 (PSD-multiplayer-rescope): a guest's numbered log lines
        // reach the host's UI events, in order, attributed by join name.
        guest.send_debug(vec!["[#0] first".into(), "[#1] second".into()]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut got = false;
        while std::time::Instant::now() < deadline {
            if let Ok(NetEvent::DebugLog { from, lines }) = host.events.try_recv() {
                assert_eq!(from, "guest-a");
                assert_eq!(lines, vec!["[#0] first".to_string(), "[#1] second".into()]);
                got = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(got, "guest debug lines must reach the host ordered");
        // STAGE 1: a guest's Begin reaches the host (author = the JOIN
        // name, never the payload's claim) AND relays live to the other
        // guest with that author stamped.
        guest.send_stroke_frame(enc_begin(&StrokeBeginWire {
            author: "liar".into(),
            stroke_id: 5,
            drawing: 1,
            layer_name: "genga".into(),
            mode: 0,
            opacity: 1.0,
            tip: None,
            grain: None,
        }));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let (mut host_got, mut relay_got) = (false, false);
        while std::time::Instant::now() < deadline && !(host_got && relay_got) {
            if let Ok(NetEvent::GuestStroke {
                author,
                frame: StrokeFrame::Begin(b),
                ..
            }) = host.events.try_recv()
            {
                assert_eq!(author, "guest-a");
                assert_eq!(b.author, "guest-a", "the join name must overwrite the claim");
                assert_eq!(b.stroke_id, 5);
                host_got = true;
            }
            if let Ok(NetEvent::Stroke(StrokeFrame::Begin(b))) = second.events.try_recv() {
                assert_eq!(b.author, "guest-a");
                relay_got = true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(host_got, "the host must see the guest's Begin");
        assert!(relay_got, "the other guest must see the relayed Begin");
    }

    #[test]
    fn repair_wire_roundtrips() {
        // v0.2.2: binary + deflated repairs — texels bit-exact, empty
        // markers survive, garbage refuses.
        let full: Vec<u16> = (0..anim_core::raster::TILE_LEN as u32)
            .map(|i| (i * 31 % 65521) as u16)
            .collect();
        let tiles = vec![(2, -3, full.clone()), (4, 5, Vec::new()), (0, 0, full.clone())];
        match dec_stroke(&enc_repair(7, "line", &tiles)) {
            Some(StrokeFrame::Repair {
                drawing: 7,
                layer_name,
                tiles: t,
            }) => {
                assert_eq!(layer_name, "line");
                assert_eq!(t.len(), 3);
                assert_eq!(t[0], (2, -3, full.clone()));
                assert_eq!(t[1], (4, 5, Vec::new()));
                assert_eq!(t[2], (0, 0, full));
            }
            _ => panic!("repair must roundtrip"),
        }
        assert!(dec_repair(&[1, 2, 3]).is_none(), "garbage refuses");
    }

    #[test]
    fn stroke_wire_roundtrips() {
        // Begin: JSON after the tag byte.
        let b = StrokeBeginWire {
            author: "ami".into(),
            stroke_id: 7,
            drawing: 3,
            layer_name: "genga".into(),
            mode: 1,
            opacity: 0.5,
            tip: Some((0xabc, 256, 256, 4)),
            grain: None,
        };
        match dec_stroke(&enc_begin(&b)) {
            Some(StrokeFrame::Begin(d)) => {
                assert_eq!(d.author, "ami");
                assert_eq!(d.stroke_id, 7);
                assert_eq!(d.mode, 1);
                assert_eq!(d.tip, Some((0xabc, 256, 256, 4)));
            }
            _ => panic!("begin must roundtrip"),
        }
        // Dabs: raw Pod bytes — BIT-exact both ways (the determinism
        // law's wire form: same dabs in, same dabs out, no JSON float
        // laundering).
        let dabs = vec![crate::paint::Dab {
            center: [1.5, -2.25],
            radius: 3.75,
            hardness: 0.85,
            color: [0.1, 0.2, 0.3, 0.4],
            dir: [1.0, 0.0],
            aspect: 1.0,
            tip: 0.0,
        }];
        match dec_stroke(&enc_dabs(9, &dabs)) {
            Some(StrokeFrame::Dabs { stroke_id, dabs: d }) => {
                assert_eq!(stroke_id, 9);
                assert_eq!(d.len(), 1);
                assert_eq!(d[0].center, [1.5, -2.25]);
                assert_eq!(d[0].color, [0.1, 0.2, 0.3, 0.4]);
            }
            _ => panic!("dabs must roundtrip"),
        }
        // Res: deflated image, size-checked on arrival.
        let rgba: Vec<u8> = (0..16 * 16 * 4).map(|i| (i % 251) as u8).collect();
        match dec_stroke(&enc_res(42, 16, 16, &rgba)) {
            Some(StrokeFrame::Res {
                hash: 42,
                w: 16,
                h: 16,
                rgba: r,
            }) => assert_eq!(r, rgba),
            _ => panic!("res must roundtrip (deflate + size check)"),
        }
        match dec_stroke(&enc_end(&StrokeEndWire {
            author: "ami".into(),
            stroke_id: 7,
            seq: 12,
            ok: true,
        })) {
            Some(StrokeFrame::End(e)) => {
                assert_eq!(e.seq, 12);
                assert!(e.ok);
            }
            _ => panic!("end must roundtrip"),
        }
        match dec_stroke(&enc_hashes(&StrokeHashesWire {
            stroke_id: 7,
            seq: 12,
            drawing: 3,
            layer_name: "genga".into(),
            tiles: vec![(0, -1, 99)],
        })) {
            Some(StrokeFrame::Hashes(h)) => assert_eq!(h.tiles, vec![(0, -1, 99)]),
            _ => panic!("hashes must roundtrip"),
        }
    }
}
