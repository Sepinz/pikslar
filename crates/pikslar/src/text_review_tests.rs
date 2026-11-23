//! Review fixes for the text tool: playback, undo limits, failed redraws, pastes, rebinding.

use crate::app::{App, ToolChoice};
use crate::test_app::*;
use pikslar_core::color::rgba;
use pikslar_platform::{Key, Mods};
use pikslar_ui::TextEdit;
use std::time::{Duration, Instant};

const RED: u32 = rgba(255, 0, 0, 255);

fn text_app(frames: usize) -> App {
    let mut a = app(frames);
    a.editor.color = RED;
    a.set_tool(ToolChoice::Text);
    draw(&mut a);
    a
}

#[test]
fn a_session_started_by_enter_while_playing_stops_the_animation_and_survives_a_tick() {
    let mut a = text_app(3);
    a.toggle_play();
    a.set_tool(ToolChoice::Text);
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.text.is_some(), "Enter opened a session");
    assert!(!a.playing, "playback stopped when the session began");
    a.next_frame_at = Instant::now() - Duration::from_millis(1);
    a.advance_playback();
    assert!(a.text.is_some(), "the session is still alive");
    assert_eq!(a.editor.frame, 0, "and the frame did not change");
}

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};

fn shift_enter(a: &mut App, times: usize) {
    let shift = Mods {
        shift: true,
        ..PLAIN
    };
    (0..times).for_each(|_| key(a, Key::Enter, shift));
}

fn lit(a: &App) -> usize {
    (0..32)
        .flat_map(|y| (0..32).map(move |x| (x, y)))
        .filter(|&(x, y)| a.editor.color_at(x, y) != 0)
        .count()
}

#[test]
fn undo_that_would_not_fit_at_the_current_size_is_refused_and_the_text_stays_drawable() {
    let mut a = text_app(1);
    let at = screen(&a, 0, 0);
    click_whole(&mut a, at, PLAIN);
    "W".repeat(144).chars().for_each(|c| a.text_typed(c));
    shift_enter(&mut a, 30);
    (0..30).for_each(|_| key(&mut a, Key::Backspace, PLAIN));
    (0..7).for_each(|_| key(&mut a, Key::Char(b'T'), CTRL));
    (0..30).for_each(|_| key(&mut a, Key::Char(b'Z'), CTRL));
    let t = a.text.as_ref().expect("session");
    let area = pikslar_core::glyphs::text_area(t.font, t.edit.text(), t.scale, t.spacing);
    assert!(area <= pikslar_core::glyphs::MAX_TEXT_AREA, "{area}");
    assert!(said(&a).contains("too large"), "{}", said(&a));
    key(&mut a, Key::Enter, PLAIN);
    assert!(lit(&a) > 0, "Enter anchored the text that was shown");
}

#[test]
fn keeping_a_drawing_that_is_over_the_limit_says_so_and_anchors_nothing() {
    let mut a = text_app(1);
    let at = screen(&a, 0, 0);
    click_whole(&mut a, at, PLAIN);
    let depth = a.editor.history().undo_depth();
    let t = a.text.as_mut().expect("session");
    t.edit = TextEdit::new(&format!("{}\n\n\n", "W".repeat(144)), 256);
    t.scale = 8;
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.text.is_none(), "the session ended");
    assert!(said(&a).contains("too large"), "{}", said(&a));
    assert_eq!(a.editor.history().undo_depth(), depth, "no empty step");
    assert_eq!(lit(&a), 0);
}

const GREEN: u32 = rgba(0, 255, 0, 255);

/// An indexed document whose colour table is full, with green in it and red not.
fn full_table_app() -> App {
    let mut a = text_app(1);
    let mut doc =
        pikslar_core::Document::new(32, 32, pikslar_core::ColorMode::Palette).expect("valid");
    let table = std::sync::Arc::make_mut(&mut doc.colors);
    table.id_for(GREEN).expect("green");
    (0..65_536u32)
        .map(|i| rgba((i >> 8) as u8, i as u8, 1, 255))
        .take_while(|&c| table.id_for(c).is_ok())
        .for_each(drop);
    assert!(table.id_for(RED).is_err(), "the table is full");
    a.replace_document(doc, None);
    a.set_tool(ToolChoice::Text);
    a.editor.color = GREEN;
    draw(&mut a);
    let at = screen(&a, 2, 2);
    click_whole(&mut a, at, PLAIN);
    a.text_typed('a');
    a
}

#[test]
fn a_redraw_the_layer_refuses_leaves_the_text_and_its_undo_as_they_were() {
    let mut a = full_table_app();
    a.editor.color = RED;
    a.text_typed('b');
    let text = a.text.as_ref().map(|t| t.edit.text().to_owned());
    assert_eq!(text.as_deref(), Some("a"), "the refused edit was not kept");
    assert!(said(&a).starts_with("Error"), "{}", said(&a));
    key(&mut a, Key::Char(b'Z'), CTRL);
    let text = a.text.as_ref().map(|t| t.edit.text().to_owned());
    assert_eq!(
        text.as_deref(),
        Some(""),
        "one undo goes back past 'a' only"
    );
}

#[test]
fn a_refused_size_change_is_taken_back_and_a_stale_colour_does_not_retry_forever() {
    let mut a = full_table_app();
    a.editor.color = RED;
    draw_whole(&mut a);
    assert!(
        said(&a).starts_with("Error"),
        "the first redraw failed: [{}]",
        said(&a)
    );
    a.message = None;
    draw_whole(&mut a);
    assert!(a.message.is_none(), "no second attempt: {}", said(&a));
    key(&mut a, Key::Char(b'T'), CTRL);
    assert_eq!(a.text.as_ref().map(|t| t.scale), Some(1), "scale not kept");
    assert_eq!(a.text_style.scale, 1);
}

fn session_text(a: &App) -> String {
    a.text
        .as_ref()
        .map(|t| t.edit.text().to_owned())
        .unwrap_or_default()
}

#[test]
fn a_paste_into_a_session_is_one_undo_entry() {
    let mut a = text_app(1);
    let at = screen(&a, 2, 2);
    click_whole(&mut a, at, PLAIN);
    a.text_typed('x');
    a.insert_pasted_text("hello\nworld");
    assert_eq!(session_text(&a), "xhello\nworld");
    key(&mut a, Key::Char(b'Z'), CTRL);
    assert_eq!(
        session_text(&a),
        "x",
        "one Ctrl+Z took the whole paste back"
    );
    key(&mut a, Key::Enter, PLAIN);
    assert!(lit(&a) > 0, "and the rest is still drawn");
}

#[test]
fn a_paste_that_would_make_the_drawing_too_large_keeps_the_longest_prefix_that_fits() {
    let mut a = text_app(1);
    let at = screen(&a, 0, 0);
    click_whole(&mut a, at, PLAIN);
    (0..7).for_each(|_| key(&mut a, Key::Char(b'T'), CTRL));
    let pasted = format!("{}{}", "W".repeat(144), "\n".repeat(60));
    a.insert_pasted_text(&pasted);
    let text = session_text(&a);
    assert!(text.starts_with(&"W".repeat(144)));
    let breaks = text.matches('\n').count();
    assert!((1..60).contains(&breaks) || breaks == 0, "{breaks}");
    assert!(text.len() < pasted.len(), "it was cut");
    assert!(said(&a).contains("too large"), "{}", said(&a));
    key(&mut a, Key::Char(b'Z'), CTRL);
    assert_eq!(session_text(&a), "", "still one entry");
}

fn rebind_undo(a: &mut App, letter: u8) {
    use crate::shortcuts::{Action, Binding};
    let binding = Binding {
        letter,
        ctrl: true,
        shift: false,
    };
    a.keymap.bind(Action::Undo, Some(binding));
}

#[test]
fn in_session_undo_follows_the_rebound_undo_key_and_the_session_survives() {
    let mut a = text_app(1);
    let at = screen(&a, 2, 2);
    click_whole(&mut a, at, PLAIN);
    "abc".chars().for_each(|c| a.text_typed(c));
    rebind_undo(&mut a, b'U');
    key(&mut a, Key::Char(b'U'), CTRL);
    assert!(a.text.is_some(), "the session is still open");
    assert_eq!(session_text(&a), "ab", "the new key undid one edit");
    key(&mut a, Key::Char(b'Z'), CTRL);
    assert!(a.text.is_some(), "the old key does not end the session");
    assert_eq!(session_text(&a), "ab", "and it is not undo any more");
}

#[test]
fn the_redo_keys_do_not_end_a_session() {
    let mut a = text_app(1);
    let at = screen(&a, 2, 2);
    click_whole(&mut a, at, PLAIN);
    a.text_typed('a');
    let shift = Mods {
        shift: true,
        ..CTRL
    };
    key(&mut a, Key::Char(b'Z'), shift);
    key(&mut a, Key::Char(b'Y'), CTRL);
    assert!(a.text.is_some());
    assert_eq!(session_text(&a), "a");
}
