use super::*;
use crate::app::ToolChoice;
use crate::test_app::*;
use pikslar_core::Tool;
use pikslar_platform::Window;

const PLAIN_MODS: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
};
const CTRL_MODS: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};

fn press(a: &mut App, key: Key, mods: Mods) {
    a.key_down(key, mods, false, Window::detached());
}

fn wait_for(a: &mut App, action: Action) {
    a.keys_dialog = Some(KeysDialog {
        page: 0,
        waiting: Some(action),
    });
}

#[test]
fn the_view_menu_opens_the_dialog() {
    let mut a = app(1);
    a.open_view_menu((10, 30));
    let at = a
        .view_items()
        .iter()
        .position(|i| i.label.starts_with("Keyboard shortcuts"));
    a.view_menu_action(at.expect("a row for the shortcuts"), Window::detached());
    assert_eq!(a.keys_dialog, Some(KeysDialog::default()));
    assert_eq!(a.menu, None);
}

#[test]
fn a_new_key_works_at_once_and_the_old_one_stops() {
    let mut a = app(1);
    a.open_keys_dialog();
    wait_for(&mut a, Action::Fill);
    press(&mut a, Key::Char(b'F'), PLAIN_MODS);
    assert_eq!(a.keys_dialog.and_then(|d| d.waiting), None);
    a.close_keys_dialog();
    press(&mut a, Key::Char(b'F'), PLAIN_MODS);
    assert_eq!(a.tool, ToolChoice::Paint(Tool::Fill));
    press(&mut a, Key::Char(b'B'), PLAIN_MODS);
    press(&mut a, Key::Char(b'G'), PLAIN_MODS);
    assert_eq!(
        a.tool,
        ToolChoice::Paint(Tool::Pencil),
        "G no longer picks Fill"
    );
}

#[test]
fn taking_a_key_in_use_says_which_action_lost_it() {
    let mut a = app(1);
    a.open_keys_dialog();
    wait_for(&mut a, Action::Fill);
    press(&mut a, Key::Char(b'S'), CTRL_MODS);
    assert_eq!(a.keymap.binding(Action::Save), None);
    let said = a.message.as_ref().map(|(m, _)| m.as_str());
    assert_eq!(said, Some("Save no longer has Ctrl S"));
}

#[test]
fn keys_pressed_in_the_dialog_do_not_reach_the_sprite() {
    let mut a = app(1);
    a.open_keys_dialog();
    let frames = a.editor.doc().frames.len();
    press(&mut a, Key::Char(b'N'), PLAIN_MODS);
    press(&mut a, Key::Char(b'E'), PLAIN_MODS);
    assert_eq!(a.editor.doc().frames.len(), frames);
    assert_eq!(a.tool, ToolChoice::Paint(Tool::Pencil));
    press(&mut a, Key::Escape, PLAIN_MODS);
    assert_eq!(a.keys_dialog, None, "Escape closes it when nothing waits");
}

#[test]
fn escape_keeps_the_old_key_and_delete_leaves_none() {
    let mut a = app(1);
    a.open_keys_dialog();
    wait_for(&mut a, Action::Line);
    press(&mut a, Key::Escape, PLAIN_MODS);
    assert!(a.keys_dialog.is_some(), "Escape only stopped the wait");
    assert_eq!(
        a.keymap.binding(Action::Line),
        Keymap::default().binding(Action::Line)
    );
    wait_for(&mut a, Action::Line);
    press(&mut a, Key::Delete, PLAIN_MODS);
    assert_eq!(a.keymap.binding(Action::Line), None);
    wait_for(&mut a, Action::Line);
    press(&mut a, Key::Left, PLAIN_MODS);
    assert!(a.message.is_some(), "an arrow cannot be a shortcut");
    assert_eq!(a.keys_dialog.and_then(|d| d.waiting), Some(Action::Line));
}

#[test]
fn the_dialog_is_drawn_whole_from_the_smallest_window_to_3x() {
    for (w, h, scale) in [(624, 561, 1), (1920, 1003, 2), (2560, 1440, 3)] {
        let mut a = App::launch(None, Some(32), None);
        a.resized(w, h, scale);
        a.open_keys_dialog();
        for (page, &(_, first, end)) in PAGES.iter().enumerate() {
            a.keys_dialog = Some(KeysDialog {
                page,
                waiting: None,
            });
            draw_whole_at(&mut a, w, h);
            let widgets = a.ui.widgets().to_vec();
            let ours: Vec<_> = widgets
                .iter()
                .filter(|(_, id)| (ID_KEYS..ID_KEYS + 100).contains(id))
                .collect();
            assert_eq!(
                ours.len(),
                PAGES.len() + 2 + end - first,
                "{w}x{h} page {page}"
            );
            for (r, id) in ours {
                let inside = r.x >= 0 && r.y >= 0 && r.right() <= w && r.bottom() <= h;
                assert!(inside, "{w}x{h}: {id} at {r:?}");
            }
        }
    }
}

#[test]
fn a_held_shortcut_acts_once_and_its_repeats_do_nothing() {
    let mut a = app(1);
    let frames = a.editor.doc().frames.len();
    a.key_down(Key::Char(b'N'), PLAIN_MODS, false, Window::detached());
    for _ in 0..5 {
        a.key_down(Key::Char(b'N'), PLAIN_MODS, true, Window::detached());
    }
    assert_eq!(a.editor.doc().frames.len(), frames + 1);
}

#[test]
fn every_action_has_a_label_on_screen() {
    for (action, ..) in ACTIONS {
        assert!(!action_label(action).is_empty());
        assert!(
            action_label(action).len() <= 16,
            "{action:?} fits its column"
        );
    }
}

#[test]
fn every_page_tab_sits_inside_the_dialog() {
    for (w, h, scale) in [(624, 561, 1), (1920, 1003, 2)] {
        let mut a = App::launch(None, Some(32), None);
        a.resized(w, h, scale);
        a.open_keys_dialog();
        draw_whole_at(&mut a, w, h);
        let panel_right = (w + WIDTH * scale) / 2;
        let tabs: Vec<_> =
            a.ui.widgets()
                .iter()
                .filter(|(_, id)| (ID_KEYS..ID_KEYS + PAGES.len() as u32).contains(id))
                .map(|(r, _)| *r)
                .collect();
        assert_eq!(tabs.len(), PAGES.len());
        let last = tabs.iter().map(|r| r.right()).max().unwrap_or(0);
        assert!(
            last <= panel_right - 8 * scale,
            "{w}: {last} > {panel_right}"
        );
    }
}

const SHIFT_MODS: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: false,
};

/// A key, then a whole draw, as the window does between messages.
fn tap(a: &mut App, key: Key, mods: Mods) {
    press(a, key, mods);
    draw_whole(a);
}

fn open_drawn() -> App {
    let mut a = app(1);
    a.open_keys_dialog();
    draw_whole(&mut a);
    a
}

#[test]
fn tab_walks_the_page_tabs_then_the_rows_then_the_buttons() {
    let mut a = open_drawn();
    let mut seen = Vec::new();
    for _ in 0..(PAGES.len() + 12 + 2) {
        tap(&mut a, Key::Tab, PLAIN_MODS);
        seen.push(a.ui.focus.focused().expect("focused"));
    }
    let tabs: Vec<u32> = (0..PAGES.len() as u32).map(|n| ID_KEYS + n).collect();
    assert_eq!(&seen[..PAGES.len()], &tabs[..]);
    assert_eq!(seen[PAGES.len()], ID_KEYS + 10, "the first row");
    assert_eq!(seen[PAGES.len() + 11], ID_KEYS + 21, "the twelfth row");
    assert_eq!(seen[PAGES.len() + 12], ID_KEYS + 5, "Usual keys");
    assert_eq!(seen[PAGES.len() + 13], ID_KEYS + 6, "Close");
    tap(&mut a, Key::Tab, SHIFT_MODS);
    assert_eq!(
        a.ui.focus.focused(),
        Some(ID_KEYS + 5),
        "Shift Tab goes back"
    );
}

#[test]
fn a_row_is_chosen_and_rebound_from_the_keyboard_alone() {
    let mut a = open_drawn();
    for _ in 0..=PAGES.len() {
        tap(&mut a, Key::Tab, PLAIN_MODS);
    }
    tap(&mut a, Key::Enter, PLAIN_MODS);
    assert_eq!(a.keys_dialog.and_then(|d| d.waiting), Some(Action::Pencil));
    tap(&mut a, Key::Char(b'P'), PLAIN_MODS);
    assert_eq!(a.keys_dialog.and_then(|d| d.waiting), None);
    assert_eq!(
        a.keymap.binding(Action::Pencil),
        Some(Binding {
            letter: b'P',
            ctrl: false,
            shift: false
        })
    );
    assert!(a.keys_dialog.is_some(), "still open");
}

#[test]
fn arrows_move_between_rows_and_between_tabs_and_space_acts() {
    let mut a = open_drawn();
    tap(&mut a, Key::Tab, PLAIN_MODS);
    tap(&mut a, Key::Right, PLAIN_MODS);
    assert_eq!(a.ui.focus.focused(), Some(ID_KEYS + 1));
    tap(&mut a, Key::Space, PLAIN_MODS);
    assert_eq!(a.keys_dialog.map(|d| d.page), Some(1), "the Text tab");
    tap(&mut a, Key::Left, PLAIN_MODS);
    assert_eq!(a.ui.focus.focused(), Some(ID_KEYS));
    // Down from a tab goes to the first row, then on down the rows.
    tap(&mut a, Key::Down, PLAIN_MODS);
    assert_eq!(a.ui.focus.focused(), Some(ID_KEYS + 10 + PAGES[1].1 as u32));
    tap(&mut a, Key::Down, PLAIN_MODS);
    assert_eq!(a.ui.focus.focused(), Some(ID_KEYS + 11 + PAGES[1].1 as u32));
    tap(&mut a, Key::Up, PLAIN_MODS);
    tap(&mut a, Key::Up, PLAIN_MODS);
    assert_eq!(
        a.ui.focus.focused(),
        Some(ID_KEYS + 1),
        "back up to the tab"
    );
}

#[test]
fn escape_still_cancels_a_wait_then_closes() {
    let mut a = open_drawn();
    for _ in 0..=PAGES.len() {
        tap(&mut a, Key::Tab, PLAIN_MODS);
    }
    tap(&mut a, Key::Enter, PLAIN_MODS);
    tap(&mut a, Key::Escape, PLAIN_MODS);
    assert_eq!(a.keys_dialog.and_then(|d| d.waiting), None);
    assert!(a.keys_dialog.is_some());
    tap(&mut a, Key::Escape, PLAIN_MODS);
    assert!(a.keys_dialog.is_none());
}

#[test]
fn enter_on_close_closes_and_the_focused_row_is_outlined() {
    let mut a = open_drawn();
    for _ in 0..=PAGES.len() {
        tap(&mut a, Key::Tab, PLAIN_MODS);
    }
    let row =
        a.ui.widgets()
            .iter()
            .find(|(_, id)| *id == ID_KEYS + 10)
            .map(|(r, _)| *r)
            .expect("row");
    let (w, h) = WINDOW;
    let mut px = vec![0u32; (w * h) as usize];
    a.draw_interface(
        &mut pikslar_render::Surface { px: &mut px, w, h },
        Window::detached(),
        &mut Vec::new(),
    );
    let at = |x: i32, y: i32| px[(y * w + x) as usize];
    assert_eq!(at(row.x - 2, row.y + row.h / 2), pikslar_ui::theme::focus());
    assert_eq!(
        at(row.x + row.w + 1, row.y + row.h / 2),
        pikslar_ui::theme::focus()
    );
    for _ in 0..2 {
        tap(&mut a, Key::Tab, PLAIN_MODS);
    }
    let mut a2 = open_drawn();
    tap(&mut a2, Key::Tab, SHIFT_MODS);
    assert_eq!(a2.ui.focus.focused(), Some(ID_KEYS + 6), "Close");
    tap(&mut a2, Key::Enter, PLAIN_MODS);
    assert!(a2.keys_dialog.is_none());
}

#[test]
fn a_dialog_opens_with_nothing_focused_and_a_click_drops_focus() {
    let mut a = open_drawn();
    tap(&mut a, Key::Tab, PLAIN_MODS);
    assert!(a.ui.focus.focused().is_some());
    a.close_keys_dialog();
    a.open_keys_dialog();
    assert_eq!(a.ui.focus.focused(), None);
    tap(&mut a, Key::Tab, PLAIN_MODS);
    a.pointer_down(5, 5, pikslar_platform::Button::Left, PLAIN_MODS);
    assert_eq!(a.ui.focus.focused(), None);
}

#[test]
fn reserved_keys_and_alt_chords_are_refused_in_the_dialog_with_a_reason() {
    let shift = Mods {
        shift: true,
        ..PLAIN_MODS
    };
    let alt = Mods {
        alt: true,
        ..PLAIN_MODS
    };
    for (key, mods) in [
        (Key::F(10), shift),
        (Key::F(6), PLAIN_MODS),
        (Key::F(6), shift),
        (Key::Char(b'B'), alt),
    ] {
        let mut a = app(1);
        a.open_keys_dialog();
        let before = a.keymap.clone();
        wait_for(&mut a, Action::CanvasOnly);
        press(&mut a, key, mods);
        assert_eq!(a.keymap, before, "{key:?} {mods:?}");
        assert!(a.message.is_some(), "it says why");
        assert_eq!(
            a.keys_dialog.and_then(|d| d.waiting),
            Some(Action::CanvasOnly),
            "and keeps waiting"
        );
    }
    let mut a = app(1);
    a.open_keys_dialog();
    wait_for(&mut a, Action::CanvasOnly);
    let ctrl = Mods {
        ctrl: true,
        ..PLAIN_MODS
    };
    press(&mut a, Key::F(6), ctrl);
    assert!(a.keymap.binding(Action::CanvasOnly).is_some_and(|b| b.ctrl));
}

#[test]
fn the_footer_shows_the_version_beside_the_buttons_in_muted_text() {
    for (w, h, scale) in [(624, 561, 1), (1920, 1003, 2)] {
        let mut a = App::launch(None, Some(32), None);
        a.resized(w, h, scale);
        a.open_keys_dialog();
        draw_whole_at(&mut a, w, h);
        let close =
            a.ui.widgets()
                .iter()
                .find(|(_, id)| *id == ID_KEYS + 6)
                .map(|(r, _)| *r)
                .expect("Close");
        let mut px = vec![0u32; (w * h) as usize];
        a.draw_interface(
            &mut pikslar_render::Surface { px: &mut px, w, h },
            Window::detached(),
            &mut Vec::new(),
        );
        let muted = (close.right() + 4..w)
            .flat_map(|x| (close.y..close.bottom()).map(move |y| (x, y)))
            .filter(|&(x, y)| px[(y * w + x) as usize] == pikslar_ui::theme::muted())
            .count();
        assert!(muted > 20, "{w}x{h}: the version is drawn right of Close");
    }
}
