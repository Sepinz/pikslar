//! The review's findings, reproduced through the same entry points the window uses.

use crate::app::ToolChoice;
use crate::test_app::*;
use pikslar_core::FrameCmd;
use pikslar_core::{FloatingCmd, SelectionCmd};
use pikslar_core::{Mask, SelectOp};
use pikslar_platform::{Button, Key};
use std::time::{Duration, Instant};

// ---- 3 and 4: a selection must not take over the arrows

#[test]
fn arrows_step_frames_while_a_selection_is_only_resting() {
    let mut a = app(3);
    select(&mut a);
    key(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.frame, 1);
    assert!(!a.editor.is_floating());
}

#[test]
fn arrows_still_move_lifted_pixels() {
    let mut a = app(3);
    select(&mut a);
    a.editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    key(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.frame, 0);
    assert_eq!(a.editor.selection().map(|m| m.bounds().0), Some(3));
}

#[test]
fn arrows_still_move_a_selection_under_the_move_tool() {
    let mut a = app(3);
    select(&mut a);
    a.set_tool(ToolChoice::Move);
    key(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.frame, 0);
    assert_eq!(a.editor.selection().map(|m| m.bounds().0), Some(3));
}

#[test]
fn a_click_with_the_move_tool_leaves_nothing_selected() {
    let mut a = app(1);
    dot(&mut a, 5, 5);
    a.set_tool(ToolChoice::Move);
    let at = screen(&a, 20, 20);
    click(&mut a, at, PLAIN);
    assert!(!a.has_selection());
}

// A drag with the Move tool and nothing selected now moves the cel: see `cel_ui_tests`.

// ---- 6, 8, 11 and 12: timing and playback

#[test]
fn timing_changes_during_playback_go_to_the_frame_chosen_before_it() {
    let mut a = app(3);
    a.set_frame(1);
    a.toggle_play();
    let depth = a.editor.history().undo_depth();
    a.set_frame(2);
    wheel_timing(&mut a, 2);
    a.set_frame(0);
    wheel_timing(&mut a, 2);
    assert_eq!(durations(&a), vec![100, 140, 100]);
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
}

#[test]
fn a_timing_change_keeps_the_differences_between_selected_frames() {
    let mut a = app(3);
    a.editor
        .apply(FrameCmd::Retime {
            from: 1,
            durations: vec![200; 1],
        })
        .expect("timing");
    draw(&mut a);
    range(&mut a, 0, 1);
    wheel_timing(&mut a, 1);
    assert_eq!(durations(&a), vec![110, 210, 100]);
}

#[test]
fn changing_loop_direction_does_not_stop_playback() {
    let mut a = app(3);
    a.editor
        .apply(pikslar_core::TagCmd::Add {
            name: "x".into(),
            from: 0,
            to: 2,
        })
        .expect("room");
    a.toggle_loop(0);
    a.toggle_play();
    key(&mut a, Key::Char(b'T'), SHIFT);
    assert!(a.playing);
}

#[test]
fn play_shows_the_current_frame_for_its_full_duration() {
    let mut a = app(3);
    a.toggle_play();
    a.advance_playback();
    assert_eq!(a.editor.frame, 0);
    assert!(a.next_frame_at > Instant::now() + Duration::from_millis(50));
}

// ---- 13 reaches the app through the gestures

#[test]
fn undo_during_a_move_takes_back_only_the_last_drag() {
    let mut a = app(1);
    dot(&mut a, 3, 3);
    select(&mut a);
    a.set_tool(ToolChoice::Move);
    for step in 0..2 {
        let (from, to) = (screen(&a, 3 + step, 3), screen(&a, 4 + step, 3));
        a.pointer_down(from.0, from.1, Button::Left, PLAIN);
        a.canvas_move(to.0, to.1);
        a.pointer_up(to.0, to.1, Button::Left);
    }
    assert_eq!(a.editor.selection().map(|m| m.bounds().0), Some(4));
    a.undo();
    assert!(a.editor.is_floating());
    assert_eq!(a.editor.selection().map(|m| m.bounds().0), Some(3));
}

// ---- 14: K and selected frames

#[test]
fn hold_honours_selected_frames_and_is_one_undo_step() {
    let mut a = app(4);
    range(&mut a, 1, 3);
    let depth = a.editor.history().undo_depth();
    key(&mut a, Key::Char(b'K'), PLAIN);
    let links: Vec<u32> = a.editor.doc().layers()[0].links().collect();
    assert!(links[1] != 0 && links[1] == links[2] && links[2] == links[3]);
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
}

#[test]
fn hold_on_frames_already_held_releases_them_all() {
    let mut a = app(4);
    range(&mut a, 1, 3);
    key(&mut a, Key::Char(b'K'), PLAIN);
    range(&mut a, 1, 3);
    key(&mut a, Key::Char(b'K'), PLAIN);
    let links: Vec<u32> = a.editor.doc().layers()[0].links().collect();
    assert_eq!(links, vec![0, 0, 0, 0]);
}

// ---- 16: the New dialog owns the keyboard

#[test]
fn keys_do_not_act_behind_the_new_dialog() {
    let mut a = app(2);
    a.open_new_dialog();
    key(&mut a, Key::Char(b'N'), PLAIN);
    assert_eq!(a.editor.doc().frames.len(), 2, "N added a frame");
    key(&mut a, Key::Period, PLAIN);
    assert_eq!(a.editor.frame, 0, "the frame changed");
    assert!(a.new_dialog.is_some());
}

#[test]
fn enter_creates_from_the_new_dialog_instead_of_playing() {
    let mut a = app(1);
    a.open_new_dialog();
    key(&mut a, Key::Enter, PLAIN);
    assert!(!a.playing);
    assert!(a.new_dialog.is_none(), "the sprite was created");
}

#[test]
fn escape_closes_the_new_dialog_before_touching_floating_pixels() {
    let mut a = app(1);
    select(&mut a);
    a.editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    a.editor
        .apply(FloatingCmd::Move {
            dx: 1,
            dy: 0,
            join: true,
        })
        .expect("move");
    a.open_new_dialog();
    key(&mut a, Key::Escape, PLAIN);
    assert!(a.new_dialog.is_none());
    assert!(
        a.editor.is_floating(),
        "the first Esc belongs to the dialog"
    );
}

// ---- 17: picking a color sticks

#[test]
fn a_picked_color_is_kept_while_live_palette_is_on() {
    let mut a = app(1);
    let swatches = a.editor.doc().colors.swatches().to_vec();
    let picked = a.editor.doc().colors.color(swatches[2]);
    a.editor.color = picked;
    dot(&mut a, 5, 5);
    a.editor.color = a.editor.doc().colors.color(swatches[5]);
    a.selected_swatch = Some(swatches[5]);
    a.live_palette = true;
    a.sync_color();
    let at = screen(&a, 5, 5);
    click(&mut a, at, ALT);
    a.sync_color();
    assert_eq!(a.editor.color, picked);
    assert_eq!(a.selected_swatch, None, "the swatch no longer describes it");
}

#[test]
fn a_folded_palette_lets_go_of_its_swatch_so_the_sliders_cannot_recolor_it() {
    let mut a = app(1);
    let swatches = a.editor.doc().colors.swatches().to_vec();
    let before = a.editor.doc().colors.color(swatches[5]);
    a.selected_swatch = Some(swatches[5]);
    a.live_palette = true;
    a.fold_section(crate::panel_right::Section::Palette, true);
    draw_whole(&mut a);
    assert_eq!(a.selected_swatch, None);
    let bright = crate::color_ui::Hsva {
        h: 0.5,
        s: 1.0,
        v: 1.0,
        a: 1.0,
    };
    a.pick_color(bright, true);
    assert_eq!(a.editor.doc().colors.color(swatches[5]), before);
}

// ---- 18: every overlapped tag can be reached

#[test]
fn a_short_tag_inside_a_long_one_can_be_clicked_and_so_can_the_long_one() {
    let mut a = app(4);
    a.editor
        .apply(pikslar_core::TagCmd::Add {
            name: "l".into(),
            from: 0,
            to: 3,
        })
        .expect("room");
    a.editor
        .apply(pikslar_core::TagCmd::Add {
            name: "s".into(),
            from: 1,
            to: 1,
        })
        .expect("room");
    draw(&mut a);
    let y = a.timeline.strip.y - 12;
    let x = |frame: i32| a.timeline.strip.x + frame * 34 + 10;
    let (on_short, on_long) = (x(1), x(3));
    click(&mut a, (on_short, y), PLAIN);
    assert_eq!(a.timeline.tag, Some(1), "the short tag");
    click(&mut a, (on_short, y), PLAIN);
    click(&mut a, (on_long, y), PLAIN);
    assert_eq!(a.timeline.tag, Some(0), "the long tag");
}

// ---- 10: naming a tag says what is happening

#[test]
fn naming_a_tag_says_how_to_finish() {
    let mut a = app(2);
    key(&mut a, Key::Char(b'T'), PLAIN);
    let status = a.status_text().unwrap_or("");
    assert!(status.contains("Enter"), "said {status:?}");
    assert!(status.contains("Delete"), "said {status:?}");
}

// ---- 23: playback gives way

#[test]
fn escape_stops_playback_and_leaves_the_selection() {
    let mut a = app(3);
    select(&mut a);
    a.toggle_play();
    key(&mut a, Key::Escape, PLAIN);
    assert!(!a.playing);
    assert!(a.has_selection());
}

#[test]
fn stepping_pauses_playback() {
    let mut a = app(3);
    a.toggle_play();
    key(&mut a, Key::Period, PLAIN);
    assert!(!a.playing);
    assert_eq!(a.editor.frame, 1);
}

// ---- found by the independent review

#[test]
fn undo_that_removes_selected_frames_still_draws() {
    let mut a = app(4);
    range(&mut a, 1, 3);
    a.undo();
    draw(&mut a);
    assert_eq!(a.editor.doc().frames.len(), 3);
    assert_eq!(
        a.timeline.selected(),
        Some((1, 2)),
        "clipped to the frames left"
    );
}

#[test]
fn pressing_a_handle_without_dragging_is_not_a_step() {
    let mut a = app(1);
    dot(&mut a, 3, 3);
    a.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(0, 0, 24, 24)),
            op: SelectOp::Replace,
        })
        .expect("select");
    a.set_tool(ToolChoice::Move);
    a.editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    a.nudge(1, 0, false);
    a.nudge(1, 0, false);
    a.sync_selection();
    let handle = a.selection_on_screen().expect("a selection");
    click(&mut a, (handle.x, handle.y), PLAIN);
    a.undo();
    assert_eq!(a.editor.selection().map(|m| m.bounds().0), Some(1));
}
