//! Palette groups in a project: the `Pgrp` section, its limits, and how older builds treat it.

use crate::piks::{load_noted, save};
use crate::piks_palette::{decode, write_group, GROUPS, MAX_SECTION_BYTES};
use crate::piks_v3::encode;
use pikslar_core::color::MAX_COLORS;
use pikslar_core::{
    rgba, ColorMode, ColorTable, Document, DrawCmd, Editor, ForeignSection, GroupKind, PaletteCmd,
    PaletteGroup, SortBy, Tool, MAX_GROUPS, MAX_GROUP_NAME,
};
use std::sync::Arc;

const COLORS: [u32; 4] = [
    rgba(200, 0, 0, 255),
    rgba(0, 200, 0, 255),
    rgba(0, 0, 200, 255),
    rgba(90, 90, 90, 255),
];

fn grouped() -> Document {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Palette).expect("valid"));
    for color in COLORS {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    e.color = COLORS[1];
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(1, 1), (4, 1)],
    })
    .expect("stroke");
    e.apply(PaletteCmd::AddGroup {
        name: "Skin \u{e9}".into(),
        ids: Arc::from(vec![3, 1]),
    })
    .expect("group");
    e.apply(PaletteCmd::AddGroup {
        name: "Hair".into(),
        ids: Arc::from(vec![2]),
    })
    .expect("group");
    e.apply(PaletteCmd::Sort {
        by: SortBy::Lightness,
    })
    .expect("sort");
    e.doc().clone()
}

/// The same document with no groups.
fn bare(doc: &Document) -> Document {
    let mut doc = doc.clone();
    Arc::make_mut(&mut doc.colors).set_groups(Vec::new());
    doc
}

/// A file as this build would write the document, with `section` as its `Pgrp`.
fn with_pgrp(doc: &Document, section: Vec<u8>) -> Vec<u8> {
    encode(&bare(doc), &[(GROUPS, section)])
}

fn section_of(bytes: &[u8]) -> Vec<u8> {
    let at = bytes.windows(4).position(|w| w == GROUPS).expect("written");
    let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("four"));
    bytes[at + 8..at + 8 + len as usize].to_vec()
}

/// A section of one group of kind `kind`, with `name` and `ids`, and `extra` after its ids.
fn one_group(kind: u8, name: &str, ids: &[u32], extra: &[u8]) -> Vec<u8> {
    let mut record = vec![kind];
    record.extend_from_slice(&(name.len() as u16).to_le_bytes());
    record.extend_from_slice(name.as_bytes());
    record.extend_from_slice(&(ids.len() as u32).to_le_bytes());
    ids.iter()
        .for_each(|id| record.extend_from_slice(&id.to_le_bytes()));
    record.extend_from_slice(extra);
    let mut section = Vec::from(1u32.to_le_bytes());
    section.extend_from_slice(&(record.len() as u32).to_le_bytes());
    section.extend_from_slice(&record);
    section
}

#[test]
fn groups_and_swatch_order_survive_save_and_load() {
    let doc = grouped();
    let (back, notes) = load_noted(&save(&doc)).expect("loads");
    assert!(notes.can_save_over(), "{notes:?}");
    assert_eq!(back.colors.groups(), doc.colors.groups());
    assert_eq!(back.colors.swatches(), doc.colors.swatches());
    assert_eq!(back.colors.colors(), doc.colors.colors());
    assert!(back.foreign.is_empty(), "known, so not carried as foreign");
    assert_eq!(save(&back), save(&doc), "saving again gives the same bytes");
}

#[test]
fn a_ramp_keeps_its_kind() {
    let mut doc = grouped();
    Arc::make_mut(&mut doc.colors).set_groups(vec![PaletteGroup {
        name: "r".into(),
        kind: GroupKind::Ramp,
        ids: vec![1, 2],
    }]);
    let (back, _) = load_noted(&save(&doc)).expect("loads");
    assert_eq!(back.colors.groups()[0].kind, GroupKind::Ramp);
}

#[test]
fn a_ramp_made_by_the_command_survives_save_and_load_exactly() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Palette).expect("valid"));
    for color in [rgba(0, 0, 0, 255), rgba(255, 255, 255, 255)] {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    e.apply(PaletteCmd::AddRamp {
        from: 1,
        to: 2,
        steps: 7,
    })
    .expect("ramp");
    e.apply(PaletteCmd::AddToGroup { group: 0, id: 1 })
        .expect("no-op");
    let doc = e.doc();
    let (back, notes) = load_noted(&save(doc)).expect("loads");
    assert!(notes.can_save_over(), "{notes:?}");
    assert_eq!(back.colors.groups(), doc.colors.groups());
    assert_eq!(back.colors.groups()[0].kind, GroupKind::Ramp);
    assert_eq!(back.colors.groups()[0].ids.len(), 9);
    assert_eq!(save(&back), save(doc));
}

#[test]
fn no_groups_writes_no_section_and_the_same_bytes_as_before() {
    let doc = bare(&grouped());
    let bytes = save(&doc);
    assert!(!bytes.windows(4).any(|w| w == GROUPS));
    // Adding and removing a group leaves the file as if it never had one.
    let mut e = Editor::new(doc);
    e.apply(PaletteCmd::AddGroup {
        name: "g".into(),
        ids: Arc::from(vec![1]),
    })
    .expect("group");
    e.apply(PaletteCmd::RemoveGroup { group: 0 })
        .expect("removed");
    assert_eq!(save(e.doc()), bytes);
}

#[test]
fn project_files_made_before_groups_are_written_back_byte_for_byte() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pikslar-safety/corpus");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).expect("corpus") {
        let path = entry.expect("entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name.starts_with("v3-") && name.ends_with(".piks")) || name.contains("recovery") {
            continue;
        }
        let bytes = std::fs::read(&path).expect("read");
        let (doc, notes) = load_noted(&bytes).expect("loads");
        if notes.can_save_over() {
            assert_eq!(save(&doc), bytes, "{name} changed when saved again");
            checked += 1;
        }
    }
    assert!(checked >= 2, "only {checked} files were checked");
}

#[test]
fn the_tag_is_lowercase_so_older_builds_carry_it() {
    assert!(GROUPS[3].is_ascii_lowercase());
}

/// What a build that does not know `Pgrp` does: it holds the section as a carried one and writes
/// it back after the sections it knows. This build reads that file with the groups intact.
#[test]
fn a_file_an_older_build_carried_the_section_through_still_has_its_groups() {
    let doc = grouped();
    let section = section_of(&save(&doc));
    let mut older = bare(&doc);
    older.foreign.push(ForeignSection {
        tag: *GROUPS,
        data: Arc::from(&section[..]),
    });
    let carried = encode(&older, &[]);
    assert_eq!(section_of(&carried), section, "carried unchanged");
    let (back, notes) = load_noted(&carried).expect("loads");
    assert_eq!(back.colors.groups(), doc.colors.groups());
    assert!(notes.can_save_over());
    assert!(back.foreign.is_empty());
}

/// The older build's own rule, from `piks_sections`: only a lowercase last letter is carried.
#[test]
fn an_older_build_keeps_a_lowercase_section_and_reports_others() {
    let doc = grouped();
    let (kept, _) = load_noted(&encode(&doc, &[(b"zzzp", vec![1])])).expect("loads");
    assert_eq!(kept.foreign.len(), 1);
    let (_, lost) = load_noted(&encode(&doc, &[(b"zzzP", vec![1])])).expect("loads");
    assert_eq!(lost.not_kept, [*b"zzzP"]);
}

#[test]
fn ids_that_are_not_swatches_are_dropped_with_a_note_not_as_damage() {
    let doc = grouped();
    let section = one_group(0, "a", &[1, 99, 1, 0], &[]);
    let (back, notes) = load_noted(&with_pgrp(&doc, section)).expect("loads");
    assert_eq!(back.colors.groups()[0].ids, [1]);
    assert!(notes.damaged.is_empty());
    assert!(notes.palette_trimmed && !notes.can_save_over());
    assert!(notes.message().expect("told").contains("palette group"));
}

#[test]
fn a_section_that_does_not_parse_is_damage_and_costs_only_the_groups() {
    let doc = grouped();
    let good = section_of(&save(&doc));
    let mut trailing = good.clone();
    trailing.push(0);
    let mut cut = good.clone();
    cut.truncate(good.len() - 2);
    let mut bad_name = good.clone();
    // The first record's name starts after the count (4), length (4), kind (1) and name length (2).
    bad_name[11] = 0xff;
    let mut many = Vec::from(257u32.to_le_bytes());
    many.resize(40, 0);
    let mut huge_ids = good.clone();
    let count_at = 4 + 4 + 1 + 2 + "Skin \u{e9}".len();
    huge_ids[count_at..count_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    for (what, section) in [
        ("trailing", trailing),
        ("cut", cut),
        ("bad name", bad_name),
        ("too many", many),
        ("huge id count", huge_ids),
        ("empty", Vec::new()),
    ] {
        let (back, notes) = load_noted(&with_pgrp(&doc, section)).expect(what);
        assert_eq!(notes.damaged, ["palette groups"], "{what}");
        assert!(back.colors.groups().is_empty(), "{what}");
        assert_eq!(back.colors.swatches(), doc.colors.swatches(), "{what}");
        assert_eq!(back.layers(), doc.layers(), "{what}: the art is whole");
    }
}

#[test]
fn a_record_longer_than_this_version_reads_is_kept_and_noted() {
    let doc = grouped();
    let (back, notes) = load_noted(&with_pgrp(&doc, one_group(7, "", &[1], b"later"))).expect("ok");
    assert_eq!(back.colors.groups()[0].kind, GroupKind::Group);
    assert_eq!(back.colors.groups()[0].ids, [1]);
    assert!(notes.newer_fields && !notes.can_save_over());
}

#[test]
fn every_byte_of_the_section_changed_never_panics() {
    let doc = grouped();
    let good = section_of(&save(&doc));
    for at in 0..good.len() {
        for value in [0u8, 1, 0x7f, 0xff] {
            let mut s = good.clone();
            s[at] = value;
            let _ = load_noted(&with_pgrp(&doc, s));
        }
    }
}

/// A document whose palette is 65,535 swatches, as many as a table holds.
fn full_palette() -> Document {
    let mut doc = Document::new(8, 8, ColorMode::Palette).expect("valid");
    let colors: Vec<u32> = (0..MAX_COLORS as u32)
        .map(|n| rgba(n as u8, (n >> 8) as u8, 7, 255))
        .collect();
    let swatches: Vec<u32> = (1..MAX_COLORS as u32).collect();
    doc.colors = Arc::new(ColorTable::from_parts(&colors, &swatches));
    doc
}

/// A section of plain groups, one per list of ids.
fn groups_section(groups: &[Vec<u32>]) -> Vec<u8> {
    let mut section = Vec::from((groups.len() as u32).to_le_bytes());
    for ids in groups {
        let one = one_group(0, "g", ids, &[]);
        section.extend_from_slice(&one[4..]);
    }
    section
}

/// Reading groups costs time in step with the section's length, not with the swatches times
/// the ids: a million ids over a full palette once took seconds, and would take an hour at the
/// largest file.
#[test]
fn a_large_section_over_a_full_palette_reads_in_time_in_step_with_its_length() {
    let doc = full_palette();
    let every: Vec<u32> = (1..MAX_COLORS as u32).rev().collect();
    let file = with_pgrp(&doc, groups_section(&vec![every.clone(); 16]));
    let started = std::time::Instant::now();
    let (back, notes) = load_noted(&file).expect("loads");
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(2), "took {took:?}");
    assert!(notes.can_save_over(), "{notes:?}");
    assert_eq!(back.colors.groups().len(), 16);
    assert_eq!(back.colors.groups()[15].ids, every);
}

#[test]
fn a_group_naming_more_ids_than_a_table_holds_is_damage() {
    let doc = grouped();
    let section = groups_section(&[vec![1; MAX_COLORS + 1]]);
    let (back, notes) = load_noted(&with_pgrp(&doc, section)).expect("loads");
    assert_eq!(notes.damaged, ["palette groups"]);
    assert!(back.colors.groups().is_empty());
    // The most a group may name still reads.
    let section = groups_section(&[vec![1; MAX_COLORS]]);
    let (back, notes) = load_noted(&with_pgrp(&doc, section)).expect("loads");
    assert!(notes.damaged.is_empty());
    assert_eq!(back.colors.groups()[0].ids, [1]);
}

#[test]
fn a_section_longer_than_the_largest_one_this_build_writes_is_damage() {
    let largest = 4 + MAX_GROUPS * (4 + 1 + 2 + MAX_GROUP_NAME * 4 + 4 + MAX_COLORS * 4);
    assert!(MAX_SECTION_BYTES >= largest);
    let mut section = groups_section(&[vec![1]]);
    section.resize(MAX_SECTION_BYTES + 1, 0);
    assert!(decode(&section).is_err());
}

#[test]
fn a_ramp_between_clear_swatches_saves_and_can_be_saved_over() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Palette).expect("valid"));
    for color in [rgba(0, 0, 0, 255), rgba(255, 255, 255, 255)] {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    for id in [1, 2] {
        e.apply(PaletteCmd::Recolor { id, color: 0 })
            .expect("recoloured");
    }
    e.apply(PaletteCmd::AddRamp {
        from: 1,
        to: 2,
        steps: 3,
    })
    .expect("ramp");
    let (back, notes) = load_noted(&save(e.doc())).expect("loads");
    assert!(notes.can_save_over(), "{notes:?}");
    assert!(notes.message().is_none());
    assert_eq!(back.colors.groups(), e.doc().colors.groups());
}

#[test]
fn a_group_name_with_control_characters_is_cleaned_and_noted() {
    let doc = grouped();
    let section = one_group(0, "a\u{7}b", &[1], &[]);
    let (back, notes) = load_noted(&with_pgrp(&doc, section)).expect("loads");
    assert_eq!(back.colors.groups()[0].name, "a b");
    assert!(notes.palette_trimmed && notes.damaged.is_empty());
}

/// The writer's cut never splits a character, whatever name it is handed.
#[test]
fn a_long_name_is_cut_where_a_character_ends() {
    let group = PaletteGroup {
        name: format!("a{}", "\u{e9}".repeat(200)),
        kind: GroupKind::Group,
        ids: vec![1],
    };
    let mut out = crate::piks::Out(Vec::new());
    out.u32(1);
    write_group(&mut out, &group);
    let (groups, _) = decode(&out.0).expect("parses");
    assert!(groups[0].name.len() <= MAX_GROUP_NAME * 4);
    assert!(groups[0].name.starts_with("a\u{e9}"));
}

#[test]
fn two_sections_are_damage_and_neither_is_taken() {
    let doc = grouped();
    let good = section_of(&save(&doc));
    let other = one_group(0, "Other", &[1], &[]);
    let bytes = encode(&bare(&doc), &[(GROUPS, good), (GROUPS, other)]);
    let (back, notes) = load_noted(&bytes).expect("the art opens");
    assert_eq!(notes.damaged, ["palette groups"]);
    assert!(!notes.can_save_over());
    assert!(back.colors.groups().is_empty());
    assert_eq!(back.colors.swatches(), doc.colors.swatches());
}
