//! Terrain sets in a project file: round trips, carrying by a version that does not know them,
//! and the noted and damaged cases.

use super::TERRAINS;
use crate::crc32;
use crate::piks::{load_noted, save, try_save};
use crate::piks_sections::is_safe_to_copy;
use pikslar_core::autotile::{TerrainKind, TerrainSet};
use pikslar_core::orient::Orientation;
use pikslar_core::tilemap::MapCell;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{Cel, ColorMode, Document, ForeignSection, Grid, Image};
use std::sync::Arc;

fn id(n: u32) -> TileId {
    TileId::new(n).expect("an id")
}

fn tile_image(color: u32) -> Arc<Image> {
    Arc::new(Grid::from_cells(4, 4, &[color; 16]).into_image())
}

/// A tilemap layer of tileset 1 (tiles 1 and 2) with an Edge4 set "Road" (tile 1 in slots 0 and
/// 15, tile 2 in slot 5) and a Blob47 set "Water" made from a template, and a second tileset
/// with no terrain set.
fn with_terrains() -> (Document, TilesetId) {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Ground", ColorMode::Rgba, (4, 4))
        .expect("room");
    doc.add_tile(set, tile_image(0xff00_00ff)).expect("fits");
    doc.add_tile(set, tile_image(0xff00_ff00)).expect("fits");
    doc.add_tileset("Plain", ColorMode::Rgba, (4, 4))
        .expect("room");
    doc.add_tilemap_layer(1, set).expect("room");
    let place = |n| MapCell::place(id(n), Orientation::IDENTITY).bits();
    let map = Cel::new(Grid::from_cells(2, 2, &[place(1), place(2), 0, place(1)]));
    doc.put_cel(1, 0, map).expect("fits");
    let road = doc
        .add_terrain(set, TerrainKind::Edge4, "Road", false)
        .expect("room");
    for (slot, tile) in [(0, 1), (15, 1), (5, 2)] {
        doc.set_terrain_slot(set, road, slot, Some(id(tile)))
            .expect("slot");
    }
    doc.add_terrain(set, TerrainKind::Blob47, "Water", true)
        .expect("room");
    (doc, set)
}

fn resealed(mut bytes: Vec<u8>) -> Vec<u8> {
    let end = bytes.len() - 4;
    let crc = crc32(&[&bytes[..end]]);
    bytes[end..].copy_from_slice(&crc.to_le_bytes());
    bytes
}

/// Where the tag of a section is, the last one with this tag.
fn tag_at(bytes: &[u8], tag: &[u8; 4]) -> Option<usize> {
    bytes.windows(4).rposition(|w| w == tag)
}

/// The file with the data of its `Atil` section replaced, its length fixed and the file resealed.
fn with_data(bytes: &[u8], data: &[u8]) -> Vec<u8> {
    let at = tag_at(bytes, TERRAINS).expect("an Atil section");
    let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("4 bytes")) as usize;
    let mut out = bytes[..at + 4].to_vec();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&bytes[at + 8 + len..]);
    resealed(out)
}

fn section(bytes: &[u8]) -> Vec<u8> {
    let at = tag_at(bytes, TERRAINS).expect("an Atil section");
    let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("4 bytes")) as usize;
    bytes[at + 8..at + 8 + len].to_vec()
}

/// The file with every `from` tag of a section renamed `to`, resealed.
fn retagged(bytes: &[u8], from: &[u8; 4], to: &[u8; 4]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let at = tag_at(&out, from).expect("the section");
    out[at..at + 4].copy_from_slice(to);
    resealed(out)
}

fn terrains(doc: &Document, set: TilesetId) -> Vec<TerrainSet> {
    doc.tileset(set).expect("tileset").terrains().to_vec()
}

#[test]
fn terrain_sets_survive_save_and_load_byte_for_byte() {
    let (doc, set) = with_terrains();
    let bytes = try_save(&doc).expect("writable");
    let (back, notes) = load_noted(&bytes).expect("opens");
    assert!(notes.can_save_over(), "{notes:?}");
    assert_eq!(*back.tilesets, *doc.tilesets);
    assert_eq!(terrains(&back, set).len(), 2);
    back.check_tilemaps().expect("the rules hold");
    assert_eq!(save(&back), bytes, "saved again, byte for byte");
}

#[test]
fn a_document_without_terrain_sets_writes_no_section() {
    let (mut doc, set) = with_terrains();
    doc.load_terrains(set, Vec::new()).expect("cleared");
    assert_eq!(tag_at(&save(&doc), TERRAINS), None);
}

#[test]
fn the_section_comes_after_the_tilesets() {
    let (doc, _) = with_terrains();
    let bytes = save(&doc);
    let tilesets = tag_at(&bytes, b"TSET").expect("TSET");
    assert!(tag_at(&bytes, TERRAINS).expect("Atil") > tilesets);
}

#[test]
fn a_version_without_terrains_carries_the_section_unchanged() {
    // A version that does not know `Atil` sees an unknown section with a lowercase last letter.
    assert!(is_safe_to_copy(TERRAINS));
    let (doc, set) = with_terrains();
    let bytes = save(&doc);
    // Read as such a version would: under a tag this one does not know either.
    let unknown = retagged(&bytes, TERRAINS, b"Ztil");
    let (older, notes) = load_noted(&unknown).expect("opens");
    assert!(notes.can_save_over(), "{notes:?}");
    assert!(terrains(&older, set).is_empty());
    assert_eq!(older.foreign.len(), 1, "carried");
    let carried = retagged(&save(&older), b"Ztil", TERRAINS);
    assert_eq!(section(&carried), section(&bytes), "unchanged");
    let (back, notes) = load_noted(&carried).expect("opens");
    assert!(notes.can_save_over(), "{notes:?}");
    assert_eq!(terrains(&back, set), terrains(&doc, set));
}

#[test]
fn a_slot_naming_a_tile_removed_where_the_section_was_carried_is_emptied_with_a_note() {
    let (doc, set) = with_terrains();
    let carried = section(&save(&doc));
    // That version removed tile 1 and kept the section as it was.
    let mut older = doc.clone();
    older.remove_tile(set, id(1)).expect("removed");
    older.load_terrains(set, Vec::new()).expect("cleared");
    older.foreign.push(ForeignSection {
        tag: *TERRAINS,
        data: carried.into(),
    });
    let (back, notes) = load_noted(&save(&older)).expect("opens");
    assert!(
        notes.terrain_trimmed && notes.damaged.is_empty(),
        "{notes:?}"
    );
    assert!(!notes.can_save_over());
    let road = back.terrain(set, 1).expect("road");
    assert_eq!((road.slot(0), road.slot(5)), (None, Some(id(2))));
    back.check_tilemaps().expect("the rules hold");
}

#[test]
fn sets_of_a_tileset_removed_where_the_section_was_carried_are_left_out_with_a_note() {
    let (mut doc, set) = with_terrains();
    let other = doc
        .add_tileset("Gone", ColorMode::Rgba, (4, 4))
        .expect("room");
    doc.add_terrain(other, TerrainKind::Edge4, "x", false)
        .expect("room");
    let bytes = save(&doc);
    let carried = section(&bytes);
    doc.remove_tileset(other).expect("unused");
    doc.load_terrains(set, Vec::new()).expect("cleared");
    doc.foreign.push(ForeignSection {
        tag: *TERRAINS,
        data: carried.into(),
    });
    let (back, notes) = load_noted(&save(&doc)).expect("opens");
    assert!(
        notes.terrain_trimmed && notes.damaged.is_empty(),
        "{notes:?}"
    );
    assert_eq!(terrains(&back, set).len(), 2, "the others are kept");
}

/// The data of a section of one tileset record holding the given set records.
fn one_tileset(tileset: u32, sets: &[Vec<u8>]) -> Vec<u8> {
    let mut record = tileset.to_le_bytes().to_vec();
    record.extend_from_slice(&(sets.len() as u32).to_le_bytes());
    for set in sets {
        record.extend_from_slice(&(set.len() as u32).to_le_bytes());
        record.extend_from_slice(set);
    }
    let mut data = 1u32.to_le_bytes().to_vec();
    data.extend_from_slice(&(record.len() as u32).to_le_bytes());
    data.extend_from_slice(&record);
    data
}

/// A set record: id, kind, name, and slots.
fn set_record(set: u16, kind: u8, name: &[u8], slots: &[u32]) -> Vec<u8> {
    let mut r = set.to_le_bytes().to_vec();
    r.push(kind);
    r.extend_from_slice(&(name.len() as u16).to_le_bytes());
    r.extend_from_slice(name);
    r.push(slots.len() as u8);
    slots
        .iter()
        .for_each(|s| r.extend_from_slice(&s.to_le_bytes()));
    r
}

fn edge(set: u16, first: u32) -> Vec<u8> {
    let mut slots = [0u32; 16];
    slots[0] = first;
    set_record(set, 0, b"Road", &slots)
}

/// Loads the file with this section data, checking the art and tilesets came through.
fn load_with(data: &[u8]) -> (Document, crate::piks::LoadNotes) {
    let (doc, set) = with_terrains();
    let (back, notes) = load_noted(&with_data(&save(&doc), data)).expect("the art opens");
    let mut plain = doc.clone();
    plain.load_terrains(set, Vec::new()).expect("cleared");
    let mut seen = back.clone();
    seen.load_terrains(set, Vec::new()).expect("cleared");
    assert_eq!(save(&seen), save(&plain), "the art and tiles are untouched");
    back.check_tilemaps().expect("the rules hold");
    (back, notes)
}

#[test]
fn a_section_that_does_not_parse_is_left_out_whole_as_damage() {
    let (doc, _) = with_terrains();
    let good = section(&save(&doc));
    let mut trailing = good.clone();
    trailing.push(0);
    let bad = [
        good[..good.len() - 3].to_vec(),
        trailing,
        one_tileset(1, &[set_record(1, 0, b"Road", &[0; 15])]),
        one_tileset(1, &[edge(1, 1), edge(1, 2)]),
        one_tileset(1, &[set_record(1, 0, b"\xff\xfe", &[0; 16])]),
        one_tileset(1, &[edge(0, 1)]),
        65u32.to_le_bytes().to_vec(),
        Vec::new(),
    ];
    for data in bad {
        let (back, notes) = load_with(&data);
        assert_eq!(notes.damaged, ["terrain sets"], "{data:?}");
        assert!(back.tilesets.iter().all(|s| s.terrains().is_empty()));
    }
    let mut twice = one_tileset(1, &[edge(1, 1)]);
    twice[0] = 2;
    let record = twice[4..].to_vec();
    twice.extend_from_slice(&record);
    assert_eq!(
        load_with(&twice).1.damaged,
        ["terrain sets"],
        "a tileset twice"
    );
}

#[test]
fn two_sections_are_damage() {
    let (doc, _) = with_terrains();
    let bytes = save(&doc);
    let mut twice = doc.clone();
    twice.foreign.push(ForeignSection {
        tag: *TERRAINS,
        data: section(&bytes).into(),
    });
    let (back, notes) = load_noted(&save(&twice)).expect("the art opens");
    assert_eq!(notes.damaged, ["terrain sets"]);
    assert!(back.tilesets.iter().all(|s| s.terrains().is_empty()));
}

#[test]
fn a_newer_kind_or_newer_fields_are_skipped_with_a_note() {
    let mut longer = edge(3, 2);
    longer.extend_from_slice(&[9, 9]);
    let newer = set_record(4, 7, b"Lava", &[1, 2]);
    let (back, notes) = load_with(&one_tileset(1, &[longer, newer]));
    assert!(notes.newer_fields && notes.damaged.is_empty(), "{notes:?}");
    let sets = terrains(&back, TilesetId::new(1));
    assert_eq!(sets.len(), 1, "the newer kind is left out");
    assert_eq!((sets[0].id(), sets[0].slot(0)), (3, Some(id(2))));
}

#[test]
fn tile_ids_no_tileset_holds_and_unclean_names_are_noted_not_damage() {
    let mut slots = [0u32; 16];
    slots[1] = 1 << 28;
    slots[2] = 2;
    let odd = set_record(5, 0, b"  Mud\n", &slots);
    let (back, notes) = load_with(&one_tileset(1, &[odd]));
    assert!(
        notes.terrain_trimmed && notes.damaged.is_empty(),
        "{notes:?}"
    );
    let mud = back.terrain(TilesetId::new(1), 5).expect("kept");
    assert_eq!(
        (mud.name(), mud.slot(1), mud.slot(2)),
        ("Mud", None, Some(id(2)))
    );
}

#[test]
fn no_mutation_of_the_section_panics_and_what_opens_keeps_the_rules() {
    let (doc, _) = with_terrains();
    let bytes = save(&doc);
    let good = section(&bytes);
    let mut opened = 0;
    for at in 0..good.len() {
        for value in [0u8, 1, 2, 0x2f, 0x7f, 0xff, good[at] ^ 0x08] {
            let mut data = good.clone();
            data[at] = value;
            let Ok((back, _)) = load_noted(&with_data(&bytes, &data)) else {
                continue;
            };
            back.check_tilemaps().expect("the rules hold");
            let again = save(&back);
            let (_, notes) = load_noted(&again).expect("what was saved opens");
            assert!(notes.can_save_over(), "{notes:?}");
            opened += 1;
        }
    }
    assert!(opened > 0);
    for cut in 0..good.len() {
        let _ = load_noted(&with_data(&bytes, &good[..cut]));
    }
}
