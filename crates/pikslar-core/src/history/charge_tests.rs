//! What history says its steps keep, compared with what they do keep.

use crate::color::rgba;
use crate::document::{ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::{CelCmd, FrameCmd, LayerCmd, PaletteCmd, TagCmd, Turn};

/// A document with a drawing that fills many tiles on every frame of the first layer.
fn drawn(frames: usize, layers: usize) -> Editor {
    let mut e = Editor::new(Document::new(256, 256, ColorMode::Rgba).expect("valid"));
    for at in 1..layers {
        e.transact(|d| d.add_layer(at, ColorMode::Rgba))
            .expect("room");
    }
    e.layer = 0;
    e.frame = 0;
    for (n, y) in (0..256).step_by(8).enumerate() {
        e.color = rgba(n as u8 * 7 + 1, 90, 200, 255);
        e.begin(Tool::Line, 0, y).expect("begin");
        e.drag(255, y + 3);
        e.commit();
    }
    for at in 1..frames {
        e.apply(FrameCmd::Add { at, copy: true }).expect("room");
    }
    e
}

#[test]
fn a_drawing_kept_by_two_steps_is_charged_once() {
    let mut e = drawn(2, 1);
    e.apply(FrameCmd::Add { at: 2, copy: false }).expect("room");
    let drawing = e.doc().layers()[0].cel(0).heap_size();
    let before = e.history().memory_used();
    // The first removal is charged nothing: the copy still shows the drawing.
    e.apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removed");
    // Now only history keeps it.
    e.apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removed");
    let charged = e.history().memory_used() - before;
    assert!(
        charged >= drawing,
        "charged {charged} for a drawing of {drawing}"
    );
    assert!(
        charged < drawing * 3 / 2,
        "charged {charged} for a drawing of {drawing}"
    );
}

#[test]
fn a_removed_layer_is_charged_while_removed_and_not_once_back() {
    let mut e = drawn(1, 2);
    let drawing = e.doc().layers()[0].cel(0).heap_size();
    let layer = e.layer_id(0).expect("layer");
    let before = e.history().memory_used();
    e.apply(LayerCmd::Remove { layer }).expect("removed");
    let removed = e.history().memory_used() - before;
    assert!(
        removed >= drawing,
        "removed: charged {removed} for {drawing}"
    );
    assert!(e.undo());
    let back = e.history().memory_used() - before;
    assert!(back < drawing / 4, "back: charged {back} for {drawing}");
    assert!(e.redo());
    let again = e.history().memory_used() - before;
    assert!(again >= drawing, "again: charged {again} for {drawing}");
}

#[test]
fn undoing_a_turn_charges_redo_for_the_turned_drawing() {
    let mut e = drawn(2, 1);
    let layer = e.layer_id(0).expect("layer");
    let before = e.history().memory_used();
    // The turn keeps the drawing from before, which the copy on frame 1 still shows.
    e.apply(CelCmd::Turn {
        layer,
        frame: 0,
        turn: Turn::FlipHorizontal,
    })
    .expect("turned");
    let turned = e.doc().layers()[0].cel(0).heap_size();
    assert!(e.history().memory_used() - before < turned / 4);
    // Now the turned drawing is kept by redo alone.
    assert!(e.undo());
    let charged = e.history().memory_used() - before;
    assert!(charged >= turned, "charged {charged} for {turned}");
}

#[test]
fn one_step_keeping_a_drawing_twice_is_charged_for_it_once() {
    let mut e = drawn(2, 1);
    e.apply(FrameCmd::Add { at: 2, copy: false }).expect("room");
    let drawing = e.doc().layers()[0].cel(0).heap_size();
    let before = e.history().memory_used();
    // Both frames show one drawing, which only this step keeps afterwards.
    e.apply(FrameCmd::Remove { from: 0, to: 1 })
        .expect("removed");
    let charged = e.history().memory_used() - before;
    assert!(charged >= drawing, "charged {charged} for {drawing}");
    assert!(charged < drawing * 3 / 2, "charged {charged} for {drawing}");
}

#[test]
fn undoing_and_redoing_a_step_charges_as_before() {
    let mut e = drawn(2, 1);
    let layer = e.layer_id(0).expect("layer");
    for frame in 0..2 {
        e.apply(CelCmd::Turn {
            layer,
            frame,
            turn: Turn::FlipHorizontal,
        })
        .expect("turned");
    }
    let charged = e.history().used;
    assert!(e.undo());
    assert!(e.redo());
    assert_eq!(e.history().used, charged);
}

/// Removes frame 0, which then only history keeps, and says what that is charged.
fn charge_for_removing_the_first(e: &mut Editor) -> usize {
    let before = e.history().memory_used();
    e.apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removed");
    e.history().memory_used() - before
}

#[test]
fn a_step_dropped_for_room_no_longer_keeps_its_drawing() {
    let mut e = drawn(2, 1);
    let drawing = e.doc().layers()[0].cel(0).heap_size();
    e.apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removed");
    let budget = e.history.budget;
    e.history.budget = 0;
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.history.budget = budget;
    let charged = charge_for_removing_the_first(&mut e);
    assert!(charged >= drawing, "charged {charged} for {drawing}");
}

#[test]
fn a_step_taken_back_no_longer_keeps_its_drawing() {
    let mut e = drawn(2, 1);
    let drawing = e.doc().layers()[0].cel(0).heap_size();
    e.apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removed");
    let mut place = (e.layer, e.frame);
    assert!(e
        .history
        .discard_last(&mut e.doc, &mut e.selection, &mut place));
    e.apply(FrameCmd::Add { at: 2, copy: false }).expect("room");
    charge_for_removing_the_first(&mut e);
    let charged = charge_for_removing_the_first(&mut e);
    assert!(charged >= drawing, "charged {charged} for {drawing}");
}

#[test]
fn what_history_says_it_keeps_includes_its_own_lists() {
    let mut e = drawn(2, 1);
    let layer = e.layer_id(0).expect("layer");
    for n in 0..300 {
        e.apply(CelCmd::Move {
            layer,
            frame: n % 2,
            dx: 1,
            dy: 0,
        })
        .expect("moved");
        e.apply(crate::StepCmd::EndDrag).expect("end");
    }
    let h = e.history();
    let slot = size_of::<(super::Edit, usize)>();
    // Each step's room is in what the steps are charged, and the room spare is counted once.
    let spare = (h.undo.capacity() - h.undo.len()) * slot;
    assert!(spare > 0, "the list has room to spare");
    assert_eq!(h.memory_used(), h.used + spare + h.held.bytes());
    assert!(h.used >= h.undo.len() * slot);
}

#[test]
fn a_step_is_not_charged_again_for_the_room_its_list_gives_it() {
    let mut e = drawn(1, 1);
    let layer = e.layer_id(0).expect("layer");
    let before = e.history().steps_used();
    e.apply(CelCmd::Move {
        layer,
        frame: 0,
        dx: 3,
        dy: 1,
    })
    .expect("moved");
    // The step is charged for its room in history's list and the frame it names, and for
    // nothing else.
    let slot = size_of::<(super::Edit, usize)>();
    assert_eq!(e.history().steps_used() - before, slot + size_of::<u32>());
}

#[test]
fn a_dot_on_a_blank_cel_is_charged_for_the_change_it_keeps() {
    let mut e = drawn(1, 2);
    e.layer = 1;
    let before = e.history().steps_used();
    e.begin(Tool::Pencil, 4, 4).expect("begin");
    e.commit();
    let change = size_of::<(crate::image::TileKey, crate::grid::Cell)>();
    assert!(e.history().steps_used() - before >= change);
}

#[test]
fn a_stroke_that_adds_a_color_is_charged_for_the_table_it_keeps() {
    let mut e = Editor::new(Document::new(64, 64, ColorMode::Palette).expect("valid"));
    for n in 1..100u8 {
        std::sync::Arc::make_mut(&mut e.doc.colors)
            .id_for(rgba(n, 3, 4, 255))
            .expect("room");
    }
    let table = e.doc().colors.kept_bytes();
    let before = e.history().steps_used();
    e.color = rgba(250, 250, 1, 255);
    e.begin(Tool::Pencil, 4, 4).expect("begin");
    e.commit();
    assert!(e.history().steps_used() - before >= table);
}

#[test]
fn a_swatch_step_is_charged_for_the_table_it_keeps() {
    let mut e = drawn(1, 1);
    for n in 1..100u8 {
        std::sync::Arc::make_mut(&mut e.doc.colors)
            .id_for(rgba(n, 3, 4, 255))
            .expect("room");
    }
    let table = e.doc().colors.kept_bytes();
    let before = e.history().steps_used();
    e.apply(PaletteCmd::AddSwatch {
        color: rgba(9, 9, 9, 255),
    })
    .expect("room");
    assert!(e.history().steps_used() - before >= table);
}

#[test]
fn a_structural_step_is_charged_for_the_tags_it_keeps() {
    let mut e = drawn(2, 1);
    for _ in 0..40 {
        e.apply(TagCmd::Add {
            name: "walk".into(),
            from: 0,
            to: 1,
        })
        .expect("room");
    }
    let layer = e.layer_id(0).expect("layer");
    let before = e.history().steps_used();
    e.apply(LayerCmd::SetOpacity { layer, opacity: 9 })
        .expect("set");
    let tags = 40 * size_of::<crate::animation::Tag>();
    assert!(e.history().steps_used() - before >= tags);
}

/// Paints one dot on the active cel as one step.
fn dot(e: &mut Editor, x: i32, y: i32) {
    e.color = rgba(250, 250, 10, 255);
    e.begin(Tool::Pencil, x, y).expect("begin");
    e.commit();
}

#[test]
fn a_stroke_that_leaves_a_drawing_to_history_alone_is_charged_for_it() {
    let mut e = drawn(2, 1);
    let drawing = e.doc().layers()[0].cel(0).heap_size();
    // Charged nothing: the copy on frame 1 still shows the drawing.
    e.apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removed");
    e.frame = 0;
    let before = e.history().memory_used();
    // The dot splits the drawing from the frame that showed it, so only the removal keeps it.
    dot(&mut e, 3, 3);
    let charged = e.history().memory_used() - before;
    assert!(
        charged >= drawing,
        "charged {charged} for a drawing of {drawing}"
    );
    assert!(
        charged < drawing * 3 / 2,
        "charged {charged} for a drawing of {drawing}"
    );
}

#[test]
fn a_stroke_that_splits_a_drawing_another_frame_still_shows_is_not_charged_for_it() {
    let mut e = drawn(2, 1);
    let drawing = e.doc().layers()[0].cel(0).heap_size();
    e.frame = 0;
    let before = e.history().memory_used();
    dot(&mut e, 3, 3);
    let charged = e.history().memory_used() - before;
    assert!(
        charged < drawing / 4,
        "charged {charged} for a drawing of {drawing}"
    );
}

#[test]
fn copying_removing_and_painting_over_and_over_stays_within_the_budget() {
    let mut e = drawn(1, 1);
    let drawing = e.doc().layers()[0].cel(0).heap_size();
    let budget = drawing * 4;
    e.history.set_budget(budget);
    let start = e.history().undo_depth();
    for n in 0..12 {
        e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
        e.apply(FrameCmd::Remove { from: 0, to: 0 })
            .expect("removed");
        e.frame = 0;
        dot(&mut e, n, 3);
    }
    let kept = e.history().steps_used();
    assert!(kept <= budget, "kept {kept} over a budget of {budget}");
    // Each removal keeps a drawing of its own, so the budget cannot keep every step.
    let depth = e.history().undo_depth();
    assert!(depth < start + 36, "{depth} steps kept of {}", start + 36);
}

#[test]
fn steps_that_keep_nothing_else_are_still_dropped_to_keep_within_the_budget() {
    let mut history = super::History::with_budget(1000);
    for _ in 0..10_000 {
        history.push(super::Edit::Selection(None));
    }
    let slot = size_of::<(super::Edit, usize)>();
    assert_eq!(history.undo_depth(), 1000 / slot);
    assert!(history.steps_used() <= 1000);
}

#[test]
fn the_counts_keep_reporting_the_room_they_hold_once_emptied() {
    let mut e = Editor::new(Document::new(64, 64, ColorMode::Rgba).expect("valid"));
    for frame in 0..100 {
        if frame > 0 {
            e.apply(FrameCmd::Add {
                at: frame,
                copy: false,
            })
            .expect("room");
        }
        e.frame = frame;
        dot(&mut e, frame as i32 % 64, frame as i32 / 64);
    }
    e.apply(FrameCmd::Remove { from: 0, to: 98 })
        .expect("removed");
    let full = e.history.held.bytes();
    // Undoing lets go of every drawing the removal kept. The table keeps its room.
    assert!(e.undo());
    assert_eq!(e.history.held.bytes(), full);
}

#[test]
fn a_selection_made_over_floating_pixels_is_charged_to_the_step_that_puts_them_down() {
    let mut e = drawn(1, 1);
    let rect = |x| crate::mask::Mask::rect(x, 10, 60, 60);
    e.select(rect(10), crate::selection::SelectOp::Replace);
    assert!(e.lift(false));
    e.apply(crate::FloatingCmd::FlipHorizontal)
        .expect("flipped");
    // A new selection puts the floating pixels down as one step, which brings the old one back.
    // A larger shape, so that the step keeps a different amount once undone.
    e.apply(crate::SelectionCmd::Select {
        shape: Box::new(crate::mask::Mask::rect(100, 10, 150, 150)),
        op: crate::selection::SelectOp::Replace,
    })
    .expect("selected");
    let (step, charged) = e.history.undo.last().expect("a step");
    let keeps_shape = matches!(
        step,
        super::Edit::Tiles {
            selection: Some(Some(_)),
            ..
        }
    );
    assert!(keeps_shape, "the step brings the old selection back");
    assert_eq!(*charged, step.size(), "charged for what it keeps");
    while e.undo() {}
    let (step, charged) = e.history.redo.first().expect("a step");
    assert_eq!(
        *charged,
        step.size(),
        "charged for what it keeps once undone"
    );
}

#[test]
fn a_stroke_on_blank_canvas_is_charged_for_its_tiles_once_undone() {
    let mut e = Editor::new(Document::new(256, 256, ColorMode::Rgba).expect("valid"));
    e.color = rgba(200, 30, 30, 255);
    e.begin(Tool::Line, 0, 0).expect("begin");
    e.drag(255, 255);
    e.commit();
    let tiles = |e: &Editor| {
        let cel = &e.doc().layers()[0].cels()[0];
        cel.heap_size() - cel.table_bytes()
    };
    let (drawn, written) = (e.history().steps_used(), tiles(&e));
    let Some((super::Edit::Tiles { changes, .. }, charged)) = e.history.undo.last() else {
        panic!("a pixel step");
    };
    let list = changes.len() * size_of::<(crate::image::TileKey, crate::grid::Cell)>();
    assert!(
        *charged >= super::SLOT + list,
        "charged for its list of tiles"
    );
    assert!(e.undo());
    assert_eq!(tiles(&e), 0, "the canvas is blank again");
    // Only the step keeps what was drawn now, for redo.
    assert_eq!(e.history().steps_used() - drawn, written);
    assert!(e.redo());
    assert_eq!(
        e.history().steps_used(),
        drawn,
        "back to keeping nothing drawn"
    );
}

#[test]
fn a_stroke_that_leaves_a_drawing_to_history_is_charged_for_its_replaced_tiles_once() {
    let mut e = drawn(2, 1);
    e.apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removed");
    e.frame = 0;
    dot(&mut e, 3, 3);
    let Some((super::Edit::Tiles { changes, split, .. }, charged)) = e.history.undo.last() else {
        panic!("a pixel step");
    };
    let old = split
        .as_ref()
        .and_then(std::sync::Weak::upgrade)
        .expect("split off");
    // The tiles the dot replaced are the old drawing's own, so the drawing covers them.
    assert!(super::tile_bytes(changes) > 0);
    let list = size_of_val::<[(crate::image::TileKey, crate::grid::Cell)]>(changes);
    assert_eq!(
        *charged,
        super::SLOT + list + crate::layer::kept_bytes(&old)
    );
}

/// Floating pixels hold the colour table while a palette change runs. Once the step is kept,
/// only history keeps the old table, so the step must be charged for it.
#[test]
fn a_palette_step_over_floating_pixels_is_charged_for_the_old_table() {
    let mut e = Editor::new(Document::new(64, 64, ColorMode::Palette).expect("valid"));
    for n in 1..100u8 {
        e.color = rgba(n, 3, 4, 255);
        dot(&mut e, i32::from(n) % 64, i32::from(n) / 64);
    }
    e.select(
        crate::mask::Mask::rect(0, 0, 32, 32),
        crate::selection::SelectOp::Replace,
    );
    assert!(e.lift(false));
    e.apply(crate::FloatingCmd::FlipHorizontal)
        .expect("flipped");
    e.apply(PaletteCmd::AddSwatch {
        color: rgba(9, 9, 9, 255),
    })
    .expect("room");
    let (step, charged) = e.history.undo.last().expect("a step");
    assert!(
        step.size() > e.doc().colors.kept_bytes(),
        "keeps the old table"
    );
    assert_eq!(*charged, step.size(), "charged for what it keeps");
}
