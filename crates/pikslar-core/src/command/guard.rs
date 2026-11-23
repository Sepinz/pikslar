//! The one check of whether a layer may be drawn on, asked by every command that would change
//! a layer's pixels or where they sit. Commands from the app and from scripts are refused alike,
//! before anything is changed.

use super::{
    CelCmd, Command, DrawCmd, FloatingCmd, FrameCmd, LayerCmd, LayerId, PaletteCmd, TerrainCmd,
    TileCmd, TileTool,
};
use crate::cel_drag::DragKind;
use crate::document::LimitError;
use crate::editor::{Editor, Tool};
use crate::layer::LayerKind;

impl Command {
    /// False for a command the paint check can never refuse, whatever the document: it names no
    /// layer to draw on and no drawing to change, so `refuse_painting` would pass it unread.
    /// Every kind is named, so a new one has to be decided here.
    #[inline]
    pub(super) fn may_paint(&self) -> bool {
        match self {
            Command::Draw(command) => match command {
                DrawCmd::Begin { .. }
                | DrawCmd::Stroke { .. }
                | DrawCmd::EraseCell { .. }
                | DrawCmd::DeleteSelected => true,
                DrawCmd::To { .. } | DrawCmd::Commit | DrawCmd::Cancel => false,
            },
            Command::Floating(command) => match command {
                FloatingCmd::Lift { .. }
                | FloatingCmd::Paste { .. }
                | FloatingCmd::ReplacePasted { .. }
                | FloatingCmd::FlipHorizontal
                | FloatingCmd::FlipVertical
                | FloatingCmd::Rotate { .. } => true,
                FloatingCmd::Move { .. }
                | FloatingCmd::Scale { .. }
                | FloatingCmd::Anchor
                | FloatingCmd::Revert
                | FloatingCmd::GestureBegin
                | FloatingCmd::GestureEnd
                | FloatingCmd::GestureCancel => false,
            },
            Command::Cel(
                CelCmd::Move { .. }
                | CelCmd::MoveOne { .. }
                | CelCmd::Turn { .. }
                | CelCmd::Drag(_)
                | CelCmd::Tween { .. },
            ) => true,
            Command::Frame(command) => frame_may_paint(command),
            Command::Layer(command) => layer_may_paint(command),
            Command::Palette(command) => palette_may_paint(command),
            // Neither moves nor draws a pixel: a resize keeps every pixel where it is in its cel,
            // and blending changes how layers combine, not what any layer holds.
            Command::Canvas(
                super::CanvasCmd::Resize { .. } | super::CanvasCmd::SetBlending { .. },
            )
            | Command::Tag(_)
            | Command::Slice(_)
            | Command::Step(_)
            | Command::Selection(_) => false,
            Command::Tile(command) => tile_may_paint(command),
            Command::Terrain(command) => terrain_may_paint(command),
        }
    }
}

/// Palette commands change colours and the order and grouping of swatches, and the remaps change
/// ids without changing what any pixel shows, so none of those can be refused for a lock: a
/// layer left on the old ids would be wrong. Load changes colours and the swatch list only, as Recolor
/// does, so it is not refused either. Replace colour changes what pixels show, so it is
/// checked like painting (`replace_refusal` in `palette_remap.rs`). Each is named, so a later
/// one has to be decided here.
#[inline]
fn palette_may_paint(command: &PaletteCmd) -> bool {
    match command {
        PaletteCmd::ReplaceColor { .. } => true,
        PaletteCmd::AddSwatch { .. }
        | PaletteCmd::RemoveSwatch { .. }
        | PaletteCmd::Recolor { .. }
        | PaletteCmd::MoveSwatch { .. }
        | PaletteCmd::Sort { .. }
        | PaletteCmd::AddGroup { .. }
        | PaletteCmd::RenameGroup { .. }
        | PaletteCmd::RemoveGroup { .. }
        | PaletteCmd::AddToGroup { .. }
        | PaletteCmd::RemoveFromGroup { .. }
        | PaletteCmd::AddRamp { .. }
        | PaletteCmd::Load { .. }
        | PaletteCmd::Remap(_) => false,
    }
}

/// Tilesets are not layers, so no tile command that edits them draws on one. Map strokes draw on
/// the active tilemap layer, so locks apply to them, though a pick only reads. Terrain strokes,
/// Alt-click's retile among them, are map strokes (`TileTool::Terrain`) and paint like a placement.
#[inline]
fn tile_may_paint(command: &TileCmd) -> bool {
    match command {
        TileCmd::Begin { .. } | TileCmd::Stroke { .. } | TileCmd::DeleteSelected => true,
        TileCmd::To { .. } | TileCmd::Commit | TileCmd::Cancel => false,
        TileCmd::NewTileset { .. }
        | TileCmd::RenameTileset { .. }
        | TileCmd::RemoveTileset { .. }
        | TileCmd::AddTile { .. }
        | TileCmd::DuplicateTile { .. }
        | TileCmd::RemoveTile { .. }
        | TileCmd::NewLayer { .. }
        | TileCmd::NewMapLayer { .. }
        | TileCmd::CopyTilesetFor { .. }
        | TileCmd::MoveTile { .. }
        | TileCmd::SetTileImage { .. } => false,
    }
}

/// Terrain sets belong to a tileset, not a layer, so no terrain command draws on one. A template
/// adds blank tiles, which no map places yet.
#[inline]
fn terrain_may_paint(command: &TerrainCmd) -> bool {
    match command {
        TerrainCmd::New { .. }
        | TerrainCmd::Rename { .. }
        | TerrainCmd::Remove { .. }
        | TerrainCmd::Assign { .. }
        | TerrainCmd::Clear { .. } => false,
    }
}

#[inline]
fn frame_may_paint(command: &FrameCmd) -> bool {
    match command {
        FrameCmd::HoldPrevious { .. } | FrameCmd::HoldAcross { .. } | FrameCmd::Release { .. } => {
            true
        }
        FrameCmd::Add { .. }
        | FrameCmd::Remove { .. }
        | FrameCmd::Move { .. }
        | FrameCmd::Retime { .. }
        | FrameCmd::Duplicate { .. }
        | FrameCmd::MoveRun { .. }
        | FrameCmd::SetRate { .. } => false,
    }
}

#[inline]
fn layer_may_paint(command: &LayerCmd) -> bool {
    match command {
        LayerCmd::SetMode { .. } | LayerCmd::MergeDown { .. } => true,
        LayerCmd::Add { .. }
        | LayerCmd::Remove { .. }
        | LayerCmd::Move { .. }
        | LayerCmd::SetOpacity { .. }
        | LayerCmd::SetVisible { .. }
        | LayerCmd::SetLocked { .. }
        | LayerCmd::SetBlend { .. }
        | LayerCmd::Rename { .. }
        | LayerCmd::Duplicate { .. }
        | LayerCmd::Group { .. }
        | LayerCmd::Ungroup { .. }
        | LayerCmd::Step { .. }
        | LayerCmd::SetFolded { .. }
        | LayerCmd::PlaceReference { .. } => false,
    }
}

impl Editor {
    /// Why the layer at `index` cannot be drawn on, or `None` if it can.
    pub fn paint_refusal(&self, index: usize) -> Option<LimitError> {
        let layer = self.doc.layers.get(index)?;
        match layer.kind {
            _ if self.doc.locked_through(index) => Some(LimitError::Locked),
            LayerKind::Paint => None,
            LayerKind::Reference => Some(LimitError::Reference),
            LayerKind::Group => Some(LimitError::Group),
            LayerKind::Tilemap(_) => Some(LimitError::Tilemap),
        }
    }

    /// Why the cels of the layer at `index` cannot be given other cels or holds: as
    /// [`Self::paint_refusal`], except that a tilemap layer's cels are cels like any other, so
    /// only a lock refuses one. These commands move or share whole cels and never read a cell
    /// as a pixel; what a tilemap layer may not do with them, its own rules refuse.
    fn cels_refusal(&self, index: usize) -> Option<LimitError> {
        match self.doc.layers.get(index)?.kind {
            LayerKind::Tilemap(_) => self.move_refusal(index),
            LayerKind::Paint | LayerKind::Reference | LayerKind::Group => self.paint_refusal(index),
        }
    }

    /// Why a pencil, eraser, line or rectangle cannot draw on the layer at `index`: as
    /// [`Self::paint_refusal`], except that on a tilemap layer they edit the tiles its cells
    /// place, so only a lock refuses. Other layers placing those tiles are not asked, as for
    /// any edit of a tile's pixels.
    fn through_refusal(&self, index: usize) -> Option<LimitError> {
        match self.doc.layers.get(index)?.kind {
            LayerKind::Tilemap(_) => self.move_refusal(index),
            LayerKind::Paint | LayerKind::Reference | LayerKind::Group => self.paint_refusal(index),
        }
    }

    /// Why the cels of the layer at `index` cannot be moved: a reference layer is moved like
    /// any other, so only a lock refuses.
    fn move_refusal(&self, index: usize) -> Option<LimitError> {
        self.doc.layers.get(index)?;
        self.doc.locked_through(index).then_some(LimitError::Locked)
    }

    /// Refuses a command that would draw on a layer that cannot be drawn on.
    pub(super) fn refuse_painting(&self, command: &Command) -> Result<(), LimitError> {
        if let Command::Cel(CelCmd::Drag(drag)) = command {
            // A link across layers names its own refusals before a reference or group would.
            if links_across(drag) {
                self.doc.check_cel_drag(drag)?;
            }
        }
        let check = match command {
            // In Pixels mode these edit the tiles a map places, as a paint layer's pixels.
            Command::Draw(DrawCmd::Begin { tool, .. } | DrawCmd::Stroke { tool, .. })
                if edits_tiles(*tool) =>
            {
                Self::through_refusal
            }
            // A map is drawn on through its cells, so only a lock refuses.
            Command::Cel(CelCmd::Move { .. } | CelCmd::MoveOne { .. }) | Command::Tile(_) => {
                Self::move_refusal
            }
            Command::Cel(CelCmd::Drag(_) | CelCmd::Tween { .. })
            | Command::Frame(FrameCmd::HoldPrevious { .. } | FrameCmd::HoldAcross { .. })
            | Command::Layer(LayerCmd::SetMode { .. } | LayerCmd::MergeDown { .. }) => {
                Self::cels_refusal
            }
            _ => Self::paint_refusal,
        };
        // Rows past the list name no layer, so a run is walked only as far as the top.
        let top = self.doc.layers.len().saturating_sub(1);
        let refusal = self
            .painted_layers(command)?
            .into_iter()
            .flat_map(|(from, to)| from..=to.min(top))
            .find_map(|index| check(self, index));
        refusal.map_or(Ok(()), Err)?;
        // The cel's own layer refuses first; then a layer it shares its drawing with.
        self.refuse_shared_lock(command)
    }

    /// Refuses a command that would change a drawing shared with a locked layer.
    fn refuse_shared_lock(&self, command: &Command) -> Result<(), LimitError> {
        match self.drawn_cel(command)? {
            Some((layer, frame)) => self.shared_refusal(layer, frame).map_or(Ok(()), Err),
            None => Ok(()),
        }
    }

    /// Why the drawing of this cel cannot be changed though its own layer may be drawn on: it
    /// is shared with a locked layer. `None` if nothing else refuses.
    pub fn shared_refusal(&self, layer: usize, frame: usize) -> Option<LimitError> {
        // One pass over the cels: an unlinked cel's members are itself alone.
        let members = self.doc.link_members(layer, frame);
        let locked =
            |&(other, _): &(usize, usize)| other != layer && self.doc.locked_through(other);
        members
            .iter()
            .any(locked)
            .then_some(LimitError::SharedLocked)
    }

    /// The cel whose drawing a command would change, if it changes one.
    pub(super) fn drawn_cel(
        &self,
        command: &Command,
    ) -> Result<Option<(usize, usize)>, LimitError> {
        let active = Some((self.layer, self.frame));
        Ok(match command {
            // On a map they change tiles, not the map's drawing.
            Command::Draw(DrawCmd::Begin { tool, .. } | DrawCmd::Stroke { tool, .. }) => {
                active.filter(|_| *tool != Tool::Picker && !self.on_tilemap())
            }
            Command::Draw(DrawCmd::EraseCell { .. } | DrawCmd::DeleteSelected) => active,
            Command::Tile(TileCmd::Begin { tool, .. } | TileCmd::Stroke { tool, .. }) => {
                active.filter(|_| *tool != TileTool::Pick)
            }
            Command::Tile(TileCmd::DeleteSelected) => active,
            Command::Floating(command) if lifts(command) => active,
            Command::Cel(CelCmd::Turn { layer, frame, .. }) => {
                Some((self.index_of(*layer)?, *frame))
            }
            _ => None,
        })
    }

    /// Which layers a command would draw on, as runs of rows counted from the bottom.
    pub(super) fn painted_layers(&self, command: &Command) -> Result<Rows, LimitError> {
        let active = || vec![(self.layer, self.layer)];
        let one = |layer: LayerId| -> Result<Rows, LimitError> {
            let index = self.index_of(layer)?;
            Ok(vec![(index, index)])
        };
        Ok(match command {
            Command::Draw(DrawCmd::Begin { tool, .. } | DrawCmd::Stroke { tool, .. }) => match tool
            {
                Tool::Picker => Vec::new(),
                _ => active(),
            },
            Command::Draw(DrawCmd::EraseCell { .. } | DrawCmd::DeleteSelected) => active(),
            Command::Tile(TileCmd::Begin { tool, .. } | TileCmd::Stroke { tool, .. }) => match tool
            {
                TileTool::Pick => Vec::new(),
                _ => active(),
            },
            Command::Tile(TileCmd::DeleteSelected) => active(),
            Command::Floating(command) if lifts(command) => active(),
            Command::Cel(
                CelCmd::Move { layer, .. }
                | CelCmd::MoveOne { layer, .. }
                | CelCmd::Turn { layer, .. },
            ) => one(*layer)?,
            Command::Cel(CelCmd::Drag(drag)) => dragged_rows(drag),
            Command::Cel(CelCmd::Tween { layer, .. }) => one(*layer)?,
            Command::Frame(
                FrameCmd::HoldPrevious { layer, .. } | FrameCmd::HoldAcross { layer, .. },
            ) => one(*layer)?,
            Command::Layer(LayerCmd::SetMode { layer, .. }) => one(*layer)?,
            // Releasing would leave a reference's frames apart; a paint layer's release draws
            // nothing, so a lock does not refuse it.
            Command::Frame(FrameCmd::Release { layer, .. }) => {
                let index = self.index_of(*layer)?;
                match self.doc.layers[index].kind {
                    LayerKind::Reference => vec![(index, index)],
                    LayerKind::Paint | LayerKind::Group | LayerKind::Tilemap(_) => Vec::new(),
                }
            }
            Command::Layer(LayerCmd::MergeDown { layer }) => {
                let index = self.index_of(*layer)?;
                vec![(index.saturating_sub(1), index)]
            }
            _ => Vec::new(),
        })
    }
}

/// True for a command that takes pixels from the active cel or puts them over it: a lift, a
/// paste, the swap of a paste's pixels for others, and a flip or turn of a selection, which lifts it first if nothing is floating.
fn lifts(command: &FloatingCmd) -> bool {
    matches!(
        command,
        FloatingCmd::Lift { .. }
            | FloatingCmd::Paste { .. }
            | FloatingCmd::ReplacePasted { .. }
            | FloatingCmd::FlipHorizontal
            | FloatingCmd::FlipVertical
            | FloatingCmd::Rotate { .. }
    )
}

/// True for the tools that, in Pixels mode, edit tiles through a tilemap layer. The fill and
/// the picker are not among them: on a map the fill fills cells, in Tiles mode.
fn edits_tiles(tool: Tool) -> bool {
    match tool {
        Tool::Pencil | Tool::Eraser | Tool::Line | Tool::Rect => true,
        Tool::Fill | Tool::Picker => false,
    }
}

/// Runs of layers, first and last, counted from the bottom.
type Rows = Vec<(usize, usize)>;

/// True for a drag that links cels to another layer.
fn links_across(drag: &crate::cel_drag::CelDrag) -> bool {
    drag.kind == DragKind::Link && drag.to_layer != drag.layers.0
}

/// A drag draws on the rows it lands on, and a move also on the rows it leaves.
fn dragged_rows(drag: &crate::cel_drag::CelDrag) -> Rows {
    let (from, to) = drag.layers;
    let landing = (
        drag.to_layer,
        drag.to_layer.saturating_add(to.saturating_sub(from)),
    );
    match drag.kind {
        // Drawing on a linked copy changes its source too.
        DragKind::Link if links_across(drag) => vec![(from, to), landing],
        DragKind::Move => vec![(from, to), landing],
        DragKind::Copy | DragKind::Link => vec![landing],
    }
}
