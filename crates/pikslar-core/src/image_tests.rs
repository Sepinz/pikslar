use super::*;
use std::collections::BTreeMap;

fn key(x: i16, y: i16) -> TileKey {
    TileKey { x, y }
}

fn solid(value: u32) -> Cell {
    Some(Arc::new(Tile::Solid(value)))
}

fn write(image: &mut Image, entries: &[(i16, i16, u32)]) -> Vec<(TileKey, Cell)> {
    let mut changes: Vec<(TileKey, Cell)> = entries
        .iter()
        .map(|&(x, y, v)| (key(x, y), (v != 0).then(|| solid(v)).flatten()))
        .collect();
    image.swap_cells(&mut changes);
    changes
}

fn drawn(entries: &[(i16, i16, u32)]) -> Image {
    let mut image = Image::blank();
    write(&mut image, entries);
    image
}

#[test]
fn a_blank_image_has_no_table() {
    let image = Image::blank();
    assert!(image.is_blank());
    assert_eq!(image.table_bytes(), 0);
    assert_eq!(image.get(5, 5), 0);
    assert!(image.tile(0, 0).is_none());
}

#[test]
fn the_table_covers_only_what_is_drawn() {
    let image = drawn(&[(10, 20, 7)]);
    assert_eq!(image.table(), (10, 20, 1, 1));
    assert_eq!(image.table_bytes(), std::mem::size_of::<Cell>());
    assert_eq!(image.get(10 * 16 + 3, 20 * 16 + 4), 7);
    assert_eq!(image.get(11 * 16, 20 * 16), 0);
    assert_eq!(image.get(9 * 16 + 15, 20 * 16), 0);
}

#[test]
fn growing_keeps_what_was_there() {
    let mut image = drawn(&[(4, 4, 1)]);
    write(&mut image, &[(1, 8, 2), (6, 2, 3)]);
    assert_eq!(image.table(), (1, 2, 6, 7));
    assert_eq!(image.tile(4, 4), Some(&Tile::Solid(1)));
    assert_eq!(image.tile(1, 8), Some(&Tile::Solid(2)));
    assert_eq!(image.tile(6, 2), Some(&Tile::Solid(3)));
    assert_eq!(image.tile(2, 2), None);
}

#[test]
fn emptying_an_edge_shrinks_the_table_to_what_remains() {
    let mut image = drawn(&[(0, 0, 1), (3, 0, 2), (3, 5, 3)]);
    write(&mut image, &[(3, 5, 0)]);
    assert_eq!(image.table(), (0, 0, 4, 1));
    write(&mut image, &[(0, 0, 0)]);
    assert_eq!(image.table(), (3, 0, 1, 1));
}

#[test]
fn emptying_the_inside_leaves_the_table_alone() {
    let mut image = drawn(&[(0, 0, 1), (2, 0, 2), (1, 0, 3)]);
    write(&mut image, &[(1, 0, 0)]);
    assert_eq!(image.table(), (0, 0, 3, 1));
}

#[test]
fn emptying_everything_leaves_no_table() {
    let mut image = drawn(&[(0, 0, 1), (9, 9, 2)]);
    write(&mut image, &[(0, 0, 0), (9, 9, 0)]);
    assert!(image.is_blank());
    assert_eq!(image, Image::blank());
    assert_eq!(image.table_bytes(), 0);
}

#[test]
fn clearing_a_tile_outside_the_table_changes_nothing() {
    let mut image = drawn(&[(2, 2, 1)]);
    let before = image.clone();
    let undone = write(&mut image, &[(40, 40, 0)]);
    assert_eq!(image, before);
    assert_eq!(undone, vec![(key(40, 40), None)]);
}

#[test]
fn swapping_the_result_back_restores_the_image_exactly() {
    let mut image = drawn(&[(1, 1, 1), (3, 3, 2)]);
    let before = image.clone();
    let mut undo = write(&mut image, &[(8, 8, 5), (1, 1, 0), (3, 3, 6)]);
    assert_ne!(image, before);
    image.swap_cells(&mut undo);
    assert_eq!(image, before);
}

#[test]
fn negative_positions_work_like_any_other() {
    let image = drawn(&[(-2, -3, 9)]);
    assert_eq!(image.table(), (-2, -3, 1, 1));
    assert_eq!(image.get(-32, -48), 9);
    assert_eq!(image.get(-17, -33), 9);
    assert_eq!(image.get(-16, -33), 0);
}

#[test]
fn far_outside_reads_as_nothing() {
    let image = drawn(&[(0, 0, 9)]);
    assert_eq!(image.get(i32::MAX, 0), 0);
    assert_eq!(image.get(0, i32::MIN), 0);
    assert_eq!(image.get(-1, -1), 0);
}

#[test]
fn tiles_are_moved_and_never_copied_when_the_table_grows() {
    let mut image = drawn(&[(0, 0, 1)]);
    let held = image.cells[0].clone().expect("drawn");
    write(&mut image, &[(50, 50, 2)]);
    let moved = image.cells[0].as_ref().expect("still there");
    assert!(Arc::ptr_eq(moved, &held));
}

#[test]
fn a_table_is_built_from_a_full_one_without_copying_tiles() {
    let tile = solid(4);
    let mut cells: Vec<Cell> = vec![None; 12];
    cells[5] = tile.clone();
    let image = Image::from_table(4, cells);
    assert_eq!(image.table(), (1, 1, 1, 1));
    assert!(Arc::ptr_eq(
        image.cells[0].as_ref().expect("kept"),
        tile.as_ref().expect("made")
    ));
}

#[test]
fn tiles_that_show_nothing_are_never_kept() {
    let packed = Tile::Packed {
        palette: Box::new([0, 5]),
        bits: 1,
        data: vec![0; TILE_AREA / 8].into_boxed_slice(),
    };
    let blank_kinds = [Tile::Solid(0), packed, Tile::Raw(Box::new([0; TILE_AREA]))];
    for blank in blank_kinds {
        assert!(blank.draws_nothing());
        let cells = vec![solid(3), Some(Arc::new(blank))];
        assert_eq!(Image::from_table(2, cells.clone()).table(), (0, 0, 1, 1));
        assert_eq!(
            Image::from_tiles(4, 4, 2, cells).expect("whole").table(),
            (4, 4, 1, 1)
        );
    }
    let mut raw = [0u32; TILE_AREA];
    raw[17] = 9;
    assert!(!Tile::pack(&raw).expect("drawn").draws_nothing());
    assert!(!Tile::Solid(1).draws_nothing());
}

#[test]
fn a_full_table_of_nothing_is_blank() {
    assert!(Image::from_table(4, vec![None; 16]).is_blank());
    assert!(Image::from_table(0, Vec::new()).is_blank());
}

#[test]
fn fitting_a_canvas_counts_partial_tiles() {
    let image = drawn(&[(2, 1, 1)]);
    assert!(image.fits(33, 17));
    assert!(image.fits(48, 32));
    assert!(!image.fits(32, 17), "the tile starts past a 32 wide canvas");
    assert!(!image.fits(33, 16));
    assert!(Image::blank().fits(1, 1));
    assert!(!drawn(&[(-1, 0, 1)]).fits(800, 800));
}

#[test]
fn to_cells_clips_to_the_canvas() {
    let image = drawn(&[(1, 0, 6)]);
    let px = image.to_cells(20, 2);
    assert_eq!(px.len(), 40);
    assert!(px[..16].iter().all(|&c| c == 0));
    assert!(px[16..20].iter().all(|&c| c == 6));
    assert!(px[36..40].iter().all(|&c| c == 6));
}

#[test]
fn mapping_leaves_transparent_cells_alone() {
    let image = Image::from_table(
        1,
        vec![Tile::pack(&{
            let mut raw = [0; TILE_AREA];
            raw[1] = 5;
            raw
        })
        .map(Arc::new)],
    );
    let mapped = image.map_cells(|v| v + 1);
    assert_eq!((mapped.get(0, 0), mapped.get(1, 0)), (0, 6));
}

#[test]
fn mapped_values_that_become_nothing_trim_the_table() {
    let image = drawn(&[(0, 0, 3), (5, 5, 4)]);
    let mapped = image.map_cells(|v| if v == 4 { 0 } else { v + 1 });
    assert_eq!(mapped.table(), (0, 0, 1, 1));
    assert_eq!(mapped.get(0, 0), 4);
    assert_eq!(image.table(), (0, 0, 6, 6), "the original is left alone");
}

fn is_tight(image: &Image) -> bool {
    if image.is_blank() {
        return image.table() == (0, 0, 0, 0);
    }
    let (x, y, w, h) = image.table();
    let row = |ty: i32| (0..w as i32).any(|c| image.tile(x + c, ty).is_some());
    let column = |tx: i32| (0..h as i32).any(|r| image.tile(tx, y + r).is_some());
    image.cells.len() == w * h
        && image.cells.capacity() == w * h
        && row(y)
        && row(y + h as i32 - 1)
        && column(x)
        && column(x + w as i32 - 1)
}

fn expected_table(model: &BTreeMap<(i16, i16), u32>) -> (i32, i32, usize, usize) {
    if model.is_empty() {
        return (0, 0, 0, 0);
    }
    let bound = |f: &dyn Fn(&(i16, i16)) -> i16| {
        let values: Vec<i32> = model.keys().map(|k| f(k) as i32).collect();
        (
            *values.iter().min().expect("not empty"),
            *values.iter().max().expect("not empty"),
        )
    };
    let (x0, x1) = bound(&|k| k.0);
    let (y0, y1) = bound(&|k| k.1);
    (x0, y0, (x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize)
}

fn matches_model(image: &Image, model: &BTreeMap<(i16, i16), u32>) {
    assert!(is_tight(image), "table is not tight: {:?}", image.table());
    assert_eq!(image.table(), expected_table(model));
    for x in -12..12i16 {
        for y in -12..12i16 {
            let want = model.get(&(x, y)).copied().unwrap_or(0);
            assert_eq!(image.get(x as i32 * 16 + 5, y as i32 * 16 + 9), want);
        }
    }
}

/// Random writes against a plain map of what should be there, undoing as it goes.
#[test]
fn random_edits_match_a_plain_model_through_undo_and_redo() {
    let mut seed = 0x51ed_270bu32;
    let mut next = move |limit: u32| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) % limit
    };
    for _ in 0..300 {
        let mut image = Image::blank();
        let mut model = BTreeMap::new();
        let mut history = Vec::new();
        for _ in 0..40 {
            let writes = 1 + next(6) as usize;
            let mut entries = Vec::with_capacity(writes);
            for _ in 0..writes {
                let (x, y) = (next(24) as i16 - 12, next(24) as i16 - 12);
                let value = next(5);
                if entries.iter().all(|&(a, b, _)| (a, b) != (x, y)) {
                    entries.push((x, y, value));
                }
            }
            let before = (image.clone(), model.clone());
            for &(x, y, value) in &entries {
                match value {
                    0 => model.remove(&(x, y)),
                    v => model.insert((x, y), v),
                };
            }
            let undo = write(&mut image, &entries);
            matches_model(&image, &model);
            history.push((undo, before));
        }
        while let Some((mut undo, (image_before, model_before))) = history.pop() {
            image.swap_cells(&mut undo);
            assert_eq!(image, image_before);
            matches_model(&image, &model_before);
        }
        assert!(image.is_blank());
    }
}

#[test]
fn a_reader_gives_the_same_answers_as_reading_one_pixel_at_a_time() {
    let cells: Vec<u32> = (0..48 * 48).map(|i| i % 7).collect();
    let image = crate::grid::Grid::from_cells(48, 48, &cells).into_image();
    let mut reader = image.reader();
    for (x, y) in [
        (0, 0),
        (15, 0),
        (16, 0),
        (47, 47),
        (-1, 0),
        (0, -1),
        (48, 3),
        (5, 5),
        (5, 6),
    ] {
        assert_eq!(reader.get(x, y), image.get(x, y), "at {x},{y}");
    }
}

#[test]
fn a_positioned_table_is_kept_where_it_was_and_trimmed() {
    let tile = solid(4);
    let mut cells: Vec<Cell> = vec![None; 6];
    cells[4] = tile.clone();
    let image = Image::from_tiles(-3, 7, 3, cells).expect("whole rows");
    assert_eq!(image.table(), (-2, 8, 1, 1));
    assert!(Arc::ptr_eq(
        image.cells[0].as_ref().expect("kept"),
        tile.as_ref().expect("made")
    ));
    assert_eq!(
        Image::from_tiles(5, 5, 2, vec![None; 4]),
        Some(Image::blank())
    );
}

#[test]
fn a_positioned_table_must_be_whole_rows_inside_the_reach() {
    let reach = MAX_TILE_REACH as i16;
    assert_eq!(Image::from_tiles(0, 0, 3, vec![None; 4]), None);
    assert_eq!(Image::from_tiles(0, 0, 0, vec![None; 1]), None);
    assert_eq!(Image::from_tiles(0, 0, 0, Vec::new()), Some(Image::blank()));
    assert!(Image::from_tiles(reach - 1, -reach, 1, vec![solid(1)]).is_some());
    assert_eq!(Image::from_tiles(reach, 0, 1, vec![solid(1)]), None);
    assert_eq!(Image::from_tiles(0, -reach - 1, 1, vec![solid(1)]), None);
    assert_eq!(Image::from_tiles(reach - 1, 0, 2, vec![None; 2]), None);
    assert_eq!(Image::from_tiles(0, reach - 1, 1, vec![None; 2]), None);
}

#[test]
fn a_packed_tile_is_blank_exactly_when_it_decodes_to_nothing() {
    let packed = |palette: &[u32], bits: u8, data: Vec<u8>| Tile::Packed {
        palette: palette.into(),
        bits,
        data: data.into_boxed_slice(),
    };
    let bytes = |bits: usize| TILE_AREA * bits / 8;
    let mut one_shows = vec![0x11u8; bytes(4)];
    one_shows[bytes(4) - 1] = 0x10;
    let cases = [
        packed(&[5, 0], 4, vec![0x11; bytes(4)]),
        packed(&[5, 0], 4, one_shows),
        // Indices past the palette read as 0.
        packed(&[5], 2, vec![0xff; bytes(2)]),
        packed(&[5, 7], 1, vec![0x00; bytes(1)]),
        packed(&[0, 9], 8, vec![0; bytes(8)]),
        packed(&[0, 9], 8, vec![1; bytes(8)]),
        packed(&[0, 9], 1, vec![0; 3]),
        // A missing byte is index 0, which shows here.
        packed(&[5, 0], 1, vec![0xff; 3]),
        packed(&[5, 0], 4, Vec::new()),
        // Widths no file holds still agree with decoding.
        packed(&[0, 0, 0], 3, vec![0; 2]),
        packed(&[0, 6], 5, vec![0x21; 300]),
        packed(&[0], 7, vec![0; TILE_AREA]),
    ];
    for tile in cases {
        let mut raw = [0u32; TILE_AREA];
        tile.decode(&mut raw);
        let decodes_blank = raw.iter().all(|&v| v == 0);
        assert_eq!(tile.draws_nothing(), decodes_blank, "{tile:?}");
    }
}
