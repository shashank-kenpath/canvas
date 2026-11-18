use super::*;
use crate::editor::{Editor, PenMode};
use crate::render;
use crate::store;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde_json::json;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

fn raster(format: ImageFormat, color: [u8; 4]) -> Vec<u8> {
    let image = RgbaImage::from_pixel(8, 4, Rgba(color));
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image).write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn file(name: &str, bytes: Vec<u8>) -> Result<Imported, String> {
    parse(PickerFile::File { name: name.into(), mime: String::new(), bytes }, "test", false)
}

fn shapes(imported: Imported) -> Vec<Shape> {
    let Imported::Shapes(shapes) = imported else { panic!("expected shapes") };
    shapes
}

fn zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

const RED: [u8; 4] = [255, 0, 0, 255];

#[test]
fn png_jpeg_webp_and_bmp_become_images() {
    for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP, ImageFormat::Bmp] {
        let result = shapes(file("photo", raster(format, RED)).unwrap());
        let shape = &result[0];
        assert_eq!(shape.tool, Tool::Image);
        assert_eq!(shape.bounds().width() / shape.bounds().height(), 2.0);
        let image = shape.image.as_ref().unwrap().pixmap().unwrap();
        assert!(image.pixel(4, 2).unwrap().red() > 230, "{format:?}");
    }
}

#[test]
fn gif_import_uses_the_first_frame() {
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        for color in [RED, [0, 0, 255, 255]] {
            let frame = image::Frame::new(RgbaImage::from_pixel(8, 4, Rgba(color)));
            encoder.encode_frame(frame).unwrap();
        }
    }
    let result = shapes(file("two-frames.gif", bytes).unwrap());
    let bitmap = result[0].image.as_ref().unwrap().pixmap().unwrap();
    let pixel = bitmap.pixel(4, 2).unwrap();
    assert!(pixel.red() > 230 && pixel.blue() < 20);
}

#[test]
fn svg_preserves_transparency_and_color() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 30 20"><path fill="#ff0000" d="M0 0h15v20H0z"/></svg>"##;
    let result = shapes(file("drawing.svg", svg.to_vec()).unwrap());
    let image = result[0].image.as_ref().unwrap().pixmap().unwrap();
    assert_eq!((image.width(), image.height()), (60, 40));
    assert_eq!(image.pixel(5, 5).unwrap().red(), 255);
    assert_eq!(image.pixel(55, 5).unwrap().alpha(), 0);
}

#[test]
fn android_pdf_page_images_keep_order_and_placement() {
    let pdf = || PickerFile::Pdf(vec![
        raster(ImageFormat::Png, RED),
        raster(ImageFormat::Png, [0, 0, 255, 255]),
    ]);
    let Imported::Pages(pages) = parse(pdf(), "pdf", false).unwrap() else { panic!("expected pages") };
    assert_eq!(pages.len(), 2);
    assert!(pages.iter().all(|p| p.len() == 1 && p[0].page_background));
    let second = &pages[1][0];
    assert_eq!(second.image.as_ref().unwrap().pixmap().unwrap().pixel(2, 2).unwrap().blue(), 255);
    assert!(!second.hits(second.bounds().center(), 10.0));
    let movable = shapes(parse(pdf(), "pdf", true).unwrap());
    assert!(movable.iter().all(|s| s.hits(s.bounds().center(), 0.0)));
    assert!(movable[1].bounds().t > movable[0].bounds().b);
}

#[test]
fn powerpoint_uses_slide_order_and_recovers_editable_content() {
    let main = br#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="100" r:id="b"/><p:sldId id="101" r:id="a"/></p:sldIdLst><p:sldSz cx="10000000" cy="6000000"/></p:presentation>"#;
    let relations = br#"<Relationships><Relationship Id="a" Type="x/slide" Target="slides/slide1.xml"/><Relationship Id="b" Type="x/slide" Target="slides/slide2.xml"/></Relationships>"#;
    let first = br#"<p:sld xmlns:p="p" xmlns:a="a" xmlns:r="r"><p:cSld><p:spTree>
        <p:sp><p:spPr><a:xfrm><a:off x="1000000" y="2000000"/><a:ext cx="3000000" cy="1000000"/></a:xfrm></p:spPr><p:txBody><a:bodyPr lIns="0" tIns="0"/><a:p><a:r><a:t>Slide &amp; text</a:t></a:r></a:p></p:txBody></p:sp>
        <p:sp><p:spPr><a:xfrm><a:off x="1000000" y="200000"/><a:ext cx="2000000" cy="1000000"/></a:xfrm><a:prstGeom prst="ellipse"/></p:spPr></p:sp>
        <p:sp><p:spPr><a:xfrm><a:off x="4200000" y="100000"/><a:ext cx="1000000" cy="800000"/></a:xfrm><a:prstGeom prst="rect"/></p:spPr></p:sp>
        <p:pic><p:blipFill><a:blip r:embed="img"/></p:blipFill><p:spPr><a:xfrm><a:off x="6000000" y="2000000"/><a:ext cx="2000000" cy="1000000"/></a:xfrm></p:spPr></p:pic>
        <p:sp><p:nvSpPr><p:nvPr><p:ph idx="3"/></p:nvPr></p:nvSpPr><p:txBody><a:bodyPr lIns="0" tIns="0"/><a:p><a:r><a:t>Inherited</a:t></a:r></a:p></p:txBody></p:sp>
        </p:spTree></p:cSld></p:sld>"#;
    let second = br#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>Layout fallback</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
    let media = br#"<Relationships><Relationship Id="img" Type="x/image" Target="../media/image.png"/><Relationship Id="layout" Type="x/slideLayout" Target="../slideLayouts/layout.xml"/></Relationships>"#;
    let layout = br#"<p:sldLayout xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph idx="3"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="3000000" y="4000000"/><a:ext cx="3000000" cy="1000000"/></a:xfrm></p:spPr></p:sp></p:spTree></p:cSld></p:sldLayout>"#;
    let png = raster(ImageFormat::Png, RED);
    let bytes = zip(&[
        ("ppt/presentation.xml", main),
        ("ppt/_rels/presentation.xml.rels", relations),
        ("ppt/slides/slide1.xml", second),
        ("ppt/slides/slide2.xml", first),
        ("ppt/slides/_rels/slide2.xml.rels", media),
        ("ppt/slideLayouts/layout.xml", layout),
        ("ppt/media/image.png", &png),
    ]);
    let Imported::Pages(pages) = file("slides.pptx", bytes).unwrap() else { panic!("expected slides") };
    assert_eq!(pages.len(), 2);
    let first = &pages[0];
    assert_eq!(first.iter().map(|s| s.tool).collect::<Vec<_>>(), [Tool::Text, Tool::Ellipse, Tool::Rectangle, Tool::Image, Tool::Text]);
    assert_eq!(first[0].points[0], Pt::new(100.0, 200.0));
    assert_eq!(first[0].text, "Slide & text");
    assert_eq!(first[3].bounds().top_left(), Pt::new(600.0, 200.0));
    assert_eq!(first[4].points[0], Pt::new(300.0, 400.0));
    assert_eq!(first[4].text, "Inherited");
    assert_eq!(pages[1][0].text, "Layout fallback");
}

#[test]
fn word_flows_headings_images_and_tables() {
    let document = br#"<w:document xmlns:w="w" xmlns:a="a" xmlns:r="r"><w:body>
        <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>A heading</w:t></w:r></w:p>
        <w:p><w:r><w:t>A paragraph</w:t><w:br/><w:t>Extra line</w:t></w:r><w:r><w:drawing><a:blip r:embed="pic"/></w:drawing></w:r></w:p>
        <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Cell 1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Cell 2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
        </w:body></w:document>"#;
    let media = br#"<Relationships><Relationship Id="pic" Type="x/image" Target="media/image.png"/></Relationships>"#;
    let png = raster(ImageFormat::Png, RED);
    let bytes = zip(&[("word/document.xml", document), ("word/_rels/document.xml.rels", media), ("word/media/image.png", &png)]);
    let shapes = shapes(file("notes.docx", bytes).unwrap());
    assert_eq!(shapes.iter().map(|s| s.tool).collect::<Vec<_>>(), [Tool::Text, Tool::Text, Tool::Image, Tool::Table]);
    assert!(shapes[0].font_size > shapes[1].font_size);
    assert_eq!(shapes[1].text, "A paragraph\nExtra line");
    assert_eq!(shapes[3].table.as_ref().unwrap().cells(), ["Cell 1", "Cell 2"]);
    assert!(shapes.windows(2).all(|pair| pair[0].bounds().b < pair[1].bounds().t));
}

#[test]
fn excel_imports_the_first_sheet_and_uses_cached_formula_values() {
    let workbook = br#"<workbook xmlns:r="r"><sheets><sheet name="First" r:id="b"/><sheet name="Second" r:id="a"/></sheets></workbook>"#;
    let rels = br#"<Relationships><Relationship Id="a" Type="x/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="b" Type="x/worksheet" Target="/xl/worksheets/sheet2.xml"/></Relationships>"#;
    let sheet = br#"<worksheet><sheetData><row r="4"><c r="B4" t="s"><v>0</v></c><c r="D4"><f>1+2</f><v>3</v></c></row>
        <row r="5"><c r="B5" t="inlineStr"><is><r><t>Cat</t></r><r><t> &amp; dog</t></r></is></c><c r="C5" t="b"><v>1</v></c></row></sheetData></worksheet>"#;
    let shared = br#"<sst><si><r><t>Food </t></r><r><t>cost</t></r><rPh><t>ignored</t></rPh></si></sst>"#;
    let bytes = zip(&[("xl/workbook.xml", workbook), ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet2.xml", sheet), ("xl/sharedStrings.xml", shared)]);
    let shapes = shapes(file("values.xlsx", bytes).unwrap());
    let table = shapes[0].table.as_ref().unwrap();
    assert_eq!((table.rows(), table.columns()), (2, 3));
    assert_eq!(table.cells(), ["Food cost", "", "3", "Cat & dog", "TRUE", ""]);
}

#[test]
fn image_survives_the_wire_and_hash_sidecar_round_trips() {
    let shape = shapes(file("red.png", raster(ImageFormat::Png, RED)).unwrap()).remove(0);
    let wire = shape.to_json();
    assert!(wire["imageData"].as_str().is_some());
    assert_eq!(Shape::from_json(&wire).unwrap(), shape);
    let temp = std::env::temp_dir().join(format!("inkboard-images-{}", std::process::id()));
    let mut extra = serde_json::Map::new();
    extra.insert("pages".into(), json!([{"shapes": [wire]}]));
    let portable = store::encode("board", "Images", "now", &[Arc::new(shape.clone())], &extra);
    store::save_atomic(&temp, "board", &portable).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(temp.join("board.json")).unwrap()).unwrap();
    assert!(json["shapes"][0].get("imageData").is_none());
    let hash = json["shapes"][0]["imageRef"].as_str().unwrap();
    assert_eq!(hash.len(), 64);
    assert_eq!(json["pages"][0]["shapes"][0]["imageRef"], hash);
    assert_eq!(std::fs::read_dir(temp.join("board.assets")).unwrap().count(), 1);
    let restored = store::load(&temp, "board").unwrap();
    assert_eq!(restored.shapes, [shape.clone()]);
    assert_eq!(Shape::from_json(&restored.extra["pages"][0]["shapes"][0]).unwrap(), shape);
    let thumbnail = render::thumbnail(&[Arc::new(shape)], 64, 64);
    assert!(thumbnail.chunks_exact(4).any(|pixel| pixel == RED));
    store::delete(&temp, "board").unwrap();
    assert!(!temp.join("board.assets").exists());
    let _ = std::fs::remove_dir(temp);
}

#[test]
fn image_moves_resizes_and_erases_through_editor_tools() {
    let source = shapes(file("red.png", raster(ImageFormat::Png, RED)).unwrap()).remove(0);
    let image = source.with_points(vec![Pt::new(60.0, 60.0), Pt::new(140.0, 110.0)]);
    let id = image.id.clone();
    let mut editor = Editor::new();
    editor.resize(512, 512, 1.0);
    editor.set_tool(Tool::Select, PenMode::Ink);
    editor.apply_remote(vec![image], Vec::new());
    let start = Pt::new(100.0, 85.0);
    let end = Pt::new(150.0, 120.0);
    editor.pointer_down(1, start, 1);
    editor.pointer_move(1, end, 5);
    editor.pointer_up(1, end, 10, false);
    let moved = editor.doc.get(&id).unwrap().bounds();
    assert_eq!(moved.top_left(), Pt::new(110.0, 95.0));
    let handle = moved.inflate(6.0).bottom_right();
    let end = handle + Pt::new(50.0, 30.0);
    editor.pointer_down(2, handle, 15);
    editor.pointer_move(2, end, 20);
    editor.pointer_up(2, end, 25, false);
    let resized = editor.doc.get(&id).unwrap().bounds();
    assert!(resized.width() > moved.width());
    editor.set_tool(Tool::Eraser, PenMode::Ink);
    editor.pointer_down(3, resized.center(), 35);
    editor.pointer_up(3, resized.center(), 40, false);
    assert!(!editor.doc.contains(&id));
    editor.undo();
    assert_eq!(editor.doc.get(&id).unwrap().bounds(), resized);
}
