//! Tilesets and tilemap layers, on files built byte by byte from the public specification. No
//! file here was made by Aseprite; `tests/ase_real.rs` reads the ones that were.

use super::orientation;
use crate::ase_build::*;
use crate::ase_expand::expand;
use crate::ase_tests::{open, refused};
use crate::{AseWarning, FormatError};
use pikslar_core::layer::LayerKind;
use pikslar_core::orient::Orientation;
use pikslar_core::tilemap::MapCell;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{rgba, ColorMode, Document};

/// Tiles flagged as the empty tile being number 0, which every recent file does.
const NEW: u32 = 4;
const OLD: u32 = 0;

/// RGBA pixels of `count` tiles of `w` by `h`, one under another, every pixel different from
/// the rest of its tile and of the others.
fn tiles_rgba(w: usize, h: usize, count: usize) -> Vec<u8> {
    (0..count * w * h)
        .flat_map(|i| {
            let (k, at) = (i / (w * h), i % (w * h));
            [
                (k * 37 + 1) as u8,
                (at % w * 11) as u8,
                (at / w * 13) as u8,
                255,
            ]
        })
        .collect()
}

fn colour(tiles: &[u8], tile: usize, (w, h): (usize, usize), (x, y): (usize, usize)) -> u32 {
    let at = (tile * w * h + y * w + x) * 4;
    rgba(tiles[at], tiles[at + 1], tiles[at + 2], tiles[at + 3])
}

fn has(warnings: &[AseWarning], what: &AseWarning) -> bool {
    warnings.contains(what)
}

fn file(depth: u16, size: (u16, u16), chunks: Vec<Vec<u8>>) -> Vec<u8> {
    Ase::new(size.0, size.1, depth).frame(100, chunks).bytes()
}

/// The common case: one 2 by 2 tileset of four tiles (tile 0 empty), one layer, one map.
fn simple(values: &[u32], dims: (u16, u16), at: (i16, i16)) -> Vec<u8> {
    file(
        32,
        (8, 8),
        vec![
            tileset_chunk(0, NEW, 4, (2, 2), "set", &tiles_rgba(2, 2, 4)),
            tilemap_layer(3, 0, "map", 0),
            cel_tilemap(0, at, dims, 32, MASKS_NEW, values),
        ],
    )
}

fn cell_bits(doc: &Document, cx: i32, cy: i32) -> u32 {
    doc.layers()[0].cel(0).image().get(cx, cy)
}

#[test]
fn a_tileset_and_a_map_come_in_with_their_tiles() {
    let (doc, warnings) = open(&simple(&[1, 2, 0, 3], (2, 2), (1, 2)));
    assert!(warnings.is_empty(), "{warnings:?}");
    let set = doc.tileset(TilesetId::new(1)).expect("the tileset");
    assert_eq!((set.name(), set.tile_size(), set.len()), ("set", (2, 2), 3));
    assert_eq!(set.mode(), ColorMode::Rgba);
    // Tile 0 is the empty tile and is not brought in, so the file's tile k is tile k here.
    let ids: Vec<u32> = set.tiles().iter().map(|t| t.id.get()).collect();
    assert_eq!(ids, [1, 2, 3]);
    let layer = &doc.layers()[0];
    assert_eq!(layer.kind, LayerKind::Tilemap(TilesetId::new(1)));
    assert_eq!((layer.name.as_str(), layer.mode), ("map", ColorMode::Rgba));
    assert_eq!(layer.cel(0).position(), (1, 2));
    let tile = |n| MapCell::place(TileId::new(n).expect("id"), Orientation::IDENTITY).bits();
    assert_eq!(cell_bits(&doc, 0, 0), tile(1));
    assert_eq!(cell_bits(&doc, 1, 0), tile(2));
    assert_eq!(cell_bits(&doc, 0, 1), 0);
    assert_eq!(cell_bits(&doc, 1, 1), tile(3));
    // The map's cell (1, 1) is the canvas rectangle from (1 + 2, 2 + 2): tile 3.
    let canvas = expand(&doc, 0, 0);
    let tiles = tiles_rgba(2, 2, 4);
    assert_eq!(canvas[4 * 8 + 3], colour(&tiles, 3, (2, 2), (0, 0)));
    assert_eq!(canvas[5 * 8 + 4], colour(&tiles, 3, (2, 2), (1, 1)));
    assert_eq!(canvas[2 * 8 + 1], colour(&tiles, 1, (2, 2), (0, 0)));
    assert_eq!(canvas[0], 0);
}

/// Tiled's rule, written out: the diagonal flip first, then horizontal, then vertical.
fn turned(src: &[u32], n: usize, (x, y, d): (bool, bool, bool)) -> Vec<u32> {
    let mut out = vec![0; n * n];
    for sy in 0..n {
        for sx in 0..n {
            let (mut px, mut py) = (sx, sy);
            if d {
                (px, py) = (py, px);
            }
            if x {
                px = n - 1 - px;
            }
            if y {
                py = n - 1 - py;
            }
            out[py * n + px] = src[sy * n + sx];
        }
    }
    out
}

#[test]
fn all_eight_flips_read_from_either_set_of_masks() {
    let tiles = tiles_rgba(3, 3, 2);
    let source: Vec<u32> = (0..9)
        .map(|i| colour(&tiles, 1, (3, 3), (i % 3, i / 3)))
        .collect();
    for masks in [MASKS_NEW, MASKS_OLD] {
        let (idm, xm, ym, dm) = (masks[0], masks[1], masks[2], masks[3]);
        for bits in 0..8u32 {
            let flags = (bits & 4 != 0, bits & 2 != 0, bits & 1 != 0);
            let value = 1 | if flags.0 { xm } else { 0 } | if flags.1 { ym } else { 0 };
            let value = value | if flags.2 { dm } else { 0 };
            let chunks = vec![
                tileset_chunk(0, NEW, 2, (3, 3), "t", &tiles),
                tilemap_layer(3, 0, "m", 0),
                cel_tilemap(0, (0, 0), (1, 1), 32, [idm, xm, ym, dm], &[value]),
            ];
            let (doc, warnings) = open(&file(32, (3, 3), chunks));
            assert!(warnings.is_empty(), "flags {flags:?}: {warnings:?}");
            let want = turned(&source, 3, flags);
            assert_eq!(expand(&doc, 0, 0), want, "flags {flags:?} masks {masks:x?}");
            let cell = MapCell::from_bits(cell_bits(&doc, 0, 0));
            assert_eq!(cell.orientation().index(), bits as u8);
        }
    }
}

#[test]
fn the_three_flags_make_the_orientation_of_tiled() {
    assert_eq!(orientation(false, false, false), Orientation::IDENTITY);
    assert_eq!(
        orientation(true, false, false),
        Orientation::flip_horizontal()
    );
    assert_eq!(
        orientation(false, true, false),
        Orientation::flip_vertical()
    );
    assert_eq!(orientation(false, false, true), Orientation::transpose());
    assert_eq!(orientation(true, true, true), Orientation::anti_transpose());
    assert_eq!(orientation(true, false, true), Orientation::rotate90());
}

#[test]
fn a_tileset_that_counts_tile_zero_as_a_tile_shifts_its_tiles_up_by_one() {
    // No flag for an empty tile 0: the empty cell is all ones, and tile 0 is a tile.
    let tiles = tiles_rgba(2, 2, 3);
    let chunks = vec![
        tileset_chunk(0, OLD, 3, (2, 2), "old", &tiles),
        tilemap_layer(3, 0, "m", 0),
        cel_tilemap(0, (0, 0), (4, 1), 32, MASKS_NEW, &[u32::MAX, 0, 1, 2]),
    ];
    let (doc, warnings) = open(&file(32, (8, 2), chunks));
    assert!(warnings.is_empty(), "{warnings:?}");
    let set = doc.tileset(TilesetId::new(1)).expect("tileset");
    assert_eq!(set.len(), 3);
    assert_eq!(cell_bits(&doc, 0, 0), 0);
    assert_eq!(cell_bits(&doc, 1, 0), 1);
    assert_eq!(cell_bits(&doc, 3, 0), 3);
    let canvas = expand(&doc, 0, 0);
    assert_eq!(canvas[2], colour(&tiles, 0, (2, 2), (0, 0)));
    assert_eq!(canvas[6], colour(&tiles, 2, (2, 2), (0, 0)));
}

#[test]
fn indexed_tiles_become_palette_places_and_the_transparent_index_is_clear() {
    // Two 2 by 1 tiles after the empty one: indices 5 0 | 3 9, with 9 transparent, so that
    // index 0 is a color like any other.
    let pixels = [0, 0, 5, 0, 3, 9];
    let chunks = vec![
        palette(10, 0, &[[1, 1, 1, 255], [2, 2, 2, 255]]),
        tileset_chunk(0, NEW, 3, (2, 1), "i", &pixels),
        tilemap_layer(3, 0, "m", 0),
        cel_tilemap(0, (0, 0), (2, 1), 32, MASKS_NEW, &[1, 2]),
    ];
    let mut ase = Ase::new(4, 1, 8);
    ase.transparent = 9;
    let (doc, warnings) = open(&ase.frame(100, chunks).bytes());
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, AseWarning::IndexPastPalette)),
        "{warnings:?}"
    );
    let set = doc.tileset(TilesetId::new(1)).expect("tileset");
    assert_eq!(
        (set.mode(), doc.layers()[0].mode),
        (ColorMode::Palette, ColorMode::Palette)
    );
    // A palette place is the index plus one; place 0 is clear.
    assert_eq!(expand(&doc, 0, 0), [6, 1, 4, 0]);
}

#[test]
fn grayscale_tiles_become_rgba() {
    // Tile 1 is two pixels: grey 7 at full alpha, grey 200 at alpha 128.
    let chunks = vec![
        tileset_chunk(0, NEW, 2, (2, 1), "g", &[0, 0, 0, 0, 7, 255, 200, 128]),
        tilemap_layer(3, 0, "m", 0),
        cel_tilemap(0, (0, 0), (1, 1), 32, MASKS_NEW, &[1]),
    ];
    let (doc, warnings) = open(&file(16, (2, 1), chunks));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(doc.layers()[0].mode, ColorMode::Rgba);
    assert_eq!(
        expand(&doc, 0, 0),
        [rgba(7, 7, 7, 255), rgba(200, 200, 200, 128)]
    );
}

#[test]
fn tiles_of_eight_and_sixteen_bits_read_as_those_of_thirty_two() {
    let values = [1, 3, 0, 2];
    let (want, _) = open(&simple(&values, (2, 2), (0, 0)));
    for bits in [8u16, 16] {
        let masks = [0x3f, 0x80, 0x40, 0x20];
        let chunks = vec![
            tileset_chunk(0, NEW, 4, (2, 2), "set", &tiles_rgba(2, 2, 4)),
            tilemap_layer(3, 0, "map", 0),
            cel_tilemap(0, (0, 0), (2, 2), bits, masks, &values),
        ];
        let (doc, warnings) = open(&file(32, (8, 8), chunks));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(expand(&doc, 0, 0), expand(&want, 0, 0), "{bits} bits");
    }
    // 8-bit tiles carry flags in their own bits too: 0x80 is the x flip here.
    let flipped = vec![
        tileset_chunk(0, NEW, 2, (2, 2), "set", &tiles_rgba(2, 2, 2)),
        tilemap_layer(3, 0, "map", 0),
        cel_tilemap(0, (0, 0), (1, 1), 8, [0x3f, 0x80, 0x40, 0x20], &[0x81]),
    ];
    let (doc, _) = open(&file(32, (2, 2), flipped));
    let cell = MapCell::from_bits(cell_bits(&doc, 0, 0));
    assert_eq!(cell.orientation(), Orientation::flip_horizontal());
}

#[test]
fn layers_that_name_one_tileset_share_ours_and_tileset_numbers_are_the_files() {
    let tiles = tiles_rgba(2, 2, 3);
    let chunks = vec![
        tileset_chunk(7, NEW, 3, (2, 2), "seven", &tiles),
        tileset_chunk(2, NEW, 2, (2, 2), "two", &tiles[..32]),
        tileset_chunk(9, NEW, 1, (4, 4), "unused", &[0; 64]),
        tilemap_layer(3, 0, "a", 2),
        tilemap_layer(3, 0, "b", 7),
        tilemap_layer(3, 0, "c", 2),
        cel_tilemap(0, (0, 0), (1, 1), 32, MASKS_NEW, &[1]),
        cel_tilemap(1, (0, 0), (1, 1), 32, MASKS_NEW, &[2]),
        cel_tilemap(2, (0, 0), (1, 1), 32, MASKS_NEW, &[1]),
    ];
    let (doc, warnings) = open(&file(32, (2, 2), chunks));
    assert!(warnings.is_empty(), "{warnings:?}");
    let names: Vec<&str> = doc.tilesets.iter().map(|t| t.name()).collect();
    assert_eq!(
        names,
        ["seven", "two", "unused"],
        "all are kept, in the file's order"
    );
    let used: Vec<_> = doc.layers().iter().map(|l| l.tileset()).collect();
    let (seven, two) = (Some(TilesetId::new(1)), Some(TilesetId::new(2)));
    assert_eq!(used, [two, seven, two]);
    assert_eq!(doc.tileset_users(TilesetId::new(3)), 0);
}

#[test]
fn the_order_of_the_chunks_does_not_matter_for_layers_but_a_cel_needs_its_tileset_first() {
    let tiles = tiles_rgba(2, 2, 2);
    let layer_first = vec![
        tilemap_layer(3, 0, "m", 0),
        tileset_chunk(0, NEW, 2, (2, 2), "t", &tiles),
        cel_tilemap(0, (0, 0), (1, 1), 32, MASKS_NEW, &[1]),
    ];
    let (doc, warnings) = open(&file(32, (2, 2), layer_first));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(cell_bits(&doc, 0, 0), 1);
    let cel_first = vec![
        tilemap_layer(3, 0, "m", 0),
        cel_tilemap(0, (0, 0), (1, 1), 32, MASKS_NEW, &[1]),
        tileset_chunk(0, NEW, 2, (2, 2), "t", &tiles),
    ];
    let (doc, warnings) = open(&file(32, (2, 2), cel_first));
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(cell_bits(&doc, 0, 0), 0);
    assert!(has(
        &warnings,
        &AseWarning::Damaged {
            layer: "m".into(),
            frame: 0
        }
    ));
}

#[test]
fn an_external_tileset_leaves_its_layers_out_with_a_warning() {
    let chunks = vec![
        tileset_external(0, 4, (2, 2), "elsewhere"),
        plain("paint"),
        tilemap_layer(3, 0, "map", 0),
        cel_raw(0, 0, 0, 1, 1, &[1, 2, 3, 255]),
        cel_tilemap(1, (0, 0), (1, 1), 32, MASKS_NEW, &[1]),
    ];
    let (doc, warnings) = open(&file(32, (2, 2), chunks));
    assert_eq!(doc.layers().len(), 1);
    assert!(doc.tilesets.is_empty());
    assert!(has(
        &warnings,
        &AseWarning::Skipped("tilesets kept in other files")
    ));
    assert!(has(&warnings, &AseWarning::TilemapLeftOut("map".into())));
    let only = file(
        32,
        (2, 2),
        vec![
            tileset_external(0, 4, (2, 2), "x"),
            tilemap_layer(3, 0, "m", 0),
        ],
    );
    assert!(matches!(refused(&only), FormatError::Unsupported(_)));
}

#[test]
fn cells_that_name_missing_tiles_are_empty_and_told() {
    // Tile 4 and tile 100 do not exist (the tileset has numbers 0 to 3).
    let (doc, warnings) = open(&simple(&[1, 4, 100, 3], (2, 2), (0, 0)));
    assert_eq!(cell_bits(&doc, 1, 0), 0);
    assert_eq!(cell_bits(&doc, 0, 1), 0);
    assert_eq!(cell_bits(&doc, 1, 1), 3);
    assert!(has(&warnings, &AseWarning::UnknownTile("map".into())));
    // A flag on tile 0 leaves an empty cell exactly 0.
    let (doc, _) = open(&simple(&[0x8000_0000, 0x4000_0000, 0, 0], (2, 2), (0, 0)));
    assert!((0..4).all(|i| cell_bits(&doc, i % 2, i / 2) == 0));
}

#[test]
fn a_diagonal_flip_on_tiles_that_are_not_square_is_dropped_and_told() {
    let tiles = tiles_rgba(3, 2, 2);
    let value = 1 | MASKS_NEW[3] | MASKS_NEW[1];
    let chunks = vec![
        tileset_chunk(0, NEW, 2, (3, 2), "t", &tiles),
        tilemap_layer(3, 0, "m", 0),
        cel_tilemap(0, (0, 0), (1, 1), 32, MASKS_NEW, &[value]),
    ];
    let (doc, warnings) = open(&file(32, (3, 2), chunks));
    let cell = MapCell::from_bits(cell_bits(&doc, 0, 0));
    assert_eq!(cell.orientation(), Orientation::flip_horizontal());
    let told = AseWarning::DiagonalOnOblong("m".into());
    assert_eq!(warnings, std::slice::from_ref(&told));
    let text = told.to_string();
    assert!(
        text.contains("\"m\"") && text.contains("Aseprite"),
        "{text}"
    );
}

#[test]
fn diagonal_flips_of_square_tiles_are_kept_without_a_warning() {
    let d = MASKS_NEW[3];
    let values = [1 | d, 2 | d | MASKS_NEW[1], 3 | MASKS_NEW[2], 1];
    let (doc, warnings) = open(&simple(&values, (2, 2), (0, 0)));
    assert!(warnings.is_empty(), "{warnings:?}");
    let cell = MapCell::from_bits(cell_bits(&doc, 1, 0));
    assert_eq!(cell.orientation(), Orientation::rotate90());
}

#[test]
fn tiles_that_cannot_be_unpacked_are_blank_and_the_layer_keeps_its_cells() {
    let tiles = tiles_rgba(2, 2, 3);
    let mut bad_sum = zlib(&tiles);
    let last = bad_sum.len() - 1;
    bad_sum[last] ^= 1;
    for stream in [bad_sum, vec![0x78, 0x9c, 0xff, 0xff, 0xff]] {
        let chunks = vec![
            tileset_packed(0, NEW, 3, (2, 2), "broken", &stream),
            tilemap_layer(3, 0, "m", 0),
            cel_tilemap(0, (0, 0), (2, 1), 32, MASKS_NEW, &[1, 2]),
        ];
        let (doc, warnings) = open(&file(32, (4, 2), chunks));
        let set = doc.tileset(TilesetId::new(1)).expect("tileset");
        assert_eq!(set.len(), 2);
        assert!(set.tiles().iter().all(|t| t.image.is_blank()));
        assert_eq!(cell_bits(&doc, 1, 0), 2);
        assert!(has(&warnings, &AseWarning::TilesetDamaged("broken".into())));
    }
}

#[test]
fn a_map_with_a_bad_stream_is_refused_and_one_that_cannot_be_read_is_empty() {
    let head = [
        tileset_chunk(0, NEW, 2, (2, 2), "t", &tiles_rgba(2, 2, 2)),
        tilemap_layer(3, 0, "m", 0),
    ];
    let with = |cel: Vec<u8>| file(32, (2, 2), vec![head[0].clone(), head[1].clone(), cel]);
    // Not a zlib stream at all: the file is refused, as for a drawing.
    let junk = cel_tilemap_packed(0, (0, 0), (1, 1), 32, MASKS_NEW, &[1, 2, 3]);
    assert!(matches!(refused(&with(junk)), FormatError::Corrupt(_)));
    // A stream whose checksum is wrong, a tile of 24 bits and a missing tileset: empty maps.
    let mut sum = zlib(&1u32.to_le_bytes());
    let last = sum.len() - 1;
    sum[last] ^= 1;
    let bad_sum = cel_tilemap_packed(0, (0, 0), (1, 1), 32, MASKS_NEW, &sum);
    let odd = cel_tilemap(0, (0, 0), (1, 1), 24, MASKS_NEW, &[1]);
    for cel in [bad_sum, odd] {
        let (doc, warnings) = open(&with(cel));
        assert_eq!(cell_bits(&doc, 0, 0), 0);
        assert!(has(
            &warnings,
            &AseWarning::Damaged {
                layer: "m".into(),
                frame: 0
            }
        ));
    }
}

#[test]
fn tilesets_a_document_cannot_hold_are_left_out_without_making_anything() {
    let sets = vec![
        tileset_chunk(0, NEW, 2, (0, 2), "zero", &[]),
        tileset_chunk(1, NEW, 2, (257, 1), "wide", &[]),
        tileset_packed(2, NEW, u32::MAX, (256, 256), "many", &[0x78, 0x9c]),
        tileset_packed(3, NEW, 65_538, (1, 1), "more", &[0x78, 0x9c]),
        tileset_packed(4, NEW, 0, (2, 2), "ok", &zlib(&[])),
    ];
    let mut chunks = sets;
    chunks.push(plain("paint"));
    for n in 0..5 {
        chunks.push(tilemap_layer(3, 0, "m", n));
    }
    let (doc, warnings) = open(&file(32, (2, 2), chunks));
    let kept: Vec<&str> = doc.tilesets.iter().map(|t| t.name()).collect();
    assert_eq!(kept, ["ok"]);
    assert_eq!(
        doc.layers().len(),
        2,
        "the paint layer and the layer of the tileset that fits"
    );
    assert!(has(
        &warnings,
        &AseWarning::Skipped("tilesets with tiles of a size or number a document cannot hold")
    ));
}

#[test]
fn at_most_sixty_four_tilesets_are_kept() {
    let mut chunks: Vec<Vec<u8>> = (0..66)
        .map(|n| tileset_chunk(n, NEW, 1, (1, 1), &format!("t{n}"), &[0; 4]))
        .collect();
    chunks.push(tilemap_layer(3, 0, "last", 65));
    chunks.push(tilemap_layer(3, 0, "first", 0));
    let (doc, warnings) = open(&file(32, (1, 1), chunks));
    assert_eq!(doc.tilesets.len(), 64);
    assert_eq!(doc.layers().len(), 1);
    assert!(has(&warnings, &AseWarning::TilemapLeftOut("last".into())));
}

#[test]
fn damaged_tileset_chunks_cost_only_their_tileset() {
    let same = tileset_chunk(3, NEW, 2, (2, 2), "dup", &tiles_rgba(2, 2, 2));
    let chunks = vec![
        chunk(0x2023, &[0; 4]),
        same.clone(),
        same,
        tilemap_layer(3, 0, "m", 3),
    ];
    let (doc, warnings) = open(&file(32, (2, 2), chunks));
    assert_eq!(doc.tilesets.len(), 1);
    assert!(has(
        &warnings,
        &AseWarning::Skipped("tilesets that are cut short")
    ));
    assert!(has(
        &warnings,
        &AseWarning::Skipped("tilesets that share a number with another")
    ));
}

#[test]
fn a_map_too_large_for_a_picture_is_refused_before_it_is_read() {
    let chunks = vec![
        tileset_chunk(0, NEW, 2, (1, 1), "t", &[0; 8]),
        tilemap_layer(3, 0, "m", 0),
        cel_tilemap_packed(0, (0, 0), (65_535, 65_535), 32, MASKS_NEW, &[]),
    ];
    assert!(matches!(
        refused(&file(32, (2, 2), chunks)),
        FormatError::TooLarge
    ));
}

#[test]
fn drawings_and_maps_on_the_wrong_kind_of_layer_are_left_out_and_told() {
    let chunks = vec![
        tileset_chunk(0, NEW, 2, (2, 2), "t", &tiles_rgba(2, 2, 2)),
        plain("paint"),
        tilemap_layer(3, 0, "map", 0),
        cel_tilemap(0, (0, 0), (1, 1), 32, MASKS_NEW, &[1]),
        cel_zlib(1, 0, 0, 1, 1, &[1, 2, 3, 255]),
    ];
    let (doc, warnings) = open(&file(32, (2, 2), chunks));
    assert_eq!(doc.layers()[0].cel(0).image().drawn_bounds(), None);
    assert_eq!(cell_bits_of(&doc, 1), 0);
    assert!(has(&warnings, &AseWarning::Skipped("tilemap cels")));
    assert!(has(
        &warnings,
        &AseWarning::Skipped("drawings on tilemap layers")
    ));
}

fn cell_bits_of(doc: &Document, layer: usize) -> u32 {
    doc.layers()[layer].cel(0).image().get(0, 0)
}

#[test]
fn a_linked_map_cel_is_a_hold_of_the_same_map() {
    let first = vec![
        tileset_chunk(0, NEW, 3, (2, 2), "t", &tiles_rgba(2, 2, 3)),
        tilemap_layer(3, 0, "m", 0),
        cel_tilemap(0, (1, 0), (2, 1), 32, MASKS_NEW, &[2, 1]),
    ];
    let second = vec![cel_linked(0, 0)];
    let bytes = Ase::new(8, 4, 32)
        .frame(100, first)
        .frame(100, second)
        .bytes();
    let (doc, warnings) = open(&bytes);
    assert!(warnings.is_empty(), "{warnings:?}");
    let layer = &doc.layers()[0];
    let ids: Vec<u32> = layer.links().collect();
    assert!(ids[0] != 0 && ids[0] == ids[1], "{ids:?}");
    assert_eq!(layer.cel(1).position(), (1, 0));
    assert_eq!(layer.cel(1).image().get(0, 0), 2);
}

#[test]
fn a_cel_opacity_cannot_be_kept_on_a_map_and_is_told() {
    let cel = cel(0, 0, 0, 100, 3, 0, &{
        let mut p = Vec::new();
        p.extend_from_slice(&[1, 0, 1, 0, 32, 0]);
        MASKS_NEW
            .iter()
            .for_each(|m| p.extend_from_slice(&m.to_le_bytes()));
        p.extend_from_slice(&[0; 10]);
        p.extend_from_slice(&zlib(&1u32.to_le_bytes()));
        p
    });
    let chunks = vec![
        tileset_chunk(0, NEW, 2, (2, 2), "t", &tiles_rgba(2, 2, 2)),
        tilemap_layer(3, 0, "m", 0),
        cel,
    ];
    let (doc, warnings) = open(&file(32, (2, 2), chunks));
    assert_eq!(cell_bits_of(&doc, 0), 1);
    assert!(has(&warnings, &AseWarning::CelOpacity("m".into())));
}

#[test]
fn a_tile_number_mask_is_read_from_where_it_starts_and_an_empty_one_leaves_every_cell_empty() {
    let chunks = |masks| {
        vec![
            tileset_chunk(0, NEW, 4, (2, 2), "t", &tiles_rgba(2, 2, 4)),
            tilemap_layer(3, 0, "m", 0),
            cel_tilemap(0, (0, 0), (2, 1), 32, masks, &[2 << 8, 3 << 8]),
        ]
    };
    let (doc, _) = open(&file(
        32,
        (4, 2),
        chunks([0xff00, 0x8000_0000, 0x4000_0000, 0x2000_0000]),
    ));
    assert_eq!((cell_bits(&doc, 0, 0), cell_bits(&doc, 1, 0)), (2, 3));
    let (doc, _) = open(&file(32, (4, 2), chunks([0, 0, 0, 0])));
    assert!(doc.layers()[0].cel(0).image().is_blank());
}

#[test]
fn a_map_of_no_cells_and_a_map_off_the_canvas_are_fine() {
    let chunks = vec![
        tileset_chunk(0, NEW, 2, (2, 2), "t", &tiles_rgba(2, 2, 2)),
        tilemap_layer(3, 0, "empty", 0),
        tilemap_layer(3, 0, "off", 0),
        cel_tilemap(0, (0, 0), (0, 0), 32, MASKS_NEW, &[]),
        cel_tilemap(1, (-5, -7), (4, 4), 32, MASKS_NEW, &[1; 16]),
    ];
    let (doc, warnings) = open(&file(32, (4, 4), chunks));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(doc.layers()[0].cel(0).image().is_blank());
    assert_eq!(doc.layers()[1].cel(0).position(), (-5, -7));
    // The last cell of each row and column begins at canvas (-1, -1) and covers the corner.
    assert_ne!(expand(&doc, 1, 0)[0], 0);
}

#[test]
fn a_tilemap_layer_keeps_its_group_visibility_and_order() {
    let chunks = vec![
        tileset_chunk(0, NEW, 2, (2, 2), "t", &tiles_rgba(2, 2, 2)),
        layer(3, 1, 0, 0, 255, "group"),
        tilemap_layer(1, 1, "inside", 0),
        plain("top"),
    ];
    let (doc, _) = open(&file(32, (2, 2), chunks));
    let names: Vec<&str> = doc.layers().iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["inside", "group", "top"]);
    assert!(doc.layers()[0].visible);
    assert_eq!(doc.layers()[0].depth, 1);
    assert_eq!(doc.layers()[0].kind, LayerKind::Tilemap(TilesetId::new(1)));
}

#[test]
fn the_rich_tile_files_import_with_what_they_hold() {
    let (doc, warnings) = open(&rich_tiles());
    let sets: Vec<(&str, usize)> = doc.tilesets.iter().map(|t| (t.name(), t.len())).collect();
    assert_eq!(
        sets,
        [("new", 3), ("old", 3)],
        "the external tileset is not kept"
    );
    let names: Vec<&str> = doc.layers().iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["base", "map a", "map b", "map c"]);
    assert!(has(&warnings, &AseWarning::TilemapLeftOut("ext".into())));
    // The second frame holds the first frame's map a.
    let links: Vec<u32> = doc.layers()[1].links().collect();
    assert!(links[0] != 0 && links[0] == links[1]);
    doc.check_tilemaps().expect("the rules of tilemaps hold");
    let (doc, warnings) = open(&rich_tiles_indexed());
    assert_eq!(doc.tilesets.len(), 1);
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, AseWarning::UnknownTile(_))),
        "{warnings:?}"
    );
    assert_eq!(
        doc.layers()
            .iter()
            .filter(|l| l.tileset().is_some())
            .count(),
        2
    );
}

/// Tiles a project file could not keep are told in the words `check_writable` uses, with the
/// 416 MiB figure and what to do, not as a file that is too large to open.
#[test]
fn tiles_past_the_project_budget_say_so() {
    let told = super::limit(pikslar_core::LimitError::TileMemory);
    assert!(matches!(told, FormatError::TilesTooLarge), "{told:?}");
    assert!(told.to_string().contains("416 MiB"));
}
