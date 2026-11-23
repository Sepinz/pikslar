//! Pixels of one cel, through the editor's own tools with a one-pixel round brush.

use super::Script;
use crate::{input, ApiError, FrameRef, LayerRef, Op, Reply};
use pikslar_core::{DrawCmd, Tool};

impl Script<'_> {
    pub(super) fn pixel(
        &mut self,
        layer: LayerRef,
        frame: FrameRef,
        x: i64,
        y: i64,
    ) -> Result<Reply, ApiError> {
        self.choose_cel(layer, frame)?;
        let (x, y) = input::pixel(self.doc(), x, y)?;
        Ok(Reply::Color(self.editor.color_at(x, y)))
    }

    pub(super) fn draw(&mut self, op: Op) -> Result<Reply, ApiError> {
        let (layer, frame, tool, path, color) = match op {
            Op::SetPixel {
                layer,
                frame,
                x,
                y,
                color,
            } => (layer, frame, Tool::Pencil, vec![(x, y)], Some(color)),
            Op::ErasePixel { layer, frame, x, y } => {
                (layer, frame, Tool::Eraser, vec![(x, y)], None)
            }
            Op::DrawLine {
                layer,
                frame,
                from,
                to,
                color,
            } => (layer, frame, Tool::Line, vec![from, to], Some(color)),
            Op::Fill {
                layer,
                frame,
                x,
                y,
                color,
            } => (layer, frame, Tool::Fill, vec![(x, y)], Some(color)),
            other => return self.call(other),
        };
        self.choose_cel(layer, frame)?;
        let path = path
            .into_iter()
            .map(|(x, y)| input::pixel(self.doc(), x, y))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(color) = color {
            self.editor.color = input::color(color)?;
        }
        self.stroke(tool, path)?;
        Ok(Reply::Done)
    }

    /// One stroke of a tool, the brush and its options as a fresh editor has them.
    fn stroke(&mut self, tool: Tool, path: Vec<(i32, i32)>) -> Result<(), ApiError> {
        let e = &mut *self.editor;
        (e.brush_size, e.pixel_perfect, e.rhythm) = (1, false, false);
        match (tool, path.first()) {
            (Tool::Fill, Some(&(x, y))) => self.apply(DrawCmd::Begin { tool, x, y })?,
            _ => self.apply(DrawCmd::Stroke { tool, path })?,
        };
        Ok(())
    }
}
