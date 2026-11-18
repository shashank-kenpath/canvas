//! Table data and cell geometry. Cell text is stored in row-major order.

use crate::geom::{Pt, Rect};
use crate::shape::{Shape, Tool};
use serde_json::{Map, Value};

pub const DEFAULT_SIZE: usize = 3;
pub const MAX_SIZE: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub row: usize,
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    rows: usize,
    columns: usize,
    cells: Vec<String>,
}

impl Default for Table {
    fn default() -> Self {
        Self::new(DEFAULT_SIZE, DEFAULT_SIZE)
    }
}

impl Table {
    pub fn new(rows: usize, columns: usize) -> Self {
        let rows = rows.clamp(1, MAX_SIZE);
        let columns = columns.clamp(1, MAX_SIZE);
        Self { rows, columns, cells: vec![String::new(); rows * columns] }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn cells(&self) -> &[String] {
        &self.cells
    }

    fn index(&self, cell: Cell) -> Option<usize> {
        (cell.row < self.rows && cell.column < self.columns)
            .then(|| cell.row * self.columns + cell.column)
    }

    pub fn text(&self, cell: Cell) -> Option<&str> {
        self.index(cell).map(|i| self.cells[i].as_str())
    }

    pub(crate) fn set_text(&mut self, cell: Cell, text: &str) -> bool {
        let Some(i) = self.index(cell) else { return false };
        if self.cells[i] == text {
            return false;
        }
        text.clone_into(&mut self.cells[i]);
        true
    }

    pub(crate) fn resized(&self, rows: usize, columns: usize) -> Self {
        let mut next = Self::new(rows, columns);
        let shared = self.columns.min(next.columns);
        for row in 0..self.rows.min(next.rows) {
            let old = row * self.columns;
            let new = row * next.columns;
            next.cells[new..new + shared].clone_from_slice(&self.cells[old..old + shared]);
        }
        next
    }

    pub fn cell_size(&self, bounds: Rect) -> Pt {
        Pt::new(bounds.width() / self.columns as f64, bounds.height() / self.rows as f64)
    }

    pub fn cell_bounds(&self, bounds: Rect, cell: Cell) -> Rect {
        let size = self.cell_size(bounds);
        let origin = bounds.top_left() + Pt::new(size.x * cell.column as f64, size.y * cell.row as f64);
        Rect::from_points(origin, origin + size)
    }

    pub fn cell_at(&self, bounds: Rect, point: Pt) -> Option<Cell> {
        if !bounds.contains(point) {
            return None;
        }
        let size = self.cell_size(bounds);
        Some(Cell {
            row: (((point.y - bounds.t) / size.y) as usize).min(self.rows - 1),
            column: (((point.x - bounds.l) / size.x) as usize).min(self.columns - 1),
        })
    }

    /// The eraser targets the grid lines so ink in an open cell can be
    /// erased on its own.
    pub(crate) fn hits_grid(&self, bounds: Rect, point: Pt, tolerance: f64) -> bool {
        if !bounds.inflate(tolerance).contains(point) {
            return false;
        }
        let size = self.cell_size(bounds);
        let distance = |offset: f64, step: f64, count: usize| {
            let nearest = (offset / step).round().clamp(0.0, count as f64) * step;
            (offset - nearest).abs()
        };
        distance(point.x - bounds.l, size.x, self.columns) <= tolerance
            || distance(point.y - bounds.t, size.y, self.rows) <= tolerance
    }

    pub(crate) fn from_json(object: &Map<String, Value>) -> Self {
        let dimension = |key: &str| {
            object.get(key).and_then(Value::as_u64)
                .map_or(DEFAULT_SIZE, |n| n.min(MAX_SIZE as u64) as usize)
        };
        let mut table = Self::new(dimension("rows"), dimension("columns"));
        if let Some(cells) = object.get("cells").and_then(Value::as_array) {
            for (cell, value) in table.cells.iter_mut().zip(cells) {
                if let Some(text) = value.as_str() {
                    text.clone_into(cell);
                }
            }
        }
        table
    }
}

pub(crate) fn centered(id: String, center: Pt, rows: usize, columns: usize) -> Shape {
    let table = Table::new(rows, columns);
    let half = Pt::new(72.0 * table.columns as f64, 36.0 * table.rows as f64);
    Shape::new(id, Tool::Table, vec![center - half, center + half])
        .styled(0xff4a5160, 2.0, false)
        .with_text("", 22.0)
        .with_table(table)
}

/// Returns the topmost cell that contains the entire handwriting group.
pub(crate) fn containing_cell<'a>(
    shapes: &'a [std::sync::Arc<Shape>],
    ink: Rect,
) -> Option<(&'a Shape, Cell)> {
    shapes.iter().rev().find_map(|shape| {
        let table = shape.table.as_deref()?;
        let bounds = shape.bounds();
        let cell = table.cell_at(bounds, ink.center())?;
        let area = table.cell_bounds(bounds, cell);
        (ink.l >= area.l && ink.r <= area.r && ink.t >= area.t && ink.b <= area.b)
            .then_some((shape.as_ref(), cell))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_hits_follow_resized_bounds() {
        let table = Table::new(2, 3);
        let bounds = Rect::ltrb(-60.0, 30.0, 240.0, 130.0);
        let middle = Cell { row: 1, column: 1 };
        assert_eq!(table.cell_at(bounds, Pt::new(40.0, 80.0)), Some(middle));
        assert_eq!(table.cell_bounds(bounds, middle), Rect::ltrb(40.0, 80.0, 140.0, 130.0));
        assert_eq!(table.cell_at(bounds, Pt::new(240.0, 100.0)), None);
    }

    #[test]
    fn grid_edits_preserve_existing_coordinates() {
        let mut table = Table::default();
        let corner = Cell { row: 2, column: 2 };
        assert!(table.set_text(corner, "corner"));
        assert_eq!(table.resized(4, 4).text(corner), Some("corner"));
        assert_eq!(table.resized(3, 2).resized(3, 3).text(corner), Some(""));
        assert_eq!(Table::new(0, usize::MAX).rows(), 1);
        assert_eq!(Table::new(0, usize::MAX).columns(), MAX_SIZE);
    }

    #[test]
    fn table_json_round_trips_and_defaults_optional_fields() {
        let cell = Cell { row: 1, column: 2 };
        let mut table = Table::new(2, 4);
        table.set_text(cell, "Weekly totals\n12");
        let shape = centered("table".into(), Pt::ZERO, 2, 4).with_table(table);
        let mut json = shape.to_json();
        assert_eq!(json["tool"], "table");
        assert_eq!(Shape::from_json(&json).unwrap(), shape);
        for field in ["rows", "columns", "cells"] {
            json.as_object_mut().unwrap().remove(field);
        }
        assert_eq!(*Shape::from_json(&json).unwrap().table.unwrap(), Table::default());
    }
}
