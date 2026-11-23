//! The limit on tile pixels in a file: the reader counts as the editor does, a document the
//! editor allows saves and opens again, and Save refuses one past it as a last guard.

use crate::piks::{load, load_recovery, save, try_save, try_save_recovery, In, Out};
use crate::piks_v3::write_image;
use crate::FormatError;
use pikslar_core::tileset::{
    stored_tile_bytes, TileEntry, TileId, Tileset, TilesetId, Tilesets, MAX_TILES, MAX_TILESETS,
    MAX_TILE_BYTES, OLD_RECORD_BYTES,
};
use pikslar_core::{ColorMode, Document, Grid, Image, LimitError};
use std::sync::Arc;

const SIDE: usize = 256;

/// A tile of one colour: 256 solid storage tiles, a few bytes each in the file but each its own
/// when read.
fn solid() -> Arc<Image> {
    Arc::new(Grid::from_cells(SIDE, SIDE, &[0xff10_2030; SIDE * SIDE]).into_image())
}

#[test]
fn the_reader_charges_a_tile_what_the_editor_counts_for_it() {
    let px: Vec<u32> = (0..40 * 24).map(|i| 0xff00_0000 | (i % 7)).collect();
    let images = [
        Image::blank(),
        Image::clone(&solid()),
        Grid::from_cells(40, 24, &px).into_image(),
    ];
    for image in images {
        let mut out = Out(Vec::new());
        write_image(&mut out, &image, 0);
        let mut input = In {
            data: &out.0,
            pos: 0,
        };
        let mut spent = 0;
        super::read_tile_image(&mut input, &mut spent).expect("reads");
        assert_eq!(spent, stored_tile_bytes(&image));
    }
}

#[test]
fn a_document_filled_to_the_limit_saves_and_opens_again() {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Big", ColorMode::Rgba, (SIDE, SIDE))
        .expect("room");
    let (big, blank) = (solid(), Arc::new(Image::blank()));
    while doc.add_tile(set, big.clone()).is_ok() {}
    assert_eq!(doc.add_tile(set, big), Err(LimitError::TileMemory));
    while doc.add_tile(set, blank.clone()).is_ok() {}
    assert!(MAX_TILE_BYTES - doc.stored_tile_bytes() < stored_tile_bytes(&blank));
    let bytes = try_save(&doc).expect("within the limit");
    let back = load(&bytes).expect("opens again");
    assert_eq!(*back.tilesets, *doc.tilesets);
    let copy = try_save_recovery(&doc, None).expect("within the limit");
    load_recovery(&copy).expect("the recovery copy opens too");
}

#[test]
fn save_refuses_a_document_past_the_limit_with_a_clear_message() {
    let big = solid();
    let count = MAX_TILE_BYTES / stored_tile_bytes(&big) + 1;
    let tiles: Vec<TileEntry> = (1..=count as u32)
        .map(|n| TileEntry {
            id: TileId::new(n).expect("id"),
            image: big.clone(),
        })
        .collect();
    let id = TilesetId::new(1);
    let set = Tileset::from_parts(
        id,
        "Big".into(),
        ColorMode::Rgba,
        (SIDE, SIDE),
        count as u32 + 1,
        tiles,
    )
    .expect("a tileset");
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    doc.tilesets = Arc::new(Tilesets::from_parts(2, vec![set]).expect("a table"));
    assert!(doc.stored_tile_bytes() > MAX_TILE_BYTES);
    let refused = try_save(&doc).expect_err("past the limit");
    assert!(matches!(refused, FormatError::TilesTooLarge));
    assert!(refused.to_string().contains("416 MiB"), "{refused}");
    assert!(try_save_recovery(&doc, None).is_err());
    // What the guard prevents: a file that cannot be opened.
    assert!(matches!(load(&save(&doc)), Err(FormatError::FileTooLarge)));
}

/// The largest tileset table a build before the honest charge let be made and saved: every
/// tileset full of blank tiles, the most records there can be, which that rule charged 64 bytes
/// each, just under its 256 MiB. It must open as it did.
#[test]
fn the_largest_document_the_old_rule_saved_still_opens() {
    let blank = Arc::new(Image::blank());
    let sets: Vec<Tileset> = (1..=MAX_TILESETS as u32)
        .map(|n| {
            let tiles = (1..=MAX_TILES as u32)
                .map(|id| TileEntry {
                    id: TileId::new(id).expect("id"),
                    image: blank.clone(),
                })
                .collect();
            let (name, size) = (format!("T{n}"), (8, 8));
            let next = MAX_TILES as u32 + 1;
            Tileset::from_parts(TilesetId::new(n), name, ColorMode::Rgba, size, next, tiles)
                .expect("a tileset")
        })
        .collect();
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    doc.tilesets = Arc::new(Tilesets::from_parts(65, sets).expect("a table"));
    let records = MAX_TILESETS * MAX_TILES;
    assert!(
        records * OLD_RECORD_BYTES <= 256 << 20,
        "the old rule saved it"
    );
    assert!(doc.stored_tile_bytes() <= MAX_TILE_BYTES);
    let bytes = try_save(&doc).expect("within the limit");
    let back = load(&bytes).expect("opens");
    assert_eq!(*back.tilesets, *doc.tilesets);
}
