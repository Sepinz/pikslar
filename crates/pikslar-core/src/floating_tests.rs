//! Tests for floating selections.

use super::*;
use crate::color::rgba;
use crate::document::Document;
use crate::editor::{Damage, Tool};
use crate::selection::SelectOp;
use crate::{PaletteCmd, StepCmd};

const RED: u32 = rgba(255, 0, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const GLASS: u32 = rgba(10, 20, 30, 77);

fn editor(mode: ColorMode) -> Editor {
    let mut e = Editor::new(Document::new(32, 32, mode).expect("valid"));
    e.color = RED;
    e.pixel_perfect = false;
    e
}

#[test]
fn a_move_does_no_pixel_work_until_something_looks() {
    let mut e = ell(ColorMode::Rgba);
    e.take_damage();
    e.lift(false);
    e.move_floating(3, 0);
    assert!(e.floating_is_stale());
    assert!(!e.has_damage());
    assert!(e.overlay.touched().is_empty());
    assert_ne!(e.take_damage(), Damage::None);
    assert!(!e.floating_is_stale());
    assert!(!e.overlay.touched().is_empty());
}

#[test]
fn only_moving_keeps_the_shape_version() {
    let mut e = ell(ColorMode::Rgba);
    e.lift(false);
    let (shape, position) = (e.selection_shape_version(), e.selection_version());
    e.move_floating(1, 1);
    assert_eq!(e.selection_shape_version(), shape);
    assert_ne!(e.selection_version(), position);
    e.flip_horizontal();
    assert_ne!(e.selection_shape_version(), shape);
}

#[test]
fn lifting_shares_the_pixels_until_a_transform_replaces_them() {
    let mut e = ell(ColorMode::Rgba);
    e.lift(false);
    let f = e.floating.as_ref().expect("lifted");
    assert!(Arc::ptr_eq(&f.pixels, &f.placed));
}

#[test]
fn a_paste_too_colorful_for_the_palette_leaves_the_table_alone() {
    let mut e = editor(ColorMode::Palette);
    let before = e.doc.colors.clone();
    let colors = (1..=90_000u32)
        .map(|i| rgba((i >> 16) as u8, (i >> 8) as u8, i as u8, 255))
        .collect();
    assert!(e.paste(&Clip::from_image(300, 300, colors), 0, 0).is_err());
    assert!(Arc::ptr_eq(&before, &e.doc.colors));
}

#[test]
fn cancelling_a_paste_forgets_the_colors_it_brought() {
    let mut e = editor(ColorMode::Palette);
    let before = e.doc.colors.clone();
    let clip = Clip::from_image(2, 1, vec![RED, BLUE]);
    e.paste(&clip, 0, 0).expect("fits");
    e.revert_floating();
    assert!(Arc::ptr_eq(&before, &e.doc.colors));
}

#[test]
fn moving_a_selection_of_nothing_is_its_own_undo_step() {
    let mut e = editor(ColorMode::Rgba);
    dot(&mut e, 20, 20, RED);
    e.select(Mask::rect(2, 2, 3, 3), SelectOp::Replace);
    e.lift(false);
    e.move_floating(4, 0);
    e.anchor();
    assert_eq!(e.selection().map(|m| m.bounds().0), Some(6));
    e.undo();
    assert_eq!(e.doc.layers[0].cel(0).get(20, 20), RED);
    assert_eq!(e.selection().map(|m| m.bounds().0), Some(2));
}

#[test]
fn a_handle_cannot_stretch_a_selection_far_past_the_canvas() {
    let mut e = editor(ColorMode::Rgba);
    e.select(Mask::rect(2, 2, 3, 3), SelectOp::Replace);
    e.lift(false);
    e.scale_floating(0, 0, 8000, 8000);
    assert_eq!(e.selection().map(|m| m.bounds()), Some((0, 0, 64, 64)));
}

fn dot(e: &mut Editor, x: i32, y: i32, color: u32) {
    e.color = color;
    e.begin(Tool::Pencil, x, y).expect("begin");
    e.commit();
}

/// An L of three cells: red at the corner, blue to its right, translucent below.
fn ell(mode: ColorMode) -> Editor {
    let mut e = editor(mode);
    dot(&mut e, 4, 4, RED);
    dot(&mut e, 5, 4, BLUE);
    dot(&mut e, 4, 5, GLASS);
    e.select(Mask::rect(4, 4, 2, 2), SelectOp::Replace);
    e
}

fn modes() -> [ColorMode; 2] {
    [ColorMode::Palette, ColorMode::Rgba]
}

fn pixels(e: &Editor) -> Vec<u32> {
    e.doc.layers[0].cel(0).to_cells(e.doc.width, e.doc.height)
}

#[test]
fn moving_is_one_undo_step_and_restores_the_marquee() {
    for mode in modes() {
        let mut e = ell(mode);
        let original = pixels(&e);
        let depth = e.history.undo_depth();
        assert!(e.lift(false));
        e.move_floating(3, 0);
        e.move_floating(0, 2);
        assert_eq!(e.color_at(4, 4), RED, "nothing lands until anchored");
        e.anchor();
        assert_eq!(e.color_at(7, 6), RED, "{mode:?}");
        assert_eq!(e.color_at(8, 6), BLUE, "{mode:?}");
        assert_eq!(e.color_at(7, 7), GLASS, "{mode:?}");
        assert_eq!(e.color_at(4, 4), 0, "{mode:?}");
        assert_eq!(e.history.undo_depth(), depth + 1);
        assert_eq!(e.selection().expect("kept").bounds().0, 7);
        assert!(e.undo());
        assert_eq!(pixels(&e), original, "{mode:?}");
        assert_eq!(e.selection().expect("kept").bounds().0, 4);
        assert!(e.redo());
        assert_eq!(e.selection().expect("kept").bounds().0, 7);
    }
}

#[test]
fn a_second_drag_does_not_destroy_what_it_passes_over() {
    let mut e = ell(ColorMode::Rgba);
    e.deselect();
    dot(&mut e, 10, 4, GLASS);
    e.select(Mask::rect(4, 4, 2, 2), SelectOp::Replace);
    e.lift(false);
    e.move_floating(6, 0);
    e.move_floating(6, 0);
    e.anchor();
    assert_eq!(e.color_at(10, 4), GLASS);
}

#[test]
fn a_lift_with_no_change_leaves_no_trace() {
    let mut e = ell(ColorMode::Rgba);
    let (original, revision) = (pixels(&e), e.revision);
    let depth = e.history.undo_depth();
    e.lift(false);
    e.anchor();
    assert_eq!(pixels(&e), original);
    assert_eq!((e.revision, e.history.undo_depth()), (revision, depth));
}

#[test]
fn undo_while_floating_puts_everything_back() {
    for mode in modes() {
        let mut e = ell(mode);
        let (original, depth) = (pixels(&e), e.history.undo_depth());
        e.lift(false);
        e.move_floating(9, 9);
        assert!(e.undo());
        assert!(!e.is_floating());
        assert_eq!(pixels(&e), original);
        assert_eq!(e.history.undo_depth(), depth);
        assert_eq!(e.selection().expect("kept").bounds().0, 4);
    }
}

#[test]
fn a_copy_lift_leaves_the_original() {
    let mut e = ell(ColorMode::Palette);
    e.lift(true);
    e.move_floating(10, 0);
    e.anchor();
    assert_eq!((e.color_at(4, 4), e.color_at(14, 4)), (RED, RED));
}

#[test]
fn dragging_off_canvas_keeps_pixels_until_anchored() {
    let mut e = ell(ColorMode::Rgba);
    e.lift(false);
    e.move_floating(-50, 0);
    e.move_floating(50, 0);
    let depth = e.history.undo_depth();
    e.anchor();
    assert_eq!(e.color_at(5, 4), BLUE);
    assert_eq!(e.history.undo_depth(), depth, "it ended where it began");
    e.lift(false);
    e.move_floating(-5, 0);
    e.anchor();
    assert_eq!(e.color_at(0, 4), BLUE, "the visible part lands");
    assert_eq!(
        e.selection().expect("kept").bounds().2,
        1,
        "selection clips"
    );
}

#[test]
fn flips_mirror_in_place_in_both_modes() {
    for mode in modes() {
        let mut e = ell(mode);
        e.flip_horizontal();
        e.anchor();
        assert_eq!(
            (e.color_at(4, 4), e.color_at(5, 4)),
            (BLUE, RED),
            "{mode:?}"
        );
        assert_eq!(e.color_at(5, 5), GLASS, "{mode:?}");
        let mut v = ell(mode);
        v.flip_vertical();
        v.anchor();
        assert_eq!(
            (v.color_at(4, 5), v.color_at(4, 4)),
            (RED, GLASS),
            "{mode:?}"
        );
    }
}

#[test]
fn rotating_clockwise_turns_the_shape() {
    let mut e = ell(ColorMode::Rgba);
    e.rotate(true);
    e.anchor();
    assert_eq!(e.color_at(5, 4), RED);
    assert_eq!(e.color_at(5, 5), BLUE);
    assert_eq!(e.color_at(4, 4), GLASS);
}

#[test]
fn four_turns_and_two_flips_are_exact() {
    let mut e = ell(ColorMode::Palette);
    let original = pixels(&e);
    e.lift(false);
    for _ in 0..4 {
        e.rotate(true);
    }
    e.flip_horizontal();
    e.flip_horizontal();
    e.flip_vertical();
    e.flip_vertical();
    e.anchor();
    assert_eq!(pixels(&e), original);
}

#[test]
fn flip_and_turn_compose_like_the_picture_on_screen() {
    let mut e = ell(ColorMode::Rgba);
    e.rotate(true);
    e.flip_horizontal();
    e.anchor();
    assert_eq!(e.color_at(4, 4), RED);
    assert_eq!(e.color_at(4, 5), BLUE);
    assert_eq!(e.color_at(5, 4), GLASS);
}

#[test]
fn scaling_is_nearest_neighbor_and_lossless_when_undone() {
    for mode in modes() {
        let mut e = ell(mode);
        let original = pixels(&e);
        e.lift(false);
        e.scale_floating(4, 4, 8, 8);
        assert_eq!(e.selection().expect("set").bounds(), (4, 4, 8, 8));
        e.scale_floating(4, 4, 1, 1);
        e.scale_floating(4, 4, 2, 2);
        e.anchor();
        assert_eq!(pixels(&e), original, "{mode:?}");
        let mut big = ell(mode);
        big.lift(false);
        big.scale_floating(4, 4, 4, 4);
        big.anchor();
        assert_eq!(big.color_at(5, 5), RED, "{mode:?}");
        assert_eq!(big.color_at(6, 4), BLUE, "{mode:?}");
        assert_eq!(big.color_at(4, 6), GLASS, "{mode:?}");
    }
}

#[test]
fn delete_clears_only_selected_cells() {
    for mode in modes() {
        let mut e = ell(mode);
        dot(&mut e, 20, 20, RED);
        e.select(Mask::rect(4, 4, 1, 1), SelectOp::Replace);
        e.delete_selected();
        assert_eq!(e.color_at(4, 4), 0, "{mode:?}");
        assert_eq!(e.color_at(5, 4), BLUE, "{mode:?}");
        assert!(e.undo());
        assert_eq!(e.color_at(4, 4), RED, "{mode:?}");
    }
}

#[test]
fn delete_while_floating_removes_the_lifted_pixels() {
    let mut e = ell(ColorMode::Rgba);
    e.lift(false);
    e.move_floating(5, 5);
    e.delete_selected();
    assert!(!e.is_floating());
    assert_eq!((e.color_at(4, 4), e.color_at(9, 9)), (0, 0));
}

#[test]
fn copy_and_paste_work_within_and_across_modes() {
    for from in modes() {
        for to in modes() {
            let source = ell(from);
            let clip = source.copy_selected().expect("selected");
            let mut target = editor(to);
            target.paste(&clip, 20, 20).expect("paste");
            assert!(target.is_pasting());
            target.anchor();
            assert_eq!(target.color_at(20, 20), RED, "{from:?} to {to:?}");
            assert_eq!(target.color_at(21, 20), BLUE, "{from:?} to {to:?}");
            assert_eq!(target.color_at(20, 21), GLASS, "{from:?} to {to:?}");
            assert_eq!(target.color_at(21, 21), 0, "{from:?} to {to:?}");
        }
    }
}

#[test]
fn pasting_into_the_same_palette_keeps_swatch_links() {
    let mut e = ell(ColorMode::Palette);
    let id = e.doc.layers[0].cel(0).get(4, 4);
    let clip = e.copy_selected().expect("selected");
    e.deselect();
    e.paste(&clip, 10, 10).expect("paste");
    e.anchor();
    assert_eq!(e.doc.layers[0].cel(0).get(10, 10), id);
    e.apply(StepCmd::EndDrag).expect("end");
    e.apply(PaletteCmd::Recolor { id, color: BLUE })
        .expect("recolor");
    assert_eq!(e.color_at(10, 10), BLUE);
}

#[test]
fn cancelling_a_paste_restores_the_old_selection() {
    let mut e = ell(ColorMode::Rgba);
    let clip = e.copy_selected().expect("selected");
    e.select(Mask::rect(1, 1, 1, 1), SelectOp::Replace);
    let (original, depth) = (pixels(&e), e.history.undo_depth());
    e.paste(&clip, 20, 20).expect("paste");
    e.revert_floating();
    assert_eq!(pixels(&e), original);
    assert_eq!(e.history.undo_depth(), depth);
    assert_eq!(e.selection().expect("kept").bounds(), (1, 1, 1, 1));
}

#[test]
fn copying_floating_pixels_copies_what_is_shown() {
    let mut e = ell(ColorMode::Rgba);
    e.flip_horizontal();
    let clip = e.copy_selected().expect("selected");
    assert_eq!(clip.colors[0], BLUE);
    assert_eq!(clip.colors[1], RED);
}

#[test]
fn oversized_pastes_are_refused() {
    let mut e = editor(ColorMode::Rgba);
    let clip = Clip::from_image(MAX_DIMENSION + 1, 1, vec![RED; MAX_DIMENSION + 1]);
    assert_eq!(e.paste(&clip, 0, 0), Err(LimitError::Dimensions));
}

#[test]
fn structural_edits_anchor_floating_pixels_first() {
    let mut e = ell(ColorMode::Rgba);
    e.lift(false);
    e.move_floating(2, 0);
    e.transact(|d| d.add_frame(1, false)).expect("room");
    assert!(!e.is_floating());
    assert_eq!(e.color_at(6, 4), RED);
}

#[test]
fn starting_a_stroke_anchors_floating_pixels_first() {
    let mut e = ell(ColorMode::Rgba);
    e.lift(false);
    e.move_floating(2, 0);
    e.begin(Tool::Pencil, 20, 20).expect("begin");
    assert!(!e.is_floating());
    assert_eq!(e.color_at(6, 4), RED);
}

#[test]
fn a_clip_whose_colors_do_not_fill_it_is_refused() {
    let mut e = editor(ColorMode::Rgba);
    let clip = Clip {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
        colors: vec![RED; 3],
        ids: None,
    };
    assert_eq!(e.paste(&clip, 0, 0), Err(LimitError::Dimensions));
    assert!(!e.is_floating());
}

#[test]
fn a_transparent_swatch_in_a_clip_places_nothing() {
    let mut e = ell(ColorMode::Palette);
    let id = e.doc.layers[0].cel(0).get(4, 4);
    Arc::make_mut(&mut e.doc.colors).set_color(id, 0);
    let clip = Clip {
        x: 0,
        y: 0,
        width: 2,
        height: 1,
        colors: vec![0, BLUE],
        ids: Some(vec![id, 0]),
    };
    e.paste(&clip, 10, 10).expect("fits");
    let view = e.floating_view().expect("pasted");
    assert_eq!(view.placed[0], 0, "cells outside the shape stay empty");
}

#[test]
fn each_lift_and_paste_is_a_new_floating_selection() {
    let mut e = ell(ColorMode::Rgba);
    e.lift(false);
    let first = e.floating_view().map(|f| f.epoch);
    e.move_floating(1, 0);
    assert_eq!(e.floating_view().map(|f| f.epoch), first, "a move keeps it");
    let clip = e.copy_selected().expect("selected");
    e.paste(&clip, 0, 0).expect("fits");
    assert_ne!(e.floating_view().map(|f| f.epoch), first);
}

#[test]
fn stretching_samples_the_cell_each_position_falls_in() {
    let across: Vec<i32> = (0..8).map(|p| stretched(p, 2, 4, 2)).collect();
    assert_eq!(across, [-1, -1, 0, 0, 1, 1, -1, -1]);
    // Shrinking takes every second cell, starting from the first.
    let shrunk: Vec<i32> = (0..3).map(|p| stretched(p, 0, 3, 6)).collect();
    assert_eq!(shrunk, [0, 2, 4]);
    // Sizes this large must not overflow on the way.
    assert_eq!(stretched(40_000, 0, 50_000, 50_000), 40_000);
}
