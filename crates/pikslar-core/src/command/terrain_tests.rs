//! The terrain commands: one step each, undone and redone to the same document, none when
//! nothing changes, and a refusal that leaves everything as it was.

use super::super::{StepCmd, TerrainCmd, TileCmd, TileSource};
use crate::autotile::TerrainKind;
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::Editor;
use crate::tileset::{TileId, TilesetId};

fn id(n: u32) -> TileId {
    TileId::new(n).expect("a tile id")
}

/// An editor with a 4 by 4 tileset of two blank tiles, made outside history.
fn editor() -> (Editor, TilesetId) {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Ground", ColorMode::Rgba, (4, 4))
        .expect("room");
    let mut e = Editor::new(doc);
    for _ in 0..2 {
        e.apply(TileCmd::AddTile {
            tileset: set,
            from: TileSource::Blank,
        })
        .expect("fits");
    }
    (e, set)
}

fn new(tileset: TilesetId, kind: TerrainKind, template: bool) -> TerrainCmd {
    TerrainCmd::New {
        tileset,
        kind,
        name: "Grass".into(),
        template,
    }
}

/// Runs each command, checking after it that it made one step, then undoes them all, checking
/// the document after each, and redoes them all.
fn check_each_is_one_undoable_step(e: &mut Editor, commands: Vec<TerrainCmd>) {
    let mut states = vec![e.doc().clone()];
    for command in commands {
        let depth = e.history().undo_depth();
        let outcome = e.apply(command.clone()).expect("runs");
        assert!(outcome.changed, "{command:?}");
        assert_eq!(e.history().undo_depth(), depth + 1, "{command:?}");
        e.doc().check_tilemaps().expect("the rules hold");
        states.push(e.doc().clone());
        e.apply(StepCmd::EndDrag).expect("ends");
    }
    for want in states.iter().rev().skip(1) {
        assert!(e.undo());
        assert_eq!(e.doc(), want);
    }
    for want in states.iter().skip(1) {
        assert!(e.redo());
        assert_eq!(e.doc(), want);
    }
}

#[test]
fn every_terrain_command_is_one_step_undone_and_redone_exactly() {
    let (mut e, ts) = editor();
    let steps = vec![
        new(ts, TerrainKind::Edge4, false),
        new(ts, TerrainKind::Blob47, true),
        TerrainCmd::Assign {
            tileset: ts,
            set: 1,
            slot: 15,
            tile: id(2),
        },
        TerrainCmd::Clear {
            tileset: ts,
            set: 2,
            slot: 46,
        },
        TerrainCmd::Rename {
            tileset: ts,
            set: 1,
            name: "Road".into(),
        },
        TerrainCmd::Remove {
            tileset: ts,
            set: 2,
        },
        new(ts, TerrainKind::Corner4, false),
    ];
    check_each_is_one_undoable_step(&mut e, steps);
}

#[test]
fn new_says_the_id_it_gave() {
    let (mut e, ts) = editor();
    let first = e.apply(new(ts, TerrainKind::Edge4, false)).expect("room");
    let second = e.apply(new(ts, TerrainKind::Edge4, false)).expect("room");
    assert_eq!((first.created, second.created), (Some(1), Some(2)));
}

#[test]
fn a_template_is_one_step_with_its_tiles() {
    let (mut e, ts) = editor();
    let before = e.doc().clone();
    e.apply(new(ts, TerrainKind::Blob47, true)).expect("room");
    assert_eq!(e.doc().tileset(ts).expect("set").len(), 2 + 47);
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
}

#[test]
fn commands_that_change_nothing_make_no_step() {
    let (mut e, ts) = editor();
    e.apply(new(ts, TerrainKind::Edge4, false)).expect("room");
    let assign = TerrainCmd::Assign {
        tileset: ts,
        set: 1,
        slot: 0,
        tile: id(1),
    };
    e.apply(assign.clone()).expect("fills");
    let depth = e.history().undo_depth();
    let same = vec![
        assign,
        TerrainCmd::Clear {
            tileset: ts,
            set: 1,
            slot: 1,
        },
        TerrainCmd::Rename {
            tileset: ts,
            set: 1,
            name: " Grass ".into(),
        },
        TerrainCmd::Rename {
            tileset: ts,
            set: 1,
            name: "\t".into(),
        },
    ];
    for command in same {
        let outcome = e.apply(command.clone()).expect("runs");
        assert!(!outcome.changed, "{command:?}");
    }
    assert_eq!(e.history().undo_depth(), depth);
}

#[test]
fn typing_a_name_is_one_step() {
    let (mut e, ts) = editor();
    e.apply(new(ts, TerrainKind::Edge4, false)).expect("room");
    let before = e.doc().clone();
    for name in ["S", "Sa", "San", "Sand"] {
        let rename = TerrainCmd::Rename {
            tileset: ts,
            set: 1,
            name: name.into(),
        };
        e.apply(rename).expect("renamed");
    }
    e.apply(StepCmd::EndDrag).expect("ended");
    assert_eq!(e.doc().terrain(ts, 1).expect("set").name(), "Sand");
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    assert!(e.redo());
    assert_eq!(e.doc().terrain(ts, 1).expect("set").name(), "Sand");
}

#[test]
fn a_refused_command_leaves_the_document_and_history_as_they_were() {
    let (mut e, ts) = editor();
    e.apply(new(ts, TerrainKind::Edge4, false)).expect("room");
    let gone = TilesetId::new(9);
    let refused = vec![
        (new(gone, TerrainKind::Edge4, false), LimitError::NoSuchItem),
        (
            TerrainCmd::Assign {
                tileset: ts,
                set: 1,
                slot: 16,
                tile: id(1),
            },
            LimitError::TerrainSlot,
        ),
        (
            TerrainCmd::Assign {
                tileset: ts,
                set: 1,
                slot: 0,
                tile: id(3),
            },
            LimitError::NoSuchItem,
        ),
        (
            TerrainCmd::Clear {
                tileset: ts,
                set: 4,
                slot: 0,
            },
            LimitError::NoSuchItem,
        ),
        (
            TerrainCmd::Remove {
                tileset: ts,
                set: 4,
            },
            LimitError::NoSuchItem,
        ),
        (
            TerrainCmd::Rename {
                tileset: ts,
                set: 4,
                name: "x".into(),
            },
            LimitError::NoSuchItem,
        ),
    ];
    let (before, depth) = (e.doc().clone(), e.history().undo_depth());
    for (command, error) in refused {
        assert_eq!(e.apply(command.clone()), Err(error), "{command:?}");
        assert_eq!(e.doc(), &before, "{command:?}");
        assert_eq!(e.history().undo_depth(), depth, "{command:?}");
    }
}

#[test]
fn removing_a_tile_clears_its_slots_in_the_same_step() {
    let (mut e, ts) = editor();
    e.apply(new(ts, TerrainKind::Edge4, false)).expect("room");
    for slot in [2, 7] {
        let assign = TerrainCmd::Assign {
            tileset: ts,
            set: 1,
            slot,
            tile: id(1),
        };
        e.apply(assign).expect("fills");
    }
    let (before, depth) = (e.doc().clone(), e.history().undo_depth());
    let remove = TileCmd::RemoveTile {
        tileset: ts,
        tile: id(1),
    };
    e.apply(remove).expect("removed");
    assert_eq!(e.history().undo_depth(), depth + 1);
    assert_eq!(e.doc().terrain(ts, 1).expect("set").defined(), 0);
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    assert_eq!(e.doc().terrain(ts, 1).expect("set").slot(7), Some(id(1)));
}

#[test]
fn the_memory_asked_for_covers_a_template() {
    let (e, ts) = editor();
    let plain = e.terrain_command_bytes(&new(ts, TerrainKind::Blob47, false));
    let template = e.terrain_command_bytes(&new(ts, TerrainKind::Blob47, true));
    assert!(template >= plain + 47 * size_of::<crate::tileset::TileEntry>());
}
