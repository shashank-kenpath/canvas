//! The live connection to a board's room: a WebSocket kept open on its own
//! thread, reconnecting after drops. Images travel separately through the
//! server's asset store and are kept in a shared cache.

use super::http::{call, Origin};
use crate::import::ImageAsset;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::HandshakeError;
use tungstenite::{Error as WsError, Message};

/// Images by their SHA-256 name, shared between the app and room threads.
pub(crate) type Images = Arc<Mutex<HashMap<String, Arc<ImageAsset>>>>;

pub enum RoomEvent {
    /// Connected; the app should say hello.
    Online,
    /// The connection dropped and will be retried.
    Offline(String),
    /// The room will not take this device back (no access, deleted, signed out).
    Refused { status: u16, message: String },
    Message(Value),
}

enum Cmd {
    Send(Value),
    /// Upload these images, then send the message that refers to them.
    SendWithImages(Value, Vec<Arc<ImageAsset>>),
}

pub struct Room {
    cmds: Sender<Cmd>,
    pub events: Receiver<RoomEvent>,
    stop: Arc<AtomicBool>,
}

impl Drop for Room {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Room {
    pub fn send(&self, msg: Value) {
        let _ = self.cmds.send(Cmd::Send(msg));
    }

    pub(crate) fn send_with_images(&self, msg: Value, images: Vec<Arc<ImageAsset>>) {
        let _ = self.cmds.send(if images.is_empty() { Cmd::Send(msg) } else { Cmd::SendWithImages(msg, images) });
    }
}

pub(crate) fn open(base: String, token: String, board: String, images: Images, wake: Arc<dyn Fn() + Send + Sync>) -> Room {
    let (cmds, cmd_rx) = mpsc::channel();
    let (ev_tx, events) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let _ = std::thread::Builder::new().name("board-room".into()).spawn(move || {
        let mut backoff = 1u64;
        let emit = |e: RoomEvent| {
            let _ = ev_tx.send(e);
            wake();
        };
        while !flag.load(Ordering::SeqCst) {
            match session(&base, &token, &board, &images, &cmd_rx, &flag, &emit) {
                Ok(()) => return,
                Err(End::Refused(status, message)) => {
                    emit(RoomEvent::Refused { status, message });
                    return;
                }
                Err(End::Dropped(why, was_open)) => {
                    emit(RoomEvent::Offline(why));
                    if was_open {
                        backoff = 1;
                    }
                }
            }
            let until = Instant::now() + Duration::from_secs(backoff);
            while Instant::now() < until {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                // Commands sent while offline are dropped; the app resends
                // unconfirmed changes when it says hello again.
                while cmd_rx.try_recv().is_ok() {}
                std::thread::sleep(Duration::from_millis(100));
            }
            backoff = (backoff * 2).min(30);
        }
    });
    Room { cmds, events, stop }
}

enum End {
    Refused(u16, String),
    /// Reason, and whether the socket had been open.
    Dropped(String, bool),
}

fn socket_url(base: &str, board: &str) -> Result<String, String> {
    let o = Origin::parse(base)?;
    Ok(format!("{}://{}/api/boards/{board}/socket", if o.tls { "wss" } else { "ws" }, o.authority()))
}

fn session(
    base: &str,
    token: &str,
    board: &str,
    images: &Images,
    cmds: &Receiver<Cmd>,
    stop: &AtomicBool,
    emit: &dyn Fn(RoomEvent),
) -> Result<(), End> {
    let origin = Origin::parse(base).map_err(|e| End::Refused(0, e))?;
    let (stream, tcp) = origin.connect(Duration::from_secs(10)).map_err(|e| End::Dropped(e, false))?;
    let mut req = socket_url(base, board).map_err(|e| End::Refused(0, e))?.into_client_request().map_err(|e| End::Refused(0, e.to_string()))?;
    req.headers_mut().insert("authorization", format!("Bearer {token}").parse().map_err(|_| End::Refused(401, "Bad sign-in".into()))?);
    req.headers_mut().insert("user-agent", "Inkboard".parse().unwrap());
    let mut ws = match tungstenite::client::client(req, stream) {
        Ok((ws, _)) => ws,
        Err(HandshakeError::Failure(WsError::Http(resp))) => {
            let status = resp.status().as_u16();
            let body = resp.body().as_deref().unwrap_or_default();
            let message = serde_json::from_slice::<Value>(body).ok().and_then(|v| v["error"].as_str().map(str::to_string))
                .unwrap_or_else(|| format!("The server answered {status}"));
            return Err(if matches!(status, 401 | 403 | 404 | 410) { End::Refused(status, message) } else { End::Dropped(message, false) });
        }
        Err(e) => return Err(End::Dropped(e.to_string(), false)),
    };
    let _ = tcp.set_read_timeout(Some(Duration::from_millis(40)));
    emit(RoomEvent::Online);
    let mut last_heard = Instant::now();
    let mut last_ping = Instant::now();
    let dropped = |e: WsError| End::Dropped(e.to_string(), true);
    loop {
        if stop.load(Ordering::SeqCst) {
            let _ = ws.close(None);
            let _ = ws.flush();
            return Ok(());
        }
        loop {
            match cmds.try_recv() {
                Ok(Cmd::Send(msg)) => ws.send(Message::Text(msg.to_string().into())).map_err(dropped)?,
                Ok(Cmd::SendWithImages(msg, list)) => {
                    for image in list {
                        upload(base, token, &image).map_err(|e| End::Dropped(e, true))?;
                    }
                    ws.send(Message::Text(msg.to_string().into())).map_err(dropped)?;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if last_ping.elapsed() > Duration::from_secs(20) {
            ws.send(Message::Text(r#"{"t":"ping"}"#.into())).map_err(dropped)?;
            last_ping = Instant::now();
        }
        if last_heard.elapsed() > Duration::from_secs(50) {
            return Err(End::Dropped("The server stopped answering".into(), true));
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                last_heard = Instant::now();
                let Ok(msg) = serde_json::from_str::<Value>(text.as_str()) else { continue };
                if msg["t"] == "pong" {
                    continue;
                }
                fetch_images(base, token, images, &msg);
                emit(RoomEvent::Message(msg));
            }
            Ok(Message::Close(frame)) => {
                let (code, reason) = frame.map(|f| (u16::from(f.code), f.reason.to_string())).unwrap_or((1005, String::new()));
                // 4001: access changed, reconnect to learn the new role. 4004: board deleted.
                return Err(match code {
                    4004 => End::Refused(404, "This board was deleted".into()),
                    _ => End::Dropped(if reason.is_empty() { "Closed".into() } else { reason }, true),
                });
            }
            Ok(_) => last_heard = Instant::now(),
            Err(WsError::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(dropped(e)),
        }
    }
}

fn upload(base: &str, token: &str, image: &ImageAsset) -> Result<(), String> {
    let bytes = image.bytes();
    let ctype = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
    let path = format!("/api/assets/{}", image.hash_name());
    match call(base, "PUT", &path, Some(token), Some((ctype, bytes)), Duration::from_secs(60)) {
        Ok((status, _)) if (200..300).contains(&status) => Ok(()),
        Ok((status, _)) => Err(format!("Image upload failed ({status})")),
        Err(e) => Err(e.message),
    }
}

/// Image names (`imageRef`) used by the shapes in a room message.
pub fn image_refs(msg: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for item in msg["items"].as_array().into_iter().flatten() {
        if let Some(r) = item["s"]["shape"]["imageRef"].as_str() {
            out.push(r.to_string());
        }
    }
    out
}

/// Downloads images the message needs that are not cached yet.
fn fetch_images(base: &str, token: &str, images: &Images, msg: &Value) {
    for name in image_refs(msg) {
        if name.len() != 64 || images.lock().map(|m| m.contains_key(&name)).unwrap_or(true) {
            continue;
        }
        if let Ok((200, bytes)) = call(base, "GET", &format!("/api/assets/{name}"), Some(token), None, Duration::from_secs(60)) {
            if let Ok(asset) = ImageAsset::from_bytes(bytes) {
                if asset.hash_name() == name {
                    if let Ok(mut m) = images.lock() {
                        m.insert(name, Arc::new(asset));
                    }
                }
            }
        }
    }
}
