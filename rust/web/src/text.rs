//! Board text in the browser: the same typefaces and shaping (cosmic-text)
//! as the desktop app, with the font files fetched by the page instead of
//! built in, so the WebAssembly stays small and fonts are cached separately.

use canvas_core::text::{TextBitmap, TextHost, FONTS, LINE_HEIGHT};
use cosmic_text::fontdb::Database;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use std::sync::Mutex;

struct State {
    fonts: FontSystem,
    cache: SwashCache,
    /// Family name for each entry of [`FONTS`], once loaded.
    families: Vec<Option<&'static str>>,
}

pub struct WebText(Mutex<State>);

impl WebText {
    pub fn new() -> WebText {
        let fonts = FontSystem::new_with_locale_and_db("en-US".into(), Database::new());
        WebText(Mutex::new(State { fonts, cache: SwashCache::new(), families: vec![None; FONTS.len()] }))
    }

    /// Adds the font file for the [`FONTS`] entry `id`.
    pub fn add_font(&self, id: &str, bytes: Vec<u8>) {
        let mut s = self.0.lock().unwrap();
        let before = s.fonts.db().len();
        s.fonts.db_mut().load_font_data(bytes);
        // Leaked once per font: family names live as long as the page.
        let family = s.fonts.db().faces().nth(before).and_then(|f| f.families.first()).map(|(name, _)| &*Box::leak(name.clone().into_boxed_str()));
        if let Some(i) = FONTS.iter().position(|f| *f == id) {
            s.families[i] = family;
        }
        // Generic families fall back to the default face.
        if id == "inter" {
            if let Some(name) = family {
                s.fonts.db_mut().set_sans_serif_family(name);
            }
        } else if id == "merriweather" {
            if let Some(name) = family {
                s.fonts.db_mut().set_serif_family(name);
            }
        } else if id == "jetbrains" {
            if let Some(name) = family {
                s.fonts.db_mut().set_monospace_family(name);
            }
        }
    }
}

impl State {
    fn layout(&mut self, text: &str, size: f64, font: u8) -> Buffer {
        let size = size.max(1.0) as f32;
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, size * LINE_HEIGHT as f32));
        let family = match FONTS.get(font as usize).copied().unwrap_or("sans") {
            "serif" => Family::Serif,
            "mono" => Family::Monospace,
            _ => match self.families.get(font as usize).copied().flatten() {
                Some(name) => Family::Name(name),
                None => Family::SansSerif,
            },
        };
        buffer.set_size(None, None);
        buffer.set_text(text, &Attrs::new().family(family), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        buffer
    }
}

fn line_count(text: &str) -> usize {
    text.split('\n').count().max(1)
}

impl TextHost for WebText {
    fn measure(&self, text: &str, font_size: f64, font: u8) -> (f64, f64) {
        let mut s = self.0.lock().unwrap();
        let buffer = s.layout(text, font_size, font);
        let widest = buffer.layout_runs().map(|r| r.line_w).fold(0.0_f32, f32::max);
        (widest as f64, line_count(text) as f64 * font_size * LINE_HEIGHT)
    }

    fn rasterize(&self, text: &str, font_px: f64, color: u32, font: u8) -> Option<TextBitmap> {
        let mut guard = self.0.lock().unwrap();
        let s = &mut *guard;
        let mut buffer = s.layout(text, font_px, font);
        let widest = buffer.layout_runs().map(|r| r.line_w).fold(0.0_f32, f32::max);
        // Slanted and script faces overhang their advance width a little.
        let w = ((widest.ceil() + (font_px * 0.25).ceil() as f32) as u32).clamp(1, 4096);
        let h = ((line_count(text) as f64 * font_px * LINE_HEIGHT).ceil() as u32).clamp(1, 4096);
        let mut rgba = vec![0u8; w as usize * h as usize * 4];
        let (a, r, g, b) = ((color >> 24) as u8, (color >> 16) as u8, (color >> 8) as u8, color as u8);
        let ink = Color::rgba(r, g, b, a);
        let State { fonts, cache, .. } = s;
        buffer.draw(fonts, cache, ink, |x, y, gw, gh, c| {
            let alpha = c.a() as u32;
            if alpha == 0 {
                return;
            }
            for yy in y.max(0)..(y + gh as i32).min(h as i32) {
                for xx in x.max(0)..(x + gw as i32).min(w as i32) {
                    let i = (yy as usize * w as usize + xx as usize) * 4;
                    // Source-over in premultiplied RGBA.
                    let inv = 255 - alpha;
                    let src = [c.r() as u32 * alpha / 255, c.g() as u32 * alpha / 255, c.b() as u32 * alpha / 255, alpha];
                    for k in 0..4 {
                        rgba[i + k] = (src[k] + rgba[i + k] as u32 * inv / 255) as u8;
                    }
                }
            }
        });
        Some(TextBitmap { width: w, height: h, rgba })
    }
}
