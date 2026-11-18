//! Editor operations for formula/graph insertion and graph range changes.

use super::{Editor, Pt, Rect, Shape, Tool};
use crate::math::{self, GraphRange};
use std::sync::Arc;

impl Editor {
    pub(crate) fn selected_math(&self) -> Option<&Arc<Shape>> {
        let [id] = self.doc.selected_ids() else { return None };
        self.doc.get(id).filter(|s| matches!(s.tool, Tool::Formula | Tool::Graph))
    }

    pub(crate) fn commit_math(&mut self, id: &str, tool: Tool, text: &str) -> Result<(), &'static str> {
        let text = text.trim();
        let calculation = (tool == Tool::Formula).then(|| math::calculation_source(text)).flatten();
        let text = calculation.as_deref().unwrap_or(text);
        match tool {
            Tool::Formula => math::validate_formula(text)?,
            Tool::Graph => math::validate_graph(text)?,
            _ => return Err("Choose a formula or a graph."),
        }
        let size = (36.0 / self.zoom).clamp(6.0, 400.0);
        if !id.is_empty() {
            if let Some(shape) = self.doc.get(id).filter(|s| s.tool == tool && s.text != text) {
                let next = (**shape).clone().with_text(text, shape.font_size);
                let applied = self.doc.replace(next);
                self.on_applied(applied);
            }
        } else if tool == Tool::Formula {
            let (w, h) = self.viewport();
            let layout = math::formula_layout(text, size, self.font);
            let center = self.world(Pt::new(w / 2.0, h / 2.0));
            let origin = center - Pt::new(layout.width / 2.0, layout.height / 2.0);
            let shape = Shape::new(self.next_id(), tool, vec![origin])
                .styled(self.color, 3.0, false).with_font(self.font).with_text(text, size);
            self.insert_selected(shape);
        } else {
            self.insert_graph(text, None);
        }
        Ok(())
    }

    pub(crate) fn plot_selected_formula(&mut self) {
        let Some(shape) = self.selected_math().filter(|s| {
            s.tool == Tool::Formula && math::graphable_equation(&s.text)
        }).cloned() else { return };
        self.insert_graph(&shape.text, Some(shape.bounds()));
    }

    fn insert_graph(&mut self, source: &str, beside: Option<Rect>) {
        let (w, h) = self.viewport();
        let screen_size = Pt::new((w * 0.56).clamp(160.0, 500.0), (h * 0.52).clamp(130.0, 340.0));
        let size = screen_size / self.zoom;
        let top_left = self.world(Pt::new(16.0, 16.0));
        let bottom_right = self.world(Pt::new(w - 16.0, h - 16.0));
        let center = self.world(Pt::new(w / 2.0, h / 2.0));
        let gap = 24.0 / self.zoom;
        let origin = beside.and_then(|b| {
            let candidates = [
                Pt::new(b.r + gap, b.t),
                Pt::new(b.l, b.b + gap),
                Pt::new(b.l - gap - size.x, b.t),
                Pt::new(b.l, b.t - gap - size.y),
            ];
            candidates.into_iter().find(|p| p.x >= top_left.x && p.y >= top_left.y
                && p.x + size.x <= bottom_right.x && p.y + size.y <= bottom_right.y)
        }).unwrap_or(center - size / 2.0);
        let shape = Shape::new(self.next_id(), Tool::Graph, vec![origin, origin + size])
            .styled(math::ACCENT, 3.0, false).with_font(self.font).with_text(source, 24.0);
        self.insert_selected(shape);
    }

    pub(crate) fn change_graph_range(&mut self, pan: Pt, scale: Option<f64>) {
        let Some(graph) = self.selected_math().filter(|s| s.tool == Tool::Graph) else { return };
        let range = scale.map_or_else(GraphRange::default, |scale| graph.graph_range.adjusted(pan, scale));
        if range != graph.graph_range {
            let mut next = (**graph).clone();
            next.graph_range = range;
            let applied = self.doc.replace(next);
            self.on_applied(applied);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_calculation_and_ink_conversion_are_undoable() {
        let mut editor = Editor::new();
        editor.commit_text("", 0.0, 0.0, "12 x 7 + 3 =");
        assert_eq!(editor.doc.shapes()[0].tool, Tool::Formula);
        assert_eq!(editor.doc.shapes()[0].text, "12 × 7 + 3 =");
        editor.undo();
        assert!(editor.doc.shapes().is_empty());
        editor.redo();
        assert_eq!(editor.doc.shapes()[0].tool, Tool::Formula);

        editor.doc.add(Shape::new("stroke", Tool::Pen, vec![Pt::ZERO, Pt::new(80.0, 30.0)]));
        editor.commit_ink_candidates(&["stroke".into()], &["1 l 2 =", "1 / 2 ="]);
        assert!(editor.doc.get("stroke").is_none());
        assert!(editor.doc.shapes().iter().any(|s| s.tool == Tool::Formula && s.text == "1 / 2 ="));
        editor.undo();
        assert_eq!(editor.doc.get("stroke").unwrap().tool, Tool::Pen);
    }

    #[test]
    fn graph_range_is_stored_on_the_shape_and_survives_undo_redo() {
        let mut editor = Editor::new();
        editor.commit_math("", Tool::Formula, "y = sin(x)").unwrap();
        editor.plot_selected_formula();
        assert_eq!(editor.doc.shapes().len(), 2);
        let graph = editor.selected_math().unwrap().clone();
        assert_eq!(graph.tool, Tool::Graph);
        editor.change_graph_range(Pt::new(0.25, 0.0), Some(0.5));
        let changed = editor.doc.get(&graph.id).unwrap().clone();
        assert_ne!(changed.graph_range, graph.graph_range);
        assert_eq!(Shape::from_json(&changed.to_json()).unwrap(), *changed);
        let mut default_range = changed.to_json();
        default_range.as_object_mut().unwrap().remove("graphRange");
        assert_eq!(Shape::from_json(&default_range).unwrap().graph_range, GraphRange::default());
        editor.undo();
        assert_eq!(editor.doc.get(&graph.id).unwrap().graph_range, graph.graph_range);
        editor.redo();
        assert_eq!(editor.doc.get(&graph.id).unwrap().graph_range, changed.graph_range);
    }
}
