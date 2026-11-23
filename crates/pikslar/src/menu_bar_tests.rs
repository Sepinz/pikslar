//! The menu bar through the window's own entry points: roles in the dump, Alt and F10, access
//! keys, arrows between menus, and F11.

use crate::app::App;
use crate::frame_menu::MenuKind;
use crate::menu_bar::MENUS;
use crate::panel_side::ID_FILE;
use crate::test_app::*;
use crate::workspace::model::Preset;
use pikslar_platform::{Key, Mods, Window};

const SIZE: (i32, i32) = (1366, 705);
const CTRL_ONLY: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};

fn at() -> App {
    let mut a = app(1);
    a.ui.record_names();
    a.resized(SIZE.0, SIZE.1, 1);
    redraw(&mut a);
    a
}

fn redraw(a: &mut App) {
    let (w, h) = a.size;
    draw_whole_at(a, w, h);
    draw_whole_at(a, w, h);
}

fn press(a: &mut App, k: Key, mods: Mods) {
    a.key_down(k, mods, false, Window::detached());
    redraw(a);
}

fn dump_line(a: &App, name: &str) -> Vec<String> {
    a.ui.layout()
        .unwrap_or_default()
        .lines()
        .map(|l| l.split('\t').map(str::to_string).collect::<Vec<_>>())
        .find(|f| f.get(5).is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("{name} is in the dump"))
}

fn open_kind(a: &App) -> Option<MenuKind> {
    a.menu.map(|m| m.kind)
}

#[test]
fn the_titles_are_menu_items_that_say_whether_their_menu_is_open() {
    let mut a = at();
    for name in ["File", "Edit", "View", "Window", "Scripts"] {
        let line = dump_line(&a, name);
        assert_eq!(line[6], "menu-item", "{name}");
        assert!(line[7].contains("collapsed"), "{name}: {}", line[7]);
    }
    press(&mut a, Key::Char(b'E'), ALT);
    assert!(dump_line(&a, "Edit")[7].contains("expanded"));
    assert!(dump_line(&a, "File")[7].contains("collapsed"));
}

#[test]
fn a_tap_of_alt_or_f10_takes_the_bar_and_a_second_gives_it_back() {
    let mut a = at();
    for k in [Key::AltTap, Key::F(10)] {
        press(&mut a, k, PLAIN);
        assert_eq!(a.ui.focus.focused(), Some(ID_FILE), "{k:?}");
        press(&mut a, k, PLAIN);
        assert_eq!(a.ui.focus.focused(), None, "{k:?} again");
    }
}

#[test]
fn arrows_walk_the_titles_and_down_opens_with_the_first_row_lit() {
    let mut a = at();
    press(&mut a, Key::F(10), PLAIN);
    press(&mut a, Key::Right, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(ID_FILE + 1));
    press(&mut a, Key::Left, PLAIN);
    press(&mut a, Key::Left, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(ID_FILE + 4), "wraps to the last");
    press(&mut a, Key::Home, PLAIN);
    press(&mut a, Key::Down, PLAIN);
    redraw(&mut a);
    assert_eq!(open_kind(&a), Some(MenuKind::File));
    assert_eq!(a.menu_cursor.map(|(_, n)| n), Some(0));
}

#[test]
fn enter_chooses_the_lit_row_and_escape_closes() {
    let mut a = at();
    press(&mut a, Key::Char(b'F'), ALT);
    assert_eq!(open_kind(&a), Some(MenuKind::File));
    press(&mut a, Key::Enter, PLAIN);
    assert!(a.menu.is_none());
    assert!(a.new_dialog.is_some(), "the first row is New sprite");
    assert_eq!(
        a.ui.focus.focused(),
        None,
        "the keyboard goes back to the canvas"
    );
    a.close_new_dialog();
    press(&mut a, Key::Char(b'F'), ALT);
    press(&mut a, Key::Escape, PLAIN);
    assert!(a.menu.is_none());
    assert_eq!(
        a.ui.focus.focused(),
        Some(ID_FILE),
        "focus rests on the title"
    );
}

#[test]
fn left_and_right_go_to_the_neighbouring_menu() {
    let mut a = at();
    press(&mut a, Key::Char(b'F'), ALT);
    press(&mut a, Key::Right, PLAIN);
    assert_eq!(open_kind(&a), Some(MenuKind::Edit));
    press(&mut a, Key::Left, PLAIN);
    press(&mut a, Key::Left, PLAIN);
    assert_eq!(open_kind(&a), Some(MenuKind::Scripts), "wraps");
    press(&mut a, Key::F(10), PLAIN);
    assert!(a.menu.is_none(), "F10 closes it");
}

#[test]
fn every_access_letter_opens_its_menu_and_alt_makes_no_tool_choice() {
    let mut a = at();
    for (letter, kind) in [
        (b'F', MenuKind::File),
        (b'E', MenuKind::Edit),
        (b'V', MenuKind::View),
        (b'W', MenuKind::Window),
        (b'S', MenuKind::Scripts),
    ] {
        press(&mut a, Key::Char(letter), ALT);
        assert_eq!(open_kind(&a), Some(kind));
    }
    press(&mut a, Key::Escape, PLAIN);
    let pencil = a.tool;
    press(&mut a, Key::Char(b'E'), PLAIN);
    assert_ne!(a.tool, pencil, "E alone is the eraser");
    let eraser = a.tool;
    press(&mut a, Key::Char(b'B'), ALT);
    assert_eq!(a.tool, eraser, "Alt B is no pencil");
}

#[test]
fn a_click_on_a_title_opens_its_menu_under_it() {
    let mut a = at();
    let r = a.window_ui.titles[1];
    let (x, y) = (r.x + 4, r.y + 4);
    a.ui.mouse = (x, y);
    a.pointer_down(x, y, pikslar_platform::Button::Left, PLAIN);
    redraw(&mut a);
    a.pointer_up(x, y, pikslar_platform::Button::Left);
    redraw(&mut a);
    let open = a.menu.expect("the Edit menu");
    assert_eq!(open.kind, MenuKind::Edit);
    assert_eq!(
        open.at,
        (r.x, a.layout.top.bottom()),
        "under the bar, clear of the ring"
    );
    assert_eq!(a.menu_cursor, None, "nothing lit for the mouse");
}

#[test]
fn f11_shows_the_canvas_alone_and_brings_the_docks_back() {
    let mut a = at();
    let before = a.layout.canvas;
    let stored = a.workspaces.active();
    press(&mut a, Key::F(11), PLAIN);
    assert!(a.window_ui.canvas_only);
    assert!(a.layout.canvas.w > before.w && a.layout.canvas.h > before.h);
    assert_eq!(a.workspaces.active(), stored, "nothing stored changed");
    press(&mut a, Key::F(11), PLAIN);
    assert!(!a.window_ui.canvas_only);
    assert_eq!(a.layout.canvas, before);
}

#[test]
fn a_preset_ends_canvas_only() {
    let mut a = at();
    press(&mut a, Key::F(11), PLAIN);
    press(&mut a, Key::Char(b'2'), CTRL_ONLY);
    assert!(!a.window_ui.canvas_only);
    assert_eq!(a.workspaces.current, Preset::Animate);
}

#[test]
fn a_rebound_function_key_runs_the_action() {
    let mut a = at();
    let nine = crate::shortcuts::Binding {
        letter: crate::shortcuts::FUNCTION_BASE + 9,
        ctrl: false,
        shift: false,
    };
    a.keymap
        .bind(crate::shortcuts::Action::CanvasOnly, Some(nine));
    press(&mut a, Key::F(11), PLAIN);
    assert!(!a.window_ui.canvas_only, "F11 no longer does it");
    press(&mut a, Key::F(9), PLAIN);
    assert!(a.window_ui.canvas_only);
}

#[test]
fn every_bar_menu_opens_inside_the_window_at_1x_and_2x_and_in_high_contrast() {
    for (w, h, scale, contrast) in [
        (640, 600, 1, false),
        (1366, 705, 1, false),
        (1920, 1003, 2, false),
        (1920, 1003, 2, true),
    ] {
        a_normal_look();
        let mut a = app(1);
        a.ui.record_names();
        if contrast {
            a.forced_look = Some(Some(pikslar_ui::theme::SystemColors::BLACK));
            a.follow_look();
        }
        a.resized(w, h, scale);
        draw_whole_at(&mut a, w, h);
        for letter in *b"FEVWS" {
            a.key_down(Key::Char(letter), ALT, false, Window::detached());
            draw_whole_at(&mut a, w, h);
            draw_whole_at(&mut a, w, h);
            let open = a.menu.expect("open");
            let r = a.menu_area(open);
            let inside = r.x >= 0 && r.y >= 0 && r.right() <= w && r.bottom() <= h;
            assert!(inside, "{}: {r:?} in {w}x{h}", letter as char);
            assert!(a.menu_cursor.is_some(), "the first row is lit");
        }
    }
    a_normal_look();
}

/// Puts the thread's colours back to normal, for the case a test left high contrast on.
fn a_normal_look() {
    let mut a = app(1);
    a.forced_look = Some(None);
    a.follow_look();
}

fn f_binding(n: u8) -> crate::shortcuts::Binding {
    crate::shortcuts::Binding {
        letter: crate::shortcuts::FUNCTION_BASE + n,
        ctrl: false,
        shift: false,
    }
}

#[test]
fn a_rebound_menu_bar_key_closes_an_open_bar_menu_and_f10_no_longer_does() {
    let mut a = at();
    a.keymap
        .bind(crate::shortcuts::Action::MenuBar, Some(f_binding(9)));
    press(&mut a, Key::Char(b'E'), ALT);
    press(&mut a, Key::F(10), PLAIN);
    assert_eq!(
        open_kind(&a),
        Some(MenuKind::Edit),
        "F10 is no longer the key"
    );
    press(&mut a, Key::F(9), PLAIN);
    assert!(a.menu.is_none());
}

#[test]
fn alt_or_the_menu_bar_key_over_a_context_menu_closes_it_and_enters_the_bar() {
    for k in [Key::AltTap, Key::F(10)] {
        let mut a = at();
        a.menu = Some(crate::frame_menu::OpenMenu {
            at: (300, 300),
            kind: MenuKind::Onion,
        });
        redraw(&mut a);
        press(&mut a, k, PLAIN);
        assert!(a.menu.is_none(), "{k:?}");
        assert_eq!(a.ui.focus.focused(), Some(ID_FILE), "{k:?} enters the bar");
    }
}

#[test]
fn alt_with_a_letter_over_a_context_menu_opens_that_bar_menu() {
    let mut a = at();
    a.menu = Some(crate::frame_menu::OpenMenu {
        at: (300, 300),
        kind: MenuKind::Onion,
    });
    redraw(&mut a);
    press(&mut a, Key::Char(b'W'), ALT);
    assert_eq!(open_kind(&a), Some(MenuKind::Window));
}

#[test]
fn left_and_right_in_a_plugin_menu_go_to_the_neighbours_of_scripts() {
    let mut a = at();
    let open = |a: &mut App| {
        a.menu = Some(crate::frame_menu::OpenMenu {
            at: (300, 100),
            kind: MenuKind::Plugin(0),
        });
        redraw(a);
    };
    open(&mut a);
    press(&mut a, Key::Left, PLAIN);
    assert_eq!(open_kind(&a), Some(MenuKind::Window));
    open(&mut a);
    press(&mut a, Key::Right, PLAIN);
    assert_eq!(open_kind(&a), Some(MenuKind::File), "wraps");
}

fn narrow(scale: i32) -> App {
    let mut a = app(1);
    a.ui.record_names();
    a.resized(640, 600, scale);
    draw_whole_at(&mut a, 640, 600);
    draw_whole_at(&mut a, 640, 600);
    a
}

#[test]
fn at_640_by_600_every_drawn_title_fits_the_bar_at_1x_2x_and_3x() {
    crate::test_app::large_text();
    for scale in [1, 2, 3] {
        let a = narrow(scale);
        let top = a.layout.top;
        let shown = MENUS - a.window_ui.hidden;
        assert!(shown >= 1, "{scale}x: File at least");
        let mut right = top.x;
        for r in &a.window_ui.titles[..shown] {
            assert!(
                r.x >= right && r.right() <= top.right(),
                "{scale}x: {r:?} in {top:?}"
            );
            right = r.right();
        }
        if a.window_ui.hidden > 0 {
            let line = dump_line(&a, "More menus");
            let (x, w): (i32, i32) = (line[1].parse().unwrap_or(0), line[3].parse().unwrap_or(0));
            assert!(
                x >= right && x + w <= top.right(),
                "{scale}x: overflow at {x}+{w}"
            );
        } else {
            assert_eq!(scale, 1, "only 1x has room for all five");
        }
    }
}

#[test]
fn a_title_that_does_not_fit_opens_under_the_overflow_button_inside_the_window() {
    let mut a = narrow(3);
    assert!(a.window_ui.hidden > 0);
    press(&mut a, Key::Char(b'S'), ALT);
    assert_eq!(open_kind(&a), Some(MenuKind::Scripts));
    let r = a.menu_area(a.menu.expect("open"));
    assert!(r.x >= 0 && r.right() <= 640 && r.bottom() <= 600, "{r:?}");
    assert_eq!(
        a.ui.focus.focused(),
        Some(ID_FILE + MENUS as u32),
        "focus is on the button"
    );
}

#[test]
fn the_overflow_menu_lists_the_hidden_titles_and_opens_the_one_chosen() {
    let mut a = narrow(3);
    let hidden = a.window_ui.hidden;
    let rows = a.menu_entries(MenuKind::More);
    assert_eq!(rows.len(), hidden);
    assert_eq!(
        rows.last().map(|(l, _)| l.to_string()),
        Some("Scripts".to_string())
    );
    press(&mut a, Key::F(10), PLAIN);
    for _ in 0..MENUS - hidden {
        press(&mut a, Key::Right, PLAIN);
    }
    assert_eq!(
        a.ui.focus.focused(),
        Some(ID_FILE + MENUS as u32),
        "Right reaches the button"
    );
    press(&mut a, Key::Down, PLAIN);
    assert_eq!(open_kind(&a), Some(MenuKind::More));
    press(&mut a, Key::End, PLAIN);
    press(&mut a, Key::Enter, PLAIN);
    assert_eq!(open_kind(&a), Some(MenuKind::Scripts));
    press(&mut a, Key::Escape, PLAIN);
    press(&mut a, Key::Right, PLAIN);
    assert_eq!(
        a.ui.focus.focused(),
        Some(ID_FILE),
        "and Right from it wraps to File"
    );
}

#[test]
fn the_menu_bar_is_a_header_band() {
    let (w, h) = (1366, 705);
    let mut a = crate::test_app::app(1);
    a.resized(w, h, 1);
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = pikslar_render::Surface { px: &mut px, w, h };
    a.draw_interface(
        &mut surface,
        pikslar_platform::Window::detached(),
        &mut Vec::new(),
    );
    let top = a.layout.top;
    assert_eq!(
        px[((top.y + 2) * w + top.w / 2) as usize],
        pikslar_ui::theme::header()
    );
    let context = a.layout.context;
    assert_eq!(
        px[((context.y + 2) * w + context.w / 2) as usize],
        pikslar_ui::theme::panel()
    );
}
