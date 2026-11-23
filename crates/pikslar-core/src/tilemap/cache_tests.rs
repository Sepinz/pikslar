use super::*;
use crate::grid::Grid;
use crate::tile::{RawTile, TILE_AREA};
use crate::tilemap::oracle::{random_case, Case};
use crate::tileset::{TileEntry, TileId, Tilesets};

const SENTINEL: u32 = 0xdead_beef;

/// Every canvas tile of the case's map, read through `cache` and without it, side by side.
fn agree(case: &Case, cache: &mut TileCache, what: &str) {
    let layer = &case.doc.layers()[0];
    for ty in 0..4 {
        for tx in 0..5 {
            let (mut plain, mut kept) = ([SENTINEL; TILE_AREA], [SENTINEL; TILE_AREA]);
            let drawn = case.doc.layer_tile(layer, 0, tx, ty, &mut plain);
            let through = case
                .doc
                .layer_tile_cached((layer, 0), (tx, ty), &mut kept, cache);
            assert_eq!(drawn, through, "{what} tile {tx},{ty}");
            assert!(plain == kept, "{what} size {:?} tile {tx},{ty}", case.size);
        }
    }
}

#[test]
fn a_cached_read_is_the_plain_read_for_random_maps_of_every_tile_size() {
    // One cache for every case: the same tileset id comes back with other sizes and pixels.
    let mut cache = TileCache::new();
    for seed in 1..=200 {
        let case = random_case(seed, (70, 55));
        agree(&case, &mut cache, &format!("seed {seed}"));
        // Twice, so that the second read comes from what the first one kept.
        agree(&case, &mut cache, &format!("seed {seed} again"));
    }
    assert!(cache.held_bytes() > 0, "small tiles were kept");
    assert!(cache.held_bytes() <= MAX_CACHED_PIXELS * 4);
}

/// The first case with tiles no larger than one storage tile and at least one drawn tile.
fn small_case() -> Case {
    (1..)
        .map(|seed| random_case(seed, (70, 55)))
        .find(|c| c.size.0 <= TILE && c.size.1 <= TILE && c.tiles.iter().any(|t| t[0] != 0))
        .expect("a small case")
}

#[test]
fn a_tile_given_new_pixels_is_read_anew() {
    let mut case = small_case();
    let mut cache = TileCache::new();
    agree(&case, &mut cache, "before");
    let (w, h) = case.size;
    let new_pixels: Vec<u32> = (0..w * h).map(|i| 0xff10_2030 + i as u32).collect();
    let mut list = Vec::new();
    for n in 1..=case.tiles.len() as u32 {
        let image = Arc::new(Grid::from_cells(w, h, &new_pixels).into_image());
        list.push((TileId::new(n).expect("an id"), image));
    }
    case.doc.swap_tile_images(case.set, &mut list);
    for tile in case.tiles.iter_mut() {
        tile.clone_from(&new_pixels);
    }
    agree(&case, &mut cache, "after the edit");
    // Undoing it gives the first images back, and those are read again too.
    case.doc.swap_tile_images(case.set, &mut list);
    agree(&case, &mut cache, "after undoing it");
}

#[test]
fn a_tile_whose_pixels_do_not_fit_is_read_from_the_tileset() {
    let case = small_case();
    let mut doc = case.doc.clone();
    // The same tiles under ids so high that their place in the cache is past its limit.
    let set = doc.tilesets.get(case.set).expect("the set").clone();
    let far = MAX_CACHED_PIXELS as u32;
    let entries: Vec<TileEntry> = set
        .tiles()
        .iter()
        .map(|e| TileEntry {
            id: TileId::new(e.id.get() + far).expect("an id"),
            image: e.image.clone(),
        })
        .collect();
    let moved = Tileset::from_parts(
        set.id(),
        "T".into(),
        set.mode(),
        set.tile_size(),
        far + 100,
        entries,
    )
    .expect("a tileset");
    doc.tilesets = Tilesets::from_parts(doc.tilesets.next_id(), vec![moved])
        .expect("a table")
        .into();
    let layer = doc.layers()[0].clone();
    let cel = layer.cel(0).image().map_cells(|v| match v & 0x0fff_ffff {
        0 => v,
        _ => v + far,
    });
    let cel = crate::layer::Cel::at(cel, case.offset.0, case.offset.1).expect("in reach");
    doc.put_cel(0, 0, cel).expect("a cel");
    let mut cache = TileCache::new();
    let mut raw: RawTile = [SENTINEL; TILE_AREA];
    let mut plain: RawTile = [SENTINEL; TILE_AREA];
    for (tx, ty) in [(0, 0), (1, 1), (2, 1), (4, 3)] {
        let layer = &doc.layers()[0];
        let a = doc.layer_tile(layer, 0, tx, ty, &mut plain);
        let b = doc.layer_tile_cached((layer, 0), (tx, ty), &mut raw, &mut cache);
        assert_eq!((a, plain), (b, raw), "tile {tx},{ty}");
    }
    assert_eq!(cache.held_bytes(), 0, "nothing past the limit was kept");
}

#[test]
fn clearing_lets_go_of_every_pixel() {
    let case = small_case();
    let mut cache = TileCache::new();
    agree(&case, &mut cache, "filled");
    assert!(cache.held_bytes() > 0);
    cache.clear();
    assert_eq!(cache.held_bytes(), 0);
    agree(&case, &mut cache, "refilled");
}
