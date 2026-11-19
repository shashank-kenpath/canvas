//! Shapes plus selection and undo history.
//!
//! History stores per-shape changes rather than whole-board snapshots, so an
//! undo step costs the same on a full board as on an empty one.

use crate::geom::{Pt, Rect};
use crate::mindmap;
use crate::shape::{Shape, Tool};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

struct Change {
    id: String,
    before: Option<Arc<Shape>>,
    after: Option<Arc<Shape>>,
    /// Stacking position when added or removed, so undo and redo put the shape
    /// back in the same layer.
    index: usize,
}

/// What one edit touched: used by the renderer to repaint only those areas.
#[derive(Default, Debug)]
pub struct Applied {
    /// Bounds of every shape version that appeared or disappeared.
    pub areas: Vec<(Rect, f64)>,
    /// Set when the only change was new shapes placed on top of everything.
    pub appended: Vec<Arc<Shape>>,
    pub changed: bool,
}

#[derive(Default)]
pub struct Document {
    shapes: Vec<Arc<Shape>>,
    past: Vec<Vec<Change>>,
    future: Vec<Vec<Change>>,
    selected: Vec<String>,
    /// Incremented whenever the shape list changes.
    pub revision: u64,
}

type Changes = Vec<(String, Option<Arc<Shape>>)>;

impl Document {
    pub fn new() -> Document {
        Document::default()
    }

    pub fn shapes(&self) -> &[Arc<Shape>] {
        &self.shapes
    }
    pub fn get(&self, id: &str) -> Option<&Arc<Shape>> {
        self.shapes.iter().find(|s| s.id == id)
    }
    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
    pub fn selected_ids(&self) -> &[String] {
        &self.selected
    }
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.iter().any(|s| s == id)
    }
    /// Selected shapes in drawing order.
    pub fn selected_shapes(&self) -> Vec<Arc<Shape>> {
        if self.selected.is_empty() {
            return Vec::new();
        }
        self.shapes.iter().filter(|s| self.is_selected(&s.id)).cloned().collect()
    }

    fn apply(&mut self, changes: Changes, positions: &HashMap<String, usize>) -> (Vec<Change>, Applied) {
        let mut applied = Vec::new();
        let mut report = Applied::default();
        let mut next = Vec::with_capacity(self.shapes.len() + changes.len());
        let mut seen = HashSet::new();
        let lookup: HashMap<&str, &Option<Arc<Shape>>> =
            changes.iter().map(|(id, s)| (id.as_str(), s)).collect();
        let mut only_appends = true;
        for (i, shape) in self.shapes.iter().enumerate() {
            match lookup.get(shape.id.as_str()) {
                Some(after) => {
                    seen.insert(shape.id.clone());
                    if let Some(a) = after {
                        if Arc::ptr_eq(a, shape) {
                            next.push(shape.clone());
                            continue;
                        }
                    }
                    only_appends = false;
                    report.areas.push((shape.bounds(), shape.width));
                    if let Some(a) = after {
                        report.areas.push((a.bounds(), a.width));
                        next.push(a.clone());
                    }
                    applied.push(Change {
                        id: shape.id.clone(),
                        before: Some(shape.clone()),
                        after: (*after).clone(),
                        index: i,
                    });
                }
                None => next.push(shape.clone()),
            }
        }
        let added: Vec<(&String, &Arc<Shape>)> = changes
            .iter()
            .filter(|(id, s)| !seen.contains(id) && s.is_some())
            .map(|(id, s)| (id, s.as_ref().unwrap()))
            .collect();
        // Restored shapes return to their old layer, lowest first so later
        // positions stay valid; brand new shapes go on top.
        let mut restored: Vec<&(&String, &Arc<Shape>)> =
            added.iter().filter(|(id, _)| positions.contains_key(*id)).collect();
        restored.sort_by_key(|(id, _)| positions[*id]);
        for (id, shape) in restored {
            let index = positions[*id].min(next.len());
            if index != next.len() {
                only_appends = false;
            }
            next.insert(index, (*shape).clone());
            report.areas.push((shape.bounds(), shape.width));
            report.appended.push((*shape).clone());
            applied.push(Change { id: (*id).clone(), before: None, after: Some((*shape).clone()), index });
        }
        for (id, shape) in &added {
            if positions.contains_key(*id) {
                continue;
            }
            applied.push(Change {
                id: (*id).clone(),
                before: None,
                after: Some((*shape).clone()),
                index: next.len(),
            });
            report.areas.push((shape.bounds(), shape.width));
            report.appended.push((*shape).clone());
            next.push((*shape).clone());
        }
        if applied.is_empty() {
            return (applied, Applied::default());
        }
        let changed_nodes: HashSet<_> = applied.iter()
            .filter(|c| c.before.iter().chain(&c.after).any(|s| s.tool == Tool::MindNode))
            .map(|c| c.id.as_str())
            .collect();
        if !changed_nodes.is_empty() {
            for (parent, child) in mindmap::connections(&self.shapes).chain(mindmap::connections(&next)) {
                if changed_nodes.contains(parent.id.as_str()) || changed_nodes.contains(child.id.as_str()) {
                    let area = mindmap::connection_bounds(parent.bounds(), child.bounds());
                    report.areas.push((area, 3.0));
                    only_appends = false;
                }
            }
        }
        if !only_appends {
            report.appended.clear();
        }
        report.changed = true;
        self.shapes = next;
        self.revision += 1;
        self.prune_selection();
        (applied, report)
    }

    fn prune_selection(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let ids: HashSet<&str> = self.shapes.iter().map(|s| s.id.as_str()).collect();
        self.selected.retain(|id| ids.contains(id.as_str()));
    }

    fn commit(&mut self, changes: Changes) -> Applied {
        let (applied, report) = self.apply(changes, &HashMap::new());
        if applied.is_empty() {
            return report;
        }
        self.past.push(applied);
        if self.past.len() > 100 {
            self.past.remove(0);
        }
        self.future.clear();
        report
    }

    pub fn add(&mut self, shape: Shape) -> Applied {
        if self.contains(&shape.id) {
            return Applied::default();
        }
        self.commit(vec![(shape.id.clone(), Some(Arc::new(shape)))])
    }

    pub fn replace(&mut self, shape: Shape) -> Applied {
        self.replace_many(vec![shape])
    }

    /// Replaces several existing shapes as one undo step.
    pub fn replace_many(&mut self, shapes: Vec<Shape>) -> Applied {
        let changes = shapes
            .into_iter()
            .filter(|s| self.contains(&s.id))
            .map(|s| (s.id.clone(), Some(Arc::new(s))))
            .collect();
        self.commit(changes)
    }

    pub fn remove_many<I: IntoIterator<Item = String>>(&mut self, ids: I) -> Applied {
        let ids: HashSet<_> = ids.into_iter().filter(|id| self.contains(id)).collect();
        let ids = mindmap::subtree_ids(&self.shapes, ids);
        let changes = ids.into_iter().map(|id| (id, None)).collect();
        self.commit(changes)
    }

    /// Removes `remove` and adds `add` as one undo step.
    pub fn swap(&mut self, remove: Vec<String>, add: Vec<Shape>) -> Applied {
        let mut changes: Changes =
            remove.into_iter().filter(|id| self.contains(id)).map(|id| (id, None)).collect();
        changes.extend(add.into_iter().map(|s| (s.id.clone(), Some(Arc::new(s)))));
        self.commit(changes)
    }

    /// Removes `remove` and adds `add` for a change made by someone else.
    /// It is not an undo step, and this person's own undo history forgets
    /// the touched shapes so undoing never brings back an overwritten version.
    pub fn apply_remote(&mut self, remove: Vec<String>, add: Vec<Shape>) -> Applied {
        let mut changes: Changes =
            remove.into_iter().filter(|id| self.contains(id)).map(|id| (id, None)).collect();
        changes.extend(add.into_iter().map(|s| (s.id.clone(), Some(Arc::new(s)))));
        let (applied, report) = self.apply(changes, &HashMap::new());
        let touched: HashSet<&str> = applied.iter().map(|c| c.id.as_str()).collect();
        if !touched.is_empty() {
            for stack in [&mut self.past, &mut self.future] {
                for step in stack.iter_mut() {
                    step.retain(|c| !touched.contains(c.id.as_str()));
                }
                stack.retain(|step| !step.is_empty());
            }
        }
        report
    }

    /// Returns true when the selection changed.
    pub fn select_many<I: IntoIterator<Item = String>>(&mut self, ids: I) -> bool {
        let mut next: Vec<String> = Vec::new();
        for id in ids {
            if self.get(&id).is_some_and(|shape| !shape.page_background) && !next.contains(&id) {
                next.push(id);
            }
        }
        if next.len() == self.selected.len() && next.iter().all(|id| self.is_selected(id)) {
            return false;
        }
        self.selected = next;
        true
    }

    pub fn select(&mut self, id: Option<&str>) -> bool {
        self.select_many(id.map(str::to_string))
    }

    pub fn undo(&mut self) -> Applied {
        let Some(entry) = self.past.pop() else {
            return Applied::default();
        };
        let changes = entry.iter().map(|c| (c.id.clone(), c.before.clone())).collect();
        let positions = entry.iter().map(|c| (c.id.clone(), c.index)).collect();
        let (_, report) = self.apply(changes, &positions);
        self.future.push(entry);
        report
    }

    pub fn redo(&mut self) -> Applied {
        let Some(entry) = self.future.pop() else {
            return Applied::default();
        };
        let changes = entry.iter().map(|c| (c.id.clone(), c.after.clone())).collect();
        let positions = entry.iter().map(|c| (c.id.clone(), c.index)).collect();
        let (_, report) = self.apply(changes, &positions);
        self.past.push(entry);
        report
    }

    pub fn load(&mut self, shapes: Vec<Shape>) -> Result<(), String> {
        let ids: HashSet<&str> = shapes.iter().map(|s| s.id.as_str()).collect();
        if ids.len() != shapes.len() {
            return Err("Duplicate shape IDs.".into());
        }
        self.shapes = shapes.into_iter().map(Arc::new).collect();
        self.revision += 1;
        self.past.clear();
        self.future.clear();
        self.selected.clear();
        Ok(())
    }

    /// Shapes whose bounds touch `area`, in drawing order.
    pub fn shapes_in(&self, area: &Rect) -> Vec<Arc<Shape>> {
        self.shapes.iter().filter(|s| s.bounds().inflate(0.5).overlaps(area)).cloned().collect()
    }

    /// Topmost shape under `point`.
    pub fn hit_test(&self, point: Pt, tolerance: f64) -> Option<Arc<Shape>> {
        self.shapes.iter().rev().find(|s| s.hits(point, tolerance)).cloned()
    }

    pub fn bounds(&self) -> Option<Rect> {
        scene_bounds(&self.shapes)
    }

    pub fn encode(&self) -> String {
        encode_shapes(&self.shapes).to_string()
    }

    pub fn decode(source: &str) -> Result<Vec<Shape>, String> {
        let v: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
        decode_shapes(&v)
    }
}

pub fn scene_bounds(shapes: &[Arc<Shape>]) -> Option<Rect> {
    let mut all: Option<Rect> = None;
    for s in shapes {
        all = Some(match all {
            None => s.bounds(),
            Some(r) => r.union(&s.bounds()),
        });
    }
    all
}

pub fn encode_shapes(shapes: &[Arc<Shape>]) -> Value {
    json!({
        "version": 1,
        "shapes": shapes.iter().map(|s| s.to_json()).collect::<Vec<_>>(),
    })
}

/// Reads `{"version": 1, "shapes": [...]}`; a saved board file has the same keys.
pub fn decode_shapes(v: &Value) -> Result<Vec<Shape>, String> {
    if v.get("version").and_then(Value::as_i64) != Some(1) {
        return Err("Unsupported board version.".into());
    }
    let list = v.get("shapes").and_then(Value::as_array).ok_or("Board has no shapes list.")?;
    let shapes = list.iter().map(Shape::from_json).collect::<Result<Vec<_>, _>>()?;
    let ids: HashSet<&str> = shapes.iter().map(|s| s.id.as_str()).collect();
    if ids.len() != shapes.len() {
        return Err("Duplicate shape IDs.".into());
    }
    Ok(shapes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::Tool;

    #[test]
    fn filled_star_hits_its_interior_and_misses_its_notches() {
        let mut doc = Document::new();
        doc.add(
            Shape::new("star", Tool::Star, vec![Pt::ZERO, Pt::new(100.0, 100.0)])
                .styled(0xff123456, 3.0, true),
        );

        assert_eq!(doc.hit_test(Pt::new(50.0, 50.0), 0.0).unwrap().id, "star");
        assert!(doc.hit_test(Pt::new(50.0, 94.0), 0.0).is_none());
        assert!(doc.hit_test(Pt::new(110.0, 50.0), 0.0).is_none());
    }

    #[test]
    fn outline_triangle_hits_only_near_its_edges() {
        let mut doc = Document::new();
        doc.add(Shape::new("triangle", Tool::Triangle, vec![Pt::ZERO, Pt::new(100.0, 100.0)]));

        assert_eq!(doc.hit_test(Pt::new(24.0, 50.0), 0.0).unwrap().id, "triangle");
        assert!(doc.hit_test(Pt::new(50.0, 50.0), 0.0).is_none());
        assert!(doc.hit_test(Pt::new(5.0, 5.0), 0.0).is_none());
    }
}
