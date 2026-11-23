//! Frame ids and id floors on every frozen file: each opens numbered as before, and once it has
//! ids of its own, saving and opening keeps them, as does a build that carries the section along
//! without knowing it. The frame prints and layer checks must hold on every kind of file the corpus has: holds,
//! links, offsets, references, groups, tilemaps and Aseprite blending.

use pikslar_core::{Document, Editor, ForeignSection, FrameInfo};
use pikslar_format::piks;
use pikslar_safety::corpus::entries;
use pikslar_safety::craft::with_sections;
use pikslar_safety::picture::frame_hashes;
use std::sync::Arc;

fn frame_ids(doc: &Document) -> Vec<u32> {
    doc.frames.iter().map(FrameInfo::id).collect()
}

fn layer_ids(doc: &Document) -> Vec<u32> {
    let ids = doc.layers().iter().map(|l| l.persisted_id().get());
    ids.collect()
}

fn section_of(bytes: &[u8]) -> Option<Vec<u8>> {
    let at = bytes.windows(4).position(|w| w == b"Nids")?;
    let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?);
    Some(bytes[at + 8..at + 8 + len as usize].to_vec())
}

/// Every frozen file that is not a recovery copy, opened.
fn frozen() -> Vec<(String, Document)> {
    let all = entries().expect("the corpus is readable");
    all.into_iter()
        .filter(|e| !e.floating)
        .map(|e| {
            let bytes = e.bytes().expect("frozen file");
            let (doc, notes) = piks::load_noted(&bytes).expect("opens");
            assert!(!notes.frame_ids_renumbered, "{}", e.name);
            (e.name, doc)
        })
        .collect()
}

/// The document with ids opening a plain file would not give it.
fn numbered(doc: &Document) -> Document {
    let mut doc = doc.clone();
    doc.raise_layer_floor(doc.layer_floor() + 3);
    doc.raise_frame_floor(doc.frame_floor() + 5);
    doc.renumber_frames();
    doc
}

#[test]
fn frozen_files_open_numbered_from_1_with_no_section() {
    for (name, doc) in frozen() {
        let count = doc.frames.len() as u32;
        assert!(frame_ids(&doc).into_iter().eq(1..=count), "{name}");
        assert_eq!(doc.frame_floor(), count, "{name}");
        assert!(section_of(&piks::save(&doc)).is_none(), "{name}");
    }
}

#[test]
fn every_kind_of_file_keeps_its_ids_through_saving_and_opening() {
    for (name, doc) in frozen() {
        let doc = numbered(&doc);
        let bytes = piks::save(&doc);
        let (back, notes) = piks::load_noted(&bytes).expect("own save opens");
        assert!(!notes.frame_ids_renumbered, "{name}: the prints held");
        assert!(!notes.layer_ids_renumbered, "{name}: the layer checks held");
        assert_eq!(layer_ids(&back), layer_ids(&doc), "{name}");
        assert!(notes.damaged.is_empty(), "{name}");
        assert_eq!(frame_ids(&back), frame_ids(&doc), "{name}");
        assert_eq!(back.frame_floor(), doc.frame_floor(), "{name}");
        assert_eq!(back.layer_floor(), doc.layer_floor(), "{name}");
        assert_eq!(
            piks::save(&back),
            bytes,
            "{name}: saved again, byte for byte"
        );
        assert_eq!(
            frame_hashes(&mut Editor::new(back)),
            frame_hashes(&mut Editor::new(doc)),
            "{name}"
        );
    }
}

/// A build that does not know `Nids` writes it back unchanged after the sections it knows.
#[test]
fn every_kind_of_file_carried_through_an_older_build_keeps_its_ids() {
    for (name, doc) in frozen() {
        let doc = numbered(&doc);
        let section = section_of(&piks::save(&doc)).expect("written");
        let mut older = doc.clone();
        older.foreign.push(ForeignSection {
            tag: *b"Nids",
            data: Arc::from(&section[..]),
        });
        // What the older build saves: its own sections, without ids, then the carried one.
        let plain = piks::save_without_ids(&older);
        let (back, notes) = piks::load_noted(&plain).expect("opens");
        assert!(!notes.frame_ids_renumbered, "{name}");
        assert!(!notes.layer_ids_renumbered, "{name}");
        assert_eq!(frame_ids(&back), frame_ids(&doc), "{name}");
        assert_eq!(layer_ids(&back), layer_ids(&doc), "{name}");
        assert!(back.foreign.is_empty(), "{name}: read, not carried again");
    }
}

#[test]
fn a_damaged_section_on_a_frozen_file_costs_only_the_ids() {
    let all = entries().expect("the corpus is readable");
    let entry = all
        .iter()
        .find(|e| e.name == "v3-noise.piks")
        .expect("in the corpus");
    let bytes = entry.bytes().expect("frozen");
    let file = with_sections(&bytes, &[(b"Nids", &[1, 0, 0, 0, 9])]);
    let (doc, notes) = piks::load_noted(&file).expect("the art opens");
    assert_eq!(notes.damaged, ["frame and layer ids"]);
    assert_eq!(frame_hashes(&mut Editor::new(doc)), entry.hashes);
}
