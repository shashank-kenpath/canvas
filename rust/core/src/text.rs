//! Text measuring and rasterising is supplied by the host (Android draws text
//! with the system fonts, so Devanagari and emoji work). Without a host, text
//! is measured with a rough estimate and not drawn; that is what unit tests use.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

/// Premultiplied RGBA pixels of one rendered text block.
pub struct TextBitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub trait TextHost: Send + Sync {
    /// Size of `text` at `font_size`, in the same units as `font_size`.
    /// `font` is an index into [`FONTS`].
    fn measure(&self, text: &str, font_size: f64, font: u8) -> (f64, f64);
    /// Renders `text` at `font_px` pixels in the ARGB `color`.
    fn rasterize(&self, text: &str, font_px: f64, color: u32, font: u8) -> Option<TextBitmap>;
}

/// Font choices for board text. The id is what is stored in board files;
/// the host maps each one to a system or bundled typeface. New fonts are only
/// ever appended so stored indexes keep their meaning.
pub const FONTS: [&str; 36] = [
    "sans", "serif", "mono", "hand", "script", "inter", "opensans", "lato", "montserrat", "poppins", "nunito",
    "oswald", "raleway", "ubuntu", "merriweather", "playfair", "lora", "robotoslab", "jetbrains", "sourcecode",
    "caveat", "patrick", "kalam", "indie", "architects", "shadows", "comicneue", "marker", "amatic", "pacifico",
    "greatvibes", "lobster", "bebas", "notodeva", "hind", "tiro",
];

/// Names shown to people for each entry of [`FONTS`].
pub const FONT_NAMES: [&str; 36] = [
    "Sans", "Serif", "Mono", "Handwriting", "Script", "Inter", "Open Sans", "Lato", "Montserrat", "Poppins", "Nunito",
    "Oswald", "Raleway", "Ubuntu", "Merriweather", "Playfair Display", "Lora", "Roboto Slab", "JetBrains Mono",
    "Source Code Pro", "Caveat", "Patrick Hand", "Kalam", "Indie Flower", "Architects Daughter", "Shadows Into Light",
    "Comic Neue", "Permanent Marker", "Amatic SC", "Pacifico", "Great Vibes", "Lobster", "Bebas Neue",
    "Noto Sans Devanagari", "Hind", "Tiro Devanagari",
];

pub fn font_index(name: &str) -> u8 {
    FONTS.iter().position(|f| *f == name).unwrap_or(0) as u8
}

static HOST: RwLock<Option<Arc<dyn TextHost>>> = RwLock::new(None);

type MeasureCache = Mutex<HashMap<(String, u64, u8), (f64, f64)>>;

fn measure_cache() -> &'static MeasureCache {
    static CACHE: OnceLock<MeasureCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn set_host(host: Arc<dyn TextHost>) {
    *HOST.write().unwrap() = Some(host);
    measure_cache().lock().unwrap().clear();
}

fn host() -> Option<Arc<dyn TextHost>> {
    HOST.read().unwrap().clone()
}

/// Line height used for board text, as a multiple of the font size.
pub const LINE_HEIGHT: f64 = 1.3;

pub fn measure(text: &str, font_size: f64, font: u8) -> (f64, f64) {
    let Some(host) = host() else {
        let lines = text.split('\n').count().max(1) as f64;
        let widest = text.split('\n').map(|l| l.chars().count()).max().unwrap_or(0) as f64;
        return (widest * font_size * 0.55, lines * font_size * LINE_HEIGHT);
    };
    let key = (text.to_string(), font_size.to_bits(), font);
    if let Some(size) = measure_cache().lock().unwrap().get(&key) {
        return *size;
    }
    let size = host.measure(text, font_size, font);
    let mut cache = measure_cache().lock().unwrap();
    if cache.len() > 4096 {
        cache.clear();
    }
    cache.insert(key, size);
    size
}

pub fn rasterize(text: &str, font_px: f64, color: u32, font: u8) -> Option<TextBitmap> {
    host()?.rasterize(text, font_px, color, font)
}
