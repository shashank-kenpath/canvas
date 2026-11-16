//! UI text and Material icons, rasterised into a premultiplied pixmap.
//!
//! Glyph coverage is cached. A string that needs a script this Roboto build
//! does not have is handed to the host rasteriser (`crate::text`) instead.

use crate::geom::IRect;
use ab_glyph::{point, Font, FontRef, Glyph, GlyphId, PxScale};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tiny_skia::PixmapMut;

/// A typeface embedded in the crate.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Face {
    /// Roboto Regular.
    Regular,
    /// Roboto Medium.
    Medium,
    /// Roboto Bold.
    Bold,
    /// Material Icons.
    Icons,
}

const CACHE_LIMIT: usize = 4000;
const ELLIPSIS: char = '\u{2026}';
/// Host bitmaps are centred on the line, this far above the baseline.
const FALLBACK_CENTER: f32 = 0.35;

/// Coverage for one glyph at one size and horizontal subpixel.
struct Mask {
    width: u32,
    height: u32,
    /// Top-left of the mask relative to the snapped pen origin and baseline.
    dx: i32,
    dy: i32,
    coverage: Vec<u8>,
}

impl Mask {
    fn empty() -> Mask {
        Mask {
            width: 0,
            height: 0,
            dx: 0,
            dy: 0,
            coverage: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MaskKey {
    face: Face,
    glyph: u16,
    /// Quantised `size_px` as `f32` bits.
    size_bits: u32,
    /// Horizontal subpixel bucket, `0..4`.
    sub_x: u8,
}

fn faces() -> &'static [FontRef<'static>; 4] {
    static FACES: OnceLock<[FontRef<'static>; 4]> = OnceLock::new();
    FACES.get_or_init(|| {
        [
            load(include_bytes!("../../fonts/Roboto-Regular.ttf")),
            load(include_bytes!("../../fonts/Roboto-Medium.ttf")),
            load(include_bytes!("../../fonts/Roboto-Bold.ttf")),
            load(include_bytes!("../../fonts/MaterialIcons-Regular.otf")),
        ]
    })
}

fn load(bytes: &'static [u8]) -> FontRef<'static> {
    FontRef::try_from_slice(bytes).expect("embedded font")
}

fn face_font(face: Face) -> &'static FontRef<'static> {
    &faces()[match face {
        Face::Regular => 0,
        Face::Medium => 1,
        Face::Bold => 2,
        Face::Icons => 3,
    }]
}

fn glyph_cache() -> &'static Mutex<HashMap<MaskKey, Mask>> {
    static CACHE: OnceLock<Mutex<HashMap<MaskKey, Mask>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn usable_size(size_px: f32) -> bool {
    size_px.is_finite() && size_px > 0.0
}

/// Snap size to 1/64 px and return that size plus its bit pattern.
fn size_key(size_px: f32) -> (f32, u32) {
    let q = (size_px * 64.0).round() / 64.0;
    (q, q.to_bits())
}

fn px_per_unit(font: &FontRef, size_px: f32) -> Option<f32> {
    let em = font.units_per_em()?;
    if em <= 0.0 || !usable_size(size_px) {
        None
    } else {
        Some(size_px / em)
    }
}

fn vertical(font: &FontRef, size_px: f32) -> (f32, f32) {
    let Some(scale) = px_per_unit(font, size_px) else {
        return (0.0, 0.0);
    };
    let ascent = (font.ascent_unscaled() * scale).max(0.0);
    // `descent_unscaled` is negative (below the baseline).
    let descent = (-font.descent_unscaled() * scale).max(0.0);
    (ascent, descent)
}

/// Newlines and tabs take the same advance as a space.
fn shape_char(c: char) -> char {
    match c {
        '\n' | '\r' | '\t' => ' ',
        other => other,
    }
}

fn missing_glyph(font: &FontRef, text: &str) -> bool {
    text.chars()
        .any(|c| !c.is_whitespace() && font.glyph_id(c).0 == 0)
}

fn layout_width(font: &FontRef, size_px: f32, text: &str) -> f32 {
    let Some(scale) = px_per_unit(font, size_px) else {
        return 0.0;
    };
    let mut pen = 0.0;
    let mut prev: Option<GlyphId> = None;
    for c in text.chars() {
        let id = font.glyph_id(shape_char(c));
        if let Some(prev) = prev {
            pen += font.kern_unscaled(prev, id) * scale;
        }
        pen += font.h_advance_unscaled(id) * scale;
        prev = Some(id);
    }
    pen
}

/// Advance `draw` will return for `text`.
///
/// `host` is true when `crate::text::rasterize` can paint. Missing glyphs then
/// take the host's measure; otherwise they are drawn with this face.
fn painted_advance(font: &FontRef, size_px: f32, text: &str, host: bool) -> f32 {
    if host && missing_glyph(font, text) {
        crate::text::measure(text, size_px as f64, 0).0 as f32
    } else {
        layout_width(font, size_px, text)
    }
}

fn host_will_paint(size_px: f32) -> bool {
    crate::text::rasterize("A", size_px as f64, 0xFF00_0000, 0).is_some()
}

/// Width, ascent above the baseline, and descent below it, for one line.
pub fn measure(face: Face, size_px: f32, text: &str) -> (f32, f32, f32) {
    if !usable_size(size_px) {
        return (0.0, 0.0, 0.0);
    }
    let font = face_font(face);
    let (ascent, descent) = vertical(font, size_px);
    let width = if text.is_empty() {
        0.0
    } else if missing_glyph(font, text) {
        crate::text::measure(text, size_px as f64, 0).0 as f32
    } else {
        layout_width(font, size_px, text)
    };
    (width, ascent, descent)
}

/// Draws one line. `x` is the left edge and `baseline` is the baseline.
///
/// `color` is straight-alpha ARGB (`0xAARRGGBB`). Only pixels inside `clip`
/// (and the pixmap) are touched. Returns the advance width.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    pixmap: &mut PixmapMut<'_>,
    face: Face,
    size_px: f32,
    text: &str,
    x: f32,
    baseline: f32,
    color: u32,
    clip: IRect,
) -> f32 {
    if !usable_size(size_px) || text.is_empty() {
        return 0.0;
    }
    let font = face_font(face);
    if missing_glyph(font, text) {
        let width = crate::text::measure(text, size_px as f64, 0).0 as f32;
        if let Some(bmp) = crate::text::rasterize(text, size_px as f64, color, 0) {
            let clip = visible_clip(pixmap, clip);
            if x.is_finite() && baseline.is_finite() && !clip.is_empty() {
                blit_bitmap(pixmap, &bmp, x, baseline, size_px, clip);
            }
            return width;
        }
    }
    paint_face(pixmap, face, font, size_px, text, x, baseline, color, clip)
}

/// Draws one line, cutting it with `…` so the advance fits in `max_width`.
#[allow(clippy::too_many_arguments)]
pub fn draw_ellipsized(
    pixmap: &mut PixmapMut<'_>,
    face: Face,
    size_px: f32,
    text: &str,
    x: f32,
    baseline: f32,
    max_width: f32,
    color: u32,
    clip: IRect,
) -> f32 {
    if !usable_size(size_px) || !max_width.is_finite() || max_width <= 0.0 || text.is_empty() {
        return 0.0;
    }
    let font = face_font(face);
    let host = if missing_glyph(font, text) || font.glyph_id(ELLIPSIS).0 == 0 {
        host_will_paint(size_px)
    } else {
        false
    };
    let width_of = |sample: &str| painted_advance(font, size_px, sample, host);
    match trim_to_width(text, max_width, width_of) {
        None => draw(pixmap, face, size_px, text, x, baseline, color, clip),
        Some(fitted) if fitted.is_empty() => 0.0,
        Some(fitted) => draw(pixmap, face, size_px, &fitted, x, baseline, color, clip),
    }
}

/// The Material icon for `code` as a one-character string.
pub fn icon(code: u32) -> String {
    char::from_u32(code)
        .map(|c| c.to_string())
        .unwrap_or_default()
}

/// Draws the Material icon `code` centred on `(cx, cy)`. `size_px` is the em size.
pub fn draw_icon(
    pixmap: &mut PixmapMut<'_>,
    code: u32,
    size_px: f32,
    cx: f32,
    cy: f32,
    color: u32,
    clip: IRect,
) {
    if !usable_size(size_px) || !cx.is_finite() || !cy.is_finite() {
        return;
    }
    let Some(ch) = char::from_u32(code) else {
        return;
    };
    let font = face_font(Face::Icons);
    let id = font.glyph_id(ch);
    if id.0 == 0 {
        return;
    }
    let (qsize, _) = size_key(size_px);
    let glyph = Glyph {
        id,
        scale: PxScale::from(qsize),
        position: point(0.0, 0.0),
    };
    let Some(outlined) = font.outline_glyph(glyph) else {
        return;
    };
    let bounds = outlined.px_bounds();
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return;
    }
    // `px_bounds` is in the same space as the glyph position (the pen on the
    // baseline). Shift that box so its centre lands on (cx, cy).
    let pen_x = cx - (bounds.min.x + bounds.max.x) * 0.5;
    let baseline = cy - (bounds.min.y + bounds.max.y) * 0.5;
    let mut buf = [0; 4];
    let text = ch.encode_utf8(&mut buf);
    let _ = paint_face(
        pixmap,
        Face::Icons,
        font,
        size_px,
        text,
        pen_x,
        baseline,
        color,
        clip,
    );
}

/// `None` when `text` already fits. `Some("")` when even `…` does not fit.
fn trim_to_width(
    text: &str,
    max_width: f32,
    mut width_of: impl FnMut(&str) -> f32,
) -> Option<String> {
    if width_of(text) <= max_width {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut best: Option<usize> = None;
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let mut candidate: String = chars[..mid].iter().collect();
        candidate.push(ELLIPSIS);
        if width_of(&candidate) <= max_width {
            best = Some(mid);
            lo = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            hi = mid - 1;
        }
    }
    let n = best?;
    let mut out: String = chars[..n].iter().collect();
    out.push(ELLIPSIS);
    Some(out)
}

fn visible_clip(pixmap: &PixmapMut<'_>, clip: IRect) -> IRect {
    let bounds = IRect::new(0, 0, pixmap.width() as i32, pixmap.height() as i32);
    bounds.intersect(&clip)
}

#[allow(clippy::too_many_arguments)]
fn paint_face(
    pixmap: &mut PixmapMut<'_>,
    face: Face,
    font: &FontRef<'static>,
    size_px: f32,
    text: &str,
    x: f32,
    baseline: f32,
    color: u32,
    clip: IRect,
) -> f32 {
    let advance = layout_width(font, size_px, text);
    let clip = visible_clip(pixmap, clip);
    if clip.is_empty() || color >> 24 == 0 || !x.is_finite() || !baseline.is_finite() {
        return advance;
    }
    let cr = (color >> 16) as u8;
    let cg = (color >> 8) as u8;
    let cb = color as u8;
    let ca = (color >> 24) as u8;
    let base_y = snap_y(baseline);
    let (qsize, size_bits) = size_key(size_px);
    let mut cache = glyph_cache().lock().unwrap_or_else(|err| err.into_inner());
    let Some(scale) = px_per_unit(font, size_px) else {
        return 0.0;
    };
    let mut pen = 0.0;
    let mut prev: Option<GlyphId> = None;
    for c in text.chars() {
        let id = font.glyph_id(shape_char(c));
        if let Some(prev) = prev {
            pen += font.kern_unscaled(prev, id) * scale;
        }
        let (origin_x, sub_x) = snap_x(x + pen);
        let key = MaskKey {
            face,
            glyph: id.0,
            size_bits,
            sub_x,
        };
        if !cache.contains_key(&key) {
            if cache.len() >= CACHE_LIMIT {
                cache.clear();
            }
            cache.insert(key, raster_mask(font, id, qsize, sub_x));
        }
        if let Some(mask) = cache.get(&key) {
            blit_mask(
                pixmap,
                mask,
                origin_x + mask.dx,
                base_y + mask.dy,
                clip,
                cr,
                cg,
                cb,
                ca,
            );
        }
        pen += font.h_advance_unscaled(id) * scale;
        prev = Some(id);
    }
    advance
}

fn raster_mask(font: &FontRef, id: GlyphId, size_px: f32, sub_x: u8) -> Mask {
    let glyph = Glyph {
        id,
        scale: PxScale::from(size_px),
        position: point(sub_x as f32 * 0.25, 0.0),
    };
    let Some(outlined) = font.outline_glyph(glyph) else {
        return Mask::empty();
    };
    let bounds = outlined.px_bounds();
    let width = bounds.width() as u32;
    let height = bounds.height() as u32;
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return Mask::empty();
    }
    let mut coverage = vec![0u8; (width * height) as usize];
    outlined.draw(|x, y, cover| {
        if x >= width || y >= height {
            return;
        }
        let v = (cover.clamp(0.0, 1.0) * 255.0).round();
        coverage[(y * width + x) as usize] = v as u8;
    });
    Mask {
        width,
        height,
        dx: bounds.min.x.round() as i32,
        dy: bounds.min.y.round() as i32,
        coverage,
    }
}

/// Quarter-pixel horizontal position. The bucket is in `0..4`.
fn snap_x(x: f32) -> (i32, u8) {
    if !x.is_finite() {
        return (0, 0);
    }
    let quarters = (x * 4.0).round().clamp(-8_000_000.0, 8_000_000.0) as i32;
    (quarters.div_euclid(4), quarters.rem_euclid(4) as u8)
}

fn snap_y(y: f32) -> i32 {
    if !y.is_finite() {
        0
    } else {
        y.round().clamp(-8_000_000.0, 8_000_000.0) as i32
    }
}

#[allow(clippy::too_many_arguments)]
fn blit_mask(
    pixmap: &mut PixmapMut<'_>,
    mask: &Mask,
    left: i32,
    top: i32,
    clip: IRect,
    cr: u8,
    cg: u8,
    cb: u8,
    ca: u8,
) {
    if mask.width == 0 || mask.height == 0 {
        return;
    }
    let x0 = left.max(clip.x0);
    let y0 = top.max(clip.y0);
    let x1 = (left.saturating_add(mask.width as i32)).min(clip.x1);
    let y1 = (top.saturating_add(mask.height as i32)).min(clip.y1);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let stride = pixmap.width() as usize;
    let data = pixmap.data_mut();
    for y in y0..y1 {
        let row_off = (y - top) as u32 * mask.width;
        let dst_row = y as usize * stride;
        for x in x0..x1 {
            let idx = (row_off + (x - left) as u32) as usize;
            let Some(&cov) = mask.coverage.get(idx) else {
                continue;
            };
            if cov == 0 {
                continue;
            }
            let dst = (dst_row + x as usize) * 4;
            blend_coverage(&mut data[dst..dst + 4], cov, cr, cg, cb, ca);
        }
    }
}

fn blit_bitmap(
    pixmap: &mut PixmapMut<'_>,
    bmp: &crate::text::TextBitmap,
    x: f32,
    baseline: f32,
    size_px: f32,
    clip: IRect,
) {
    let width = bmp.width;
    let height = bmp.height;
    let stride_src = width as usize * 4;
    if width == 0 || height == 0 || bmp.rgba.len() < stride_src * height as usize {
        return;
    }
    let left = snap_y(x);
    let center = baseline - FALLBACK_CENTER * size_px;
    let top = snap_y(center - height as f32 * 0.5);
    let x0 = left.max(clip.x0);
    let y0 = top.max(clip.y0);
    let x1 = left.saturating_add(width as i32).min(clip.x1);
    let y1 = top.saturating_add(height as i32).min(clip.y1);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let stride = pixmap.width() as usize;
    let data = pixmap.data_mut();
    for y in y0..y1 {
        let src_row = (y - top) as usize * stride_src;
        let dst_row = y as usize * stride;
        for x in x0..x1 {
            let src = src_row + (x - left) as usize * 4;
            let Some(pixel) = bmp.rgba.get(src..src + 4) else {
                continue;
            };
            if pixel[3] == 0 {
                continue;
            }
            let dst = (dst_row + x as usize) * 4;
            blend_premul(&mut data[dst..dst + 4], pixel);
        }
    }
}

/// `a * b / 255`, rounded.
#[inline]
fn mul255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}

/// Coverage times straight `color`, premultiplied, source-over.
#[inline]
fn blend_coverage(dst: &mut [u8], cov: u8, cr: u8, cg: u8, cb: u8, ca: u8) {
    let sa = mul255(cov as u32, ca as u32);
    if sa == 0 {
        return;
    }
    let inv = 255 - sa;
    dst[0] = (mul255(cr as u32, sa) + mul255(dst[0] as u32, inv)) as u8;
    dst[1] = (mul255(cg as u32, sa) + mul255(dst[1] as u32, inv)) as u8;
    dst[2] = (mul255(cb as u32, sa) + mul255(dst[2] as u32, inv)) as u8;
    dst[3] = (sa + mul255(dst[3] as u32, inv)) as u8;
}

/// Source-over of an already premultiplied pixel.
#[inline]
fn blend_premul(dst: &mut [u8], src: &[u8]) {
    let sa = src[3] as u32;
    if sa == 0 {
        return;
    }
    if sa == 255 {
        dst[..4].copy_from_slice(&src[..4]);
        return;
    }
    let inv = 255 - sa;
    dst[0] = (src[0] as u32 + mul255(dst[0] as u32, inv)) as u8;
    dst[1] = (src[1] as u32 + mul255(dst[1] as u32, inv)) as u8;
    dst[2] = (src[2] as u32 + mul255(dst[2] as u32, inv)) as u8;
    dst[3] = (sa + mul255(dst[3] as u32, inv)) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Pixmap;

    fn canvas(w: u32, h: u32) -> Pixmap {
        let mut pm = Pixmap::new(w, h).expect("pixmap");
        pm.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
        pm
    }

    fn full(w: i32, h: i32) -> IRect {
        IRect::new(0, 0, w, h)
    }

    #[test]
    fn measure_canvas_studio() {
        for face in [Face::Regular, Face::Medium, Face::Bold] {
            for size in [12.0_f32, 16.0, 32.0] {
                let (w, ascent, descent) = measure(face, size, "Canvas Studio");
                assert!(
                    w > 5.0 * size && w < 9.0 * size,
                    "{face:?} at {size}px measured {w}"
                );
                assert!(
                    ascent > 0.5 * size && ascent < 1.4 * size,
                    "ascent {ascent}"
                );
                assert!(descent > 0.0 && descent < 0.6 * size, "descent {descent}");
            }
        }
    }

    #[test]
    fn draw_respects_clip_and_matches_measure() {
        let mut pm = canvas(200, 40);
        let before = pm.data().to_vec();
        let clip = IRect::new(40, 4, 160, 36);
        let size = 18.0;
        let (measured, _, _) = measure(Face::Regular, size, "Canvas Studio");
        let adv = draw(
            &mut pm.as_mut(),
            Face::Regular,
            size,
            "Canvas Studio",
            2.0,
            28.0,
            0xFF00_0000,
            clip,
        );
        assert!(
            (adv - measured).abs() < 0.01,
            "draw {adv} measure {measured}"
        );
        let mut changed = 0;
        let data = pm.data();
        for y in 0..40 {
            for x in 0..200 {
                let i = (y * 200 + x) * 4;
                let same = before[i..i + 4] == data[i..i + 4];
                let inside = (40..160).contains(&x) && (4..36).contains(&y);
                if inside {
                    if !same {
                        changed += 1;
                    }
                } else {
                    assert!(same, "pixel {x},{y} outside the clip changed");
                }
            }
        }
        assert!(changed > 10, "black text did not change the white pixmap");
    }

    #[test]
    fn icon_paints_near_centre() {
        assert_eq!(icon(0xe3dc), "\u{e3dc}");
        assert_eq!(icon(0xf05ed), "\u{f05ed}");
        let mut pm = Pixmap::new(64, 64).expect("pixmap");
        draw_icon(
            &mut pm.as_mut(),
            0xe3dc,
            32.0,
            32.0,
            32.0,
            0xFF00_0000,
            full(64, 64),
        );
        let mut near = 0;
        let mut edge = 0;
        for y in 0..64i32 {
            for x in 0..64i32 {
                let a = pm.data()[((y as u32 * 64 + x as u32) * 4 + 3) as usize];
                if a == 0 {
                    continue;
                }
                if (x - 32).abs() <= 14 && (y - 32).abs() <= 14 {
                    near += 1;
                }
                if x < 4 || y < 4 || x >= 60 || y >= 60 {
                    edge += 1;
                }
            }
        }
        assert!(near > 0, "no coverage within 14px of the centre");
        assert_eq!(edge, 0, "icon reached the pixmap edge");
    }

    #[test]
    fn ellipsize_stays_within_width() {
        let mut pm = canvas(300, 48);
        let text = "Canvas Studio is a whiteboard for long document titles";
        let max_w = 90.0;
        let size = 16.0;
        assert!(measure(Face::Regular, size, text).0 > max_w);
        let adv = draw_ellipsized(
            &mut pm.as_mut(),
            Face::Medium,
            size,
            text,
            4.0,
            32.0,
            max_w,
            0xFF22_2222,
            full(300, 48),
        );
        assert!(adv > 0.0, "ellipsis should still paint");
        assert!(adv <= max_w, "advance {adv} exceeded {max_w}");
    }

    #[test]
    fn empty_and_degenerate_do_not_panic() {
        let mut pm = canvas(16, 16);
        let before = pm.data().to_vec();
        let clip = full(16, 16);
        assert_eq!(measure(Face::Regular, 16.0, "").0, 0.0);
        assert_eq!(measure(Face::Regular, 0.0, "Hi"), (0.0, 0.0, 0.0));
        assert_eq!(
            draw(
                &mut pm.as_mut(),
                Face::Regular,
                16.0,
                "",
                1.0,
                12.0,
                0xFF00_0000,
                clip
            ),
            0.0
        );
        assert_eq!(
            draw_ellipsized(
                &mut pm.as_mut(),
                Face::Regular,
                16.0,
                "",
                1.0,
                12.0,
                40.0,
                0xFF00_0000,
                clip
            ),
            0.0
        );
        assert_eq!(
            draw(
                &mut pm.as_mut(),
                Face::Regular,
                0.0,
                "Hi",
                1.0,
                12.0,
                0xFF00_0000,
                clip
            ),
            0.0
        );
        let adv = draw(
            &mut pm.as_mut(),
            Face::Regular,
            14.0,
            "Hi",
            0.0,
            12.0,
            0xFF00_0000,
            IRect::new(100, 100, 120, 110),
        );
        assert!(adv > 0.0);
        draw(
            &mut pm.as_mut(),
            Face::Regular,
            14.0,
            "Hi",
            0.0,
            12.0,
            0xFF00_0000,
            IRect::new(8, 8, 2, 2),
        );
        draw_icon(&mut pm.as_mut(), 0xe3dc, 0.0, 8.0, 8.0, 0xFF00_0000, clip);
        assert_eq!(pm.data(), before.as_slice());
    }

    #[test]
    fn semi_transparent_red_is_premultiplied() {
        let mut pm = Pixmap::new(80, 48).expect("pixmap");
        let adv = draw(
            &mut pm.as_mut(),
            Face::Bold,
            32.0,
            "H",
            4.0,
            36.0,
            0x80FF_0000,
            full(80, 48),
        );
        assert!(adv > 0.0);
        let mut saw = false;
        for px in pm.data().as_chunks::<4>().0 {
            if px[3] == 0 {
                continue;
            }
            saw = true;
            assert_eq!(px[1], 0, "green leaked: {px:?}");
            assert_eq!(px[2], 0, "blue leaked: {px:?}");
            assert!(
                (px[0] as i32 - px[3] as i32).abs() <= 2,
                "not premultiplied: {px:?}"
            );
            assert!(px[3] <= 140, "alpha ignored colour alpha: {px:?}");
        }
        assert!(saw, "glyph painted nothing");
    }

    #[test]
    fn newline_has_the_same_advance_as_space() {
        let size = 16.0;
        let spaced = measure(Face::Regular, size, "A B").0;
        let broken = measure(Face::Regular, size, "A\nB").0;
        assert!(
            (spaced - broken).abs() < 0.01,
            "space {spaced} newline {broken}"
        );
    }

    #[test]
    fn cached_redraw_matches() {
        let mut first = Pixmap::new(120, 40).expect("pixmap");
        let mut second = Pixmap::new(120, 40).expect("pixmap");
        let clip = full(120, 40);
        let color = 0xFF11_2233;
        draw(
            &mut first.as_mut(),
            Face::Regular,
            18.0,
            "Cache",
            3.5,
            28.0,
            color,
            clip,
        );
        draw(
            &mut second.as_mut(),
            Face::Regular,
            18.0,
            "Cache",
            3.5,
            28.0,
            color,
            clip,
        );
        assert_eq!(first.data(), second.data());
    }
}
