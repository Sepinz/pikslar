//! Dragging a block of cels in the timeline: one model for moving, copying and linking, landing
//! over what is there or in new frames. A drag never copies a pixel; cels share their drawings.

use crate::document::{Document, FrameInfo, LimitError, MAX_FRAMES};
use crate::layer::{Cel, LayerKind};

/// What happens to the cels a drag carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    /// The cels leave empty cels behind. A held cel stays in its hold on its own layer.
    Move,
    /// The cels stay, and each landing cel is a cel of its own showing the same drawing.
    Copy,
    /// The cels stay, and each landing cel joins its source's link, on any paint layer of the
    /// same color mode.
    Link,
}

/// Where the carried cels land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// Over the cels already there. The frame count does not change.
    Overwrite,
    /// In as many new frames as the block is wide, put in before the target frame.
    Insert,
}

/// A block of cels and where its first layer and frame go. Layers count from the bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CelDrag {
    /// The first and last layer of the block.
    pub layers: (usize, usize),
    /// The first and last frame of the block.
    pub frames: (usize, usize),
    /// Where the block's first layer lands.
    pub to_layer: usize,
    /// Where the block's first frame lands. With [`Landing::Insert`], the new frames' position.
    pub to_frame: usize,
    pub kind: DragKind,
    pub landing: Landing,
}

impl CelDrag {
    fn height(&self) -> usize {
        self.layers.1 - self.layers.0 + 1
    }

    fn width(&self) -> usize {
        self.frames.1 - self.frames.0 + 1
    }

    /// True when nothing would change: the block lands on itself.
    pub fn stays(&self) -> bool {
        self.landing == Landing::Overwrite
            && (self.to_layer, self.to_frame) == (self.layers.0, self.frames.0)
    }

    /// Where a source frame is once the new frames of an insert are in.
    fn shifted(&self, frame: usize) -> usize {
        match self.landing {
            Landing::Insert if frame >= self.to_frame => frame + self.width(),
            _ => frame,
        }
    }
}

impl Document {
    /// Whether this drag could be made, without making it.
    pub fn check_cel_drag(&self, drag: &CelDrag) -> Result<(), LimitError> {
        self.check_drag_reach(drag)?;
        let rows = (0..drag.height()).map(|row| (drag.layers.0 + row, drag.to_layer + row));
        let alike = rows
            .clone()
            .all(|(from, to)| self.layers[from].mode == self.layers[to].mode);
        // Cells name tiles of one tileset, and are not pixels, so they land only where the same
        // tiles are named, and a tilemap layer's cels never land on any other kind of layer.
        let same_tilesets = rows
            .clone()
            .all(|(from, to)| self.layers[from].tileset() == self.layers[to].tileset());
        if drag.kind == DragKind::Link && drag.to_layer != drag.layers.0 {
            let shares = |index: usize| {
                matches!(
                    self.layers[index].kind,
                    LayerKind::Paint | LayerKind::Tilemap(_)
                )
            };
            if !rows.clone().all(|(from, to)| shares(from) && shares(to)) {
                return Err(LimitError::LinkKind);
            }
            if !alike {
                return Err(LimitError::LinkModes);
            }
            if !same_tilesets {
                return Err(LimitError::LinkTileset);
            }
        }
        if !alike {
            return Err(LimitError::CelPlacement);
        }
        if !same_tilesets {
            return Err(LimitError::TilemapCels);
        }
        Ok(())
    }

    /// Refuses a drag from or to layers and frames the document does not have, or that would
    /// add frames past [`MAX_FRAMES`].
    fn check_drag_reach(&self, drag: &CelDrag) -> Result<(), LimitError> {
        let (layers, frames) = (self.layers.len(), self.frames.len());
        // Each test only once the ones before it hold, so a block given back to front is
        // refused before its width is worked out.
        let fits = drag.layers.0 <= drag.layers.1
            && drag.frames.0 <= drag.frames.1
            && drag.layers.1 < layers
            && drag.frames.1 < frames
            && drag.to_layer.saturating_add(drag.height()) <= layers
            && match drag.landing {
                Landing::Overwrite => drag.to_frame.saturating_add(drag.width()) <= frames,
                Landing::Insert => drag.to_frame <= frames,
            };
        if !fits {
            return Err(LimitError::NoSuchItem);
        }
        if drag.landing == Landing::Insert && frames + drag.width() > MAX_FRAMES {
            return Err(LimitError::TooManyFrames);
        }
        Ok(())
    }

    /// Makes a drag. Everything that can refuse it is checked before anything changes; an
    /// editor step undoes the rest should a later part still fail.
    pub fn drag_cels(&mut self, drag: &CelDrag) -> Result<(), LimitError> {
        self.check_cel_drag(drag)?;
        if drag.stays() {
            return Ok(());
        }
        let carried = self.carried(drag);
        if drag.landing == Landing::Insert {
            self.insert_blank_frames(drag);
        }
        if drag.kind == DragKind::Move {
            self.empty_sources(drag)?;
        }
        let mut fresh = self.new_link_id();
        for (row, cels) in carried.into_iter().enumerate() {
            let (source, target) = (drag.layers.0 + row, drag.to_layer + row);
            for (column, cel) in cels.into_iter().enumerate() {
                let frame = drag.to_frame + column;
                let link = match drag.kind {
                    DragKind::Move if source == target => cel.link(),
                    DragKind::Move | DragKind::Copy => 0,
                    DragKind::Link => {
                        let from = drag.shifted(drag.frames.0 + column);
                        self.join_hold(source, from, &cel, &mut fresh)
                    }
                };
                let layer = &mut self.layers[target];
                layer.put_cel(frame, cel)?;
                layer.set_link(frame, link);
            }
        }
        self.tidy_holds();
        self.hold_references()
    }

    /// The block's cels, row by row, before anything moves.
    fn carried(&self, drag: &CelDrag) -> Vec<Vec<Cel>> {
        (drag.layers.0..=drag.layers.1)
            .map(|layer| {
                let cels = self.layers[layer].cels();
                cels[drag.frames.0..=drag.frames.1].to_vec()
            })
            .collect()
    }

    /// Puts in the new frames of an insert, empty on every layer. Each takes the timing of the
    /// frame that will land in it.
    fn insert_blank_frames(&mut self, drag: &CelDrag) {
        let timing: Vec<FrameInfo> = self.frames[drag.frames.0..=drag.frames.1].to_vec();
        for (column, info) in timing.into_iter().enumerate() {
            let at = drag.to_frame + column;
            for layer in &mut self.layers {
                layer.insert_frame(at, Cel::blank(), 0);
            }
            let info = self.new_frame_like(info);
            self.frames.insert(at, info);
            self.tags_after_insert(at);
        }
    }

    /// Leaves empty cels where a move took its cels from.
    fn empty_sources(&mut self, drag: &CelDrag) -> Result<(), LimitError> {
        for layer in drag.layers.0..=drag.layers.1 {
            for frame in drag.frames.0..=drag.frames.1 {
                let at = drag.shifted(frame);
                let l = &mut self.layers[layer];
                l.put_cel(at, Cel::blank())?;
                l.set_link(at, 0);
            }
        }
        Ok(())
    }

    /// The link a linked copy of `cel` joins: its source's, or a new one the source joins too.
    fn join_hold(&mut self, layer: usize, from: usize, cel: &Cel, fresh: &mut u32) -> u32 {
        if cel.link() != 0 {
            return cel.link();
        }
        let id = *fresh;
        *fresh += 1;
        let l = &mut self.layers[layer];
        // The source may already be gone under an earlier landing cel; then it holds nothing.
        if l.cel(from).same_image(cel) && l.link(from) == 0 {
            l.set_link(from, id);
        }
        id
    }
}
