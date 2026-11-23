//! The export panel's range: a Range button, then the first and last frame, counted from 1 as
//! the timeline shows them, kept in order and inside the frames there are.

use super::*;
use crate::test_app::{app, draw_whole_at, PLAIN};
use pikslar_platform::Button;

const FROM_LESS: u32 = ID_EXPORT + 14;
const FROM_MORE: u32 = ID_EXPORT + 15;
const TO_LESS: u32 = ID_EXPORT + 16;
const RANGE: u32 = ID_EXPORT + 13;

fn widget(a: &App, id: u32) -> Option<Rect> {
    a.ui.widgets()
        .iter()
        .find(|(_, w)| *w == id)
        .map(|(r, _)| *r)
}

/// Draws the whole window, then presses and lets go of the control `id`.
fn press(a: &mut App, id: u32, (w, h): (i32, i32)) {
    draw_whole_at(a, w, h);
    let r = widget(a, id).unwrap_or_else(|| panic!("{id} is on screen"));
    let (x, y) = (r.x + r.w / 2, r.y + r.h / 2);
    a.pointer_down(x, y, Button::Left, PLAIN);
    draw_whole_at(a, w, h);
    a.pointer_up(x, y, Button::Left);
    draw_whole_at(a, w, h);
}

fn frames(a: &App) -> Option<FrameChoice> {
    a.export_dialog.map(|c| c.frames)
}

#[test]
fn a_range_starts_at_the_frame_shown_and_its_ends_step_in_order() {
    let size = (1280, 800);
    let mut a = app(6);
    a.editor.frame = 2;
    a.open_export_dialog(true);
    draw_whole_at(&mut a, size.0, size.1);
    assert!(widget(&a, FROM_LESS).is_none(), "no ends before a range");
    press(&mut a, RANGE, size);
    assert_eq!(frames(&a), Some(FrameChoice::Range(2, 5)));
    press(&mut a, TO_LESS, size);
    press(&mut a, TO_LESS, size);
    press(&mut a, TO_LESS, size);
    assert_eq!(
        frames(&a),
        Some(FrameChoice::Range(2, 2)),
        "To stops at From"
    );
    press(&mut a, TO_LESS, size);
    assert_eq!(
        frames(&a),
        Some(FrameChoice::Range(1, 1)),
        "and takes it along"
    );
    press(&mut a, FROM_MORE, size);
    assert_eq!(frames(&a), Some(FrameChoice::Range(2, 2)), "From pushes To");
    for _ in 0..8 {
        press(&mut a, FROM_MORE, size);
    }
    assert_eq!(
        frames(&a),
        Some(FrameChoice::Range(5, 5)),
        "never past the last"
    );
}

#[test]
fn the_range_row_fits_the_panel_from_the_smallest_window_to_3x() {
    for (w, h, scale) in [(640, 600, 1), (1920, 1003, 2), (2560, 1440, 3)] {
        let mut a = app(4);
        a.resized(w, h, scale);
        a.open_export_dialog(true);
        a.export_dialog = a.export_dialog.map(|c| ExportChoice {
            frames: FrameChoice::Range(0, 3),
            ..c
        });
        draw_whole_at(&mut a, w, h);
        let ours: Vec<_> =
            a.ui.widgets()
                .iter()
                .filter(|(_, id)| (ID_EXPORT..ID_EXPORT + 100).contains(id))
                .copied()
                .collect();
        let bottom = ours.iter().map(|(r, _)| r.bottom()).max().unwrap_or(0);
        for (r, id) in &ours {
            let inside = r.x >= 0 && r.y >= 0 && r.right() <= w && r.bottom() <= h;
            assert!(inside, "{w}x{h}: {id} at {r:?}");
            let overlaps = ours
                .iter()
                .any(|(o, other)| other != id && !o.intersect(*r).is_empty());
            assert!(!overlaps, "{w}x{h}: {id} overlaps another control");
        }
        assert!(widget(&a, TO_LESS).is_some_and(|r| r.bottom() < bottom));
    }
}

#[test]
fn a_still_image_shows_no_range_even_when_one_was_chosen() {
    let mut a = app(4);
    a.export_choice = ExportChoice {
        frames: FrameChoice::Range(1, 2),
        ..ExportChoice::default()
    };
    a.open_export_dialog(false);
    draw_whole_at(&mut a, 1280, 800);
    assert!(widget(&a, FROM_LESS).is_none());
    assert!(!has_range(a.export_dialog.expect("open")));
}

#[test]
fn in_the_smallest_window_a_click_on_the_panel_never_reaches_what_is_under_it() {
    crate::test_app::large_text();
    let (w, h) = (640, 600);
    let mut a = app(4);
    a.resized(w, h, 1);
    a.open_export_dialog(true);
    draw_whole_at(&mut a, w, h);
    let panel = a.export_panel_rect(a.export_dialog.expect("open"));
    let widgets = a.ui.widgets().to_vec();
    let ours = |x: i32, y: i32| {
        widgets
            .iter()
            .any(|(r, id)| (ID_EXPORT..ID_EXPORT + 100).contains(id) && r.contains(x, y))
    };
    let under: Vec<_> = widgets
        .iter()
        .filter(|(_, id)| !(ID_EXPORT..ID_EXPORT + 100).contains(id))
        .filter_map(|(r, id)| {
            let both = r.intersect(panel);
            let (x, y) = (both.x + both.w / 2, both.y + both.h / 2);
            (!both.is_empty() && !ours(x, y)).then_some((*id, x, y))
        })
        .collect();
    assert!(
        !under.is_empty(),
        "the panel reaches over other controls here"
    );
    let before = (
        a.tool,
        a.workspaces.clone(),
        a.editor.revision(),
        a.interface_size,
    );
    for (id, x, y) in under {
        a.pointer_down(x, y, Button::Left, PLAIN);
        draw_whole_at(&mut a, w, h);
        a.pointer_up(x, y, Button::Left);
        draw_whole_at(&mut a, w, h);
        let after = (
            a.tool,
            a.workspaces.clone(),
            a.editor.revision(),
            a.interface_size,
        );
        assert_eq!(after, before, "{id} under the panel was reached");
        assert!(a.menu.is_none() && a.keys_dialog.is_none(), "{id}");
        assert!(a.export_dialog.is_some());
    }
}

/// The default button is marked by a Text outline, notched like every face, as in every
/// dialog; Accent stays for what is chosen.
#[test]
fn the_default_export_button_is_outlined_not_amber() {
    let (w, h) = (1280, 800);
    let mut a = app(2);
    a.open_export_dialog(true);
    let mut px = vec![0u32; (w * h) as usize];
    for _ in 0..2 {
        let mut surface = pikslar_render::Surface { px: &mut px, w, h };
        a.draw_interface(
            &mut surface,
            pikslar_platform::Window::detached(),
            &mut Vec::new(),
        );
    }
    let r = widget(&a, ID_EXPORT + 50).expect("the Export button");
    let at = |x: i32, y: i32| px[(y * w + x) as usize];
    assert_ne!(at(r.x + r.w / 2, r.y + r.h / 2), theme::accent());
    assert_ne!(at(r.x + r.w / 2, r.y + r.h / 2), theme::accent_hover());
    assert_eq!(at(r.x + r.w / 2, r.y), theme::text(), "the top edge");
    assert_eq!(at(r.x, r.y + r.h / 2), theme::text(), "the left edge");
    assert_ne!(at(r.x, r.y), theme::text(), "a notched corner");
    let cancel = widget(&a, ID_EXPORT + 51).expect("the Cancel button");
    assert_ne!(at(cancel.x + cancel.w / 2, cancel.y), theme::text());
}

fn shown(a: &App, id: u32) -> bool {
    widget(a, id).is_some()
}

#[test]
fn every_row_is_named_and_every_choice_shows_both_of_its_options() {
    let size = (1280, 800);
    let mut a = app(3);
    a.open_export_dialog(false);
    pikslar_ui::font::log_text(true);
    draw_whole_at(&mut a, size.0, size.1);
    draw_whole_at(&mut a, size.0, size.1);
    let log = pikslar_ui::font::take_text_log();
    pikslar_ui::font::log_text(false);
    for name in ["Format", "Frames", "Layers", "Colour"] {
        assert!(
            log.texts.iter().any(|(_, t)| t == name),
            "{name} is a row name"
        );
    }
    // A still image: frame or all frames, visible or this layer, full colour or indexed.
    for id in [10, 11, 20, 21, 36, 30] {
        assert!(shown(&a, ID_EXPORT + id), "button {id} is shown");
    }
    a.open_export_dialog(true);
    draw_whole_at(&mut a, size.0, size.1);
    for id in [37, 31, 36, 30] {
        assert!(shown(&a, ID_EXPORT + id), "sheet button {id} is shown");
    }
}

#[test]
fn all_frames_on_a_still_image_asks_for_a_picture_each() {
    let size = (1280, 800);
    let mut a = app(3);
    a.open_export_dialog(false);
    press(&mut a, ID_EXPORT + 11, size);
    let c = a.export_dialog.expect("still open");
    assert_eq!((c.kind, c.frames), (Kind::Sequence, FrameChoice::All));
}

#[test]
fn full_colour_and_indexed_and_the_json_shapes_are_each_a_choice() {
    let size = (1280, 800);
    let mut a = app(3);
    a.open_export_dialog(true);
    press(&mut a, ID_EXPORT + 30, size);
    press(&mut a, ID_EXPORT + 31, size);
    let c = a.export_dialog.expect("open");
    assert!(c.indexed && c.hash);
    press(&mut a, ID_EXPORT + 36, size);
    press(&mut a, ID_EXPORT + 37, size);
    let c = a.export_dialog.expect("open");
    assert!(
        !c.indexed && !c.hash,
        "the other option of each pair turns it off"
    );
}

#[test]
fn this_frame_takes_a_still_image_back_from_the_frames_kind_it_was_sent_to() {
    let size = (1280, 800);
    let mut a = app(3);
    a.open_export_dialog(false);
    press(&mut a, ID_EXPORT + 11, size);
    assert_eq!(a.export_dialog.map(|c| c.kind), Some(Kind::Sequence));
    press(&mut a, ID_EXPORT + 10, size);
    let c = a.export_dialog.expect("open");
    assert_eq!((c.kind, c.frames), (Kind::Png, FrameChoice::This));
}

#[test]
fn a_frames_kind_chosen_on_purpose_stays_and_an_automatic_one_is_not_kept() {
    let size = (1280, 800);
    let mut a = app(3);
    a.open_export_dialog(false);
    // Chosen by the kind row: This frame only narrows the frames.
    a.export_dialog = a.export_dialog.map(|c| c.with_kind(Kind::Sequence));
    draw_whole_at(&mut a, size.0, size.1);
    press(&mut a, ID_EXPORT + 10, size);
    assert_eq!(a.export_dialog.map(|c| c.kind), Some(Kind::Sequence));
    // Sent there by All frames: the next export starts again from a PNG.
    let auto = ExportChoice::default().all_frames_of_still();
    assert_eq!(auto.settled().kind, Kind::Png);
    assert_eq!(auto.settled().frames, FrameChoice::This);
    let chosen = ExportChoice::default().with_kind(Kind::Sequence);
    assert_eq!(chosen.settled(), chosen);
}

/// Presses `key` on the focused control and draws the window, twice as the window does.
fn key_on(a: &mut App, key: pikslar_platform::Key, (w, h): (i32, i32)) {
    a.key_down(key, PLAIN, false, pikslar_platform::Window::detached());
    draw_whole_at(a, w, h);
    draw_whole_at(a, w, h);
}

#[test]
fn a_scale_button_turned_off_by_its_own_key_hands_focus_to_the_other_button() {
    use pikslar_platform::Key;
    let (minus, plus) = (ID_EXPORT + 22, ID_EXPORT + 23);
    let size = (1280, 800);
    let mut a = app(1);
    a.open_export_dialog(false);
    if let Some(c) = a.export_dialog.as_mut() {
        c.scale = 2;
    }
    draw_whole_at(&mut a, size.0, size.1);
    a.ui.focus.set(minus);
    key_on(&mut a, Key::Space, size);
    assert_eq!(a.export_dialog.map(|c| c.scale), Some(1));
    assert_eq!(
        a.ui.focus.focused(),
        Some(plus),
        "focus stays on the stepper"
    );
    if let Some(c) = a.export_dialog.as_mut() {
        c.scale = MOST_SCALE - 1;
    }
    draw_whole_at(&mut a, size.0, size.1);
    key_on(&mut a, Key::Space, size);
    assert_eq!(a.export_dialog.map(|c| c.scale), Some(MOST_SCALE));
    assert_eq!(a.ui.focus.focused(), Some(minus), "and back the other way");
}
