//! Terrain strokes through the editor: the kinds, erase, rectangles, fills, pins, Alt-click, the
//! preview, links, locks and the one undo step each makes.

use super::terrain_fixture::{
    at, by_vertex, mid, placed, slot, terrain, terrain_editor, through, OTHER,
};
use super::PINNED;
use crate::autotile::TerrainKind;
use crate::command::{TerrainShape, TileCmd, TileTool};
use crate::document::LimitError;
use crate::editor::Editor;
use crate::mask::Mask;
use crate::{FrameCmd, LayerCmd, SelectionCmd, TerrainCmd};

use TerrainShape::{Brush, Fill, Rect, Retile};

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

fn begin(tool: TileTool, c: (i32, i32)) -> TileCmd {
    let (x, y) = mid(c);
    TileCmd::Begin { tool, x, y }
}

fn to(c: (i32, i32)) -> TileCmd {
    let (x, y) = mid(c);
    TileCmd::To { x, y }
}

#[test]
fn a_brush_stroke_paints_edge4_as_one_step() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    one_step(
        &mut e,
        through(terrain(set, Brush, false), &[(2, 2), (4, 2)]),
    );
    // N = 1, E = 2, S = 4, W = 8.
    let row: Vec<_> = (1..=5).map(|x| slot(&e, (x, 2))).collect();
    assert_eq!(row, [None, Some(2), Some(10), Some(8), None]);
    assert_eq!(
        slot(&e, (3, 1)),
        None,
        "a neighbour that is not a member stays"
    );
}

#[test]
fn blob47_and_corner4_strokes_place_the_conventional_slots() {
    let (mut e, set) = terrain_editor(TerrainKind::Blob47);
    let block = [(1, 1), (2, 1), (2, 2), (1, 2)];
    e.apply(through(terrain(set, Brush, false), &block))
        .expect("paints");
    let slots: Vec<_> = block.iter().map(|&c| slot(&e, c)).collect();
    assert_eq!(slots, [Some(10), Some(26), Some(34), Some(4)]);

    let (mut e, set) = terrain_editor(TerrainKind::Corner4);
    e.apply(through(terrain(set, Brush, false), &[(5, 5)]))
        .expect("paints");
    let ring: Vec<_> = [
        (4, 4),
        (5, 4),
        (6, 4),
        (4, 5),
        (5, 5),
        (6, 5),
        (4, 6),
        (5, 6),
        (6, 6),
    ]
    .iter()
    .map(|&c| slot(&e, c))
    .collect();
    let want = [2, 6, 4, 3, 15, 12, 1, 9, 8].map(Some);
    assert_eq!(ring, want);
    // Painted by a corner, the press at a cell corner sets the four cells around it.
    let (mut e, set) = terrain_editor(TerrainKind::Corner4);
    let tool = by_vertex(terrain(set, Brush, false));
    e.apply(TileCmd::Begin { tool, x: 12, y: 12 })
        .expect("begins");
    e.apply(TileCmd::Commit).expect("commits");
    let four: Vec<_> = [(2, 2), (3, 2), (2, 3), (3, 3)]
        .iter()
        .map(|&c| slot(&e, c))
        .collect();
    assert_eq!(four, [2, 4, 1, 8].map(Some));
}

#[test]
fn the_eraser_clears_membership_and_the_neighbours_follow() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    e.apply(through(terrain(set, Brush, false), &[(1, 1), (3, 1)]))
        .expect("paints");
    one_step(&mut e, through(terrain(set, Brush, true), &[(2, 1)]));
    assert_eq!(at(&e, (2, 1)), 0);
    assert_eq!((slot(&e, (1, 1)), slot(&e, (3, 1))), (Some(0), Some(0)));
}

#[test]
fn a_rectangle_is_filled_and_redrawn_from_the_press() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    e.apply(begin(terrain(set, Rect, false), (1, 1)))
        .expect("begins");
    for c in [(6, 6), (3, 2)] {
        e.apply(to(c)).expect("drags");
    }
    e.apply(TileCmd::Commit).expect("commits");
    assert_eq!(e.history().undo_depth(), 1);
    // Cells (1, 1) to (3, 2): two rows of three.
    assert_eq!(slot(&e, (1, 1)), Some(6), "E and S");
    assert_eq!(slot(&e, (2, 2)), Some(11), "N, E and W");
    assert_eq!(at(&e, (6, 6)), 0, "the earlier rectangle left nothing");
}

#[test]
fn a_fill_paints_the_region_of_one_value_and_stops_at_others() {
    let (mut e, set) = terrain_editor(TerrainKind::Blob47);
    e.set_current_tile(set, crate::tileset::TileId::new(OTHER).expect("id"))
        .expect("tile");
    // A wall of another tile down column 4.
    let wall: Vec<_> = (0..12).map(|y| (4, y)).collect();
    e.apply(through(TileTool::Place, &wall)).expect("walls");
    one_step(&mut e, begin(terrain(set, Fill, false), (1, 1)));
    assert!(!e.is_stroking(), "a fill is finished at once");
    assert_eq!(
        slot(&e, (1, 1)),
        Some(46),
        "the inside is the full interior"
    );
    // N, S, SW, W and NW: mask 241, slot 42 of the 47.
    assert_eq!(
        slot(&e, (3, 5)),
        Some(42),
        "the edge by the wall has no east"
    );
    assert_eq!(at(&e, (4, 5)), placed(OTHER));
    assert_eq!(at(&e, (5, 5)), 0, "past the wall");
}

#[test]
fn placing_a_terrain_tile_by_hand_pins_it_and_other_tiles_stay_unpinned() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    e.set_current_tile(set, crate::tileset::TileId::new(1).expect("id"))
        .expect("tile");
    e.apply(through(TileTool::Place, &[(5, 5)]))
        .expect("places");
    assert_eq!(at(&e, (5, 5)), placed(1) | PINNED);
    e.set_current_tile(set, crate::tileset::TileId::new(OTHER).expect("id"))
        .expect("tile");
    e.apply(through(TileTool::Place, &[(6, 6)]))
        .expect("places");
    assert_eq!(at(&e, (6, 6)), placed(OTHER));
    // Without terrain sets nothing is pinned.
    e.apply(TerrainCmd::Remove {
        tileset: set,
        set: 1,
    })
    .expect("removes");
    e.set_current_tile(set, crate::tileset::TileId::new(1).expect("id"))
        .expect("tile");
    e.apply(through(TileTool::Place, &[(7, 7)]))
        .expect("places");
    assert_eq!(at(&e, (7, 7)), placed(1));
}

#[test]
fn a_pinned_cell_keeps_its_tile_until_painted_over() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    e.set_current_tile(set, crate::tileset::TileId::new(1).expect("id"))
        .expect("tile");
    e.apply(through(TileTool::Place, &[(5, 5)]))
        .expect("places");
    e.apply(through(terrain(set, Brush, false), &[(4, 5)]))
        .expect("paints");
    assert_eq!(
        at(&e, (5, 5)),
        placed(1) | PINNED,
        "unchanged as a neighbour"
    );
    assert_eq!(
        slot(&e, (4, 5)),
        Some(2),
        "and it counts as a member to the east"
    );
    e.apply(through(terrain(set, Brush, false), &[(5, 5)]))
        .expect("paints");
    assert_eq!(at(&e, (5, 5)), placed(9), "painted over: W, unpinned");
}

#[test]
fn alt_click_retiles_one_cell_and_unpins_it_as_one_step() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    e.apply(through(terrain(set, Brush, false), &[(4, 5), (6, 5)]))
        .expect("paints");
    e.set_current_tile(set, crate::tileset::TileId::new(1).expect("id"))
        .expect("tile");
    e.apply(through(TileTool::Place, &[(5, 5)]))
        .expect("places");
    let (left, right) = (at(&e, (4, 5)), at(&e, (6, 5)));
    one_step(&mut e, begin(terrain(set, Retile, false), (5, 5)));
    assert_eq!(at(&e, (5, 5)), placed(11), "E and W, unpinned");
    assert_eq!((at(&e, (4, 5)), at(&e, (6, 5))), (left, right));
    // On a cell that is not a member it does nothing.
    let out = e
        .apply(begin(terrain(set, Retile, false), (0, 0)))
        .expect("runs");
    assert!(!out.changed);
}

#[test]
fn the_preview_lists_what_the_commit_writes_and_nothing_is_written_before() {
    let (mut e, set) = terrain_editor(TerrainKind::Blob47);
    assert_eq!(e.terrain_preview(), None);
    let before = e.doc().clone();
    e.apply(begin(terrain(set, Brush, false), (2, 2)))
        .expect("begins");
    e.apply(to((5, 3))).expect("drags");
    let preview = e.terrain_preview().expect("a stroke");
    assert_eq!((preview.layer, preview.frame), (1, 0));
    assert_eq!(e.doc(), &before);
    assert_eq!(e.history().undo_depth(), 0);
    e.apply(TileCmd::Commit).expect("commits");
    let changed: Vec<_> = (0..12)
        .flat_map(|y| (0..12).map(move |x| (x, y)))
        .filter(|&c| at(&e, c) != 0)
        .map(|c| (c, super::MapCell::from_bits(at(&e, c))))
        .collect();
    assert_eq!(preview.cells, changed);
    assert_eq!(e.terrain_preview(), None);
}

#[test]
fn a_cancelled_stroke_changes_nothing() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    let before = e.doc().clone();
    e.apply(begin(terrain(set, Brush, false), (2, 2)))
        .expect("begins");
    e.apply(to((6, 6))).expect("drags");
    e.apply(TileCmd::Cancel).expect("cancels");
    assert!(!e.is_stroking());
    assert_eq!((e.doc(), e.history().undo_depth()), (&before, 0));
}

#[test]
fn a_stroke_is_written_through_to_every_frame_that_holds_the_drawing() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    let layer = e.layer_id(1).expect("layer");
    let hold = FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 0,
        to: 2,
    };
    e.apply(hold).expect("holds");
    e.apply(through(terrain(set, Brush, false), &[(3, 3), (4, 3)]))
        .expect("paints");
    for frame in 0..3 {
        let image = e.doc().layers()[1].cel(frame).image();
        assert_eq!(image.get(3, 3), placed(3), "frame {frame}");
    }
    assert!(e.undo());
    for frame in 0..3 {
        assert_eq!(e.doc().layers()[1].cel(frame).image().get(3, 3), 0);
    }
}

#[test]
fn a_locked_layer_refuses_every_terrain_press() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    e.apply(through(terrain(set, Brush, false), &[(3, 3)]))
        .expect("paints");
    let layer = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::SetLocked {
        layer,
        locked: true,
    })
    .expect("locks");
    let before = e.doc().clone();
    for shape in [Brush, Rect, Fill, Retile] {
        for clear in [false, true] {
            let tool = terrain(set, shape, clear);
            assert_eq!(e.apply(begin(tool, (3, 3))), Err(LimitError::Locked));
            assert_eq!(e.apply(through(tool, &[(3, 3)])), Err(LimitError::Locked));
        }
    }
    assert_eq!(e.doc(), &before);
}

#[test]
fn a_terrain_press_needs_a_map_of_the_sets_tileset_and_the_set() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    let other = crate::tileset::TilesetId::new(99);
    assert_eq!(
        e.apply(begin(terrain(other, Brush, false), (1, 1))),
        Err(LimitError::NoTileChosen)
    );
    let missing = match terrain(set, Brush, false) {
        TileTool::Terrain(t) => TileTool::Terrain(crate::command::TerrainTool { set: 9, ..t }),
        tool => tool,
    };
    assert_eq!(e.apply(begin(missing, (1, 1))), Err(LimitError::NoSuchItem));
    e.layer = 0;
    assert_eq!(
        e.apply(begin(terrain(set, Brush, false), (1, 1))),
        Err(LimitError::NotTilemap)
    );
}

#[test]
fn a_selection_limits_which_cells_change() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    let shape = Box::new(Mask::rect(0, 0, 12, 48));
    e.apply(SelectionCmd::Select {
        shape,
        op: crate::SelectOp::Replace,
    })
    .expect("selects");
    e.apply(through(terrain(set, Brush, false), &[(0, 0), (5, 0)]))
        .expect("paints");
    let row: Vec<_> = (0..5).map(|x| slot(&e, (x, 0))).collect();
    assert_eq!(row, [Some(2), Some(10), Some(8), None, None]);
}

/// Cell values of the whole map, row by row.
fn whole_map(e: &Editor) -> Vec<u32> {
    (0..CELLS_ACROSS)
        .flat_map(|y| (0..CELLS_ACROSS).map(move |x| (x, y)))
        .map(|c| at(e, c))
        .collect()
}

const CELLS_ACROSS: i32 = 12;

/// A rectangle dragged far past the map visits no more points than the map's window holds, and
/// paints the same cells as one dragged to the map's corner.
#[test]
fn a_far_off_rectangle_drag_visits_only_the_window_and_equals_the_clamped_one() {
    for (kind, far) in [
        (TerrainKind::Blob47, (40_000, 40_000)),
        (TerrainKind::Blob47, (-40_000, 40_000)),
        (TerrainKind::Corner4, (40_000, -40_000)),
    ] {
        for vertex in [false, true] {
            let tool = |s| {
                let t = terrain(s, Rect, false);
                if vertex {
                    by_vertex(t)
                } else {
                    t
                }
            };
            let run = |to: (i32, i32)| {
                let (mut e, set) = terrain_editor(kind);
                let (x, y) = mid((3, 3));
                e.apply(TileCmd::Begin {
                    tool: tool(set),
                    x,
                    y,
                })
                .expect("begins");
                crate::autotile::stroke::ADDS_TRIED.with(|n| n.set(0));
                e.apply(TileCmd::To { x: to.0, y: to.1 }).expect("drags");
                let tried = crate::autotile::stroke::ADDS_TRIED.with(std::cell::Cell::get);
                e.apply(TileCmd::Commit).expect("commits");
                (whole_map(&e), tried)
            };
            let (far_map, tried) = run(far);
            // The work is bounded by the window of cells (one more each way for vertices), not
            // by how far the pointer went: an unclamped loop tries about 100 million points.
            let bound = (CELLS_ACROSS as usize + 3).pow(2);
            assert!(tried <= bound, "{kind:?} {far:?} tried {tried} points");
            // The same corner of the map, reached without leaving it.
            let near = (far.0.clamp(-4, 48 + 4), far.1.clamp(-4, 48 + 4));
            assert_eq!(far_map, run(near).0, "{kind:?} {far:?} vertex {vertex}");
        }
    }
}

#[test]
fn a_fill_leaves_pinned_cells_alone_and_stops_at_them() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    // Two unpinned cells of tile 1, with a hand-placed (pinned) tile 1 between them.
    e.apply(through(terrain(set, Brush, false), &[(2, 2)]))
        .expect("paints");
    e.apply(through(terrain(set, Brush, false), &[(4, 2)]))
        .expect("paints");
    e.set_current_tile(set, crate::tileset::TileId::new(1).expect("id"))
        .expect("tile");
    e.apply(through(TileTool::Place, &[(3, 2)]))
        .expect("places");
    assert_eq!(at(&e, (3, 2)), placed(1) | PINNED);
    let far = at(&e, (4, 2));
    one_step(&mut e, begin(terrain(set, Fill, false), (2, 2)));
    assert_eq!(slot(&e, (2, 2)), Some(2), "the pressed cell is filled");
    assert_eq!(at(&e, (3, 2)), placed(1) | PINNED, "the pin and tile stay");
    assert_eq!(at(&e, (4, 2)), far, "the pin walls the flood in");
    // Pressing on a pinned cell fills nothing.
    let before = e.doc().clone();
    let out = e
        .apply(begin(terrain(set, Fill, false), (3, 2)))
        .expect("runs");
    assert!(!out.changed);
    assert_eq!(e.doc(), &before);
}

/// A plain fill compares whole cell values, so a pinned cell never equals an unpinned one: it is
/// kept and walls the flood too.
#[test]
fn a_plain_fill_keeps_pinned_cells_as_walls() {
    let (mut e, set) = terrain_editor(TerrainKind::Edge4);
    e.apply(through(terrain(set, Brush, false), &[(2, 2)]))
        .expect("paints");
    e.apply(through(terrain(set, Brush, false), &[(4, 2)]))
        .expect("paints");
    e.set_current_tile(set, crate::tileset::TileId::new(1).expect("id"))
        .expect("tile");
    e.apply(through(TileTool::Place, &[(3, 2)]))
        .expect("places");
    let far = at(&e, (4, 2));
    e.set_current_tile(set, crate::tileset::TileId::new(OTHER).expect("id"))
        .expect("tile");
    e.apply(begin(TileTool::Fill, (2, 2))).expect("fills");
    assert_eq!(at(&e, (2, 2)), placed(OTHER));
    assert_eq!(at(&e, (3, 2)), placed(1) | PINNED);
    assert_eq!(at(&e, (4, 2)), far);
}
