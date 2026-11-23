//! A dragged value that comes to nothing leaves redo as it was.

use crate::color::rgba;
use crate::document::{ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::{CelCmd, Command, FrameCmd, LayerCmd, PaletteCmd, StepCmd, TagCmd};

/// An editor with a swatch and a tag, and a dot undone so that it can be redone.
fn with_redo() -> (Editor, u32) {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("valid"));
    e.transact(|d| d.add_frame(1, false)).expect("room");
    let swatch = e
        .apply(PaletteCmd::AddSwatch {
            color: rgba(1, 2, 3, 255),
        })
        .expect("room")
        .created
        .expect("swatch");
    e.apply(TagCmd::Add {
        name: "walk".into(),
        from: 0,
        to: 1,
    })
    .expect("room");
    e.apply(StepCmd::EndDrag).expect("end");
    e.color = rgba(200, 10, 10, 255);
    e.begin(Tool::Pencil, 3, 3).expect("begin");
    e.commit();
    assert!(e.undo());
    assert_eq!(e.history().redo_depth(), 1);
    (e, swatch)
}

/// Each dragged value, as a change to it and the change that takes it back.
fn drags(e: &Editor, swatch: u32) -> Vec<(&'static str, Command, Command)> {
    let layer = e.layer_id(0).expect("layer");
    let opacity = |opacity| LayerCmd::SetOpacity { layer, opacity }.into();
    let timing = |ms| {
        FrameCmd::Retime {
            from: 0,
            durations: vec![ms],
        }
        .into()
    };
    let recolor = |color| PaletteCmd::Recolor { id: swatch, color }.into();
    let name = |name: &str| {
        TagCmd::SetName {
            tag: 0,
            name: name.into(),
        }
        .into()
    };
    let shift = |dx| {
        CelCmd::Move {
            layer,
            frame: 0,
            dx,
            dy: 0,
        }
        .into()
    };
    let frame = e.doc.frames[0].duration_ms;
    vec![
        ("opacity", opacity(40), opacity(255)),
        ("timing", timing(frame + 50), timing(frame)),
        (
            "recolor",
            recolor(rgba(9, 9, 9, 255)),
            recolor(rgba(1, 2, 3, 255)),
        ),
        ("tag name", name("run"), name("walk")),
        ("cel drag", shift(2), shift(-2)),
    ]
}

/// Runs a check on each dragged value, from the same start.
fn each_drag(check: impl Fn(&str, Editor, Command, Command)) {
    let count = {
        let (e, swatch) = with_redo();
        drags(&e, swatch).len()
    };
    for at in 0..count {
        let (e, swatch) = with_redo();
        let (what, there, back) = drags(&e, swatch).swap_remove(at);
        check(what, e, there, back);
    }
}

#[test]
fn a_drag_that_ends_where_it_began_keeps_redo() {
    each_drag(|what, mut e, there, back| {
        let memory = e.history().memory_used();
        e.apply(there).expect("change");
        e.apply(back).expect("change back");
        e.apply(StepCmd::EndDrag).expect("end");
        assert_eq!(e.history().redo_depth(), 1, "{what}");
        assert_eq!(e.history().memory_used(), memory, "{what}");
        assert!(e.redo(), "{what}");
        assert!(
            e.doc.layers[0].cel(0).heap_size() > 0,
            "{what}: the dot is back"
        );
    });
}

#[test]
fn a_cancelled_drag_keeps_redo() {
    each_drag(|what, mut e, there, _| {
        let memory = e.history().memory_used();
        e.apply(there).expect("change");
        e.apply(StepCmd::AbortJoin).expect("cancel");
        assert_eq!(e.history().redo_depth(), 1, "{what}");
        assert_eq!(e.history().memory_used(), memory, "{what}");
        assert!(e.redo(), "{what}");
        assert!(
            e.doc.layers[0].cel(0).heap_size() > 0,
            "{what}: the dot is back"
        );
    });
}

#[test]
fn a_drag_that_changes_something_drops_redo() {
    each_drag(|what, mut e, there, _| {
        e.apply(there).expect("change");
        e.apply(StepCmd::EndDrag).expect("end");
        assert_eq!(e.history().redo_depth(), 0, "{what}");
        assert_eq!(e.history.aside_depth(), 0, "{what}: nothing kept for it");
        assert!(!e.redo(), "{what}");
    });
}

#[test]
fn undo_during_a_drag_drops_the_redo_set_aside() {
    each_drag(|what, mut e, there, _| {
        e.apply(there).expect("change");
        assert!(e.undo(), "{what}");
        assert_eq!(e.history().redo_depth(), 1, "{what}: only the drag to redo");
        assert_eq!(e.history.aside_depth(), 0, "{what}: nothing kept for it");
        assert!(e.redo(), "{what}: the drag comes back");
        assert!(!e.redo(), "{what}: the dot's step is gone");
    });
}

#[test]
fn another_step_during_a_drag_drops_the_redo_set_aside() {
    each_drag(|what, mut e, there, back| {
        e.apply(there).expect("change");
        e.apply(FrameCmd::Add { at: 2, copy: false }).expect("room");
        e.apply(back).expect("change back");
        e.apply(StepCmd::EndDrag).expect("end");
        e.apply(StepCmd::AbortJoin).expect("cancel");
        assert_eq!(e.history().redo_depth(), 0, "{what}");
    });
}

#[test]
fn redo_set_aside_is_dropped_first_when_history_is_over_its_budget() {
    let count = drags(&with_redo().0, 0).len();
    for at in 0..count {
        // What the drag adds while redo is set aside, measured on a copy of the session.
        let (mut e, swatch) = with_redo();
        let before = e.history().steps_used();
        let (_, there, _) = drags(&e, swatch).swap_remove(at);
        e.apply(there).expect("change");
        let with_aside = e.history().steps_used();
        // One byte short: dropping the set-aside list makes room, so no step is dropped.
        let (mut e, swatch) = with_redo();
        let (what, there, _) = drags(&e, swatch).swap_remove(at);
        let steps = e.history().undo_depth();
        e.history.set_budget(with_aside - 1);
        e.apply(there).expect("change");
        assert!(
            e.history().steps_used() < with_aside,
            "{what}: set aside dropped"
        );
        assert_eq!(
            e.history().undo_depth(),
            steps + 1,
            "{what}: no step dropped"
        );
        e.apply(StepCmd::AbortJoin).expect("cancel");
        assert_eq!(e.history().redo_depth(), 0, "{what}");
        assert!(e.history().steps_used() < before, "{what}");
    }
}

#[test]
fn a_stroke_during_a_drag_drops_the_redo_set_aside() {
    each_drag(|what, mut e, there, _| {
        e.apply(there).expect("change");
        assert_eq!(e.history.aside_depth(), 1, "{what}: set aside");
        e.begin(Tool::Pencil, 9, 9).expect("begin");
        e.commit();
        assert_eq!(e.history.aside_depth(), 0, "{what}: dropped");
        e.apply(StepCmd::AbortJoin).expect("cancel");
        assert_eq!(e.history().redo_depth(), 0, "{what}");
    });
}

#[test]
fn a_cel_drag_over_floating_pixels_that_were_not_moved_keeps_redo() {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("valid"));
    e.color = rgba(200, 10, 10, 255);
    e.begin(Tool::Rect, 0, 0).expect("begin");
    e.drag(6, 6);
    e.commit();
    // The selection is made before the step that is undone, so lifting is all that follows.
    e.select(
        crate::mask::Mask::rect(0, 0, 4, 4),
        crate::selection::SelectOp::Replace,
    );
    e.color = rgba(10, 200, 10, 255);
    e.begin(Tool::Pencil, 2, 2).expect("begin");
    e.commit();
    assert!(e.undo());
    assert!(e.lift(false));
    assert_eq!(e.history().redo_depth(), 1, "lifting keeps redo");
    let layer = e.layer_id(0).expect("layer");
    let shift = |dx| CelCmd::Move {
        layer,
        frame: 0,
        dx,
        dy: 0,
    };
    e.apply(shift(1)).expect("move");
    e.apply(shift(-1)).expect("move back");
    e.apply(StepCmd::EndDrag).expect("end");
    assert_eq!(e.history().redo_depth(), 1);
}

#[test]
fn a_tag_added_and_then_cancelled_keeps_redo() {
    let (mut e, _) = with_redo();
    let memory = e.history().memory_used();
    e.apply(TagCmd::Add {
        name: "new".into(),
        from: 1,
        to: 1,
    })
    .expect("room");
    e.apply(StepCmd::AbortJoin).expect("cancel");
    assert_eq!(e.doc.tags.len(), 1, "the new tag is gone");
    assert_eq!(e.history().redo_depth(), 1);
    assert_eq!(e.history().memory_used(), memory);
}
