//! The key sequence behind the RC3 Soak B stop at minute 98 (2026-11-24): text "ab" kept with
//! Enter, the pencil chosen, then R and Shift+R. Kept text stays selected, so the turns lift it
//! and it floats until Enter, Esc, undo or Select none. These pin that behaviour and what each
//! way out leaves, so the soak harness and the program agree on it.

use crate::app::App;
use crate::test_app::*;
use pikslar_core::color::rgba;
use pikslar_platform::{Key, Mods};

const RED: u32 = rgba(255, 0, 0, 255);
const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};

fn pixels(a: &App) -> Vec<u32> {
    (0..32)
        .flat_map(|y| (0..32).map(move |x| (x, y)))
        .map(|(x, y)| a.editor.color_at(x, y))
        .collect()
}

/// What the soak's w10 does: the text tool, Enter, "ab" typed, Enter, the pencil again.
fn text_ab() -> App {
    let mut a = app(1);
    a.editor.color = RED;
    key(&mut a, Key::Char(b'X'), PLAIN);
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.text.is_some(), "Enter opened a session");
    a.text_typed('a');
    a.text_typed('b');
    key(&mut a, Key::Enter, PLAIN);
    key(&mut a, Key::Char(b'B'), PLAIN);
    a
}

/// What the soak's old w4 did: rotate right, then left.
fn turn_and_back(a: &mut App) {
    key(a, Key::Char(b'R'), PLAIN);
    key(a, Key::Char(b'R'), SHIFT);
}

#[test]
fn kept_text_is_drawn_and_stays_selected_with_nothing_floating() {
    let blank = pixels(&app(1));
    let a = text_ab();
    assert!(a.text.is_none(), "the session ended");
    assert!(!a.editor.is_floating());
    assert!(a.has_selection(), "the kept text is selected");
    assert_ne!(pixels(&a), blank, "the text was drawn");
}

#[test]
fn turning_the_selected_text_lifts_it_and_enter_puts_it_down_as_it_was() {
    let mut a = text_ab();
    let kept = pixels(&a);
    key(&mut a, Key::Char(b'R'), PLAIN);
    assert!(a.editor.is_floating(), "R lifted the selection");
    key(&mut a, Key::Char(b'R'), SHIFT);
    assert!(
        a.editor.is_floating(),
        "still floating after turning back: the minute-98 state"
    );
    key(&mut a, Key::Enter, PLAIN);
    assert!(!a.editor.is_floating(), "Enter anchored it");
    assert!(a.has_selection(), "and it is still selected");
    assert_eq!(
        pixels(&a),
        kept,
        "a turn and a turn back leave the text as it was"
    );
}

/// A turn and a turn back change no pixel, so anchoring them is no step, and the next undo is
/// of the text itself.
#[test]
fn anchoring_turns_that_cancel_out_takes_no_step() {
    let blank = pixels(&app(1));
    let mut a = text_ab();
    let (kept, depth) = (pixels(&a), a.editor.history().undo_depth());
    turn_and_back(&mut a);
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(a.editor.history().undo_depth(), depth, "no step");
    assert_eq!(pixels(&a), kept);
    key(&mut a, Key::Char(b'Z'), CTRL);
    assert!(!a.editor.is_floating());
    assert_eq!(pixels(&a), blank, "undo then takes the text away");
}

/// A single turn does change the pixels: anchored, it is one step, and undo brings the text back
/// as it was kept.
#[test]
fn an_anchored_single_turn_is_one_step_and_undo_restores_the_text() {
    let mut a = text_ab();
    let (kept, depth) = (pixels(&a), a.editor.history().undo_depth());
    key(&mut a, Key::Char(b'R'), PLAIN);
    key(&mut a, Key::Enter, PLAIN);
    assert!(!a.editor.is_floating());
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    assert_ne!(pixels(&a), kept, "the text turned");
    key(&mut a, Key::Char(b'Z'), CTRL);
    assert!(
        !a.editor.is_floating(),
        "undo does not bring the float back"
    );
    assert_eq!(pixels(&a), kept);
}

#[test]
fn escape_puts_the_lifted_text_back_and_takes_no_step() {
    let mut a = text_ab();
    let (kept, depth) = (pixels(&a), a.editor.history().undo_depth());
    turn_and_back(&mut a);
    key(&mut a, Key::Escape, PLAIN);
    assert!(!a.editor.is_floating());
    assert_eq!(pixels(&a), kept);
    assert_eq!(a.editor.history().undo_depth(), depth);
}

/// Undo while it floats steps back the turns one at a time, then puts it back; the text stays.
#[test]
fn undo_while_the_text_floats_steps_back_the_turns_then_puts_it_back() {
    let mut a = text_ab();
    let (kept, depth) = (pixels(&a), a.editor.history().undo_depth());
    turn_and_back(&mut a);
    key(&mut a, Key::Char(b'Z'), CTRL);
    assert!(
        a.editor.is_floating(),
        "the first undo takes back Shift+R only"
    );
    key(&mut a, Key::Char(b'Z'), CTRL);
    let mut undos = 2;
    while a.editor.is_floating() && undos < 4 {
        key(&mut a, Key::Char(b'Z'), CTRL);
        undos += 1;
    }
    assert!(!a.editor.is_floating(), "put back after {undos} undos");
    assert_eq!(pixels(&a), kept, "only the lift was undone, not the text");
    assert_eq!(a.editor.history().undo_depth(), depth);
}

#[test]
fn select_none_anchors_the_lifted_text_and_drops_the_selection() {
    let mut a = text_ab();
    let kept = pixels(&a);
    turn_and_back(&mut a);
    key(&mut a, Key::Char(b'D'), CTRL);
    assert!(!a.editor.is_floating());
    assert!(!a.has_selection());
    assert_eq!(pixels(&a), kept);
}

/// The soak's contract since 2026-11-24: Ctrl+D ends w10 and starts w4, so the turns are of the
/// whole cel and nothing floats; the picture comes back after the turn back.
#[test]
fn with_the_selection_dropped_first_the_turns_are_of_the_whole_cel() {
    let mut a = text_ab();
    let kept = pixels(&a);
    key(&mut a, Key::Char(b'D'), CTRL);
    assert!(!a.has_selection());
    let depth = a.editor.history().undo_depth();
    key(&mut a, Key::Char(b'D'), CTRL);
    assert_eq!(
        a.editor.history().undo_depth(),
        depth,
        "Select none with no selection takes no step"
    );
    key(&mut a, Key::Char(b'R'), PLAIN);
    assert!(!a.editor.is_floating(), "a whole-cel turn does not float");
    assert_ne!(pixels(&a), kept, "the cel turned");
    key(&mut a, Key::Char(b'R'), SHIFT);
    assert!(!a.editor.is_floating());
    assert_eq!(pixels(&a), kept, "and turned back");
    assert_eq!(a.editor.history().undo_depth(), depth + 2, "two steps");
}
