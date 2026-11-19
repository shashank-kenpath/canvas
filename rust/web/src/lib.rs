//! Inkboard in a browser. The whole app (library, editor, toolbars, team
//! boards with live collaboration) is `canvas_core`, the same code the
//! Android and desktop apps run; this host is the glue the page drives:
//! it draws frames into memory the page copies to a `<canvas>`, takes
//! pointer, wheel and keyboard input, and queues the app's requests (show a
//! text field, copy, download a picture, pick a file) for the page.
//!
//! Files live in memory (`canvas_core::sys::fs`); the page restores them
//! from IndexedDB before starting and saves [`take_changes`] after frames.

mod text;

use canvas_core::import::{FileKind, PickerFile};
use canvas_core::ui::app::{App, Phase, Platform, Sample, Shortcut};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use wasm_bindgen::prelude::*;

/// Set when background work finished and the app wants a frame.
static WAKE: AtomicBool = AtomicBool::new(true);
/// Requests for the page, as JSON objects.
static REQUESTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static TEXT: OnceLock<Arc<text::WebText>> = OnceLock::new();

fn request(v: Value) {
    REQUESTS.lock().unwrap().push(v);
    WAKE.store(true, Ordering::SeqCst);
}

fn text_host() -> Arc<text::WebText> {
    TEXT.get_or_init(|| Arc::new(text::WebText::new())).clone()
}

/// Adds a font file before (or after) starting; `id` is its name in
/// `canvas_core::text::FONTS`.
#[wasm_bindgen]
pub fn add_font(id: &str, bytes: Vec<u8>) {
    let host = text_host();
    host.add_font(id, bytes);
    // Re-registering clears cached text sizes measured with fallbacks.
    canvas_core::text::set_host(host);
}

/// Restores a file saved in an earlier visit. Call before [`Inkboard::new`].
#[wasm_bindgen]
pub fn preload(path: &str, bytes: Vec<u8>) {
    canvas_core::sys::fs::preload(path, bytes);
}

/// Files changed since the last call, as `[path, bytes | null, ...]`.
#[wasm_bindgen]
pub fn take_changes() -> js_sys::Array {
    let out = js_sys::Array::new();
    for (path, bytes) in canvas_core::sys::fs::take_changes() {
        out.push(&JsValue::from_str(&path));
        out.push(&match bytes {
            Some(b) => js_sys::Uint8Array::from(&b[..]).into(),
            None => JsValue::NULL,
        });
    }
    out
}

/// Requests for the page since the last call, as a JSON array.
#[wasm_bindgen]
pub fn take_requests() -> String {
    Value::Array(std::mem::take(&mut *REQUESTS.lock().unwrap())).to_string()
}

struct WebPlatform;

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl Platform for WebPlatform {
    fn show_keyboard(&mut self, text: &str, multiline: bool) {
        request(json!({ "t": "keyboard", "text": text, "multiline": multiline }));
    }

    fn hide_keyboard(&mut self) {
        request(json!({ "t": "hideKeyboard" }));
    }

    fn read_ink(&mut self, token: u32, _lang: &str, _data: &[f32]) {
        request(json!({ "t": "ink", "token": token }));
    }

    fn check_updates(&mut self, manual: bool) {
        if manual {
            request(json!({ "t": "updates" }));
        }
    }

    fn copy_text(&mut self, text: &str) {
        request(json!({ "t": "copy", "text": text }));
    }

    fn open_url(&mut self, url: &str) {
        request(json!({ "t": "open", "url": url }));
    }

    fn sign_in_in_browser(&mut self, _server: &str) -> bool {
        // The page goes to Google and comes back to /app/#signin=…
        request(json!({ "t": "signin" }));
        true
    }

    fn save_png(&mut self, name: &str, png: &[u8]) {
        request(json!({ "t": "png", "name": name, "data": base64(png) }));
    }

    fn pick_boards(&mut self) {
        request(json!({ "t": "pickBoards" }));
    }

    fn pick_file(&mut self, token: i32, kind: FileKind) {
        let kind = match kind {
            FileKind::Image => "image",
            FileKind::Pdf => "pdf",
            FileKind::Document => "document",
        };
        request(json!({ "t": "pickFile", "token": token, "kind": kind }));
    }

    fn exit(&mut self) {
        request(json!({ "t": "exit" }));
    }
}

#[wasm_bindgen]
pub struct Inkboard {
    app: App,
    dirty: [i32; 4],
    now: f64,
}

fn sample(id: i32, x: f32, y: f32, time: f64, stylus: bool, eraser: bool, size: f32) -> Sample {
    Sample { id, x, y, time: time as u64, size, stylus, eraser, norm: 0.0 }
}

fn phase(p: &str) -> Phase {
    match p {
        "down" => Phase::Down,
        "up" => Phase::Up,
        "cancel" => Phase::Cancel,
        _ => Phase::Move,
    }
}

#[wasm_bindgen]
impl Inkboard {
    /// Starts the app. `server` is the page's origin, where team boards live.
    #[wasm_bindgen(constructor)]
    pub fn new(server: &str, version: &str, utc_offset_min: i32) -> Inkboard {
        canvas_core::text::set_host(text_host());
        // Team boards come from the server that serves the page.
        let settings = "/inkboard/settings.json";
        let mut v: Value = canvas_core::sys::fs::read_to_string(settings).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}));
        if let Some(m) = v.as_object_mut() {
            m.insert("server".into(), json!(server));
        }
        let _ = canvas_core::sys::fs::write(settings, v.to_string());
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| WAKE.store(true, Ordering::SeqCst));
        let app = App::new(Box::new(WebPlatform), "/inkboard/boards".into(), settings.into(), version.into(), utc_offset_min, wake);
        Inkboard { app, dirty: [0; 4], now: 0.0 }
    }

    pub fn resize(&mut self, w: u32, h: u32, scale: f32) {
        self.app.resize(w, h, scale);
        WAKE.store(true, Ordering::SeqCst);
    }

    /// Whether something changed since the last frame (background work,
    /// a request for the page).
    pub fn needs_frame(&self) -> bool {
        WAKE.load(Ordering::SeqCst)
    }

    /// Milliseconds (on the page's `performance.now()` clock) when the app
    /// next needs a frame, or -1.
    pub fn next_deadline(&self) -> f64 {
        self.app.next_deadline().map_or(-1.0, |d| d as f64)
    }

    /// Draws a frame. Returns whether pixels changed; [`Self::dirty`] says where.
    pub fn frame(&mut self, now: f64) -> bool {
        WAKE.store(false, Ordering::SeqCst);
        self.now = now;
        match self.app.frame(now as u64) {
            Some(r) => {
                self.dirty = [r.x0, r.y0, r.x1, r.y1];
                true
            }
            None => false,
        }
    }

    /// The changed rectangle of the last frame: x0, y0, x1, y1.
    pub fn dirty(&self) -> Vec<i32> {
        self.dirty.to_vec()
    }

    pub fn width(&self) -> u32 {
        self.app.output().w
    }

    pub fn height(&self) -> u32 {
        self.app.output().h
    }

    /// Address of the RGBA frame in WebAssembly memory.
    pub fn pixels(&self) -> *const u8 {
        self.app.output().data.as_ptr()
    }

    pub fn pointer(&mut self, phase_name: &str, id: i32, x: f32, y: f32, time: f64, stylus: bool, eraser: bool, size: f32) {
        self.app.touch(phase(phase_name), &[sample(id, x, y, time, stylus, eraser, size)]);
    }

    pub fn hover(&mut self, x: f32, y: f32) {
        self.app.hover(x, y);
    }

    pub fn hover_exit(&mut self) {
        self.app.hover_exit();
    }

    pub fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32, zoom: bool) {
        self.app.wheel(x, y, dx, dy, zoom);
    }

    /// Android key codes: 19-22 arrows, 66 enter.
    pub fn key(&mut self, code: i32) -> bool {
        self.app.key(code)
    }

    pub fn back(&mut self) -> bool {
        self.app.back()
    }

    pub fn shortcut(&mut self, name: &str) {
        let s = match name {
            "undo" => Shortcut::Undo,
            "redo" => Shortcut::Redo,
            "delete" => Shortcut::Delete,
            "zoomIn" => Shortcut::ZoomIn,
            "zoomOut" => Shortcut::ZoomOut,
            "fit" => Shortcut::Fit,
            _ => return,
        };
        self.app.shortcut(s);
    }

    pub fn text_input(&mut self, value: &str, submit: bool) {
        self.app.text_input(value, submit);
    }

    /// The page came back from Google with a session token.
    pub fn finish_sign_in(&mut self, token: &str) {
        self.app.finish_sign_in(token);
    }

    pub fn sign_in_failed(&mut self, message: &str) {
        self.app.sign_in_failed(message);
    }

    /// Opens a board from a `#board=<id>` link.
    pub fn open_board_link(&mut self, id: &str) {
        self.app.open_board_link(id);
    }

    pub fn toast(&mut self, text: &str) {
        self.app.toast(text);
    }

    pub fn ink_result(&mut self, token: u32) {
        self.app.ink_result(token, "");
    }

    pub fn no_updates(&mut self) {
        self.app.update_result(None, false, true);
    }

    pub fn file_chosen(&mut self, token: i32, name: &str, mime: &str, bytes: Vec<u8>) {
        self.app.file_result(token, PickerFile::File { name: name.into(), mime: mime.into(), bytes });
    }

    /// PDF pages rendered by the page, each a PNG.
    pub fn pdf_chosen(&mut self, token: i32, pages: js_sys::Array) {
        let pages = pages.iter().map(|p| js_sys::Uint8Array::new(&p).to_vec()).collect();
        self.app.file_result(token, PickerFile::Pdf(pages));
    }

    pub fn file_failed(&mut self, token: i32, message: &str) {
        let result = if message.is_empty() { PickerFile::Cancelled } else { PickerFile::Error(message.into()) };
        self.app.file_result(token, result);
    }

    /// Board files the person chose to bring in, as a JSON array of strings.
    pub fn import_boards(&mut self, files: &str) {
        let files: Vec<String> = serde_json::from_str(files).unwrap_or_default();
        self.app.import_boards(files);
    }

    /// Saves the open board now (the page is being hidden).
    pub fn save_now(&mut self) {
        let (tx, _rx) = mpsc::sync_channel(1);
        self.app.pause(tx);
        // Disk work runs in the next frame.
        let _ = self.app.frame(self.now as u64);
    }
}
