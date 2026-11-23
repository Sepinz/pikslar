//! Slices: named rectangles of the canvas with an optional pivot and centre, kept through undo
//! and moved with the canvas.

use crate::command::CanvasCmd;
use crate::slice::{Slice, SliceRect, MAX_SLICES, MAX_SLICE_NAME, SLICE_REACH};
use crate::{ColorMode, Document, Editor, LimitError, SliceCmd, StepCmd};

fn editor() -> Editor {
    Editor::new(Document::new(32, 24, ColorMode::Rgba).expect("valid"))
}

fn rect(x: i32, y: i32, width: u32, height: u32) -> SliceRect {
    SliceRect {
        x,
        y,
        width,
        height,
    }
}

fn slice(name: &str) -> Slice {
    Slice {
        name: name.to_string(),
        color: 0xff00_00ff,
        bounds: rect(2, 3, 10, 8),
        pivot: Some((5, 7)),
        center: Some(rect(2, 2, 6, 4)),
    }
}

#[test]
fn a_slice_is_added_changed_and_removed_each_in_one_step() {
    let mut e = editor();
    let added = e
        .apply(SliceCmd::Add {
            slice: Box::new(slice("Head")),
        })
        .expect("added");
    assert_eq!(added.created, Some(0));
    assert_eq!(e.doc().slices, [slice("Head")]);
    let mut moved = slice("Head");
    moved.bounds = rect(-4, 0, 3, 3);
    moved.center = None;
    e.apply(SliceCmd::Set {
        index: 0,
        slice: Box::new(moved.clone()),
    })
    .expect("set");
    assert_eq!(e.doc().slices, [moved]);
    e.apply(SliceCmd::Remove { index: 0 }).expect("removed");
    assert!(e.doc().slices.is_empty());
    for _ in 0..3 {
        e.apply(StepCmd::Undo).expect("undo");
    }
    assert!(e.doc().slices.is_empty());
    e.apply(StepCmd::Redo).expect("redo");
    assert_eq!(e.doc().slices, [slice("Head")]);
}

#[test]
fn the_same_slice_set_again_is_no_step() {
    let mut e = editor();
    e.apply(SliceCmd::Add {
        slice: Box::new(slice("A")),
    })
    .expect("added");
    let again = SliceCmd::Set {
        index: 0,
        slice: Box::new(slice("A")),
    };
    assert!(!e.apply(again).expect("same").changed);
}

#[test]
fn a_flat_slice_a_flat_centre_or_a_bad_replacement_is_refused() {
    let mut e = editor();
    let mut flat = slice("A");
    flat.bounds.height = 0;
    flat.center = None;
    let refused = e.apply(SliceCmd::Add {
        slice: Box::new(flat.clone()),
    });
    assert_eq!(refused, Err(LimitError::Dimensions));
    let mut flat_centre = slice("A");
    flat_centre.center = Some(rect(1, 1, 0, 2));
    let refused = e.apply(SliceCmd::Add {
        slice: Box::new(flat_centre),
    });
    assert_eq!(refused, Err(LimitError::Dimensions));
    e.apply(SliceCmd::Add {
        slice: Box::new(slice("A")),
    })
    .expect("added");
    let bad = SliceCmd::Set {
        index: 0,
        slice: Box::new(flat),
    };
    assert_eq!(e.apply(bad), Err(LimitError::Dimensions));
    assert_eq!(e.doc().slices, [slice("A")]);
}

#[test]
fn a_slice_with_no_area_or_a_centre_outside_it_is_refused() {
    let mut e = editor();
    let mut empty = slice("A");
    empty.bounds.width = 0;
    assert_eq!(
        e.apply(SliceCmd::Add {
            slice: Box::new(empty)
        }),
        Err(LimitError::Dimensions)
    );
    let mut outside = slice("A");
    outside.center = Some(rect(5, 0, 6, 4));
    let refused = e.apply(SliceCmd::Add {
        slice: Box::new(outside),
    });
    assert_eq!(refused, Err(LimitError::Dimensions));
    let mut far = slice("A");
    far.bounds.x = SLICE_REACH;
    assert_eq!(
        e.apply(SliceCmd::Add {
            slice: Box::new(far)
        }),
        Err(LimitError::CanvasReach)
    );
    let mut far_pivot = slice("A");
    far_pivot.pivot = Some((0, -SLICE_REACH - 1));
    let refused = e.apply(SliceCmd::Add {
        slice: Box::new(far_pivot),
    });
    assert_eq!(refused, Err(LimitError::CanvasReach));
    assert!(e.doc().slices.is_empty());
    let missing = SliceCmd::Remove { index: 0 };
    assert_eq!(e.apply(missing), Err(LimitError::NoSuchItem));
}

#[test]
fn a_long_name_is_cut_and_slices_stop_at_the_limit() {
    let mut e = editor();
    let long = "é".repeat(MAX_SLICE_NAME + 5);
    e.apply(SliceCmd::Add {
        slice: Box::new(slice(&long)),
    })
    .expect("added");
    assert_eq!(e.doc().slices[0].name.chars().count(), MAX_SLICE_NAME);
    for _ in 1..MAX_SLICES {
        e.apply(SliceCmd::Add {
            slice: Box::new(slice("x")),
        })
        .expect("room");
    }
    let full = e.apply(SliceCmd::Add {
        slice: Box::new(slice("x")),
    });
    assert_eq!(full, Err(LimitError::TooManySlices));
}

#[test]
fn a_canvas_resize_moves_every_slice_with_the_pixels_and_undo_moves_it_back() {
    let mut e = editor();
    e.apply(SliceCmd::Add {
        slice: Box::new(slice("A")),
    })
    .expect("added");
    let resize = CanvasCmd::Resize {
        width: 40,
        height: 20,
        dx: 6,
        dy: -5,
    };
    e.apply(resize).expect("resized");
    let s = &e.doc().slices[0];
    assert_eq!(s.bounds, rect(8, -2, 10, 8));
    assert_eq!((s.pivot, s.center), (Some((5, 7)), Some(rect(2, 2, 6, 4))));
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.doc().slices, [slice("A")]);
}

#[test]
fn a_resize_that_would_push_a_slice_out_of_reach_is_refused() {
    let mut e = editor();
    let mut edge = slice("A");
    edge.bounds.x = SLICE_REACH - 20;
    e.apply(SliceCmd::Add {
        slice: Box::new(edge.clone()),
    })
    .expect("added");
    let resize = CanvasCmd::Resize {
        width: 40,
        height: 20,
        dx: 30,
        dy: 0,
    };
    assert_eq!(e.apply(resize), Err(LimitError::CanvasReach));
    assert_eq!(e.doc().slices, [edge]);
    assert_eq!(e.doc().width, 32);
}
