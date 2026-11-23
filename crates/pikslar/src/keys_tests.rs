//! The keyboard's rules: Shift is the bigger version of the same action.

use crate::app::{Gesture, ToolChoice};
use crate::test_app::*;
use pikslar_core::FloatingCmd;
use pikslar_core::Tool;
use pikslar_core::{ColorMode, Document};
use pikslar_platform::{Button, Key, Mods, Window};

fn said(a: &crate::app::App) -> &str {
    a.message.as_ref().map_or("", |(text, _)| text)
}

fn layers(frames: usize, count: usize) -> crate::app::App {
    let mut a = app(frames);
    for _ in 1..count {
        a.layer_action(0);
    }
    a.editor.layer = 0;
    a
}

#[test]
fn shift_left_and_right_move_the_frame_like_shift_comma() {
    let mut a = app(3);
    a.run(pikslar_core::FrameCmd::Retime {
        from: 0,
        durations: vec![250],
    });
    let depth = a.editor.history().undo_depth();
    key(&mut a, Key::Right, SHIFT);
    assert_eq!(a.editor.frame, 1);
    assert_eq!(
        a.editor.history().undo_depth(),
        depth + 1,
        "the frame moved"
    );
}

#[test]
fn shift_backspace_removes_frames_like_shift_delete() {
    let mut a = app(3);
    key(&mut a, Key::Backspace, SHIFT);
    assert_eq!(a.editor.doc().frames.len(), 2);
}

#[test]
fn up_and_down_change_the_layer() {
    let mut a = layers(1, 3);
    key(&mut a, Key::Up, PLAIN);
    assert_eq!(a.editor.layer, 1);
    key(&mut a, Key::Up, PLAIN);
    key(&mut a, Key::Up, PLAIN);
    assert_eq!(a.editor.layer, 2, "stops at the top");
    key(&mut a, Key::Down, PLAIN);
    assert_eq!(a.editor.layer, 1);
}

#[test]
fn shift_up_and_down_move_the_layer() {
    let mut a = layers(1, 3);
    let first = a.editor.doc().layers()[0].id();
    key(&mut a, Key::Up, SHIFT);
    assert_eq!(a.editor.layer, 1, "the active layer stays the same one");
    assert_eq!(a.editor.doc().layers()[1].id(), first);
}

#[test]
fn up_and_down_still_nudge_lifted_pixels() {
    let mut a = layers(1, 2);
    select(&mut a);
    a.editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    let y = a.editor.selection().map(|m| m.bounds().1);
    key(&mut a, Key::Down, PLAIN);
    assert_eq!(a.editor.layer, 0);
    assert_eq!(a.editor.selection().map(|m| m.bounds().1), y.map(|v| v + 1));
    key(&mut a, Key::Up, PLAIN);
    assert_eq!(a.editor.layer, 0);
    assert_eq!(a.editor.selection().map(|m| m.bounds().1), y);
}

#[test]
fn home_and_end_go_to_the_first_and_last_frame() {
    let mut a = app(5);
    key(&mut a, Key::End, PLAIN);
    assert_eq!(a.editor.frame, 4);
    key(&mut a, Key::Home, PLAIN);
    assert_eq!(a.editor.frame, 0);
}

#[test]
fn home_and_end_stay_inside_the_loop_like_stepping_does() {
    let mut a = app(6);
    a.set_frame(2);
    range(&mut a, 2, 3);
    assert!(a.looping_span().is_some());
    key(&mut a, Key::End, PLAIN);
    assert_eq!(a.editor.frame, 3);
    key(&mut a, Key::Home, PLAIN);
    assert_eq!(a.editor.frame, 2);
}

#[test]
fn home_stops_playback_like_stepping_does() {
    let mut a = app(3);
    a.toggle_play();
    key(&mut a, Key::Home, PLAIN);
    assert!(!a.playing);
}

#[test]
fn a_transform_key_without_a_selection_on_an_empty_frame_explains_itself() {
    for (letter, mods) in [(b'R', PLAIN), (b'R', SHIFT), (b'H', SHIFT), (b'V', SHIFT)] {
        let mut a = app(1);
        a.set_tool(ToolChoice::Paint(Tool::Eraser));
        key(&mut a, Key::Char(letter), mods);
        assert!(said(&a).contains("nothing"), "{}", letter as char);
        assert!(!a.editor.history().can_undo());
        assert_eq!(a.tool, ToolChoice::Paint(Tool::Eraser), "the tool stays");
    }
}

#[test]
fn v_alone_still_picks_the_move_tool() {
    let mut a = app(1);
    key(&mut a, Key::Char(b'V'), PLAIN);
    assert_eq!(a.tool, ToolChoice::Move);
}

#[test]
fn delete_with_nothing_selected_says_what_would_work() {
    let mut a = app(3);
    key(&mut a, Key::Delete, PLAIN);
    assert_eq!(a.editor.doc().frames.len(), 3);
    assert!(said(&a).contains("Shift"));
}

#[test]
fn moving_a_frame_past_the_end_says_so() {
    let mut a = app(2);
    key(&mut a, Key::Left, SHIFT);
    assert!(said(&a).contains("end"));
}

#[test]
fn moving_a_layer_past_the_end_says_so() {
    let mut a = layers(1, 2);
    key(&mut a, Key::Down, SHIFT);
    assert!(said(&a).contains("end"));
}

#[test]
fn a_long_tag_name_says_why_it_stopped() {
    let mut a = app(2);
    key(&mut a, Key::Char(b'T'), PLAIN);
    for _ in 0..pikslar_core::MAX_TAG_NAME + 1 {
        key(&mut a, Key::Char(b'A'), PLAIN);
    }
    assert!(said(&a).contains(&pikslar_core::MAX_TAG_NAME.to_string()));
}

#[test]
fn a_new_sprite_suggests_what_to_do_next() {
    let mut a = app(1);
    assert!(a
        .first_use_hint()
        .expect("something to try")
        .contains("Hover"));
    dot(&mut a, 5, 5);
    assert!(a.first_use_hint().expect("the next thing").contains('N'));
    a.new_frame(false);
    assert_eq!(a.first_use_hint(), None);
    a.editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    assert_eq!(a.first_use_hint(), None, "once learned, it stays quiet");
}

#[test]
fn the_suggestion_stays_off_the_rewind_track() {
    let a = app(1);
    assert_eq!(a.status_text(), None);
}

#[test]
fn a_file_or_imported_image_gets_no_suggestions() {
    let mut a = app(1);
    a.replace_document(Document::new(32, 32, ColorMode::Rgba).expect("valid"), None);
    assert_eq!(a.first_use_hint(), None);
}

#[test]
fn a_sprite_made_with_new_gets_the_suggestions_again() {
    let mut a = app(1);
    a.replace_document(Document::new(32, 32, ColorMode::Rgba).expect("valid"), None);
    let dialog = crate::app::NewDialog {
        size: 16,
        mode: ColorMode::Palette,
    };
    assert!(a.create_new(Window::detached(), dialog));
    assert!(a.first_use_hint().is_some());
}

#[test]
fn hovering_the_rewind_track_leaves_its_handle_visible() {
    let mut a = app(1);
    let bottom = a.layout.bottom;
    a.ui.track(bottom.x + bottom.w / 2, bottom.y + 66);
    draw(&mut a);
    assert_eq!(a.status_text(), None);
}

#[test]
fn keys_wait_while_a_control_is_held() {
    let mut a = layers(3, 2);
    a.ui.active = 7;
    key(&mut a, Key::Up, PLAIN);
    key(&mut a, Key::Right, PLAIN);
    key(&mut a, Key::Home, PLAIN);
    assert_eq!((a.editor.layer, a.editor.frame), (0, 0));
}

#[test]
fn keys_that_would_end_a_pan_wait_for_it() {
    let mut a = layers(3, 2);
    let at = screen(&a, 5, 5);
    a.pointer_down(at.0, at.1, Button::Middle, PLAIN);
    key(&mut a, Key::Up, PLAIN);
    key(&mut a, Key::End, PLAIN);
    assert!(matches!(a.gesture, Gesture::Pan { .. }));
    assert_eq!((a.editor.layer, a.editor.frame), (0, 0));
}

#[test]
fn holding_shift_arrow_moves_a_frame_or_layer_only_once() {
    let mut a = layers(6, 4);
    let held = |a: &mut crate::app::App, k| {
        for repeat in [false, true, true, true] {
            a.key_down(k, SHIFT, repeat, Window::detached());
        }
    };
    held(&mut a, Key::Right);
    assert_eq!(a.editor.frame, 1);
    held(&mut a, Key::Up);
    assert_eq!(a.editor.layer, 1);
    held(&mut a, Key::Period);
    assert_eq!(a.editor.frame, 2);
}

#[test]
fn ctrl_shift_arrow_does_not_reorder() {
    let mut a = app(3);
    let depth = a.editor.history().undo_depth();
    let both = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };
    key(&mut a, Key::Right, both);
    assert_eq!(a.editor.frame, 1);
    assert_eq!(a.editor.history().undo_depth(), depth);
}

#[test]
fn home_on_the_first_frame_leaves_lifted_pixels_alone() {
    let mut a = app(3);
    select(&mut a);
    a.editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    key(&mut a, Key::Home, PLAIN);
    assert!(a.editor.is_floating());
}

#[test]
fn what_a_panel_asks_for_happens_after_drawing_and_in_order() {
    let mut a = app(1);
    a.defer(|a| {
        a.layer_action(0);
    });
    a.defer(|a| a.editor.layer = 0);
    assert_eq!(a.editor.doc().layers().len(), 1, "nothing yet");
    let (w, h) = (1280, 800);
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = pikslar_render::Surface { px: &mut px, w, h };
    a.draw_interface(&mut surface, Window::detached(), &mut Vec::new());
    assert_eq!(a.editor.doc().layers().len(), 2);
    assert_eq!(a.editor.layer, 0, "later actions see earlier ones");
}
