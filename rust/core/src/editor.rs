//! The interactive board: tools, gestures, camera and incremental rendering.
//!
//! Two screen-sized layers are kept:
//!  * `scene`  - background, grid and every committed shape for the current
//!               camera. It is only repainted where something changed, and a
//!               newly finished stroke is drawn straight on top of it, so
//!               adding ink costs the same on a full board as an empty one.
//!  * `frame`  - `scene` plus live feedback (the stroke being drawn, selection
//!               frames, shapes being dragged). This is what goes on screen.
//!
//! Input handlers only record *damage*; [`Editor::render`] repaints exactly
//! the damaged rectangle and returns it so the host can post just that part.

use crate::document::{scene_bounds, Applied, Document};
use crate::geom::{IRect, Pt, Rect};
use crate::recognize::recognize_shape;
use crate::render::{self, Layer, Target, View};
use crate::shape::{Shape, Tool};
use crate::text;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tiny_skia::Transform;

pub const EV_STATE: u32 = 1; // tool, selection, undo/redo availability, style
pub const EV_DOC: u32 = 2; // shapes changed: schedule a save
pub const EV_TEXT: u32 = 4; // a text request is waiting
pub const EV_HAPTIC: u32 = 8; // a held stroke snapped to a shape
pub const EV_VIEW: u32 = 16; // zoom changed
pub const EV_INK: u32 = 32; // handwriting is waiting to be read

/// What the touch panel says about one contact.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Contact {
    /// Longest side of the touched area in buffer pixels; 0 when unknown.
    pub size: f64,
    pub stylus: bool,
}

/// Handwriting the host should read and answer with [`Editor::commit_ink`].
#[derive(Clone, Debug, PartialEq)]
pub struct InkRequest {
    pub token: u32,
    pub ids: Vec<String>,
    pub strokes: Vec<Vec<Pt>>,
    pub bounds: Rect,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PenMode {
    /// Keeps the stroke as ink.
    Ink,
    /// Replaces a rough outline with a clean shape as soon as the pen lifts.
    Shape,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    None,
    Draw,
    Shape,
    Move,
    Resize,
    Marquee,
    Erase,
    Pan,
    Pinch,
    Text,
    Blocked,
}

/// The host should ask the user for text and call [`Editor::commit_text`].
#[derive(Clone, Debug, PartialEq)]
pub struct TextRequest {
    /// Shape being edited, or empty for new text.
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub text: String,
}

/// How long the pen rests before handwriting is read, in milliseconds.
const INK_PAUSE: u64 = 1100;

const EMPTY: IRect = IRect { x0: 0, y0: 0, x1: 0, y1: 0 };

pub struct Editor {
    pub doc: Document,
    tool: Tool,
    pen_mode: PenMode,
    color: u32,
    width: f64,
    filled: bool,
    grid: bool,
    snap_shapes: bool,
    auto_convert: bool,
    auto_text: bool,
    font: u8,
    palm_rejection: bool,
    /// Contacts at least this big (logical pixels) are a palm.
    palm_size: f64,

    // Camera, in logical pixels (buffer pixels / dpr).
    zoom: f64,
    camera: Pt,
    dpr: f64,

    // Pointer state.
    pointers: Vec<(i32, Pt)>,
    ignored: Vec<i32>,
    mode: Mode,
    primary: Option<i32>,
    down_stamp: u64,
    travel: f64,
    start_world: Pt,
    anchor_world: Pt,
    start_zoom: f64,
    pinch_distance: f64,
    stylus: Option<i32>,
    stylus_lifted: Option<u64>,
    primary_size: f64,

    // Handwriting waiting to become text.
    pending: Vec<String>,
    pending_bounds: Rect,
    ink_deadline: Option<u64>,
    ink_requests: VecDeque<InkRequest>,
    ink_token: u32,
    /// The last stroke that was tidied into a shape on its own, kept so it
    /// can go back to ink if it turns out to be the first letter of a word.
    last_auto: Option<(Shape, u64)>,

    // In-progress ink.
    draft: Option<Vec<Pt>>,
    draft_bounds: Rect,
    draft_id: String,
    last_screen: Pt,
    still_anchor: Pt,
    snapped: Option<Shape>,
    snap_deadline: Option<u64>,
    draft_end: Pt,

    // Selection transforms and erasing.
    moving: Vec<Arc<Shape>>,
    dragging: bool,
    delta: Pt,
    resize_origin: Pt,
    resize_bounds: Rect,
    handle: usize,
    scale_x: f64,
    scale_y: f64,
    marquee: Option<Rect>,
    hidden: HashSet<String>,
    last_erase: Pt,
    last_tap_id: Option<String>,
    last_tap_time: u64,

    // Rendering.
    scene: Layer,
    frame: Layer,
    scene_view: Option<View>,
    scene_damage: Vec<Rect>,
    damage: IRect,
    overlay_dirty: bool,
    overlay_prev: IRect,

    events: u32,
    text_request: Option<TextRequest>,
    client_id: String,
    sequence: u64,
    /// Number of full scene repaints, for tests and diagnostics.
    pub full_repaints: u64,
}

fn base36(mut n: u128) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

fn micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0)
}

fn corners(r: &Rect) -> [Pt; 4] {
    [r.top_left(), r.top_right(), r.bottom_left(), r.bottom_right()]
}

fn scaled(s: &Shape, origin: Pt, sx: f64, sy: f64) -> Shape {
    let points = s
        .points
        .iter()
        .map(|p| Pt::new(origin.x + (p.x - origin.x) * sx, origin.y + (p.y - origin.y) * sy))
        .collect();
    let mut next = s.with_points(points);
    if s.tool == Tool::Text {
        let size = (s.font_size * sy).clamp(6.0, 2000.0);
        next = next.with_text(s.text.clone(), size);
    }
    next
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new()
    }
}

impl Editor {
    pub fn new() -> Editor {
        let seed = micros() ^ ((&EMPTY as *const IRect as usize as u128) << 20);
        Editor {
            doc: Document::new(),
            tool: Tool::Pen,
            pen_mode: PenMode::Ink,
            color: 0xff1d1d1d,
            width: 3.5,
            filled: false,
            grid: true,
            snap_shapes: true,
            auto_convert: true,
            auto_text: true,
            font: 0,
            palm_rejection: true,
            palm_size: 52.0,
            zoom: 1.0,
            camera: Pt::ZERO,
            dpr: 1.0,
            pointers: Vec::new(),
            ignored: Vec::new(),
            mode: Mode::None,
            primary: None,
            down_stamp: 0,
            travel: 0.0,
            start_world: Pt::ZERO,
            anchor_world: Pt::ZERO,
            start_zoom: 1.0,
            pinch_distance: 1.0,
            stylus: None,
            stylus_lifted: None,
            primary_size: 0.0,
            pending: Vec::new(),
            pending_bounds: Rect::ZERO,
            ink_deadline: None,
            ink_requests: VecDeque::new(),
            ink_token: 0,
            last_auto: None,
            draft: None,
            draft_bounds: Rect::ZERO,
            draft_id: String::new(),
            last_screen: Pt::ZERO,
            still_anchor: Pt::ZERO,
            snapped: None,
            snap_deadline: None,
            draft_end: Pt::ZERO,
            moving: Vec::new(),
            dragging: false,
            delta: Pt::ZERO,
            resize_origin: Pt::ZERO,
            resize_bounds: Rect::ZERO,
            handle: 0,
            scale_x: 1.0,
            scale_y: 1.0,
            marquee: None,
            hidden: HashSet::new(),
            last_erase: Pt::ZERO,
            last_tap_id: None,
            last_tap_time: 0,
            scene: Layer::new(0, 0),
            frame: Layer::new(0, 0),
            scene_view: None,
            scene_damage: Vec::new(),
            damage: EMPTY,
            overlay_dirty: false,
            overlay_prev: EMPTY,
            events: 0,
            text_request: None,
            client_id: format!("r{}", base36(seed & 0xffff_ffff_ffff)),
            sequence: 0,
            full_repaints: 0,
        }
    }

    // ------------------------------------------------------------ accessors

    pub fn tool(&self) -> Tool {
        self.tool
    }
    pub fn pen_mode(&self) -> PenMode {
        self.pen_mode
    }
    pub fn color(&self) -> u32 {
        self.color
    }
    pub fn stroke_width(&self) -> f64 {
        self.width
    }
    pub fn filled(&self) -> bool {
        self.filled
    }
    pub fn grid(&self) -> bool {
        self.grid
    }
    pub fn snap_shapes(&self) -> bool {
        self.snap_shapes
    }
    pub fn zoom(&self) -> f64 {
        self.zoom
    }
    pub fn auto_convert(&self) -> bool {
        self.auto_convert
    }
    pub fn font(&self) -> u8 {
        self.font
    }
    pub fn palm_rejection(&self) -> bool {
        self.palm_rejection
    }
    pub fn selected_text_count(&self) -> usize {
        self.doc.selected_shapes().iter().filter(|s| s.tool == Tool::Text).count()
    }
    pub fn take_ink_request(&mut self) -> Option<InkRequest> {
        self.ink_requests.pop_front()
    }
    pub fn camera(&self) -> Pt {
        self.camera
    }
    pub fn pointer_down_now(&self) -> bool {
        !self.pointers.is_empty() || !self.ignored.is_empty()
    }
    pub fn frame(&self) -> &Layer {
        &self.frame
    }
    /// Events raised since the last call.
    pub fn take_events(&mut self) -> u32 {
        std::mem::take(&mut self.events)
    }
    pub fn take_text_request(&mut self) -> Option<TextRequest> {
        self.text_request.take()
    }
    /// When [`Editor::tick`] next needs to run, in the caller's milliseconds.
    pub fn next_deadline(&self) -> Option<u64> {
        match (self.snap_deadline, self.ink_deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn view(&self) -> View {
        View { cam: self.camera, zoom: self.zoom, dpr: self.dpr }
    }
    fn viewport(&self) -> (f64, f64) {
        (self.scene.w as f64 / self.dpr, self.scene.h as f64 / self.dpr)
    }
    pub fn world(&self, screen: Pt) -> Pt {
        (screen - self.camera) / self.zoom
    }
    fn next_id(&mut self) -> String {
        self.sequence += 1;
        format!("{}-{}-{}", self.client_id, base36(micros()), self.sequence - 1)
    }

    // ------------------------------------------------------- host commands

    /// Sets the buffer size in pixels and how many pixels make one logical
    /// pixel (the unit board coordinates are stored in at zoom 1).
    pub fn resize(&mut self, w: u32, h: u32, dpr: f64) {
        if self.scene.w == w && self.scene.h == h && self.dpr == dpr {
            return;
        }
        self.dpr = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
        self.scene = Layer::new(w, h);
        self.frame = Layer::new(w, h);
        self.invalidate_all();
    }

    /// Forgets cached pixels, for example after the surface was recreated.
    pub fn invalidate_all(&mut self) {
        self.scene_view = None;
        self.overlay_dirty = true;
    }

    pub fn load(&mut self, shapes: Vec<Shape>) -> Result<(), String> {
        self.cancel_action();
        self.pointers.clear();
        self.ignored.clear();
        self.mode = Mode::None;
        self.forget_pending();
        self.ink_requests.clear();
        self.doc.load(shapes)?;
        self.invalidate_all();
        self.events |= EV_STATE;
        Ok(())
    }

    pub fn set_tool(&mut self, tool: Tool, pen_mode: PenMode) {
        self.cancel_action();
        self.flush_pending();
        self.last_auto = None;
        self.tool = tool;
        if tool == Tool::Pen {
            self.pen_mode = pen_mode;
        }
        if tool != Tool::Select {
            self.doc.select(None);
        }
        self.overlay_dirty = true;
        self.events |= EV_STATE;
    }

    /// Changes the current style and restyles the selection.
    pub fn apply_style(&mut self, color: Option<u32>, width: Option<f64>, filled: Option<bool>) {
        self.color = color.unwrap_or(self.color);
        self.width = width.filter(|w| w.is_finite() && *w > 0.0).unwrap_or(self.width);
        self.filled = filled.unwrap_or(self.filled);
        let restyled: Vec<Shape> = self
            .doc
            .selected_shapes()
            .iter()
            .map(|s| {
                let mut n = (**s).clone();
                n.color = color.unwrap_or(n.color);
                n.width = width.unwrap_or(n.width);
                n.filled = filled.unwrap_or(n.filled);
                n
            })
            .filter(|n| !self.doc.get(&n.id).is_some_and(|old| **old == *n))
            .collect();
        if !restyled.is_empty() {
            let applied = self.doc.replace_many(restyled);
            self.on_applied(applied);
        }
        self.events |= EV_STATE;
    }

    pub fn set_grid(&mut self, grid: bool) {
        if self.grid != grid {
            self.grid = grid;
            self.invalidate_all();
        }
    }

    pub fn set_snap_shapes(&mut self, snap: bool) {
        self.snap_shapes = snap;
    }

    /// When on, pen strokes that look like a shape become that shape.
    pub fn set_auto_convert(&mut self, auto: bool) {
        self.auto_convert = auto;
        self.events |= EV_STATE;
    }

    /// When on, handwriting becomes typed text after a short pause.
    pub fn set_auto_text(&mut self, auto: bool) {
        if !auto {
            self.forget_pending();
        }
        self.auto_text = auto;
        self.events |= EV_STATE;
    }

    pub fn auto_text(&self) -> bool {
        self.auto_text
    }

    /// `size` is the smallest contact, in logical pixels, treated as a palm.
    pub fn set_palm_rejection(&mut self, on: bool, size: Option<f64>) {
        self.palm_rejection = on;
        if let Some(size) = size.filter(|s| s.is_finite() && *s >= 8.0) {
            self.palm_size = size;
        }
        self.events |= EV_STATE;
    }

    /// Chooses the font for new text and for the selected text.
    pub fn set_font(&mut self, font: u8) {
        if font as usize >= text::FONTS.len() {
            return;
        }
        self.font = font;
        let changed: Vec<Shape> = self
            .doc
            .selected_shapes()
            .iter()
            .filter(|s| s.tool == Tool::Text && s.font != font)
            .map(|s| (**s).clone().with_font(font))
            .collect();
        if !changed.is_empty() {
            let applied = self.doc.replace_many(changed);
            self.on_applied(applied);
        }
        self.events |= EV_STATE;
    }

    /// Makes the selected text larger or smaller.
    pub fn scale_text(&mut self, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let changed: Vec<Shape> = self
            .doc
            .selected_shapes()
            .iter()
            .filter(|s| s.tool == Tool::Text)
            .map(|s| (**s).clone().with_text(s.text.clone(), (s.font_size * factor).clamp(6.0, 2000.0)))
            .collect();
        if !changed.is_empty() {
            let applied = self.doc.replace_many(changed);
            self.on_applied(applied);
        }
    }

    pub fn undo(&mut self) {
        if self.mode != Mode::None {
            return;
        }
        self.forget_pending();
        let applied = self.doc.undo();
        self.on_applied(applied);
    }

    pub fn redo(&mut self) {
        if self.mode != Mode::None {
            return;
        }
        self.forget_pending();
        let applied = self.doc.redo();
        self.on_applied(applied);
    }

    pub fn delete_selection(&mut self) {
        let ids: Vec<String> = self.doc.selected_ids().to_vec();
        let applied = self.doc.remove_many(ids);
        self.on_applied(applied);
    }

    /// Fits the whole board in view.
    pub fn fit(&mut self) {
        let (vw, vh) = self.viewport();
        match self.doc.bounds() {
            Some(b) if vw > 0.0 && vh > 0.0 => {
                self.zoom = f64::min(
                    (vw - 160.0) / f64::max(b.width(), 1.0),
                    (vh - 200.0) / f64::max(b.height(), 1.0),
                )
                .clamp(0.1, 2.0);
                self.camera = Pt::new(vw / 2.0, vh / 2.0) - b.center() * self.zoom;
            }
            _ => {
                self.zoom = 1.0;
                self.camera = Pt::ZERO;
            }
        }
        self.view_changed();
    }

    /// Zooms about `focus` (logical screen point), or the centre of the view.
    pub fn zoom_to(&mut self, next: f64, focus: Option<Pt>) {
        let (vw, vh) = self.viewport();
        let point = focus.unwrap_or(Pt::new(vw / 2.0, vh / 2.0));
        let anchor = self.world(point);
        self.zoom = next.clamp(0.1, 8.0);
        self.camera = point - anchor * self.zoom;
        self.view_changed();
    }

    /// Adds new text at world (`x`, `y`), or changes the text of shape `id`.
    pub fn commit_text(&mut self, id: &str, x: f64, y: f64, text: &str) {
        if text.is_empty() {
            return;
        }
        if !id.is_empty() {
            if let Some(existing) = self.doc.get(id).cloned() {
                let next = (*existing).clone().with_text(text, existing.font_size);
                let applied = self.doc.replace(next);
                self.on_applied(applied);
            }
            return;
        }
        let id = self.next_id();
        let size = (28.0 / self.zoom).clamp(12.0, 400.0);
        let shape = Shape::new(id.clone(), Tool::Text, vec![Pt::new(x, y)])
            .styled(self.color, 3.0, false)
            .with_font(self.font)
            .with_text(text, size);
        let applied = self.doc.add(shape);
        self.on_applied(applied);
        self.tool = Tool::Select;
        self.doc.select(Some(&id));
        self.overlay_dirty = true;
        self.events |= EV_STATE;
    }

    // -------------------------------------------------------------- pointers

    /// `pos` is in buffer pixels; `time` is a monotonic clock in milliseconds.
    pub fn pointer_down(&mut self, id: i32, pos: Pt, time: u64) {
        self.pointer_down_with(id, pos, time, Contact::default());
    }

    fn is_palm(&self, contact: Contact) -> bool {
        self.palm_rejection && !contact.stylus && contact.size / self.dpr >= self.palm_size
    }

    /// Stops whatever the touches on the panel were doing and ignores them
    /// until they lift.
    fn dismiss_pointers(&mut self) {
        self.cancel_action();
        let ids: Vec<i32> = self.pointers.drain(..).map(|(id, _)| id).collect();
        self.ignored.extend(ids);
        self.mode = Mode::None;
        self.primary = None;
    }

    pub fn pointer_down_with(&mut self, id: i32, pos: Pt, time: u64, contact: Contact) {
        let pos = pos / self.dpr;
        if self.palm_rejection {
            if contact.stylus {
                // The pen always wins: anything already touching is the hand
                // holding it.
                if self.stylus.is_none() && !self.pointers.is_empty() {
                    self.dismiss_pointers();
                }
                self.stylus = Some(id);
            } else if self.stylus.is_some() || self.stylus_lifted.is_some_and(|t| time.saturating_sub(t) < 350) {
                self.ignored.push(id);
                return;
            }
            if self.is_palm(contact) {
                self.ignored.push(id);
                return;
            }
            // A contact far bigger than the finger that is drawing is a palm.
            let size = contact.size / self.dpr;
            if !self.pointers.is_empty() && self.primary_size > 0.0 && size >= self.primary_size * 2.5 && size >= 28.0 {
                self.ignored.push(id);
                return;
            }
        }
        if self.pointers.is_empty() {
            self.ignored.clear();
            if self.mode == Mode::Blocked {
                self.mode = Mode::None;
            }
        }
        if self.mode == Mode::Blocked || self.pointers.len() >= 2 {
            self.ignored.push(id);
            return;
        }
        if self.pointers.len() == 1 {
            // A second touch only becomes a pinch if it lands right as the
            // gesture starts. Later contacts (a resting palm or sleeve on a
            // large touch panel) are ignored so they cannot interrupt a stroke.
            let early = time.saturating_sub(self.down_stamp) < 220 && self.travel < 36.0;
            let interruptible = matches!(self.mode, Mode::Pan | Mode::None | Mode::Text | Mode::Marquee);
            if !interruptible && !early {
                self.ignored.push(id);
                return;
            }
            self.cancel_action();
            self.pointers.push((id, pos));
            let (a, b) = (self.pointers[0].1, self.pointers[1].1);
            self.pinch_distance = f64::max(24.0, a.dist(b));
            self.anchor_world = self.world((a + b) / 2.0);
            self.start_zoom = self.zoom;
            self.mode = Mode::Pinch;
            return;
        }
        self.pointers.push((id, pos));
        self.primary = Some(id);
        self.primary_size = contact.size / self.dpr;
        self.down_stamp = time;
        self.travel = 0.0;
        self.begin(pos, time);
    }

    pub fn pointer_move(&mut self, id: i32, pos: Pt, time: u64) {
        self.pointer_move_with(id, pos, time, Contact::default());
    }

    pub fn pointer_move_with(&mut self, id: i32, pos: Pt, time: u64, contact: Contact) {
        let pos = pos / self.dpr;
        if self.ignored.contains(&id) {
            return;
        }
        // A hand that lands edge first only reports its full size a moment
        // later: drop what it started.
        if self.is_palm(contact)
            && time.saturating_sub(self.down_stamp) < 450
            && self.pointers.iter().any(|(p, _)| *p == id)
        {
            if Some(id) == self.primary || self.mode == Mode::Pinch {
                self.dismiss_pointers();
            } else {
                self.pointers.retain(|(p, _)| *p != id);
                self.ignored.push(id);
            }
            return;
        }
        let Some(slot) = self.pointers.iter_mut().find(|(p, _)| *p == id) else {
            return;
        };
        let previous = slot.1;
        slot.1 = pos;
        if self.mode == Mode::Pinch {
            if self.pointers.len() < 2 {
                return;
            }
            let (a, b) = (self.pointers[0].1, self.pointers[1].1);
            let focal = (a + b) / 2.0;
            self.zoom = (self.start_zoom * a.dist(b) / self.pinch_distance).clamp(0.1, 8.0);
            self.camera = focal - self.anchor_world * self.zoom;
            self.view_changed();
            return;
        }
        if Some(id) != self.primary {
            return;
        }
        self.travel += pos.dist(previous);
        self.continue_(pos, time);
    }

    pub fn pointer_up(&mut self, id: i32, pos: Pt, time: u64, cancelled: bool) {
        let pos = pos / self.dpr;
        if self.stylus == Some(id) {
            self.stylus = None;
            self.stylus_lifted = Some(time);
        }
        if let Some(i) = self.ignored.iter().position(|p| *p == id) {
            self.ignored.remove(i);
            return;
        }
        let Some(i) = self.pointers.iter().position(|(p, _)| *p == id) else {
            return;
        };
        self.pointers.remove(i);
        if self.mode == Mode::Pinch {
            self.mode = if self.pointers.is_empty() { Mode::None } else { Mode::Blocked };
            return;
        }
        if self.mode == Mode::Blocked {
            if self.pointers.is_empty() {
                self.mode = Mode::None;
            }
            return;
        }
        if Some(id) == self.primary {
            if cancelled {
                self.cancel_action();
            } else {
                self.finish(pos, time);
            }
            self.mode = Mode::None;
            self.primary = None;
            if !self.pending.is_empty() && self.ink_deadline.is_none() {
                self.ink_deadline = Some(time + INK_PAUSE);
            }
        }
    }

    /// Preserve the last sample when Android backgrounds the app mid-stroke.
    pub fn finish_input(&mut self, now: u64) {
        let pointers = self.pointers.clone();
        for (id, pos) in pointers {
            self.pointer_up(id, pos * self.dpr, now, false);
        }
        self.ignored.clear();
    }

    /// Runs timers; call when [`Editor::next_deadline`] has passed.
    pub fn tick(&mut self, now: u64) {
        if self.snap_deadline.is_some_and(|d| now >= d) {
            self.snap_deadline = None;
            self.try_snap();
        }
        if self.ink_deadline.is_some_and(|d| now >= d) {
            if self.pointers.is_empty() {
                self.flush_pending();
            } else {
                self.ink_deadline = Some(now + 300);
            }
        }
    }

    // ----------------------------------------------- automatic conversion

    fn forget_pending(&mut self) {
        self.pending.clear();
        self.ink_deadline = None;
        self.last_auto = None;
    }

    /// Height used to judge whether a stroke belongs to the writing so far.
    fn line_height(&self, r: &Rect) -> f64 {
        r.height().max(36.0 / self.zoom)
    }

    /// True when `b` continues the line of writing that covers `group`.
    fn continues(&self, b: &Rect, group: &Rect) -> bool {
        let h = self.line_height(group);
        let reach = Rect::ltrb(group.l - h * 1.5, group.t - h * 0.7, group.r + h * 2.5, group.b + h * 0.7);
        reach.overlaps(b) && b.height() <= h * 2.2
    }

    /// The clean shape a finished stroke should become, if any.
    fn auto_shape(&self, raw: &Shape) -> Option<Shape> {
        let shape = recognize_shape(raw)?;
        let b = raw.bounds();
        let straight = shape.tool != Tool::Rectangle && shape.tool != Tool::Ellipse;
        if !self.pending.is_empty() && self.continues(&b, &self.pending_bounds) {
            // In the middle of writing only a frame around the words or a
            // long underline is a shape; "o", "l" and "-" are letters.
            let g = self.pending_bounds;
            let encloses = b.l <= g.l && b.r >= g.r && b.t <= g.t && b.b >= g.b;
            let long = straight && b.longest_side() >= self.line_height(&g) * 2.5;
            return if encloses || long { Some(shape) } else { None };
        }
        let (vw, vh) = self.viewport();
        let side = vw.min(vh);
        let least = if straight { f64::max(130.0, side * 0.15) } else { f64::max(90.0, side * 0.10) };
        if b.longest_side() * self.zoom >= least {
            Some(shape)
        } else {
            None
        }
    }

    /// Adds a finished ink stroke to the handwriting waiting to be read.
    fn note_ink(&mut self, id: &str, bounds: Rect, time: u64) {
        if !self.pending.is_empty() && !self.continues(&bounds, &self.pending_bounds) {
            self.flush_pending();
        }
        self.pending_bounds = if self.pending.is_empty() { bounds } else { self.pending_bounds.union(&bounds) };
        self.pending.push(id.to_string());
        self.ink_deadline = Some(time + INK_PAUSE);
    }

    /// Hands the waiting handwriting to the host.
    fn flush_pending(&mut self) {
        self.ink_deadline = None;
        let ids = std::mem::take(&mut self.pending);
        let shapes: Vec<Arc<Shape>> =
            ids.iter().filter_map(|id| self.doc.get(id)).filter(|s| s.tool == Tool::Pen).cloned().collect();
        let Some(bounds) = scene_bounds(&shapes) else { return };
        // Something that fills much of the screen is a drawing, not writing.
        let (_, vh) = self.viewport();
        if vh > 0.0 && bounds.height() * self.zoom > vh * 0.45 {
            return;
        }
        self.ink_token = self.ink_token.wrapping_add(1);
        self.ink_requests.push_back(InkRequest {
            token: self.ink_token,
            ids: shapes.iter().map(|s| s.id.clone()).collect(),
            strokes: shapes.iter().map(|s| s.points.clone()).collect(),
            bounds,
        });
        self.events |= EV_INK;
    }

    /// Number of selected pen strokes.
    pub fn selected_ink_count(&self) -> usize {
        self.doc.selected_shapes().iter().filter(|s| s.tool == Tool::Pen).count()
    }

    /// Asks the host to read the selected pen strokes as handwriting, even
    /// when automatic conversion is off.
    pub fn convert_selection(&mut self) {
        let shapes: Vec<Arc<Shape>> = self.doc.selected_shapes().into_iter().filter(|s| s.tool == Tool::Pen).collect();
        let Some(bounds) = scene_bounds(&shapes) else { return };
        self.ink_token = self.ink_token.wrapping_add(1);
        self.ink_requests.push_back(InkRequest {
            token: self.ink_token,
            ids: shapes.iter().map(|s| s.id.clone()).collect(),
            strokes: shapes.iter().map(|s| s.points.clone()).collect(),
            bounds,
        });
        self.events |= EV_INK;
    }

    /// Replaces the strokes of an [`InkRequest`] with typed `text`, as one
    /// undo step. Does nothing if any of the strokes has since changed.
    pub fn commit_ink(&mut self, ids: &[String], text: &str) {
        let text = text.trim();
        if text.is_empty() || ids.is_empty() {
            return;
        }
        let mut shapes = Vec::new();
        for id in ids {
            match self.doc.get(id) {
                Some(s) if s.tool == Tool::Pen => shapes.push(s.clone()),
                _ => return,
            }
        }
        let bounds = scene_bounds(&shapes).unwrap();
        let letters = text.chars().count().max(1) as f64;
        // Tall enough to match the handwriting; a lone dash or dot has no
        // height, so its width is used instead.
        let by_height = bounds.height() / 0.8;
        let by_width = bounds.width() / (letters * 0.55) * 0.8;
        let size = by_height.max(by_width.min(by_height * 3.0 + 14.0 / self.zoom)).clamp(8.0, 2000.0);
        let id = self.next_id();
        let (_, h) = text::measure(text, size, self.font);
        let origin = Pt::new(bounds.l, bounds.center().y - h / 2.0);
        let typed = Shape::new(id, Tool::Text, vec![origin])
            .styled(shapes[0].color, 3.0, false)
            .with_font(self.font)
            .with_text(text, size);
        let applied = self.doc.swap(ids.to_vec(), vec![typed]);
        self.on_applied(applied);
    }

    fn begin(&mut self, position: Pt, time: u64) {
        let w = self.world(position);
        self.start_world = w;
        match self.tool {
            Tool::Hand => {
                self.mode = Mode::Pan;
                self.anchor_world = w;
            }
            Tool::Pen => {
                self.mode = Mode::Draw;
                self.draft_id = self.next_id();
                self.draft = Some(vec![w]);
                self.draft_bounds = Rect::from_points(w, w);
                self.last_screen = position;
                self.snapped = None;
                self.ink_deadline = None;
                self.reset_still(position, time);
                self.damage_world(&Rect::from_points(w, w), self.width / 2.0);
            }
            Tool::Rectangle | Tool::Ellipse | Tool::Arrow => {
                self.mode = Mode::Shape;
                self.draft_end = w;
            }
            Tool::Text => self.mode = Mode::Text,
            Tool::Eraser => {
                self.mode = Mode::Erase;
                self.last_erase = w;
                self.erase_at(w);
            }
            Tool::Select => self.begin_select(position, w),
        }
    }

    fn begin_select(&mut self, position: Pt, w: Pt) {
        let selected = self.doc.selected_shapes();
        let bounds = scene_bounds(&selected);
        self.dragging = false;
        self.delta = Pt::ZERO;
        self.scale_x = 1.0;
        self.scale_y = 1.0;
        self.overlay_dirty = true;
        if let Some(b) = bounds {
            let frame = b.inflate(6.0 / self.zoom);
            let cs = corners(&frame);
            for (i, c) in cs.iter().enumerate() {
                if (*c * self.zoom + self.camera).dist(position) <= 22.0 {
                    self.mode = Mode::Resize;
                    self.handle = i;
                    self.resize_bounds = frame;
                    self.resize_origin = cs[3 - i];
                    self.moving = selected;
                    return;
                }
            }
        }
        if let Some(hit) = self.doc.hit_test(w, 12.0 / self.zoom) {
            if !self.doc.is_selected(&hit.id) {
                self.doc.select(Some(&hit.id));
                self.events |= EV_STATE;
            }
            self.moving = self.doc.selected_shapes();
            self.mode = Mode::Move;
        } else if bounds.is_some_and(|b| selected.len() > 1 && b.inflate(6.0 / self.zoom).contains(w)) {
            self.moving = selected;
            self.mode = Mode::Move;
        } else {
            if self.doc.select(None) {
                self.events |= EV_STATE;
            }
            self.mode = Mode::Marquee;
            self.marquee = Some(Rect::from_points(w, w));
        }
    }

    fn continue_(&mut self, position: Pt, time: u64) {
        let w = self.world(position);
        match self.mode {
            Mode::Draw => {
                if self.snapped.is_some() || (position - self.last_screen).len2() < 1.0 {
                    return;
                }
                let Some(draft) = self.draft.as_mut() else { return };
                draft.push(w);
                let n = draft.len();
                // The new curve piece replaces the provisional tail, so the
                // last three points bound everything that changed.
                let mut area = Rect::from_points(draft[n - 2], w);
                if n >= 3 {
                    area = area.union(&Rect::from_points(draft[n - 3], draft[n - 3]));
                }
                self.draft_bounds = self.draft_bounds.union(&Rect::from_points(w, w));
                self.last_screen = position;
                self.damage_world(&area, self.width / 2.0);
                if position.dist(self.still_anchor) > 16.0 {
                    self.reset_still(position, time);
                }
            }
            Mode::Shape => {
                self.draft_end = w;
                self.overlay_dirty = true;
            }
            Mode::Erase => {
                let steps = ((w.dist(self.last_erase) * self.zoom / 8.0).ceil() as i64).max(1);
                for i in 1..=steps {
                    let p = self.last_erase + (w - self.last_erase) * (i as f64 / steps as f64);
                    self.erase_at(p);
                }
                self.last_erase = w;
            }
            Mode::Move => {
                self.delta = w - self.start_world;
                self.start_dragging();
                self.overlay_dirty = true;
            }
            Mode::Resize => {
                let corner = corners(&self.resize_bounds)[self.handle];
                let (dx, dy) = (corner.x - self.resize_origin.x, corner.y - self.resize_origin.y);
                self.scale_x =
                    if dx.abs() < 1.0 { 1.0 } else { ((w.x - self.resize_origin.x) / dx).clamp(0.05, 100.0) };
                self.scale_y =
                    if dy.abs() < 1.0 { 1.0 } else { ((w.y - self.resize_origin.y) / dy).clamp(0.05, 100.0) };
                self.start_dragging();
                self.overlay_dirty = true;
            }
            Mode::Marquee => {
                self.marquee = Some(Rect::from_points(self.start_world, w));
                self.overlay_dirty = true;
            }
            Mode::Pan => {
                self.camera = position - self.anchor_world * self.zoom;
                self.view_changed();
            }
            Mode::None | Mode::Pinch | Mode::Text | Mode::Blocked => {}
        }
    }

    fn start_dragging(&mut self) {
        if self.dragging || self.travel < 4.0 {
            return;
        }
        self.dragging = true;
        let moving = std::mem::take(&mut self.moving);
        for s in &moving {
            self.hidden.insert(s.id.clone());
            self.scene_damage.push(render::paint_bounds(s));
        }
        self.moving = moving;
    }

    fn finish(&mut self, position: Pt, time: u64) {
        let w = self.world(position);
        self.overlay_dirty = true;
        match self.mode {
            Mode::Draw => {
                self.snap_deadline = None;
                let Some(points) = self.draft.take() else { return };
                let raw = Shape::new(std::mem::take(&mut self.draft_id), Tool::Pen, points)
                    .styled(self.color, self.width, false);
                let held = self.snapped.take();
                let mut auto = false;
                // A shape that is followed straight away by more writing was
                // a letter after all: put its ink back and read it as text.
                if let Some((first, at)) = self.last_auto.take() {
                    let tidy = self.doc.get(&first.id).is_some_and(|s| s.tool != Tool::Pen || s.points.len() == 2);
                    if self.auto_text
                        && held.is_none()
                        && tidy
                        && self.pending.is_empty()
                        && time.saturating_sub(at) < 1600
                        && self.continues(&raw.bounds(), &first.bounds())
                        && first.bounds().height() <= self.line_height(&raw.bounds()) * 2.2
                    {
                        let (id, bounds) = (first.id.clone(), first.bounds());
                        let applied = self.doc.replace(first);
                        self.on_applied(applied);
                        self.note_ink(&id, bounds, time);
                    }
                }
                let snapped = held.or_else(|| {
                    if self.pen_mode == PenMode::Shape {
                        recognize_shape(&raw)
                    } else if self.auto_convert {
                        auto = true;
                        self.auto_shape(&raw)
                    } else {
                        None
                    }
                });
                if snapped.is_some() {
                    // The rough ink on screen is replaced by the clean shape.
                    self.damage_world(&self.draft_bounds.clone(), self.width / 2.0);
                    if auto {
                        self.last_auto = Some((raw.clone(), time));
                    }
                } else if self.auto_text {
                    self.note_ink(&raw.id, raw.bounds(), time);
                }
                let applied = self.doc.add(snapped.unwrap_or(raw));
                self.on_applied(applied);
            }
            Mode::Shape => {
                if self.draft_end.dist(self.start_world) * self.zoom > 6.0 {
                    let id = self.next_id();
                    let shape = Shape::new(id.clone(), self.tool, vec![self.start_world, self.draft_end])
                        .styled(self.color, self.width, self.filled);
                    let applied = self.doc.add(shape);
                    self.on_applied(applied);
                    // Like tldraw, drawing a shape returns to Select with it selected.
                    self.tool = Tool::Select;
                    self.doc.select(Some(&id));
                    self.events |= EV_STATE;
                }
            }
            Mode::Text => {
                if self.travel < 16.0 {
                    self.text_request = Some(TextRequest { id: String::new(), x: w.x, y: w.y, text: String::new() });
                    self.events |= EV_TEXT;
                }
            }
            Mode::Erase => {
                let ids: Vec<String> = self.hidden.drain().collect();
                let applied = self.doc.remove_many(ids);
                self.on_applied(applied);
            }
            Mode::Move => {
                if self.dragging {
                    let delta = self.delta;
                    let moved = self
                        .moving
                        .iter()
                        .map(|s| s.with_points(s.points.iter().map(|p| *p + delta).collect()))
                        .collect();
                    self.clear_transform();
                    let applied = self.doc.replace_many(moved);
                    self.on_applied(applied);
                } else {
                    let hit = self.doc.hit_test(w, 12.0 / self.zoom);
                    if let Some(h) = &hit {
                        if h.tool == Tool::Text
                            && self.last_tap_id.as_deref() == Some(h.id.as_str())
                            && time.saturating_sub(self.last_tap_time) < 450
                        {
                            self.text_request = Some(TextRequest {
                                id: h.id.clone(),
                                x: h.points[0].x,
                                y: h.points[0].y,
                                text: h.text.clone(),
                            });
                            self.events |= EV_TEXT;
                        }
                    }
                    self.last_tap_id = hit.map(|h| h.id.clone());
                    self.last_tap_time = time;
                    self.clear_transform();
                }
            }
            Mode::Resize => {
                if self.dragging {
                    let (o, sx, sy) = (self.resize_origin, self.scale_x, self.scale_y);
                    let resized = self.moving.iter().map(|s| scaled(s, o, sx, sy)).collect();
                    self.clear_transform();
                    let applied = self.doc.replace_many(resized);
                    self.on_applied(applied);
                } else {
                    self.clear_transform();
                }
            }
            Mode::Marquee => {
                if let Some(area) = self.marquee.take() {
                    if area.longest_side() * self.zoom > 6.0 {
                        let ids: Vec<String> = self.doc.shapes_in(&area).iter().map(|s| s.id.clone()).collect();
                        self.doc.select_many(ids);
                    }
                }
                self.events |= EV_STATE;
            }
            Mode::Pan | Mode::None | Mode::Pinch | Mode::Blocked => {}
        }
    }

    fn unhide_all(&mut self) {
        let ids: Vec<String> = self.hidden.drain().collect();
        for id in ids {
            if let Some(s) = self.doc.get(&id) {
                self.scene_damage.push(render::paint_bounds(s));
            }
        }
    }

    fn clear_transform(&mut self) {
        self.moving.clear();
        self.dragging = false;
        self.delta = Pt::ZERO;
        self.scale_x = 1.0;
        self.scale_y = 1.0;
        self.unhide_all();
    }

    /// Abandons the in-progress action without changing the document.
    fn cancel_action(&mut self) {
        self.snap_deadline = None;
        if self.draft.take().is_some() || self.snapped.is_some() {
            self.damage_world(&self.draft_bounds.clone(), self.width / 2.0);
        }
        self.snapped = None;
        self.marquee = None;
        self.clear_transform();
        if self.mode != Mode::Pinch && self.mode != Mode::Blocked {
            self.mode = Mode::None;
        }
        self.overlay_dirty = true;
    }

    fn erase_at(&mut self, w: Pt) {
        let tolerance = 12.0 / self.zoom;
        for shape in self.doc.shapes() {
            if self.hidden.contains(&shape.id) || !shape.hits(w, tolerance) {
                continue;
            }
            self.hidden.insert(shape.id.clone());
            self.scene_damage.push(render::paint_bounds(shape));
            self.overlay_dirty = true;
        }
    }

    fn reset_still(&mut self, position: Pt, time: u64) {
        self.still_anchor = position;
        self.snap_deadline = if self.snap_shapes { Some(time + 500) } else { None };
    }

    /// Holding the pen still at the end of a stroke replaces it with the clean
    /// shape it resembles (line, arrow, rectangle or ellipse).
    fn try_snap(&mut self) {
        let Some(draft) = &self.draft else { return };
        if self.mode != Mode::Draw || draft.len() < 6 {
            return;
        }
        let raw = Shape::new(self.draft_id.clone(), Tool::Pen, draft.clone()).styled(self.color, self.width, false);
        if let Some(shape) = recognize_shape(&raw) {
            self.damage_world(&self.draft_bounds.clone(), self.width / 2.0);
            self.snapped = Some(shape);
            self.overlay_dirty = true;
            self.events |= EV_HAPTIC;
        }
    }

    // ------------------------------------------------------------ rendering

    fn view_changed(&mut self) {
        self.overlay_dirty = true;
        self.events |= EV_VIEW;
    }

    /// Marks a world rectangle (grown by `pad` world units) of the frame stale.
    fn damage_world(&mut self, r: &Rect, pad: f64) {
        let px = IRect::covering(&self.view().rect_px(&r.inflate(pad)), 2.0);
        self.damage = self.damage.union(&px);
    }

    fn on_applied(&mut self, applied: Applied) {
        if !applied.changed {
            return;
        }
        self.events |= EV_DOC | EV_STATE;
        self.overlay_dirty = true;
        let view = self.view();
        let direct = !applied.appended.is_empty()
            && self.scene_view == Some(view)
            && self.scene_damage.is_empty()
            && self.hidden.is_empty();
        if direct {
            // New shapes sit on top of everything: paint them straight onto
            // the cached scene instead of repainting what is underneath.
            let (w, h) = (self.scene.w, self.scene.h);
            let full = self.scene.bounds();
            if let Some(mut t) = Target::new(&mut self.scene.data, full, view) {
                for s in &applied.appended {
                    render::draw_shape(&mut t, s, 1.0, None);
                }
            }
            let _ = (w, h);
            for s in &applied.appended {
                self.damage_world(&render::paint_bounds(s), 0.0);
            }
        } else {
            for (bounds, width) in &applied.areas {
                // Generous padding covers arrow heads and text overhang.
                self.scene_damage.push(bounds.inflate(width * 4.5 + 16.0));
            }
        }
    }

    fn paint_scene(scene: &mut Layer, doc: &Document, hidden: &HashSet<String>, view: View, grid: bool, r: IRect) {
        let r = r.intersect(&scene.bounds());
        if r.is_empty() {
            return;
        }
        let whole = r == scene.bounds();
        let mut scratch = if whole { Vec::new() } else { vec![0u8; r.width() as usize * r.height() as usize * 4] };
        {
            let buf: &mut [u8] = if whole { &mut scene.data } else { &mut scratch };
            let Some(mut t) = Target::new(buf, r, view) else { return };
            render::draw_background(&mut t, grid);
            for s in doc.shapes() {
                if hidden.contains(&s.id) {
                    continue;
                }
                let px = IRect::covering(&view.rect_px(&render::paint_bounds(s)), 2.0);
                if px.intersects(&r) {
                    render::draw_shape(&mut t, s, 1.0, None);
                }
            }
        }
        if !whole {
            scene.write(r, &scratch);
        }
    }

    fn transform_extra(&self) -> Option<Transform> {
        if !self.dragging {
            return None;
        }
        match self.mode {
            Mode::Move => Some(Transform::from_translate(self.delta.x as f32, self.delta.y as f32)),
            Mode::Resize => {
                let o = self.resize_origin;
                Some(
                    Transform::from_translate(-o.x as f32, -o.y as f32)
                        .post_scale(self.scale_x as f32, self.scale_y as f32)
                        .post_translate(o.x as f32, o.y as f32),
                )
            }
            _ => None,
        }
    }

    fn map_rect(&self, r: &Rect) -> Rect {
        if !self.dragging {
            return *r;
        }
        match self.mode {
            Mode::Move => r.shift(self.delta),
            Mode::Resize => {
                let o = self.resize_origin;
                let m = |p: Pt| Pt::new(o.x + (p.x - o.x) * self.scale_x, o.y + (p.y - o.y) * self.scale_y);
                Rect::from_points(m(r.top_left()), m(r.bottom_right()))
            }
            _ => *r,
        }
    }

    fn draft_shape(&self) -> Option<Shape> {
        if self.mode != Mode::Shape {
            return None;
        }
        Some(
            Shape::new("draft", self.tool, vec![self.start_world, self.draft_end])
                .styled(self.color, self.width, self.filled),
        )
    }

    fn selection_for_overlay(&self) -> Vec<Arc<Shape>> {
        if self.tool != Tool::Select {
            return Vec::new();
        }
        if self.dragging && matches!(self.mode, Mode::Move | Mode::Resize) {
            self.moving.clone()
        } else {
            self.doc.selected_shapes()
        }
    }

    /// Screen pixels covered by live feedback other than the pen stroke.
    fn overlay_bounds(&self) -> IRect {
        let view = self.view();
        let mut out = EMPTY;
        let mut add = |r: Rect, pad_px: f64| out = out.union(&IRect::covering(&view.rect_px(&r), pad_px));
        if self.mode == Mode::Erase {
            for s in self.doc.shapes().iter().filter(|s| self.hidden.contains(&s.id)) {
                add(render::paint_bounds(s), 2.0);
            }
        }
        if self.dragging {
            for s in &self.moving {
                // Resizing also scales stroke widths and arrow heads.
                let grow = self.scale_x.abs().max(self.scale_y.abs()).max(1.0);
                let pad = (render::paint_bounds(s).width() - s.bounds().width()) / 2.0 * grow;
                add(self.map_rect(&s.bounds()).inflate(pad), 3.0);
            }
        }
        if let Some(s) = &self.snapped {
            add(render::paint_bounds(s), 2.0);
        }
        if let Some(s) = self.draft_shape() {
            add(render::paint_bounds(&s), 2.0);
        }
        let selected = self.selection_for_overlay();
        if !selected.is_empty() {
            let mut all: Option<Rect> = None;
            for s in &selected {
                let r = self.map_rect(&s.bounds());
                all = Some(all.map_or(r, |a| a.union(&r)));
            }
            add(all.unwrap().inflate(12.0 / self.zoom), 3.0);
        }
        if let Some(m) = &self.marquee {
            add(*m, 3.0);
        }
        out
    }

    fn paint_live(&self, t: &mut Target) {
        if self.mode == Mode::Erase {
            for s in self.doc.shapes().iter().filter(|s| self.hidden.contains(&s.id)) {
                render::draw_shape(t, s, 0.22, None);
            }
        }
        if let Some(extra) = self.transform_extra() {
            for s in &self.moving {
                render::draw_shape(t, s, 1.0, Some(extra));
            }
        }
        if let Some(s) = &self.snapped {
            render::draw_shape(t, s, 1.0, None);
        } else if let Some(draft) = &self.draft {
            render::draw_draft(t, draft, self.color, self.width);
        }
        if let Some(s) = self.draft_shape() {
            render::draw_shape(t, &s, 1.0, None);
        }
        let selected = self.selection_for_overlay();
        if !selected.is_empty() {
            let z = self.zoom;
            let mut all: Option<Rect> = None;
            for s in &selected {
                let r = self.map_rect(&s.bounds());
                all = Some(all.map_or(r, |a| a.union(&r)));
                if selected.len() > 1 {
                    render::draw_frame(t, &r.inflate(2.0 / z), 1.5, None, 0.0);
                }
            }
            let frame = all.unwrap().inflate(6.0 / z);
            render::draw_frame(t, &frame, 1.5, None, 0.0);
            let half = 5.0 / z;
            for c in corners(&frame) {
                let handle = Rect::ltrb(c.x - half, c.y - half, c.x + half, c.y + half);
                render::draw_frame(t, &handle, 1.5, Some(0xffffffff), 1.5 / z);
            }
        }
        if let Some(m) = &self.marquee {
            render::draw_frame(t, m, 1.0, Some(0x1a2f80ed), 0.0);
        }
    }

    fn has_live(&self) -> bool {
        self.draft.is_some()
            || self.snapped.is_some()
            || self.marquee.is_some()
            || self.dragging
            || self.mode == Mode::Shape
            || (self.mode == Mode::Erase && !self.hidden.is_empty())
            || (self.tool == Tool::Select && !self.doc.selected_ids().is_empty())
    }

    /// Brings `frame` up to date. Returns the pixel rectangle that changed,
    /// or `None` when there is nothing new to show.
    pub fn render(&mut self) -> Option<IRect> {
        let full = self.scene.bounds();
        if full.is_empty() {
            return None;
        }
        let view = self.view();
        // 1. Bring the cached scene to the current camera.
        match self.scene_view {
            Some(old) if old == view => {}
            Some(old) if old.zoom == view.zoom && old.dpr == view.dpr => {
                let (dx, dy) = (
                    (view.offset_px().0 - old.offset_px().0) as i64,
                    (view.offset_px().1 - old.offset_px().1) as i64,
                );
                if dx.abs() < full.x1 as i64 && dy.abs() < full.y1 as i64 {
                    // Panning: slide the picture and paint only what scrolled in.
                    let (dx, dy) = (dx as i32, dy as i32);
                    if dx != 0 || dy != 0 {
                        self.scene.scroll(dx, dy);
                        let (w, h) = (full.x1, full.y1);
                        let mut strips = Vec::new();
                        if dx > 0 {
                            strips.push(IRect::new(0, 0, dx, h));
                        } else if dx < 0 {
                            strips.push(IRect::new(w + dx, 0, w, h));
                        }
                        if dy > 0 {
                            strips.push(IRect::new(0, 0, w, dy));
                        } else if dy < 0 {
                            strips.push(IRect::new(0, h + dy, w, h));
                        }
                        for strip in strips {
                            Self::paint_scene(&mut self.scene, &self.doc, &self.hidden, view, self.grid, strip);
                        }
                        self.damage = full;
                    }
                } else {
                    Self::paint_scene(&mut self.scene, &self.doc, &self.hidden, view, self.grid, full);
                    self.full_repaints += 1;
                    self.scene_damage.clear();
                    self.damage = full;
                }
            }
            _ => {
                Self::paint_scene(&mut self.scene, &self.doc, &self.hidden, view, self.grid, full);
                self.full_repaints += 1;
                self.scene_damage.clear();
                self.damage = full;
            }
        }
        self.scene_view = Some(view);
        // 2. Repaint the parts of the scene whose shapes changed.
        if !self.scene_damage.is_empty() {
            let mut rects: Vec<IRect> = std::mem::take(&mut self.scene_damage)
                .iter()
                .map(|r| IRect::covering(&view.rect_px(r), 3.0).intersect(&full))
                .filter(|r| !r.is_empty())
                .collect();
            if rects.len() > 6 {
                let all = rects.iter().fold(EMPTY, |a, r| a.union(r));
                rects = vec![all];
            }
            for r in rects {
                Self::paint_scene(&mut self.scene, &self.doc, &self.hidden, view, self.grid, r);
                self.damage = self.damage.union(&r);
            }
        }
        // 3. Live feedback that moved: repaint where it was and where it is.
        if self.overlay_dirty {
            self.overlay_dirty = false;
            let now = self.overlay_bounds();
            self.damage = self.damage.union(&self.overlay_prev).union(&now);
            self.overlay_prev = now;
        }
        // 4. Compose the damaged rectangle of the frame.
        let d = std::mem::replace(&mut self.damage, EMPTY).intersect(&full);
        if d.is_empty() {
            return None;
        }
        if self.has_live() {
            let mut scratch = self.scene.read(d);
            if let Some(mut t) = Target::new(&mut scratch, d, view) {
                self.paint_live(&mut t);
            }
            self.frame.write(d, &scratch);
        } else {
            self.frame.copy_from(&self.scene, d);
        }
        Some(d)
    }
}
