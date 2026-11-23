//! The shape of the outline the pointer carries: the round brush, a preset, or a custom brush,
//! as the cells one stamp paints. Built when the brush or its size changes, not per move.

use crate::app::App;
use pikslar_core::custom_brush::BrushChoice;
use pikslar_core::{brush_origin, brush_outline, Edge, Tool};

/// An outline relative to the stroke point, and the box that holds it.
pub(crate) struct Shape {
    choice: BrushChoice,
    size: i32,
    /// Changes whenever the shape does, so a moved or turned brush is seen as a change.
    pub generation: u64,
    pub outline: Vec<Edge>,
    /// Left, top, width and height of the cells, relative to the stroke point.
    pub bounds: (i32, i32, i32, i32),
}

impl App {
    /// Makes sure the cached shape is the one for the brush in use.
    pub(crate) fn refresh_shape(&self) {
        let choice = self.editor.brushes().active();
        let size = self.editor.brush_size;
        let mut cache = self.footprint.shape.borrow_mut();
        if cache
            .as_ref()
            .is_some_and(|s| s.size == size && s.choice == *choice)
        {
            return;
        }
        let generation = cache.as_ref().map_or(1, |s| s.generation + 1);
        let (outline, bounds) = match self.editor.brush_footprint(Tool::Pencil) {
            Some(stamp) => {
                let (x, y) = stamp.offset();
                let (w, h) = stamp.size();
                (stamp.mask().outline(), (x, y, w, h))
            }
            None => round_shape(size),
        };
        *cache = Some(Shape {
            choice: choice.clone(),
            size,
            generation,
            outline,
            bounds,
        });
    }

    /// The cached shape's box and generation, after refreshing it.
    pub(crate) fn shape_bounds(&self) -> ((i32, i32, i32, i32), u64) {
        self.refresh_shape();
        let cache = self.footprint.shape.borrow();
        cache
            .as_ref()
            .map_or(((0, 0, 1, 1), 0), |s| (s.bounds, s.generation))
    }

    /// The outline of one stamp with its stroke point at the document cell `(x, y)`.
    pub(crate) fn outline_at(&self, x: i32, y: i32) -> Vec<Edge> {
        self.refresh_shape();
        let cache = self.footprint.shape.borrow();
        cache
            .iter()
            .flat_map(|s| s.outline.iter())
            .map(|e| Edge {
                x: e.x + x,
                y: e.y + y,
                ..*e
            })
            .collect()
    }
}

/// The round brush: its outline starts at its top-left cell, which is `brush_origin` away.
fn round_shape(size: i32) -> (Vec<Edge>, (i32, i32, i32, i32)) {
    let (ox, oy) = brush_origin(0, 0, size);
    let outline = brush_outline(size)
        .into_iter()
        .map(|e| Edge {
            x: e.x + ox,
            y: e.y + oy,
            ..e
        })
        .collect();
    (outline, (ox, oy, size, size))
}
