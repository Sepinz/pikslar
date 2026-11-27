//! The onion skin menu as an artist uses it: right click the onion button, choose.

use crate::app::App;
use crate::frame_menu::MenuKind;
use crate::test_app::{app, click, dot, draw, PLAIN};
use pikslar_platform::{Button, Handler, Window};
use pikslar_render::{OnionColors, OnionStyle, Surface};

fn right_click(a: &mut App, at: (i32, i32)) {
    a.pointer_down(at.0, at.1, Button::Right, PLAIN);
    draw(a);
    a.pointer_up(at.0, at.1, Button::Right);
    draw(a);
}

fn open(a: &mut App) {
    let r = a.onion_button;
    right_click(a, (r.x + r.w / 2, r.y + r.h / 2));
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Onion));
}

/// Chooses item `n` of the open menu by clicking it.
fn choose(a: &mut App, n: i32) {
    let open = a.menu.expect("a menu is open");
    let r = a.menu_area(open);
    let row = r.h / a.onion_items().len() as i32;
    click(a, (r.x + r.w / 2, r.y + n * row + row / 2), PLAIN);
    assert_eq!(a.menu, None, "choosing closes the menu");
}

fn labels(a: &App) -> Vec<&'static str> {
    a.onion_items().iter().map(|i| i.label).collect()
}

#[test]
fn a_right_click_on_the_onion_button_opens_its_menu_and_changes_nothing() {
    let mut a = app(3);
    let (depth, revision) = (a.editor.history().undo_depth(), a.editor.revision());
    a.playing = true;
    open(&mut a);
    assert!(
        !a.playing,
        "playback stops, so the skin's changes can be seen"
    );
    assert_eq!(
        labels(&a),
        [
            "Hide earlier frames",
            "Hide later frames",
            "Stronger",
            "Fainter",
            "Colors: red and blue",
            "Depth: 1 frame",
        ]
    );
    assert!(!a.comp.onion, "opening the menu does not turn the skin on");
    assert_eq!(a.editor.history().undo_depth(), depth);
    assert_eq!(a.editor.revision(), revision);
}

#[test]
fn hiding_a_side_turns_the_skin_on_and_the_last_side_cannot_be_hidden() {
    let mut a = app(3);
    open(&mut a);
    choose(&mut a, 0);
    assert!(a.comp.onion);
    assert_eq!(
        a.comp.onion_style,
        OnionStyle {
            past: false,
            ..OnionStyle::default()
        }
    );
    open(&mut a);
    assert_eq!(labels(&a)[0], "Show earlier frames");
    assert!(
        !a.onion_items()[1].enabled,
        "later frames are all that is left"
    );
    choose_disabled(&mut a, 1);
    assert!(a.comp.onion_style.future);
}

/// Clicks a disabled item, which leaves the menu open, then closes it.
fn choose_disabled(a: &mut App, n: i32) {
    let open = a.menu.expect("a menu is open");
    let r = a.menu_area(open);
    let row = r.h / a.onion_items().len() as i32;
    click(a, (r.x + r.w / 2, r.y + n * row + row / 2), PLAIN);
    assert!(a.menu.is_some(), "a disabled item does nothing");
    a.close_menu();
}

#[test]
fn strength_colours_and_depth_go_through_their_choices() {
    let mut a = app(3);
    for _ in 0..2 {
        open(&mut a);
        choose(&mut a, 2);
    }
    assert_eq!(a.comp.onion_style.strength, 4);
    assert!(a.message.as_ref().is_some_and(|(t, _)| t.contains("200%")));
    open(&mut a);
    assert!(
        !a.onion_items()[2].enabled,
        "no stronger than the strongest"
    );
    choose(&mut a, 4);
    assert_eq!(a.comp.onion_style.colors, OnionColors::GreenPurple);
    open(&mut a);
    choose(&mut a, 5);
    assert_eq!(a.comp.onion_depth, 2);
    open(&mut a);
    assert_eq!(
        labels(&a)[4..],
        ["Colors: green and purple", "Depth: 2 frames"]
    );
}

/// The window's pixels, kept between draws as a real window keeps them, so a change the app
/// forgot to redraw shows as a stale pixel.
struct Screen(Vec<u32>);

impl Screen {
    fn new() -> Self {
        let (w, h) = crate::test_app::WINDOW;
        Screen(vec![0u32; (w * h) as usize])
    }

    /// The canvas pixel over document cell (x, y) after the app draws.
    fn at(&mut self, a: &mut App, x: i32, y: i32) -> u32 {
        let (w, h) = crate::test_app::WINDOW;
        let mut surface = Surface {
            px: &mut self.0,
            w,
            h,
        };
        a.ui_dirty = true;
        a.frame(&mut surface, Window::detached(), &mut Vec::new());
        let (sx, sy) = crate::test_app::screen(a, x, y);
        self.0[(sy * w + sx) as usize]
    }
}

#[test]
fn the_canvas_shows_the_chosen_style() {
    let mut a = app(3);
    a.set_frame(0);
    dot(&mut a, 4, 4);
    a.set_frame(2);
    dot(&mut a, 6, 6);
    a.set_frame(1);
    let mut screen = Screen::new();
    // The same spots with the skin off: the checkerboard differs from spot to spot.
    let (bare_early, bare_late) = (screen.at(&mut a, 4, 4), screen.at(&mut a, 6, 6));
    a.toggle_onion();
    let (earlier, later) = (screen.at(&mut a, 4, 4), screen.at(&mut a, 6, 6));
    assert!(
        earlier != bare_early && later != bare_late,
        "both neighbours show"
    );
    open(&mut a);
    choose(&mut a, 0);
    assert_eq!(
        screen.at(&mut a, 4, 4),
        bare_early,
        "the earlier frame is hidden"
    );
    assert_eq!(screen.at(&mut a, 6, 6), later, "the later one is not");
    open(&mut a);
    choose(&mut a, 3);
    let fainter = screen.at(&mut a, 6, 6);
    assert!(
        fainter != later && fainter != bare_late,
        "fainter, but still there"
    );
    open(&mut a);
    choose(&mut a, 4);
    let recoloured = screen.at(&mut a, 6, 6);
    assert!(
        recoloured != fainter && recoloured != bare_late,
        "in other colours"
    );
}
