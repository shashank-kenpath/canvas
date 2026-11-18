//! Board files: one JSON document per board, the same format the Flutter app
//! writes (`version`, `id`, `title`, `updated`, `shapes`, optional `share` and
//! `synced`). Keys this app does not understand are kept and written back.

use crate::document::decode_shapes;
use crate::shape::Shape;
use serde_json::{json, Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct BoardFile {
    pub id: String,
    pub title: String,
    pub updated: String,
    pub shapes: Vec<Shape>,
    /// Everything else in the file (for example the sharing link).
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoardMeta {
    pub id: String,
    pub title: String,
    pub updated: String,
    pub shapes: usize,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 120 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn board_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("Invalid board id.".into());
    }
    Ok(dir.join(format!("{id}.json")))
}

pub fn encode(id: &str, title: &str, updated: &str, shapes: &[Arc<Shape>], extra: &Map<String, Value>) -> String {
    let mut m = Map::new();
    m.insert("version".into(), json!(1));
    m.insert("id".into(), json!(id));
    m.insert("title".into(), json!(title));
    m.insert("updated".into(), json!(updated));
    m.insert("shapes".into(), Value::Array(shapes.iter().map(|s| s.to_json()).collect()));
    for (k, v) in extra {
        if !m.contains_key(k) {
            m.insert(k.clone(), v.clone());
        }
    }
    Value::Object(m).to_string()
}

pub fn parse(source: &str) -> Result<BoardFile, String> {
    let v: Value = serde_json::from_str(source).map_err(|e| format!("Not a board file: {e}"))?;
    let shapes = decode_shapes(&v)?;
    let Value::Object(mut m) = v else {
        return Err("Not a board file.".into());
    };
    let mut text = |key: &str| match m.remove(key) {
        Some(Value::String(s)) => Ok(s),
        _ => Err(format!("Board has no {key}.")),
    };
    let (id, title, updated) = (text("id")?, text("title")?, text("updated")?);
    m.remove("version");
    m.remove("shapes");
    Ok(BoardFile { id, title, updated, shapes, extra: m })
}

pub fn load(dir: &Path, id: &str) -> Result<BoardFile, String> {
    let source = fs::read_to_string(board_path(dir, id)?).map_err(|e| e.to_string())?;
    parse(&source)
}

/// Writes the whole file under a temporary name, flushes it to disk and then
/// renames it into place, so a crash or power cut never leaves half a board.
pub fn save_atomic(dir: &Path, id: &str, contents: &str) -> Result<(), String> {
    let target = board_path(dir, id)?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let temp = dir.join(format!("{id}.tmp"));
    let write = || -> std::io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &target)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Could not save the board: {e}")
    })
}

pub fn delete(dir: &Path, id: &str) -> Result<(), String> {
    match fs::remove_file(board_path(dir, id)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Changes only the title and timestamp, leaving every other byte of meaning
/// in the file (shapes, sharing state) as it was.
pub fn rename(dir: &Path, id: &str, title: &str, updated: &str) -> Result<(), String> {
    let source = fs::read_to_string(board_path(dir, id)?).map_err(|e| e.to_string())?;
    let mut v: Value = serde_json::from_str(&source).map_err(|e| e.to_string())?;
    let m = v.as_object_mut().ok_or("Not a board file.")?;
    m.insert("title".into(), json!(title));
    m.insert("updated".into(), json!(updated));
    save_atomic(dir, id, &v.to_string())
}

/// Boards in `dir`, most recently changed first. Unreadable files are skipped.
pub fn list(dir: &Path) -> Vec<BoardMeta> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&source) else { continue };
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        if let (Some(id), Some(title), Some(updated)) = (text("id"), text("title"), text("updated")) {
            let shapes = v.get("shapes").and_then(Value::as_array).map_or(0, Vec::len);
            out.push(BoardMeta { id, title, updated, shapes });
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out
}

pub fn list_json(dir: &Path) -> String {
    Value::Array(
        list(dir)
            .into_iter()
            .map(|b| json!({"id": b.id, "title": b.title, "updated": b.updated, "shapes": b.shapes}))
            .collect(),
    )
    .to_string()
}

/// Copies a board file from another app (or another device) into `dir`.
/// Sharing state is dropped, since it belongs to the original copy. A board
/// already here with the same id and timestamp is skipped; one with the same
/// id but other content gets `fresh_id()` so nothing is overwritten.
/// Returns the board's title, or `None` when it was already here.
pub fn import(dir: &Path, source: &str, fresh_id: impl Fn() -> String) -> Result<Option<String>, String> {
    let board = parse(source)?;
    let mut extra = board.extra;
    for key in ["share", "synced", "lanShare"] {
        extra.remove(key);
    }
    let mut id = board.id;
    if valid_id(&id) {
        if let Ok(existing) = load(dir, &id) {
            if existing.updated == board.updated && existing.shapes.len() == board.shapes.len() {
                return Ok(None);
            }
            id = fresh_id();
        }
    } else {
        id = fresh_id();
    }
    let title = if board.title.trim().is_empty() { "Imported board".to_string() } else { board.title };
    let shapes: Vec<Arc<Shape>> = board.shapes.into_iter().map(Arc::new).collect();
    save_atomic(dir, &id, &encode(&id, &title, &board.updated, &shapes, &extra))?;
    Ok(Some(title))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("inkboard-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    const BOARD: &str = r#"{"version":1,"id":"1794833531605072","title":"convv","updated":"2026-11-17T17:41:31.249460","shapes":[],"share":{"id":"x"},"synced":"2026"}"#;

    #[test]
    fn import_keeps_id_and_drops_sharing() {
        let dir = temp_dir("keep");
        assert_eq!(import(&dir, BOARD, || "fresh".into()).unwrap().as_deref(), Some("convv"));
        let board = load(&dir, "1794833531605072").unwrap();
        assert_eq!(board.title, "convv");
        assert_eq!(board.updated, "2026-11-17T17:41:31.249460");
        assert!(board.extra.get("share").is_none() && board.extra.get("synced").is_none());
        // The same file again is skipped rather than duplicated.
        assert_eq!(import(&dir, BOARD, || "fresh".into()).unwrap(), None);
        assert_eq!(list(&dir).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_never_overwrites_a_different_board() {
        let dir = temp_dir("clash");
        import(&dir, BOARD, || "fresh".into()).unwrap();
        let newer = BOARD.replace("2026-11-17T17:41:31", "2026-11-18T09:00:00");
        import(&dir, &newer, || "fresh".into()).unwrap();
        assert_eq!(list(&dir).len(), 2);
        assert_eq!(load(&dir, "fresh").unwrap().updated, "2026-11-18T09:00:00.249460");
        let bad = BOARD.replace("1794833531605072", "../evil");
        import(&dir, &bad, || "other".into()).unwrap();
        assert!(load(&dir, "other").is_ok());
        assert!(import(&dir, "{\"nope\":1}", || "x".into()).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
