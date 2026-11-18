use super::{request, Resp, MAX_BODY};
use crate::shape::Shape;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

// The relay wraps the response in base64 and an envelope under MAX_BODY.
const PACKET_BYTES: usize = MAX_BODY * 3 / 4 - (64 << 10);
const SNAPSHOT_BYTES: usize = 96 << 20;

fn append_item(packet: &mut Vec<u8>, item: &[u8], first: bool) -> bool {
    if packet.len() + item.len() + 64 > PACKET_BYTES {
        return false;
    }
    if !first {
        packet.push(b',');
    }
    packet.extend_from_slice(item);
    true
}

pub(crate) fn snapshot(shapes: &[Arc<Shape>], revision: u64, offset: usize) -> Resp {
    let mut packet = format!("{{\"revision\":{revision},\"shapes\":[").into_bytes();
    let mut next = offset;
    for shape in shapes.iter().skip(offset) {
        let item = shape.to_json().to_string();
        if !append_item(&mut packet, item.as_bytes(), next == offset) {
            if next == offset {
                return Resp::empty(413);
            }
            break;
        }
        next += 1;
    }
    packet.push(b']');
    if next < shapes.len() {
        packet.extend_from_slice(format!(",\"nextOffset\":{next}").as_bytes());
    }
    packet.push(b'}');
    Resp { status: 200, ctype: "application/json", body: packet }
}

pub(super) fn fetch(
    host: &str,
    code: &str,
    since: u64,
    timeout: Duration,
) -> Result<Option<(u64, Vec<Value>)>, String> {
    let mut revision = since;
    let mut offset = 0;
    let mut bytes = 0;
    let mut shapes = Vec::new();
    loop {
        let path = format!("/canvas/board?code={code}&since={revision}&offset={offset}");
        let (status, body) = request("GET", host, &path, None, timeout)?;
        if status == 409 && offset > 0 {
            // An edit landed mid-fetch. The next poll will retry the new revision.
            return Ok(None);
        }
        if status != 200 {
            return Err("Could not read the shared board".into());
        }
        bytes += body.len();
        if bytes > SNAPSHOT_BYTES {
            return Err("Shared board is too large".into());
        }
        let mut page: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        let current = page["revision"].as_u64().ok_or("Shared board revision missing")?;
        if offset > 0 && current != revision {
            return Ok(None);
        }
        revision = current;
        let Value::Array(items) = page["shapes"].take() else {
            return if offset == 0 { Ok(None) } else { Err("Shared board shapes missing".into()) };
        };
        shapes.extend(items);
        let Some(next) = page["nextOffset"].as_u64() else {
            return Ok(Some((revision, shapes)));
        };
        if next <= offset as u64 || next != shapes.len() as u64 {
            return Err("Invalid shared board offset".into());
        }
        offset = shapes.len();
    }
}

pub(super) fn send_ops(host: &str, code: &str, ops: &mut Vec<Value>, timeout: Duration) -> Result<(), String> {
    while !ops.is_empty() {
        let mut packet = b"{\"operations\":[".to_vec();
        let mut count = 0;
        for op in ops.iter() {
            if !append_item(&mut packet, op.to_string().as_bytes(), count == 0) {
                break;
            }
            count += 1;
        }
        if count == 0 {
            return Err("A shared shape is too large".into());
        }
        packet.extend_from_slice(b"]}");
        let (status, _) = request("POST", host, &format!("/canvas/ops?code={code}"), Some(&packet), timeout)?;
        if status != 200 {
            return Err("Could not update the shared board".into());
        }
        ops.drain(..count);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Pt;
    use crate::import::ImageAsset;
    use crate::shape::Tool;

    #[test]
    fn snapshot_splits_large_images_and_preserves_order() {
        let mut png = tiny_skia::Pixmap::new(2, 2).unwrap().encode_png().unwrap();
        // Padding simulates large encoded assets without needing large decoded bitmaps.
        png.resize(3 << 20, 0);
        let image = Arc::new(ImageAsset::from_bytes(png).unwrap());
        let shapes: Vec<_> = ["first", "second"].into_iter().map(|id| Arc::new(
            Shape::new(id, Tool::Image, vec![Pt::ZERO, Pt::new(20.0, 20.0)])
                .with_image(image.clone(), false),
        )).collect();
        let read = |offset| {
            let response = snapshot(&shapes, 17, offset);
            assert_eq!(response.status, 200);
            assert!(response.body.len() < PACKET_BYTES);
            serde_json::from_slice::<Value>(&response.body).unwrap()
        };
        let first = read(0);
        let second = read(1);
        assert_eq!(first["nextOffset"], 1);
        assert!(second.get("nextOffset").is_none());
        for (value, expected) in [first, second].iter().zip(shapes) {
            assert_eq!(value["revision"], 17);
            let shape = Shape::from_json(&value["shapes"][0]).unwrap();
            assert_eq!(&shape, expected.as_ref());
        }
    }
}
