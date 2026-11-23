//! The ownership rules of section 6 of the creative tools plan, one test each: duplicating,
//! links, canvas changes, colour modes, merging, groups, linting and the screen.

use super::fixture::{cell, corner, doc_with_map, editor_with_map, id, map_cel, tile_image};
use crate::cel_drag::{CelDrag, DragKind, Landing};
use crate::command::CanvasCmd;
use crate::command::TileCmd;
use crate::document::{ColorMode, LimitError};
use crate::layer::LayerKind;
use crate::tileset::TilesetId;
use crate::{CelCmd, Editor, FrameCmd, LayerCmd};

fn drag(kind: DragKind, from: usize, to: usize) -> CelCmd {
    CelCmd::Drag(CelDrag {
        layers: (from, from),
        frames: (0, 0),
        to_layer: to,
        to_frame: 0,
        kind,
        landing: Landing::Overwrite,
    })
}

/// A second tileset with a tile and a tilemap layer on it at the top, so layers 1 and 2 are
/// tilemap layers of different tilesets and layer 3 is a tilemap layer of the first.
fn two_tilesets() -> (Editor, TilesetId, TilesetId) {
    let (mut e, first) = editor_with_map();
    let made = e
        .apply(TileCmd::NewTileset {
            name: "Floor".into(),
            mode: ColorMode::Rgba,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("room");
    let second = TilesetId::new(made.created.expect("id"));
    e.apply(TileCmd::AddTile {
        tileset: second,
        from: crate::command::TileSource::Image(tile_image(0xffff_0000)),
    })
    .expect("fits");
    e.apply(TileCmd::NewLayer {
        at: 2,
        tileset: second,
    })
    .expect("room");
    e.apply(TileCmd::NewLayer {
        at: 3,
        tileset: first,
    })
    .expect("room");
    (e, first, second)
}

#[test]
fn a_duplicate_of_a_tilemap_layer_uses_the_same_tileset_and_shares_the_maps() {
    let (mut e, set) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::Duplicate { layer }).expect("copies");
    let layers = e.doc().layers();
    assert_eq!(layers[2].kind, LayerKind::Tilemap(set));
    assert_eq!(layers[2].mode, layers[1].mode);
    assert!(layers[2].cel(0).same_image(layers[1].cel(0)));
    assert_eq!(e.doc().tileset_users(set), 2);
    assert_eq!(e.doc().check_tilemaps(), Ok(()));
}

#[test]
fn links_join_tilemap_layers_of_one_tileset_and_no_other_layers() {
    let (mut e, first, _) = two_tilesets();
    // Layer 3 uses the same tileset as layer 1.
    e.apply(drag(DragKind::Link, 1, 3)).expect("same tileset");
    let doc = e.doc();
    assert!(doc.layers()[3].cel(0).same_image(doc.layers()[1].cel(0)));
    assert!(doc.links_across(1, 0));
    assert_eq!(doc.check_links(), Ok(()));
    assert_eq!(doc.layers()[3].tileset(), Some(first));
}

#[test]
fn a_link_across_tilesets_or_to_a_paint_layer_is_refused() {
    let (mut e, _, _) = two_tilesets();
    let before = e.doc().clone();
    assert_eq!(
        e.apply(drag(DragKind::Link, 1, 2)),
        Err(LimitError::LinkTileset)
    );
    assert_eq!(
        e.apply(drag(DragKind::Link, 1, 0)),
        Err(LimitError::LinkTileset)
    );
    assert_eq!(
        e.apply(drag(DragKind::Link, 0, 1)),
        Err(LimitError::LinkTileset)
    );
    assert_eq!(e.doc(), &before);
}

#[test]
fn moved_and_copied_map_cels_land_only_on_a_layer_of_the_same_tileset() {
    let (mut e, _, _) = two_tilesets();
    let before = e.doc().clone();
    for kind in [DragKind::Move, DragKind::Copy] {
        for to in [0, 2] {
            assert_eq!(
                e.apply(drag(kind, 1, to)),
                Err(LimitError::TilemapCels),
                "{kind:?} to {to}"
            );
        }
        assert_eq!(
            e.apply(drag(kind, 0, 1)),
            Err(LimitError::TilemapCels),
            "{kind:?} a paint cel onto a map"
        );
    }
    assert_eq!(e.doc(), &before);
    e.apply(drag(DragKind::Copy, 1, 3)).expect("same tileset");
    assert!(e.doc().layers()[3]
        .cel(0)
        .same_image(e.doc().layers()[1].cel(0)));
    assert_eq!(e.doc().check_links(), Ok(()));
}

#[test]
fn holds_work_on_a_tilemap_layer_as_on_a_paint_layer() {
    let (mut e, _) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    e.apply(FrameCmd::HoldPrevious { layer, frame: 1 })
        .expect("holds");
    let cels = e.doc().layers()[1].cels();
    assert!(cels[0].same_image(&cels[1]));
    e.apply(FrameCmd::Release {
        layer,
        from: 0,
        to: 1,
    })
    .expect("releases");
    assert!(!e.doc().is_held(1, 0));
    assert_eq!(e.doc().check_links(), Ok(()));
}

#[test]
fn making_a_tileset_independent_leaves_the_links_to_other_layers() {
    let (mut e, _, _) = two_tilesets();
    e.apply(drag(DragKind::Link, 1, 3)).expect("same tileset");
    let layer = e.layer_id(3).expect("layer");
    e.apply(TileCmd::CopyTilesetFor { layer }).expect("room");
    let doc = e.doc();
    assert!(!doc.links_across(3, 0), "the link to layer 1 ended");
    assert!(!doc.links_across(1, 0));
    assert_eq!(doc.check_links(), Ok(()));
    assert_eq!(doc.check_tilemaps(), Ok(()));
    // Each still shows the map it had.
    assert_eq!(
        corner(doc.layers()[3].cel(0)),
        corner(doc.layers()[1].cel(0))
    );
}

#[test]
fn a_link_between_layers_of_different_tilesets_breaks_the_link_rule() {
    let (mut e, _, _) = two_tilesets();
    let doc = e.doc().clone();
    let mut ids: Vec<Vec<u32>> = doc
        .layers()
        .iter()
        .map(|l| vec![0; l.frame_count()])
        .collect();
    ids[1][0] = 1;
    ids[2][0] = 1;
    let mut loaded = doc.clone();
    // A file naming such a link has it taken apart, not kept.
    assert_eq!(loaded.set_links(&ids), Ok(1));
    assert_eq!(loaded.check_links(), Ok(()));
    let _ = &mut e;
}

#[test]
fn the_canvas_resize_moves_the_maps_with_the_pixels() {
    let (mut e, _) = editor_with_map();
    let before = corner(e.doc().layers()[1].cel(0));
    let at = e.doc().layers()[1].cel(0).position();
    e.apply(CanvasCmd::Resize {
        width: 24,
        height: 20,
        dx: 6,
        dy: 3,
    })
    .expect("fits");
    let cel = e.doc().layers()[1].cel(0);
    assert_eq!(cel.position(), (at.0 + 6, at.1 + 3));
    assert_eq!(
        corner(cel),
        before,
        "the cells are the same, so the grid moved whole"
    );
    assert_eq!(e.doc().check_tilemaps(), Ok(()));
    assert!(e.undo());
    assert_eq!(e.doc().layers()[1].cel(0).position(), at);
}

#[test]
fn a_tilemap_layers_colour_mode_is_not_converted() {
    let (mut e, _) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    let before = e.doc().clone();
    let depth = e.history().undo_depth();
    assert_eq!(
        e.apply(LayerCmd::SetMode {
            layer,
            mode: ColorMode::Palette
        }),
        Err(LimitError::TilemapMode)
    );
    assert_eq!(e.doc(), &before);
    assert_eq!(e.history().undo_depth(), depth);
}

#[test]
fn merging_down_is_refused_onto_a_tilemap_layer_and_draws_a_map_onto_paint() {
    let (mut e, _) = editor_with_map();
    e.apply(LayerCmd::Add {
        at: 2,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    let before = e.doc().clone();
    let top = e.layer_id(2).expect("layer");
    // A paint layer onto the map below it.
    assert_eq!(
        e.apply(LayerCmd::MergeDown { layer: top }),
        Err(LimitError::TilemapMerge)
    );
    assert_eq!(e.doc(), &before);
    // The map onto the paint layer below it draws it as pixels, in one step.
    let map = e.layer_id(1).expect("layer");
    let depth = e.history().undo_depth();
    e.apply(LayerCmd::MergeDown { layer: map }).expect("merges");
    assert_eq!(e.history().undo_depth(), depth + 1);
    assert_eq!(e.doc().layers().len(), 2);
    assert!(e.doc().layers().iter().all(|l| !l.is_tilemap()));
    assert_eq!(e.doc().check_tilemaps(), Ok(()));
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
}

#[test]
fn a_tilemap_layer_goes_into_a_group_and_comes_out_with_its_tileset() {
    let (mut e, set) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::Group { layer }).expect("groups");
    let doc = e.doc();
    assert_eq!(doc.layers()[1].depth, 1);
    assert_eq!(doc.layers()[1].tileset(), Some(set));
    let group = e.layer_id(2).expect("group");
    let before = e.doc().clone();
    e.apply(LayerCmd::Ungroup { layer: group })
        .expect("ungroups");
    assert_eq!(e.doc().layers()[1].depth, 0);
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    // A duplicate of the group copies the layer with it, on the same tileset.
    let group = e.layer_id(2).expect("group");
    e.apply(LayerCmd::Duplicate { layer: group })
        .expect("copies");
    assert_eq!(e.doc().tileset_users(set), 2);
    assert_eq!(e.doc().check_tilemaps(), Ok(()));
}

#[test]
fn a_shown_tilemap_layer_is_on_the_screen_and_in_the_picture() {
    let (doc, _) = doc_with_map();
    // Its tiles are sampled like pixels, so it is drawn and exported like a paint layer.
    assert_eq!(doc.screen_layers(), vec![true, true]);
    assert_eq!(doc.picture_layers(), vec![true, true]);
    assert!(doc.layers()[1].in_picture());
}

#[test]
fn pixel_lint_skips_a_tilemap_layer() {
    let (doc, _) = doc_with_map();
    // A stray pixel on a paint layer is found; the cells of a map are never pixels.
    let mut noisy = doc.clone();
    let mut cells = vec![0u32; 16 * 16];
    cells[5 * 16 + 5] = 0xff00_00ff;
    noisy
        .set_cel(0, 0, crate::grid::Grid::from_cells(16, 16, &cells))
        .expect("fits");
    let found = |d: &crate::Document, layer: usize| {
        crate::lint_layer(&d.layers()[layer], 0, (16, 16), |c| c)
    };
    assert!(!found(&noisy, 0).is_empty());
    assert!(found(&noisy, 1).is_empty());
}

#[test]
fn the_picker_the_wand_select_opaque_and_copy_read_what_a_map_draws() {
    let (mut e, _) = editor_with_map();
    e.apply(crate::SelectionCmd::All).expect("selects");
    assert_eq!(e.color_at(1, 1), 0xff00_00ff, "tile 1, not its cell value");
    // Frame 0 places `1 2 / 2 1` of 4 by 4 tiles: red, green, green, red.
    let joined = e.wand_shape(1, 1, true).expect("on the canvas");
    assert_eq!(
        joined.count(),
        16,
        "one red tile; the other only meets it at a corner"
    );
    assert!(joined.contains(3, 3) && !joined.contains(4, 4));
    let anywhere = e.wand_shape(1, 1, false).expect("on the canvas");
    assert_eq!(anywhere.count(), 32);
    assert!(anywhere.contains(5, 5) && !anywhere.contains(5, 1));
    let clear = e.wand_shape(12, 12, true).expect("on the canvas");
    assert_eq!(clear.count(), 16 * 16 - 64, "nothing drawn past the cells");
    let opaque = e.opaque_shape();
    assert_eq!(
        (opaque.count(), opaque.trimmed().bounds()),
        (64, (0, 0, 8, 8))
    );
    let clip = e.copy_selected().expect("a selection");
    assert_eq!((clip.width, clip.height), (16, 16));
    let at = |x: usize, y: usize| clip.colors[y * 16 + x];
    assert_eq!(
        (at(1, 1), at(5, 1), at(6, 6), at(9, 9)),
        (0xff00_00ff, 0xff00_ff00, 0xff00_00ff, 0)
    );
    assert_eq!(clip.ids, None, "an RGBA map copies colours");
    let _ = (cell(1), id(1), map_cel([0; 4]));
}

#[test]
fn a_frame_without_cells_reads_as_nothing_drawn() {
    let (mut e, _) = editor_with_map();
    e.frame = 2;
    assert!(e.opaque_shape().is_empty());
    assert_eq!(e.wand_shape(1, 1, true).map(|m| m.count()), Some(256));
}
