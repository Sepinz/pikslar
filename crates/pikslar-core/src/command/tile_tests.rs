//! The tileset and tile commands: one step each, undone and redone to the same document.

use super::{StepCmd, TileCmd, TileSource};
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::Editor;
use crate::grid::Grid;
use crate::image::Image;
use crate::tileset::{TileId, TilesetId, MAX_TILESETS};
use std::sync::Arc;

fn editor() -> Editor {
    Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("size"))
}

fn picture(value: u32) -> Arc<Image> {
    Arc::new(Grid::from_cells(4, 4, &[value; 16]).into_image())
}

fn new_tileset(e: &mut Editor) -> TilesetId {
    let made = e
        .apply(TileCmd::NewTileset {
            name: "Walls".into(),
            mode: ColorMode::Rgba,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("room");
    TilesetId::new(made.created.expect("an id"))
}

fn add_tile(e: &mut Editor, tileset: TilesetId, from: TileSource) -> TileId {
    let made = e.apply(TileCmd::AddTile { tileset, from }).expect("fits");
    TileId::new(made.created.expect("an id")).expect("a tile id")
}

fn order(e: &Editor, set: TilesetId) -> Vec<u32> {
    let tiles = e.doc().tileset(set).expect("there").tiles();
    tiles.iter().map(|t| t.id.get()).collect()
}

/// Runs each command, checking after it that it made one step, then undoes them all, checking
/// the document after each, and redoes them all.
fn check_each_is_one_undoable_step(e: &mut Editor, commands: Vec<TileCmd>) {
    let mut states = vec![e.doc().clone()];
    for command in commands {
        let depth = e.history().undo_depth();
        let outcome = e.apply(command.clone()).expect("runs");
        assert!(outcome.changed, "{command:?}");
        assert_eq!(e.history().undo_depth(), depth + 1, "{command:?}");
        states.push(e.doc().clone());
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
fn every_tile_command_is_one_step_undone_and_redone_exactly() {
    let mut e = editor();
    let set = new_tileset(&mut e);
    let (a, b) = (TileId::new(1).expect("id"), TileId::new(2).expect("id"));
    let steps = vec![
        TileCmd::AddTile {
            tileset: set,
            from: TileSource::Image(picture(5)),
        },
        TileCmd::AddTile {
            tileset: set,
            from: TileSource::Blank,
        },
        TileCmd::DuplicateTile {
            tileset: set,
            tile: a,
        },
        TileCmd::MoveTile {
            tileset: set,
            tile: b,
            to: 0,
        },
        TileCmd::SetTileImage {
            tileset: set,
            tile: b,
            image: picture(9),
        },
        TileCmd::RenameTileset {
            tileset: set,
            name: "Floors".into(),
        },
        TileCmd::RemoveTile {
            tileset: set,
            tile: a,
        },
        TileCmd::RemoveTileset { tileset: set },
        TileCmd::NewTileset {
            name: String::new(),
            mode: ColorMode::Palette,
            tile_w: 8,
            tile_h: 16,
        },
    ];
    check_each_is_one_undoable_step(&mut e, steps);
}

#[test]
fn ids_are_not_reused_even_after_undo_and_redo() {
    let mut e = editor();
    let first = new_tileset(&mut e);
    let a = add_tile(&mut e, first, TileSource::Blank);
    e.apply(TileCmd::RemoveTile {
        tileset: first,
        tile: a,
    })
    .expect("removed");
    let b = add_tile(&mut e, first, TileSource::Blank);
    assert_ne!(a, b, "a removed tile's id is not given again");
    e.apply(TileCmd::RemoveTileset { tileset: first })
        .expect("unused");
    let second = new_tileset(&mut e);
    assert_ne!(first, second);
    // Undone, the counters go back with the tables, and the same ids come again.
    while e.undo() {}
    assert!(e.doc().tilesets.is_empty());
    let again = new_tileset(&mut e);
    assert_eq!(again, first);
}

#[test]
fn a_new_tile_takes_the_end_and_a_copy_the_place_after_its_original() {
    let mut e = editor();
    let set = new_tileset(&mut e);
    let a = add_tile(&mut e, set, TileSource::Image(picture(3)));
    let b = add_tile(&mut e, set, TileSource::Blank);
    let made = e
        .apply(TileCmd::DuplicateTile {
            tileset: set,
            tile: a,
        })
        .expect("there");
    let copy = made.created.expect("an id");
    assert_eq!(order(&e, set), [a.get(), copy, b.get()]);
    let tiles = e.doc().tileset(set).expect("there");
    let (original, copied) = (&tiles.tiles()[0], &tiles.tiles()[1]);
    assert!(
        Arc::ptr_eq(&original.image, &copied.image),
        "the copy shares the pixels until one is edited"
    );
}

#[test]
fn commands_that_change_nothing_make_no_step() {
    let mut e = editor();
    let set = new_tileset(&mut e);
    let a = add_tile(&mut e, set, TileSource::Image(picture(3)));
    let depth = e.history().undo_depth();
    let same: Vec<TileCmd> = vec![
        TileCmd::RenameTileset {
            tileset: set,
            name: "Walls".into(),
        },
        TileCmd::RenameTileset {
            tileset: set,
            name: "  \n".into(),
        },
        TileCmd::MoveTile {
            tileset: set,
            tile: a,
            to: 5,
        },
        TileCmd::SetTileImage {
            tileset: set,
            tile: a,
            image: picture(3),
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
    let mut e = editor();
    let set = new_tileset(&mut e);
    let before = e.doc().clone();
    for name in ["F", "Fl", "Flo", "Floor"] {
        e.apply(TileCmd::RenameTileset {
            tileset: set,
            name: name.into(),
        })
        .expect("renamed");
    }
    e.apply(StepCmd::EndDrag).expect("ended");
    assert_eq!(e.doc().tileset(set).expect("there").name(), "Floor");
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    assert!(e.redo());
    assert_eq!(e.doc().tileset(set).expect("there").name(), "Floor");
}

#[test]
fn a_refused_command_leaves_the_document_and_history_as_they_were() {
    let mut e = editor();
    let set = new_tileset(&mut e);
    let gone = TileId::new(40).expect("id");
    let big = Arc::new(Grid::from_cells(5, 4, &[1; 20]).into_image());
    let refused: Vec<(TileCmd, LimitError)> = vec![
        (
            TileCmd::NewTileset {
                name: "x".into(),
                mode: ColorMode::Rgba,
                tile_w: 0,
                tile_h: 8,
            },
            LimitError::TileSize,
        ),
        (
            TileCmd::NewTileset {
                name: "x".into(),
                mode: ColorMode::Rgba,
                tile_w: 8,
                tile_h: 257,
            },
            LimitError::TileSize,
        ),
        (
            TileCmd::AddTile {
                tileset: set,
                from: TileSource::Image(big.clone()),
            },
            LimitError::TileSize,
        ),
        (
            TileCmd::AddTile {
                tileset: TilesetId::new(77),
                from: TileSource::Blank,
            },
            LimitError::NoSuchItem,
        ),
        (
            TileCmd::RemoveTile {
                tileset: set,
                tile: gone,
            },
            LimitError::NoSuchItem,
        ),
        (
            TileCmd::DuplicateTile {
                tileset: set,
                tile: gone,
            },
            LimitError::NoSuchItem,
        ),
        (
            TileCmd::SetTileImage {
                tileset: set,
                tile: gone,
                image: picture(1),
            },
            LimitError::NoSuchItem,
        ),
        (
            TileCmd::RemoveTileset {
                tileset: TilesetId::new(77),
            },
            LimitError::NoSuchItem,
        ),
    ];
    let (doc, depth) = (e.doc().clone(), e.history().undo_depth());
    for (command, error) in refused {
        assert_eq!(e.apply(command.clone()), Err(error), "{command:?}");
        assert_eq!(e.doc(), &doc, "{command:?}");
        assert_eq!(e.history().undo_depth(), depth, "{command:?}");
    }
    let a = add_tile(&mut e, set, TileSource::Blank);
    let refused = e.apply(TileCmd::SetTileImage {
        tileset: set,
        tile: a,
        image: big,
    });
    assert_eq!(refused, Err(LimitError::TileSize));
}

#[test]
fn at_most_64_tilesets() {
    let mut e = editor();
    for _ in 0..MAX_TILESETS {
        new_tileset(&mut e);
    }
    let (doc, depth) = (e.doc().clone(), e.history().undo_depth());
    let more = e.apply(TileCmd::NewTileset {
        name: "one too many".into(),
        mode: ColorMode::Rgba,
        tile_w: 8,
        tile_h: 8,
    });
    assert_eq!(more, Err(LimitError::TooManyTilesets));
    assert_eq!((e.doc(), e.history().undo_depth()), (&doc, depth));
}

#[test]
fn a_palette_tileset_takes_only_colours_the_palette_has() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Palette).expect("size"));
    let made = e
        .apply(TileCmd::NewTileset {
            name: "Indexed".into(),
            mode: ColorMode::Palette,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("room");
    let set = TilesetId::new(made.created.expect("an id"));
    let unknown = e.apply(TileCmd::AddTile {
        tileset: set,
        from: TileSource::Image(picture(3)),
    });
    assert_eq!(
        unknown,
        Err(LimitError::NoSuchItem),
        "the palette has no colour 3"
    );
    for color in [0xff00_00ff, 0xff00_ff00, 0xffff_0000] {
        e.apply(crate::PaletteCmd::AddSwatch { color })
            .expect("room");
    }
    let known = e.apply(TileCmd::AddTile {
        tileset: set,
        from: TileSource::Image(picture(3)),
    });
    assert!(known.is_ok_and(|o| o.changed));
}

#[test]
fn editing_a_tile_swaps_its_image_and_undo_brings_the_old_one_back_without_a_copy() {
    let mut e = editor();
    let set = new_tileset(&mut e);
    let original = picture(4);
    let a = add_tile(&mut e, set, TileSource::Image(original.clone()));
    let revision = e.revision();
    e.apply(TileCmd::SetTileImage {
        tileset: set,
        tile: a,
        image: picture(8),
    })
    .expect("edited");
    assert!(e.revision() > revision, "the file changed");
    let shown = |e: &Editor| {
        e.doc()
            .tileset(set)
            .expect("there")
            .tile(a)
            .expect("a")
            .image
            .clone()
    };
    assert_eq!(shown(&e).get(0, 0), 8);
    assert!(e.undo());
    assert!(
        Arc::ptr_eq(&shown(&e), &original),
        "the same image, not a copy"
    );
    assert!(e.redo());
    assert_eq!(shown(&e).get(3, 3), 8);
}

#[test]
fn tile_commands_never_need_the_paint_check() {
    // A locked layer does not stop a tileset from changing: no layer is drawn on.
    let mut e = editor();
    let layer = e.layer_id(0).expect("layer");
    e.apply(crate::LayerCmd::SetLocked {
        layer,
        locked: true,
    })
    .expect("locked");
    assert!(e
        .apply(TileCmd::NewTileset {
            name: "x".into(),
            mode: ColorMode::Rgba,
            tile_w: 8,
            tile_h: 8
        })
        .is_ok());
}

#[test]
fn a_tile_step_made_with_floating_pixels_up_undoes_with_the_rest() {
    let mut e = editor();
    let set = new_tileset(&mut e);
    let a = add_tile(&mut e, set, TileSource::Blank);
    e.apply(crate::SelectionCmd::All).expect("selected");
    e.apply(crate::FloatingCmd::Lift { copy: true })
        .expect("lifted");
    e.apply(TileCmd::SetTileImage {
        tileset: set,
        tile: a,
        image: picture(2),
    })
    .expect("edited");
    while e.undo() {}
    assert!(
        e.doc().tilesets.is_empty(),
        "everything is undone, in order"
    );
    while e.redo() {}
    let image = &e
        .doc()
        .tileset(set)
        .expect("there")
        .tile(a)
        .expect("a")
        .image;
    assert_eq!(image.get(0, 0), 2);
}
