//! The shape of the brush, shared by what paints and what shows where it will paint.

use crate::mask::{Edge, Mask};

/// The brush's top-left cell when it is centered on a point.
pub fn brush_origin(x: i32, y: i32, size: i32) -> (i32, i32) {
    (x - (size - 1) / 2, y - (size - 1) / 2)
}

/// Whether a cell of the brush's bounding square is part of the brush.
pub fn in_brush(size: i32, dx: i32, dy: i32) -> bool {
    let (ex, ey) = (dx * 2 - (size - 1), dy * 2 - (size - 1));
    size <= 2 || ex * ex + ey * ey <= size * size
}

/// The outline of a brush whose top-left cell is at the origin.
pub fn brush_outline(size: i32) -> Vec<Edge> {
    let mut shape = Mask::blank(0, 0, size, size);
    for dy in 0..size {
        for dx in (0..size).filter(|&dx| in_brush(size, dx, dy)) {
            shape.set(dx, dy);
        }
    }
    shape.outline()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::rgba;
    use crate::document::{ColorMode, Document};
    use crate::editor::{Editor, Tool};
    use std::collections::HashSet;

    #[test]
    fn what_the_outline_shows_is_exactly_what_a_stamp_paints() {
        for size in 1..=12 {
            let mut e = Editor::new(Document::new(32, 32, ColorMode::Rgba).expect("valid"));
            e.color = rgba(1, 2, 3, 255);
            e.brush_size = size;
            let (cx, cy) = (15, 16);
            e.begin(Tool::Pencil, cx, cy).expect("begin");
            e.commit();
            let cel = &e.doc.layers[0].cel(0);
            let painted: HashSet<(i32, i32)> = (0..32)
                .flat_map(|y| (0..32).map(move |x| (x, y)))
                .filter(|&(x, y)| cel.get(x, y) != 0)
                .collect();
            let (left, top) = brush_origin(cx, cy, size);
            let shown: HashSet<(i32, i32)> = (0..size)
                .flat_map(|dy| (0..size).map(move |dx| (dx, dy)))
                .filter(|&(dx, dy)| in_brush(size, dx, dy))
                .map(|(dx, dy)| (left + dx, top + dy))
                .collect();
            assert_eq!(painted, shown, "brush size {size}");
        }
    }

    #[test]
    fn the_outline_surrounds_every_size() {
        for size in 1..=64 {
            let edges = brush_outline(size);
            assert!(!edges.is_empty(), "brush size {size}");
            assert!(edges
                .iter()
                .all(|e| { e.x >= 0 && e.y >= 0 && e.x <= size && e.y <= size }));
        }
    }
}
