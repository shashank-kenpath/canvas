//! The live connection to a board's room in a browser: the page's WebSocket,
//! reconnecting after drops. Same interface as the native `room.rs`.
//!
//! Browsers cannot send an Authorization header with a WebSocket, so the
//! session token travels as the subprotocol `bearer.<token>` next to
//! `inkboard`, which the server picks.

use super::http::call;
use crate::import::ImageAsset;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::{Rc, Weak};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, MessageEvent, WebSocket};

/// Images by their SHA-256 name, shared between the app and the room.
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

struct Inner {
    base: String,
    token: String,
    board: String,
    images: Images,
    wake: Arc<dyn Fn() + Send + Sync>,
    events: Sender<RoomEvent>,
    ws: RefCell<Option<WebSocket>>,
    /// Handlers of the current socket, kept alive while it is open.
    handlers: RefCell<Vec<Closure<dyn FnMut(JsValue)>>>,
    stopped: Cell<bool>,
    open: Cell<bool>,
    backoff: Cell<u32>,
    last_heard: Cell<f64>,
    last_ping: Cell<f64>,
    outgoing: RefCell<VecDeque<Cmd>>,
    sending: Cell<bool>,
    incoming: RefCell<VecDeque<Value>>,
    receiving: Cell<bool>,
}

pub struct Room {
    pub events: Receiver<RoomEvent>,
    inner: Rc<Inner>,
}

impl Drop for Room {
    fn drop(&mut self) {
        self.inner.stopped.set(true);
        if let Some(ws) = self.inner.ws.borrow_mut().take() {
            let _ = ws.close();
        }
        self.inner.handlers.borrow_mut().clear();
    }
}

impl Room {
    pub fn send(&self, msg: Value) {
        self.queue(Cmd::Send(msg));
    }

    pub(crate) fn send_with_images(&self, msg: Value, images: Vec<Arc<ImageAsset>>) {
        self.queue(if images.is_empty() { Cmd::Send(msg) } else { Cmd::SendWithImages(msg, images) });
    }

    fn queue(&self, cmd: Cmd) {
        // Commands sent while offline are dropped; the app resends
        // unconfirmed changes when it says hello again.
        if !self.inner.open.get() {
            return;
        }
        self.inner.outgoing.borrow_mut().push_back(cmd);
        drain_outgoing(&self.inner);
    }
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn emit(inner: &Inner, e: RoomEvent) {
    if inner.events.send(e).is_ok() {
        (inner.wake)();
    }
}

pub(crate) fn open(base: String, token: String, board: String, images: Images, wake: Arc<dyn Fn() + Send + Sync>) -> Room {
    let (tx, events) = mpsc::channel();
    let inner = Rc::new(Inner {
        base,
        token,
        board,
        images,
        wake,
        events: tx,
        ws: RefCell::new(None),
        handlers: RefCell::new(Vec::new()),
        stopped: Cell::new(false),
        open: Cell::new(false),
        backoff: Cell::new(1),
        last_heard: Cell::new(now()),
        last_ping: Cell::new(now()),
        outgoing: RefCell::new(VecDeque::new()),
        sending: Cell::new(false),
        incoming: RefCell::new(VecDeque::new()),
        receiving: Cell::new(false),
    });
    connect(&inner);
    keep_alive(Rc::downgrade(&inner));
    Room { events, inner }
}

fn socket_url(base: &str, board: &str) -> String {
    let base = base.trim_end_matches('/');
    let ws = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    };
    format!("{ws}/api/boards/{board}/socket")
}

fn connect(inner: &Rc<Inner>) {
    if inner.stopped.get() {
        return;
    }
    let protocols = js_sys::Array::of2(&"inkboard".into(), &format!("bearer.{}", inner.token).into());
    let ws = match WebSocket::new_with_str_sequence(&socket_url(&inner.base, &inner.board), &protocols) {
        Ok(ws) => ws,
        Err(_) => {
            emit(inner, RoomEvent::Offline("Could not open a connection".into()));
            retry_later(inner);
            return;
        }
    };
    let mut handlers = Vec::new();

    let weak = Rc::downgrade(inner);
    let on_open = Closure::<dyn FnMut(JsValue)>::new(move |_| {
        let Some(inner) = weak.upgrade() else { return };
        inner.open.set(true);
        inner.backoff.set(1);
        inner.last_heard.set(now());
        emit(&inner, RoomEvent::Online);
    });
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    handlers.push(on_open);

    let weak = Rc::downgrade(inner);
    let on_message = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
        let Some(inner) = weak.upgrade() else { return };
        inner.last_heard.set(now());
        let Some(text) = e.unchecked_into::<MessageEvent>().data().as_string() else { return };
        let Ok(msg) = serde_json::from_str::<Value>(&text) else { return };
        if msg["t"] == "pong" {
            return;
        }
        inner.incoming.borrow_mut().push_back(msg);
        drain_incoming(&inner);
    });
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    handlers.push(on_message);

    let weak = Rc::downgrade(inner);
    let on_close = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
        let Some(inner) = weak.upgrade() else { return };
        let e: CloseEvent = e.unchecked_into();
        let was_open = inner.open.replace(false);
        inner.ws.borrow_mut().take();
        inner.outgoing.borrow_mut().clear();
        if inner.stopped.get() {
            return;
        }
        if e.code() == 4004 {
            emit(&inner, RoomEvent::Refused { status: 404, message: "This board was deleted".into() });
            return;
        }
        if was_open {
            // 4001: access changed; reconnect to learn the new role.
            let reason = e.reason();
            emit(&inner, RoomEvent::Offline(if reason.is_empty() { "Closed".into() } else { reason }));
            retry_later(&inner);
        } else {
            // The browser hides why a handshake failed; ask the API instead.
            wasm_bindgen_futures::spawn_local(why_refused(inner));
        }
    });
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    handlers.push(on_close);

    *inner.handlers.borrow_mut() = handlers;
    *inner.ws.borrow_mut() = Some(ws);
}

async fn why_refused(inner: Rc<Inner>) {
    match call(&inner.base, "GET", &format!("/api/boards/{}", inner.board), Some(&inner.token), None).await {
        Ok((status, body)) if matches!(status, 401 | 403 | 404 | 410) => {
            let message = serde_json::from_slice::<Value>(&body).ok().and_then(|v| v["error"].as_str().map(str::to_string))
                .unwrap_or_else(|| format!("The server answered {status}"));
            emit(&inner, RoomEvent::Refused { status, message });
        }
        Ok(_) => {
            emit(&inner, RoomEvent::Offline("Could not connect to the board".into()));
            retry_later(&inner);
        }
        Err(e) => {
            emit(&inner, RoomEvent::Offline(e.message));
            retry_later(&inner);
        }
    }
}

fn retry_later(inner: &Rc<Inner>) {
    let delay = inner.backoff.get();
    inner.backoff.set((delay * 2).min(30));
    let weak = Rc::downgrade(inner);
    wasm_bindgen_futures::spawn_local(async move {
        super::sleep(delay * 1000).await;
        if let Some(inner) = weak.upgrade() {
            connect(&inner);
        }
    });
}

/// Pings every 20 s and gives up on a socket that has gone quiet for 50 s.
fn keep_alive(weak: Weak<Inner>) {
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            super::sleep(5000).await;
            let Some(inner) = weak.upgrade() else { return };
            if inner.stopped.get() {
                return;
            }
            if !inner.open.get() {
                continue;
            }
            let t = now();
            if t - inner.last_heard.get() > 50_000.0 {
                if let Some(ws) = inner.ws.borrow().as_ref() {
                    let _ = ws.close_with_code_and_reason(4000, "The server stopped answering");
                }
            } else if t - inner.last_ping.get() > 20_000.0 {
                inner.last_ping.set(t);
                if let Some(ws) = inner.ws.borrow().as_ref() {
                    let _ = ws.send_with_str(r#"{"t":"ping"}"#);
                }
            }
        }
    });
}

/// Sends queued commands in order, uploading their images first.
fn drain_outgoing(inner: &Rc<Inner>) {
    if inner.sending.replace(true) {
        return;
    }
    let inner = inner.clone();
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            let Some(cmd) = inner.outgoing.borrow_mut().pop_front() else { break };
            let msg = match cmd {
                Cmd::Send(msg) => msg,
                Cmd::SendWithImages(msg, list) => {
                    let mut failed = None;
                    for image in list {
                        if let Err(e) = upload(&inner.base, &inner.token, &image).await {
                            failed = Some(e);
                            break;
                        }
                    }
                    if let Some(e) = failed {
                        // Drop the connection; the app resends after reconnecting.
                        if let Some(ws) = inner.ws.borrow().as_ref() {
                            let _ = ws.close_with_code_and_reason(4000, &e);
                        }
                        inner.outgoing.borrow_mut().clear();
                        break;
                    }
                    msg
                }
            };
            if let Some(ws) = inner.ws.borrow().as_ref() {
                let _ = ws.send_with_str(&msg.to_string());
            }
        }
        inner.sending.set(false);
    });
}

/// Hands messages to the app in order, after fetching images they need.
fn drain_incoming(inner: &Rc<Inner>) {
    if inner.receiving.replace(true) {
        return;
    }
    let inner = inner.clone();
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            let Some(msg) = inner.incoming.borrow_mut().pop_front() else { break };
            fetch_images(&inner.base, &inner.token, &inner.images, &msg).await;
            if inner.stopped.get() {
                break;
            }
            emit(&inner, RoomEvent::Message(msg));
        }
        inner.receiving.set(false);
    });
}

async fn upload(base: &str, token: &str, image: &ImageAsset) -> Result<(), String> {
    let bytes = image.bytes();
    let ctype = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
    let path = format!("/api/assets/{}", image.hash_name());
    match call(base, "PUT", &path, Some(token), Some((ctype, bytes))).await {
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
async fn fetch_images(base: &str, token: &str, images: &Images, msg: &Value) {
    for name in image_refs(msg) {
        if name.len() != 64 || images.lock().map(|m| m.contains_key(&name)).unwrap_or(true) {
            continue;
        }
        if let Ok((200, bytes)) = call(base, "GET", &format!("/api/assets/{name}"), Some(token), None).await {
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
