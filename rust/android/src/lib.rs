//! Android glue for the Canvas Studio core.
//!
//! Threads:
//! * The **UI thread** only forwards touch samples into a channel
//!   (`Native.touch`). It never waits for a lock, so input is never blocked by
//!   drawing.
//! * The **render thread** owns presenting. It blocks on the channel, drains
//!   every sample that is waiting, lets the editor repaint the few pixels that
//!   changed and copies exactly that rectangle into the window buffer
//!   (`ANativeWindow_lock` with a dirty rectangle, then `unlockAndPost`).
//!
//! Everything is software rendered, so there is no dependence on a GPU driver.

#![cfg(target_os = "android")]
#![allow(non_snake_case)]

mod fastpresent;
mod ui_host;

use canvas_core::editor::{Contact, Editor, PenMode};
use canvas_core::geom::{IRect, Pt};
use canvas_core::render::Layer;
use canvas_core::shape::Tool;
use canvas_core::text::{self, TextBitmap, TextHost};
use canvas_core::{render, store};
use jni::objects::{GlobalRef, JClass, JFloatArray, JIntArray, JObject, JString, JValue};
use jni::sys::{jdouble, jdoubleArray, jfloatArray, jint, jintArray, jlong, jobject, jstring};
use jni::{JNIEnv, JavaVM};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_long, c_void, CString};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

// ------------------------------------------------------------ system calls

#[repr(C)]
struct ANativeWindow {
    _private: [u8; 0],
}

#[repr(C)]
struct ANativeWindowBuffer {
    width: i32,
    height: i32,
    stride: i32,
    format: i32,
    bits: *mut c_void,
    reserved: [u32; 6],
}

#[repr(C)]
struct ARect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct Timespec {
    tv_sec: c_long,
    tv_nsec: c_long,
}

#[link(name = "android")]
extern "C" {
    fn ANativeWindow_fromSurface(env: *mut jni::sys::JNIEnv, surface: jobject) -> *mut ANativeWindow;
    fn ANativeWindow_release(window: *mut ANativeWindow);
    fn ANativeWindow_lock(window: *mut ANativeWindow, out: *mut ANativeWindowBuffer, dirty: *mut ARect) -> i32;
    fn ANativeWindow_unlockAndPost(window: *mut ANativeWindow) -> i32;
}

#[link(name = "log")]
extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

extern "C" {
    fn clock_gettime(clock: c_int, ts: *mut Timespec) -> c_int;
}

fn log(prio: c_int, message: &str) {
    if let Ok(text) = CString::new(message.replace('\0', " ")) {
        unsafe { __android_log_write(prio, b"CanvasRust\0".as_ptr() as *const c_char, text.as_ptr()) };
    }
}

/// Milliseconds on CLOCK_MONOTONIC, the clock `MotionEvent.getEventTime` uses.
fn now_ms() -> f64 {
    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { clock_gettime(1, &mut ts) };
    ts.tv_sec as f64 * 1000.0 + ts.tv_nsec as f64 / 1e6
}

struct Window(*mut ANativeWindow);
unsafe impl Send for Window {}
impl Drop for Window {
    fn drop(&mut self) {
        fastpresent::forget(self.0);
        unsafe { ANativeWindow_release(self.0) };
    }
}

/// Copies `dirty` (or the larger area the system says must be redrawn) from
/// `frame` into the window and shows it.
fn present(window: &Window, frame: &Layer, dirty: IRect) -> bool {
    if let Some(ok) = fastpresent::present(window.0, frame, dirty) {
        return ok;
    }
    let dirty = if fastpresent::take_full_redraw() { IRect { x0: 0, y0: 0, x1: frame.w as i32, y1: frame.h as i32 } } else { dirty };
    let mut buffer = ANativeWindowBuffer { width: 0, height: 0, stride: 0, format: 0, bits: std::ptr::null_mut(), reserved: [0; 6] };
    let mut rect = ARect { left: dirty.x0, top: dirty.y0, right: dirty.x1, bottom: dirty.y1 };
    if unsafe { ANativeWindow_lock(window.0, &mut buffer, &mut rect) } != 0 {
        return false;
    }
    let w = buffer.width.min(frame.w as i32);
    let h = buffer.height.min(frame.h as i32);
    let (x0, x1) = (rect.left.clamp(0, w), rect.right.clamp(0, w));
    let (y0, y1) = (rect.top.clamp(0, h), rect.bottom.clamp(0, h));
    if !buffer.bits.is_null() && x1 > x0 && y1 > y0 {
        let n = (x1 - x0) as usize;
        for y in y0..y1 {
            let s = (y as usize * frame.w as usize + x0 as usize) * 4;
            let src = &frame.data[s..s + n * 4];
            unsafe {
                match buffer.format {
                    // RGBA_8888 and RGBX_8888: the frame is opaque, so bytes copy as they are.
                    1 | 2 => {
                        let dst = (buffer.bits as *mut u8).add((y as usize * buffer.stride as usize + x0 as usize) * 4);
                        std::ptr::copy_nonoverlapping(src.as_ptr(), dst, n * 4);
                    }
                    // RGB_565, in case a device refuses the 32 bit format.
                    4 => {
                        let dst = (buffer.bits as *mut u16).add(y as usize * buffer.stride as usize + x0 as usize);
                        for (i, px) in src.chunks_exact(4).enumerate() {
                            let v = ((px[0] as u16 >> 3) << 11) | ((px[1] as u16 >> 2) << 5) | (px[2] as u16 >> 3);
                            dst.add(i).write_unaligned(v);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    unsafe { ANativeWindow_unlockAndPost(window.0) == 0 }
}

// ------------------------------------------------------------------ engine

/// Floats per touch sample.
const SAMPLE: usize = 6;

enum Msg {
    /// `data` holds samples of (pointer id, x, y, milliseconds after `base`,
    /// touch size in pixels, 1 for a stylus, 2 for its eraser end).
    Touch { action: i32, base: i64, data: Vec<f32> },
    Surface { window: Option<Window>, w: u32, h: u32, dpr: f64, done: SyncSender<()> },
    Wake,
    Quit,
}

struct Engine {
    editor: Arc<Mutex<Editor>>,
    tx: Mutex<Sender<Msg>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    /// Parts of the open board file this app does not edit (sharing state).
    extra: Mutex<serde_json::Map<String, serde_json::Value>>,
    /// Handwriting handed to the host and not answered yet: token -> strokes.
    ink: Mutex<HashMap<u32, Vec<String>>>,
}

impl Engine {
    fn send(&self, msg: Msg) {
        let _ = self.tx.lock().unwrap().send(msg);
    }
}

#[derive(Default)]
struct StrokeStats {
    frames: u32,
    samples: u32,
    latency_sum: f64,
    latency_max: f64,
    work_sum: f64,
    work_max: f64,
    pixels: u64,
}

fn render_loop(vm: JavaVM, listener: GlobalRef, editor: Arc<Mutex<Editor>>, rx: Receiver<Msg>) {
    let mut env = match vm.attach_current_thread_as_daemon() {
        Ok(env) => env,
        Err(e) => {
            log(6, &format!("render thread could not attach: {e}"));
            return;
        }
    };
    let mut window: Option<Window> = None;
    let mut deadline: Option<u64> = None;
    let mut stats = StrokeStats::default();
    loop {
        let first = match deadline {
            Some(at) => {
                let wait = (at as f64 - now_ms()).max(0.0);
                match rx.recv_timeout(Duration::from_micros((wait * 1000.0) as u64 + 500)) {
                    Ok(m) => Some(m),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            None => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            },
        };
        let started = now_ms();
        let mut newest_sample: Option<f64> = None;
        let mut lifted = false;
        let mut acks: Vec<SyncSender<()>> = Vec::new();
        let mut quit = false;
        let events;
        {
            let mut ed = editor.lock().unwrap();
            let mut next = first;
            // Take everything that is waiting, so one frame shows all of it.
            while let Some(msg) = next {
                match msg {
                    Msg::Touch { action, base, data } => {
                        for s in data.chunks_exact(SAMPLE) {
                            let id = s[0] as i32;
                            let pos = Pt::new(s[1] as f64, s[2] as f64);
                            let time = (base as f64 + s[3] as f64).max(0.0);
                            let contact = Contact { size: s[4].max(0.0) as f64, stylus: s[5] != 0.0, eraser: s[5] > 1.5, norm: 0.0 };
                            newest_sample = Some(time);
                            stats.samples += 1;
                            match action {
                                0 => ed.pointer_down_with(id, pos, time as u64, contact),
                                1 => ed.pointer_move_with(id, pos, time as u64, contact),
                                2 => ed.pointer_up(id, pos, time as u64, false),
                                _ => ed.pointer_up(id, pos, time as u64, true),
                            }
                        }
                        lifted |= action >= 2;
                    }
                    Msg::Surface { window: next_window, w, h, dpr, done } => {
                        window = next_window; // releases the old one first
                        if window.is_some() {
                            ed.resize(w, h, dpr);
                            ed.invalidate_all();
                        }
                        acks.push(done);
                    }
                    Msg::Wake => {}
                    Msg::Quit => quit = true,
                }
                next = rx.try_recv().ok();
            }
            ed.tick(now_ms() as u64);
            if let Some(win) = &window {
                if let Some(dirty) = ed.render() {
                    let drawn = now_ms();
                    present(win, ed.frame(), dirty);
                    if let Some(sample) = newest_sample {
                        let shown = now_ms();
                        stats.frames += 1;
                        stats.latency_sum += shown - sample;
                        stats.latency_max = stats.latency_max.max(shown - sample);
                        stats.work_sum += shown - started;
                        stats.work_max = stats.work_max.max(shown - started);
                        stats.pixels += (dirty.width().max(0) as u64) * (dirty.height().max(0) as u64);
                        let _ = drawn;
                    }
                }
            }
            deadline = ed.next_deadline();
            events = ed.take_events();
            if lifted && !ed.pointer_down_now() {
                if stats.frames > 0 {
                    let f = stats.frames as f64;
                    log(
                        4,
                        &format!(
                            "gesture: {} samples in {} frames; sample-to-screen avg {:.1} ms max {:.1} ms; draw+present avg {:.2} ms max {:.2} ms; avg {:.0} px repainted per frame; {} shapes; full repaints {}",
                            stats.samples,
                            stats.frames,
                            stats.latency_sum / f,
                            stats.latency_max,
                            stats.work_sum / f,
                            stats.work_max,
                            stats.pixels as f64 / f,
                            ed.doc.shapes().len(),
                            ed.full_repaints,
                        ),
                    );
                }
                stats = StrokeStats::default();
            }
        }
        for done in acks {
            let _ = done.send(());
        }
        if events != 0 {
            if env.call_method(&listener, "onEvent", "(I)V", &[JValue::Int(events as jint)]).is_err() {
                let _ = env.exception_clear();
            }
        }
        if quit {
            return;
        }
    }
}

// --------------------------------------------------------------- text host

struct AndroidText {
    vm: JavaVM,
    class: GlobalRef,
}

impl AndroidText {
    fn with_env<T>(&self, f: impl FnOnce(&mut JNIEnv, &JClass) -> jni::errors::Result<T>) -> Option<T> {
        let mut env = self.vm.attach_current_thread_as_daemon().ok()?;
        let class: &JClass = self.class.as_obj().into();
        let result = env.with_local_frame(8, |env| f(env, class));
        match result {
            Ok(v) => Some(v),
            Err(_) => {
                let _ = env.exception_clear();
                None
            }
        }
    }
}

impl TextHost for AndroidText {
    fn measure(&self, content: &str, font_size: f64, font: u8) -> (f64, f64) {
        self.with_env(|env, class| {
            let s = env.new_string(content)?;
            let out = env
                .call_static_method(class, "measure", "(Ljava/lang/String;FI)[F", &[JValue::Object(&s), JValue::Float(font_size as f32), JValue::Int(font as jint)])?
                .l()?;
            let mut size = [0f32; 2];
            env.get_float_array_region(JFloatArray::from(out), 0, &mut size)?;
            Ok((size[0] as f64, size[1] as f64))
        })
        .unwrap_or((content.chars().count() as f64 * font_size * 0.55, font_size * text::LINE_HEIGHT))
    }

    fn rasterize(&self, content: &str, font_px: f64, color: u32, font: u8) -> Option<TextBitmap> {
        self.with_env(|env, class| {
            let s = env.new_string(content)?;
            let out = env
                .call_static_method(
                    class,
                    "render",
                    "(Ljava/lang/String;FII)[I",
                    &[JValue::Object(&s), JValue::Float(font_px as f32), JValue::Int(color as i32), JValue::Int(font as jint)],
                )?
                .l()?;
            if out.is_null() {
                return Ok(None);
            }
            let array = JIntArray::from(out);
            let len = env.get_array_length(&array)? as usize;
            if len < 3 {
                return Ok(None);
            }
            let mut argb = vec![0i32; len];
            env.get_int_array_region(&array, 0, &mut argb)?;
            let (w, h) = (argb[0] as usize, argb[1] as usize);
            if w == 0 || h == 0 || w * h + 2 != len {
                return Ok(None);
            }
            let mut rgba = Vec::with_capacity(w * h * 4);
            for px in &argb[2..] {
                let px = *px as u32;
                let a = px >> 24;
                let pre = |c: u32| ((c & 0xff) * a / 255) as u8;
                rgba.extend_from_slice(&[pre(px >> 16), pre(px >> 8), pre(px), a as u8]);
            }
            Ok(Some(TextBitmap { width: w as u32, height: h as u32, rgba }))
        })
        .flatten()
    }
}

// --------------------------------------------------------------------- JNI

fn engine<'a>(handle: jlong) -> Option<&'a Engine> {
    if handle == 0 {
        None
    } else {
        Some(unsafe { &*(handle as *const Engine) })
    }
}

fn string(env: &mut JNIEnv, s: &JString) -> String {
    if s.is_null() {
        return String::new();
    }
    env.get_string(s).map(|s| s.into()).unwrap_or_default()
}

fn jstring_or_null(env: &mut JNIEnv, s: Option<String>) -> jstring {
    match s.and_then(|s| env.new_string(s).ok()) {
        Some(s) => s.into_raw(),
        None => std::ptr::null_mut(),
    }
}

static INIT: OnceLock<()> = OnceLock::new();

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_init<'l>(env: JNIEnv<'l>, _: JClass<'l>, text_class: JClass<'l>) {
    INIT.get_or_init(|| {
        std::panic::set_hook(Box::new(|info| log(6, &format!("panic: {info}"))));
        if let (Ok(vm), Ok(class)) = (env.get_java_vm(), env.new_global_ref(&text_class)) {
            text::set_host(Arc::new(AndroidText { vm, class }));
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_create<'l>(env: JNIEnv<'l>, _: JClass<'l>, listener: JObject<'l>) -> jlong {
    let (Ok(vm), Ok(listener)) = (env.get_java_vm(), env.new_global_ref(&listener)) else {
        return 0;
    };
    let editor = Arc::new(Mutex::new(Editor::new()));
    let (tx, rx) = mpsc::channel();
    let shared = editor.clone();
    let thread = std::thread::Builder::new()
        .name("ink-render".into())
        .spawn(move || render_loop(vm, listener, shared, rx));
    let Ok(thread) = thread else { return 0 };
    let engine = Engine { editor, tx: Mutex::new(tx), thread: Mutex::new(Some(thread)), extra: Mutex::new(Default::default()), ink: Mutex::new(HashMap::new()) };
    Box::into_raw(Box::new(engine)) as jlong
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_destroy<'l>(_: JNIEnv<'l>, _: JClass<'l>, handle: jlong) {
    if handle == 0 {
        return;
    }
    let engine = unsafe { Box::from_raw(handle as *mut Engine) };
    engine.send(Msg::Quit);
    let thread = engine.thread.lock().unwrap().take();
    if let Some(thread) = thread {
        let _ = thread.join();
    }
}

/// Gives the render thread a new surface (or takes it away when `surface` is
/// null) and returns only once the old one is no longer in use.
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_surface<'l>(
    env: JNIEnv<'l>,
    _: JClass<'l>,
    handle: jlong,
    surface: JObject<'l>,
    w: jint,
    h: jint,
    dpr: jdouble,
) {
    let Some(engine) = engine(handle) else { return };
    let window = if surface.is_null() || w <= 0 || h <= 0 {
        None
    } else {
        let ptr = unsafe { ANativeWindow_fromSurface(env.get_raw(), surface.as_raw()) };
        if ptr.is_null() {
            None
        } else {
            Some(Window(ptr))
        }
    };
    let (done, wait) = mpsc::sync_channel(1);
    engine.send(Msg::Surface { window, w: w.max(0) as u32, h: h.max(0) as u32, dpr, done });
    let _ = wait.recv_timeout(Duration::from_secs(3));
}

/// `action`: 0 down, 1 move, 2 up, 3 cancel. `data` holds `count` samples of
/// (pointer id, x, y, milliseconds after `base`, touch size in pixels,
/// 1 for a stylus, 2 for its eraser end), oldest first.
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_touch<'l>(
    env: JNIEnv<'l>,
    _: JClass<'l>,
    handle: jlong,
    action: jint,
    base: jlong,
    data: JFloatArray<'l>,
    count: jint,
) {
    let Some(engine) = engine(handle) else { return };
    let mut samples = vec![0f32; count.max(0) as usize * SAMPLE];
    if env.get_float_array_region(&data, 0, &mut samples).is_ok() {
        engine.send(Msg::Touch { action, base, data: samples });
    }
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_command<'l>(
    _: JNIEnv<'l>,
    _: JClass<'l>,
    handle: jlong,
    command: jint,
    a: jdouble,
    b: jdouble,
) {
    let Some(engine) = engine(handle) else { return };
    {
        let mut ed = engine.editor.lock().unwrap();
        match command {
            1 => {
                if let Some(tool) = Tool::from_index(a as i32) {
                    ed.set_tool(tool, if b != 0.0 { PenMode::Shape } else { PenMode::Ink });
                }
            }
            2 => ed.apply_style(Some(a as i64 as u32), None, None),
            3 => ed.apply_style(None, Some(a), None),
            4 => ed.apply_style(None, None, Some(a != 0.0)),
            5 => ed.undo(),
            6 => ed.redo(),
            7 => ed.delete_selection(),
            8 => ed.fit(),
            9 => {
                let next = ed.zoom() * a;
                ed.zoom_to(next, None);
            }
            10 => ed.set_grid(a != 0.0),
            11 => ed.set_snap_shapes(a != 0.0),
            12 => ed.zoom_to(1.0, None),
            13 => ed.set_auto_convert(a != 0.0),
            17 => ed.set_auto_text(a != 0.0),
            14 => ed.set_font(a.clamp(0.0, 255.0) as u8),
            15 => ed.scale_text(a),
            16 => ed.set_palm_rejection(a != 0.0, if b > 0.0 { Some(b) } else { None }),
            _ => {}
        }
    }
    engine.send(Msg::Wake);
}

/// [tool, penMode, canUndo, canRedo, selected, zoom, colour, width, filled,
/// shapes, revision, grid, pointerDown, snapShapes, font, autoConvert,
/// selectedText, palmRejection]
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_state<'l>(env: JNIEnv<'l>, _: JClass<'l>, handle: jlong) -> jdoubleArray {
    let Some(engine) = engine(handle) else { return std::ptr::null_mut() };
    let values: [f64; 19] = {
        let ed = engine.editor.lock().unwrap();
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        [
            ed.tool().index() as f64,
            flag(ed.pen_mode() == PenMode::Shape),
            flag(ed.doc.can_undo()),
            flag(ed.doc.can_redo()),
            ed.doc.selected_ids().len() as f64,
            ed.zoom(),
            ed.color() as f64,
            ed.stroke_width(),
            flag(ed.filled()),
            ed.doc.shapes().len() as f64,
            ed.doc.revision as f64,
            flag(ed.grid()),
            flag(ed.pointer_down_now()),
            flag(ed.snap_shapes()),
            ed.font() as f64,
            flag(ed.auto_convert()),
            ed.selected_text_count() as f64,
            flag(ed.palm_rejection()),
            flag(ed.auto_text()),
        ]
    };
    let Ok(array) = env.new_double_array(values.len() as i32) else { return std::ptr::null_mut() };
    let _ = env.set_double_array_region(&array, 0, &values);
    array.into_raw()
}

/// The waiting text request as "id\u{1f}x\u{1f}y\u{1f}text", or null.
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_takeTextRequest<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, handle: jlong) -> jstring {
    let request = engine(handle).and_then(|e| e.editor.lock().unwrap().take_text_request());
    jstring_or_null(&mut env, request.map(|r| format!("{}\u{1f}{}\u{1f}{}\u{1f}{}", r.id, r.x, r.y, r.text)))
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_commitText<'l>(
    mut env: JNIEnv<'l>,
    _: JClass<'l>,
    handle: jlong,
    id: JString<'l>,
    x: jdouble,
    y: jdouble,
    content: JString<'l>,
) {
    let Some(engine) = engine(handle) else { return };
    let (id, content) = (string(&mut env, &id), string(&mut env, &content));
    engine.editor.lock().unwrap().commit_text(&id, x, y, &content);
    engine.send(Msg::Wake);
}

/// Handwriting waiting to be read, or null: [token, strokes, then for each
/// stroke its point count followed by x, y pairs]. Coordinates are relative
/// to the top left of the writing.
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_takeInkRequest<'l>(env: JNIEnv<'l>, _: JClass<'l>, handle: jlong) -> jfloatArray {
    let Some(engine) = engine(handle) else { return std::ptr::null_mut() };
    let Some(request) = engine.editor.lock().unwrap().take_ink_request() else { return std::ptr::null_mut() };
    let mut out = vec![request.token as f32, request.strokes.len() as f32];
    for stroke in &request.strokes {
        out.push(stroke.len() as f32);
        for p in stroke {
            out.push((p.x - request.bounds.l) as f32);
            out.push((p.y - request.bounds.t) as f32);
        }
    }
    let Ok(array) = env.new_float_array(out.len() as i32) else { return std::ptr::null_mut() };
    let _ = env.set_float_array_region(&array, 0, &out);
    let mut waiting = engine.ink.lock().unwrap();
    if waiting.len() > 64 {
        waiting.clear();
    }
    waiting.insert(request.token, request.ids);
    array.into_raw()
}

/// Answers a handwriting request with the text it reads as. Empty text
/// leaves the ink as it is.
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_commitInk<'l>(
    mut env: JNIEnv<'l>,
    _: JClass<'l>,
    handle: jlong,
    token: jint,
    content: JString<'l>,
) {
    let Some(engine) = engine(handle) else { return };
    let content = string(&mut env, &content);
    let Some(ids) = engine.ink.lock().unwrap().remove(&(token as u32)) else { return };
    engine.editor.lock().unwrap().commit_ink(&ids, &content);
    engine.send(Msg::Wake);
}

/// Loads a board into the editor. Returns its title, or null if it could not
/// be read. Call from a background thread.
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_loadBoard<'l>(
    mut env: JNIEnv<'l>,
    _: JClass<'l>,
    handle: jlong,
    dir: JString<'l>,
    id: JString<'l>,
) -> jstring {
    let Some(engine) = engine(handle) else { return std::ptr::null_mut() };
    let (dir, id) = (string(&mut env, &dir), string(&mut env, &id));
    let title = match store::load(Path::new(&dir), &id) {
        Ok(board) => {
            engine.ink.lock().unwrap().clear();
            let loaded = engine.editor.lock().unwrap().load(board.shapes);
            match loaded {
                Ok(()) => {
                    *engine.extra.lock().unwrap() = board.extra;
                    engine.editor.lock().unwrap().fit();
                    engine.send(Msg::Wake);
                    Some(board.title)
                }
                Err(e) => {
                    log(6, &format!("board {id} is not valid: {e}"));
                    None
                }
            }
        }
        Err(e) => {
            log(6, &format!("board {id} could not be read: {e}"));
            None
        }
    };
    jstring_or_null(&mut env, title)
}

/// Writes the open board. Returns 0 when saved, 1 on failure and 2 when a
/// finger is down (the caller tries again later). Call from a background
/// thread; the editor is locked only long enough to copy the shape list.
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_saveBoard<'l>(
    mut env: JNIEnv<'l>,
    _: JClass<'l>,
    handle: jlong,
    dir: JString<'l>,
    id: JString<'l>,
    title: JString<'l>,
    updated: JString<'l>,
    even_if_drawing: jint,
) -> jint {
    let Some(engine) = engine(handle) else { return 1 };
    let (dir, id) = (string(&mut env, &dir), string(&mut env, &id));
    let (title, updated) = (string(&mut env, &title), string(&mut env, &updated));
    let shapes = {
        let ed = engine.editor.lock().unwrap();
        if ed.pointer_down_now() && even_if_drawing == 0 {
            return 2;
        }
        ed.doc.shapes().to_vec()
    };
    let extra = engine.extra.lock().unwrap().clone();
    let contents = store::encode(&id, &title, &updated, &shapes, &extra);
    match store::save_atomic(Path::new(&dir), &id, &contents) {
        Ok(()) => 0,
        Err(e) => {
            log(6, &e);
            1
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_listBoards<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, dir: JString<'l>) -> jstring {
    let dir = string(&mut env, &dir);
    let json = store::list_json(Path::new(&dir));
    jstring_or_null(&mut env, Some(json))
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_createBoard<'l>(
    mut env: JNIEnv<'l>,
    _: JClass<'l>,
    dir: JString<'l>,
    id: JString<'l>,
    title: JString<'l>,
    updated: JString<'l>,
) -> jint {
    let (dir, id) = (string(&mut env, &dir), string(&mut env, &id));
    let (title, updated) = (string(&mut env, &title), string(&mut env, &updated));
    let contents = store::encode(&id, &title, &updated, &[], &Default::default());
    store::save_atomic(Path::new(&dir), &id, &contents).is_err() as jint
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_deleteBoard<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, dir: JString<'l>, id: JString<'l>) -> jint {
    let (dir, id) = (string(&mut env, &dir), string(&mut env, &id));
    store::delete(Path::new(&dir), &id).is_err() as jint
}

#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_renameBoard<'l>(
    mut env: JNIEnv<'l>,
    _: JClass<'l>,
    dir: JString<'l>,
    id: JString<'l>,
    title: JString<'l>,
    updated: JString<'l>,
) -> jint {
    let (dir, id) = (string(&mut env, &dir), string(&mut env, &id));
    let (title, updated) = (string(&mut env, &title), string(&mut env, &updated));
    store::rename(Path::new(&dir), &id, &title, &updated).is_err() as jint
}

/// A preview of a saved board as ARGB pixels, or null.
#[no_mangle]
pub extern "system" fn Java_com_canvasstudio_rust_Native_thumbnail<'l>(
    mut env: JNIEnv<'l>,
    _: JClass<'l>,
    dir: JString<'l>,
    id: JString<'l>,
    w: jint,
    h: jint,
) -> jintArray {
    let (dir, id) = (string(&mut env, &dir), string(&mut env, &id));
    if w <= 0 || h <= 0 || w > 2048 || h > 2048 {
        return std::ptr::null_mut();
    }
    let Ok(board) = store::load(Path::new(&dir), &id) else { return std::ptr::null_mut() };
    let shapes: Vec<_> = board.shapes.into_iter().map(Arc::new).collect();
    let rgba = render::thumbnail(&shapes, w as u32, h as u32);
    let argb: Vec<i32> = rgba
        .chunks_exact(4)
        .map(|p| (0xff00_0000u32 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32) as i32)
        .collect();
    let Ok(array) = env.new_int_array(argb.len() as i32) else { return std::ptr::null_mut() };
    let _ = env.set_int_array_region(&array, 0, &argb);
    array.into_raw()
}
