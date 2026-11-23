//! Map strokes: placing, erasing, filling, lines, rectangles, pick, delete under a selection,
//! locks, offsets, and the one undo step each makes.

use super::fixture::{cell, corner, editor_with_map, id, TILE};
use super::MapCell;
use crate::command::{TileCmd, TileTool};
use crate::document::LimitError;
use crate::editor::Editor;
use crate::layer::Turn;
use crate::mask::Mask;
use crate::orient::Orientation;
use crate::{FrameCmd, LayerCmd, SelectionCmd};

/// The cell at (`cx`, `cy`) of the active frame's map.
fn at(e: &Editor, cx: i32, cy: i32) -> u32 {
    e.doc().layers()[e.layer].cel(e.frame).image().get(cx, cy)
}

/// The pixel in the middle of cell (`cx`, `cy`) of the fixture's map.
fn mid(cx: i32, cy: i32) -> (i32, i32) {
    (cx * TILE as i32 + 1, cy * TILE as i32 + 1)
}

fn begin(tool: TileTool, (x, y): (i32, i32)) -> TileCmd {
    TileCmd::Begin { tool, x, y }
}

fn stroke(tool: TileTool, cells: &[(i32, i32)]) -> TileCmd {
    TileCmd::Stroke {
        tool,
        path: cells.iter().map(|&(cx, cy)| mid(cx, cy)).collect(),
    }
}

/// An editor on the fixture map with tile 2 chosen.
fn ready() -> Editor {
    let (mut e, set) = editor_with_map();
    e.set_current_tile(set, id(2)).expect("a tile of the set");
    e
}

/// Runs a command that must be one undo step, then checks undo and redo give the states.
fn one_step(e: &mut Editor, command: TileCmd) {
    let (before, depth) = (e.doc().clone(), e.history().undo_depth());
    assert!(e.apply(command).expect("runs").changed);
    assert_eq!(e.history().undo_depth(), depth + 1, "one step");
    assert!(!e.is_stroking());
    let after = e.doc().clone();
    assert_eq!(after.check_tilemaps(), Ok(()));
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    assert!(e.redo());
    assert_eq!(e.doc(), &after);
}

#[test]
fn a_stroke_places_the_tile_along_its_cells_as_one_step() {
    let mut e = ready();
    one_step(&mut e, stroke(TileTool::Place, &[(0, 3), (3, 3)]));
    for cx in 0..4 {
        assert_eq!(at(&e, cx, 3), cell(2), "cell {cx}");
    }
    assert_eq!(at(&e, 0, 2), 0);
}

#[test]
fn events_and_a_whole_stroke_make_the_same_map() {
    let mut whole = ready();
    let mut parts = ready();
    let path = [(0, 2), (2, 3), (3, 0)];
    whole.apply(stroke(TileTool::Place, &path)).expect("stroke");
    let first = mid(path[0].0, path[0].1);
    parts.apply(begin(TileTool::Place, first)).expect("begin");
    assert!(parts.is_stroking());
    for &(cx, cy) in &path[1..] {
        let (x, y) = mid(cx, cy);
        parts.apply(TileCmd::To { x, y }).expect("to");
    }
    assert_eq!(parts.history().undo_depth(), 0, "nothing before the commit");
    parts.apply(TileCmd::Commit).expect("commit");
    assert_eq!(parts.doc(), whole.doc());
}

#[test]
fn a_cancelled_stroke_changes_nothing() {
    let mut e = ready();
    let before = e.doc().clone();
    e.apply(begin(TileTool::Place, mid(2, 2))).expect("begin");
    e.apply(TileCmd::To { x: 12, y: 12 }).expect("to");
    e.apply(TileCmd::Cancel).expect("cancel");
    assert!(!e.is_stroking());
    assert_eq!((e.doc(), e.history().undo_depth()), (&before, 0));
}

#[test]
fn staying_in_one_cell_places_once_and_a_stroke_that_changes_nothing_is_no_step() {
    let mut e = ready();
    // Cell (1, 0) already holds tile 2 in frame 0.
    let out = e
        .apply(stroke(TileTool::Place, &[(1, 0), (1, 0)]))
        .expect("runs");
    assert!(!out.changed);
    assert_eq!(e.history().undo_depth(), 0);
}

#[test]
fn the_eraser_clears_cells_and_leaves_an_exact_zero() {
    let mut e = ready();
    one_step(&mut e, stroke(TileTool::Erase, &[(0, 0), (1, 0)]));
    assert_eq!(corner(e.doc().layers()[1].cel(0))[..2], [0, 0]);
}

#[test]
fn a_fill_replaces_the_connected_cells_and_stops_at_other_values() {
    let mut e = ready();
    // Frame 0 is `1 2 / 2 1` over a blank rest of 4 by 4 cells.
    e.apply(begin(TileTool::Fill, mid(3, 3))).expect("fill");
    assert!(!e.is_stroking());
    let empty_before = 12;
    let filled = (0..4)
        .flat_map(|y| (0..4).map(move |x| (x, y)))
        .filter(|&(x, y)| at(&e, x, y) == cell(2))
        .count();
    // The two tile-2 cells that were there, plus all twelve empty cells.
    assert_eq!(filled, 2 + empty_before);
    assert_eq!(at(&e, 0, 0), cell(1), "another value is a wall");
    assert!(e.undo());
    assert_eq!(at(&e, 3, 3), 0);
    assert_eq!(e.history().undo_depth(), 0);
}

#[test]
fn a_fill_of_a_block_repeats_it_as_a_pattern() {
    let (mut e, set) = editor_with_map();
    e.set_tile_block(set, 2, 1, &[Some(id(1)), Some(id(2))])
        .expect("block");
    e.apply(begin(TileTool::Fill, mid(2, 2))).expect("fill");
    assert_eq!((at(&e, 2, 2), at(&e, 3, 2)), (cell(1), cell(2)));
    assert_eq!((at(&e, 2, 3), at(&e, 3, 3)), (cell(1), cell(2)));
}

#[test]
fn a_line_and_a_rectangle_redraw_from_the_press_and_are_one_step() {
    let mut e = ready();
    e.apply(begin(TileTool::Rect, mid(1, 1))).expect("begin");
    for to in [(3, 3), (2, 2), (3, 2)] {
        let (x, y) = mid(to.0, to.1);
        e.apply(TileCmd::To { x, y }).expect("to");
    }
    e.apply(TileCmd::Commit).expect("commit");
    // The last rectangle is cells (1,1) to (3,2); the earlier shapes left nothing behind.
    assert_eq!(e.history().undo_depth(), 1);
    assert_eq!(at(&e, 3, 3), 0);
    assert_eq!(at(&e, 2, 1), cell(2));
    assert_eq!(at(&e, 3, 2), cell(2));
    one_step(&mut e, stroke(TileTool::Line, &[(0, 3), (3, 3)]));
}

#[test]
fn a_stamp_of_cells_is_centred_by_the_brush_rule_and_clipped_by_the_window() {
    let (mut e, set) = editor_with_map();
    e.set_tile_block(set, 2, 2, &[Some(id(1)), None, Some(id(2)), Some(id(1))])
        .expect("block");
    // A 2 by 2 block sits with its corner at the cell, as a 2 wide brush does.
    e.apply(stroke(TileTool::Place, &[(2, 2)])).expect("stroke");
    assert_eq!(at(&e, 2, 2), cell(1));
    assert_eq!(
        at(&e, 3, 2),
        0,
        "an empty cell of the stamp leaves the map alone"
    );
    assert_eq!(at(&e, 2, 3), cell(2));
    assert_eq!(at(&e, 3, 3), cell(1));
    // At the corner of the window the cells past it are dropped, not wrapped.
    e.apply(stroke(TileTool::Place, &[(3, 3)])).expect("stroke");
    assert_eq!(at(&e, 4, 4), 0);
    assert_eq!(at(&e, 3, 3), cell(1));
    assert_eq!(e.doc().check_tilemaps(), Ok(()));
}

#[test]
fn turning_a_stamp_turns_positions_and_composes_each_cells_turn() {
    let (mut e, set) = editor_with_map();
    e.set_tile_block(set, 2, 1, &[Some(id(1)), Some(id(2))])
        .expect("block");
    let original = e.tiles().stamp().cloned().expect("a stamp");
    e.turn_tile_stamp(Turn::Clockwise).expect("turns");
    let stamp = e.tiles().stamp().cloned().expect("a stamp");
    assert_eq!(stamp.size(), (1, 2));
    let turn = Orientation::rotate90();
    assert_eq!(stamp.cell(0, 0), MapCell::place(id(1), turn));
    assert_eq!(stamp.cell(0, 1), MapCell::place(id(2), turn));
    for _ in 0..3 {
        e.turn_tile_stamp(Turn::Clockwise).expect("turns");
    }
    assert_eq!(e.tiles().stamp(), Some(&original));
    // Placed, the turn is in the cells.
    e.turn_tile_stamp(Turn::FlipHorizontal).expect("flips");
    e.apply(stroke(TileTool::Place, &[(2, 2)])).expect("stroke");
    let flip = Orientation::flip_horizontal();
    assert_eq!(at(&e, 2, 2), MapCell::place(id(2), flip).bits());
    assert_eq!(at(&e, 3, 2), MapCell::place(id(1), flip).bits());
}

#[test]
fn picking_takes_the_tile_and_its_turn_and_is_no_step() {
    let (mut e, set) = editor_with_map();
    e.set_current_tile(set, id(1)).expect("tile");
    e.turn_tile_stamp(Turn::Clockwise).expect("turns");
    e.apply(stroke(TileTool::Place, &[(3, 3)])).expect("stroke");
    e.clear_tile_stamp();
    let depth = e.history().undo_depth();
    let out = e.apply(begin(TileTool::Pick, mid(3, 3))).expect("pick");
    assert!(!out.changed);
    assert_eq!(e.history().undo_depth(), depth);
    assert_eq!(
        e.tiles().current(),
        Some((set, id(1), Orientation::rotate90()))
    );
    // An empty cell leaves the stamp as it is.
    e.apply(begin(TileTool::Pick, mid(2, 3))).expect("pick");
    assert_eq!(
        e.tiles().current(),
        Some((set, id(1), Orientation::rotate90()))
    );
}

#[test]
fn a_stamp_can_be_captured_from_the_map_under_the_selection() {
    let (mut e, set) = editor_with_map();
    let shape = Box::new(Mask::rect(0, 0, 8, 4));
    e.apply(SelectionCmd::Select {
        shape,
        op: crate::SelectOp::Replace,
    })
    .expect("selects");
    e.capture_tile_stamp().expect("captured");
    let stamp = e.tiles().stamp().cloned().expect("a stamp");
    assert_eq!((stamp.tileset(), stamp.size()), (set, (2, 1)));
    assert_eq!(
        (stamp.cell(0, 0), stamp.cell(1, 0)),
        (MapCell::from_bits(cell(1)), MapCell::from_bits(cell(2)))
    );
    e.apply(SelectionCmd::None).expect("deselects");
    assert_eq!(e.capture_tile_stamp(), Err(LimitError::NothingToCapture));
}

#[test]
fn delete_clears_the_cells_whose_centre_the_selection_holds() {
    let mut e = ready();
    // The pixel rectangle 0..6 holds the centre (2) of cell 0 and not that of cell 1 (6).
    let shape = Box::new(Mask::rect(0, 0, 6, 8));
    e.apply(SelectionCmd::Select {
        shape,
        op: crate::SelectOp::Replace,
    })
    .expect("selects");
    one_step(&mut e, TileCmd::DeleteSelected);
    assert_eq!([at(&e, 0, 0), at(&e, 0, 1)], [0, 0]);
    assert_eq!([at(&e, 1, 0), at(&e, 1, 1)], [cell(2), cell(1)]);
    e.apply(SelectionCmd::None).expect("deselects");
    let out = e.apply(TileCmd::DeleteSelected).expect("nothing selected");
    assert!(!out.changed);
}

#[test]
fn a_selection_limits_where_a_stroke_places() {
    let mut e = ready();
    let shape = Box::new(Mask::rect(8, 8, 8, 8));
    e.apply(SelectionCmd::Select {
        shape,
        op: crate::SelectOp::Replace,
    })
    .expect("selects");
    e.apply(stroke(TileTool::Place, &[(0, 3), (3, 3)]))
        .expect("stroke");
    let placed: Vec<i32> = (0..4).filter(|&x| at(&e, x, 3) != 0).collect();
    assert_eq!(placed, [2, 3]);
}

#[test]
fn a_locked_layer_refuses_strokes_but_not_a_pick() {
    let mut e = ready();
    let layer = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::SetLocked {
        layer,
        locked: true,
    })
    .expect("locks");
    let before = e.doc().clone();
    assert_eq!(
        e.apply(begin(TileTool::Place, mid(2, 2))),
        Err(LimitError::Locked)
    );
    assert_eq!(
        e.apply(stroke(TileTool::Erase, &[(0, 0)])),
        Err(LimitError::Locked)
    );
    assert_eq!(e.apply(TileCmd::DeleteSelected), Err(LimitError::Locked));
    assert!(e.apply(begin(TileTool::Pick, mid(0, 0))).is_ok());
    assert_eq!(e.doc(), &before);
}

#[test]
fn only_a_tilemap_layer_takes_a_stroke_and_it_needs_a_tile_of_its_tileset() {
    let (mut e, _) = editor_with_map();
    assert_eq!(
        e.apply(begin(TileTool::Place, mid(0, 0))),
        Err(LimitError::NoTileChosen)
    );
    e.layer = 0;
    assert_eq!(
        e.apply(begin(TileTool::Erase, mid(0, 0))),
        Err(LimitError::NotTilemap)
    );
    assert_eq!(
        e.apply(TileCmd::DeleteSelected),
        Err(LimitError::NotTilemap)
    );
    // A stamp of another tileset is not placed here.
    e.layer = 1;
    let other = crate::tileset::TilesetId::new(99);
    assert!(e.set_current_tile(other, id(1)).is_err());
}

#[test]
fn a_stroke_is_written_through_to_every_frame_that_holds_the_drawing() {
    let mut e = ready();
    let layer = e.layer_id(1).expect("layer");
    e.apply(FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 0,
        to: 2,
    })
    .expect("holds");
    e.apply(stroke(TileTool::Place, &[(3, 3)])).expect("stroke");
    for frame in 0..3 {
        assert_eq!(
            e.doc().layers()[1].cel(frame).image().get(3, 3),
            cell(2),
            "frame {frame}"
        );
    }
    assert_eq!(e.history().undo_depth(), 2);
    assert!(e.undo());
    for frame in 0..3 {
        assert_eq!(
            e.doc().layers()[1].cel(frame).image().get(3, 3),
            0,
            "frame {frame}"
        );
    }
}

#[test]
fn the_grid_moves_with_the_cel_and_cells_before_the_origin_are_placed() {
    let mut e = ready();
    let layer = e.layer_id(1).expect("layer");
    e.apply(crate::CelCmd::Move {
        layer,
        frame: 0,
        dx: 2,
        dy: 0,
    })
    .expect("moves");
    // The cel's offset is 2, so canvas x 0..2 is the end of cell -1 and x 2..6 is cell 0.
    e.apply(begin(TileTool::Place, (0, 1))).expect("begin");
    e.apply(TileCmd::Commit).expect("commit");
    assert_eq!(at(&e, -1, 0), cell(2));
    e.apply(begin(TileTool::Pick, (3, 1))).expect("pick");
    assert_eq!(e.tiles().current().map(|(_, t, _)| t), Some(id(1)));
    assert_eq!(e.doc().check_tilemaps(), Ok(()));
    assert!(e.undo());
    assert_eq!(at(&e, -1, 0), 0);
}

#[test]
fn oblong_tiles_take_flips_and_refuse_quarter_turns() {
    let mut doc = crate::Document::new(16, 16, crate::ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Bricks", crate::ColorMode::Rgba, (4, 2))
        .expect("room");
    let image = std::sync::Arc::new(crate::Image::blank());
    let tile = doc.add_tile(set, image).expect("fits");
    doc.add_tilemap_layer(1, set).expect("room");
    let mut e = Editor::new(doc);
    e.layer = 1;
    e.set_current_tile(set, tile).expect("tile");
    assert_eq!(
        e.turn_tile_stamp(Turn::Clockwise),
        Err(LimitError::SquareTiles)
    );
    assert_eq!(e.turn_tile_stamp(Turn::FlipVertical), Ok(true));
    e.apply(stroke(TileTool::Place, &[])).expect("empty path");
    e.apply(TileCmd::Stroke {
        tool: TileTool::Place,
        path: vec![(1, 1), (9, 5)],
    })
    .expect("stroke");
    assert_eq!(e.doc().check_tilemaps(), Ok(()));
}

#[test]
fn a_stamp_whose_tile_was_removed_is_refused_and_a_block_drops_the_gone_cells() {
    let (mut e, set) = editor_with_map();
    e.set_current_tile(set, id(2)).expect("tile");
    e.apply(TileCmd::RemoveTile {
        tileset: set,
        tile: id(2),
    })
    .expect("removed");
    assert_eq!(
        e.apply(begin(TileTool::Place, mid(2, 2))),
        Err(LimitError::NoTileChosen)
    );
    e.set_tile_block(set, 2, 1, &[Some(id(1)), None])
        .expect("block");
    e.apply(stroke(TileTool::Place, &[(2, 2)])).expect("stroke");
    assert_eq!(at(&e, 2, 2), cell(1));
}

#[test]
fn a_transaction_waits_for_a_map_stroke_in_progress() {
    let mut e = ready();
    e.apply(begin(TileTool::Place, mid(0, 0))).expect("pressed");
    let ran = e.transaction(|e: &mut Editor| e.apply(FrameCmd::Add { at: 1, copy: false }));
    assert_eq!(ran.map(|_| ()), Err(LimitError::Busy));
    assert!(e.is_stroking(), "the map stroke goes on");
}
