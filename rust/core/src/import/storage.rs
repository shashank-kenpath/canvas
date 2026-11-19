//! Board-local image sidecars. Shape JSON stays self-contained on the wire.

use super::{ImageAsset, MAX_ASSET_BYTES};
use serde_json::Value;
use crate::sys::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn dir(board_path: &Path) -> PathBuf {
    board_path.with_extension("assets")
}

fn map_images(board: &mut Value, all_pages: bool, mut map: impl FnMut(&mut Value) -> Result<(), String>) -> Result<(), String> {
    fn map_shapes(
        page: &mut Value,
        map: &mut impl FnMut(&mut Value) -> Result<(), String>,
    ) -> Result<(), String> {
        if let Some(shapes) = page.get_mut("shapes").and_then(Value::as_array_mut) {
            for shape in shapes.iter_mut().filter(|shape| shape["tool"] == "image") {
                map(shape)?;
            }
        }
        Ok(())
    }
    map_shapes(board, &mut map)?;
    if all_pages {
        if let Some(pages) = board.get_mut("pages").and_then(Value::as_array_mut) {
            for page in pages {
                map_shapes(page, &mut map)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn hydrate(board: &mut Value, board_path: &Path, all_pages: bool) -> Result<(), String> {
    let dir = dir(board_path);
    map_images(board, all_pages, |shape| {
        let Some(hash) = shape.get("imageRef").and_then(Value::as_str) else { return Ok(()) };
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Invalid board image reference.".into());
        }
        let path = dir.join(hash);
        let mut bytes = Vec::new();
        fs::File::open(&path).and_then(|file| file.take(MAX_ASSET_BYTES as u64 + 1).read_to_end(&mut bytes))
            .map_err(|e| format!("Could not read board image: {e}"))?;
        let asset = ImageAsset::from_bytes(bytes)?;
        if asset.hash_name() != hash {
            return Err("Board image does not match its reference.".into());
        }
        let shape = shape.as_object_mut().unwrap();
        shape.remove("imageRef");
        shape.insert("imageData".into(), Value::String(asset.base64()));
        Ok(())
    })
}

pub(crate) fn externalize(board: &mut Value, board_path: &Path) -> Result<(), String> {
    let dir = dir(board_path);
    map_images(board, true, |shape| {
        let Some(inline) = shape.get("imageData").and_then(Value::as_str) else { return Ok(()) };
        let asset = ImageAsset::from_base64(inline)?;
        let hash = stage(&dir, &asset)?;
        let shape = shape.as_object_mut().unwrap();
        shape.remove("imageData");
        shape.insert("imageRef".into(), Value::String(hash));
        Ok(())
    })
}

pub(crate) fn stage(dir: &Path, asset: &ImageAsset) -> Result<String, String> {
    let hash = asset.hash_name();
    write(dir, &hash, asset.bytes()).map_err(|e| format!("Could not save board image: {e}"))?;
    Ok(hash)
}

fn write(dir: &Path, hash: &str, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = dir.join(hash);
    if fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() == bytes.len() as u64) {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!("{}-{}.tmp", crate::sys::now_ms(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let mut file = fs::File::create_new(&temp)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| fs::rename(&temp, &path));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
