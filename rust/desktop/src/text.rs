//! Board text for desktop: the same bundled typefaces as the Android app,
//! laid out with complex-script shaping (Devanagari, Arabic, emoji through
//! system fallback fonts) by cosmic-text.

use canvas_core::text::{TextBitmap, TextHost, FONTS, LINE_HEIGHT};
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use std::sync::Mutex;

macro_rules! bundled {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../../app-android/app/src/main/assets/fonts/", $name, ".ttf")) as &[u8])),*]
    };
}

/// Typefaces shipped with the app, by their id in [`FONTS`].
const BUNDLED: &[(&str, &[u8])] = bundled!(
    "inter", "opensans", "lato", "montserrat", "poppins", "nunito", "oswald", "raleway", "ubuntu", "merriweather",
    "playfair", "lora", "robotoslab", "jetbrains", "sourcecode", "caveat", "patrick", "kalam", "indie", "architects",
    "shadows", "comicneue", "marker", "amatic", "pacifico", "greatvibes", "lobster", "bebas", "notodeva", "hind", "tiro",
);

struct State {
    fonts: FontSystem,
    cache: SwashCache,
    /// Family name for each entry of [`FONTS`], when bundled.
    families: Vec<Option<&'static str>>,
}

pub struct DesktopText(Mutex<State>);

impl DesktopText {
    pub fn new() -> DesktopText {
        let mut fonts = FontSystem::new();
        let mut families = vec![None; FONTS.len()];
        for (id, bytes) in BUNDLED {
            let before = fonts.db().len();
            fonts.db_mut().load_font_data(bytes.to_vec());
            // Leaked once at start-up: family names live as long as the app.
            let family = fonts.db().faces().nth(before).and_then(|f| f.families.first()).map(|(name, _)| &*Box::leak(name.clone().into_boxed_str()));
            if let Some(i) = FONTS.iter().position(|f| f == id) {
                families[i] = family;
            }
        }
        DesktopText(Mutex::new(State { fonts, cache: SwashCache::new(), families }))
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

impl TextHost for DesktopText {
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
