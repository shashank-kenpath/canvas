//! Mind-map topology and layout. Connectors are derived from each node's parent.

use crate::geom::{Pt, Rect};
use crate::shape::{Shape, Tool};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const NODE_SIZE: Pt = Pt { x: 192.0, y: 76.0 };
const HORIZONTAL_GAP: f64 = 72.0;
const VERTICAL_GAP: f64 = 28.0;
const ACCENT: u32 = 0xff2748e8;

pub(crate) fn root(id: String, center: Pt) -> Shape {
    node(id, center - NODE_SIZE / 2.0, None, ACCENT)
}

fn node(id: String, origin: Pt, parent: Option<String>, color: u32) -> Shape {
    let title = if parent.is_some() { "New topic" } else { "Central topic" };
    let mut shape = Shape::new(id, Tool::MindNode, vec![origin, origin + NODE_SIZE])
        .styled(color, 2.5, true)
        .with_text(title, 24.0);
    shape.parent_id = parent;
    shape
}

/// Appends a child to the right of its parent, below siblings and clear of
/// other nodes in the same horizontal band. Existing positions are retained.
pub(crate) fn child(shapes: &[Arc<Shape>], parent: &Shape, id: String) -> Shape {
    let bounds = parent.bounds();
    let x = bounds.r + HORIZONTAL_GAP;
    let mut y = bounds.center().y - NODE_SIZE.y / 2.0;
    let mut obstacles = Vec::new();
    for s in shapes.iter().filter(|s| s.tool == Tool::MindNode) {
        let b = s.bounds();
        if s.parent_id.as_deref() == Some(parent.id.as_str()) {
            y = y.max(b.b + VERTICAL_GAP);
        }
        if b.l < x + NODE_SIZE.x && b.r > x {
            obstacles.push(b);
        }
    }
    obstacles.sort_by(|a, b| a.t.total_cmp(&b.t));
    for obstacle in obstacles {
        if y + NODE_SIZE.y + VERTICAL_GAP > obstacle.t {
            y = y.max(obstacle.b + VERTICAL_GAP);
        } else {
            break;
        }
    }
    node(id, Pt::new(x, y), Some(parent.id.clone()), parent.color)
}

/// Includes each input ID and all descendants. The visited set also protects
/// traversal of boards imported with cyclic parent references.
pub(crate) fn subtree_ids(shapes: &[Arc<Shape>], mut ids: HashSet<String>) -> HashSet<String> {
    if ids.is_empty() {
        return ids;
    }
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for (parent, child) in connections(shapes) {
        children.entry(&parent.id).or_default().push(&child.id);
    }
    let mut pending: Vec<_> = ids.iter().flat_map(|id| children.get(id.as_str()).into_iter().flatten().copied()).collect();
    while let Some(id) = pending.pop() {
        if ids.insert(id.to_string()) {
            pending.extend(children.get(id).into_iter().flatten().copied());
        }
    }
    ids
}

pub(crate) fn connections(shapes: &[Arc<Shape>]) -> impl Iterator<Item = (&Shape, &Shape)> {
    let nodes: HashMap<&str, &Shape> = shapes.iter()
        .filter(|s| s.tool == Tool::MindNode)
        .map(|s| (s.id.as_str(), s.as_ref()))
        .collect();
    shapes.iter().filter_map(move |s| {
        let parent_id = s.parent_id.as_deref().filter(|id| *id != s.id)?;
        Some((*nodes.get(parent_id)?, s.as_ref()))
    })
}

pub(crate) fn endpoints(parent: Rect, child: Rect) -> (Pt, Pt) {
    if child.center().x >= parent.center().x {
        (Pt::new(parent.r, parent.center().y), Pt::new(child.l, child.center().y))
    } else {
        (Pt::new(parent.l, parent.center().y), Pt::new(child.r, child.center().y))
    }
}

pub(crate) fn connection_bounds(parent: Rect, child: Rect) -> Rect {
    let (from, to) = endpoints(parent, child);
    Rect::from_points(from, to).inflate(2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_clear_siblings_and_nearby_branches() {
        let root = Arc::new(root("root".into(), Pt::ZERO));
        let first = Arc::new(child(&[], &root, "first".into()));
        let second = Arc::new(child(&[first.clone()], &root, "second".into()));
        assert!(first.bounds().l > root.bounds().r);
        assert_eq!(first.bounds().center().y, root.bounds().center().y);
        assert!(second.bounds().t > first.bounds().b);
        let first_leaf = Arc::new(child(&[], &first, "first-leaf".into()));
        let second_leaf = child(&[first_leaf.clone()], &first, "second-leaf".into());
        let other_branch = child(&[first_leaf, Arc::new(second_leaf.clone())], &second, "other".into());
        assert!(!other_branch.bounds().overlaps(&second_leaf.bounds()));
    }

    #[test]
    fn node_json_preserves_parent_and_text() {
        let root = root("root".into(), Pt::ZERO);
        let node = child(&[], &root, "leaf".into()).with_text("A new idea", 22.0);
        let json = node.to_json();
        assert_eq!(json["tool"], "mindnode");
        assert_eq!(json["parentId"], "root");
        assert_eq!(Shape::from_json(&json).unwrap(), node);
        assert_eq!(Shape::from_json(&root.to_json()).unwrap().parent_id, None);
    }
}
