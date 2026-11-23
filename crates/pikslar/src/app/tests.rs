//! Tests for pointer gestures, document replacement and random input on the app.

use super::starter::STARTER_PALETTE;
use super::*;
use pikslar_core::SelectionCmd;
use pikslar_platform::{Event, Handler, Key};

const CENTER: (i32, i32) = (288, 283);

fn scratch_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-launch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("folder");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("written");
    path
}

#[test]
fn launching_with_an_image_never_saves_over_it() {
    let image = pikslar_format::png::Image {
        width: 2,
        height: 2,
        px: vec![0xff00_00ff; 4],
    };
    let path = scratch_file(
        "art.png",
        &pikslar_format::png::encode(&image).expect("encodes"),
    );
    let launched = App::launch(Some(path), None, None);
    assert_eq!(launched.path, None);
    assert!(!launched.first_use, "this is not an empty first session");
}

#[test]
fn launching_with_a_project_saves_back_to_it() {
    let doc = Document::new(4, 4, ColorMode::Palette).expect("valid");
    let path = scratch_file("art.piks", &pikslar_format::piks::save(&doc));
    assert_eq!(App::launch(Some(path.clone()), None, None).path, Some(path));
}

#[test]
fn a_file_with_data_this_version_cannot_keep_is_protected_under_any_spelling() {
    let doc = Document::new(4, 4, ColorMode::Palette).expect("valid");
    let path = scratch_file("newer.piks", &pikslar_format::piks::save(&doc));
    let mut launched = App::launch(Some(path.clone()), None, None);
    assert!(!launched.is_protected(&path), "nothing to protect yet");
    launched.protected_path = Some(path.clone());
    let upper = path.with_file_name("NEWER.PIKS");
    assert!(launched.is_protected(&upper));
    assert!(!launched.is_protected(&path.with_file_name("other.piks")));
}

#[test]
fn only_complete_projects_may_be_saved_over() {
    use crate::files::{protected_path, save_path_for};
    use pikslar_format::piks::LoadNotes;
    let path = std::path::PathBuf::from("art.piks");
    let clean = LoadNotes::default();
    assert_eq!(save_path_for(path.clone(), &clean), Some(path.clone()));
    assert_eq!(protected_path(&path, &clean), None);
    let newer = LoadNotes {
        not_kept: vec![*b"ABCD"],
        ..LoadNotes::default()
    };
    assert_eq!(save_path_for(path.clone(), &newer), None);
    assert_eq!(protected_path(&path, &newer), Some(path));
    let damaged = LoadNotes {
        damaged: vec!["tags"],
        ..LoadNotes::default()
    };
    assert_eq!(save_path_for("a.png".into(), &clean), None);
    assert_eq!(save_path_for("a.piks".into(), &damaged), None);
}

fn app() -> App {
    let mut app = App::launch(None, Some(32), None);
    app.resized(800, 600, 1);
    app
}

fn press(a: &mut App, x: i32, y: i32) {
    a.pointer_down(x, y, Button::Left, Mods::default());
}

fn release(a: &mut App, x: i32, y: i32) {
    a.pointer_up(x, y, Button::Left);
}

fn drag(a: &mut App, from: (i32, i32), to: (i32, i32)) {
    press(a, from.0, from.1);
    a.canvas_move(to.0, to.1);
    release(a, to.0, to.1);
}

#[test]
fn a_second_button_leaves_the_stroke_alone() {
    let mut a = app();
    press(&mut a, CENTER.0, CENTER.1);
    assert_eq!(a.gesture, Gesture::Stroke);
    a.pointer_down(CENTER.0, CENTER.1, Button::Right, Mods::default());
    a.pointer_up(CENTER.0, CENTER.1, Button::Right);
    assert!(a.editor.is_stroking());
    release(&mut a, CENTER.0, CENTER.1);
    assert_eq!(a.gesture, Gesture::None);
    assert!(a.is_modified());
}

#[test]
fn a_click_on_the_canvas_paints_the_chosen_color() {
    let mut a = app();
    press(&mut a, CENTER.0, CENTER.1);
    release(&mut a, CENTER.0, CENTER.1);
    let (x, y) = a.view.to_doc(CENTER.0, CENTER.1);
    assert_eq!(a.editor.color_at(x, y), STARTER_PALETTE[0]);
}

#[test]
fn opening_another_document_fits_it_to_the_view() {
    let mut a = app();
    let before = a.view;
    let doc = Document::new(512, 512, ColorMode::Rgba).expect("valid");
    a.replace_document(doc, None);
    assert_ne!((a.view.num, a.view.den), (before.num, before.den));
    assert_eq!(
        a.editor.doc().colors.swatches().len(),
        STARTER_PALETTE.len()
    );
}

#[test]
fn dragging_a_marquee_selects_and_a_click_outside_clears_it() {
    let mut a = app();
    a.set_tool(ToolChoice::Marquee);
    drag(&mut a, CENTER, (CENTER.0 + 80, CENTER.1 + 40));
    assert!(a.has_selection());
    assert_eq!(a.gesture, Gesture::None);
    let far = (CENTER.0 - 150, CENTER.1 - 150);
    press(&mut a, far.0, far.1);
    release(&mut a, far.0, far.1);
    assert!(!a.has_selection());
    assert!(!a.is_modified(), "selecting is not an edit");
}

#[test]
fn a_click_inside_a_selection_never_lifts_pixels() {
    let mut a = app();
    a.set_tool(ToolChoice::Marquee);
    drag(&mut a, CENTER, (CENTER.0 + 80, CENTER.1 + 40));
    let inside = (CENTER.0 + 20, CENTER.1 + 20);
    press(&mut a, inside.0, inside.1);
    release(&mut a, inside.0, inside.1);
    assert!(!a.editor.is_floating());
    assert!(!a.has_selection(), "a plain click clears the marquee");
}

#[test]
fn dragging_inside_a_selection_moves_it_and_undo_restores_it() {
    let mut a = app();
    press(&mut a, CENTER.0, CENTER.1);
    release(&mut a, CENTER.0, CENTER.1);
    let (x, y) = a.view.to_doc(CENTER.0, CENTER.1);
    a.select_change(SelectionCmd::All);
    a.set_tool(ToolChoice::Move);
    let scale = a.view.num / a.view.den;
    drag(&mut a, CENTER, (CENTER.0 + scale * 3, CENTER.1));
    assert!(a.editor.is_floating());
    a.settle();
    assert_eq!(a.editor.color_at(x + 3, y), STARTER_PALETTE[0]);
    assert_eq!(a.editor.color_at(x, y), 0);
    a.undo();
    assert_eq!(a.editor.color_at(x, y), STARTER_PALETTE[0]);
}

#[test]
fn changing_frame_places_floating_pixels_first() {
    let mut a = app();
    a.select_change(SelectionCmd::All);
    a.nudge(1, 0, false);
    assert!(a.editor.is_floating());
    a.set_frame(0);
    assert!(!a.editor.is_floating());
}

#[test]
fn shift_turns_on_line_rhythm_for_one_stroke() {
    let mut a = app();
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    a.pointer_down(CENTER.0, CENTER.1, Button::Left, shift);
    assert!(a.editor.rhythm);
    a.pointer_up(CENTER.0, CENTER.1, Button::Left);
    press(&mut a, CENTER.0, CENTER.1);
    assert!(!a.editor.rhythm);
}

fn all_keys() -> Vec<Key> {
    let letters = b"BEGILUMQWVORHAZYCXD0S".iter().map(|&c| Key::Char(c));
    letters
        .chain([
            Key::Space,
            Key::Escape,
            Key::Enter,
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::Home,
            Key::End,
            Key::BracketLeft,
            Key::BracketRight,
            Key::Comma,
            Key::Period,
            Key::Plus,
            Key::Minus,
            Key::Delete,
            Key::Backspace,
        ])
        .collect()
}

/// Random input in any order must never panic or corrupt the frame loop.
#[test]
fn random_input_never_breaks_the_editor() {
    let window = Window::detached();
    let (w, h) = (800, 600);
    let mut px = vec![0u32; (w * h) as usize];
    let keys = all_keys();
    for seed in 0..6u32 {
        let mut a = app();
        let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(7);
        let mut next = move |limit: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % limit
        };
        for _ in 0..4_000 {
            let (x, y) = (next(w as u32) as i32, next(h as u32) as i32);
            let mods = Mods {
                ctrl: next(4) == 0,
                shift: next(4) == 0,
                alt: next(6) == 0,
            };
            let button =
                [Button::Left, Button::Left, Button::Right, Button::Middle][next(4) as usize];
            let event = match next(10) {
                0..=3 => Event::Move { x, y },
                4 | 5 => Event::Down { x, y, button, mods },
                6 | 7 => Event::Up { x, y, button },
                8 => Event::KeyDown {
                    key: keys[next(keys.len() as u32) as usize],
                    mods,
                    repeat: false,
                },
                _ => Event::Wheel {
                    x,
                    y,
                    ticks: 1 - next(3) as i32,
                },
            };
            a.event(event, window);
            if next(8) == 0 {
                let mut surface = Surface { px: &mut px, w, h };
                let mut dirty = Vec::new();
                a.frame(&mut surface, window, &mut dirty);
            }
        }
        a.settle();
        a.undo();
        assert_eq!(a.gesture, Gesture::None);
    }
}
