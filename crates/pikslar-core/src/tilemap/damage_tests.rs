use crate::command::TileCmd;
use crate::editor::Damage;
use crate::tile::TILE;
use crate::tilemap::fixture::{doc_with_map, editor_with_map, id, tile_image};
use crate::tilemap::oracle::{random_case, Case, Rng};
use crate::tilemap::TileEdit;
use crate::tileset::TileId;
use std::collections::BTreeSet;

/// Which canvas tiles the oracle says show a cell placing one of `ids`.
fn oracle_tiles(case: &Case, ids: &[u32]) -> BTreeSet<usize> {
    let (w, h) = (case.doc.width, case.doc.height);
    let across = w.div_ceil(TILE);
    let mut found = BTreeSet::new();
    for y in 0..h {
        for x in 0..w {
            if case.places_at(x as i32, y as i32, ids) {
                found.insert(y / TILE * across + x / TILE);
            }
        }
    }
    found
}

#[test]
fn damage_is_exactly_the_canvas_tiles_where_an_edited_tile_is_placed() {
    for seed in 1..=150 {
        let case = random_case(seed, (70, 55));
        let mut rng = Rng(seed ^ 0xabcd);
        let count = case.tiles.len() as u32;
        let ids: Vec<u32> = (0..1 + rng.below(3))
            .map(|_| 1 + rng.below(count))
            .collect();
        let tiles: Vec<TileId> = ids.iter().map(|&n| id(n)).collect();
        let edit = TileEdit::new(case.set, &tiles);
        let mut found = BTreeSet::new();
        case.doc.placed_canvas_tiles(&edit, 0, &[true], |i| {
            found.insert(i);
        });
        assert_eq!(
            found,
            oracle_tiles(&case, &ids),
            "seed {seed} size {:?}",
            case.size
        );
        // A layer the screen does not draw has nothing to redraw.
        let mut none = BTreeSet::new();
        case.doc.placed_canvas_tiles(&edit, 0, &[false], |i| {
            none.insert(i);
        });
        assert!(none.is_empty());
    }
}

#[test]
fn another_tileset_and_another_frame_place_nothing() {
    let (doc, set) = doc_with_map();
    let edit = TileEdit::new(set, &[id(1), id(2)]);
    let mut all = Vec::new();
    doc.placed_canvas_tiles(&edit, 2, &[true, true], |i| all.push(i));
    assert!(all.is_empty(), "frame 2 holds no cells");
    let other = TileEdit::new(crate::tileset::TilesetId::new(set.get() + 9), &[id(1)]);
    doc.placed_canvas_tiles(&other, 0, &[true, true], |i| all.push(i));
    assert!(all.is_empty());
}

#[test]
fn tile_edits_join_only_within_one_tileset() {
    let (_, set) = doc_with_map();
    let mut a = TileEdit::new(set, &[id(2), id(1), id(2)]);
    assert_eq!(a.tiles(), [id(1), id(2)]);
    assert!(a.join(&TileEdit::new(set, &[id(3)])));
    assert_eq!(a.tiles(), [id(1), id(2), id(3)]);
    let elsewhere = TileEdit::new(crate::tileset::TilesetId::new(set.get() + 1), &[id(1)]);
    assert!(!a.join(&elsewhere));
}

fn edit_tile(e: &mut crate::Editor, set: crate::tileset::TilesetId, n: u32, color: u32) {
    e.apply(TileCmd::SetTileImage {
        tileset: set,
        tile: id(n),
        image: tile_image(color),
    })
    .expect("fits");
}

#[test]
fn editing_a_tile_and_undoing_it_damage_only_where_it_is_placed() {
    let (mut e, set) = editor_with_map();
    e.take_damage();
    edit_tile(&mut e, set, 1, 0xff11_2233);
    let want = Damage::Placed(TileEdit::new(set, &[id(1)]));
    assert_eq!(e.take_damage(), want);
    assert!(e.undo());
    assert_eq!(e.take_damage(), want, "undo");
    assert!(e.redo());
    assert_eq!(e.take_damage(), want, "redo");
}

#[test]
fn edits_of_one_tileset_in_one_round_are_one_damage() {
    let (mut e, set) = editor_with_map();
    e.take_damage();
    edit_tile(&mut e, set, 1, 0xff11_2233);
    edit_tile(&mut e, set, 2, 0xff33_2211);
    assert_eq!(
        e.take_damage(),
        Damage::Placed(TileEdit::new(set, &[id(1), id(2)]))
    );
}

#[test]
fn a_tile_edit_next_to_anything_else_draws_everything() {
    let (mut e, set) = editor_with_map();
    e.take_damage();
    e.layer = 0;
    e.apply(crate::DrawCmd::Begin {
        tool: crate::Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("begin");
    edit_tile(&mut e, set, 1, 0xff11_2233);
    assert_eq!(e.take_damage(), Damage::All);
    // The other way round: a stroke after the edit.
    edit_tile(&mut e, set, 2, 0xff33_2211);
    e.apply(crate::DrawCmd::Begin {
        tool: crate::Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("begin");
    assert_eq!(e.take_damage(), Damage::All);
}

#[test]
fn undoing_an_edit_from_another_frame_redraws_the_frame_shown() {
    let (mut e, set) = editor_with_map();
    edit_tile(&mut e, set, 1, 0xff11_2233);
    e.frame = 1;
    e.take_damage();
    assert!(e.undo());
    assert_eq!(e.frame, 1, "a tile edit belongs to no frame");
    assert_eq!(
        e.take_damage(),
        Damage::Placed(TileEdit::new(set, &[id(1)]))
    );
}

#[test]
fn other_steps_still_draw_everything_when_undone() {
    let (mut e, _) = editor_with_map();
    e.take_damage();
    e.apply(crate::FrameCmd::Add { at: 1, copy: false })
        .expect("room");
    e.take_damage();
    assert!(e.undo());
    assert_eq!(e.take_damage(), Damage::All);
}

#[test]
fn an_edit_draws_its_own_canvas_tiles_too_and_joins_them() {
    let (doc, set) = doc_with_map();
    let edit = TileEdit::new(set, &[]).with_canvas_tiles(&[0, 0, 99]);
    assert_eq!(edit.canvas_tiles(), &[0, 99]);
    let mut found = Vec::new();
    doc.placed_canvas_tiles(&edit, 2, &[true, true], |i| found.push(i));
    assert_eq!(found, [0], "a tile past the canvas is left out");
    let mut joined = TileEdit::new(set, &[id(1)]);
    assert!(joined.join(&edit));
    assert_eq!(
        (joined.tiles(), joined.canvas_tiles()),
        (&[id(1)][..], &[0, 99][..])
    );
}

/// Tile edits, alone and among other steps, give the damage of stepping one at a time when
/// scrubbed past in one go.
#[test]
fn scrubbing_past_tile_edits_damages_as_stepping_does() {
    let make = |with_stroke: bool| {
        let (mut e, set) = editor_with_map();
        edit_tile(&mut e, set, 1, 0xff11_2233);
        edit_tile(&mut e, set, 2, 0xff33_2211);
        if with_stroke {
            e.layer = 0;
            e.apply(crate::DrawCmd::Begin {
                tool: crate::Tool::Pencil,
                x: 1,
                y: 1,
            })
            .expect("begin");
            e.apply(crate::DrawCmd::Commit).expect("commit");
        }
        edit_tile(&mut e, set, 1, 0xff44_5566);
        e.take_damage();
        e
    };
    for with_stroke in [false, true] {
        let (mut scrubbed, mut stepped) = (make(with_stroke), make(with_stroke));
        let top = scrubbed.history().undo_depth();
        for depth in [top - 2, 0, top] {
            scrubbed
                .apply(crate::StepCmd::ScrubTo { depth })
                .expect("scrub");
            while stepped.history().undo_depth() > depth {
                stepped.apply(crate::StepCmd::Undo).expect("undo");
            }
            while stepped.history().undo_depth() < depth {
                stepped.apply(crate::StepCmd::Redo).expect("redo");
            }
            assert_eq!(scrubbed.take_damage(), stepped.take_damage(), "{depth}");
            assert!(scrubbed.doc() == stepped.doc(), "{depth}");
            assert_eq!(
                (scrubbed.layer, scrubbed.frame, scrubbed.revision()),
                (stepped.layer, stepped.frame, stepped.revision())
            );
        }
    }
}
