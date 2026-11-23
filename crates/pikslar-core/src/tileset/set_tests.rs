use super::*;
use crate::grid::Grid;

fn set(w: usize, h: usize) -> Tileset {
    Tileset::new(TilesetId::new(1), "Tiles".into(), ColorMode::Rgba, (w, h)).expect("a size")
}

fn drawn(w: usize, h: usize, value: u32) -> Arc<Image> {
    Arc::new(Grid::from_cells(w, h, &vec![value; w * h]).into_image())
}

#[test]
fn a_tile_is_one_to_256_pixels_each_way() {
    for (w, h) in [(0, 8), (8, 0), (257, 8), (8, 257)] {
        let made = Tileset::new(TilesetId::new(1), String::new(), ColorMode::Rgba, (w, h));
        assert_eq!(made.err(), Some(LimitError::TileSize), "{w} x {h}");
    }
    assert!(Tileset::new(TilesetId::new(1), String::new(), ColorMode::Rgba, (256, 1)).is_ok());
}

#[test]
fn ids_count_up_from_one_and_are_never_given_twice() {
    let mut s = set(8, 8);
    let a = s.insert_tile(0, drawn(8, 8, 1)).expect("room");
    let b = s.insert_tile(1, drawn(8, 8, 2)).expect("room");
    assert_eq!((a.get(), b.get()), (1, 2));
    s.remove_tile(b);
    let c = s.insert_tile(9, drawn(8, 8, 3)).expect("room");
    assert_eq!(c.get(), 3, "the removed id is not reused");
    assert_eq!(s.next_tile_id(), 4);
}

#[test]
fn the_index_follows_tiles_that_are_inserted_removed_and_moved() {
    let mut s = set(8, 8);
    let ids: Vec<TileId> = (0..4)
        .map(|n| s.insert_tile(n, drawn(8, 8, n as u32 + 1)).expect("room"))
        .collect();
    s.remove_tile(ids[1]);
    assert_eq!(s.position(ids[1]), None);
    assert_eq!(s.position(ids[2]), Some(1));
    assert!(s.move_tile(ids[3], 0));
    let order: Vec<TileId> = s.tiles().iter().map(|e| e.id).collect();
    assert_eq!(order, [ids[3], ids[0], ids[2]]);
    for (at, id) in order.iter().enumerate() {
        assert_eq!(s.position(*id), Some(at));
    }
    assert!(!s.move_tile(ids[3], 0), "already there");
}

#[test]
fn the_most_tiles_are_refused_and_ids_run_out_with_the_counter() {
    let mut s = set(1, 1);
    let blank = Arc::new(Image::blank());
    s.tiles = vec![
        TileEntry {
            id: TileId::new(1).expect("an id"),
            image: blank.clone()
        };
        MAX_TILES
    ];
    assert_eq!(
        s.insert_tile(0, blank.clone()),
        Err(LimitError::TooManyTiles)
    );
    let mut t = set(1, 1);
    t.next_tile = MAX_TILE_ID + 1;
    assert_eq!(t.insert_tile(0, blank), Err(LimitError::TooManyTiles));
}

#[test]
fn a_tile_holds_only_pixels_inside_its_size() {
    let s = set(4, 3);
    assert!(s.holds(&Image::blank()));
    assert!(s.holds(&drawn(4, 3, 7)));
    assert!(!s.holds(&drawn(5, 3, 7)));
    assert!(!s.holds(&drawn(4, 4, 7)));
}

#[test]
fn names_are_trimmed_cut_and_free_of_control_characters() {
    assert_eq!(clean_name("  Walls\n "), "Walls");
    let long = "é".repeat(MAX_TILESET_NAME + 5);
    assert_eq!(clean_name(&long).chars().count(), MAX_TILESET_NAME);
}

#[test]
fn tiles_added_at_the_end_are_indexed_without_a_rebuild_and_in_the_middle_with_one() {
    let mut s = set(2, 2);
    let mut ids = Vec::new();
    for n in 0..300u32 {
        ids.push(s.insert_tile(usize::MAX, drawn(2, 2, n + 1)).expect("room"));
    }
    let middle = s.insert_tile(5, drawn(2, 2, 999)).expect("room");
    assert_eq!(s.position(middle), Some(5));
    assert_eq!(s.position(ids[4]), Some(4));
    assert_eq!(
        s.position(ids[5]),
        Some(6),
        "a tile after the new one moved up"
    );
    for (at, entry) in s.tiles().iter().enumerate() {
        assert_eq!(s.position(entry.id), Some(at));
    }
}

#[test]
fn a_cleaned_name_is_clean_already() {
    let long = format!("{} {}", "w".repeat(MAX_TILESET_NAME - 1), "x".repeat(8));
    for name in ["Walls \u{1}", "\u{7}  Walls", "a \t\u{0} b", long.as_str()] {
        let once = clean_name(name);
        assert_eq!(clean_name(&once), once, "{name:?}");
        assert_eq!(once.trim(), once, "{name:?}");
    }
}

#[test]
fn names_lose_direction_and_zero_width_characters() {
    assert_eq!(clean_name("Wa\u{202E}ll\u{200B}s\u{2066}"), "Walls");
}
