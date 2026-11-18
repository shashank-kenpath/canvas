use super::{square, table, Act, App, ButtonKind, Face, Ico, Rf, Ui, TEXT1};

impl App {
    pub(super) fn layout_table_size(&self, ui: &mut Ui, rows: usize, columns: usize) {
        let (w, h) = self.extent();
        let dw = 520.0_f32.min(w - 48.0);
        let dh = 384.0_f32.min(h - 32.0);
        let r = Rf::new((w - dw) / 2.0, (h - dh) / 2.0, dw, dh);
        let inset = (dw * 0.075).min(40.0);
        let line_h = ((dh - 160.0) / 3.0).clamp(0.0, 72.0);
        ui.surface(r, 32.0, 3, Some(Act::Nothing));
        ui.label(
            Rf::new(r.x + inset, r.y + 16.0, dw - 2.0 * inset, line_h),
            "Insert table", Face::Bold, 36.0, TEXT1, false,
        );
        for (i, (label, value, minus, plus)) in [
            ("Rows", rows, Act::TableDimensions(-1, 0), Act::TableDimensions(1, 0)),
            ("Columns", columns, Act::TableDimensions(0, -1), Act::TableDimensions(0, 1)),
        ].into_iter().enumerate() {
            let y = r.y + 20.0 + (1 + i) as f32 * (line_h + 4.0);
            let right = r.right() - inset;
            let button = 64.0_f32.min(dw / 6.0).min(line_h);
            ui.label(
                Rf::new(r.x + inset, y, dw - 2.0 * inset - 3.0 * button, line_h),
                label, Face::Medium, 24.0, TEXT1, false,
            );
            ui.icon(square(right - 3.0 * button, y, button), Ico::Minus, false, value > 1, minus);
            ui.label(
                Rf::new(right - 2.0 * button, y, button, button),
                value.to_string(), Face::Bold, 24.0, TEXT1, true,
            );
            ui.icon(square(right - button, y, button), Ico::Plus, false, value < table::MAX_SIZE, plus);
        }
        self.dialog_buttons(ui, r, inset, "Cancel", "Insert", ButtonKind::Primary);
    }
}
