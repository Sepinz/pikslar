//! Color ids a script run has seen are never given again while a document is open: a color
//! taken back by undo or a failed run leaves its id retired, and the next new color goes
//! above it. The interface on its own numbers colors as before. See `docs/stable-ids.md`.

use super::*;
use crate::command::{DrawCmd, PaletteCmd, StepCmd};
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};

const RED: u32 = rgba(220, 20, 20, 255);
const GREEN: u32 = rgba(20, 220, 20, 255);
const GOLD: u32 = rgba(220, 200, 20, 255);

fn editor() -> Editor {
    Editor::new(Document::new(8, 8, ColorMode::Palette).expect("a small canvas"))
}

fn dot(e: &mut Editor, color: u32, at: (i32, i32)) {
    e.color = color;
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![at],
    })
    .expect("a dot");
    e.apply(StepCmd::EndDrag).expect("ends");
}

fn id_of(e: &Editor, color: u32) -> u32 {
    e.doc().colors.id_for_existing(color).expect("in the table")
}

fn undo(e: &mut Editor) {
    e.apply(StepCmd::Undo).expect("undoes");
}

/// A script run that does nothing but look: it sees every id the table holds.
fn look(e: &mut Editor) {
    e.transaction(|_| Ok::<_, LimitError>(())).expect("runs");
}

fn add_swatch(e: &mut Editor, color: u32) -> u32 {
    let made = e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    made.created.expect("an id")
}

#[test]
fn the_interface_on_its_own_numbers_colors_as_before() {
    let mut e = editor();
    dot(&mut e, RED, (1, 1));
    let red = id_of(&e, RED);
    undo(&mut e);
    dot(&mut e, GREEN, (2, 2));
    assert_eq!(id_of(&e, GREEN), red, "no run saw the id, so it is free");
    assert_eq!(e.doc().colors.id_floor(), 0);
}

#[test]
fn a_color_a_run_saw_leaves_its_id_retired_after_undo() {
    let mut e = editor();
    dot(&mut e, RED, (1, 1));
    let red = id_of(&e, RED);
    look(&mut e);
    undo(&mut e);
    assert_eq!(e.doc().colors.id_for_existing(RED), None);
    dot(&mut e, GREEN, (2, 2));
    let green = id_of(&e, GREEN);
    assert!(green > red, "{green} goes above the retired {red}");
    let colors = &e.doc().colors;
    assert_eq!(colors.color(red), 0, "a retired id is transparent");
    assert!(!colors.swatches().contains(&red));
}

#[test]
fn a_run_retires_what_the_table_held_and_its_caller_what_it_handed_out() {
    let mut e = editor();
    let held = add_swatch(&mut e, GREEN);
    let mut given = 0;
    let failed = e.transaction(|s| {
        given = add_swatch(s, RED);
        Err::<(), _>(LimitError::NoSuchItem)
    });
    assert_eq!(failed, Err(LimitError::NoSuchItem));
    assert_eq!(
        e.doc().colors.id_floor(),
        held,
        "what the table held at the start"
    );
    // The run's new color is retired only by a caller that knows it handed the id out, as
    // `pikslar-api` does: a color a run only painted with leaves no gap.
    e.doc().colors.retire_up_to(given);
    assert_eq!(e.doc().colors.id_for_existing(RED), None);
    assert!(add_swatch(&mut e, GOLD) > given);
}

#[test]
fn undo_and_redo_never_lower_the_floor() {
    let mut e = editor();
    let a = add_swatch(&mut e, RED);
    look(&mut e);
    undo(&mut e);
    e.apply(StepCmd::Redo).expect("redoes");
    undo(&mut e);
    let b = add_swatch(&mut e, GOLD);
    assert_ne!(a, b);
    assert!(e.doc().colors.id_floor() >= a);
}

#[test]
fn copies_share_a_floor_and_new_tables_start_without_one() {
    let mut grown = ColorTable::new();
    let kept = grown.clone();
    grown.id_for(RED).expect("room");
    grown.id_for(GREEN).expect("room");
    kept.take_on_floor(&grown);
    assert_eq!(kept.id_floor(), 2);
    assert_eq!(grown.id_floor(), 2, "the copy it came from shares it");
    let mut back = kept.clone();
    assert_eq!(back.id_for(GOLD), Ok(3));
    assert_eq!(back.colors(), &[0, 0, 0, GOLD]);
    let fresh = ColorTable::from_parts(&[0, RED, GREEN], &[1, 2]);
    assert_eq!(fresh.id_floor(), 0);
    assert_eq!(ColorTable::new().id_floor(), 0);
}

#[test]
fn the_floor_is_not_part_of_what_a_table_is() {
    let plain = ColorTable::from_parts(&[0, RED], &[1]);
    let raised = plain.clone();
    let mut bigger = plain.clone();
    bigger.id_for(GREEN).expect("room");
    raised.take_on_floor(&bigger);
    assert_eq!(plain, ColorTable::from_parts(&[0, RED], &[1]));
    assert_eq!(raised, ColorTable::from_parts(&[0, RED], &[1]));
}

#[test]
fn a_table_that_is_not_a_copy_gives_no_floor() {
    let mut table = ColorTable::from_parts(&[0, RED, GREEN, GOLD], &[1, 2, 3]);
    let remapped = table.remapped(&[3], &[0, 0, 0, 1]);
    remapped.take_on_floor(&table);
    table.take_on_floor(&remapped);
    assert_eq!(remapped.id_floor(), 0, "a remap numbers afresh on purpose");
    assert_eq!(table.id_floor(), 0);
    assert_eq!(table.id_for(rgba(1, 2, 3, 255)), Ok(4));
}

#[test]
fn a_floor_at_the_limit_gives_the_first_free_id_as_before() {
    let mut full = ColorTable::new();
    let mut small = full.clone();
    for n in 1..MAX_COLORS as u32 {
        full.id_for(n | 0xff00_0000).expect("room");
    }
    small.take_on_floor(&full);
    assert_eq!(small.id_floor(), MAX_COLORS as u32 - 1);
    assert_eq!(small.id_for(RED), Ok(1));
}
