use super::{Act, App, AskFor, ButtonKind, Look, Rf, Ui, SURFACE};
use crate::{math, Pt, Shape, Tool};

impl App {
    pub(super) fn ask_math(&mut self, tool: Tool, id: String, text: &str) {
        let (title, hint) = match (tool, id.is_empty()) {
            (Tool::Formula, true) => ("Add formula", "x^2 + 1/2 = sqrt(y)"),
            (Tool::Formula, false) => ("Edit formula", "x^2 + 1/2 = sqrt(y)"),
            (Tool::Graph, true) => ("Add graph", "y = sin(x); y = x^2 (trig uses radians)"),
            (Tool::Graph, false) => ("Edit graph", "y = sin(x); y = x^2 (trig uses radians)"),
            _ => return,
        };
        self.ask(title, text, hint, true, AskFor::Math { id, tool });
    }

    pub(super) fn edit_math(&mut self) {
        if let Some(shape) = self.ed.selected_math().cloned() {
            self.ask_math(shape.tool, shape.id.clone(), &shape.text);
        }
    }

    pub(super) fn math_actions(shape: &Shape) -> Vec<(&'static str, f32, Act, bool)> {
        let mut actions = vec![("Edit", 82.0, Act::EditMath, true)];
        if shape.tool == Tool::Graph {
            actions.push(("Range", 98.0, Act::GraphRange, true));
        } else if math::graphable_equation(&shape.text) {
            actions.push(("Plot", 82.0, Act::Plot, true));
        }
        actions
    }

    pub(super) fn layout_graph_range(&self, ui: &mut Ui, selection: Rf) {
        let (w, h) = self.extent();
        let scale = ((w - 32.0) / 332.0).min((h - 32.0) / 174.0).min(1.0);
        if scale <= 0.0 {
            return;
        }
        let (width, height) = (332.0 * scale, 174.0 * scale);
        let preferred = selection.bottom() + 12.0;
        let x = (selection.cx() - width / 2.0).clamp(16.0, w - width - 16.0);
        let y = preferred.min(h - height - 16.0);
        let r = Rf::new(x, y, width, height);
        ui.scaled(
            r, Look::Surface { color: SURFACE, radius: 28.0, level: 2, border: true },
            Some(Act::Nothing), scale,
        );
        let button = |x, y, w, label: &str, act, ui: &mut Ui| {
            ui.scaled(
                Rf::new(r.x + x * scale, r.y + y * scale, w * scale, 72.0 * scale),
                Look::Button { icon: None, text: label.into(), kind: ButtonKind::Secondary, pill: false, level: 0 },
                Some(act), scale,
            );
        };
        for (i, (label, x, y)) in [("←", -0.25, 0.0), ("↑", 0.0, 0.25), ("↓", 0.0, -0.25), ("→", 0.25, 0.0)]
            .into_iter().enumerate()
        {
            button(12.0 + i as f32 * 78.0, 12.0, 72.0, label,
                Act::GraphAdjust { pan: Pt::new(x, y), scale: Some(1.0) }, ui);
        }
        for (label, x, width, scale) in [
            ("−", 12.0, 72.0, Some(1.25)), ("Reset", 90.0, 152.0, None), ("+", 248.0, 72.0, Some(0.8)),
        ] {
            button(x, 90.0, width, label, Act::GraphAdjust { pan: Pt::ZERO, scale }, ui);
        }
    }
}
