//! The table of the kind audit: one case for each `Command` variant, tried with a tilemap layer
//! active. The tables are kept one case to a line so that a missing one is easy to see.

use super::fixture::{editor_with_map, id, tile_image};
use crate::cel_drag::{CelDrag, DragKind, Landing};
use crate::color::SortBy;
use crate::command::TerrainShape::{self, Brush, Fill, Retile};
use crate::command::{CanvasCmd, TerrainTool, TileSource, TileTool};
use crate::document::{BlendMode, ColorMode, LimitError};
use crate::editor::Tool;
use crate::floating::Clip;
use crate::layer::Turn;
use crate::mask::Mask;
use crate::palette_ops::remap::RemapKind;
use crate::selection::SelectOp;
use crate::slice::{Slice, SliceRect};
use crate::tween::Ease;
use crate::{
    CelCmd, Command, DrawCmd, Editor, FloatingCmd, FrameCmd, LayerCmd, LayerId, PaletteCmd,
    SelectionCmd, SliceCmd, StepCmd, TagCmd, TerrainCmd, TileCmd,
};

/// What a command does with a tilemap layer active.
#[derive(Clone, Copy, Debug)]
pub(super) enum Expect {
    /// Refused with this message, changing nothing and recording nothing.
    Refused(LimitError),
    /// Runs and changes the document.
    Works,
    /// Runs and changes nothing.
    Nothing,
    /// Not about layers: runs or is refused, and leaves the rules kept either way.
    Any,
}

use Expect::{Any, Nothing, Refused, Works};

/// A case: how to make the scene, the command, and what it should do.
pub(super) type Case = (fn(&mut Editor), Command, Expect);

/// The scene every case starts from: [`editor_with_map`] with a tag, a slice and everything
/// selected, so commands that name those have something to act on. The commands of a case are
/// made from the same editor they run on, since a layer's id belongs to one session's layer.
pub(super) fn scene() -> Editor {
    let (mut e, _) = editor_with_map();
    let tag = TagCmd::Add {
        name: "walk".into(),
        from: 0,
        to: 1,
    };
    e.apply(tag).expect("tag");
    e.apply(SliceCmd::Add {
        slice: Box::new(slice("hit")),
    })
    .expect("slice");
    e.apply(SelectionCmd::All).expect("selects");
    e
}

fn slice(name: &str) -> Slice {
    let bounds = SliceRect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    Slice {
        name: name.into(),
        color: 0xff00_00ff,
        bounds,
        pivot: None,
        center: None,
    }
}

fn plain(_: &mut Editor) {}

fn spread(e: &mut Editor) {
    let layer = e.layer_id(1).expect("layer");
    let hold = FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 0,
        to: 2,
    };
    e.apply(hold).expect("holds");
    // A tween needs keys that sit apart, so the last frame's cel is moved on its own.
    let apart = CelCmd::MoveOne {
        layer,
        frame: 2,
        dx: 6,
        dy: 0,
    };
    e.apply(apart).expect("moves");
}

/// Every case, for the scene the editor holds.
pub(super) fn cases(e: &Editor) -> Vec<Case> {
    let map = e.layer_id(1).expect("map layer");
    let mut all = canvas_and_layers(map);
    all.extend(frames_tags_slices(map));
    all.extend(palette_and_steps());
    all.extend(drawing());
    all.extend(cels(map));
    all.extend(tiles(e, map));
    all.extend(terrains(e));
    all
}

fn at(command: impl Into<Command>, expect: Expect) -> Case {
    (plain, command.into(), expect)
}

#[rustfmt::skip]
fn canvas_and_layers(map: LayerId) -> Vec<Case> {
    let rgba = ColorMode::Rgba;
    let reference = LayerCmd::PlaceReference { below: 0, image: tile_image(0xff00_00ff), name: "ref".into() };
    vec![
        at(CanvasCmd::Resize { width: 20, height: 16, dx: 2, dy: 0 }, Works),
        at(CanvasCmd::SetBlending { blending: crate::Blending::Aseprite }, Works),
        at(LayerCmd::Add { at: 0, mode: rgba }, Works),
        at(LayerCmd::Remove { layer: map }, Works),
        at(LayerCmd::Move { layer: map, to: 0 }, Works),
        at(LayerCmd::SetOpacity { layer: map, opacity: 100 }, Works),
        at(LayerCmd::SetVisible { layer: map, visible: false }, Works),
        at(LayerCmd::SetLocked { layer: map, locked: true }, Works),
        at(LayerCmd::SetBlend { layer: map, blend: BlendMode::Multiply }, Works),
        at(LayerCmd::SetMode { layer: map, mode: ColorMode::Palette }, Refused(LimitError::TilemapMode)),
        at(LayerCmd::Rename { layer: map, name: "Walls".into() }, Works),
        at(LayerCmd::Duplicate { layer: map }, Works),
        at(LayerCmd::MergeDown { layer: map }, Works),
        at(LayerCmd::Group { layer: map }, Works),
        at(LayerCmd::Ungroup { layer: map }, Refused(LimitError::NoSuchItem)),
        at(LayerCmd::Step { layer: map, up: false }, Works),
        at(LayerCmd::SetFolded { layer: map, folded: true }, Nothing),
        at(reference, Works),
    ]
}

#[rustfmt::skip]
fn frames_tags_slices(map: LayerId) -> Vec<Case> {
    vec![
        at(FrameCmd::Add { at: 1, copy: true }, Works),
        at(FrameCmd::Remove { from: 2, to: 2 }, Works),
        at(FrameCmd::Move { from: 0, to: 2 }, Works),
        at(FrameCmd::Retime { from: 0, durations: vec![50] }, Any),
        at(FrameCmd::HoldPrevious { layer: map, frame: 1 }, Works),
        at(FrameCmd::HoldAcross { layer: map, source: 0, from: 0, to: 2 }, Works),
        at(FrameCmd::Release { layer: map, from: 0, to: 2 }, Any),
        at(FrameCmd::Duplicate { from: 0, to: 1 }, Works),
        at(FrameCmd::MoveRun { from: 0, to: 0, at: 2 }, Any),
        at(FrameCmd::SetRate { fps: Some(10) }, Works),
        at(TagCmd::Add { name: "run".into(), from: 1, to: 2 }, Works),
        at(TagCmd::SetName { tag: 0, name: "idle".into() }, Works),
        at(TagCmd::Remove { tag: 0 }, Works),
        at(TagCmd::CycleDirection { tag: 0 }, Works),
        at(TagCmd::SetRepeat { tag: 0, repeat: 2 }, Works),
        at(TagCmd::SetRange { tag: 0, from: 0, to: 2 }, Works),
        at(SliceCmd::Add { slice: Box::new(slice("b")) }, Works),
        at(SliceCmd::Set { index: 0, slice: Box::new(slice("c")) }, Works),
        at(SliceCmd::Remove { index: 0 }, Works),
    ]
}

#[rustfmt::skip]
fn palette_and_steps() -> Vec<Case> {
    vec![
        at(PaletteCmd::AddSwatch { color: 0xff12_3456 }, Any),
        at(PaletteCmd::RemoveSwatch { id: 1 }, Any),
        at(PaletteCmd::Recolor { id: 1, color: 0xff00_ff00 }, Any),
        at(PaletteCmd::MoveSwatch { from: 0, to: 1 }, Any),
        at(PaletteCmd::Sort { by: SortBy::Hue }, Any),
        at(PaletteCmd::AddGroup { name: "g".into(), ids: vec![1u32].into() }, Any),
        at(PaletteCmd::RenameGroup { group: 0, name: "h".into() }, Any),
        at(PaletteCmd::RemoveGroup { group: 0 }, Any),
        at(PaletteCmd::AddToGroup { group: 0, id: 1 }, Any),
        at(PaletteCmd::RemoveFromGroup { group: 0, id: 1 }, Any),
        at(PaletteCmd::AddRamp { from: 1, to: 2, steps: 3 }, Any),
        at(PaletteCmd::Remap(RemapKind::ToSwatchOrder), Any),
        at(PaletteCmd::Remap(RemapKind::MergeDuplicates), Any),
        at(PaletteCmd::Remap(RemapKind::RemoveUnused), Any),
        at(PaletteCmd::ReplaceColor { from: 1, to: 2, layer: None }, Any),
        at(PaletteCmd::Load { colors: vec![0xff12_3456u32].into(), how: crate::LoadHow::Append }, Any),
        at(StepCmd::EndDrag, Any),
        at(StepCmd::AbortJoin, Any),
        at(StepCmd::Undo, Any),
        at(StepCmd::Redo, Any),
        at(StepCmd::ScrubTo { depth: 0 }, Any),
    ]
}

/// The pixel commands: the pencil, eraser, line and rectangle edit the tiles under them (Pixels
/// mode), the picker reads what the map draws, and every other one that draws is refused.
#[rustfmt::skip]
fn drawing() -> Vec<Case> {
    let no = Refused(LimitError::Tilemap);
    let begin = |tool| DrawCmd::Begin { tool, x: 1, y: 1 };
    let clip = Clip { x: 0, y: 0, width: 1, height: 1, colors: vec![0xff00_00ff], ids: None };
    let shape = || Box::new(Mask::rect(0, 0, 4, 4));
    vec![
        at(begin(Tool::Pencil), Nothing), at(begin(Tool::Eraser), Nothing), at(begin(Tool::Fill), no),
        at(begin(Tool::Line), Nothing), at(begin(Tool::Rect), Nothing),
        at(begin(Tool::Picker), Nothing),
        at(DrawCmd::To { x: 2, y: 2 }, Nothing),
        at(DrawCmd::Commit, Nothing),
        at(DrawCmd::Cancel, Nothing),
        at(DrawCmd::Stroke { tool: Tool::Pencil, path: vec![(1, 1), (5, 5)] }, Works),
        at(DrawCmd::EraseCell { x: 1, y: 1 }, no),
        at(DrawCmd::DeleteSelected, no),
        at(SelectionCmd::Select { shape: shape(), op: SelectOp::Replace }, Any),
        at(SelectionCmd::Temporary { shape: shape() }, Any),
        at(SelectionCmd::All, Any), at(SelectionCmd::None, Any), at(SelectionCmd::Invert, Any),
        at(FloatingCmd::Lift { copy: false }, no),
        at(FloatingCmd::Move { dx: 1, dy: 1, join: false }, Any),
        at(FloatingCmd::Scale { x: 0, y: 0, w: 2, h: 2 }, Any),
        at(FloatingCmd::FlipHorizontal, no),
        at(FloatingCmd::FlipVertical, no),
        at(FloatingCmd::Rotate { clockwise: true }, no),
        at(FloatingCmd::Anchor, Any), at(FloatingCmd::Revert, Any),
        at(FloatingCmd::Paste { clip: clip.clone().into(), x: 0, y: 0 }, no),
        at(FloatingCmd::ReplacePasted { clip: clip.into() }, no),
        at(FloatingCmd::GestureBegin, Any), at(FloatingCmd::GestureEnd, Any),
        at(FloatingCmd::GestureCancel, Any),
    ]
}

#[rustfmt::skip]
fn cels(map: LayerId) -> Vec<Case> {
    let drag = CelDrag {
        layers: (1, 1), frames: (0, 0), to_layer: 0, to_frame: 0,
        kind: DragKind::Move, landing: Landing::Overwrite,
    };
    vec![
        at(CelCmd::Move { layer: map, frame: 0, dx: 2, dy: 1 }, Works),
        at(CelCmd::MoveOne { layer: map, frame: 0, dx: 2, dy: 1 }, Works),
        at(CelCmd::Turn { layer: map, frame: 0, turn: Turn::FlipHorizontal }, Refused(LimitError::Tilemap)),
        at(CelCmd::Drag(drag), Refused(LimitError::TilemapCels)),
        (spread, CelCmd::Tween { layer: map, from: 0, to: 2, ease: Ease::Linear }.into(), Works),
    ]
}

/// Chooses tile 2 of the map's tileset as the stamp, so strokes have something to place.
fn choose(e: &mut Editor) {
    let set = e.doc().layers()[1].tileset().expect("tileset");
    e.set_current_tile(set, id(2)).expect("a tile of the set");
}

#[rustfmt::skip]
fn tiles(e: &Editor, map: LayerId) -> Vec<Case> {
    let set = e.doc().layers()[1].tileset().expect("tileset");
    let tile = id(1);
    let new = TileCmd::NewTileset { name: "T".into(), mode: ColorMode::Rgba, tile_w: 8, tile_h: 8 };
    vec![
        at(new, Works),
        at(TileCmd::RenameTileset { tileset: set, name: "Stone".into() }, Works),
        at(TileCmd::RemoveTileset { tileset: set }, Refused(LimitError::TilesetInUse)),
        at(TileCmd::AddTile { tileset: set, from: TileSource::Blank }, Works),
        at(TileCmd::DuplicateTile { tileset: set, tile }, Works),
        at(TileCmd::RemoveTile { tileset: set, tile }, Works),
        at(TileCmd::MoveTile { tileset: set, tile, to: 1 }, Works),
        at(TileCmd::SetTileImage { tileset: set, tile, image: tile_image(0xff00_ff00) }, Works),
        at(TileCmd::NewLayer { at: 0, tileset: set }, Works),
        at(TileCmd::NewMapLayer { at: 2, name: "M".into(), mode: ColorMode::Rgba, tile_w: 8, tile_h: 8 }, Works),
        at(TileCmd::CopyTilesetFor { layer: map }, Works),
        // The map's own pixels are cut: every tile is one the tileset has, so none is added.
        at(TileCmd::AddTile { tileset: set, from: TileSource::Selection }, Nothing),
        at(TileCmd::Begin { tool: TileTool::Place, x: 1, y: 1 }, Refused(LimitError::NoTileChosen)),
        (choose, TileCmd::Begin { tool: TileTool::Place, x: 1, y: 1 }.into(), Nothing),
        (choose, TileCmd::Begin { tool: TileTool::Fill, x: 1, y: 1 }.into(), Works),
        at(TileCmd::Begin { tool: TileTool::Erase, x: 1, y: 1 }, Nothing),
        at(TileCmd::Begin { tool: TileTool::Pick, x: 1, y: 1 }, Nothing),
        at(TileCmd::To { x: 5, y: 5 }, Nothing),
        at(TileCmd::Commit, Nothing),
        at(TileCmd::Cancel, Nothing),
        (choose, TileCmd::Stroke { tool: TileTool::Line, path: vec![(1, 1), (7, 1)] }.into(), Works),
        at(TileCmd::Stroke { tool: TileTool::Erase, path: vec![(1, 1), (5, 5)] }, Works),
        at(TileCmd::DeleteSelected, Works),
        at(TileCmd::Begin { tool: ground(set, Brush, false), x: 1, y: 1 }, Refused(LimitError::NoSuchItem)),
        (terrain, TileCmd::Begin { tool: ground(set, Brush, false), x: 1, y: 1 }.into(), Nothing),
        (terrain, TileCmd::Begin { tool: ground(set, Fill, true), x: 1, y: 1 }.into(), Works),
        (terrain, TileCmd::Begin { tool: ground(set, Retile, false), x: 1, y: 1 }.into(), Nothing),
        (terrain, TileCmd::Stroke { tool: ground(set, Brush, true), path: vec![(1, 1), (5, 1)] }.into(), Works),
    ]
}

/// The terrain tool of set 1 of a tileset.
fn ground(tileset: crate::tileset::TilesetId, shape: TerrainShape, clear: bool) -> TileTool {
    let corners = crate::command::Corners::Cell;
    TileTool::Terrain(TerrainTool {
        tileset,
        set: 1,
        shape,
        clear,
        corners,
    })
}

/// Gives the map's tileset an Edge4 terrain set, id 1, with tile 1 in slot 0.
fn terrain(e: &mut Editor) {
    let tileset = e.doc().layers()[1].tileset().expect("tileset");
    let kind = crate::autotile::TerrainKind::Edge4;
    e.apply(TerrainCmd::New {
        tileset,
        kind,
        name: "Grass".into(),
        template: false,
    })
    .expect("room");
    e.apply(TerrainCmd::Assign {
        tileset,
        set: 1,
        slot: 0,
        tile: id(1),
    })
    .expect("fills");
}

/// Terrain sets belong to the tileset, so a tilemap layer being active changes nothing for them.
#[rustfmt::skip]
fn terrains(e: &Editor) -> Vec<Case> {
    let tileset = e.doc().layers()[1].tileset().expect("tileset");
    let kind = crate::autotile::TerrainKind::Blob47;
    let at_set = |command: TerrainCmd, expect| -> Case { (terrain, command.into(), expect) };
    vec![
        at(TerrainCmd::New { tileset, kind, name: "Water".into(), template: true }, Works),
        at_set(TerrainCmd::Rename { tileset, set: 1, name: "Road".into() }, Works),
        at_set(TerrainCmd::Remove { tileset, set: 1 }, Works),
        at_set(TerrainCmd::Assign { tileset, set: 1, slot: 3, tile: id(2) }, Works),
        at_set(TerrainCmd::Clear { tileset, set: 1, slot: 0 }, Works),
    ]
}
