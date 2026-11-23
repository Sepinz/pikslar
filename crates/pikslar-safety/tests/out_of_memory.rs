//! With an allocator that really refuses large requests, commands that need a lot of memory say
//! so and leave the document alone. Everything is in one test because the limit is global.

use pikslar_core::{
    rgba, Clip, ColorMode, Command, Document, DrawCmd, Editor, FloatingCmd, LayerCmd, LimitError,
    SelectionCmd, StepCmd, Tool,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering};

const NO_LIMIT: usize = usize::MAX;
const LIMIT: usize = 32 << 20;
const SIDE: usize = 4096;

static LARGEST: AtomicUsize = AtomicUsize::new(NO_LIMIT);

struct Refusing;

// SAFETY: every request is either passed unchanged to the system allocator, or answered with
// null, which is how an allocator is allowed to report failure.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() > LARGEST.load(Ordering::Relaxed) {
            return null_mut();
        }
        // SAFETY: the caller's layout is passed on as it was given.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the pointer came from `alloc` above, which got it from the system allocator.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// Lifts the limit however the test ends, so that a failure can still be reported.
struct Limited;

impl Limited {
    fn begin() -> Self {
        Limited::at(LIMIT)
    }

    fn at(limit: usize) -> Self {
        LARGEST.store(limit, Ordering::Relaxed);
        Limited
    }
}

impl Drop for Limited {
    fn drop(&mut self) {
        LARGEST.store(NO_LIMIT, Ordering::Relaxed);
    }
}

fn expect_refused(editor: &mut Editor, command: Command) {
    let before = editor.doc().clone();
    let floating = editor.is_floating();
    let (pushes, revision) = (editor.history().pushes(), editor.revision());
    assert_eq!(editor.apply(command), Err(LimitError::NoMemory));
    assert!(editor.doc() == &before, "the document changed");
    assert_eq!(editor.history().pushes(), pushes);
    assert_eq!(editor.revision(), revision);
    assert_eq!(editor.is_floating(), floating);
}

fn floating_pixels_too_large_to_apply_stay_floating(editor: &mut Editor) {
    let moved = FloatingCmd::Move {
        dx: 1,
        dy: 0,
        join: false,
    };
    editor.apply(moved).expect("moved");
    let limited = Limited::begin();
    expect_refused(editor, FloatingCmd::Anchor.into());
    drop(limited);
    assert!(editor.is_floating());
    editor.apply(FloatingCmd::Anchor).expect("applied");
}

fn strokes_and_pick_ups_too_large_are_refused(editor: &mut Editor) {
    editor.apply(SelectionCmd::All).expect("selected");
    let stroke: Command = DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 1,
        y: 1,
    }
    .into();
    let limited = Limited::begin();
    expect_refused(editor, stroke);
    expect_refused(editor, FloatingCmd::Lift { copy: false }.into());
    drop(limited);
}

/// Undoing may have to rebuild the table of a drawing as big as the canvas.
fn an_undo_without_room_for_a_table_is_refused() {
    let doc = Document::new(8192, 8192, ColorMode::Rgba).expect("valid");
    let mut editor = Editor::new(doc);
    editor.color = rgba(1, 2, 3, 255);
    let dot = DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(8191, 8191)],
    };
    editor.apply(dot).expect("drawn");
    let limited = Limited::at(1 << 20);
    let before = editor.doc().clone();
    let (pushes, revision) = (editor.history().pushes(), editor.revision());
    assert_eq!(editor.apply(StepCmd::Undo), Err(LimitError::NoMemory));
    drop(limited);
    assert!(editor.doc() == &before, "the document changed");
    assert_eq!(
        (editor.history().pushes(), editor.revision()),
        (pushes, revision)
    );
    assert!(
        editor.apply(StepCmd::Undo).is_ok(),
        "there was room after all"
    );
}

#[test]
fn large_commands_are_refused_and_change_nothing() {
    an_undo_without_room_for_a_table_is_refused();
    let doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid");
    let mut editor = Editor::new(doc);
    let clip = Clip::from_image(SIDE, SIDE, vec![rgba(1, 2, 3, 255); SIDE * SIDE]);
    let paste: Command = FloatingCmd::Paste {
        clip: std::sync::Arc::new(clip),
        x: 0,
        y: 0,
    }
    .into();
    let layer = editor.layer_id(0).expect("layer");
    let convert: Command = LayerCmd::SetMode {
        layer,
        mode: ColorMode::Palette,
    }
    .into();

    // Copied now, because making a copy of a command needs memory too.
    let (refused_paste, refused_convert) = (paste.clone(), convert.clone());
    let limited = Limited::begin();
    expect_refused(&mut editor, refused_paste);
    expect_refused(&mut editor, refused_convert);
    let small = editor.apply(LayerCmd::SetOpacity { layer, opacity: 90 });
    let selected = editor.apply(SelectionCmd::Select {
        shape: Box::new(pikslar_core::Mask::rect(0, 0, 8, 8)),
        op: pikslar_core::SelectOp::Replace,
    });
    drop(limited);

    assert!(small.is_ok(), "a small command must still work: {small:?}");
    assert!(selected.is_ok(), "and so must a selection: {selected:?}");
    assert!(editor.apply(convert).is_ok(), "there was room after all");
    assert!(editor.apply(paste).is_ok(), "there was room after all");
    floating_pixels_too_large_to_apply_stay_floating(&mut editor);
    strokes_and_pick_ups_too_large_are_refused(&mut editor);
}
