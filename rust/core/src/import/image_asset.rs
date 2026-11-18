use base64::{engine::general_purpose::STANDARD, Engine};
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageFormat, ImageReader, Limits};
use resvg::usvg::{ImageHrefResolver, Options, Tree};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::sync::{Arc, OnceLock};
use tiny_skia::{IntSize, Pixmap, Transform};

const MAX_DECODE_PIXELS: u64 = 24_000_000;
const MAX_SIDE: u32 = 2400;
pub(crate) const MAX_ASSET_BYTES: usize = 4 << 20;

/// A bounded PNG or JPEG. Cloning a shape keeps the same compressed asset.
#[derive(Debug)]
pub(crate) struct ImageAsset {
    bytes: Vec<u8>,
    hash: [u8; 32],
}

impl PartialEq for ImageAsset {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl ImageAsset {
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() > MAX_ASSET_BYTES
            || !matches!(image::guess_format(&bytes), Ok(ImageFormat::Jpeg | ImageFormat::Png))
        {
            return Err("Invalid board image data.".into());
        }
        Ok(Self { hash: Sha256::digest(&bytes).into(), bytes })
    }

    pub(crate) fn from_base64(text: &str) -> Result<Self, String> {
        if text.len() > MAX_ASSET_BYTES.div_ceil(3) * 4 {
            return Err("Board image data is too large.".into());
        }
        let bytes = STANDARD.decode(text).map_err(|_| "Invalid board image data.")?;
        Self::from_bytes(bytes)
    }

    pub(crate) fn base64(&self) -> String {
        STANDARD.encode(&self.bytes)
    }

    pub(crate) fn hash(&self) -> [u8; 32] {
        self.hash
    }

    pub(crate) fn hash_name(&self) -> String {
        self.hash.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn pixmap(&self) -> Option<Pixmap> {
        let (image, _) = decode(&self.bytes, MAX_SIDE, u64::from(MAX_SIDE).pow(2)).ok()?;
        let rgba = image.into_rgba8();
        let size = IntSize::from_wh(rgba.width(), rgba.height())?;
        let mut pixels = rgba.into_raw();
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = u16::from(pixel[3]);
            for color in &mut pixel[..3] {
                *color = ((u16::from(*color) * alpha + 127) / 255) as u8;
            }
        }
        Pixmap::from_vec(pixels, size)
    }
}

fn decode(bytes: &[u8], max_side: u32, max_pixels: u64) -> Result<(DynamicImage, bool), String> {
    let format = image::guess_format(bytes).map_err(|_| "Unrecognized image format.")?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif | ImageFormat::Bmp) {
        return Err("Unsupported image format.".into());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(max_pixels * 8);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().map_err(|e| format!("Could not read image: {e}"))?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > max_pixels {
        return Err("Image dimensions are too large.".into());
    }
    limits
        .reserve(decoder.total_bytes())
        .map_err(|_| "Image is too large to decode.")?;
    decoder.set_limits(limits).map_err(|e| format!("Could not read image: {e}"))?;
    let orientation = decoder.orientation().map_err(|e| format!("Could not read image: {e}"))?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|e| format!("Could not read image: {e}"))?;
    image.apply_orientation(orientation);
    Ok((image, orientation == image::metadata::Orientation::NoTransforms))
}

pub(super) fn raster(bytes: &[u8]) -> Result<(ImageAsset, u32, u32), String> {
    let (image, upright) = decode(bytes, 8192, MAX_DECODE_PIXELS)?;
    let (width, height) = image.dimensions();
    if upright && width.max(height) <= MAX_SIDE && bytes.len() <= MAX_ASSET_BYTES {
        if let Ok(asset) = ImageAsset::from_bytes(bytes.to_vec()) {
            return Ok((asset, width, height));
        }
    }
    let image = if width.max(height) > MAX_SIDE {
        image.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let (width, height) = image.dimensions();
    let rgba = image.into_rgba8();
    let asset = if rgba.pixels().any(|pixel| pixel.0[3] < 255) {
        encode_png(DynamicImage::ImageRgba8(rgba))?
    } else {
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 88)
            .encode_image(&DynamicImage::ImageRgba8(rgba).into_rgb8())
            .map_err(|e| format!("Could not store image: {e}"))?;
        ImageAsset::from_bytes(encoded)?
    };
    Ok((asset, width, height))
}

fn encode_png(mut image: DynamicImage) -> Result<ImageAsset, String> {
    loop {
        let mut output = Cursor::new(Vec::new());
        image.write_to(&mut output, ImageFormat::Png)
            .map_err(|e| format!("Could not store image: {e}"))?;
        let bytes = output.into_inner();
        if bytes.len() <= MAX_ASSET_BYTES {
            return ImageAsset::from_bytes(bytes);
        }
        let side = image.width().max(image.height()) * 3 / 4;
        image = image.resize(side, side, image::imageops::FilterType::Triangle);
    }
}

pub(super) fn svg(bytes: &[u8]) -> Result<(ImageAsset, u32, u32), String> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut fonts = resvg::usvg::fontdb::Database::new();
        fonts.load_font_data(include_bytes!("../../fonts/Roboto-Regular.ttf").to_vec());
        fonts.set_sans_serif_family("Roboto");
        fonts.set_serif_family("Roboto");
        fonts.set_monospace_family("Roboto");
        Arc::new(fonts)
    });
    let resolve = ImageHrefResolver::default_data_resolver();
    let options = Options {
        font_family: "Roboto".into(),
        fontdb: fonts.clone(),
        image_href_resolver: ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            resolve_data: Box::new(move |mime, data, options| {
                // Bound embedded bitmaps too. Ignore nested SVGs and external resources.
                let (image, _) = decode(&data, MAX_SIDE, u64::from(MAX_SIDE).pow(2)).ok()?;
                drop(image);
                resolve(mime, data, options)
            }),
        },
        ..Options::default()
    };
    // Do not accept an SVGZ stream masquerading as a small SVG.
    let source = std::str::from_utf8(bytes).map_err(|_| "SVG must be UTF-8.")?;
    let tree = Tree::from_str(source, &options).map_err(|e| format!("Could not read SVG: {e}"))?;
    let (width, height) = (tree.size().width(), tree.size().height());
    let scale = (MAX_SIDE as f32 / width.max(height)).min(2.0);
    let w = (width * scale).ceil().max(1.0) as u32;
    let h = (height * scale).ceil().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h).ok_or("Could not allocate SVG image.")?;
    resvg::render(&tree, Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let bytes = pixmap.encode_png().map_err(|e| format!("Could not store SVG: {e}"))?;
    let asset = if bytes.len() > MAX_ASSET_BYTES {
        let (decoded, _) = decode(&bytes, MAX_SIDE, u64::from(MAX_SIDE).pow(2))?;
        encode_png(decoded)?
    } else {
        ImageAsset::from_bytes(bytes)?
    };
    Ok((asset, width.ceil() as u32, height.ceil() as u32))
}
