//! Tilesets and tilemap layers in a project file: round trips, the refused and noted cases, and a
//! mutation run over the `TSET` section.

use crate::piks::{load, load_noted, load_recovery, save, save_recovery, try_save};
use crate::piks_tilesets::TILESETS;
use crate::{crc32, FormatError};
use pikslar_core::orient::{Orientation, FLIPPED_DIAGONALLY, FLIPPED_HORIZONTALLY};
use pikslar_core::tilemap::MapCell;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{Cel, ColorMode, Document, Grid, Image};
use std::sync::Arc;

const SIDE: usize = 4;

fn id(n: u32) -> TileId {
    TileId::new(n).expect("an id")
}

fn cell(n: u32) -> u32 {
    MapCell::place(id(n), Orientation::IDENTITY).bits()
}

fn tile_image(color: u32) -> Arc<Image> {
    Arc::new(Grid::from_cells(SIDE, SIDE, &[color; SIDE * SIDE]).into_image())
}

fn map_cel(cells: [u32; 4]) -> Cel {
    Cel::new(Grid::from_cells(2, 2, &cells))
}

/// A paint layer and a tilemap layer of tileset "Walls" (tiles 1 and 2) over two frames.
fn with_map() -> Document {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    doc.add_frame(1, false).expect("room");
    let set = doc
        .add_tileset("Walls", ColorMode::Rgba, (SIDE, SIDE))
        .expect("room");
    doc.add_tile(set, tile_image(0xff00_00ff)).expect("fits");
    doc.add_tile(set, tile_image(0xff00_ff00)).expect("fits");
    doc.add_tilemap_layer(1, set).expect("room");
    let (a, b) = (cell(1), cell(2));
    doc.put_cel(1, 0, map_cel([a, b, b, a])).expect("fits");
    doc.put_cel(1, 1, map_cel([b, b, 0, a])).expect("fits");
    doc
}

fn resealed(mut bytes: Vec<u8>) -> Vec<u8> {
    let end = bytes.len() - 4;
    let crc = crc32(&[&bytes[..end]]);
    bytes[end..].copy_from_slice(&crc.to_le_bytes());
    bytes
}

/// Where the section's data begins: after its tag and length.
fn data_at(bytes: &[u8]) -> usize {
    let tag = bytes
        .windows(4)
        .rposition(|w| w == TILESETS)
        .expect("a TSET section");
    tag + 8
}

/// Offsets of fields in the section of [`with_map`], from the start of its data.
const NEXT_ID: usize = 0;
const COUNT: usize = 4;
const FIRST_ID: usize = 12;
const MODE: usize = 23;
const NEXT_TILE: usize = 28;
const FIRST_TILE: usize = 36;

fn patched(bytes: &[u8], at: usize, value: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let start = data_at(bytes) + at;
    out[start..start + value.len()].copy_from_slice(value);
    resealed(out)
}

fn refused(bytes: &[u8]) -> bool {
    matches!(
        load(bytes),
        Err(FormatError::Corrupt(_) | FormatError::Unsupported(_) | FormatError::FileTooLarge)
    )
}

#[test]
fn tilesets_and_maps_survive_save_and_load_byte_for_byte() {
    let doc = with_map();
    let bytes = try_save(&doc).expect("writable");
    assert_eq!(bytes[8..12], 4u32.to_le_bytes(), "required feature bit 2");
    let (back, notes) = load_noted(&bytes).expect("opens");
    assert!(notes.can_save_over(), "{notes:?}");
    assert_eq!(*back.tilesets, *doc.tilesets);
    assert_eq!(back.layers()[1].kind, doc.layers()[1].kind);
    back.check_tilemaps().expect("the rules hold");
    for frame in 0..2 {
        let (a, b) = (doc.layers()[1].cel(frame), back.layers()[1].cel(frame));
        assert_eq!(a.image().to_cells(2, 2), b.image().to_cells(2, 2));
    }
    assert_eq!(save(&back), bytes, "saved again, byte for byte");
}

#[test]
fn a_document_without_tilesets_sets_no_bit_and_writes_no_section() {
    let doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let bytes = save(&doc);
    assert_eq!(bytes[8..12], 0u32.to_le_bytes());
    assert!(!bytes.windows(4).any(|w| w == TILESETS));
}

#[test]
fn counters_survive_so_ids_are_never_reused() {
    let mut doc = with_map();
    let set = doc.tilesets.iter().next().expect("one").id();
    doc.remove_tile(set, id(2)).expect("removed");
    let other = doc
        .add_tileset("Spare", ColorMode::Palette, (8, 4))
        .expect("room");
    doc.remove_tileset(other).expect("unused");
    let back = load(&save(&doc)).expect("opens");
    let kept = back.tilesets.get(set).expect("the tileset");
    assert_eq!(kept.next_tile_id(), 3, "tile 2 is not given again");
    assert_eq!(back.tilesets.next_id(), 3, "tileset 2 is not given again");
    assert_eq!(*back.tilesets, *doc.tilesets);
    assert_eq!(save(&back), save(&doc));
}

#[test]
fn a_counter_with_no_tileset_left_is_kept_and_needs_no_feature() {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Gone", ColorMode::Rgba, (8, 8))
        .expect("room");
    doc.remove_tileset(set).expect("unused");
    let bytes = save(&doc);
    assert_eq!(bytes[8..12], 0u32.to_le_bytes(), "nothing for an old build");
    assert_eq!(load(&bytes).expect("opens").tilesets.next_id(), 2);
}

#[test]
fn a_tileset_with_no_layer_and_palette_tilesets_round_trip() {
    let mut doc = Document::new(16, 16, ColorMode::Palette).expect("size");
    let set = doc
        .add_tileset("Tiles ü", ColorMode::Palette, (16, 8))
        .expect("room");
    let image = Image::blank();
    doc.add_tile(set, Arc::new(image)).expect("fits");
    doc.add_tilemap_layer(1, set).expect("room");
    let back = load(&save(&doc)).expect("opens");
    assert_eq!(*back.tilesets, *doc.tilesets);
    assert_eq!(back.layers()[1].mode, ColorMode::Palette);
}

#[test]
fn a_recovery_copy_carries_the_tilesets() {
    let doc = with_map();
    let bytes = save_recovery(&doc, None);
    let back = load_recovery(&bytes).expect("opens");
    assert_eq!(*back.doc.tilesets, *doc.tilesets);
    assert!(back.notes.can_save_over());
}

#[test]
fn an_older_build_refuses_the_required_bit_untouched() {
    // What a build of M7 and earlier does: every bit it does not know refuses the file.
    let known = 1u32 | 2;
    let bytes = save(&with_map());
    let features = u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"));
    assert_ne!(features & !known, 0);
}

#[test]
fn a_layer_naming_a_missing_tileset_is_refused() {
    let bytes = save(&with_map());
    let bytes = patched(&bytes, FIRST_ID, &7u32.to_le_bytes());
    assert!(refused(&patched(&bytes, NEXT_ID, &8u32.to_le_bytes())));
}

#[test]
fn a_layer_and_tileset_of_different_modes_are_refused() {
    let bytes = save(&with_map());
    assert!(refused(&patched(&bytes, MODE, &[0])));
}

#[test]
fn duplicate_ids_and_ids_past_their_counters_are_refused() {
    let bytes = save(&with_map());
    assert!(refused(&patched(&bytes, NEXT_TILE, &2u32.to_le_bytes())));
    assert!(refused(&patched(&bytes, NEXT_ID, &1u32.to_le_bytes())));
    // A tile is its id, 9 bytes of image header and a 1-bit packed storage tile of 36 bytes
    // after the 8-byte table: the second id is 57 bytes on.
    assert!(refused(&patched(
        &bytes,
        FIRST_TILE + 57,
        &1u32.to_le_bytes()
    )));
    assert!(refused(&patched(&bytes, FIRST_TILE, &0u32.to_le_bytes())));
}

#[test]
fn a_damaged_section_is_refused_whole() {
    let bytes = save(&with_map());
    assert!(refused(&patched(&bytes, COUNT, &99u32.to_le_bytes())));
    assert!(refused(&patched(&bytes, COUNT, &0u32.to_le_bytes())));
    // A tile image of the map kind, and a mode this version does not know.
    assert!(refused(&patched(&bytes, FIRST_TILE + 4, &[1])));
    assert!(refused(&patched(&bytes, MODE, &[9])));
    // A name that is not text.
    assert!(refused(&patched(&bytes, 18, &[0xff])));
}

#[test]
fn a_tile_image_bigger_than_its_tile_is_refused() {
    let bytes = save(&with_map());
    // A table 17 tiles wide cannot lie inside any tile.
    assert!(refused(&patched(
        &bytes,
        FIRST_TILE + 9,
        &17u16.to_le_bytes()
    )));
    assert!(
        refused(&patched(&bytes, 24, &[2, 0])),
        "a tile narrower than its image"
    );
}

#[test]
fn a_cell_naming_a_missing_tile_is_made_empty_with_a_note() {
    let mut doc = with_map();
    doc.put_cel(1, 0, map_cel([cell(1), cell(9), 0, cell(2)]))
        .expect("fits");
    let (back, notes) = load_noted(&save(&doc)).expect("opens");
    assert!(!notes.can_save_over());
    assert!(notes.damaged.contains(&"tilemap cells"));
    let cells = back.layers()[1].cel(0).image().to_cells(2, 2);
    assert_eq!(cells, [cell(1), 0, 0, cell(2)]);
    back.check_tilemaps().expect("the rules hold");
}

#[test]
fn flag_bits_on_an_empty_cell_are_cleared_with_a_note() {
    let mut doc = with_map();
    doc.put_cel(1, 0, map_cel([FLIPPED_HORIZONTALLY, cell(1), 0, 0]))
        .expect("fits");
    let (back, notes) = load_noted(&save(&doc)).expect("opens");
    assert!(!notes.can_save_over());
    let cells = back.layers()[1].cel(0).image().to_cells(2, 2);
    assert_eq!(cells, [0, cell(1), 0, 0]);
}

#[test]
fn a_diagonal_flip_on_oblong_tiles_is_dropped_with_a_note() {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Oblong", ColorMode::Rgba, (6, 4))
        .expect("room");
    let image = Grid::from_cells(6, 4, &[0xff00_00ff; 24]).into_image();
    doc.add_tile(set, Arc::new(image)).expect("fits");
    doc.add_tilemap_layer(1, set).expect("room");
    let turned = cell(1) | FLIPPED_DIAGONALLY | FLIPPED_HORIZONTALLY;
    doc.put_cel(1, 0, map_cel([turned, cell(1), 0, 0]))
        .expect("fits");
    let (back, notes) = load_noted(&save(&doc)).expect("opens");
    assert!(!notes.can_save_over());
    let cells = back.layers()[1].cel(0).image().to_cells(2, 2);
    assert_eq!(cells, [cell(1) | FLIPPED_HORIZONTALLY, cell(1), 0, 0]);
    back.check_tilemaps().expect("the rules hold");
}

#[test]
fn a_map_layer_with_a_paint_image_is_refused() {
    let bytes = save(&with_map());
    // The layer record's kind byte (3) read as paint: its image is a map.
    let at = bytes
        .windows(7)
        .position(|w| w == [0, 3, 0, 1, 0, 0, 0])
        .expect("the tilemap layer's flags, kind, depth and tileset");
    let mut other = bytes.clone();
    other[at + 1] = 0;
    assert!(refused(&resealed(other)));
}

#[test]
fn tilesets_without_their_feature_are_refused() {
    let mut bytes = save(&with_map());
    bytes[8..12].copy_from_slice(&0u32.to_le_bytes());
    assert!(refused(&resealed(bytes)));
}

#[test]
fn a_tile_that_is_shared_in_memory_is_written_and_read_whole() {
    let mut doc = with_map();
    let set = doc.tilesets.iter().next().expect("one").id();
    doc.duplicate_tile(set, id(1)).expect("room");
    let back = load(&save(&doc)).expect("opens");
    assert_eq!(*back.tilesets, *doc.tilesets);
    assert_eq!(back.tilesets.get(TilesetId::new(1)).expect("set").len(), 3);
}

#[test]
fn bytes_after_a_known_record_are_skipped_with_a_note() {
    let bytes = save(&with_map());
    let at = data_at(&bytes);
    let rec_len = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().expect("four"));
    let section_len_at = at - 4;
    let end = at + 12 + rec_len as usize;
    let mut longer = bytes.clone();
    longer.splice(end..end, [7, 7]);
    let patch = |buf: &mut Vec<u8>, pos: usize, add: u32| {
        let v = u32::from_le_bytes(buf[pos..pos + 4].try_into().expect("four")) + add;
        buf[pos..pos + 4].copy_from_slice(&v.to_le_bytes());
    };
    patch(&mut longer, at + 8, 2);
    patch(&mut longer, section_len_at, 2);
    let (back, notes) = load_noted(&resealed(longer)).expect("opens");
    assert!(notes.newer_fields);
    assert_eq!(*back.tilesets, *with_map().tilesets);
}

#[test]
fn no_mutation_of_the_section_panics() {
    let bytes = save(&with_map());
    let from = data_at(&bytes);
    let to = bytes.len() - 4;
    let mut opened = 0;
    for at in from..to {
        for value in [0u8, 1, 0x7f, 0x80, 0xff, bytes[at] ^ 0x10] {
            let mut mutated = bytes.clone();
            mutated[at] = value;
            if let Ok((doc, _)) = load_noted(&resealed(mutated)) {
                doc.check_tilemaps()
                    .expect("a loaded document keeps the rules");
                let _ = save(&doc);
                opened += 1;
            }
        }
    }
    assert!(opened > 0, "some mutations are harmless");
    for cut in from..to {
        let _ = load(&resealed(
            bytes[..cut].iter().chain(&bytes[to..]).copied().collect(),
        ));
    }
}

#[test]
fn a_layer_given_a_tileset_of_its_own_from_a_shared_map_saves_and_opens() {
    let mut doc = with_map();
    let set = doc.layers()[1].tileset().expect("a map layer");
    doc.add_tilemap_layer(2, set).expect("room");
    let shown = doc.layers()[1].cel(0).clone();
    doc.put_cel(2, 0, shown).expect("fits");
    doc.copy_tileset_for(1).expect("room");
    doc.check_tilemaps().expect("the rules hold");
    let back = load(&try_save(&doc).expect("writable")).expect("opens");
    back.check_tilemaps().expect("the rules hold");
    for layer in 1..3 {
        let (a, b) = (doc.layers()[layer].cel(0), back.layers()[layer].cel(0));
        assert_eq!(a.image().to_cells(2, 2), b.image().to_cells(2, 2));
    }
}

#[test]
fn a_blank_frame_added_over_maps_of_two_tilesets_saves_and_opens() {
    let mut doc = with_map();
    let other = doc
        .add_tileset("Floor", ColorMode::Rgba, (SIDE, SIDE))
        .expect("room");
    doc.add_tile(other, tile_image(0xffff_0000)).expect("fits");
    doc.add_tilemap_layer(2, other).expect("room");
    // One blank drawing goes to every layer of the new frame.
    doc.add_frame(2, false).expect("room");
    assert!(doc.layers()[1].cel(2).same_image(doc.layers()[2].cel(2)));
    let back = load(&try_save(&doc).expect("writable")).expect("opens");
    back.check_tilemaps().expect("the rules hold");
    assert_eq!(back.layers()[2].tileset(), Some(other));
    assert!(back.layers()[1].cel(2).is_blank());
}
