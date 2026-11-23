use super::*;
use crate::document::ColorMode;
use crate::grid::Grid;

fn id(n: u32) -> TileId {
    TileId::new(n).expect("a tile id")
}

/// A document with one 4 by 4 tileset of two tiles.
fn doc() -> (Document, TilesetId) {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Ground", ColorMode::Rgba, (4, 4))
        .expect("room");
    let image = Arc::new(Grid::from_cells(4, 4, &[0xff00_00ff; 16]).into_image());
    doc.add_tile(set, image.clone()).expect("fits");
    doc.add_tile(set, image).expect("fits");
    (doc, set)
}

#[test]
fn a_new_set_is_empty_named_and_numbered() {
    let (mut d, ts) = doc();
    assert_eq!(d.add_terrain(ts, TerrainKind::Edge4, "", false), Ok(1));
    assert_eq!(
        d.add_terrain(ts, TerrainKind::Blob47, " Water ", false),
        Ok(2)
    );
    let names: Vec<&str> = d
        .tileset(ts)
        .expect("set")
        .terrains()
        .iter()
        .map(TerrainSet::name)
        .collect();
    assert_eq!(names, ["Terrain 1", "Water"]);
    assert_eq!(d.tileset(ts).expect("set").len(), 2, "no tiles added");
    assert_eq!(
        d.add_terrain(TilesetId::new(9), TerrainKind::Edge4, "", false),
        Err(LimitError::NoSuchItem)
    );
}

#[test]
fn a_template_adds_a_blank_tile_for_every_slot_in_slot_order() {
    let (mut d, ts) = doc();
    let set = d
        .add_terrain(ts, TerrainKind::Blob47, "Grass", true)
        .expect("room");
    let tiles = d.tileset(ts).expect("set").tiles();
    assert_eq!(tiles.len(), 2 + 47);
    assert!(tiles[2..].iter().all(|t| t.image.is_blank()));
    let terrain = d.terrain(ts, set).expect("set");
    for (slot, entry) in tiles[2..].iter().enumerate() {
        assert_eq!(terrain.slot(slot), Some(entry.id), "slot {slot}");
    }
    assert_eq!(terrain.defined(), (1u64 << 47) - 1);
}

#[test]
fn a_template_with_no_room_for_its_tiles_is_refused_before_anything_changes() {
    let (mut d, ts) = doc();
    let blank = Arc::new(Image::blank());
    for _ in 2..MAX_TILES - 10 {
        d.add_tile(ts, blank.clone()).expect("room");
    }
    let before = d.clone();
    assert_eq!(
        d.add_terrain(ts, TerrainKind::Edge4, "", true),
        Err(LimitError::TooManyTiles)
    );
    assert_eq!(d, before);
    assert_eq!(d.add_terrain(ts, TerrainKind::Edge4, "", false), Ok(1));
}

#[test]
fn at_most_64_sets_a_tileset() {
    let (mut d, ts) = doc();
    for _ in 0..MAX_TERRAINS {
        d.add_terrain(ts, TerrainKind::Corner4, "", false)
            .expect("room");
    }
    assert_eq!(
        d.add_terrain(ts, TerrainKind::Corner4, "", false),
        Err(LimitError::TooManyTerrains)
    );
}

#[test]
fn renaming_cleans_the_name_and_says_when_nothing_changed() {
    let (mut d, ts) = doc();
    let set = d
        .add_terrain(ts, TerrainKind::Edge4, "Sand", false)
        .expect("room");
    assert_eq!(d.rename_terrain(ts, set, " Sand "), Ok(false));
    assert_eq!(d.rename_terrain(ts, set, "\n"), Ok(false));
    assert_eq!(d.rename_terrain(ts, set, "Dunes"), Ok(true));
    assert_eq!(d.terrain(ts, set).expect("set").name(), "Dunes");
    assert_eq!(d.rename_terrain(ts, 9, "x"), Err(LimitError::NoSuchItem));
}

#[test]
fn slots_take_only_tiles_of_the_tileset_and_only_slots_of_the_kind() {
    let (mut d, ts) = doc();
    let set = d
        .add_terrain(ts, TerrainKind::Edge4, "", false)
        .expect("room");
    assert_eq!(d.set_terrain_slot(ts, set, 0, Some(id(2))), Ok(true));
    assert_eq!(d.set_terrain_slot(ts, set, 0, Some(id(2))), Ok(false));
    assert_eq!(
        d.set_terrain_slot(ts, set, 0, Some(id(3))),
        Err(LimitError::NoSuchItem)
    );
    assert_eq!(
        d.set_terrain_slot(ts, set, 16, Some(id(1))),
        Err(LimitError::TerrainSlot)
    );
    assert_eq!(d.set_terrain_slot(ts, set, 0, None), Ok(true));
    assert_eq!(d.set_terrain_slot(ts, set, 0, None), Ok(false));
    assert_eq!(
        d.set_terrain_slot(ts, 7, 0, None),
        Err(LimitError::NoSuchItem)
    );
}

#[test]
fn removing_a_tile_empties_its_slots_in_every_set() {
    let (mut d, ts) = doc();
    let a = d
        .add_terrain(ts, TerrainKind::Edge4, "", false)
        .expect("room");
    let b = d
        .add_terrain(ts, TerrainKind::Blob47, "", false)
        .expect("room");
    d.set_terrain_slot(ts, a, 3, Some(id(1))).expect("slot");
    d.set_terrain_slot(ts, a, 4, Some(id(2))).expect("slot");
    d.set_terrain_slot(ts, b, 40, Some(id(1))).expect("slot");
    d.remove_tile(ts, id(1)).expect("removed");
    assert_eq!(d.terrain(ts, a).expect("a").slot(3), None);
    assert_eq!(d.terrain(ts, a).expect("a").slot(4), Some(id(2)));
    assert_eq!(d.terrain(ts, b).expect("b").defined(), 0);
}

#[test]
fn removing_a_set_keeps_its_tiles() {
    let (mut d, ts) = doc();
    let set = d
        .add_terrain(ts, TerrainKind::Edge4, "", true)
        .expect("room");
    d.remove_terrain(ts, set).expect("removed");
    assert!(d.tileset(ts).expect("set").terrains().is_empty());
    assert_eq!(d.tileset(ts).expect("set").len(), 18);
    assert_eq!(d.remove_terrain(ts, set), Err(LimitError::NoSuchItem));
}

#[test]
fn a_copy_of_a_tileset_keeps_its_terrains() {
    let (mut d, ts) = doc();
    let set = d
        .add_terrain(ts, TerrainKind::Edge4, "Road", false)
        .expect("room");
    d.set_terrain_slot(ts, set, 5, Some(id(2))).expect("slot");
    let copy = Arc::make_mut(&mut d.tilesets)
        .copy(ts, "Copy")
        .expect("room");
    assert_eq!(
        d.tileset(copy).expect("copy").terrains(),
        d.tileset(ts).expect("set").terrains()
    );
}

#[test]
fn loaded_sets_lose_slots_naming_gone_tiles_and_refuse_repeated_ids() {
    let (mut d, ts) = doc();
    let mut slots = vec![None; 16];
    slots[0] = Some(id(1));
    slots[1] = Some(id(99));
    let set = TerrainSet::from_parts(4, "Rock".into(), TerrainKind::Edge4, slots).expect("valid");
    assert_eq!(d.load_terrains(ts, vec![set.clone()]), Ok(true));
    let kept = d.terrain(ts, 4).expect("loaded");
    assert_eq!((kept.slot(0), kept.slot(1)), (Some(id(1)), None));
    let clean = d.tileset(ts).expect("set").terrains().to_vec();
    assert_eq!(d.load_terrains(ts, clean), Ok(false), "nothing to trim");
    let before = d.clone();
    assert_eq!(
        d.load_terrains(ts, vec![set.clone(), set]),
        Err(LimitError::TooManyTerrains)
    );
    assert_eq!(d, before);
}

#[test]
fn checking_loaded_sets_refuses_what_loading_would_and_changes_nothing() {
    let (d, ts) = doc();
    let set = TerrainSet::from_parts(4, "Rock".into(), TerrainKind::Edge4, vec![None; 16])
        .expect("valid");
    let before = d.clone();
    assert_eq!(d.check_terrains(ts, std::slice::from_ref(&set)), Ok(()));
    assert_eq!(
        d.check_terrains(ts, &[set.clone(), set]),
        Err(LimitError::TooManyTerrains)
    );
    assert_eq!(
        d.check_terrains(TilesetId::new(99), &[]),
        Err(LimitError::NoSuchItem)
    );
    assert_eq!(d, before);
}

#[test]
fn the_tilemap_rules_name_a_terrain_set_that_fills_a_slot_with_a_gone_tile() {
    use crate::tilemap::TilemapFault;
    let (mut d, ts) = doc();
    let mut slots = vec![None; 16];
    slots[0] = Some(id(1));
    let set = TerrainSet::from_parts(1, "Rock".into(), TerrainKind::Edge4, slots).expect("valid");
    d.load_terrains(ts, vec![set.clone()]).expect("loaded");
    assert_eq!(d.check_tilemaps(), Ok(()));
    let mut gone = set.clone();
    gone.set_slot(1, Some(id(50))).expect("slot");
    d.tileset_mut(ts).expect("set").terrains_mut().push(gone);
    let fault = Err(TilemapFault::Terrain { tileset: ts.get() });
    assert_eq!(d.check_tilemaps(), fault, "a gone tile, and a repeated id");
}
