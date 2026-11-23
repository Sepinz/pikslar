use super::*;
use crate::autotile::rules::pick_slot;
use crate::document::LimitError;

fn id(n: u32) -> TileId {
    TileId::new(n).expect("a tile id")
}

#[test]
fn a_new_set_has_one_empty_slot_for_each_of_its_kind() {
    for (kind, count) in [
        (TerrainKind::Edge4, 16),
        (TerrainKind::Corner4, 16),
        (TerrainKind::Blob47, 47),
    ] {
        let set = TerrainSet::new(1, "Grass".into(), kind);
        assert_eq!(set.slots().len(), count);
        assert!(set.slots().iter().all(Option::is_none));
        assert_eq!(set.defined(), 0);
    }
}

#[test]
fn from_parts_needs_an_id_and_the_kinds_slot_count() {
    let slots = |n| vec![None; n];
    assert!(TerrainSet::from_parts(1, "a".into(), TerrainKind::Blob47, slots(47)).is_some());
    assert!(TerrainSet::from_parts(0, "a".into(), TerrainKind::Blob47, slots(47)).is_none());
    assert!(TerrainSet::from_parts(1, "a".into(), TerrainKind::Blob47, slots(16)).is_none());
    assert!(TerrainSet::from_parts(1, "a".into(), TerrainKind::Edge4, slots(17)).is_none());
}

#[test]
fn slots_are_set_cleared_and_said_unchanged_when_they_hold_that_already() {
    let mut set = TerrainSet::new(1, "Grass".into(), TerrainKind::Edge4);
    assert_eq!(set.set_slot(3, Some(id(7))), Ok(true));
    assert_eq!(set.set_slot(3, Some(id(7))), Ok(false));
    assert_eq!(set.slot(3), Some(id(7)));
    assert_eq!(set.set_slot(16, Some(id(7))), Err(LimitError::TerrainSlot));
    assert_eq!(set.slot(16), None);
    assert_eq!(set.set_slot(3, None), Ok(true));
    assert_eq!(set.set_slot(3, None), Ok(false));
}

#[test]
fn defined_slots_feed_the_rule_engine() {
    let mut set = TerrainSet::new(1, "Grass".into(), TerrainKind::Blob47);
    set.set_slot(0, Some(id(1))).expect("slot");
    set.set_slot(46, Some(id(2))).expect("slot");
    assert_eq!(set.defined(), 1 | 1 << 46);
    assert_eq!(pick_slot(set.kind(), 46, set.defined()), Some(46));
    assert_eq!(pick_slot(set.kind(), 45, set.defined()), Some(46));
}

#[test]
fn forgetting_a_tile_empties_every_slot_it_fills() {
    let mut set = TerrainSet::new(1, "Grass".into(), TerrainKind::Corner4);
    for slot in [1, 5, 9] {
        set.set_slot(slot, Some(id(4))).expect("slot");
    }
    set.set_slot(2, Some(id(5))).expect("slot");
    assert!(set.holds(id(4)));
    assert!(set.forget(id(4)));
    assert!(!set.forget(id(4)));
    assert!(!set.holds(id(4)));
    assert_eq!(set.slot(2), Some(id(5)));
}

#[test]
fn names_are_cleaned_as_tileset_names_are() {
    assert_eq!(clean_terrain_name("  Grass\n "), "Grass");
    assert_eq!(clean_terrain_name(&"x".repeat(100)).len(), MAX_TERRAIN_NAME);
}

#[test]
fn new_ids_count_up_and_fill_a_gap_only_at_the_top() {
    let set = |n| TerrainSet::new(n, String::new(), TerrainKind::Edge4);
    assert_eq!(next_terrain_id(&[]), Some(1));
    assert_eq!(next_terrain_id(&[set(1), set(3)]), Some(4));
    assert_eq!(next_terrain_id(&[set(u16::MAX), set(1)]), Some(2));
    let full: Vec<TerrainSet> = (1..=MAX_TERRAINS as u16).map(set).collect();
    assert_eq!(next_terrain_id(&full), None);
}

#[test]
fn a_cleaned_terrain_name_is_clean_already() {
    let long = format!("{} {}", "g".repeat(MAX_TERRAIN_NAME - 1), "x".repeat(8));
    for name in ["terrain \u{1}", "\u{7}  Grass", long.as_str()] {
        let once = clean_terrain_name(name);
        assert_eq!(clean_terrain_name(&once), once, "{name:?}");
        assert_eq!(once.trim(), once, "{name:?}");
    }
}

#[test]
fn terrain_names_lose_direction_and_zero_width_characters() {
    assert_eq!(clean_terrain_name("Gr\u{202E}as\u{200B}s\u{2069}"), "Grass");
}
