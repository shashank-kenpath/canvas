//! The whole app as the person sees it: the board library, the board editor
//! with its floating toolbars, menus, dialogs and messages. It mirrors the
//! Flutter Canvas Studio layout and draws everything itself, so the host only
//! supplies a window, touches and a few system services ([`Platform`]).

use super::font::{self, Face};
use super::net;
use super::paint::{self, Painter, Rf};
use crate::editor::{Contact, Editor, PenMode, EV_DOC, EV_HAPTIC, EV_INK, EV_TEXT};
use crate::geom::{IRect, Pt};
use crate::render::{self, Layer};
use crate::shape::{Shape, Tool};
use crate::store::{self, BoardFile, BoardMeta};
use crate::text;
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use tiny_skia::PixmapMut;

// ------------------------------------------------------------------ colours

const PURPLE: u32 = 0xff6750d8;
const INK: u32 = 0xff272536;
const PAGE: u32 = 0xfff6f5fa;
const BLUE: u32 = 0xff2f80ed;
const BORDER: u32 = 0xffe8e8ec;
const ICON: u32 = 0xff1d1d1d;
const DISABLED: u32 = 0xffc2c5cb;
const STATUS: u32 = 0xff8b9098;
const CHOSEN: u32 = 0xffeceef1;
const HINT: u32 = 0xff5f6570;
const GREY: u32 = 0xff9e9e9e;
const WHITE: u32 = 0xffffffff;
const PRIMARY: u32 = 0xff644ac9;
const FAB_BG: u32 = 0xffe7deff;
const FAB_FG: u32 = 0xff21005e;
const DIALOG: u32 = 0xffefe9f8;
const MENU: u32 = 0xfff3edf7;
const ON_SURFACE: u32 = 0xff1d1b20;
const ON_VARIANT: u32 = 0xff49454f;
const ERROR: u32 = 0xffb3261e;
const TOAST_BG: u32 = 0xff322f35;
const TOAST_FG: u32 = 0xfff5eff7;

const PALETTE: [u32; 12] = [
    0xff1d1d1d, 0xff9fa8b2, 0xffe085f4, 0xffae3ec9, 0xff4465e9, 0xff4ba1f1, 0xfff1ac4b, 0xffe16919,
    0xff099268, 0xff4cb05e, 0xfff87777, 0xffe03131,
];
const SIZES: [(&str, f64); 4] = [("S", 2.0), ("M", 3.5), ("L", 5.0), ("XL", 10.0)];
/// Where the panel is reached from other networks (see server/relay.mjs).
const RELAY: &str = "https://inkboard.taila6db10.ts.net";
const LANGS: [(&str, &str); 2] = [("en", "English"), ("hi", "हिन्दी")];

// -------------------------------------------------------- Material icons

mod icon {
    pub const SELECT: u32 = 0xf206;
    pub const HAND: u32 = 0xf250;
    pub const PEN: u32 = 0xf00d;
    /// Drawn by hand: Material has no eraser glyph in this font.
    pub const ERASER: u32 = u32::MAX;
    pub const WRITE: u32 = 0xe5e1;
    pub const SMART: u32 = 0xeea9;
    pub const SHARE: u32 = 0xf378;
    pub const JOIN: u32 = 0xf0d8;
    pub const CLAUDE: u32 = 0xf3a8;
    pub const FONT: u32 = 0xf096;
    pub const ARROW: u32 = 0xe443;
    pub const RECTANGLE: u32 = 0xe1ae;
    pub const ELLIPSE: u32 = 0xef53;
    pub const BACK: u32 = 0xe092;
    pub const MENU: u32 = 0xe3dc;
    pub const UNDO: u32 = 0xe68c;
    pub const REDO: u32 = 0xe512;
    pub const DELETE: u32 = 0xe1bb;
    pub const REMOVE: u32 = 0xe516;
    pub const ADD: u32 = 0xe047;
    pub const FIT: u32 = 0xf079;
    pub const NO_FILL: u32 = 0xe1ae;
    pub const FILL: u32 = 0xf0385;
    pub const UPDATE: u32 = 0xf41a;
    pub const DRAW: u32 = 0xf05ed;
    pub const MORE: u32 = 0xe404;
    pub const BIGGER: u32 = 0xf058b;
    pub const SMALLER: u32 = 0xf058a;
    pub const SPEED: u32 = 0xf3c3;
    pub const CHECK: u32 = 0xe156;
    pub const TO_TEXT: u32 = 0xe649;
}

// ----------------------------------------------------------- host side

/// System services the app asks its host for. Every call comes from the
/// thread that drives the [`App`].
pub trait Platform: Send {
    fn haptic(&mut self) {}
    /// Shows the on-screen keyboard editing `text`. The host reports every
    /// change with [`App::text_input`].
    fn show_keyboard(&mut self, _text: &str, _multiline: bool) {}
    fn hide_keyboard(&mut self) {}
    /// Gets handwriting reading ready for a language ("en" or "hi").
    fn prepare_handwriting(&mut self, _lang: &str) {}
    /// Reads handwriting and answers with [`App::ink_result`]. `data` is
    /// [token, strokes, then per stroke its point count and x, y pairs].
    fn read_ink(&mut self, _token: u32, _lang: &str, _data: &[f32]) {}
    /// Looks for a newer release and answers with [`App::update_result`].
    fn check_updates(&mut self, _manual: bool) {}
    fn install_update(&mut self, _url: &str) {}
    /// Draw at the panel's full resolution instead of a capped one.
    fn set_full_resolution(&mut self, _on: bool) {}
    /// Show live ink through a dedicated low-latency layer.
    fn set_fast_ink(&mut self, _on: bool) {}
    /// Puts `text` on the clipboard.
    fn copy_text(&mut self, _text: &str) {}
    /// Saves a PNG picture to the device's Pictures.
    fn save_png(&mut self, _name: &str, _png: &[u8]) {}
    /// Lets the user choose board files to bring in. The host answers with
    /// [`App::import_boards`].
    fn pick_boards(&mut self) {}
    fn exit(&mut self) {}
}

/// One touch sample. Positions are buffer pixels; `time` is milliseconds on
/// the host's monotonic clock.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub id: i32,
    pub x: f32,
    pub y: f32,
    pub time: u64,
    pub size: f32,
    pub stylus: bool,
    /// The eraser end of a stylus.
    pub eraser: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Android key codes the app understands.
pub mod keys {
    pub const UP: i32 = 19;
    pub const DOWN: i32 = 20;
    pub const LEFT: i32 = 21;
    pub const RIGHT: i32 = 22;
    pub const CENTER: i32 = 23;
    pub const ENTER: i32 = 66;
    pub const NUMPAD_ENTER: i32 = 160;
}

// ---------------------------------------------------------------- disk

enum Job {
    List,
    Thumb {
        id: String,
        updated: String,
        w: u32,
        h: u32,
    },
    Load(String),
    Save {
        id: String,
        title: String,
        updated: String,
        shapes: Vec<Arc<Shape>>,
        extra: Map<String, Value>,
        seq: u64,
        ack: Option<SyncSender<()>>,
    },
    Create {
        id: String,
        title: String,
        updated: String,
        extra: Map<String, Value>,
    },
    Delete(String),
    Rename {
        id: String,
        title: String,
        updated: String,
    },
    /// The text of board files to copy in.
    Import(Vec<String>),
}

enum Done {
    Listed(Vec<BoardMeta>),
    Imported {
        added: usize,
        skipped: usize,
        failed: usize,
        list: Vec<BoardMeta>,
    },
    Thumb {
        id: String,
        updated: String,
        w: u32,
        h: u32,
        rgba: Vec<u8>,
    },
    Loaded(String, Result<BoardFile, String>),
    Saved {
        seq: u64,
        ok: bool,
    },
    Created(String, bool),
}

fn disk_worker(
    dir: PathBuf,
    jobs: Receiver<Job>,
    done: Sender<Done>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    while let Ok(job) = jobs.recv() {
        let out = match job {
            Job::List => Done::Listed(store::list(&dir)),
            Job::Thumb { id, updated, w, h } => match store::load(&dir, &id) {
                Ok(board) => {
                    let shapes: Vec<Arc<Shape>> = board.shapes.into_iter().map(Arc::new).collect();
                    Done::Thumb {
                        id,
                        updated,
                        w,
                        h,
                        rgba: render::thumbnail(&shapes, w, h),
                    }
                }
                Err(_) => continue,
            },
            Job::Load(id) => {
                let result = store::load(&dir, &id);
                Done::Loaded(id, result)
            }
            Job::Save {
                id,
                title,
                updated,
                shapes,
                extra,
                seq,
                ack,
            } => {
                let contents = store::encode(&id, &title, &updated, &shapes, &extra);
                let ok = store::save_atomic(&dir, &id, &contents).is_ok();
                if let Some(ack) = ack {
                    let _ = ack.send(());
                }
                Done::Saved { seq, ok }
            }
            Job::Create { id, title, updated, extra } => {
                let contents = store::encode(&id, &title, &updated, &[], &extra);
                let ok = store::save_atomic(&dir, &id, &contents).is_ok();
                Done::Created(id, ok)
            }
            Job::Delete(id) => {
                let _ = store::delete(&dir, &id);
                Done::Listed(store::list(&dir))
            }
            Job::Rename { id, title, updated } => {
                let _ = store::rename(&dir, &id, &title, &updated);
                Done::Listed(store::list(&dir))
            }
            Job::Import(files) => {
                let (mut added, mut skipped, mut failed) = (0, 0, 0);
                for (i, source) in files.iter().enumerate() {
                    match store::import(&dir, source, || format!("{}{i}", App::new_board_id())) {
                        Ok(Some(_)) => added += 1,
                        Ok(None) => skipped += 1,
                        Err(_) => failed += 1,
                    }
                }
                Done::Imported {
                    added,
                    skipped,
                    failed,
                    list: store::list(&dir),
                }
            }
        };
        if done.send(out).is_err() {
            return;
        }
        wake();
    }
}

// ------------------------------------------------------------ settings

/// How far ahead the live stroke is drawn for each prediction level, in ms.
const PREDICT_MS: [f64; 4] = [0.0, 12.0, 24.0, 36.0];

#[derive(Clone, PartialEq, Debug)]
struct Settings {
    tidy: bool,
    typed: bool,
    palm: bool,
    grid: bool,
    snap: bool,
    font: u8,
    full_res: bool,
    /// Low-latency ink layer.
    fast_ink: bool,
    /// Ink prediction level, 0 (off) to 3.
    predict: u8,
    /// With a stylus, fingers pan instead of drawing.
    pen_priority: bool,
    /// Several fingers can write at once.
    multi_ink: bool,
    lang: String,
    /// Secret part of the Claude (MCP) address.
    mcp_key: String,
    /// This panel's name and password at the relay.
    relay_id: String,
    relay_secret: String,
    relay_url: String,
}

/// A random lowercase token, for keys that only need to be unguessable.
fn random_token(len: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = String::with_capacity(len);
    let mut round = 0u64;
    while out.len() < len {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(round);
        h.write_i64(wall_millis());
        h.write_usize(&out as *const String as usize);
        let mut v = h.finish();
        for _ in 0..10 {
            if out.len() < len {
                out.push(digits[(v % 36) as usize] as char);
                v /= 36;
            }
        }
        round += 1;
    }
    out
}

/// The 6-digit code in what someone typed to join a board.
fn share_code(input: &str) -> String {
    input
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|p| p.len() == 6 && p.chars().all(|c| c.is_ascii_digit()))
        .last()
        .map(str::to_string)
        .unwrap_or_else(|| input.chars().filter(|c| c.is_ascii_digit()).collect())
}

impl Settings {
    fn load(path: &Path) -> Settings {
        let v: Value = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(true);
        let token = |k: &str, min: usize, len: usize| {
            v.get(k)
                .and_then(Value::as_str)
                .filter(|t| {
                    t.len() >= min
                        && t.len() <= 32
                        && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                })
                .map(str::to_string)
                .unwrap_or_else(|| random_token(len))
        };
        Settings {
            tidy: flag("tidy"),
            typed: flag("typed"),
            palm: flag("palm"),
            grid: flag("grid"),
            snap: flag("snap"),
            font: v
                .get("font")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(text::FONT_NAMES.len() as u64 - 1) as u8,
            full_res: v.get("fullRes").and_then(Value::as_bool).unwrap_or(false),
            fast_ink: flag("fastInk"),
            predict: v.get("predict").and_then(Value::as_u64).unwrap_or(1).min(3) as u8,
            pen_priority: v.get("penPriority").and_then(Value::as_bool).unwrap_or(false),
            multi_ink: v.get("multiInk").and_then(Value::as_bool).unwrap_or(false),
            lang: v
                .get("lang")
                .and_then(Value::as_str)
                .filter(|l| LANGS.iter().any(|(c, _)| c == l))
                .unwrap_or("en")
                .to_string(),
            mcp_key: token("mcpKey", 24, 24),
            relay_id: token("relayId", 8, 16),
            relay_secret: token("relaySecret", 16, 32),
            relay_url: v
                .get("relayUrl")
                .and_then(Value::as_str)
                .unwrap_or(RELAY)
                .to_string(),
        }
    }

    fn save(&self, path: &Path) {
        let v = serde_json::json!({
            "tidy": self.tidy, "typed": self.typed, "palm": self.palm, "grid": self.grid,
            "snap": self.snap, "font": self.font, "fullRes": self.full_res, "lang": self.lang,
            "fastInk": self.fast_ink, "predict": self.predict, "penPriority": self.pen_priority,
            "multiInk": self.multi_ink,
            "mcpKey": self.mcp_key, "relayId": self.relay_id, "relaySecret": self.relay_secret,
            "relayUrl": self.relay_url,
        });
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

// -------------------------------------------------------------- the tree

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Opt {
    Grid,
    Snap,
    Tidy,
    Typed,
    Palm,
    FastInk,
    PenPriority,
    MultiInk,
}

/// What a control does.
#[derive(Clone, PartialEq, Debug)]
enum Act {
    Open(String),
    CardMenu(String),
    NewBoard,
    Display,
    Import,
    Updates,
    Back,
    Menu,
    Undo,
    Redo,
    Delete,
    Convert,
    Bigger,
    Smaller,
    Style,
    Color(u32),
    Width(usize),
    Fill(bool),
    ZoomOut,
    ZoomIn,
    Zoom100,
    Fit,
    Tool(usize, PenMode),
    Retry,
    Toggle(Opt),
    Lang(usize),
    FontPanel,
    Font(u8),
    FullRes,
    Predict(u8),
    Rename(String),
    AskDelete(String),
    Field,
    Cancel,
    Done,
    Share,
    StopShare,
    Join,
    Claude,
    Export,
    Copy(String),
    /// A surface that takes touches so they do not fall through.
    Nothing,
    /// The empty area around a menu or dialog.
    Outside,
}

#[derive(Clone, PartialEq, Debug)]
enum Look {
    Surface {
        color: u32,
        radius: f32,
        elev: f32,
        border: bool,
    },
    Btn {
        icon: u32,
        isz: f32,
        active: bool,
        enabled: bool,
    },
    AppIcon {
        icon: u32,
        badge: bool,
    },
    Label {
        text: String,
        face: Face,
        size: f32,
        color: u32,
        center: bool,
    },
    Para {
        text: String,
        face: Face,
        size: f32,
        line: f32,
        color: u32,
        center: bool,
    },
    Glyph {
        icon: u32,
        size: f32,
        color: u32,
    },
    Swatch {
        color: u32,
        chosen: bool,
    },
    Chip {
        text: String,
        chosen: bool,
    },
    StyleDot {
        color: u32,
        open: bool,
    },
    Rule,
    Logo,
    Thumb {
        id: String,
        gen: u64,
    },
    Fab,
    DlgBtn {
        text: String,
        filled: bool,
    },
    Field {
        text: String,
        hint: String,
        multiline: bool,
    },
    Item {
        text: String,
        check: Option<bool>,
    },
    TitleBlock {
        title: String,
        status: String,
        failed: bool,
    },
    Toast(String),
    Scrim(u32),
    /// An address or command, wrapped anywhere, on a tinted box.
    Code(String),
    /// A font's name written in that font.
    FontTile {
        font: u8,
        chosen: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hl {
    None,
    Pressed,
    Focused,
}

#[derive(Clone, PartialEq, Debug)]
struct Node {
    rect: Rf,
    look: Look,
    act: Option<Act>,
    hl: Hl,
}

impl Node {
    /// Pixels the node may paint, shadows and focus rings included.
    fn paint_bounds(&self, d: f32) -> IRect {
        let pad = match &self.look {
            Look::Surface { elev, .. } => 2.0 + 10.0 * d * elev.max(0.0),
            Look::Fab => 2.0 + 10.0 * d * 3.0,
            Look::Toast(_) => 2.0 + 10.0 * d * 2.0,
            _ => 2.0,
        } + if self.hl == Hl::Focused { 4.0 * d } else { 0.0 };
        self.rect.covering(pad)
    }

    fn radius(&self, d: f32) -> f32 {
        d * match &self.look {
            Look::Btn { .. } | Look::Swatch { .. } | Look::Chip { .. } | Look::StyleDot { .. } => {
                8.0
            }
            Look::AppIcon { .. } => 24.0,
            Look::Surface { radius, .. } => *radius,
            Look::Fab => 16.0,
            Look::DlgBtn { .. } => 20.0,
            Look::Item { .. } => 0.0,
            Look::Thumb { .. } => 8.0,
            Look::FontTile { .. } => 10.0,
            _ => 8.0,
        }
    }
}

/// Collects the nodes of one layout.
struct Ui<'a> {
    nodes: Vec<Node>,
    d: f32,
    pressed: Option<&'a Act>,
    focus: Option<&'a Act>,
}

impl<'a> Ui<'a> {
    fn push(&mut self, rect: Rf, look: Look, act: Option<Act>) {
        let hl = match &act {
            Some(a) if Some(a) == self.pressed => Hl::Pressed,
            Some(a) if Some(a) == self.focus => Hl::Focused,
            _ => Hl::None,
        };
        self.nodes.push(Node {
            rect,
            look,
            act,
            hl,
        });
    }

    /// A floating toolbar panel.
    fn panel(&mut self, r: Rf) {
        self.push(
            r,
            Look::Surface {
                color: WHITE,
                radius: 11.0,
                elev: 3.0,
                border: true,
            },
            None,
        );
    }

    /// A toolbar button of `size` (margin included) with its top left at (x, y).
    fn btn(&mut self, x: f32, y: f32, size: f32, icon: u32, active: bool, enabled: bool, act: Act) {
        let d = self.d;
        let r = Rf::new(x + d, y + d, size - 2.0 * d, size - 2.0 * d);
        self.push(
            r,
            Look::Btn {
                icon,
                isz: (size - 2.0 * d) * 0.5,
                active,
                enabled,
            },
            if enabled { Some(act) } else { None },
        );
    }
}

// -------------------------------------------------------------- overlays

#[derive(Clone, PartialEq, Debug)]
enum MenuKind {
    Main,
    Display,
    Card(String),
}

enum Entry {
    Item(String, Option<bool>, Act),
    Rule,
}

#[derive(Clone, Debug)]
enum AskFor {
    NewBoard,
    Rename(String),
    Join,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PanelKind {
    Share,
    Claude,
    Fonts,
}

#[derive(Clone, Debug)]
enum ConfirmFor {
    Delete(String),
    Install(String),
}

enum Overlay {
    Menu {
        kind: MenuKind,
        anchor: Rf,
        right: bool,
    },
    Ask {
        title: String,
        text: String,
        hint: String,
        multiline: bool,
        purpose: AskFor,
    },
    Confirm {
        title: String,
        body: String,
        cancel: String,
        ok: String,
        purpose: ConfirmFor,
    },
    /// A larger sheet with scrolling content.
    Panel {
        kind: PanelKind,
        scroll: f32,
    },
}

// ----------------------------------------------------------------- board

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SaveState {
    Saved,
    Saving,
    Failed,
}

struct OpenBoard {
    id: String,
    title: String,
    extra: Map<String, Value>,
}

struct Thumb {
    updated: String,
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    gen: u64,
}

enum Capture {
    Editor,
    /// Pressing a control; on the library a drag turns into scrolling.
    Control {
        act: Act,
        start: (f32, f32),
        last_y: f32,
        scrolling: bool,
    },
    Scroll {
        last_y: f32,
    },
    /// Pressed outside an open menu or dialog.
    Outside,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Library,
    Board,
}

pub struct App {
    pub ed: Editor,
    platform: Box<dyn Platform>,
    version: String,
    utc_offset_min: i32,
    settings_path: PathBuf,
    settings: Settings,

    jobs: Sender<Job>,
    done: Receiver<Done>,

    w: u32,
    h: u32,
    d: f32,
    out: Layer,
    nodes: Vec<Node>,
    full: bool,
    now: u64,

    screen: Screen,
    boards: Vec<BoardMeta>,
    listed: bool,
    scroll: f32,
    thumbs: HashMap<String, Thumb>,
    thumbs_wanted: HashSet<String>,
    thumb_gen: u64,

    board: Option<OpenBoard>,
    opening: Option<String>,
    save_state: SaveState,
    unsaved: bool,
    save_due: Option<u64>,
    save_seq: u64,
    close_after_save: bool,
    style_open: bool,

    overlay: Option<Overlay>,
    toast: Option<(String, u64)>,
    update: Option<(String, String)>,
    update_offered: bool,

    captures: HashMap<i32, Capture>,
    pressed: Option<Act>,
    focus: Option<Act>,
    ink: HashMap<u32, Vec<String>>,

    wake: Arc<dyn Fn() + Send + Sync>,
    /// Kept so the request channel and the listener live as long as the app.
    #[allow(dead_code)]
    net_tx: Sender<net::Req>,
    net_rx: Receiver<net::Req>,
    #[allow(dead_code)]
    server: Option<net::Server>,
    relay: Option<net::Relay>,
    /// Counts changes to the open board; joiners poll by it.
    doc_gen: u64,
    /// A board joined from another device.
    link: Option<net::Link>,
    known: HashMap<String, Value>,
    ops_sent: u64,
    link_online: Option<bool>,
    joining: Option<Receiver<(String, net::Found)>>,
    lan_ip: Option<std::net::Ipv4Addr>,
    font_tiles: RefCell<HashMap<(u8, u32), Option<text::TextBitmap>>>,
}

/// Floats per stroke point in an ink request.
fn ink_data(token: u32, strokes: &[Vec<Pt>], origin: Pt) -> Vec<f32> {
    let mut out = vec![token as f32, strokes.len() as f32];
    for s in strokes {
        out.push(s.len() as f32);
        for p in s {
            out.push((p.x - origin.x) as f32);
            out.push((p.y - origin.y) as f32);
        }
    }
    out
}

/// True when dotted version `a` is higher than `b`.
pub fn newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| {
        s.split(['.', '-', '+'])
            .map(|p| p.parse::<i64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let (x, y) = (parse(a), parse(b));
    for i in 0..x.len().max(y.len()) {
        let d = x.get(i).copied().unwrap_or(0) - y.get(i).copied().unwrap_or(0);
        if d != 0 {
            return d > 0;
        }
    }
    false
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

fn wall_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn base36(mut v: u64) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(digits[(v % 36) as usize]);
        v /= 36;
        if v == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

impl App {
    /// `dir` holds the boards, `settings_path` the app's settings.
    /// `wake` must make the host call [`App::frame`] soon; it is called from
    /// a background thread when disk work finishes.
    pub fn new(
        platform: Box<dyn Platform>,
        dir: PathBuf,
        settings_path: PathBuf,
        version: String,
        utc_offset_min: i32,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> App {
        let _ = std::fs::create_dir_all(&dir);
        let (jobs, job_rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let worker_dir = dir.clone();
        let disk_wake = wake.clone();
        let _ = std::thread::Builder::new()
            .name("boards".into())
            .spawn(move || disk_worker(worker_dir, job_rx, done_tx, disk_wake));
        let settings = Settings::load(&settings_path);
        settings.save(&settings_path);
        let (net_tx, net_rx) = mpsc::channel();
        let (server, relay) = if cfg!(test) {
            (None, None)
        } else {
            let relay = (!settings.relay_url.is_empty()).then(|| {
                net::relay(
                    settings.relay_url.clone(),
                    settings.relay_id.clone(),
                    settings.relay_secret.clone(),
                    net_tx.clone(),
                    wake.clone(),
                )
            });
            (net::serve(net_tx.clone(), wake.clone()).ok(), relay)
        };
        let mut app = App {
            ed: Editor::new(),
            platform,
            version,
            utc_offset_min,
            settings_path,
            settings,
            jobs,
            done,
            w: 0,
            h: 0,
            d: 1.0,
            out: Layer::new(0, 0),
            nodes: Vec::new(),
            full: true,
            now: 0,
            screen: Screen::Library,
            boards: Vec::new(),
            listed: false,
            scroll: 0.0,
            thumbs: HashMap::new(),
            thumbs_wanted: HashSet::new(),
            thumb_gen: 0,
            board: None,
            opening: None,
            save_state: SaveState::Saved,
            unsaved: false,
            save_due: None,
            save_seq: 0,
            close_after_save: false,
            style_open: false,
            overlay: None,
            toast: None,
            update: None,
            update_offered: false,
            captures: HashMap::new(),
            pressed: None,
            focus: None,
            ink: HashMap::new(),
            wake,
            net_tx,
            net_rx,
            server,
            relay,
            doc_gen: 1,
            link: None,
            known: HashMap::new(),
            ops_sent: 0,
            link_online: None,
            joining: None,
            lan_ip: None,
            font_tiles: RefCell::new(HashMap::new()),
        };
        app.apply_settings();
        app.ed.set_tool(Tool::Pen, PenMode::Ink);
        if app.settings.typed {
            let lang = app.settings.lang.clone();
            app.platform.prepare_handwriting(&lang);
        }
        app.platform.set_full_resolution(app.settings.full_res);
        app.platform.set_fast_ink(app.settings.fast_ink);
        app.platform.check_updates(false);
        app.send(Job::List);
        app
    }

    fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    fn apply_settings(&mut self) {
        let s = self.settings.clone();
        self.ed.set_auto_convert(s.tidy);
        self.ed.set_auto_text(s.typed);
        self.ed.set_palm_rejection(s.palm, None);
        self.ed.set_pen_priority(s.pen_priority);
        self.ed.set_multi_ink(s.multi_ink);
        self.ed.set_prediction(PREDICT_MS[s.predict.min(3) as usize]);
        self.ed.set_grid(s.grid);
        self.ed.set_snap_shapes(s.snap);
        self.ed.set_font(s.font);
    }

    fn timestamp(&self) -> String {
        let ms = wall_millis() + self.utc_offset_min as i64 * 60_000;
        let secs = ms.div_euclid(1000);
        let (y, m, d) = civil(secs.div_euclid(86_400));
        let sod = secs.rem_euclid(86_400);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}",
            sod / 3600,
            sod % 3600 / 60,
            sod % 60,
            ms.rem_euclid(1000)
        )
    }

    fn show_toast(&mut self, text: &str) {
        self.toast = Some((text.to_string(), self.now + 2800));
    }

    /// A message from the host, shown like the app's own.
    pub fn toast(&mut self, text: &str) {
        self.show_toast(text);
    }

    /// Board files the user picked after [`Platform::pick_boards`].
    pub fn import_boards(&mut self, files: Vec<String>) {
        if files.is_empty() {
            return;
        }
        self.show_toast("Importing boards…");
        self.send(Job::Import(files));
    }

    pub fn is_board_open(&self) -> bool {
        self.screen == Screen::Board
    }

    // ------------------------------------------------------------- frame

    /// Sets the drawing buffer size and how many buffer pixels make one dp.
    pub fn resize(&mut self, w: u32, h: u32, d: f32) {
        self.w = w;
        self.h = h;
        self.d = d.max(0.5);
        self.out = Layer::new(w, h);
        self.ed.resize(w, h, self.d as f64);
        self.ed.invalidate_all();
        self.full = true;
        self.clamp_scroll();
    }

    /// The finished picture; [`App::frame`] says which part changed.
    pub fn output(&self) -> &Layer {
        &self.out
    }

    /// Synchronize timers before handling an asynchronous platform callback.
    pub fn advance_clock(&mut self, now: u64) {
        self.now = self.now.max(now);
    }

    /// When [`App::frame`] next needs to run even without input.
    pub fn next_deadline(&self) -> Option<u64> {
        [
            self.ed.next_deadline(),
            self.save_due,
            self.toast.as_ref().map(|t| t.1),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Brings everything up to date at `now` (host milliseconds) and
    /// repaints. Returns the area of [`App::output`] that changed.
    pub fn frame(&mut self, now: u64) -> Option<IRect> {
        self.now = self.now.max(now);
        self.take_disk();
        self.take_net();
        self.take_link();
        self.take_join();
        self.ed.tick(self.now);
        self.take_editor_events();
        if self.toast.as_ref().is_some_and(|t| self.now >= t.1) {
            self.toast = None;
        }
        if self.save_due.is_some_and(|due| self.now >= due) {
            if self.ed.pointer_down_now() {
                self.save_due = Some(self.now + 700);
            } else {
                self.save_due = None;
                self.queue_save(None);
            }
        }
        if self.w == 0 || self.h == 0 {
            return None;
        }

        let editor_dirty = if self.screen == Screen::Board {
            self.ed.render()
        } else {
            None
        };
        let fresh = self.layout();
        let mut regions: Vec<IRect> = Vec::new();
        if self.full {
            regions.push(self.out.bounds());
            self.full = false;
        } else {
            if let Some(r) = editor_dirty {
                regions.push(r);
            }
            if let Some(r) = self.diff(&fresh) {
                regions.push(r);
            }
        }
        self.nodes = fresh;
        self.want_thumbs();

        let bounds = self.out.bounds();
        let mut total: Option<IRect> = None;
        for r in regions {
            let r = r.intersect(&bounds);
            if r.is_empty() {
                continue;
            }
            self.compose(r);
            total = Some(total.map_or(r, |t| t.union(&r)));
        }
        total
    }

    /// Area covered by nodes that differ between the shown tree and `fresh`.
    fn diff(&self, fresh: &[Node]) -> Option<IRect> {
        let d = self.d;
        let mut dirty: Option<IRect> = None;
        let mut add = |r: IRect| dirty = Some(dirty.map_or(r, |t: IRect| t.union(&r)));
        let n = self.nodes.len().max(fresh.len());
        for i in 0..n {
            match (self.nodes.get(i), fresh.get(i)) {
                (Some(a), Some(b)) if a == b => {}
                (a, b) => {
                    if let Some(a) = a {
                        add(a.paint_bounds(d));
                    }
                    if let Some(b) = b {
                        add(b.paint_bounds(d));
                    }
                }
            }
        }
        dirty
    }

    fn compose(&mut self, r: IRect) {
        let (w, h) = (r.width() as u32, r.height() as u32);
        let frame = self.ed.frame();
        let mut buf =
            if self.screen == Screen::Board && frame.w == self.out.w && frame.h == self.out.h {
                frame.read(r)
            } else {
                let px = [(PAGE >> 16) as u8, (PAGE >> 8) as u8, PAGE as u8, 0xff];
                px.repeat(w as usize * h as usize)
            };
        if let Some(pm) = PixmapMut::from_bytes(&mut buf, w, h) {
            let mut p = Painter {
                pm,
                ox: r.x0 as f32,
                oy: r.y0 as f32,
            };
            for node in &self.nodes {
                if node.paint_bounds(self.d).intersects(&r) {
                    self.draw(&mut p, node);
                }
            }
        }
        self.out.write(r, &buf);
    }

    // ------------------------------------------------------------- disk

    fn take_disk(&mut self) {
        while let Ok(done) = self.done.try_recv() {
            match done {
                Done::Listed(list) => {
                    self.boards = list;
                    self.listed = true;
                    self.clamp_scroll();
                }
                Done::Imported {
                    added,
                    skipped,
                    failed,
                    list,
                } => {
                    self.boards = list;
                    self.listed = true;
                    self.clamp_scroll();
                    let plural = |n: usize| if n == 1 { "board" } else { "boards" };
                    let mut text = format!("Imported {added} {}", plural(added));
                    if skipped > 0 {
                        text += &format!(", {skipped} already here");
                    }
                    if failed > 0 {
                        text += &format!(", {failed} not readable");
                    }
                    self.show_toast(&text);
                }
                Done::Thumb {
                    id,
                    updated,
                    w,
                    h,
                    rgba,
                } => {
                    self.thumbs_wanted.remove(&id);
                    self.thumb_gen += 1;
                    self.thumbs.insert(
                        id,
                        Thumb {
                            updated,
                            w,
                            h,
                            rgba,
                            gen: self.thumb_gen,
                        },
                    );
                }
                Done::Loaded(id, result) => {
                    if self.opening.as_deref() != Some(id.as_str()) {
                        continue;
                    }
                    self.opening = None;
                    match result.map_err(|e| e).and_then(|board| {
                        let title = board.title.clone();
                        let extra = board.extra.clone();
                        self.ed.load(board.shapes).map(|_| (title, extra))
                    }) {
                        Ok((title, extra)) => {
                            self.ink.clear();
                            self.ed.fit();
                            self.ed.take_events();
                            self.ed.invalidate_all();
                            self.board = Some(OpenBoard { id, title, extra });
                            self.screen = Screen::Board;
                            self.save_state = SaveState::Saved;
                            self.unsaved = false;
                            self.save_due = None;
                            self.style_open = false;
                            self.focus = None;
                            self.full = true;
                            self.doc_gen += 1;
                            self.start_link();
                            self.sync_relay_codes();
                        }
                        Err(_) => self.show_toast("This board could not be opened"),
                    }
                }
                Done::Saved { seq, ok } => {
                    if seq == self.save_seq {
                        self.save_state = if ok {
                            SaveState::Saved
                        } else {
                            SaveState::Failed
                        };
                        if !ok {
                            self.unsaved = true;
                        }
                        if self.close_after_save {
                            self.close_after_save = false;
                            if ok {
                                self.return_to_library();
                            } else {
                                self.show_toast("Could not save. Your board is still open; tap the save status to retry.");
                            }
                        }
                    } else if !ok {
                        self.save_state = SaveState::Failed;
                    }
                }
                Done::Created(id, ok) => {
                    if ok {
                        self.open(id);
                    } else {
                        self.show_toast("Could not create the board");
                    }
                }
            }
        }
    }

    fn open(&mut self, id: String) {
        self.opening = Some(id.clone());
        self.send(Job::Load(id));
    }

    fn queue_save(&mut self, ack: Option<SyncSender<()>>) {
        let Some(board) = &self.board else {
            if let Some(ack) = ack {
                let _ = ack.send(());
            }
            return;
        };
        self.save_seq += 1;
        self.unsaved = false;
        self.save_state = SaveState::Saving;
        let job = Job::Save {
            id: board.id.clone(),
            title: board.title.clone(),
            updated: self.timestamp(),
            shapes: self.ed.doc.shapes().to_vec(),
            extra: board.extra.clone(),
            seq: self.save_seq,
            ack,
        };
        self.send(job);
    }

    /// The host is going to the background. `ack` is answered once the open
    /// board is safely on disk.
    pub fn pause(&mut self, ack: SyncSender<()>) {
        self.ed.finish_input(self.now);
        self.take_editor_events();
        self.captures.clear();
        self.pressed = None;
        if self.board.is_some() {
            self.save_due = None;
            self.queue_save(Some(ack));
        } else {
            let _ = ack.send(());
        }
    }

    fn close_board(&mut self) {
        if self.close_after_save {
            return;
        }
        self.ed.finish_input(self.now);
        self.take_editor_events();
        if self.unsaved
            || self.save_due.is_some()
            || matches!(self.save_state, SaveState::Saving | SaveState::Failed)
        {
            self.save_due = None;
            self.close_after_save = true;
            self.queue_save(None);
            return;
        }
        self.return_to_library();
    }

    fn return_to_library(&mut self) {
        self.board = None;
        self.link = None;
        self.known.clear();
        self.link_online = None;
        self.sync_relay_codes();
        self.screen = Screen::Library;
        self.overlay = None;
        self.focus = None;
        self.full = true;
        self.send(Job::List);
    }

    fn want_thumbs(&mut self) {
        let mut wanted = Vec::new();
        for node in &self.nodes {
            if let Look::Thumb { id, .. } = &node.look {
                let (w, h) = (node.rect.w.round() as u32, node.rect.h.round() as u32);
                if w == 0 || h == 0 || w > 2048 || h > 2048 || self.thumbs_wanted.contains(id) {
                    continue;
                }
                let Some(meta) = self.boards.iter().find(|b| &b.id == id) else {
                    continue;
                };
                let fresh = self
                    .thumbs
                    .get(id)
                    .is_some_and(|t| t.updated == meta.updated && t.w == w && t.h == h);
                if !fresh {
                    wanted.push((id.clone(), meta.updated.clone(), w, h));
                }
            }
        }
        for (id, updated, w, h) in wanted {
            self.thumbs_wanted.insert(id.clone());
            self.send(Job::Thumb { id, updated, w, h });
        }
    }

    // ------------------------------------------------------------ editor

    fn take_editor_events(&mut self) {
        let events = self.ed.take_events();
        if events & EV_HAPTIC != 0 {
            self.platform.haptic();
        }
        if events & EV_DOC != 0 && self.board.is_some() {
            self.unsaved = true;
            self.save_due = Some(self.now + 1200);
            self.doc_gen += 1;
            if self.link.is_some() {
                let now = self.shape_map();
                let ops = net::diff(&self.known, &now);
                if !ops.is_empty() {
                    if let Some(link) = &self.link {
                        let _ = link.ops.send(ops);
                    }
                    self.ops_sent += 1;
                    self.known = now;
                }
            }
        }
        if events & EV_TEXT != 0 {
            // Text comes from handwriting; there is no typing on the board.
            let _ = self.ed.take_text_request();
        }
        if events & EV_INK != 0 {
            while let Some(request) = self.ed.take_ink_request() {
                let data = ink_data(
                    request.token,
                    &request.strokes,
                    Pt::new(request.bounds.l, request.bounds.t),
                );
                if self.ink.len() > 64 {
                    self.ink.clear();
                }
                self.ink.insert(request.token, request.ids);
                let lang = self.settings.lang.clone();
                self.platform.read_ink(request.token, &lang, &data);
            }
        }
    }

    /// The text handwriting `token` reads as; empty leaves the ink alone.
    pub fn ink_result(&mut self, token: u32, text: &str) {
        if let Some(ids) = self.ink.remove(&token) {
            self.ed.commit_ink(&ids, text);
        }
    }

    /// The answer to [`Platform::check_updates`]: the newest release's
    /// version and download address, if one was found.
    pub fn update_result(&mut self, found: Option<(String, String)>, failed: bool, manual: bool) {
        match found {
            Some((version, url)) if newer(&version, &self.version) => {
                let offer = manual || !self.update_offered;
                self.update = Some((version, url));
                if offer {
                    self.offer_update();
                }
            }
            _ => {
                if manual {
                    let text = if failed {
                        "Could not check for updates".to_string()
                    } else {
                        format!("You have the latest version ({})", self.version)
                    };
                    self.show_toast(&text);
                }
            }
        }
    }

    fn offer_update(&mut self) {
        let Some((version, url)) = self.update.clone() else {
            return;
        };
        if self.overlay.is_some() {
            return;
        }
        self.update_offered = true;
        self.overlay = Some(Overlay::Confirm {
            title: "Update available".into(),
            body: format!("Version {version} is ready. You have {}.", self.version),
            cancel: "Later".into(),
            ok: "Update".into(),
            purpose: ConfirmFor::Install(url),
        });
    }

    // ------------------------------------------------------------- input

    pub fn touch(&mut self, phase: Phase, samples: &[Sample]) {
        for s in samples {
            self.now = self.now.max(s.time);
            match phase {
                Phase::Down => self.down(s),
                Phase::Move => self.moved(s),
                Phase::Up => self.up(s, false),
                Phase::Cancel => self.up(s, true),
            }
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<Act> {
        self.nodes
            .iter()
            .rev()
            .find(|n| n.act.is_some() && n.rect.contains(x, y))
            .and_then(|n| n.act.clone())
    }

    fn contact(s: &Sample) -> Contact {
        Contact {
            size: s.size.max(0.0) as f64,
            stylus: s.stylus,
            eraser: s.eraser,
        }
    }

    fn down(&mut self, s: &Sample) {
        if self.close_after_save {
            return;
        }
        self.focus = None;
        let hit = self.hit(s.x, s.y);
        let capture = match hit {
            Some(Act::Outside) | None if self.overlay.is_some() => Capture::Outside,
            Some(act) => {
                self.pressed = Some(act.clone());
                Capture::Control {
                    act,
                    start: (s.x, s.y),
                    last_y: s.y,
                    scrolling: false,
                }
            }
            None if self.screen == Screen::Board => {
                self.ed.pointer_down_with(
                    s.id,
                    Pt::new(s.x as f64, s.y as f64),
                    s.time,
                    Self::contact(s),
                );
                Capture::Editor
            }
            None => Capture::Scroll { last_y: s.y },
        };
        self.captures.insert(s.id, capture);
    }

    fn moved(&mut self, s: &Sample) {
        let d = self.d;
        let panel = matches!(self.overlay, Some(Overlay::Panel { .. }));
        let library = (self.screen == Screen::Library && self.overlay.is_none()) || panel;
        let mut scroll_by = 0.0;
        let mut release = false;
        match self.captures.get_mut(&s.id) {
            Some(Capture::Editor) => {
                self.ed.pointer_move_with(
                    s.id,
                    Pt::new(s.x as f64, s.y as f64),
                    s.time,
                    Self::contact(s),
                );
            }
            Some(Capture::Scroll { last_y }) => {
                scroll_by = *last_y - s.y;
                *last_y = s.y;
            }
            Some(Capture::Control {
                start,
                last_y,
                scrolling,
                ..
            }) => {
                let far = (s.x - start.0).hypot(s.y - start.1) > 10.0 * d;
                if library && (far || *scrolling) {
                    *scrolling = true;
                    scroll_by = *last_y - s.y;
                    release = true;
                }
                *last_y = s.y;
                if far && !library {
                    release = true;
                }
            }
            _ => {}
        }
        if release {
            self.pressed = None;
        }
        if scroll_by != 0.0 {
            if panel {
                let max = self.panel_max_scroll();
                if let Some(Overlay::Panel { scroll, .. }) = &mut self.overlay {
                    *scroll = (*scroll + scroll_by).clamp(0.0, max);
                }
            } else {
                self.scroll += scroll_by;
                self.clamp_scroll();
            }
        }
    }

    fn up(&mut self, s: &Sample, cancelled: bool) {
        let Some(capture) = self.captures.remove(&s.id) else {
            return;
        };
        match capture {
            Capture::Editor => {
                self.ed
                    .pointer_up(s.id, Pt::new(s.x as f64, s.y as f64), s.time, cancelled)
            }
            Capture::Control { act, scrolling, .. } => {
                let still = self.pressed.as_ref() == Some(&act);
                self.pressed = None;
                if !cancelled && !scrolling && still && self.hit(s.x, s.y).as_ref() == Some(&act) {
                    self.run(act);
                }
            }
            Capture::Outside => {
                if !cancelled && matches!(self.hit(s.x, s.y), Some(Act::Outside) | None) {
                    self.dismiss();
                }
            }
            Capture::Scroll { .. } => {}
        }
    }

    /// The host's back button. Returns false when the app should close.
    pub fn back(&mut self) -> bool {
        if self.overlay.is_some() {
            self.dismiss();
            true
        } else if self.screen == Screen::Board {
            self.close_board();
            true
        } else {
            false
        }
    }

    pub fn request_back(&mut self) {
        if !self.back() {
            self.platform.exit();
        }
    }

    /// A remote control or keyboard key. Returns true when it was used.
    pub fn key(&mut self, code: i32) -> bool {
        let dir = match code {
            keys::UP => (0.0, -1.0),
            keys::DOWN => (0.0, 1.0),
            keys::LEFT => (-1.0, 0.0),
            keys::RIGHT => (1.0, 0.0),
            keys::CENTER | keys::ENTER | keys::NUMPAD_ENTER => {
                if let Some(act) = self.focus.clone() {
                    self.run(act);
                    return true;
                }
                return false;
            }
            _ => return false,
        };
        let candidates: Vec<&Node> = self
            .nodes
            .iter()
            .filter(|n| n.act.is_some() && n.act != Some(Act::Outside) && n.act != Some(Act::Nothing))
            .collect();
        if candidates.is_empty() {
            return false;
        }
        let current = self
            .focus
            .as_ref()
            .and_then(|f| candidates.iter().find(|n| n.act.as_ref() == Some(f)));
        let next = match current {
            None => candidates.first().map(|n| (n.act.clone(), n.rect)),
            Some(cur) => {
                let (cx, cy) = (cur.rect.cx(), cur.rect.cy());
                candidates
                    .iter()
                    .filter(|n| n.act != cur.act)
                    .filter_map(|n| {
                        let (dx, dy) = (n.rect.cx() - cx, n.rect.cy() - cy);
                        let along = dx * dir.0 + dy * dir.1;
                        let across = (dx * dir.1 - dy * dir.0).abs();
                        if along <= 1.0 {
                            None
                        } else {
                            Some((along + across * 2.0, n))
                        }
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, n)| (n.act.clone(), n.rect))
            }
        };
        if let Some((act, rect)) = next {
            self.focus = act;
            if self.screen == Screen::Library && self.overlay.is_none() {
                // Keep the focused card on screen.
                let top = 64.0 * self.d;
                if rect.y < top {
                    self.scroll -= top - rect.y + 16.0 * self.d;
                } else if rect.bottom() > self.h as f32 {
                    self.scroll += rect.bottom() - self.h as f32 + 16.0 * self.d;
                }
                self.clamp_scroll();
            }
        }
        true
    }

    /// Text typed into the open field. `submit` is the keyboard's done key.
    pub fn text_input(&mut self, value: &str, submit: bool) {
        if let Some(Overlay::Ask {
            text, multiline, ..
        }) = &mut self.overlay
        {
            *text = value
                .chars()
                .take(if *multiline { 2000 } else { 300 })
                .collect();
            if submit && !*multiline {
                self.run(Act::Done);
            }
        }
    }

    // ------------------------------------------------------------ actions

    fn ask(&mut self, title: &str, text: &str, hint: &str, multiline: bool, purpose: AskFor) {
        self.overlay = Some(Overlay::Ask {
            title: title.into(),
            text: text.into(),
            hint: hint.into(),
            multiline,
            purpose,
        });
        self.focus = None;
        self.platform.show_keyboard(text, multiline);
    }

    fn dismiss(&mut self) {
        if let Some(Overlay::Ask { .. }) = self.overlay {
            self.platform.hide_keyboard();
        }
        self.overlay = None;
        self.focus = None;
    }

    fn menu(&mut self, kind: MenuKind, anchor: Rf, right: bool) {
        self.overlay = Some(Overlay::Menu {
            kind,
            anchor,
            right,
        });
        self.focus = None;
    }

    fn node_rect(&self, act: &Act) -> Rf {
        self.nodes
            .iter()
            .find(|n| n.act.as_ref() == Some(act))
            .map(|n| n.rect)
            .unwrap_or_default()
    }

    fn set(&mut self, f: impl FnOnce(&mut Settings)) {
        let before = self.settings.clone();
        f(&mut self.settings);
        if self.settings != before {
            self.settings.save(&self.settings_path);
            self.apply_settings();
            if self.settings.typed && (!before.typed || before.lang != self.settings.lang) {
                let lang = self.settings.lang.clone();
                self.platform.prepare_handwriting(&lang);
            }
            if self.settings.fast_ink != before.fast_ink {
                self.platform.set_fast_ink(self.settings.fast_ink);
            }
            if self.settings.full_res != before.full_res {
                self.platform.set_full_resolution(self.settings.full_res);
            }
        }
    }

    fn run(&mut self, act: Act) {
        let in_menu = matches!(self.overlay, Some(Overlay::Menu { .. }));
        match act {
            Act::Open(id) => self.open(id),
            Act::CardMenu(id) => {
                let r = self.node_rect(&Act::CardMenu(id.clone()));
                self.menu(MenuKind::Card(id), r, true);
            }
            Act::NewBoard => self.ask(
                "A new space for ideas",
                "",
                "Give it a name",
                false,
                AskFor::NewBoard,
            ),
            Act::Display => {
                let r = self.node_rect(&Act::Display);
                self.menu(MenuKind::Display, r, true);
            }
            Act::Import => {
                if in_menu {
                    self.dismiss();
                }
                self.platform.pick_boards();
            }
            Act::Updates => {
                if self.update.is_some() {
                    self.offer_update();
                } else {
                    self.show_toast("Checking for updates…");
                    self.platform.check_updates(true);
                }
            }
            Act::Back => self.close_board(),
            Act::Menu => {
                let r = self.node_rect(&Act::Menu);
                self.menu(MenuKind::Main, r, false);
            }
            Act::Undo => self.ed.undo(),
            Act::Redo => self.ed.redo(),
            Act::Delete => self.ed.delete_selection(),
            Act::Convert => self.ed.convert_selection(),
            Act::Bigger => self.ed.scale_text(1.25),
            Act::Smaller => self.ed.scale_text(0.8),
            Act::Style => self.style_open = !self.style_open,
            Act::Color(c) => self.ed.apply_style(Some(c), None, None),
            Act::Width(i) => self.ed.apply_style(None, Some(SIZES[i].1), None),
            Act::Fill(f) => self.ed.apply_style(None, None, Some(f)),
            Act::ZoomOut => {
                let z = self.ed.zoom() / 1.25;
                self.ed.zoom_to(z, None);
            }
            Act::ZoomIn => {
                let z = self.ed.zoom() * 1.25;
                self.ed.zoom_to(z, None);
            }
            Act::Zoom100 => self.ed.zoom_to(1.0, None),
            Act::Fit => {
                self.ed.fit();
                if in_menu {
                    self.dismiss();
                }
            }
            Act::Tool(i, mode) => {
                if let Some(tool) = Tool::from_index(i as i32) {
                    self.ed.set_tool(tool, mode);
                    if tool == Tool::Pen && mode == PenMode::Write {
                        let lang = self.settings.lang.clone();
                        self.platform.prepare_handwriting(&lang);
                    }
                }
            }
            Act::Retry => self.queue_save(None),
            Act::Toggle(opt) => {
                self.set(|s| match opt {
                    Opt::Grid => s.grid = !s.grid,
                    Opt::Snap => s.snap = !s.snap,
                    Opt::Tidy => s.tidy = !s.tidy,
                    Opt::Typed => s.typed = !s.typed,
                    Opt::Palm => s.palm = !s.palm,
                    Opt::FastInk => s.fast_ink = !s.fast_ink,
                    Opt::PenPriority => s.pen_priority = !s.pen_priority,
                    Opt::MultiInk => s.multi_ink = !s.multi_ink,
                });
                self.dismiss();
            }
            Act::Lang(i) => {
                self.set(|s| s.lang = LANGS[i].0.to_string());
                self.dismiss();
            }
            Act::FontPanel => self.panel(PanelKind::Fonts),
            Act::Font(f) => {
                self.set(|s| s.font = f);
                self.dismiss();
            }
            Act::Predict(level) => {
                self.set(|s| s.predict = level.min(3));
                self.dismiss();
            }
            Act::FullRes => {
                self.set(|s| s.full_res = !s.full_res);
                self.dismiss();
            }
            Act::Rename(id) => {
                self.overlay = None;
                let title = self
                    .boards
                    .iter()
                    .find(|b| b.id == id)
                    .map(|b| b.title.clone())
                    .unwrap_or_default();
                self.ask(
                    "Rename board",
                    &title,
                    "Give it a name",
                    false,
                    AskFor::Rename(id),
                );
            }
            Act::AskDelete(id) => {
                let title = self
                    .boards
                    .iter()
                    .find(|b| b.id == id)
                    .map(|b| b.title.clone())
                    .unwrap_or_default();
                self.overlay = Some(Overlay::Confirm {
                    title: "Delete this board?".into(),
                    body: format!("“{title}” will be removed from this device."),
                    cancel: "Keep".into(),
                    ok: "Delete".into(),
                    purpose: ConfirmFor::Delete(id),
                });
            }
            Act::Field => {
                if let Some(Overlay::Ask {
                    text, multiline, ..
                }) = &self.overlay
                {
                    let (text, multiline) = (text.clone(), *multiline);
                    self.platform.show_keyboard(&text, multiline);
                }
            }
            Act::Cancel | Act::Outside => self.dismiss(),
            Act::Done => self.done(),
            Act::Share => self.share(),
            Act::StopShare => {
                if let Some(b) = &mut self.board {
                    b.extra.remove("lanHost");
                    self.unsaved = true;
                    self.save_due = Some(self.now + 300);
                }
                self.sync_relay_codes();
                self.dismiss();
                self.show_toast("Sharing stopped");
            }
            Act::Join => self.ask(
                "Join a shared board",
                "",
                "Board code, like 123456",
                false,
                AskFor::Join,
            ),
            Act::Claude => {
                self.lan_ip = net::local_ip();
                self.panel(PanelKind::Claude);
            }
            Act::Export => {
                if in_menu {
                    self.dismiss();
                }
                let title = self.board.as_ref().map(|b| b.title.clone()).unwrap_or_default();
                match board_png(self.ed.doc.shapes(), 4096) {
                    Some(png) => self.platform.save_png(&title, &png),
                    None => self.show_toast("Could not make the picture"),
                }
            }
            Act::Copy(text) => {
                self.platform.copy_text(&text);
                self.show_toast("Copied");
            }
            Act::Nothing => {}
        }
    }

    fn panel(&mut self, kind: PanelKind) {
        self.overlay = Some(Overlay::Panel { kind, scroll: 0.0 });
        self.focus = None;
    }

    fn share(&mut self) {
        let Some(board) = &mut self.board else {
            return;
        };
        if !board.extra.contains_key("lanShare") && !board.extra.contains_key("lanHost") {
            let n = random_token(12).bytes().fold(0u64, |a, b| a.wrapping_mul(37).wrapping_add(b as u64));
            let code = format!("{:06}", 100_000 + n % 900_000);
            board.extra.insert("lanHost".into(), Value::String(code));
            self.unsaved = true;
            self.save_due = Some(self.now + 300);
            self.sync_relay_codes();
        }
        self.lan_ip = net::local_ip();
        self.panel(PanelKind::Share);
    }

    fn new_board_id() -> String {
        let salt = (wall_millis() as u64)
            .wrapping_mul(6_364_136_223_846_793_005)
            .rotate_left(17)
            % 46_656;
        format!("b{}{}", base36(wall_millis() as u64), base36(salt))
    }

    fn done(&mut self) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        self.focus = None;
        match overlay {
            Overlay::Ask { text, purpose, .. } => {
                self.platform.hide_keyboard();
                let name = text.trim();
                match purpose {
                    AskFor::NewBoard => {
                        let title = if name.is_empty() {
                            "Untitled board".to_string()
                        } else {
                            name.to_string()
                        };
                        let id = Self::new_board_id();
                        let updated = self.timestamp();
                        self.send(Job::Create {
                            id,
                            title,
                            updated,
                            extra: Map::new(),
                        });
                    }
                    AskFor::Rename(id) => {
                        if !name.is_empty() {
                            let updated = self.timestamp();
                            self.send(Job::Rename {
                                id,
                                title: name.to_string(),
                                updated,
                            });
                        }
                    }
                    AskFor::Join => {
                        let input = name.to_string();
                        let relay = self.settings.relay_url.clone();
                        let wake = self.wake.clone();
                        let (tx, rx) = mpsc::channel();
                        self.joining = Some(rx);
                        std::thread::spawn(move || {
                            let found = net::discover(&input, &relay);
                            let _ = tx.send((share_code(&input), found));
                            wake();
                        });
                        self.show_toast("Looking for the board…");
                    }
                }
            }
            Overlay::Confirm { purpose, .. } => match purpose {
                ConfirmFor::Delete(id) => {
                    self.thumbs.remove(&id);
                    self.send(Job::Delete(id));
                }
                ConfirmFor::Install(url) => {
                    self.show_toast("Downloading the update…");
                    self.platform.install_update(&url);
                }
            },
            Overlay::Menu { .. } | Overlay::Panel { .. } => {}
        }
    }

    // ------------------------------------------------------------ layout

    fn wdp(&self) -> f32 {
        self.w as f32 / self.d
    }

    fn layout(&self) -> Vec<Node> {
        let mut ui = Ui {
            nodes: Vec::with_capacity(64),
            d: self.d,
            pressed: self.pressed.as_ref(),
            focus: self.focus.as_ref(),
        };
        match self.screen {
            Screen::Library => self.layout_library(&mut ui),
            Screen::Board => self.layout_board(&mut ui),
        }
        if let Some(overlay) = &self.overlay {
            self.layout_overlay(&mut ui, overlay);
        }
        if let Some((text, _)) = &self.toast {
            let d = self.d;
            let tw = font::measure(Face::Regular, 14.0 * d, text).0;
            let w = (tw + 32.0 * d).min(self.w as f32 - 32.0 * d);
            let r = Rf::new(
                (self.w as f32 - w) / 2.0,
                self.h as f32 - 104.0 * d,
                w,
                48.0 * d,
            );
            ui.push(r, Look::Toast(text.clone()), None);
        }
        ui.nodes
    }

    /// Library geometry: (padding, columns, card width, card height, top of
    /// the grid before scrolling, content height).
    fn library_metrics(&self) -> (f32, usize, f32, f32, f32, f32) {
        let d = self.d;
        let wdp = self.wdp();
        let pad = if wdp >= 600.0 { 36.0 } else { 20.0 } * d;
        let cols = if wdp >= 1000.0 {
            4
        } else if wdp >= 600.0 {
            3
        } else {
            2
        };
        let avail = self.w as f32 - 2.0 * pad;
        let cw = ((avail - 14.0 * d * (cols as f32 - 1.0)) / cols as f32).max(1.0);
        let ch = cw / 0.95;
        let heading = paint::wrap(Face::Bold, 32.0 * d, "Make room for ideas.", avail).len() as f32
            * 40.0
            * d;
        let grid_top = 64.0 * d + pad + heading + 8.0 * d + 20.0 * d + 28.0 * d;
        let rows = self.boards.len().div_ceil(cols) as f32;
        let content = grid_top + rows * ch + (rows - 1.0).max(0.0) * 14.0 * d + 90.0 * d;
        (pad, cols, cw, ch, grid_top, content)
    }

    fn clamp_scroll(&mut self) {
        let (.., content) = self.library_metrics();
        let max = (content - self.h as f32).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    fn layout_library(&self, ui: &mut Ui) {
        let d = self.d;
        let (w, h) = (self.w as f32, self.h as f32);
        let (pad, cols, cw, ch, grid_top, _) = self.library_metrics();
        let avail = w - 2.0 * pad;
        let top = 64.0 * d + pad - self.scroll;

        // Heading.
        let lines = paint::wrap(Face::Bold, 32.0 * d, "Make room for ideas.", avail).len() as f32;
        let heading = lines * 40.0 * d;
        ui.push(
            Rf::new(pad, top, avail, heading),
            Look::Para {
                text: "Make room for ideas.".into(),
                face: Face::Bold,
                size: 32.0 * d,
                line: 40.0 * d,
                color: INK,
                center: false,
            },
            None,
        );
        ui.push(
            Rf::new(pad, top + heading + 8.0 * d, avail, 20.0 * d),
            Look::Label {
                text: "A blank canvas. Endless possibilities.".into(),
                face: Face::Regular,
                size: 14.0 * d,
                color: HINT,
                center: false,
            },
            None,
        );

        let grid_y = grid_top - self.scroll;
        if self.listed && self.boards.is_empty() {
            let mid = (grid_y + h) / 2.0 - 40.0 * d;
            ui.push(
                Rf::new(0.0, mid - 64.0 * d, w, 64.0 * d),
                Look::Glyph {
                    icon: icon::DRAW,
                    size: 64.0 * d,
                    color: PURPLE,
                },
                None,
            );
            ui.push(
                Rf::new(pad, mid + 20.0 * d, avail, 28.0 * d),
                Look::Label {
                    text: "Your next idea starts here".into(),
                    face: Face::Regular,
                    size: 22.0 * d,
                    color: INK,
                    center: true,
                },
                None,
            );
            ui.push(
                Rf::new(pad, mid + 56.0 * d, avail, 20.0 * d),
                Look::Label {
                    text: "Create a board and make your first mark.".into(),
                    face: Face::Regular,
                    size: 14.0 * d,
                    color: HINT,
                    center: true,
                },
                None,
            );
        }
        for (i, b) in self.boards.iter().enumerate() {
            let (row, col) = (i / cols, i % cols);
            let r = Rf::new(
                pad + col as f32 * (cw + 14.0 * d),
                grid_y + row as f32 * (ch + 14.0 * d),
                cw,
                ch,
            );
            if r.bottom() < 64.0 * d || r.y > h {
                continue;
            }
            ui.push(
                r,
                Look::Surface {
                    color: WHITE,
                    radius: 12.0,
                    elev: 1.0,
                    border: false,
                },
                Some(Act::Open(b.id.clone())),
            );
            let thumb = Rf::new(
                r.x + 14.0 * d,
                r.y + 14.0 * d,
                r.w - 28.0 * d,
                (r.h - 28.0 * d - 56.0 * d).max(1.0),
            );
            let gen = self.thumbs.get(&b.id).map_or(0, |t| t.gen);
            ui.push(
                thumb,
                Look::Thumb {
                    id: b.id.clone(),
                    gen,
                },
                None,
            );
            let row_y = r.bottom() - 56.0 * d;
            let text_w = r.w - 14.0 * d - 48.0 * d;
            ui.push(
                Rf::new(r.x + 14.0 * d, row_y + 6.0 * d, text_w, 20.0 * d),
                Look::Label {
                    text: b.title.clone(),
                    face: Face::Medium,
                    size: 14.0 * d,
                    color: INK,
                    center: false,
                },
                None,
            );
            let marks = if b.shapes == 1 {
                "1 mark".to_string()
            } else {
                format!("{} marks", b.shapes)
            };
            ui.push(
                Rf::new(r.x + 14.0 * d, row_y + 26.0 * d, text_w, 16.0 * d),
                Look::Label {
                    text: marks,
                    face: Face::Regular,
                    size: 12.0 * d,
                    color: GREY,
                    center: false,
                },
                None,
            );
            ui.push(
                Rf::new(r.right() - 48.0 * d, row_y, 48.0 * d, 48.0 * d),
                Look::AppIcon {
                    icon: icon::MORE,
                    badge: false,
                },
                Some(Act::CardMenu(b.id.clone())),
            );
        }

        // App bar, over the scrolled content.
        ui.push(
            Rf::new(0.0, 0.0, w, 64.0 * d),
            Look::Surface {
                color: PAGE,
                radius: 0.0,
                elev: if self.scroll > 0.0 { 0.6 } else { 0.0 },
                border: false,
            },
            None,
        );
        ui.push(
            Rf::new(16.0 * d, 14.0 * d, 36.0 * d, 36.0 * d),
            Look::Logo,
            None,
        );
        ui.push(
            Rf::new(62.0 * d, 0.0, (w - 62.0 * d - 212.0 * d).max(0.0), 64.0 * d),
            Look::Label {
                text: "Inkboard".into(),
                face: Face::Regular,
                size: 22.0 * d,
                color: INK,
                center: false,
            },
            None,
        );
        ui.push(
            Rf::new(w - 200.0 * d, 8.0 * d, 48.0 * d, 48.0 * d),
            Look::AppIcon {
                icon: icon::JOIN,
                badge: false,
            },
            Some(Act::Join),
        );
        ui.push(
            Rf::new(w - 152.0 * d, 8.0 * d, 48.0 * d, 48.0 * d),
            Look::AppIcon {
                icon: icon::CLAUDE,
                badge: false,
            },
            Some(Act::Claude),
        );
        ui.push(
            Rf::new(w - 104.0 * d, 8.0 * d, 48.0 * d, 48.0 * d),
            Look::AppIcon {
                icon: icon::SPEED,
                badge: false,
            },
            Some(Act::Display),
        );
        ui.push(
            Rf::new(w - 56.0 * d, 8.0 * d, 48.0 * d, 48.0 * d),
            Look::AppIcon {
                icon: icon::UPDATE,
                badge: self.update.is_some(),
            },
            Some(Act::Updates),
        );

        // New board button.
        let label_w = font::measure(Face::Medium, 14.0 * d, "New board").0;
        let fw = 16.0 * d + 24.0 * d + 8.0 * d + label_w + 20.0 * d;
        ui.push(
            Rf::new(w - 16.0 * d - fw, h - 16.0 * d - 56.0 * d, fw, 56.0 * d),
            Look::Fab,
            Some(Act::NewBoard),
        );
    }

    fn layout_board(&self, ui: &mut Ui) {
        let d = self.d;
        let w = self.w as f32;
        let h = self.h as f32;
        let wdp = self.wdp();
        let wide = wdp >= 700.0;
        let large = wdp >= 1200.0;
        let b = 42.0 * d; // button slot: 40 plus a margin of 1 on each side
        let pad = 4.0 * d;
        let ed = &self.ed;

        // Top left: back, menu, title, undo, redo.
        let title = self
            .board
            .as_ref()
            .map(|b| b.title.clone())
            .unwrap_or_default();
        let (status, failed) = match self.save_state {
            SaveState::Saved => ("Saved", false),
            SaveState::Saving => ("Saving…", false),
            SaveState::Failed => ("Save failed • tap to retry", true),
        };
        let status = if failed {
            status.to_string()
        } else if let Some(code) = self.host_code() {
            format!("{status} • sharing {code}")
        } else if self.link.is_some() {
            match self.link_online {
                Some(true) => format!("{status} • live"),
                Some(false) => format!("{status} • reconnecting…"),
                None => format!("{status} • connecting…"),
            }
        } else {
            status.to_string()
        };
        let sharing = self.host_code().is_some() || self.link.is_some();
        let title_w = if wide {
            let tw = font::measure(Face::Medium, 14.0 * d, &title)
                .0
                .max(font::measure(Face::Regular, 11.0 * d, &status).0);
            tw.min(220.0 * d) + 12.0 * d
        } else {
            0.0
        };
        let x0 = 10.0 * d;
        let y0 = 10.0 * d;
        let panel_w = pad * 2.0 + b * 5.0 + title_w;
        ui.panel(Rf::new(x0, y0, panel_w, pad * 2.0 + b));
        let mut x = x0 + pad;
        let y = y0 + pad;
        ui.btn(x, y, b, icon::BACK, false, true, Act::Back);
        x += b;
        let menu_r = Rf::new(x + d, y + d, b - 2.0 * d, b - 2.0 * d);
        ui.push(
            menu_r,
            Look::Btn {
                icon: icon::MENU,
                isz: 20.0 * d,
                active: false,
                enabled: true,
            },
            Some(Act::Menu),
        );
        x += b;
        if wide {
            ui.push(
                Rf::new(x, y, title_w, b),
                Look::TitleBlock {
                    title,
                    status,
                    failed,
                },
                if failed { Some(Act::Retry) } else { None },
            );
            x += title_w;
        }
        ui.btn(x, y, b, icon::UNDO, false, ed.doc.can_undo(), Act::Undo);
        x += b;
        ui.btn(x, y, b, icon::REDO, false, ed.doc.can_redo(), Act::Redo);
        x += b;
        ui.btn(x, y, b, icon::SHARE, sharing, true, Act::Share);

        // Selection actions.
        let selected = ed.doc.selected_ids().len();
        if selected > 0 {
            let texts = ed.selected_text_count();
            let inks = ed.selected_ink_count();
            let mut acts = vec![(icon::DELETE, Act::Delete)];
            if inks > 0 {
                acts.push((icon::TO_TEXT, Act::Convert));
            }
            if texts > 0 {
                acts.push((icon::FONT, Act::FontPanel));
                acts.push((icon::BIGGER, Act::Bigger));
                acts.push((icon::SMALLER, Act::Smaller));
            }
            let sy = y0 + pad * 2.0 + b + 8.0 * d;
            ui.panel(Rf::new(
                x0,
                sy,
                pad * 2.0 + b * acts.len() as f32,
                pad * 2.0 + b,
            ));
            for (i, (code, act)) in acts.into_iter().enumerate() {
                ui.btn(x0 + pad + b * i as f32, sy + pad, b, code, false, true, act);
            }
        }

        // Top right: style.
        let style_x = w - 10.0 * d - (pad * 2.0 + b);
        ui.panel(Rf::new(style_x, y0, pad * 2.0 + b, pad * 2.0 + b));
        ui.push(
            Rf::new(style_x + pad + d, y0 + pad + d, b - 2.0 * d, b - 2.0 * d),
            Look::StyleDot {
                color: ed.color(),
                open: self.style_open,
            },
            Some(Act::Style),
        );
        if self.style_open || large {
            let pw = 168.0 * d;
            let inner = pw - 16.0 * d;
            let sw = 42.0 * d;
            let per_row = ((inner / sw).floor() as usize).max(1);
            let rows = PALETTE.len().div_ceil(per_row);
            let gap = (inner - sw * per_row as f32) / (per_row as f32 - 1.0).max(1.0);
            let ph =
                8.0 * d + rows as f32 * sw + 14.0 * d + 40.0 * d + 14.0 * d + 36.0 * d + 8.0 * d;
            let px = w - 10.0 * d - pw;
            let py = y0 + pad * 2.0 + b + 8.0 * d;
            ui.panel(Rf::new(px, py, pw, ph));
            let ix = px + 8.0 * d;
            let mut iy = py + 8.0 * d;
            for (i, c) in PALETTE.iter().enumerate() {
                let (row, col) = (i / per_row, i % per_row);
                let r = Rf::new(ix + col as f32 * (sw + gap), iy + row as f32 * sw, sw, sw);
                ui.push(
                    r,
                    Look::Swatch {
                        color: *c,
                        chosen: ed.color() == *c,
                    },
                    Some(Act::Color(*c)),
                );
            }
            iy += rows as f32 * sw;
            ui.push(Rf::new(ix, iy, inner, 14.0 * d), Look::Rule, None);
            iy += 14.0 * d;
            let cwid = inner / SIZES.len() as f32;
            for (i, (label, width)) in SIZES.iter().enumerate() {
                let chosen = (ed.stroke_width() - width).abs() < 0.01;
                ui.push(
                    Rf::new(ix + cwid * i as f32, iy, cwid, 40.0 * d),
                    Look::Chip {
                        text: (*label).into(),
                        chosen,
                    },
                    Some(Act::Width(i)),
                );
            }
            iy += 40.0 * d;
            ui.push(Rf::new(ix, iy, inner, 14.0 * d), Look::Rule, None);
            iy += 14.0 * d;
            ui.push(
                Rf::new(ix + 6.0 * d, iy, 60.0 * d, 36.0 * d),
                Look::Label {
                    text: "Fill".into(),
                    face: Face::Regular,
                    size: 14.0 * d,
                    color: ICON,
                    center: false,
                },
                None,
            );
            let filled = ed.filled();
            let fx = ix + inner - 72.0 * d;
            ui.push(
                Rf::new(fx, iy, 36.0 * d, 36.0 * d),
                Look::Btn {
                    icon: icon::NO_FILL,
                    isz: 18.0 * d,
                    active: !filled,
                    enabled: true,
                },
                Some(Act::Fill(false)),
            );
            ui.push(
                Rf::new(fx + 36.0 * d, iy, 36.0 * d, 36.0 * d),
                Look::Btn {
                    icon: icon::FILL,
                    isz: 18.0 * d,
                    active: filled,
                    enabled: true,
                },
                Some(Act::Fill(true)),
            );
        }

        // Bottom left: zoom.
        if wide {
            let zw = pad * 2.0 + b * 3.0 + 54.0 * d;
            let zy = h - 10.0 * d - (pad * 2.0 + b);
            ui.panel(Rf::new(x0, zy, zw, pad * 2.0 + b));
            let mut zx = x0 + pad;
            ui.btn(zx, zy + pad, b, icon::REMOVE, false, true, Act::ZoomOut);
            zx += b;
            let pct = format!("{}%", (ed.zoom() * 100.0).round() as i64);
            ui.push(
                Rf::new(zx + d, zy + pad + d, 52.0 * d, 40.0 * d),
                Look::Chip {
                    text: pct,
                    chosen: false,
                },
                Some(Act::Zoom100),
            );
            zx += 54.0 * d;
            ui.btn(zx, zy + pad, b, icon::ADD, false, true, Act::ZoomIn);
            zx += b;
            ui.btn(zx, zy + pad, b, icon::FIT, false, true, Act::Fit);
        }

        // Bottom centre: tools.
        let tools: [(u32, Tool, PenMode); 9] = [
            (icon::SELECT, Tool::Select, PenMode::Ink),
            (icon::HAND, Tool::Hand, PenMode::Ink),
            (icon::PEN, Tool::Pen, PenMode::Ink),
            (icon::WRITE, Tool::Pen, PenMode::Write),
            (icon::SMART, Tool::Pen, PenMode::Shape),
            (icon::ERASER, Tool::Eraser, PenMode::Ink),
            (icon::ARROW, Tool::Arrow, PenMode::Ink),
            (icon::RECTANGLE, Tool::Rectangle, PenMode::Ink),
            (icon::ELLIPSE, Tool::Ellipse, PenMode::Ink),
        ];
        let tb = (if large { 54.0 } else { 44.0 } * d)
            .min((w - 20.0 * d - pad * 2.0) / tools.len() as f32);
        let tw = pad * 2.0 + tb * tools.len() as f32;
        let tx = (w - tw) / 2.0;
        let ty = h - 10.0 * d - (pad * 2.0 + tb);
        ui.panel(Rf::new(tx, ty, tw, pad * 2.0 + tb));
        for (i, (code, tool, mode)) in tools.iter().enumerate() {
            let active = ed.tool() == *tool && (*tool != Tool::Pen || ed.pen_mode() == *mode);
            ui.btn(
                tx + pad + tb * i as f32,
                ty + pad,
                tb,
                *code,
                active,
                true,
                Act::Tool(tool.index() as usize, *mode),
            );
        }
    }

    fn menu_entries(&self, kind: &MenuKind) -> Vec<Entry> {
        let s = &self.settings;
        let item = |t: &str, c: Option<bool>, a: Act| Entry::Item(t.to_string(), c, a);
        match kind {
            MenuKind::Main => {
                let mut v = vec![
                    item("Share board…", None, Act::Share),
                    item("Connect Claude…", None, Act::Claude),
                    item("Export as PNG", None, Act::Export),
                    Entry::Rule,
                    item("Zoom to fit", None, Act::Fit),
                    item("Show grid", Some(s.grid), Act::Toggle(Opt::Grid)),
                    Entry::Rule,
                    item("Hold to snap shapes", Some(s.snap), Act::Toggle(Opt::Snap)),
                    Entry::Rule,
                ];
                for (i, (code, name)) in LANGS.iter().enumerate() {
                    v.push(item(
                        &format!("Handwriting: {name}"),
                        Some(s.lang == *code),
                        Act::Lang(i),
                    ));
                }
                v.push(Entry::Rule);
                v.push(item(
                    "Tidy drawn shapes",
                    Some(s.tidy),
                    Act::Toggle(Opt::Tidy),
                ));
                v.push(item(
                    "Turn handwriting into typed text",
                    Some(s.typed),
                    Act::Toggle(Opt::Typed),
                ));
                v.push(item(
                    "Ignore palm touches",
                    Some(s.palm),
                    Act::Toggle(Opt::Palm),
                ));
                v.push(item(
                    "Pen draws, fingers move the board",
                    Some(s.pen_priority),
                    Act::Toggle(Opt::PenPriority),
                ));
                v.push(item(
                    "Several fingers can write at once",
                    Some(s.multi_ink),
                    Act::Toggle(Opt::MultiInk),
                ));
                v.push(item("Text font…", None, Act::FontPanel));
                v.push(item("Display and speed…", None, Act::Display));
                v
            }
            MenuKind::Display => {
                let mut v = vec![
                    item("Full panel resolution (slower on 4K)", Some(s.full_res), Act::FullRes),
                    item("Low-latency ink", Some(s.fast_ink), Act::Toggle(Opt::FastInk)),
                    Entry::Rule,
                ];
                for (i, name) in ["Ink prediction: off", "Ink prediction: low", "Ink prediction: medium", "Ink prediction: high"]
                    .iter()
                    .enumerate()
                {
                    v.push(item(name, Some(s.predict as usize == i), Act::Predict(i as u8)));
                }
                v.push(Entry::Rule);
                v.push(item("Import boards…", None, Act::Import));
                v
            }
            MenuKind::Card(id) => vec![
                item("Rename", None, Act::Rename(id.clone())),
                item("Delete", None, Act::AskDelete(id.clone())),
            ],
        }
    }

    fn layout_overlay(&self, ui: &mut Ui, overlay: &Overlay) {
        let d = self.d;
        let (w, h) = (self.w as f32, self.h as f32);
        match overlay {
            Overlay::Menu {
                kind,
                anchor,
                right,
            } => {
                ui.push(Rf::new(0.0, 0.0, w, h), Look::Scrim(0), Some(Act::Outside));
                let entries = self.menu_entries(kind);
                let checks = entries
                    .iter()
                    .any(|e| matches!(e, Entry::Item(_, Some(_), _)));
                let lead = if checks { 48.0 * d } else { 12.0 * d };
                let widest = entries
                    .iter()
                    .map(|e| match e {
                        Entry::Item(t, ..) => font::measure(Face::Regular, 14.0 * d, t).0,
                        Entry::Rule => 0.0,
                    })
                    .fold(0.0, f32::max);
                let mw = (lead + widest + 24.0 * d)
                    .clamp(112.0 * d, 320.0 * d)
                    .min(w - 16.0 * d);
                let items = entries
                    .iter()
                    .filter(|e| matches!(e, Entry::Item(..)))
                    .count() as f32;
                let rules = entries.len() as f32 - items;
                let room = h - 16.0 * d;
                let mut ih = 48.0 * d;
                if 16.0 * d + items * ih + rules * 16.0 * d > room {
                    ih = ((room - 16.0 * d - rules * 16.0 * d) / items.max(1.0)).max(32.0 * d);
                }
                let mh = 16.0 * d + items * ih + rules * 16.0 * d;
                let mx = if *right {
                    anchor.right() - mw
                } else {
                    anchor.x
                }
                .clamp(8.0 * d, (w - 8.0 * d - mw).max(8.0 * d));
                let my = anchor.y.clamp(8.0 * d, (h - 8.0 * d - mh).max(8.0 * d));
                ui.push(
                    Rf::new(mx, my, mw, mh),
                    Look::Surface {
                        color: MENU,
                        radius: 4.0,
                        elev: 3.0,
                        border: false,
                    },
                    None,
                );
                let mut y = my + 8.0 * d;
                for e in entries {
                    match e {
                        Entry::Item(text, check, act) => {
                            ui.push(
                                Rf::new(mx, y, mw, ih),
                                Look::Item {
                                    text,
                                    check: if checks {
                                        Some(check.unwrap_or(false))
                                    } else {
                                        None
                                    },
                                },
                                Some(act),
                            );
                            y += ih;
                        }
                        Entry::Rule => {
                            ui.push(Rf::new(mx, y, mw, 16.0 * d), Look::Rule, None);
                            y += 16.0 * d;
                        }
                    }
                }
            }
            Overlay::Ask {
                title,
                text,
                hint,
                multiline,
                ..
            } => {
                let dw = (w - 80.0 * d).clamp((280.0 * d).min(w - 16.0 * d), 560.0 * d);
                let inner = dw - 48.0 * d;
                let field_h = if *multiline {
                    let lines = paint::wrap(Face::Regular, 16.0 * d, text, inner)
                        .len()
                        .clamp(2, 6) as f32;
                    lines * 24.0 * d + 16.0 * d
                } else {
                    48.0 * d
                };
                let title_h =
                    paint::wrap(Face::Regular, 24.0 * d, title, inner).len() as f32 * 32.0 * d;
                let dh = 24.0 * d + title_h + 16.0 * d + field_h + 24.0 * d + 40.0 * d + 24.0 * d;
                let (dx, dy) = ((w - dw) / 2.0, ((h - dh) / 2.0).max(8.0 * d));
                ui.push(
                    Rf::new(0.0, 0.0, w, h),
                    Look::Scrim(0x8a000000),
                    Some(Act::Outside),
                );
                ui.push(
                    Rf::new(dx, dy, dw, dh),
                    Look::Surface {
                        color: DIALOG,
                        radius: 28.0,
                        elev: 3.0,
                        border: false,
                    },
                    None,
                );
                let mut y = dy + 24.0 * d;
                ui.push(
                    Rf::new(dx + 24.0 * d, y, inner, title_h),
                    Look::Para {
                        text: title.clone(),
                        face: Face::Regular,
                        size: 24.0 * d,
                        line: 32.0 * d,
                        color: ON_SURFACE,
                        center: false,
                    },
                    None,
                );
                y += title_h + 16.0 * d;
                ui.push(
                    Rf::new(dx + 24.0 * d, y, inner, field_h),
                    Look::Field {
                        text: text.clone(),
                        hint: hint.clone(),
                        multiline: *multiline,
                    },
                    Some(Act::Field),
                );
                y += field_h + 24.0 * d;
                self.dialog_buttons(ui, dx + dw - 24.0 * d, y, "Cancel", "Done");
            }
            Overlay::Confirm {
                title,
                body,
                cancel,
                ok,
                ..
            } => {
                let dw = (w - 80.0 * d).clamp((280.0 * d).min(w - 16.0 * d), 560.0 * d);
                let inner = dw - 48.0 * d;
                let title_h =
                    paint::wrap(Face::Regular, 24.0 * d, title, inner).len() as f32 * 32.0 * d;
                let body_h =
                    paint::wrap(Face::Regular, 14.0 * d, body, inner).len() as f32 * 20.0 * d;
                let dh = 24.0 * d + title_h + 16.0 * d + body_h + 24.0 * d + 40.0 * d + 24.0 * d;
                let (dx, dy) = ((w - dw) / 2.0, ((h - dh) / 2.0).max(8.0 * d));
                ui.push(
                    Rf::new(0.0, 0.0, w, h),
                    Look::Scrim(0x8a000000),
                    Some(Act::Outside),
                );
                ui.push(
                    Rf::new(dx, dy, dw, dh),
                    Look::Surface {
                        color: DIALOG,
                        radius: 28.0,
                        elev: 3.0,
                        border: false,
                    },
                    None,
                );
                let mut y = dy + 24.0 * d;
                ui.push(
                    Rf::new(dx + 24.0 * d, y, inner, title_h),
                    Look::Para {
                        text: title.clone(),
                        face: Face::Regular,
                        size: 24.0 * d,
                        line: 32.0 * d,
                        color: ON_SURFACE,
                        center: false,
                    },
                    None,
                );
                y += title_h + 16.0 * d;
                ui.push(
                    Rf::new(dx + 24.0 * d, y, inner, body_h),
                    Look::Para {
                        text: body.clone(),
                        face: Face::Regular,
                        size: 14.0 * d,
                        line: 20.0 * d,
                        color: ON_VARIANT,
                        center: false,
                    },
                    None,
                );
                y += body_h + 24.0 * d;
                self.dialog_buttons(ui, dx + dw - 24.0 * d, y, cancel, ok);
            }
            Overlay::Panel { kind, scroll } => self.layout_panel(ui, *kind, *scroll),
        }
    }

    /// The open board's share code, when this device shares it.
    fn host_code(&self) -> Option<String> {
        self.board
            .as_ref()?
            .extra
            .get("lanHost")?
            .as_str()
            .map(str::to_string)
    }

    fn relay_online(&self) -> bool {
        self.relay
            .as_ref()
            .is_some_and(|r| r.online.load(std::sync::atomic::Ordering::SeqCst))
    }

    fn lan_mcp(&self) -> Option<String> {
        let ip = self.lan_ip?;
        Some(format!("http://{ip}:{}/mcp/{}", net::PORT, self.settings.mcp_key))
    }

    fn public_mcp(&self) -> Option<String> {
        let base = self.settings.relay_url.trim_end_matches('/');
        (!base.is_empty()).then(|| {
            format!(
                "{base}/d/{}/mcp/{}",
                self.settings.relay_id, self.settings.mcp_key
            )
        })
    }

    /// Size of the panel sheet and the height its content needs.
    fn panel_frame(&self, kind: PanelKind) -> (Rf, f32, f32) {
        let d = self.d;
        let (w, h) = (self.w as f32, self.h as f32);
        let dw = (w - 80.0 * d).clamp((280.0 * d).min(w - 16.0 * d), 680.0 * d);
        let inner = dw - 48.0 * d;
        let (_, content) = self.panel_rows(kind, inner);
        let chrome = 24.0 * d + 32.0 * d + 16.0 * d + 24.0 * d + 40.0 * d + 24.0 * d;
        let dh = (chrome + content).min(h - 32.0 * d).max(chrome.min(h));
        let r = Rf::new((w - dw) / 2.0, ((h - dh) / 2.0).max(8.0 * d), dw, dh);
        let view = dh - chrome;
        (r, content, view)
    }

    fn panel_max_scroll(&self) -> f32 {
        match &self.overlay {
            Some(Overlay::Panel { kind, .. }) => {
                let (_, content, view) = self.panel_frame(*kind);
                (content - view).max(0.0)
            }
            _ => 0.0,
        }
    }

    /// Content of a panel, positioned from (0, 0), and its height.
    fn panel_rows(&self, kind: PanelKind, inner: f32) -> (Vec<(Rf, Look, Option<Act>)>, f32) {
        let d = self.d;
        let mut out: Vec<(Rf, Look, Option<Act>)> = Vec::new();
        let mut y = 0.0;
        let para = |out: &mut Vec<(Rf, Look, Option<Act>)>, y: &mut f32, text: &str, face: Face, color: u32| {
            let lines = paint::wrap(face, 14.0 * d, text, inner).len() as f32;
            out.push((
                Rf::new(0.0, *y, inner, lines * 20.0 * d),
                Look::Para {
                    text: text.into(),
                    face,
                    size: 14.0 * d,
                    line: 20.0 * d,
                    color,
                    center: false,
                },
                None,
            ));
            *y += lines * 20.0 * d + 12.0 * d;
        };
        let code = |out: &mut Vec<(Rf, Look, Option<Act>)>, y: &mut f32, text: &str| {
            let bw = font::measure(Face::Medium, 14.0 * d, "Copy").0 + 32.0 * d;
            let cw = inner - bw - 8.0 * d;
            let lines = wrap_chars(14.0 * d, text, cw - 24.0 * d).len() as f32;
            let ch = (lines * 20.0 * d + 16.0 * d).max(40.0 * d);
            out.push((Rf::new(0.0, *y, cw, ch), Look::Code(text.into()), None));
            out.push((
                Rf::new(inner - bw, *y + (ch - 40.0 * d) / 2.0, bw, 40.0 * d),
                Look::DlgBtn {
                    text: "Copy".into(),
                    filled: false,
                },
                Some(Act::Copy(text.into())),
            ));
            *y += ch + 16.0 * d;
        };
        match kind {
            PanelKind::Claude => {
                para(&mut out, &mut y, "Claude can read the open board, add notes, shapes, arrows and whole diagrams, move or delete things, and look at a picture of the board. Add this panel to Claude as an MCP connector:", Face::Regular, ON_VARIANT);
                para(&mut out, &mut y, "From anywhere", Face::Medium, ON_SURFACE);
                let online = self.relay_online();
                para(
                    &mut out,
                    &mut y,
                    if online {
                        "Relay online. Works from claude.ai, the Claude apps and Claude Code."
                    } else {
                        "Relay offline right now. This address starts working as soon as the relay can be reached."
                    },
                    Face::Regular,
                    if online { ON_VARIANT } else { ERROR },
                );
                if let Some(url) = self.public_mcp() {
                    code(&mut out, &mut y, &url);
                }
                para(&mut out, &mut y, "On this network", Face::Medium, ON_SURFACE);
                match self.lan_mcp() {
                    Some(url) => code(&mut out, &mut y, &url),
                    None => para(&mut out, &mut y, "Connect to Wi-Fi or Ethernet to get a local address.", Face::Regular, ON_VARIANT),
                }
                para(&mut out, &mut y, "Claude Code", Face::Medium, ON_SURFACE);
                if let Some(url) = self.public_mcp().or_else(|| self.lan_mcp()) {
                    code(&mut out, &mut y, &format!("claude mcp add --transport http inkboard {url}"));
                }
                para(&mut out, &mut y, "Claude app or claude.ai: Settings → Connectors → Add custom connector, name it Inkboard and paste the address from anywhere.", Face::Regular, ON_VARIANT);
                para(&mut out, &mut y, "Suggested models: Claude Opus 5.5 for diagrams, plans and big layouts; Claude Sonnet 5.5 for quick notes and edits.", Face::Regular, ON_VARIANT);
                para(&mut out, &mut y, "Anyone with these addresses can change the open board. Keep them private.", Face::Regular, ERROR);
            }
            PanelKind::Share => {
                if let Some(c) = self.host_code() {
                    para(&mut out, &mut y, "On another device open Inkboard, tap Join (the person-plus icon) and enter:", Face::Regular, ON_VARIANT);
                    out.push((
                        Rf::new(0.0, y, inner, 56.0 * d),
                        Look::Label {
                            text: c.clone(),
                            face: Face::Bold,
                            size: 40.0 * d,
                            color: PRIMARY,
                            center: true,
                        },
                        None,
                    ));
                    y += 64.0 * d;
                    code(&mut out, &mut y, &c);
                    let route = if self.relay_online() {
                        "Devices on this network connect directly; others connect through the relay."
                    } else {
                        "Devices on this network connect directly. The relay is offline, so other networks cannot join yet."
                    };
                    para(&mut out, &mut y, route, Face::Regular, ON_VARIANT);
                    if let Some(ip) = self.lan_ip {
                        para(&mut out, &mut y, &format!("This panel: {ip}"), Face::Regular, STATUS);
                    }
                    let sw = font::measure(Face::Medium, 14.0 * d, "Stop sharing").0 + 32.0 * d;
                    out.push((
                        Rf::new(0.0, y, sw, 40.0 * d),
                        Look::DlgBtn {
                            text: "Stop sharing".into(),
                            filled: false,
                        },
                        Some(Act::StopShare),
                    ));
                    y += 52.0 * d;
                } else {
                    let state = match self.link_online {
                        Some(true) => "live: changes appear on both devices.",
                        Some(false) => "reconnecting to the device that shares it…",
                        None => "connecting to the device that shares it…",
                    };
                    para(&mut out, &mut y, &format!("This board was joined from another device and is {state}"), Face::Regular, ON_VARIANT);
                }
            }
            PanelKind::Fonts => {
                let gap = 8.0 * d;
                let cols = (((inner + gap) / (168.0 * d)).floor() as usize).max(1);
                let tw = (inner - gap * (cols as f32 - 1.0)) / cols as f32;
                let th = 56.0 * d;
                for (i, _) in text::FONT_NAMES.iter().enumerate() {
                    let (c, r) = (i % cols, i / cols);
                    out.push((
                        Rf::new(c as f32 * (tw + gap), r as f32 * (th + gap), tw, th),
                        Look::FontTile {
                            font: i as u8,
                            chosen: self.settings.font == i as u8,
                        },
                        Some(Act::Font(i as u8)),
                    ));
                }
                let rows = text::FONT_NAMES.len().div_ceil(cols) as f32;
                y = rows * (th + gap);
            }
        }
        (out, y.max(0.0))
    }

    fn layout_panel(&self, ui: &mut Ui, kind: PanelKind, scroll: f32) {
        let d = self.d;
        let (w, h) = (self.w as f32, self.h as f32);
        let (r, _, view) = self.panel_frame(kind);
        ui.push(
            Rf::new(0.0, 0.0, w, h),
            Look::Scrim(0x8a000000),
            Some(Act::Outside),
        );
        ui.push(
            r,
            Look::Surface {
                color: DIALOG,
                radius: 28.0,
                elev: 3.0,
                border: false,
            },
            Some(Act::Nothing),
        );
        let title = match kind {
            PanelKind::Share => "Share board",
            PanelKind::Claude => "Connect Claude",
            PanelKind::Fonts => "Text font",
        };
        let inner = r.w - 48.0 * d;
        ui.push(
            Rf::new(r.x + 24.0 * d, r.y + 24.0 * d, inner, 32.0 * d),
            Look::Label {
                text: title.into(),
                face: Face::Regular,
                size: 24.0 * d,
                color: ON_SURFACE,
                center: false,
            },
            None,
        );
        let top = r.y + 24.0 * d + 32.0 * d + 16.0 * d;
        let bottom = top + view;
        let (rows, _) = self.panel_rows(kind, inner);
        for (rr, look, act) in rows {
            let placed = Rf::new(r.x + 24.0 * d + rr.x, top + rr.y - scroll, rr.w, rr.h);
            if placed.y >= top - 0.5 && placed.bottom() <= bottom + 0.5 {
                ui.push(placed, look, act);
            }
        }
        let ow = font::measure(Face::Medium, 14.0 * d, "Done").0 + 48.0 * d;
        let by = r.bottom() - 24.0 * d - 40.0 * d;
        if self.panel_max_scroll() > 0.0 {
            ui.push(
                Rf::new(r.x + 24.0 * d, by, inner - ow - 8.0 * d, 40.0 * d),
                Look::Label {
                    text: "Drag to see more".into(),
                    face: Face::Regular,
                    size: 12.0 * d,
                    color: STATUS,
                    center: false,
                },
                None,
            );
        }
        ui.push(
            Rf::new(r.right() - 24.0 * d - ow, by, ow, 40.0 * d),
            Look::DlgBtn {
                text: "Done".into(),
                filled: true,
            },
            Some(Act::Cancel),
        );
    }

    // ------------------------------------------------------------ sharing

    fn shape_map(&self) -> HashMap<String, Value> {
        self.ed
            .doc
            .shapes()
            .iter()
            .map(|s| (s.id.clone(), s.to_json()))
            .collect()
    }

    /// Joined boards remember where they came from; reopening reconnects.
    fn start_link(&mut self) {
        self.link = None;
        self.known.clear();
        self.link_online = None;
        let Some(share) = self.board.as_ref().and_then(|b| b.extra.get("lanShare")) else {
            return;
        };
        let (Some(host), Some(code)) = (share["host"].as_str(), share["code"].as_str()) else {
            return;
        };
        self.known = self.shape_map();
        self.ops_sent = 0;
        self.link = Some(net::link(host.to_string(), code.to_string(), self.wake.clone()));
    }

    /// Tells the relay which share codes may be looked up.
    fn sync_relay_codes(&mut self) {
        let code = self.host_code().unwrap_or_default();
        if let Some(relay) = &self.relay {
            if let Ok(mut c) = relay.codes.lock() {
                *c = code;
            }
        }
    }

    fn take_net(&mut self) {
        while let Ok(req) = self.net_rx.try_recv() {
            let resp = self.answer(&req);
            let _ = req.reply.send(resp);
        }
    }

    fn answer(&mut self, req: &net::Req) -> net::Resp {
        let path = req.path.trim_end_matches('/');
        if let Some(key) = path.strip_prefix("/mcp/") {
            if key != self.settings.mcp_key {
                return net::Resp::empty(404);
            }
            let version = self.version.clone();
            return net::mcp(self, &req.method, &req.body, &version);
        }
        if path == "/health" {
            return net::Resp::json(200, &json!({ "app": "Inkboard", "version": self.version }));
        }
        let Some(what) = path.strip_prefix("/canvas/") else {
            return net::Resp::empty(404);
        };
        let code = req.query.get("code").cloned().unwrap_or_default();
        if code.is_empty() || self.host_code().as_deref() != Some(code.as_str()) {
            return net::Resp::json(404, &json!({ "error": "No board is shared with that code" }));
        }
        match what {
            "hello" => {
                let name = self.board.as_ref().map(|b| b.title.clone()).unwrap_or_default();
                net::Resp::json(200, &json!({ "name": name }))
            }
            "board" => {
                let since = req.query.get("since").and_then(|s| s.parse::<u64>().ok());
                let mut v = json!({ "revision": self.doc_gen });
                if since != Some(self.doc_gen) {
                    v["shapes"] = Value::Array(
                        self.ed.doc.shapes().iter().map(|s| s.to_json()).collect(),
                    );
                }
                net::Resp::json(200, &v)
            }
            "ops" if req.method == "POST" => {
                let ops = serde_json::from_slice::<Value>(&req.body)
                    .ok()
                    .and_then(|v| v["operations"].as_array().cloned())
                    .unwrap_or_default();
                match net::parse_ops(&ops) {
                    Ok((put, remove)) => {
                        if !put.is_empty() || !remove.is_empty() {
                            self.ed.apply_remote(put, remove);
                            self.take_editor_events();
                        }
                        net::Resp::json(200, &json!({ "revision": self.doc_gen }))
                    }
                    Err(e) => net::Resp::json(400, &json!({ "error": e })),
                }
            }
            _ => net::Resp::empty(404),
        }
    }

    fn take_link(&mut self) {
        let mut snapshot = None;
        let mut status = None;
        if let Some(link) = &self.link {
            while let Ok(ev) = link.events.try_recv() {
                match ev {
                    net::LinkEvent::Snapshot { shapes, batches, .. } => {
                        if batches >= self.ops_sent {
                            snapshot = Some(shapes);
                        }
                    }
                    net::LinkEvent::Status(on) => status = Some(on),
                }
            }
        }
        if let Some(on) = status {
            if self.link_online != Some(on) {
                self.link_online = Some(on);
                self.full = true;
            }
        }
        let Some(shapes) = snapshot else {
            return;
        };
        let theirs: HashMap<String, Value> = shapes
            .into_iter()
            .filter_map(|v| Some((v["id"].as_str()?.to_string(), v)))
            .collect();
        let mine = self.shape_map();
        let mut put = Vec::new();
        for (id, v) in &theirs {
            if mine.get(id) != Some(v) {
                if let Ok(s) = Shape::from_json(v) {
                    put.push(s);
                }
            }
        }
        let remove: Vec<String> = mine.keys().filter(|id| !theirs.contains_key(*id)).cloned().collect();
        // What the host has is now known, so these changes are not sent back.
        self.known = theirs;
        if !put.is_empty() || !remove.is_empty() {
            self.ed.apply_remote(put, remove);
            self.take_editor_events();
        }
    }

    fn take_join(&mut self) {
        let Some(rx) = &self.joining else {
            return;
        };
        let Ok((code, found)) = rx.try_recv() else {
            return;
        };
        self.joining = None;
        match found {
            Ok((host, name)) => {
                let mut extra = Map::new();
                extra.insert("lanShare".into(), json!({ "host": host, "code": code }));
                let title = if name.is_empty() { "Shared board".to_string() } else { name };
                let id = Self::new_board_id();
                let updated = self.timestamp();
                self.send(Job::Create {
                    id,
                    title,
                    updated,
                    extra,
                });
            }
            Err(e) => self.show_toast(&e),
        }
    }


    /// A text button and a filled button, right aligned at `right`.
    fn dialog_buttons(&self, ui: &mut Ui, right: f32, y: f32, cancel: &str, ok: &str) {
        let d = self.d;
        let ow = font::measure(Face::Medium, 14.0 * d, ok).0 + 48.0 * d;
        let cw = font::measure(Face::Medium, 14.0 * d, cancel).0 + 24.0 * d;
        ui.push(
            Rf::new(right - ow, y, ow, 40.0 * d),
            Look::DlgBtn {
                text: ok.into(),
                filled: true,
            },
            Some(Act::Done),
        );
        ui.push(
            Rf::new(right - ow - 8.0 * d - cw, y, cw, 40.0 * d),
            Look::DlgBtn {
                text: cancel.into(),
                filled: false,
            },
            Some(Act::Cancel),
        );
    }

    // ------------------------------------------------------------ drawing

    fn draw(&self, p: &mut Painter, n: &Node) {
        let d = self.d;
        let r = n.rect;
        let middle = |p: &Painter, face: Face, size: f32, top: f32, height: f32| {
            let _ = p;
            let (_, asc, desc) = font::measure(face, size, "Hg");
            top + (height + asc - desc) / 2.0
        };
        match &n.look {
            Look::Scrim(color) => {
                if color >> 24 != 0 {
                    p.rrect(r, 0.0, *color);
                }
            }
            Look::Surface {
                color,
                radius,
                elev,
                border,
            } => {
                if *elev > 0.0 {
                    p.shadow(r, radius * d, d, *elev / 2.0);
                }
                p.rrect(r, radius * d, *color);
                if *border {
                    p.rrect_line(r, radius * d, BORDER, d.max(1.0));
                }
            }
            Look::Btn {
                icon,
                isz,
                active,
                enabled,
            } => {
                let color = if *active {
                    p.rrect(r, 8.0 * d, BLUE);
                    WHITE
                } else if *enabled {
                    ICON
                } else {
                    DISABLED
                };
                if *icon == icon::ERASER {
                    p.eraser(*isz, r.cx(), r.cy(), color);
                } else {
                    p.icon(*icon, *isz, r.cx(), r.cy(), color);
                }
            }
            Look::AppIcon { icon, badge } => {
                p.icon(*icon, 24.0 * d, r.cx(), r.cy(), ON_VARIANT);
                if *badge {
                    p.circle(r.cx() + 9.0 * d, r.cy() - 9.0 * d, 4.0 * d, ERROR);
                }
            }
            Look::Label {
                text,
                face,
                size,
                color,
                center,
            } => {
                let base = middle(p, *face, *size, r.y, r.h);
                if *center {
                    let tw = font::measure(*face, *size, text).0.min(r.w);
                    p.text_fit(*face, *size, text, r.cx() - tw / 2.0, base, r.w, *color);
                } else {
                    p.text_fit(*face, *size, text, r.x, base, r.w, *color);
                }
            }
            Look::Para {
                text,
                face,
                size,
                line,
                color,
                center,
            } => {
                for (i, l) in paint::wrap(*face, *size, text, r.w).iter().enumerate() {
                    let base = middle(p, *face, *size, r.y + *line * i as f32, *line);
                    let x = if *center {
                        r.cx() - font::measure(*face, *size, l).0 / 2.0
                    } else {
                        r.x
                    };
                    p.text(*face, *size, l, x, base, *color);
                }
            }
            Look::Glyph { icon, size, color } => p.icon(*icon, *size, r.cx(), r.cy(), *color),
            Look::Swatch { color, chosen } => {
                if *chosen {
                    p.rrect(r, 8.0 * d, CHOSEN);
                }
                p.circle(r.cx(), r.cy(), 11.0 * d, *color);
            }
            Look::Chip { text, chosen } => {
                if *chosen {
                    p.rrect(r, 8.0 * d, CHOSEN);
                }
                let size = if text.ends_with('%') { 12.0 } else { 14.0 } * d;
                let tw = font::measure(Face::Medium, size, text).0;
                let base = middle(p, Face::Medium, size, r.y, r.h);
                p.text(Face::Medium, size, text, r.cx() - tw / 2.0, base, ICON);
            }
            Look::StyleDot { color, open } => {
                if *open {
                    p.rrect(r, 8.0 * d, CHOSEN);
                }
                p.circle(r.cx(), r.cy(), 11.0 * d, *color);
                p.circle_line(r.cx(), r.cy(), 11.0 * d, 0x33000000, d.max(1.0));
            }
            Look::Rule => {
                let y = (r.cy() - d / 2.0).round();
                p.rrect(Rf::new(r.x, y, r.w, d.max(1.0).round()), 0.0, BORDER);
            }
            Look::Logo => p.logo(r),
            Look::Thumb { id, .. } => {
                p.rrect(r, 8.0 * d, render::BACKGROUND);
                if let Some(t) = self.thumbs.get(id) {
                    let (x, y) = (r.cx() - t.w as f32 / 2.0, r.cy() - t.h as f32 / 2.0);
                    p.image(&t.rgba, t.w, t.h, x, y);
                }
            }
            Look::Fab => {
                p.shadow(r, 16.0 * d, d, 1.5);
                p.rrect(r, 16.0 * d, FAB_BG);
                p.icon(
                    icon::ADD,
                    24.0 * d,
                    r.x + 16.0 * d + 12.0 * d,
                    r.cy(),
                    FAB_FG,
                );
                let base = middle(p, Face::Medium, 14.0 * d, r.y, r.h);
                p.text(
                    Face::Medium,
                    14.0 * d,
                    "New board",
                    r.x + 48.0 * d,
                    base,
                    FAB_FG,
                );
            }
            Look::DlgBtn { text, filled } => {
                let color = if *filled {
                    p.rrect(r, 20.0 * d, PRIMARY);
                    WHITE
                } else {
                    PRIMARY
                };
                let tw = font::measure(Face::Medium, 14.0 * d, text).0;
                let base = middle(p, Face::Medium, 14.0 * d, r.y, r.h);
                p.text(Face::Medium, 14.0 * d, text, r.cx() - tw / 2.0, base, color);
            }
            Look::Field {
                text,
                hint,
                multiline,
            } => {
                let size = 16.0 * d;
                let (mut caret_x, mut caret_y) = (r.x, r.y + 12.0 * d);
                if text.is_empty() {
                    let base = if *multiline {
                        middle(p, Face::Regular, size, r.y + 8.0 * d, 24.0 * d)
                    } else {
                        middle(p, Face::Regular, size, r.y, r.h)
                    };
                    p.text_fit(Face::Regular, size, hint, r.x, base, r.w, HINT);
                    if !*multiline {
                        caret_y = r.cy() - 10.0 * d;
                    } else {
                        caret_y = r.y + 10.0 * d;
                    }
                } else if *multiline {
                    let lines = paint::wrap(Face::Regular, size, text, r.w);
                    let shown = lines.len().saturating_sub(6);
                    for (i, l) in lines[shown..].iter().enumerate() {
                        let top = r.y + 8.0 * d + 24.0 * d * i as f32;
                        p.text(
                            Face::Regular,
                            size,
                            l,
                            r.x,
                            middle(p, Face::Regular, size, top, 24.0 * d),
                            ON_SURFACE,
                        );
                        caret_x = r.x + font::measure(Face::Regular, size, l).0;
                        caret_y = top + 2.0 * d;
                    }
                } else {
                    // Show the end of a long line, where the typing is.
                    let mut shown: &str = text;
                    let mut prefix = "";
                    while font::measure(Face::Regular, size, &format!("{prefix}{shown}")).0
                        > r.w - 4.0 * d
                        && !shown.is_empty()
                    {
                        let mut it = shown.chars();
                        it.next();
                        shown = it.as_str();
                        prefix = "…";
                    }
                    let line = format!("{prefix}{shown}");
                    let base = middle(p, Face::Regular, size, r.y, r.h);
                    caret_x = r.x + p.text(Face::Regular, size, &line, r.x, base, ON_SURFACE);
                    caret_y = r.cy() - 10.0 * d;
                }
                p.rrect(
                    Rf::new(caret_x + d, caret_y, 2.0 * d, 20.0 * d),
                    0.0,
                    PRIMARY,
                );
                p.rrect(
                    Rf::new(r.x, r.bottom() - 2.0 * d, r.w, 2.0 * d),
                    0.0,
                    PRIMARY,
                );
            }
            Look::Item { text, check } => {
                let mut x = r.x + 12.0 * d;
                if let Some(checked) = check {
                    if *checked {
                        p.icon(icon::CHECK, 24.0 * d, x + 12.0 * d, r.cy(), ON_VARIANT);
                    }
                    x += 36.0 * d;
                }
                let base = middle(p, Face::Regular, 14.0 * d, r.y, r.h);
                p.text_fit(
                    Face::Regular,
                    14.0 * d,
                    text,
                    x,
                    base,
                    r.right() - x - 12.0 * d,
                    ON_SURFACE,
                );
            }
            Look::TitleBlock {
                title,
                status,
                failed,
            } => {
                p.text_fit(
                    Face::Medium,
                    14.0 * d,
                    title,
                    r.x + 6.0 * d,
                    r.y + 19.0 * d,
                    r.w - 12.0 * d,
                    ICON,
                );
                p.text_fit(
                    Face::Regular,
                    11.0 * d,
                    status,
                    r.x + 6.0 * d,
                    r.y + 34.0 * d,
                    r.w - 12.0 * d,
                    if *failed { ERROR } else { STATUS },
                );
            }
            Look::Code(text) => {
                p.rrect(r, 8.0 * d, CHOSEN);
                let size = 14.0 * d;
                for (i, l) in wrap_chars(size, text, r.w - 24.0 * d).iter().enumerate() {
                    let base = middle(p, Face::Regular, size, r.y + 8.0 * d + 20.0 * d * i as f32, 20.0 * d);
                    p.text(Face::Regular, size, l, r.x + 12.0 * d, base, ON_SURFACE);
                }
            }
            Look::FontTile { font: f, chosen } => {
                if *chosen {
                    p.rrect(r, 10.0 * d, FAB_BG);
                    p.rrect_line(r, 10.0 * d, PRIMARY, 2.0 * d);
                } else {
                    p.rrect(r, 10.0 * d, WHITE);
                    p.rrect_line(r, 10.0 * d, BORDER, d.max(1.0));
                }
                let name = text::FONT_NAMES[*f as usize];
                let px = (22.0 * d).round() as u32;
                let mut cache = self.font_tiles.borrow_mut();
                let bitmap = cache
                    .entry((*f, px))
                    .or_insert_with(|| text::rasterize(name, px as f64, INK, *f));
                match bitmap {
                    Some(b) if b.width as f32 <= r.w - 16.0 * d => {
                        let x = r.cx() - b.width as f32 / 2.0;
                        let y = r.cy() - b.height as f32 / 2.0;
                        p.image(&b.rgba, b.width, b.height, x, y);
                    }
                    _ => {
                        let tw = font::measure(Face::Regular, 16.0 * d, name).0.min(r.w - 16.0 * d);
                        let base = middle(p, Face::Regular, 16.0 * d, r.y, r.h);
                        p.text_fit(Face::Regular, 16.0 * d, name, r.cx() - tw / 2.0, base, r.w - 16.0 * d, INK);
                    }
                }
            }
            Look::Toast(text) => {
                p.shadow(r, 4.0 * d, d, 1.0);
                p.rrect(r, 4.0 * d, TOAST_BG);
                let base = middle(p, Face::Regular, 14.0 * d, r.y, r.h);
                p.text_fit(
                    Face::Regular,
                    14.0 * d,
                    text,
                    r.x + 16.0 * d,
                    base,
                    r.w - 32.0 * d,
                    TOAST_FG,
                );
            }
        }
        match n.hl {
            Hl::Pressed => p.rrect(r, n.radius(d), 0x1f000000),
            Hl::Focused => {
                p.rrect(r, n.radius(d), 0x14000000);
                let g = 3.0 * d;
                p.rrect_line(
                    Rf::new(r.x - g, r.y - g, r.w + 2.0 * g, r.h + 2.0 * g),
                    n.radius(d) + g,
                    BLUE,
                    2.5 * d,
                );
            }
            Hl::None => {}
        }
    }
}

/// Splits `text` into lines no wider than `width`, breaking anywhere (for
/// addresses and commands, which have few spaces).
fn wrap_chars(size: f32, text: &str, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for c in text.chars() {
        line.push(c);
        if line.chars().count() > 1 && font::measure(Face::Regular, size, &line).0 > width {
            line.pop();
            lines.push(std::mem::take(&mut line));
            line.push(c);
        }
    }
    lines.push(line);
    lines
}

/// The whole board as a PNG no larger than `max_side` on either side.
fn board_png(shapes: &[Arc<Shape>], max_side: u32) -> Option<Vec<u8>> {
    let b = crate::document::scene_bounds(shapes)?;
    let (bw, bh) = (b.width() + 32.0, b.height() + 32.0);
    let zoom = (max_side as f64 / bw.max(bh)).min(1.5);
    let w = ((bw * zoom).ceil() as u32).clamp(16, max_side);
    let h = ((bh * zoom).ceil() as u32).clamp(16, max_side);
    let rgba = render::thumbnail(shapes, w, h);
    let size = tiny_skia::IntSize::from_wh(w, h)?;
    tiny_skia::Pixmap::from_vec(rgba, size)?.encode_png().ok()
}

impl net::Board for App {
    fn info(&self) -> Option<(String, usize, u64)> {
        let b = self.board.as_ref()?;
        Some((b.title.clone(), self.ed.doc.shapes().len(), self.doc_gen))
    }

    fn shapes(&self) -> Vec<Value> {
        if self.board.is_none() {
            return Vec::new();
        }
        self.ed.doc.shapes().iter().map(|s| s.to_json()).collect()
    }

    fn has(&self, id: &str) -> bool {
        self.ed.doc.contains(id)
    }

    fn apply(&mut self, put: Vec<Shape>, remove: Vec<String>) -> u64 {
        if self.board.is_some() && (!put.is_empty() || !remove.is_empty()) {
            self.ed.apply_remote(put, remove);
            self.take_editor_events();
        }
        self.doc_gen
    }

    fn png(&mut self, max_side: u32) -> Option<Vec<u8>> {
        self.board.as_ref()?;
        board_png(self.ed.doc.shapes(), max_side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Quiet;
    impl Platform for Quiet {}

    fn app() -> (App, PathBuf) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "canvas-ui-{}-{}-{}",
            std::process::id(),
            wall_millis(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let app = App::new(
            Box::new(Quiet),
            dir.join("boards"),
            dir.join("settings.json"),
            "0.5.0".into(),
            330,
            Arc::new(|| {}),
        );
        (app, dir)
    }

    fn settle(app: &mut App) {
        for i in 0..50 {
            app.frame(1000 + i);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn tap(app: &mut App, x: f32, y: f32, t: u64) {
        let s = Sample {
            id: 0,
            x,
            y,
            time: t,
            size: 0.0,
            stylus: false,
            eraser: false,
        };
        app.touch(Phase::Down, &[s]);
        app.touch(Phase::Up, &[s]);
        app.frame(t);
    }

    #[test]
    fn versions_compare() {
        assert!(newer("0.5.0", "0.4.9"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("0.5.0", "0.5.0"));
        assert!(!newer("0.4.0", "0.5"));
    }

    #[test]
    fn pause_saves_a_stroke_without_an_intervening_frame() {
        let (mut app, dir) = app();
        app.resize(1280, 800, 1.0);
        app.run(Act::NewBoard);
        app.text_input("Before background", true);
        settle(&mut app);
        let mut sample = Sample {
            id: 9,
            x: 600.0,
            y: 400.0,
            time: 5000,
            stylus: true,
            eraser: false,
            ..Sample::default()
        };
        app.touch(Phase::Down, &[sample]);
        sample.x += 100.0;
        sample.time += 100;
        app.touch(Phase::Move, &[sample]);
        let (tx, rx) = mpsc::sync_channel(1);
        app.pause(tx);
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let board = store::load(&dir.join("boards"), &app.board.as_ref().unwrap().id).unwrap();
        assert_eq!(board.shapes.len(), 1);
        assert!(!app.ed.pointer_down_now());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failed_save_keeps_the_board_open_until_retry_succeeds() {
        let (mut app, dir) = app();
        app.resize(1280, 800, 1.0);
        app.run(Act::NewBoard);
        app.text_input("Keep my work", true);
        settle(&mut app);
        let sample = Sample {
            id: 1,
            x: 600.0,
            y: 400.0,
            time: 5000,
            stylus: true,
            eraser: false,
            ..Sample::default()
        };
        app.touch(Phase::Down, &[sample]);
        app.touch(
            Phase::Up,
            &[Sample {
                x: 700.0,
                time: 5010,
                ..sample
            }],
        );
        let path = dir.join("boards");
        let backup = dir.join("boards-backup");
        std::fs::rename(&path, &backup).unwrap();
        std::fs::write(&path, b"cannot create a board under a regular file").unwrap();
        app.run(Act::Back);
        settle(&mut app);
        assert!(app.is_board_open());
        assert!(matches!(app.save_state, SaveState::Failed));
        assert_eq!(app.ed.doc.shapes().len(), 1);
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&backup, &path).unwrap();
        app.run(Act::Back);
        settle(&mut app);
        assert!(!app.is_board_open());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn req(app: &mut App, method: &str, path: &str, query: &[(&str, &str)], body: Value) -> (u16, Value) {
        let (reply, _rx) = mpsc::sync_channel(1);
        let r = net::Req {
            method: method.into(),
            path: path.into(),
            query: query.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: if body.is_null() { Vec::new() } else { body.to_string().into_bytes() },
            reply,
        };
        let resp = app.answer(&r);
        (resp.status, serde_json::from_slice(&resp.body).unwrap_or(Value::Null))
    }

    #[test]
    fn claude_and_sharing_reach_the_open_board() {
        let (mut app, dir) = app();
        app.resize(1280, 800, 1.5);
        settle(&mut app);
        app.run(Act::NewBoard);
        app.text_input("Team", true);
        settle(&mut app);
        assert!(app.is_board_open());
        let key = app.settings.mcp_key.clone();
        assert_eq!(key.len(), 24);
        // A wrong key is not found; the right one is an MCP server.
        assert_eq!(req(&mut app, "POST", "/mcp/nope", &[], json!({})).0, 404);
        let mcp = format!("/mcp/{key}");
        let (status, init) = req(&mut app, "POST", &mcp, &[], json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}));
        assert_eq!(status, 200);
        assert_eq!(init["result"]["serverInfo"]["name"], "inkboard");
        let add = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_shapes","arguments":{"shapes":[
            {"id":"c1","tool":"rectangle","points":[[0,0],[200,120]]},
            {"id":"c2","tool":"arrow","points":[[220,60],[400,60]]}]}}});
        let (_, added) = req(&mut app, "POST", &mcp, &[], add);
        assert!(added["result"].get("isError").is_none(), "{added}");
        assert_eq!(app.ed.doc.shapes().len(), 2);
        assert!(app.unsaved);
        let (_, image) = req(&mut app, "POST", &mcp, &[], json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_board_image","arguments":{}}}));
        assert_eq!(image["result"]["content"][0]["type"], "image");
        let png = net::unbase64(image["result"]["content"][0]["data"].as_str().unwrap());
        assert_eq!(&png[1..4], b"PNG");

        // Not shared yet: the canvas endpoints refuse.
        assert_eq!(req(&mut app, "GET", "/canvas/hello", &[("code", "123456")], Value::Null).0, 404);
        app.run(Act::Share);
        let code = app.host_code().unwrap();
        assert_eq!(code.len(), 6);
        assert!(matches!(app.overlay, Some(Overlay::Panel { kind: PanelKind::Share, .. })));
        app.frame(5000);
        let (_, hello) = req(&mut app, "GET", "/canvas/hello", &[("code", &code)], Value::Null);
        assert_eq!(hello["name"], "Team");
        let (_, board) = req(&mut app, "GET", "/canvas/board", &[("code", &code), ("since", "0")], Value::Null);
        assert_eq!(board["shapes"].as_array().unwrap().len(), 2);
        let rev = board["revision"].as_u64().unwrap().to_string();
        let (_, same) = req(&mut app, "GET", "/canvas/board", &[("code", &code), ("since", &rev)], Value::Null);
        assert!(same.get("shapes").is_none());
        let ops = json!({"operations":[{"type":"delete","id":"c2"}]});
        let (status, _) = req(&mut app, "POST", "/canvas/ops", &[("code", &code)], ops);
        assert_eq!(status, 200);
        assert_eq!(app.ed.doc.shapes().len(), 1);
        app.run(Act::StopShare);
        assert!(app.host_code().is_none());
        assert_eq!(req(&mut app, "GET", "/canvas/hello", &[("code", &code)], Value::Null).0, 404);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn panels_lay_out_and_choose_fonts() {
        let (mut app, dir) = app();
        app.resize(720, 1280, 2.0);
        settle(&mut app);
        app.run(Act::Claude);
        app.frame(3000);
        assert!(app.nodes.iter().any(|n| matches!(&n.act, Some(Act::Copy(t)) if t.contains("/mcp/"))));
        app.run(Act::Cancel);
        app.panel(PanelKind::Fonts);
        app.frame(3100);
        let tiles = app.nodes.iter().filter(|n| matches!(n.look, Look::FontTile { .. })).count();
        assert!(tiles > 4);
        assert!(app.panel_max_scroll() >= 0.0);
        app.run(Act::Font(3));
        assert_eq!(app.settings.font, 3);
        assert!(app.overlay.is_none());
        // The settings keep the generated keys.
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["mcpKey"], app.settings.mcp_key.as_str());
        assert_eq!(share_code("join 654321 please"), "654321");
        assert_eq!(wrap_chars(14.0, "abcdefghij", 30.0).concat(), "abcdefghij");
        let _ = std::fs::remove_dir_all(dir);
    }


    #[test]
    fn dates_are_civil() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_773), (2026, 11, 16));
    }

    #[test]
    fn create_open_draw_and_return() {
        let (mut app, dir) = app();
        app.resize(1280, 800, 1.5);
        settle(&mut app);
        assert!(app.listed);
        // New board, named through the dialog.
        app.run(Act::NewBoard);
        app.text_input("Plans", true);
        settle(&mut app);
        assert!(app.is_board_open());
        assert_eq!(app.board.as_ref().unwrap().title, "Plans");
        // A stroke in the middle of the board reaches the editor.
        let (cx, cy) = (640.0, 400.0);
        let mut s = Sample {
            id: 1,
            x: cx,
            y: cy,
            time: 2000,
            size: 0.0,
            stylus: true,
            eraser: false,
        };
        app.touch(Phase::Down, &[s]);
        for i in 0..20 {
            s.x += 6.0;
            s.y += (i % 3) as f32;
            s.time += 8;
            app.touch(Phase::Move, &[s]);
            assert!(app.frame(s.time).is_some() || i == 0);
        }
        app.touch(Phase::Up, &[s]);
        app.frame(s.time);
        assert_eq!(app.ed.doc.shapes().len(), 1);
        // Back saves and returns to the library, which lists the mark.
        let (tx, rx) = mpsc::sync_channel(1);
        app.pause(tx);
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert!(app.back());
        settle(&mut app);
        assert!(!app.is_board_open());
        assert_eq!(app.boards.len(), 1);
        assert_eq!(app.boards[0].shapes, 1);
        assert!(!app.back());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn menu_toggles_and_remote_focus() {
        let (mut app, dir) = app();
        app.resize(1920, 1080, 2.0);
        app.run(Act::NewBoard);
        app.text_input("", true);
        settle(&mut app);
        assert!(app.is_board_open());
        let menu = app.node_rect(&Act::Menu);
        tap(&mut app, menu.cx(), menu.cy(), 3000);
        assert!(matches!(app.overlay, Some(Overlay::Menu { .. })));
        let grid = app.node_rect(&Act::Toggle(Opt::Grid));
        tap(&mut app, grid.cx(), grid.cy(), 3100);
        assert!(app.overlay.is_none());
        assert!(!app.settings.grid);
        assert!(!Settings::load(&app.settings_path).grid);
        // The remote moves focus between the controls.
        assert!(app.key(keys::RIGHT));
        app.frame(3200);
        assert!(app.focus.is_some());
        assert!(app.key(keys::RIGHT));
        app.frame(3300);
        assert!(app.nodes.iter().any(|n| n.hl == Hl::Focused));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn library_scrolls_and_deletes() {
        let (mut app, dir) = app();
        app.resize(720, 1280, 2.0);
        for i in 0..9 {
            let id = format!("b{i}");
            let contents = store::encode(
                &id,
                &format!("Board {i}"),
                &format!("2026-11-16T10:00:0{i}.000"),
                &[],
                &Map::new(),
            );
            store::save_atomic(&dir.join("boards"), &id, &contents).unwrap();
        }
        app.send(Job::List);
        settle(&mut app);
        assert_eq!(app.boards.len(), 9);
        let s = Sample {
            id: 0,
            x: 300.0,
            y: 1000.0,
            time: 5000,
            size: 0.0,
            stylus: false,
            eraser: false,
        };
        app.touch(Phase::Down, &[s]);
        app.touch(
            Phase::Move,
            &[Sample {
                y: 600.0,
                time: 5050,
                ..s
            }],
        );
        app.touch(
            Phase::Up,
            &[Sample {
                y: 600.0,
                time: 5060,
                ..s
            }],
        );
        assert!(app.scroll > 100.0);
        let id = app.boards[0].id.clone();
        app.run(Act::AskDelete(id));
        app.run(Act::Done);
        settle(&mut app);
        assert_eq!(app.boards.len(), 8);
        let _ = std::fs::remove_dir_all(dir);
    }
}
