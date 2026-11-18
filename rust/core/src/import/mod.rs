//! Converts system-picker input into regular Inkboard shapes.

mod image_asset;
mod office;
mod ooxml;
pub(crate) mod storage;

pub(crate) use image_asset::{ImageAsset, MAX_ASSET_BYTES};

use crate::geom::{Pt, Rect};
use crate::shape::{Shape, Tool};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_FILE_BYTES: usize = 32 << 20;
const MAX_PAGES: usize = 80;
const MAX_IMPORT_IMAGES: usize = 48 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Pdf,
    Document,
}

impl FileKind {
    pub fn picker_id(self) -> i32 {
        match self {
            Self::Image => 0,
            Self::Pdf => 1,
            Self::Document => 2,
        }
    }
}

pub enum PickerFile {
    File { name: String, mime: String, bytes: Vec<u8> },
    /// In page order, rasterized with Android's PdfRenderer.
    Pdf(Vec<Vec<u8>>),
    Error(String),
    Cancelled,
}

pub(crate) enum Imported {
    Shapes(Vec<Shape>),
    Pages(Vec<Vec<Shape>>),
}

struct Importer<'a> {
    prefix: &'a str,
    count: usize,
    images: HashMap<String, (Arc<ImageAsset>, u32, u32)>,
    image_bytes: usize,
}

impl<'a> Importer<'a> {
    fn new(prefix: &'a str) -> Self {
        Self { prefix, count: 0, images: HashMap::new(), image_bytes: 0 }
    }

    fn shape(&mut self, tool: Tool, points: Vec<Pt>) -> Shape {
        self.count += 1;
        Shape::new(format!("{}-{}", self.prefix, self.count), tool, points)
    }

    fn image(&mut self, asset: Arc<ImageAsset>, bounds: Rect, background: bool) -> Shape {
        self.shape(Tool::Image, vec![bounds.top_left(), bounds.bottom_right()])
            .with_image(asset, background)
    }

    fn keep_asset(&mut self, asset: ImageAsset) -> Result<Arc<ImageAsset>, String> {
        self.image_bytes += asset.bytes().len();
        if self.image_bytes > MAX_IMPORT_IMAGES {
            return Err("The imported images exceed Inkboard's size limit.".into());
        }
        Ok(Arc::new(asset))
    }

    fn embedded_image(
        &mut self,
        package: &mut ooxml::Package<'_>,
        path: &str,
    ) -> Result<Option<(Arc<ImageAsset>, u32, u32)>, String> {
        if let Some(asset) = self.images.get(path) {
            return Ok(Some(asset.clone()));
        }
        let Some(bytes) = package.read(path)? else { return Ok(None) };
        let result = if path.to_ascii_lowercase().ends_with(".svg") {
            image_asset::svg(&bytes)
        } else {
            image_asset::raster(&bytes)
        };
        let Ok((asset, w, h)) = result else { return Ok(None) };
        let value = (self.keep_asset(asset)?, w, h);
        self.images.insert(path.into(), value.clone());
        Ok(Some(value))
    }

    fn text(&mut self, at: Pt, text: String, size: f64) -> Shape {
        self.shape(Tool::Text, vec![at]).with_text(text, size)
    }
}

pub(crate) fn parse(file: PickerFile, prefix: &str, current_page: bool) -> Result<Imported, String> {
    let mut importer = Importer::new(prefix);
    match file {
        PickerFile::Pdf(pages) => {
            if pages.is_empty() || pages.len() > MAX_PAGES {
                return Err(format!("Inkboard accepts PDFs with 1 to {MAX_PAGES} pages."));
            }
            let mut shapes = Vec::new();
            let mut y = 0.0;
            for png in pages {
                let (asset, w, h) = image_asset::raster(&png)?;
                let asset = importer.keep_asset(asset)?;
                let size = fit(w, h, 1120.0, 1120.0, false);
                let origin = Pt::new(0.0, y);
                shapes.push(importer.image(asset, Rect::from_points(origin, origin + size), !current_page));
                if current_page {
                    y += size.y + 32.0;
                }
            }
            if current_page {
                Ok(Imported::Shapes(shapes))
            } else {
                Ok(Imported::Pages(shapes.into_iter().map(|s| vec![s]).collect()))
            }
        }
        PickerFile::File { name, mime, bytes } => {
            if bytes.len() > MAX_FILE_BYTES {
                return Err("Files must be 32 MiB or smaller.".into());
            }
            let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase());
            match (extension.as_deref(), mime.as_str()) {
                (Some("pptx"), _) | (_, "application/vnd.openxmlformats-officedocument.presentationml.presentation") => {
                    office::presentation(&mut importer, &bytes).map(Imported::Pages)
                }
                (Some("docx"), _) | (_, "application/vnd.openxmlformats-officedocument.wordprocessingml.document") => {
                    office::document(&mut importer, &bytes).map(Imported::Shapes)
                }
                (Some("xlsx"), _) | (_, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet") => {
                    office::spreadsheet(&mut importer, &bytes).map(Imported::Shapes)
                }
                _ => {
                    let (asset, w, h) = if extension.as_deref() == Some("svg") || mime == "image/svg+xml" {
                        image_asset::svg(&bytes)?
                    } else {
                        image_asset::raster(&bytes)?
                    };
                    let size = fit(w, h, 960.0, 680.0, false);
                    let shape = importer.image(Arc::new(asset), Rect::from_points(Pt::ZERO, size), false);
                    Ok(Imported::Shapes(vec![shape]))
                }
            }
        }
        PickerFile::Error(error) => Err(error),
        PickerFile::Cancelled => Err("File import cancelled.".into()),
    }
}

fn fit(w: u32, h: u32, max_w: f64, max_h: f64, shrink_only: bool) -> Pt {
    let (w, h) = (f64::from(w.max(1)), f64::from(h.max(1)));
    let scale = (max_w / w).min(max_h / h);
    let scale = if shrink_only { scale.min(1.0) } else { scale };
    Pt::new(w * scale, h * scale)
}

#[cfg(test)]
mod tests;
