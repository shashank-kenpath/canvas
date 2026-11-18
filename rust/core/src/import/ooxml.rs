use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use zip::result::ZipError;
use zip::ZipArchive;

const MAX_PART_BYTES: usize = 16 << 20;
const MAX_EXPANDED_BYTES: usize = 64 << 20;
const MAX_ELEMENTS: usize = 300_000;

pub(super) struct Package<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    bytes_read: usize,
}

impl<'a> Package<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, String> {
        let zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("Could not open Office file: {e}"))?;
        if zip.len() > 12_000 {
            return Err("Office file has too many parts.".into());
        }
        Ok(Self { zip, bytes_read: 0 })
    }

    pub fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, String> {
        let part = match self.zip.by_name(path) {
            Ok(part) => part,
            Err(ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(format!("Could not open Office part {path}: {e}")),
        };
        let limit = MAX_PART_BYTES.min(MAX_EXPANDED_BYTES - self.bytes_read);
        if part.size() > limit as u64 {
            return Err("Office file is too large to import.".into());
        }
        let mut bytes = Vec::new();
        part.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() > limit {
            return Err("Office file is too large to import.".into());
        }
        self.bytes_read += bytes.len();
        Ok(Some(bytes))
    }

    pub fn xml(&mut self, path: &str) -> Result<Option<Node>, String> {
        self.read(path)?.map(|bytes| parse_xml(&bytes)).transpose()
    }

    pub fn required(&mut self, path: &str) -> Result<Node, String> {
        self.xml(path)?.ok_or_else(|| format!("Office file is missing {path}."))
    }

    pub fn main_part(&mut self, fallback: &str) -> Result<String, String> {
        Ok(self.rels("")?.into_values()
            .find(|rel| rel.kind == "officeDocument")
            .map_or_else(|| fallback.to_string(), |rel| rel.path))
    }

    pub fn rels(&mut self, part: &str) -> Result<HashMap<String, Relationship>, String> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let rels_path = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        let Some(root) = self.xml(&rels_path)? else { return Ok(HashMap::new()) };
        Ok(root.children("Relationship").filter_map(|node| {
            if node.attr("TargetMode") == Some("External") {
                return None;
            }
            let id = node.attr("Id")?.to_owned();
            let kind = node.attr("Type")?.rsplit('/').next()?.to_owned();
            let path = resolve(dir, node.attr("Target")?)?;
            Some((id, Relationship { kind, path }))
        }).collect())
    }
}

pub(super) struct Relationship {
    pub kind: String,
    pub path: String,
}

fn resolve(dir: &str, target: &str) -> Option<String> {
    // Relationship targets are URI paths within the package.
    if target.contains(['\\', ':', '#', '?']) {
        return None;
    }
    let mut path: Vec<&str> = if dir.is_empty() || target.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for piece in target.split('/') {
        match piece {
            "" | "." => {}
            ".." => { path.pop()?; }
            _ => path.push(piece),
        }
    }
    (!path.is_empty()).then(|| path.join("/"))
}

#[derive(Default)]
pub(super) struct Node {
    pub name: String,
    pub text: String,
    attrs: Vec<(String, String)>,
    pub nodes: Vec<Node>,
}

impl Node {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(name, _)| name.rsplit(':').next() == Some(key))
            .map(|(_, value)| value.as_str())
    }

    pub fn relationship_id(&self) -> Option<&str> {
        self.attrs.iter().find(|(key, _)| key.ends_with(":id")).map(|(_, value)| value.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Self> {
        self.nodes.iter().find(|node| node.name == name)
    }

    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Self> + 'a {
        self.nodes.iter().filter(move |node| node.name == name)
    }

    pub fn find(&self, name: &str) -> Option<&Self> {
        if self.name == name {
            Some(self)
        } else {
            self.nodes.iter().find_map(|node| node.find(name))
        }
    }

    pub fn collect_text(&self, out: &mut String) {
        match self.name.as_str() {
            "t" => out.push_str(&self.text),
            "tab" => out.push('\t'),
            "br" | "cr" => out.push('\n'),
            "rPh" => {}
            _ => self.nodes.iter().for_each(|node| node.collect_text(out)),
        }
    }
}

fn parse_element(element: &BytesStart<'_>, reader: &Reader<&[u8]>) -> Result<Node, String> {
    let name = std::str::from_utf8(element.local_name().as_ref()).map_err(|e| e.to_string())?.to_owned();
    let mut node = Node { name, ..Node::default() };
    for attr in element.attributes() {
        let attr = attr.map_err(|e| e.to_string())?;
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| e.to_string())?;
        let value = attr.decode_and_unescape_value(reader.decoder()).map_err(|e| e.to_string())?;
        node.attrs.push((key.into(), value.into_owned()));
    }
    Ok(node)
}

fn parse_xml(bytes: &[u8]) -> Result<Node, String> {
    let mut reader = Reader::from_reader(bytes);
    let mut stack = vec![Node::default()];
    let mut count = 0;
    loop {
        let event = reader.read_event().map_err(|e| format!("Invalid Office XML: {e}"))?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(tag) | Event::Empty(tag) => {
                count += 1;
                if count > MAX_ELEMENTS || stack.len() > 64 {
                    return Err("Office XML is too complex to import.".into());
                }
                let node = parse_element(&tag, &reader)?;
                if empty {
                    stack.last_mut().unwrap().nodes.push(node);
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err("Invalid Office XML element.".into());
                }
                let node = stack.pop().unwrap();
                stack.last_mut().unwrap().nodes.push(node);
            }
            Event::Text(value) => {
                stack.last_mut().unwrap().text.push_str(&value.xml_content().map_err(|e| e.to_string())?);
            }
            Event::CData(value) => {
                stack.last_mut().unwrap().text.push_str(&value.decode().map_err(|e| e.to_string())?);
            }
            Event::GeneralRef(value) => {
                let out = &mut stack.last_mut().unwrap().text;
                if let Some(ch) = value.resolve_char_ref().map_err(|e| e.to_string())? {
                    out.push(ch);
                } else {
                    let reference = value.decode().map_err(|e| e.to_string())?;
                    out.push_str(quick_xml::escape::resolve_predefined_entity(&reference).ok_or("Invalid Office XML entity.")?);
                }
            }
            Event::DocType(_) => return Err("Office XML cannot contain a document type.".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 || stack[0].nodes.len() != 1 {
        return Err("Invalid Office XML document.".into());
    }
    Ok(stack.remove(0).nodes.remove(0))
}
