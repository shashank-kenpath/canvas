use super::{
    Act, App, BoardChrome, Done, FileKind, Imported, InsertKind, Look, Page, PenMode, Pt, Rf, Tool, Ui,
    INSERT,
};
use crate::document::scene_bounds;
use crate::import::{self, PickerFile};
use crate::ui::icons::Ico;
use std::sync::Arc;

pub(super) struct PendingFile {
    token: i32,
    current_page: bool,
    parsing: bool,
}

impl App {
    pub(super) fn pick_file(&mut self, kind: FileKind, current_page: bool) {
        self.dismiss();
        if self.board.is_none() {
            return;
        }
        if self.pending_file.is_some() {
            self.show_toast("A file import is in progress");
            return;
        }
        self.file_token = self.file_token.wrapping_add(1);
        self.pending_file = Some(PendingFile { token: self.file_token, current_page, parsing: false });
        self.platform.pick_file(self.file_token, kind);
    }

    /// Result from Android's system picker. Parsing runs off the render thread.
    pub fn file_result(&mut self, token: i32, file: PickerFile) {
        let Some(pending) = self.pending_file.as_mut().filter(|p| p.token == token && !p.parsing) else {
            return;
        };
        match file {
            PickerFile::Cancelled => {
                self.pending_file = None;
                return;
            }
            PickerFile::Error(error) => {
                self.pending_file = None;
                self.error_toast(&error);
                return;
            }
            _ => {}
        }
        pending.parsing = true;
        let current_page = pending.current_page;
        let prefix = self.ed.next_id();
        let done = self.file_done.clone();
        let wake = self.wake.clone();
        let worker = std::thread::Builder::new().name("file-import".into()).spawn(move || {
            let result = import::parse(file, &prefix, current_page);
            if done.send(Done::File(token, result)).is_ok() {
                wake();
            }
        });
        if worker.is_ok() {
            self.show_toast("Importing file…");
        } else {
            self.pending_file = None;
            self.error_toast("Could not start file import");
        }
    }

    pub(super) fn apply_file(&mut self, token: i32, result: Result<Imported, String>) {
        if self.pending_file.as_ref().is_none_or(|p| p.token != token) {
            return;
        }
        self.pending_file = None;
        match result {
            Err(error) => self.error_toast(&error),
            Ok(Imported::Pages(_)) if self.joined() => {
                self.error_toast("Open a board you own to import a presentation");
            }
            Ok(Imported::Pages(pages)) => {
                let count = pages.len();
                let first = self.pages.len();
                self.pages.extend(pages.into_iter().map(|shapes| Page {
                    shapes: shapes.into_iter().map(Arc::new).collect(),
                    view: None,
                }));
                self.change_page(super::PageChange::Open(first));
                self.show_toast(&format!("Inserted {count} {}", if count == 1 { "page" } else { "pages" }));
            }
            Ok(Imported::Shapes(shapes)) => {
                let shapes: Vec<_> = shapes.into_iter().map(Arc::new).collect();
                let Some(bounds) = scene_bounds(&shapes) else { return };
                self.ed.finish_input(self.now);
                self.take_editor_events();
                let width = f64::from(self.w) / f64::from(self.d);
                let height = f64::from(self.h) / f64::from(self.d);
                let view = self.ed.world(Pt::new(width, height)) - self.ed.world(Pt::ZERO);
                // Keep the top of long documents and PDF strips in view.
                let half = Pt::new(bounds.width().min(view.x * 0.75), bounds.height().min(view.y * 0.75)) * 0.5;
                let offset = self.ed.world(Pt::new(width, height) * 0.5) - half - bounds.top_left();
                let shapes: Vec<_> = shapes.iter().map(|s| {
                    s.with_points(s.points.iter().map(|p| *p + offset).collect())
                }).collect();
                let image = (shapes.len() == 1 && shapes[0].tool == Tool::Image)
                    .then(|| shapes[0].id.clone());
                self.ed.apply_remote(shapes, Vec::new());
                if let Some(id) = image {
                    self.choose_tool(Tool::Select, PenMode::Ink, false);
                    self.ed.doc.select(Some(&id));
                }
                self.take_editor_events();
                self.show_toast("File inserted");
            }
        }
    }

    pub(super) fn layout_insert(&self, ui: &mut Ui, chrome: &BoardChrome, pdf: bool) {
        let rows: Vec<_> = if pdf {
            vec![
                (Ico::PagePlus, "New pages", "Locked backgrounds", Act::ImportPdf(false)),
                (Ico::Page, "Current page", "Movable images", Act::ImportPdf(true)),
            ]
        } else {
            INSERT.iter().map(|&(icon, label, kind)| {
                let detail = match kind {
                    InsertKind::File(FileKind::Document) => "PPTX, DOCX, XLSX, SVG",
                    _ => "",
                };
                (icon, label, detail, Act::InsertItem(kind))
            }).collect()
        };
        let width = 520.0;
        let row_height = 64.0;
        let height = 24.0 + row_height * rows.len() as f32;
        let (r, scale) = self.popover(chrome.dock, width, height, chrome.buttons[8], 20.0, true);
        if scale <= 0.0 {
            return;
        }
        ui.scaled(
            r,
            Look::Surface { color: super::SURFACE, radius: 28.0, level: 2, border: true },
            Some(Act::Nothing),
            scale,
        );
        for (index, (icon, label, detail, act)) in rows.into_iter().enumerate() {
            ui.scaled(
                Rf::new(r.x + 12.0 * scale, r.y + (12.0 + row_height * index as f32) * scale,
                    (width - 24.0) * scale, row_height * scale),
                Look::Row {
                    icon: Some(icon),
                    text: label.into(),
                    value: detail.into(),
                    chevron: !self.joined() && act == Act::InsertItem(InsertKind::File(FileKind::Pdf)),
                    danger: false,
                },
                Some(act),
                scale,
            );
        }
    }
}
