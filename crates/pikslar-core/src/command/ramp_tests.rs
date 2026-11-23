//! Ramps and group membership through the command core: one undo step each, no id or pixel
//! touched.

use super::*;
use crate::color::{rgba, MAX_COLORS};
use crate::command::{DrawCmd, StepCmd};
use crate::document::ColorMode;
use crate::editor::Tool;

const BLACK: u32 = rgba(0, 0, 0, 255);
const WHITE: u32 = rgba(255, 255, 255, 255);
const MID_GREY: u32 = rgba(99, 99, 99, 255);

/// Black and white as swatches, and a few pixels of each.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Palette).expect("valid"));
    for color in [BLACK, WHITE] {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
        e.color = color;
        let row = if color == BLACK { 0 } else { 1 };
        let path = (0..4).map(|x| (x, row)).collect();
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        })
        .expect("stroke");
    }
    e.apply(StepCmd::EndDrag).expect("end");
    e
}

fn pixels(e: &Editor) -> Vec<u32> {
    let cel = e.doc.layers[0].cel(0);
    (0..64).map(|i| cel.get(i % 8, i / 8)).collect()
}

fn ramp(from: u32, to: u32, steps: u32) -> PaletteCmd {
    PaletteCmd::AddRamp { from, to, steps }
}

#[test]
fn a_ramp_adds_swatches_and_a_ramp_group_in_one_step() {
    let mut e = editor();
    let (colors, pix) = (e.doc.colors.colors().to_vec(), pixels(&e));
    let depth = e.history.undo_depth();
    let outcome = e.apply(ramp(1, 2, 3)).expect("ramp");
    assert_eq!(outcome.created, Some(0));
    assert_eq!(e.history.undo_depth(), depth + 1);
    let table = &e.doc.colors;
    assert_eq!(&table.colors()[..3], &colors[..], "no id changed");
    assert_eq!(pixels(&e), pix);
    let group = &table.groups()[0];
    assert_eq!(group.kind, GroupKind::Ramp);
    assert_eq!(group.ids, [1, 3, 4, 5, 2]);
    let shown: Vec<u32> = group.ids.iter().map(|&id| table.color(id)).collect();
    let (a, b, c) = (rgba(34, 34, 34, 255), MID_GREY, rgba(174, 174, 174, 255));
    assert_eq!(shown, [BLACK, a, b, c, WHITE]);
    assert_eq!(table.swatches(), [1, 2, 3, 4, 5]);
}

#[test]
fn undo_and_redo_give_back_identical_documents() {
    let mut e = editor();
    let (before_table, before_pix) = ((*e.doc.colors).clone(), pixels(&e));
    e.apply(ramp(1, 2, 3)).expect("ramp");
    let after_table = (*e.doc.colors).clone();
    assert!(e.undo());
    assert_eq!(*e.doc.colors, before_table);
    assert_eq!(pixels(&e), before_pix);
    assert!(e.redo());
    assert_eq!(*e.doc.colors, after_table);
    assert_eq!(pixels(&e), before_pix);
}

#[test]
fn a_colour_the_table_has_keeps_its_id() {
    let mut e = editor();
    let grey = e
        .apply(PaletteCmd::AddSwatch { color: MID_GREY })
        .expect("room")
        .created
        .expect("id");
    e.apply(ramp(1, 2, 3)).expect("ramp");
    let table = &e.doc.colors;
    assert_eq!(table.len(), 6, "the grey is not added twice");
    assert!(table.groups()[0].ids.contains(&grey));
    assert_eq!(table.groups()[0].ids.len(), 5);
}

#[test]
fn a_ramp_that_makes_no_new_colour_still_lists_what_it_has() {
    let mut e = editor();
    e.apply(ramp(1, 2, 1)).expect("ramp");
    e.apply(ramp(1, 2, 1)).expect("again");
    let table = &e.doc.colors;
    assert_eq!(table.len(), 4);
    assert_eq!(table.groups()[0].ids, table.groups()[1].ids);
}

#[test]
fn bad_ramps_are_refused_and_change_nothing() {
    let mut e = editor();
    let depth = e.history.undo_depth();
    let table = (*e.doc.colors).clone();
    assert_eq!(e.apply(ramp(1, 1, 3)), Err(LimitError::RampSteps));
    assert_eq!(e.apply(ramp(1, 2, 0)), Err(LimitError::RampSteps));
    assert_eq!(e.apply(ramp(1, 2, 255)), Err(LimitError::RampSteps));
    assert_eq!(e.apply(ramp(1, 9, 3)), Err(LimitError::NoSuchItem));
    assert_eq!(e.apply(ramp(0, 2, 3)), Err(LimitError::NoSuchItem));
    assert_eq!(e.history.undo_depth(), depth);
    assert_eq!(*e.doc.colors, table);
    assert!(e.apply(ramp(1, 2, 254)).is_ok(), "the most is allowed");
}

#[test]
fn a_full_table_refuses_the_whole_ramp() {
    let mut e = editor();
    let table = Arc::make_mut(&mut e.doc.colors);
    for n in 0..MAX_COLORS as u32 - 4 {
        table.id_for(0xff00_0000 | (n + 1)).expect("room");
    }
    let (before, depth) = ((*e.doc.colors).clone(), e.history.undo_depth());
    assert_eq!(e.apply(ramp(1, 2, 20)), Err(LimitError::TableFull));
    assert_eq!(*e.doc.colors, before, "no half ramp is left");
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn the_group_limit_stops_a_ramp() {
    let mut e = editor();
    for _ in 0..crate::color::MAX_GROUPS {
        e.apply(ramp(1, 2, 1)).expect("room");
    }
    assert_eq!(e.apply(ramp(1, 2, 1)), Err(LimitError::TooManyGroups));
}

fn with_group() -> Editor {
    let mut e = editor();
    e.apply(PaletteCmd::AddGroup {
        name: "g".into(),
        ids: Arc::from(vec![1]),
    })
    .expect("group");
    e
}

#[test]
fn adding_a_swatch_to_a_group_is_one_undoable_step() {
    let mut e = with_group();
    let before = (*e.doc.colors).clone();
    let depth = e.history.undo_depth();
    let outcome = e.apply(PaletteCmd::AddToGroup { group: 0, id: 2 });
    assert_eq!(outcome, Ok(Outcome::CHANGED));
    assert_eq!(e.doc.colors.groups()[0].ids, [1, 2]);
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert!(e.undo());
    assert_eq!(*e.doc.colors, before);
    assert!(e.redo());
    assert_eq!(e.doc.colors.groups()[0].ids, [1, 2]);
}

#[test]
fn removing_a_swatch_from_a_group_keeps_the_swatch() {
    let mut e = with_group();
    e.apply(PaletteCmd::AddToGroup { group: 0, id: 2 })
        .expect("added");
    let (before, depth) = ((*e.doc.colors).clone(), e.history.undo_depth());
    e.apply(PaletteCmd::RemoveFromGroup { group: 0, id: 1 })
        .expect("removed");
    assert_eq!(e.doc.colors.groups()[0].ids, [2]);
    assert_eq!(e.doc.colors.swatches(), [1, 2]);
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert!(e.undo());
    assert_eq!(*e.doc.colors, before);
}

#[test]
fn membership_changes_that_change_nothing_make_no_step() {
    let mut e = with_group();
    let depth = e.history.undo_depth();
    let same = e.apply(PaletteCmd::AddToGroup { group: 0, id: 1 });
    assert_eq!(same, Ok(Outcome::UNCHANGED));
    let absent = e.apply(PaletteCmd::RemoveFromGroup { group: 0, id: 2 });
    assert_eq!(absent, Ok(Outcome::UNCHANGED));
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn membership_commands_check_their_targets() {
    let mut e = with_group();
    let add = |group, id| PaletteCmd::AddToGroup { group, id };
    assert_eq!(e.apply(add(5, 2)), Err(LimitError::NoSuchItem));
    assert_eq!(e.apply(add(0, 9)), Err(LimitError::NoSuchItem));
    assert_eq!(e.apply(add(0, 0)), Err(LimitError::NoSuchItem));
    let remove = PaletteCmd::RemoveFromGroup { group: 5, id: 1 };
    assert_eq!(e.apply(remove), Err(LimitError::NoSuchItem));
}

#[test]
fn a_ramp_can_be_edited_like_any_group() {
    let mut e = editor();
    e.apply(ramp(1, 2, 3)).expect("ramp");
    e.apply(PaletteCmd::RemoveFromGroup { group: 0, id: 4 })
        .expect("removed");
    let group = &e.doc.colors.groups()[0];
    assert_eq!(group.kind, GroupKind::Ramp);
    assert_eq!(group.ids, [1, 3, 5, 2]);
}

/// A clear colour is id 0, which is never a swatch, so a ramp between two swatches made clear
/// holds only its two ends, and a file it is saved in reads back with nothing dropped.
#[test]
fn a_ramp_between_clear_swatches_names_only_real_swatches() {
    let mut e = editor();
    for id in [1, 2] {
        e.apply(PaletteCmd::Recolor { id, color: 0 })
            .expect("recoloured");
    }
    e.apply(StepCmd::EndDrag).expect("end");
    let swatches = e.doc.colors.swatches().to_vec();
    e.apply(ramp(1, 2, 5)).expect("ramp");
    let group = &e.doc.colors.groups()[0];
    assert_eq!(group.ids, [1, 2], "no id 0 among the members");
    assert_eq!(e.doc.colors.swatches(), swatches, "no swatch added");
}
