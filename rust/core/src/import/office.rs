use super::ooxml::{Node, Package, Relationship};
use super::{fit, Importer};
use crate::geom::{Pt, Rect};
use crate::shape::{Shape, Tool};
use crate::table::{Cell, Table, MAX_SIZE};
use std::collections::HashMap;

const MAX_GRID_CELLS: usize = 48_000;
const CELL_WIDTH: f64 = 132.0;
const CELL_HEIGHT: f64 = 36.0;

pub(super) fn presentation(importer: &mut Importer<'_>, bytes: &[u8]) -> Result<Vec<Vec<Shape>>, String> {
    let mut package = Package::new(bytes)?;
    let main = package.main_part("ppt/presentation.xml")?;
    let presentation = package.required(&main)?;
    let rels = package.rels(&main)?;
    let slides = presentation.child("sldIdLst").ok_or("Presentation has no slides.")?;
    let slides: Vec<_> = slides.children("sldId").collect();
    if slides.is_empty() || slides.len() > super::MAX_PAGES {
        return Err(format!("Inkboard accepts presentations with 1 to {} slides.", super::MAX_PAGES));
    }
    let scale = presentation.child("sldSz").and_then(|size| number(size, "cx"))
        .filter(|w| *w > 0.0).map_or(1000.0 / 9_144_000.0, |w| 1000.0 / w);

    slides.into_iter().map(|id| {
        let slide = id.relationship_id().and_then(|id| rels.get(id))
            .filter(|rel| rel.kind == "slide").ok_or("Presentation is missing a slide.")?;
        let part = package.required(&slide.path)?;
        let rels = package.rels(&slide.path)?;
        let (layout, master) = slide_layout(&mut package, &rels)?;
        let mut shapes = Vec::new();
        let mut fallback = Vec::new();
        if let Some(tree) = part.find("spTree") {
            for node in &tree.nodes {
                let properties = node.child("spPr");
                let bounds = properties.and_then(|props| props.child("xfrm"))
                    .or_else(|| inherited_transform(node, layout.as_ref(), master.as_ref()))
                    .and_then(|transform| slide_bounds(transform, scale));
                match node.name.as_str() {
                    "pic" => {
                        if let (Some(bounds), Some(path)) = (bounds, image_path(node, &rels)) {
                            if let Some((asset, _, _)) = importer.embedded_image(&mut package, path)? {
                                shapes.push(importer.image(asset, bounds, false));
                            }
                        }
                    }
                    "sp" => {
                        if let Some(bounds) = bounds {
                            let preset = properties.and_then(|p| p.child("prstGeom")).and_then(|g| g.attr("prst"));
                            let tool = match preset {
                                Some("ellipse") => Some(Tool::Ellipse),
                                Some("rect" | "roundRect") if !text_box(node, properties) => Some(Tool::Rectangle),
                                _ => None,
                            };
                            if let (Some(tool), Some(properties)) = (tool, properties) {
                                let fill = properties.child("solidFill");
                                let stroke = properties.child("ln");
                                let color = fill.or_else(|| stroke.and_then(|s| s.child("solidFill")))
                                    .and_then(|f| f.child("srgbClr")).and_then(|f| f.attr("val"))
                                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                                    .map_or(0xff2748e8, |rgb| 0xff000000 | rgb);
                                let width = stroke.and_then(|s| number(s, "w")).map_or(2.0, |w| (w * scale).max(0.5));
                                shapes.push(importer.shape(tool, vec![bounds.top_left(), bounds.bottom_right()])
                                    .styled(color, width, fill.is_some()));
                            }
                            if let Some(body) = node.child("txBody") {
                                let text = slide_text(body);
                                if !text.trim().is_empty() {
                                    let size = body.find("rPr").and_then(|run| number(run, "sz"))
                                        .map_or(23.0, |sz| (sz * 127.0 * scale).clamp(6.0, 150.0));
                                    let inset = body.child("bodyPr").and_then(|p| number(p, "lIns"))
                                        .map_or(6.0, |v| v * scale);
                                    let top = body.child("bodyPr").and_then(|p| number(p, "tIns"))
                                        .map_or(4.0, |v| v * scale);
                                    let text = wrap(&text, bounds.width() - inset * 2.0, size);
                                    shapes.push(importer.text(bounds.top_left() + Pt::new(inset, top), text, size));
                                }
                            }
                        } else if let Some(body) = node.child("txBody") {
                            fallback.push(slide_text(body));
                        }
                    }
                    "grpSp" | "graphicFrame" => {
                        let mut text = String::new();
                        fallback_text(node, &mut text);
                        fallback.push(text);
                    }
                    _ => {}
                }
            }
        }
        let mut y = if shapes.is_empty() { 16.0 } else {
            shapes.iter().map(|s| s.bounds().b).fold(16.0, f64::max) + 16.0
        };
        for text in fallback.into_iter().filter(|text| !text.trim().is_empty()) {
            let shape = importer.text(Pt::new(16.0, y), wrap(&text, 960.0, 23.0), 23.0);
            y = shape.bounds().b + 12.0;
            shapes.push(shape);
        }
        Ok(shapes)
    }).collect()
}

fn slide_layout(
    package: &mut Package<'_>,
    rels: &HashMap<String, Relationship>,
) -> Result<(Option<Node>, Option<Node>), String> {
    let Some(layout) = rels.values().find(|rel| rel.kind == "slideLayout") else {
        return Ok((None, None));
    };
    let master = package.rels(&layout.path)?.into_values().find(|rel| rel.kind == "slideMaster");
    let master = master.map(|rel| package.xml(&rel.path)).transpose()?.flatten();
    Ok((package.xml(&layout.path)?, master))
}

fn inherited_transform<'a>(shape: &Node, layout: Option<&'a Node>, master: Option<&'a Node>) -> Option<&'a Node> {
    let placeholder = shape.find("ph")?;
    let index = placeholder.attr("idx").unwrap_or("0");
    let layout_shape = layout.and_then(|root| root.find("spTree")).and_then(|tree| {
        tree.nodes.iter().find(|shape| {
            shape.find("ph").is_some_and(|ph| ph.attr("idx").unwrap_or("0") == index)
        })
    });
    if let Some(transform) = layout_shape.and_then(|shape| shape.child("spPr")?.child("xfrm")) {
        return Some(transform);
    }
    let role = layout_shape.and_then(|shape| shape.find("ph")?.attr("type"))
        .or_else(|| placeholder.attr("type")).unwrap_or("obj");
    let role = match role {
        "ctrTitle" => "title",
        "obj" => "body",
        role => role,
    };
    master?.find("spTree")?.nodes.iter().find_map(|shape| {
        if shape.find("ph")?.attr("type")? == role {
            shape.child("spPr")?.child("xfrm")
        } else {
            None
        }
    })
}

fn number(node: &Node, attr: &str) -> Option<f64> {
    node.attr(attr)?.parse::<i64>().ok()
        .filter(|n| n.unsigned_abs() <= 1_000_000_000)
        .map(|n| n as f64)
}

fn slide_bounds(transform: &Node, scale: f64) -> Option<Rect> {
    let offset = transform.child("off")?;
    let extent = transform.child("ext")?;
    let origin = Pt::new(number(offset, "x")?, number(offset, "y")?) * scale;
    let size = Pt::new(number(extent, "cx")?, number(extent, "cy")?) * scale;
    (size.x > 0.0 && size.y > 0.0).then(|| Rect::from_points(origin, origin + size))
}

fn text_box(node: &Node, properties: Option<&Node>) -> bool {
    node.child("nvSpPr").and_then(|p| p.child("cNvSpPr")).and_then(|n| n.attr("txBox")) == Some("1")
        || properties.is_some_and(|properties| properties.child("noFill").is_some()
            && properties.child("ln").is_none_or(|line| line.child("noFill").is_some()))
}

fn slide_text(body: &Node) -> String {
    body.children("p").map(node_text).collect::<Vec<_>>().join("\n")
}

fn fallback_text(node: &Node, out: &mut String) {
    if node.name == "p" {
        node.collect_text(out);
        out.push('\n');
    } else {
        for child in &node.nodes {
            fallback_text(child, out);
        }
    }
}

fn image_path<'a>(node: &Node, rels: &'a HashMap<String, Relationship>) -> Option<&'a str> {
    let blip = node.find("blip")?;
    let image = rels.get(blip.attr("embed")?)?;
    (image.kind == "image").then_some(&image.path)
}

pub(super) fn document(importer: &mut Importer<'_>, bytes: &[u8]) -> Result<Vec<Shape>, String> {
    let mut package = Package::new(bytes)?;
    let main = package.main_part("word/document.xml")?;
    let document = package.required(&main)?;
    let body = document.child("body").ok_or("Document has no body.")?;
    let rels = package.rels(&main)?;
    let mut flow = WordFlow { importer, package, rels, shapes: Vec::new(), y: 0.0 };
    flow.blocks(body)?;
    if flow.shapes.is_empty() {
        return Err("This document has no supported content.".into());
    }
    Ok(flow.shapes)
}

struct WordFlow<'a, 'b> {
    importer: &'a mut Importer<'b>,
    package: Package<'a>,
    rels: HashMap<String, Relationship>,
    shapes: Vec<Shape>,
    y: f64,
}

impl WordFlow<'_, '_> {
    fn blocks(&mut self, body: &Node) -> Result<(), String> {
        for node in &body.nodes {
            match node.name.as_str() {
                "p" => {
                    let mut content = String::new();
                    word_text(node, &mut content);
                    if content.trim().is_empty() {
                        self.y += 12.0;
                    } else {
                        let style = node.child("pPr").and_then(|p| p.child("pStyle"))
                            .and_then(|s| s.attr("val")).unwrap_or_default().to_ascii_lowercase();
                        let size = if style == "title" || style == "heading1" { 34.0 }
                            else if style.starts_with("heading") { 28.0 }
                            else { 20.0 };
                        let font = u8::from(size > 20.0);
                        let text = wrap(&content, 760.0, size);
                        let shape = self.importer.text(Pt::new(0.0, self.y), text, size).with_font(font);
                        self.y = shape.bounds().b + 12.0;
                        self.shapes.push(shape);
                    }
                    self.pictures(node)?;
                }
                "tbl" => {
                    let rows: Vec<Vec<String>> = node.children("tr").map(|row| {
                        row.children("tc").map(|cell| {
                            cell.children("p").map(|p| {
                                let mut text = String::new();
                                word_text(p, &mut text);
                                text
                            }).collect::<Vec<_>>().join("\n")
                        }).collect()
                    }).collect();
                    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
                    if rows.len().saturating_mul(width) > MAX_GRID_CELLS {
                        return Err("The document table is too large to import.".into());
                    }
                    tables(self.importer, &mut self.shapes, &rows, width, self.y);
                    self.y += rows.len() as f64 * CELL_HEIGHT + 18.0;
                    self.pictures(node)?;
                }
                "sdt" | "sdtContent" | "customXml" | "ins" => self.blocks(node)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn pictures(&mut self, node: &Node) -> Result<(), String> {
        if matches!(node.name.as_str(), "drawing" | "pict") {
            let path = image_path(node, &self.rels).or_else(|| {
                let image = self.rels.get(node.find("imagedata")?.relationship_id()?)?;
                (image.kind == "image").then_some(image.path.as_str())
            });
            if let Some(path) = path {
                if let Some((asset, width, height)) = self.importer.embedded_image(&mut self.package, path)? {
                    let mut size = fit(width, height, 700.0, 600.0, true);
                    if let Some(extent) = node.find("extent") {
                        if let (Some(w), Some(h)) = (number(extent, "cx"), number(extent, "cy")) {
                            if w > 0.0 && h > 0.0 {
                                size = fit(w as u32, h as u32, (w / 9525.0).min(700.0), (h / 9525.0).min(600.0), false);
                            }
                        }
                    }
                    let origin = Pt::new(0.0, self.y);
                    self.shapes.push(self.importer.image(asset, Rect::from_points(origin, origin + size), false));
                    self.y += size.y + 12.0;
                }
            }
        } else if node.name == "AlternateContent" {
            if let Some(choice) = node.child("Choice").or_else(|| node.child("Fallback")) {
                self.pictures(choice)?;
            }
        } else if node.name != "del" {
            for child in &node.nodes {
                self.pictures(child)?;
            }
        }
        Ok(())
    }
}

fn word_text(node: &Node, text: &mut String) {
    match node.name.as_str() {
        "t" | "tab" | "br" | "cr" => node.collect_text(text),
        "drawing" | "pict" | "del" => {}
        _ => node.nodes.iter().for_each(|child| word_text(child, text)),
    }
}

fn node_text(node: &Node) -> String {
    let mut text = String::new();
    node.collect_text(&mut text);
    text
}

fn wrap(text: &str, width: f64, size: f64) -> String {
    let max = (width / (size * 0.54)).clamp(10.0, 140.0) as usize;
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut occupied = 0;
        for word in line.split_whitespace() {
            let len = word.chars().count();
            if occupied > 0 {
                if occupied + len + 1 > max {
                    out.push('\n');
                    occupied = 0;
                } else {
                    out.push(' ');
                    occupied += 1;
                }
            }
            for ch in word.chars() {
                if occupied == max {
                    out.push('\n');
                    occupied = 0;
                }
                out.push(ch);
                occupied += 1;
            }
        }
    }
    out
}

pub(super) fn spreadsheet(importer: &mut Importer<'_>, bytes: &[u8]) -> Result<Vec<Shape>, String> {
    let mut package = Package::new(bytes)?;
    let main = package.main_part("xl/workbook.xml")?;
    let workbook = package.required(&main)?;
    let rels = package.rels(&main)?;
    let sheet = workbook.child("sheets").and_then(|list| list.child("sheet"))
        .and_then(|sheet| sheet.relationship_id()).and_then(|id| rels.get(id))
        .filter(|rel| rel.kind == "worksheet").ok_or("Workbook has no first worksheet.")?;
    let worksheet = package.required(&sheet.path)?;
    let shared = if let Some(part) = rels.values().find(|r| r.kind == "sharedStrings") {
        package.xml(&part.path)?
    } else {
        package.xml("xl/sharedStrings.xml")?
    };
    let shared: Vec<String> = shared.as_ref().into_iter().flat_map(|s| s.children("si"))
        .map(node_text).collect();
    let mut cells = HashMap::new();
    let data = worksheet.child("sheetData").ok_or("Worksheet has no cell data.")?;
    let mut next_row = 0;
    for row in data.children("row") {
        let row_index = row.attr("r").and_then(|r| r.parse::<usize>().ok())
            .and_then(|r| r.checked_sub(1)).unwrap_or(next_row);
        next_row = row_index.saturating_add(1);
        let mut next_col = 0;
        for cell in row.children("c") {
            let (r, c) = cell.attr("r").and_then(cell_position).unwrap_or((row_index, next_col));
            next_col = c.saturating_add(1);
            if r >= 1_048_576 || c >= 16_384 {
                return Err("Invalid worksheet cell position.".into());
            }
            // Formula results are in v alongside f. A missing cache is left blank.
            let value = cell.child("v").map_or("", |value| value.text.as_str());
            let text = match cell.attr("t") {
                Some("s") => value.trim().parse::<usize>().ok().and_then(|i| shared.get(i)).cloned().unwrap_or_default(),
                Some("inlineStr") => cell.child("is").map(node_text).unwrap_or_default(),
                Some("b") => match value.trim() {
                    "1" => "TRUE".to_string(),
                    "0" => "FALSE".to_string(),
                    _ => String::new(),
                },
                _ => value.to_string(),
            };
            if !text.is_empty() {
                cells.insert((r, c), text);
            }
        }
    }
    let low_r = cells.keys().map(|p| p.0).min().ok_or("Worksheet has no values to import.")?;
    let low_c = cells.keys().map(|p| p.1).min().unwrap();
    let height = cells.keys().map(|p| p.0).max().unwrap() - low_r + 1;
    let width = cells.keys().map(|p| p.1).max().unwrap() - low_c + 1;
    if height.saturating_mul(width) > MAX_GRID_CELLS {
        return Err("The worksheet's used area is too large to import.".into());
    }
    let mut rows = vec![vec![String::new(); width]; height];
    for ((row, col), value) in cells {
        rows[row - low_r][col - low_c] = value;
    }
    let mut shapes = Vec::new();
    tables(importer, &mut shapes, &rows, width, 0.0);
    Ok(shapes)
}

fn cell_position(addr: &str) -> Option<(usize, usize)> {
    let mut split = addr.splitn(2, |ch: char| ch.is_ascii_digit());
    let letters = split.next()?;
    let column = letters.bytes().try_fold(0usize, |value, ch| {
        value.checked_mul(26)?.checked_add(usize::from(ch.to_ascii_uppercase().checked_sub(b'A')?) + 1)
    })?.checked_sub(1)?;
    let row = addr.get(letters.len()..)?.parse::<usize>().ok()?.checked_sub(1)?;
    Some((row, column))
}

fn tables(importer: &mut Importer<'_>, shapes: &mut Vec<Shape>, rows: &[Vec<String>], width: usize, y: f64) {
    for first_row in (0..rows.len()).step_by(MAX_SIZE) {
        for first_col in (0..width).step_by(MAX_SIZE) {
            let row_count = (rows.len() - first_row).min(MAX_SIZE);
            let col_count = (width - first_col).min(MAX_SIZE);
            let mut table = Table::new(row_count, col_count);
            for row in 0..row_count {
                for column in 0..col_count {
                    if let Some(value) = rows[first_row + row].get(first_col + column) {
                        table.set_text(Cell { row, column }, value);
                    }
                }
            }
            let origin = Pt::new(first_col as f64 * CELL_WIDTH, y + first_row as f64 * CELL_HEIGHT);
            let size = Pt::new(col_count as f64 * CELL_WIDTH, row_count as f64 * CELL_HEIGHT);
            let shape = importer.shape(Tool::Table, vec![origin, origin + size])
                .styled(0xff4a5160, 1.5, false).with_text("", 16.0).with_table(table);
            shapes.push(shape);
        }
    }
}
