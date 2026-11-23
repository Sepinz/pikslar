//! The `Nids` section: round trips, damage, limits and hand-made frames. What builds that do
//! not know it do is in `piks_ids_older_tests.rs`.

use super::{decode, matched_ids, Pair, DAMAGED, FRAME_FLOOR_CAP, IDS, LAYER_FLOOR_CAP, VERSION};
use crate::piks::{load, load_noted, save, LoadNotes};
use crate::piks_ids_hash::{frame_prints, layer_checks};
use crate::piks_v3::encode_with;
use pikslar_core::{
    rgba, ColorMode, Document, DrawCmd, Editor, FrameCmd, FrameInfo, LayerCmd, Tool, MAX_FRAMES,
    MAX_FRAME_ID,
};
use std::sync::Arc;

#[path = "piks_ids_older_tests.rs"]
mod older;

pub(super) fn frame_ids(doc: &Document) -> Vec<u32> {
    doc.frames.iter().map(FrameInfo::id).collect()
}

pub(super) fn layer_ids(doc: &Document) -> Vec<u32> {
    doc.layers()
        .iter()
        .map(|l| l.persisted_id().get())
        .collect()
}

pub(super) fn contains(bytes: &[u8], tag: &[u8; 4]) -> bool {
    bytes.windows(4).any(|w| w == tag)
}

pub(super) fn section_of(bytes: &[u8]) -> Vec<u8> {
    let at = bytes.windows(4).position(|w| w == IDS).expect("written");
    let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("four"));
    bytes[at + 8..at + 8 + len as usize].to_vec()
}

pub(super) fn dot(e: &mut Editor, frame: usize, x: i32) {
    e.frame = frame;
    e.color = rgba(200, 40, 10, 255);
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(x, 1)],
    })
    .expect("stroke");
}

pub(super) fn add_top_layer(e: &mut Editor) {
    let at = e.doc().layers().len();
    e.apply(LayerCmd::Add {
        at,
        mode: ColorMode::Rgba,
    })
    .expect("room");
}

pub(super) fn remove_top_layer(e: &mut Editor) {
    let top = e.doc().layers().len() - 1;
    let layer = e.layer_id(top).expect("a layer");
    e.apply(LayerCmd::Remove { layer }).expect("removes");
}

/// Three frames that look and play differently, moved and one removed, and the top layer of
/// two removed: every id and floor is something opening a plain file would not work out.
pub(super) fn edited() -> Editor {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    for at in 1..4 {
        e.apply(FrameCmd::Add { at, copy: false }).expect("room");
    }
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![40, 50, 60, 70],
    })
    .expect("retimes");
    dot(&mut e, 1, 2);
    e.apply(FrameCmd::Move { from: 0, to: 2 }).expect("moves");
    e.apply(FrameCmd::Remove { from: 3, to: 3 })
        .expect("removes");
    add_top_layer(&mut e);
    remove_top_layer(&mut e);
    e
}

/// The pairs a document's ids and prints or checks make.
fn paired(ids: &[u32], prints: &[u64]) -> Vec<Pair> {
    ids.iter().copied().zip(prints.iter().copied()).collect()
}

/// A section for `doc` with the given floors and frame ids, its layers listed as they are.
fn section(doc: &Document, floors: (u32, u32), ids: &[u32], extra: &[u8]) -> Vec<u8> {
    let layers = paired(&layer_ids(doc), &layer_checks(doc));
    let frames = paired(ids, &frame_prints(doc));
    with_lists(floors, &layers, &frames, extra)
}

fn with_lists(floors: (u32, u32), layers: &[Pair], frames: &[Pair], extra: &[u8]) -> Vec<u8> {
    let mut out = Vec::from(VERSION.to_le_bytes());
    out.extend_from_slice(&floors.0.to_le_bytes());
    out.extend_from_slice(&floors.1.to_le_bytes());
    for list in [layers, frames] {
        out.extend_from_slice(&(list.len() as u32).to_le_bytes());
        for (id, print) in list {
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&print.to_le_bytes());
        }
    }
    out.extend_from_slice(extra);
    out
}

/// `doc` saved with `section` as its only `Nids`.
fn with_section(doc: &Document, section: Vec<u8>) -> Vec<u8> {
    encode_with(doc, &[(IDS, section)], false)
}

#[test]
fn the_tag_is_lowercase_so_older_builds_carry_it() {
    assert!(IDS[3].is_ascii_lowercase());
}

#[test]
fn documents_that_never_used_the_ids_write_no_section() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    assert!(!contains(&save(e.doc()), IDS));
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    add_top_layer(&mut e);
    dot(&mut e, 0, 3);
    let bytes = save(e.doc());
    assert!(
        !contains(&bytes, IDS),
        "frames 1 to n, floors at the ids in use"
    );
    assert_eq!(save(&load(&bytes).expect("opens")), bytes);
}

#[test]
fn ids_and_floors_survive_save_and_load() {
    let e = edited();
    let bytes = save(e.doc());
    assert!(contains(&bytes, IDS));
    let (back, notes) = load_noted(&bytes).expect("opens");
    assert_eq!(notes, LoadNotes::default());
    assert_eq!(frame_ids(&back), frame_ids(e.doc()));
    assert_eq!(layer_ids(&back), layer_ids(e.doc()));
    assert_eq!(back.frame_floor(), e.doc().frame_floor());
    assert_eq!(back.layer_floor(), e.doc().layer_floor());
    assert!(back.foreign.is_empty(), "known, so not carried as foreign");
    assert_eq!(save(&back), bytes, "saved again, byte for byte");
}

#[test]
fn frames_that_look_alike_keep_their_ids_through_a_round_trip() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    for at in 1..4 {
        e.apply(FrameCmd::Add { at, copy: false }).expect("room");
    }
    e.apply(FrameCmd::Move { from: 0, to: 3 }).expect("moves");
    let prints = frame_prints(e.doc());
    assert!(prints.iter().all(|&p| p == prints[0]), "four blank frames");
    let (back, notes) = load_noted(&save(e.doc())).expect("opens");
    assert_eq!(notes, LoadNotes::default());
    assert_eq!(frame_ids(&back), frame_ids(e.doc()));
}

#[test]
fn after_reopening_no_removed_id_is_given_again() {
    let e = edited();
    let (frame_floor, layer_floor) = (e.doc().frame_floor(), e.doc().layer_floor());
    let mut back = Editor::new(load(&save(e.doc())).expect("opens"));
    add_top_layer(&mut back);
    let top = *layer_ids(back.doc()).last().expect("a layer");
    assert!(top > layer_floor, "{top} after {layer_floor}");
    back.apply(FrameCmd::Add { at: 0, copy: false })
        .expect("room");
    assert!(frame_ids(back.doc())[0] > frame_floor);
}

/// A file can give a layer an id past the cap, which raises the floor with it; once that layer
/// is gone no listed id is as high, so the floor written must be one a reader takes.
#[test]
fn floors_past_the_caps_are_written_so_the_file_opens_and_saves_the_same() {
    let mut doc = edited().doc().clone();
    doc.raise_layer_floor(LAYER_FLOOR_CAP + 1000);
    doc.raise_frame_floor(FRAME_FLOOR_CAP + 5);
    let bytes = save(&doc);
    let (back, notes) = load_noted(&bytes).expect("opens");
    assert_eq!(notes, LoadNotes::default(), "its own section is not damage");
    assert_eq!(frame_ids(&back), frame_ids(&doc));
    assert_eq!(layer_ids(&back), layer_ids(&doc));
    assert_eq!(back.layer_floor(), LAYER_FLOOR_CAP);
    assert_eq!(back.frame_floor(), FRAME_FLOOR_CAP);
    assert_eq!(save(&back), bytes, "saved again, byte for byte");
}

#[test]
fn recovery_copies_keep_the_ids() {
    let e = edited();
    let copy = crate::piks::save_recovery(e.doc(), None);
    let back = crate::piks::load_recovery(&copy).expect("opens").doc;
    assert_eq!(frame_ids(&back), frame_ids(e.doc()));
    assert_eq!(back.layer_floor(), e.doc().layer_floor());
}

#[test]
fn matching_keeps_the_ids_of_prints_that_are_as_many_as_before() {
    let saved = [(4, 10), (5, 20), (6, 20), (7, 30)];
    assert_eq!(
        matched_ids(&saved, &[10, 20, 20, 30]),
        [Some(4), Some(5), Some(6), Some(7)]
    );
    assert_eq!(
        matched_ids(&saved, &[30, 20, 10, 20]),
        [Some(7), Some(5), Some(4), Some(6)]
    );
    assert_eq!(matched_ids(&saved, &[10, 20, 30]), [Some(4), None, Some(7)]);
    assert_eq!(
        matched_ids(&saved, &[10, 20, 20, 20]),
        [Some(4), None, None, None]
    );
    assert_eq!(
        matched_ids(&saved, &[11, 20, 20, 30, 40]),
        [None, Some(5), Some(6), Some(7), None]
    );
    assert_eq!(matched_ids(&saved, &[]), []);
}

/// The matching as first written, with maps: what the sorted one must give.
fn matched_by_maps(saved: &[Pair], now: &[u64]) -> Vec<Option<u32>> {
    use std::collections::HashMap;
    let mut by_print: HashMap<u64, (Vec<u32>, usize)> = HashMap::new();
    for &(id, print) in saved {
        by_print.entry(print).or_default().0.push(id);
    }
    for print in now {
        if let Some(entry) = by_print.get_mut(print) {
            entry.1 += 1;
        }
    }
    let mut taken: HashMap<u64, usize> = HashMap::new();
    let mut id_for = |print: &u64| {
        let (ids, _) = by_print.get(print).filter(|(ids, n)| ids.len() == *n)?;
        let next = taken.entry(*print).or_insert(0);
        let id = ids.get(*next).copied();
        *next += 1;
        id
    };
    now.iter().map(&mut id_for).collect()
}

#[test]
fn matching_by_sorting_gives_what_matching_by_maps_gave() {
    let mut seed = 9u64;
    let mut next = |n: u64| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 33) % n
    };
    for round in 0..400 {
        let kinds = 1 + next(6);
        let print = |k: u64| if k == 0 { u64::MAX } else { k * 0x9e37_79b9 };
        let saved: Vec<Pair> = (0..next(12))
            .map(|i| (100 + i as u32 * 3 + round, print(next(kinds))))
            .collect();
        let now: Vec<u64> = (0..next(12)).map(|_| print(next(kinds))).collect();
        assert_eq!(matched_ids(&saved, &now), matched_by_maps(&saved, &now));
    }
}

pub(super) fn plain_three() -> Document {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    for at in 1..3 {
        e.apply(FrameCmd::Add { at, copy: false }).expect("room");
    }
    dot(&mut e, 1, 1);
    e.apply(FrameCmd::Retime {
        from: 2,
        durations: vec![120],
    })
    .expect("retimes");
    e.doc().clone()
}

fn damage_of(doc: &Document, section: Vec<u8>) -> (Document, LoadNotes) {
    let file = with_section(doc, section);
    let (back, notes) = load_noted(&file).expect("the art opens");
    assert_eq!(back.frames.len(), doc.frames.len());
    assert_eq!(back, *doc, "the art and timing are as saved");
    (back, notes)
}

/// A frame floor too high for any id to be listed under it, though in range.
const HIGH_FLOOR: u32 = FRAME_FLOOR_CAP + 1;

/// Each malformed section, and the frame ids it opens with: above the saved floors when those
/// can be read, so no id given before names a frame, otherwise 1 to n.
fn malformed(doc: &Document) -> Vec<(&'static str, Vec<u8>, [u32; 3])> {
    let ok = |floors, ids: &[u32]| section(doc, floors, ids, &[]);
    let layer = paired(&[1], &layer_checks(doc));
    let frames = paired(&[4, 5, 6], &frame_prints(doc));
    let lists = |layers: &[Pair]| with_lists((3, 9), layers, &frames, &[]);
    let set = |mut s: Vec<u8>, at: usize, value: u32| {
        s[at..at + 4].copy_from_slice(&value.to_le_bytes());
        s
    };
    let above = [10, 11, 12];
    let mut cases = vec![
        ("version 0", set(ok((1, 9), &[4, 5, 6]), 0, 0), [1, 2, 3]),
        ("duplicate ids", ok((1, 9), &[4, 4, 6]), above),
        ("id 0", ok((1, 9), &[0, 5, 6]), above),
        ("id above the floor", ok((1, 5), &[4, 5, 6]), [6, 7, 8]),
        ("layer id 0", lists(&[(0, layer[0].1)]), above),
        ("layer id above its floor", lists(&[(4, layer[0].1)]), above),
        ("repeated layer ids", lists(&[layer[0], layer[0]]), above),
        (
            "oversize layer count",
            set(ok((1, 9), &[4, 5, 6]), 12, 257),
            above,
        ),
        (
            "oversize count",
            set(ok((1, 9), &[]), 28, MAX_FRAMES as u32 + 1),
            above,
        ),
        (
            "count past the end",
            set(ok((1, 9), &[4, 5, 6]), 28, 4),
            above,
        ),
    ];
    cases.extend(floors_past_their_limits(doc));
    cases
}

/// Floors a file may not give. None can be used, so the frames are numbered 1 to n.
fn floors_past_their_limits(doc: &Document) -> Vec<(&'static str, Vec<u8>, [u32; 3])> {
    let ok = |floors, ids: &[u32]| section(doc, floors, ids, &[]);
    vec![
        (
            "layer floor past 2^24",
            ok((1 << 24 | 1, 9), &[4, 5, 6]),
            [1, 2, 3],
        ),
        (
            "frame floor past 2^31",
            ok((1, MAX_FRAME_ID + 1), &[]),
            [1, 2, 3],
        ),
        (
            "frame floor above the cap",
            ok((1, HIGH_FLOOR), &[4, 5, 6]),
            [1, 2, 3],
        ),
        (
            "layer floor above the cap",
            ok((LAYER_FLOOR_CAP + 1, 9), &[4, 5, 6]),
            [1, 2, 3],
        ),
    ]
}

#[test]
fn malformed_sections_are_damage_that_costs_only_the_ids() {
    let doc = plain_three();
    for (what, bad, expected) in malformed(&doc) {
        let (back, notes) = damage_of(&doc, bad);
        assert_eq!(notes.damaged, [DAMAGED], "{what}");
        assert!(!notes.can_save_over(), "{what}: the original is kept");
        assert_eq!(frame_ids(&back), expected, "{what}");
    }
}

#[test]
fn every_truncation_is_damage_and_opens() {
    let doc = plain_three();
    let good = section(&doc, (1, 9), &[4, 5, 6], &[]);
    for len in 0..good.len() {
        let (back, notes) = damage_of(&doc, good[..len].to_vec());
        assert_eq!(notes.damaged, [DAMAGED], "cut to {len}");
        let expected = if len < 12 { [1, 2, 3] } else { [10, 11, 12] };
        assert_eq!(frame_ids(&back), expected, "cut to {len}");
    }
}

#[test]
fn a_good_section_written_by_hand_is_taken() {
    let doc = plain_three();
    let (back, notes) = damage_of(&doc, section(&doc, (7, 9), &[4, 9, 6], &[]));
    assert_eq!(notes, LoadNotes::default());
    assert_eq!(frame_ids(&back), [4, 9, 6]);
    assert_eq!((back.layer_floor(), back.frame_floor()), (7, 9));
}

#[test]
fn a_list_for_fewer_frames_keeps_the_ids_it_has_prints_for() {
    let doc = plain_three();
    let prints = frame_prints(&doc);
    let layers = paired(&[1], &layer_checks(&doc));
    let frames = [(4, prints[0]), (5, prints[2])];
    let (back, notes) = damage_of(&doc, with_lists((1, 9), &layers, &frames, &[]));
    assert!(notes.damaged.is_empty() && notes.frame_ids_renumbered && notes.can_save_over());
    assert_eq!(frame_ids(&back), [4, 10, 5]);
}

#[test]
fn wrong_prints_renumber_above_the_floor() {
    let doc = plain_three();
    let layers = paired(&[1], &layer_checks(&doc));
    let frames = [(4, 1), (5, 2), (6, 3)];
    let (back, notes) = damage_of(&doc, with_lists((1, 9), &layers, &frames, &[]));
    assert!(notes.frame_ids_renumbered && notes.can_save_over());
    assert_eq!(frame_ids(&back), [10, 11, 12]);
}

#[test]
fn a_newer_version_or_newer_fields_are_noted_and_protect_the_file() {
    let doc = plain_three();
    let mut newer = section(&doc, (1, 9), &[4, 5, 6], &[]);
    newer[..4].copy_from_slice(&2u32.to_le_bytes());
    let (back, notes) = damage_of(&doc, newer);
    assert!(notes.newer_fields && notes.damaged.is_empty() && !notes.can_save_over());
    assert_eq!(
        frame_ids(&back),
        [10, 11, 12],
        "its floors, where every version keeps them, are used; its list is not"
    );
    let longer = section(&doc, (1, 9), &[4, 5, 6], b"later");
    let (back, notes) = damage_of(&doc, longer);
    assert!(notes.newer_fields && !notes.can_save_over());
    assert_eq!(
        frame_ids(&back),
        [4, 5, 6],
        "the fields this build reads are used"
    );
}

#[test]
fn two_sections_are_damage_and_neither_list_is_taken() {
    let doc = plain_three();
    let one = section(&doc, (1, 9), &[4, 5, 6], &[]);
    let file = encode_with(&doc, &[(IDS, one.clone()), (IDS, one)], false);
    let (back, notes) = load_noted(&file).expect("opens");
    assert_eq!(notes.damaged, [DAMAGED]);
    assert_eq!(
        frame_ids(&back),
        [10, 11, 12],
        "above the first one's floor"
    );
}

#[test]
fn every_byte_of_the_section_changed_never_panics_or_costs_the_art() {
    let e = edited();
    let good = section_of(&save(e.doc()));
    let plain = load(&encode_with(e.doc(), &[], false)).expect("opens");
    for at in 0..good.len() {
        for value in [0u8, 1, 0x7f, 0xff] {
            let mut s = good.clone();
            s[at] = value;
            let (back, _) = load_noted(&with_section(&plain, s)).expect("opens");
            assert_eq!(back, plain);
            let ids = frame_ids(&back);
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), ids.len(), "ids stay distinct");
            assert!(back.frame_floor() >= *sorted.last().expect("a frame"));
        }
    }
}

#[test]
fn decoding_reads_back_what_was_written() {
    let e = edited();
    let read = decode(&section_of(&save(e.doc())))
        .expect("parses")
        .expect("this version");
    assert_eq!(read.frame_floor, e.doc().frame_floor());
    assert_eq!(read.layer_floor, e.doc().layer_floor());
    let frames = paired(&frame_ids(e.doc()), &frame_prints(e.doc()));
    assert_eq!(read.frames, frames);
    let layers = paired(&layer_ids(e.doc()), &layer_checks(e.doc()));
    assert_eq!(read.layers, layers);
    assert!(!read.newer);
}

/// `doc.frames` is public, so code can push a frame that shares an id. The file still opens to
/// what it holds and saves the same again, so the save check that refuses anything else passes.
#[test]
fn hand_made_duplicate_frames_save_ids_that_open_as_saved() {
    let mut doc = edited().doc().clone();
    doc.frames.push(doc.frames[0]);
    for layer in doc.layers_mut() {
        let cel = layer.cels()[0].clone();
        let cels = layer.cels().iter().cloned().chain(std::iter::once(cel));
        *layer = layer.clone().with_cels(cels.collect());
    }
    let bytes = save(&doc);
    let (back, notes) = load_noted(&bytes).expect("opens");
    assert_eq!(notes, LoadNotes::default());
    let ids = frame_ids(&back);
    assert_eq!(ids[..3], frame_ids(&doc)[..3], "the first holder keeps it");
    assert!(ids[3] > doc.frame_floor(), "{ids:?}: above every id given");
    assert_eq!(save(&back), bytes, "saved again, byte for byte");
    let mut plain = plain_three();
    plain.frames.push(FrameInfo::new(100));
    for layer in plain.layers_mut() {
        let cels = layer
            .cels()
            .iter()
            .cloned()
            .chain(std::iter::once(pikslar_core::Cel::blank()));
        *layer = layer.clone().with_cels(cels.collect());
    }
    let bytes = save(&plain);
    assert!(
        !contains(&bytes, IDS),
        "a new frame with no id becomes frame 4"
    );
    assert_eq!(save(&load(&bytes).expect("opens")), bytes);
}

#[test]
fn an_aseprite_import_numbers_its_frames_from_1_and_saves_no_section() {
    use crate::ase_build::{plain, Ase};
    let ase = Ase::new(7, 5, 32)
        .frame(120, vec![plain("a")])
        .frame(80, vec![])
        .frame(60, vec![]);
    let (doc, _) = crate::import_ase(&ase.bytes()).expect("imports");
    assert_eq!(frame_ids(&doc), [1, 2, 3]);
    assert_eq!((doc.frame_floor(), doc.layer_floor()), (3, 1));
    assert!(!contains(&save(&doc), IDS));
}

/// Opening drops tiles that draw nothing and trims an image's table to its drawn tiles. The
/// prints are of what the frames show, so a drawing erased in memory but not yet tidied still
/// opens with its ids, and the save check, which refuses a save that opens with notes, passes.
#[test]
fn erased_and_untidy_drawings_keep_their_ids() {
    use pikslar_core::{Cel, Image, Tile};
    let mut e = edited();
    e.color = rgba(9, 9, 9, 255);
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(0, 0), (7, 7)],
    })
    .expect("stroke");
    e.apply(DrawCmd::Stroke {
        tool: Tool::Eraser,
        path: vec![(0, 0), (7, 7)],
    })
    .expect("erase");
    let mut doc = e.doc().clone();
    // An image whose table holds a tile that draws nothing beside one that draws.
    let cells = vec![
        Some(Arc::new(Tile::Solid(rgba(1, 2, 3, 255)))),
        Some(Arc::new(Tile::Solid(0))),
    ];
    let untidy = Image::from_tiles(0, 0, 2, cells).expect("in range");
    doc.put_cel(0, 1, Cel::from_image(untidy)).expect("placed");
    let bytes = save(&doc);
    let (back, notes) = load_noted(&bytes).expect("opens");
    assert_eq!(notes, LoadNotes::default());
    assert_eq!(frame_ids(&back), frame_ids(&doc));
    assert_eq!(layer_ids(&back), layer_ids(&doc));
    assert_eq!(save(&back), bytes);
}

/// The most frames there may be, all alike, with every id high: opening takes linear time.
fn full_with_section(floor: u32, listed: impl Fn(usize) -> u32, right: bool) -> Vec<u8> {
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
    while doc.frames.len() < MAX_FRAMES {
        doc.add_frame(0, false).expect("room");
    }
    let layers = paired(&layer_ids(&doc), &layer_checks(&doc));
    let ids: Vec<u32> = (0..MAX_FRAMES).map(listed).collect();
    let prints = match right {
        true => frame_prints(&doc),
        false => vec![7; MAX_FRAMES],
    };
    let frames = paired(&ids, &prints);
    with_section(&doc, with_lists((1, floor), &layers, &frames, &[]))
}

#[test]
fn a_file_asking_for_the_largest_floor_opens_quickly() {
    let high = |i: usize| MAX_FRAME_ID - i as u32;
    let files = [
        (
            "no id as high",
            full_with_section(MAX_FRAME_ID, |i| i as u32 + 1, true),
        ),
        ("an id as high", full_with_section(MAX_FRAME_ID, high, true)),
        (
            "an id as high, frames changed",
            full_with_section(MAX_FRAME_ID, high, false),
        ),
    ];
    for (what, file) in files {
        let started = std::time::Instant::now();
        let (doc, notes) = load_noted(&file).expect("opens");
        let mut e = Editor::new(doc);
        for _ in 0..200 {
            e.apply(FrameCmd::Remove { from: 0, to: 0 })
                .expect("removes");
            e.apply(FrameCmd::Add { at: 0, copy: false }).expect("room");
        }
        // Linear work is well under a second unoptimised; the old way took minutes.
        assert!(
            started.elapsed().as_secs() < 5,
            "{what}: {:?}",
            started.elapsed()
        );
        let mut ids = frame_ids(e.doc());
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), MAX_FRAMES, "{what}: distinct");
        let refused = notes.damaged == [DAMAGED];
        assert_eq!(refused, what == "no id as high", "{what}: {notes:?}");
    }
}

#[test]
fn renewed_ids_are_told_in_the_app_without_stopping_a_save_over_the_file() {
    let frames = LoadNotes {
        frame_ids_renumbered: true,
        ..LoadNotes::default()
    };
    let said = frames.message().expect("told");
    assert!(
        said.contains("new frame ids") && !said.contains("new name"),
        "{said}"
    );
    assert!(frames.can_save_over());
    let layers = LoadNotes {
        layer_ids_renumbered: true,
        ..LoadNotes::default()
    };
    let said = layers.message().expect("told");
    assert!(
        said.contains("new layer ids") && !said.contains("new name"),
        "{said}"
    );
    let both = LoadNotes {
        frame_ids_renumbered: true,
        damaged: vec![DAMAGED],
        ..LoadNotes::default()
    };
    let said = both.message().expect("told");
    assert!(
        said.contains("new frame ids") && said.contains("new name"),
        "{said}"
    );
    assert_eq!(LoadNotes::default().message(), None);
}
