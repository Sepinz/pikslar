use crate::tile::{RawTile, TILE};
use crate::tilemap::oracle::{random_case, Case};

const SENTINEL: u32 = 0xdead_beef;

/// The oracle's 16 by 16 canvas tile.
fn expected(case: &Case, tx: usize, ty: usize) -> RawTile {
    let mut out = [0u32; TILE * TILE];
    for (i, v) in out.iter_mut().enumerate() {
        let x = (tx * TILE + i % TILE) as i32;
        let y = (ty * TILE + i / TILE) as i32;
        *v = case.pixel(x, y);
    }
    out
}

#[test]
fn every_canvas_tile_of_a_random_map_is_what_the_cells_say() {
    for seed in 1..=120 {
        let case = random_case(seed, (70, 55));
        let layer = &case.doc.layers()[0];
        for ty in 0..4 {
            for tx in 0..5 {
                let want = expected(&case, tx, ty);
                let mut out = [SENTINEL; TILE * TILE];
                let drawn = case.doc.layer_tile(layer, 0, tx, ty, &mut out);
                if drawn {
                    assert!(
                        out == want,
                        "seed {seed} size {:?} tile {tx},{ty}",
                        case.size
                    );
                } else {
                    assert!(want.iter().all(|&v| v == 0), "seed {seed} tile {tx},{ty}");
                    assert!(out.iter().all(|&v| v == SENTINEL), "left alone");
                }
            }
        }
    }
}

#[test]
fn every_pixel_of_a_random_map_reads_as_the_cells_say() {
    for seed in 200..260 {
        let case = random_case(seed, (40, 40));
        let layer = &case.doc.layers()[0];
        for y in -45..90 {
            for x in -45..90 {
                let got = case.doc.layer_pixel(layer, 0, x, y);
                assert_eq!(
                    got,
                    case.pixel(x, y),
                    "seed {seed} size {:?} at {x},{y}",
                    case.size
                );
            }
        }
    }
}

#[test]
fn a_frame_without_a_map_and_a_missing_tileset_draw_nothing() {
    let mut case = random_case(7, (40, 40));
    case.doc.add_frame(1, false).expect("room");
    let layer = &case.doc.layers()[0];
    let mut out = [SENTINEL; TILE * TILE];
    // A new frame has a blank cel.
    assert!(!case.doc.layer_tile(layer, 1, 0, 0, &mut out));
    assert_eq!(case.doc.layer_pixel(layer, 1, 3, 3), 0);
    let mut lonely = case.doc.clone();
    lonely.tilesets = Default::default();
    assert!(!lonely.layer_tile(layer, 0, 0, 0, &mut out));
    assert_eq!(lonely.layer_pixel(layer, 0, 3, 3), 0);
    assert!(out.iter().all(|&v| v == SENTINEL));
}

/// Eight orientations of one asymmetric 3 by 3 tile, against the corner cases by hand.
#[test]
fn each_of_the_eight_turns_is_the_one_tiled_defines() {
    use crate::tilemap::oracle::place;
    // Row by row: 1 2 3 / 4 5 6 / 7 8 9.
    let tile: Vec<u32> = (1..=9).collect();
    let rows = |bits: u32| place(&tile, (3, 3), bits);
    assert_eq!(rows(0), tile);
    assert_eq!(rows(0x8000_0000), [3, 2, 1, 6, 5, 4, 9, 8, 7]);
    assert_eq!(rows(0x4000_0000), [7, 8, 9, 4, 5, 6, 1, 2, 3]);
    assert_eq!(rows(0x2000_0000), [1, 4, 7, 2, 5, 8, 3, 6, 9]);
    assert_eq!(rows(0xa000_0000), [7, 4, 1, 8, 5, 2, 9, 6, 3]);
    assert_eq!(rows(0x6000_0000), [3, 6, 9, 2, 5, 8, 1, 4, 7]);
    assert_eq!(rows(0xc000_0000), [9, 8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!(rows(0xe000_0000), [9, 6, 3, 8, 5, 2, 7, 4, 1]);
}
