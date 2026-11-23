//! Loading a palette's colours: Append, Replace and Load as colours. One undo step each, refused
//! before anything changes, and never a pixel changed behind the person's back.

use super::*;
use crate::color::{rgba, GroupKind, PaletteGroup, MAX_COLORS};
use crate::command::{Command, DrawCmd, PaletteCmd};
use crate::document::{ColorMode, Document};
use crate::editor::Tool;
use crate::palette_ops::load::{unlisted_used, LoadHow};

const RED: u32 = rgba(255, 0, 0, 255);
const GREEN: u32 = rgba(0, 255, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const GREY: u32 = rgba(128, 128, 128, 255);
const GOLD: u32 = rgba(255, 215, 0, 255);

/// Swatches red, green, blue; the first row of the drawing is red, green, blue pixels.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Palette).expect("valid"));
    for color in [RED, GREEN, BLUE] {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    for (x, color) in [RED, GREEN, BLUE].into_iter().enumerate() {
        e.color = color;
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(x as i32, 0)],
        })
        .expect("stroke");
    }
    e
}

fn load(e: &mut Editor, colors: &[u32], how: LoadHow) -> Result<Outcome, LimitError> {
    e.apply(PaletteCmd::Load {
        colors: colors.into(),
        how,
    })
}

fn shown(e: &Editor) -> Vec<u32> {
    let t = &e.doc.colors;
    t.swatches().iter().map(|&id| t.color(id)).collect()
}

fn ids(e: &Editor) -> Vec<u32> {
    e.doc.colors.swatches().to_vec()
}

/// What the first row of the drawing looks like.
fn row(e: &Editor) -> Vec<u32> {
    let cel = e.doc.layers[0].cel(0);
    (0..8).map(|x| e.doc.colors.color(cel.get(x, 0))).collect()
}

fn raw_row(e: &Editor) -> Vec<u32> {
    let cel = e.doc.layers[0].cel(0);
    (0..8).map(|x| cel.get(x, 0)).collect()
}

#[test]
fn append_adds_new_colours_as_swatches_and_reuses_known_ones() {
    let mut e = editor();
    let before = raw_row(&e);
    let outcome = load(&mut e, &[GOLD, GREEN, GREY], LoadHow::Append).expect("loads");
    assert!(outcome.changed);
    assert_eq!(shown(&e), [RED, GREEN, BLUE, GOLD, GREY]);
    assert_eq!(raw_row(&e), before, "no pixel id moves");
}

#[test]
fn append_makes_a_colour_the_table_has_but_not_as_a_swatch_into_one() {
    let mut e = editor();
    e.apply(PaletteCmd::RemoveSwatch { id: 2 })
        .expect("removes");
    let known = e.doc.colors.id_for_existing(GREEN).expect("still known");
    load(&mut e, &[GREEN], LoadHow::Append).expect("loads");
    assert_eq!(
        *ids(&e).last().expect("one"),
        known,
        "the same id comes back"
    );
}

#[test]
fn append_of_colours_already_swatches_is_no_step() {
    let mut e = editor();
    let depth = e.history.undo_depth();
    let outcome = load(&mut e, &[BLUE, RED], LoadHow::Append).expect("loads");
    assert!(!outcome.changed);
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn every_way_of_loading_is_one_undo_step_and_undo_restores_the_document() {
    for (colors, how) in [
        (vec![GOLD, GREY], LoadHow::Append),
        (vec![GOLD, GREY], LoadHow::Replace),
        (vec![GOLD, GREY, BLUE], LoadHow::AsColors),
    ] {
        let mut e = editor();
        let (swatches, table, pix) = (ids(&e), e.doc.colors.clone(), raw_row(&e));
        let depth = e.history.undo_depth();
        load(&mut e, &colors, how).expect("loads");
        assert_eq!(e.history.undo_depth(), depth + 1, "{how:?}");
        e.apply(Command::Step(crate::command::StepCmd::Undo))
            .expect("undo");
        assert_eq!(ids(&e), swatches, "{how:?}");
        assert_eq!(*e.doc.colors, *table, "{how:?}");
        assert_eq!(raw_row(&e), pix, "{how:?}");
    }
}

#[test]
fn replace_sets_the_swatches_and_leaves_every_pixel_as_it_looks() {
    let mut e = editor();
    let looks = row(&e);
    load(&mut e, &[GOLD, GREY, GREEN], LoadHow::Replace).expect("loads");
    assert_eq!(shown(&e), [GOLD, GREY, GREEN]);
    assert_eq!(
        row(&e),
        looks,
        "pixels of ids that are no longer swatches keep their colour"
    );
    assert_eq!(raw_row(&e)[..3], [1, 2, 3], "and keep their ids");
}

#[test]
fn replace_says_how_many_colours_in_use_stopped_being_swatches() {
    let mut e = editor();
    assert_eq!(unlisted_used(&e.doc), 0);
    load(&mut e, &[GOLD, GREEN], LoadHow::Replace).expect("loads");
    // Red and blue are still painted and no longer in the palette.
    assert_eq!(unlisted_used(&e.doc), 2);
}

#[test]
fn replace_drops_group_members_that_left_and_groups_left_empty() {
    let mut e = editor();
    let all: Arc<[u32]> = ids(&e).into();
    e.apply(PaletteCmd::AddGroup {
        name: "all".into(),
        ids: all,
    })
    .expect("group");
    e.apply(PaletteCmd::AddGroup {
        name: "red".into(),
        ids: vec![1].into(),
    })
    .expect("group");
    load(&mut e, &[GREEN, GOLD], LoadHow::Replace).expect("loads");
    let groups = e.doc.colors.groups();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "all");
    assert_eq!(groups[0].ids, [2]);
    assert_eq!(groups[0].kind, GroupKind::Group);
}

#[test]
fn replace_with_the_same_colours_in_the_same_order_is_no_step() {
    let mut e = editor();
    let depth = e.history.undo_depth();
    let outcome = load(&mut e, &[RED, GREEN, BLUE, GREEN], LoadHow::Replace).expect("loads");
    assert!(!outcome.changed);
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn replace_puts_swatches_in_the_files_order_without_repeats() {
    let mut e = editor();
    load(&mut e, &[BLUE, RED, BLUE], LoadHow::Replace).expect("loads");
    assert_eq!(shown(&e), [BLUE, RED]);
    assert_eq!(ids(&e), [3, 1], "ids are the colours' own");
}

#[test]
fn as_colors_recolours_ids_in_order_so_the_art_changes_and_no_id_moves() {
    let mut e = editor();
    let swatch_ids = ids(&e);
    let ids_before = raw_row(&e);
    load(&mut e, &[GOLD, GREY, RED], LoadHow::AsColors).expect("loads");
    assert_eq!(ids(&e), swatch_ids);
    assert_eq!(raw_row(&e), ids_before, "every pixel keeps its id");
    assert_eq!(
        row(&e)[..3],
        [GOLD, GREY, RED],
        "and so shows the new colour"
    );
}

#[test]
fn as_colors_can_swap_two_colours_and_the_lookup_stays_right() {
    let mut e = editor();
    load(&mut e, &[GREEN, RED, BLUE], LoadHow::AsColors).expect("loads");
    assert_eq!(e.doc.colors.id_for_existing(GREEN), Some(1));
    assert_eq!(e.doc.colors.id_for_existing(RED), Some(2));
    assert_eq!(e.doc.colors.id_for_existing(BLUE), Some(3));
    assert_eq!(e.doc.colors.id_for_existing(GOLD), None);
}

#[test]
fn as_colors_refuses_a_file_with_another_number_of_colours() {
    for colors in [vec![GOLD, GREY], vec![GOLD, GREY, RED, BLUE]] {
        let mut e = editor();
        let table = e.doc.colors.clone();
        let depth = e.history.undo_depth();
        let error = load(&mut e, &colors, LoadHow::AsColors).expect_err("refused");
        assert_eq!(error, LimitError::PaletteSize);
        assert_eq!(*e.doc.colors, *table);
        assert_eq!(e.history.undo_depth(), depth);
    }
}

#[test]
fn as_colors_with_the_same_colours_is_no_step() {
    let mut e = editor();
    let depth = e.history.undo_depth();
    let outcome = load(&mut e, &[RED, GREEN, BLUE], LoadHow::AsColors).expect("loads");
    assert!(!outcome.changed);
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn an_empty_list_is_refused_for_every_way() {
    for how in [LoadHow::Append, LoadHow::Replace, LoadHow::AsColors] {
        let mut e = editor();
        assert_eq!(load(&mut e, &[], how), Err(LimitError::PaletteNoColors));
    }
}

#[test]
fn loaded_colours_are_opaque_whatever_alpha_they_arrive_with() {
    let mut e = editor();
    load(&mut e, &[rgba(1, 2, 3, 40)], LoadHow::Append).expect("loads");
    assert_eq!(shown(&e).last(), Some(&rgba(1, 2, 3, 255)));
}

#[test]
fn a_full_table_refuses_before_changing_anything() {
    let mut e = editor();
    let fill: Vec<u32> = (0..MAX_COLORS as u32 - 4)
        .map(|i| rgba(i as u8, (i >> 8) as u8, 7, 255))
        .collect();
    load(&mut e, &fill, LoadHow::Append).expect("fits exactly");
    assert_eq!(e.doc.colors.len(), MAX_COLORS);
    let (table, depth) = (e.doc.colors.clone(), e.history.undo_depth());
    for how in [LoadHow::Append, LoadHow::Replace] {
        let error = load(&mut e, &[rgba(9, 9, 9, 255)], how).expect_err("full");
        assert_eq!(error, LimitError::TableFull, "{how:?}");
        assert_eq!(*e.doc.colors, *table);
        assert_eq!(e.history.undo_depth(), depth);
    }
}

#[test]
fn a_group_survives_append_untouched() {
    let mut e = editor();
    e.apply(PaletteCmd::AddGroup {
        name: "g".into(),
        ids: vec![1, 2].into(),
    })
    .expect("group");
    load(&mut e, &[GOLD], LoadHow::Append).expect("loads");
    assert_eq!(
        e.doc.colors.groups(),
        [PaletteGroup {
            name: "g".into(),
            kind: GroupKind::Group,
            ids: vec![1, 2]
        }]
    );
}
