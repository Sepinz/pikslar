//! A popup menu: a short l.map(|i| row_width(i.label, ts, scale))st of commands drawn above the panels.
//!
//! The caller says where the menu will be with [`Ui::cover`] before drawing anything under it,
//! so the panels below ignore the pointer, then draws it last with [`menu`].

use crate::font::{cap_height, draw_text, text_width};
use crate::text_fit::fit_to_width;
use crate::theme;
use crate::widgets::Ui;
use pikslar_render::{Rect, Surface};

/// The layer menus are drawn on.
pub const MENU_LEVEL: u8 = 1;
const ROW: i32 = 24;
const PAD: i32 = 10;
/// The space, in interface units, between a menu's left edge and what a row shows first.
pub const MENU_PAD: i32 = PAD;
const MIN_W: i32 = 120;

/// The key a row shows at its right edge is written after a tab in its label, such as
/// "Save", a tab, "Ctrl S". The tab and the key are not part of the row's name.
pub fn split_hint(label: &str) -> (&str, &str) {
    label.split_once('\t').unwrap_or((label, ""))
}

/// The units between a row's words and its key.
const HINT_GAP: i32 = 16;

/// What a row needs in width, in pixels, without its padding.
fn row_width(label: &str, ts: i32, scale: i32) -> i32 {
    let (name, hint) = split_hint(label);
    let gap = if hint.is_empty() { 0 } else { HINT_GAP * scale };
    text_width(name, ts) + gap + text_width(hint, ts)
}

/// One line of a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItem<'a> {
    pub label: &'a str,
    pub enabled: bool,
}

/// What happened to a menu this pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuResult {
    Open,
    Chosen(usize),
    /// The pointer went down outside it.
    Dismissed,
}

/// Where a menu opened at `at` is drawn: below and right of the point, moved back inside
/// `bounds` when it would leave them.
pub fn menu_rect(scale: i32, at: (i32, i32), items: &[MenuItem<'_>], bounds: Rect) -> Rect {
    menu_rect_with_room(scale, at, items, bounds, 0)
}

/// [`menu_rect`] for a menu whose labels start `room` interface units further right, to leave
/// a place for a picture the caller draws in each row (see [`menu_with_room`]).
pub fn menu_rect_with_room(
    scale: i32,
    at: (i32, i32),
    items: &[MenuItem<'_>],
    bounds: Rect,
    room: i32,
) -> Rect {
    let ts = crate::font::text_scale(scale.max(1));
    let widest = items
        .iter()
        .map(|i| row_width(i.label, ts, scale))
        .max()
        .unwrap_or(0);
    let w = (widest + (2 * PAD + room) * scale)
        .max(MIN_W * scale)
        .min(bounds.w);
    // Past the window's height the rows page: only the whole rows that fit are drawn.
    let h = (items.len() as i32 * ROW * scale).min(bounds.h);
    let x = if at.0 + w > bounds.right() {
        at.0 - w
    } else {
        at.0
    };
    let y = if at.1 + h > bounds.bottom() {
        at.1 - h
    } else {
        at.1
    };
    let x = x.clamp(bounds.x, bounds.right() - w);
    let y = y.clamp(bounds.y, bounds.bottom() - h);
    Rect::new(x, y, w, h)
}

/// Draws the menu in `rect`, from [`menu_rect`], with item `n` given widget id `id + n`.
pub fn menu(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    rect: Rect,
    items: &[MenuItem<'_>],
) -> MenuResult {
    menu_with_room(ui, sf, id, rect, items, 0)
}

/// Row `n` of a menu drawn in `rect`.
pub fn menu_row(rect: Rect, n: usize, scale: i32) -> Rect {
    Rect::new(rect.x, rect.y + n as i32 * ROW * scale, rect.w, ROW * scale)
}

/// [`menu`] with each label `room` interface units further right. The caller draws what goes
/// in that room afterwards, from [`menu_row`] and [`MENU_PAD`].
pub fn menu_with_room(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    rect: Rect,
    items: &[MenuItem<'_>],
    room: i32,
) -> MenuResult {
    menu_grouped(ui, sf, (id, rect), items, room, &[])
}

/// [`menu_with_room`] with the items in groups: each row in `groups` starts one, and a
/// one-unit rule inside the top of that row parts it from the row above. The rows keep their
/// height, so every row stays where [`menu_row`] says.
pub fn menu_grouped(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    (id, rect): (u32, Rect),
    items: &[MenuItem<'_>],
    room: i32,
    groups: &[usize],
) -> MenuResult {
    let back = ui.enter_level(MENU_LEVEL);
    let s = ui.scale;
    crate::chrome::popup(sf, rect, s);
    let mut result = MenuResult::Open;
    let visible = (rect.h / (ROW * s)).max(1) as usize;
    let first = shown_from(ui, items.len(), visible);
    let text_room = rect.w - (2 * PAD + room) * s;
    for (n, item) in items.iter().enumerate().skip(first).take(visible) {
        let r = menu_row(rect, n - first, s);
        if draw_row(ui, sf, (id + n as u32, n), r, item, (room, text_room)) {
            result = MenuResult::Chosen(n);
        }
    }
    // Drawn after the rows, so a row's hover face never covers the rule above it.
    for &n in groups.iter().filter(|&&n| n > first && n < first + visible) {
        let r = menu_row(rect, n - first, s);
        sf.fill(
            Rect::new(r.x + PAD * s, r.y, r.w - 2 * PAD * s, s),
            theme::rule(),
        );
    }
    if items.len() > visible {
        draw_scroll_mark(sf, rect, s, (first, visible, items.len()));
    }
    let (x, y) = ui.mouse;
    if ui.pressed && !rect.contains(x, y) {
        result = MenuResult::Dismissed;
    }
    ui.enter_level(back);
    result
}

/// One row of a menu: its feedback, its label cut to `text_room`, and whether it was clicked.
fn draw_row(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    (id, n): (u32, usize),
    r: Rect,
    item: &MenuItem<'_>,
    (room, text_room): (i32, i32),
) -> bool {
    let (s, ts) = (ui.scale, ui.text_scale());
    let state = ui.interact(id, r);
    ui.describe(
        crate::role::Role::MenuItem,
        crate::role::State {
            disabled: !item.enabled,
            ..crate::role::State::NONE
        },
    );
    let (words, hint) = split_hint(item.label);
    ui.name(id, words);
    if item.enabled {
        paint_row_feedback(sf, r, s, state, ui.menu_highlight == Some(n));
    }
    let ink = if item.enabled {
        theme::text()
    } else {
        theme::disabled()
    };
    let at = (r.x + (PAD + room) * s, r.y + (r.h - cap_height() * ts) / 2);
    let hint_w = if hint.is_empty() {
        0
    } else {
        text_width(hint, ts) + HINT_GAP * s
    };
    let shown = fit_to_width(words, text_room - hint_w, ts);
    draw_text(sf, at.0, at.1, &shown, ink, ts);
    if !hint.is_empty() {
        let x = r.right() - PAD * s - text_width(hint, ts);
        // A row that cannot be chosen dims its key too.
        let key_ink = if item.enabled {
            theme::muted()
        } else {
            theme::disabled()
        };
        draw_text(sf, x, at.1, hint, key_ink, ts);
    }
    item.enabled && state.clicked
}

/// A choosable row's feedback: the pointer over it or holding it, and the keyboard's highlight.
fn paint_row_feedback(
    sf: &mut Surface<'_>,
    r: Rect,
    s: i32,
    state: crate::widgets::Interaction,
    highlighted: bool,
) {
    if state.hover {
        let tone = if state.held {
            theme::pressed()
        } else {
            theme::hover()
        };
        sf.fill(r, tone);
    }
    if highlighted {
        sf.fill(r, theme::hover());
        sf.fill(Rect::new(r.x, r.y, 2 * s, r.h), theme::accent());
    }
}

/// The first row shown: where the pointer or wheel left the list, kept in range and moved so
/// that the highlighted row is in view. Remembered in `ui.menu_first`.
fn shown_from(ui: &mut Ui, count: usize, visible: usize) -> usize {
    let mut first = ui.menu_first.min(count.saturating_sub(visible));
    if let Some(h) = ui.menu_highlight {
        if h < first {
            first = h;
        } else if h >= first + visible {
            first = h + 1 - visible;
        }
    }
    ui.menu_first = first;
    first
}

/// A thin bar down the menu's right edge showing which rows are in view.
fn draw_scroll_mark(
    sf: &mut Surface<'_>,
    rect: Rect,
    s: i32,
    (first, shown, count): (usize, usize, usize),
) {
    let (top, h) = (
        rect.y + (rect.h as i64 * first as i64 / count as i64) as i32,
        (rect.h as i64 * shown as i64 / count as i64) as i32,
    );
    sf.fill(
        Rect::new(rect.right() - 2 * s, top, 2 * s, h.max(s)),
        theme::muted(),
    );
}

/// A key a menu answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    Home,
    End,
    Enter,
    /// A letter or digit, which jumps to a row starting with it.
    Letter(char),
}

/// What a key does to a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuNav {
    /// Move the highlight to this row.
    Highlight(usize),
    /// Choose this row.
    Chosen(usize),
    None,
}

/// What `key` does when row `current` is highlighted. Disabled rows are skipped, and Up and
/// Down wrap around.
pub fn menu_key(items: &[MenuItem<'_>], current: Option<usize>, key: MenuKey) -> MenuNav {
    let count = items.len();
    let enabled = |n: usize| items.get(n).is_some_and(|i| i.enabled);
    // The first enabled row met walking `step` rows at a time from `from`, wrapping.
    let walk = |from: usize, forward: bool, accept: &dyn Fn(usize) -> bool| {
        (1..=count)
            .map(|d| {
                if forward {
                    (from + d) % count
                } else {
                    (from + count - d % count) % count
                }
            })
            .find(|&n| enabled(n) && accept(n))
    };
    let found = |n: Option<usize>| n.map_or(MenuNav::None, MenuNav::Highlight);
    if count == 0 {
        return MenuNav::None;
    }
    match key {
        MenuKey::Down => found(walk(current.unwrap_or(count - 1), true, &|_| true)),
        MenuKey::Up => found(walk(current.unwrap_or(0), false, &|_| true)),
        MenuKey::Home => found(walk(count - 1, true, &|_| true)),
        MenuKey::End => found(walk(0, false, &|_| true)),
        MenuKey::Enter => current
            .filter(|&n| enabled(n))
            .map_or(MenuNav::None, MenuNav::Chosen),
        MenuKey::Letter(c) => found(walk(current.unwrap_or(count - 1), true, &|n| {
            first_letter(items[n].label).is_some_and(|f| f.eq_ignore_ascii_case(&c))
        })),
    }
}

fn first_letter(label: &str) -> Option<char> {
    label.chars().find(|c| c.is_alphanumeric())
}

#[cfg(test)]
#[path = "menu_long_tests.rs"]
mod long_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Interaction;

    const ITEMS: [MenuItem<'static>; 3] = [
        MenuItem {
            label: "New frame",
            enabled: true,
        },
        MenuItem {
            label: "Duplicate frame",
            enabled: true,
        },
        MenuItem {
            label: "Delete frame",
            enabled: false,
        },
    ];
    const SCREEN: Rect = Rect {
        x: 0,
        y: 0,
        w: 400,
        h: 300,
    };

    fn ui_at(x: i32, y: i32) -> Ui {
        let mut ui = Ui::default();
        ui.scale = 1;
        ui.mouse = (x, y);
        ui
    }

    /// One pass: a panel button under the menu, then the menu over it.
    fn pass(ui: &mut Ui, sf: &mut Surface<'_>, rect: Rect) -> (Interaction, MenuResult) {
        ui.begin();
        ui.cover(SCREEN, MENU_LEVEL);
        let under = ui.interact(5, Rect::new(0, 0, 400, 300));
        let result = menu(ui, sf, 100, rect, &ITEMS);
        ui.end();
        (under, result)
    }

    fn surface(px: &mut Vec<u32>) -> Surface<'_> {
        px.resize(400 * 300, 0);
        Surface { px, w: 400, h: 300 }
    }

    #[test]
    fn a_menu_opens_beside_the_point_and_stays_on_screen() {
        let r = menu_rect(1, (10, 20), &ITEMS, SCREEN);
        assert_eq!((r.x, r.y, r.h), (10, 20, 72));
        assert!(r.w >= MIN_W);
        let near = menu_rect(1, (390, 290), &ITEMS, SCREEN);
        assert_eq!((near.right(), near.bottom()), (390, 290));
        let tiny = Rect::new(0, 0, 50, 30);
        let squeezed = menu_rect(1, (40, 25), &ITEMS, tiny);
        assert_eq!(squeezed, tiny);
        assert_eq!(menu_rect(2, (0, 0), &ITEMS, SCREEN).h, 144);
    }

    #[test]
    fn the_panel_under_a_menu_never_takes_the_pointer() {
        let mut px = Vec::new();
        let mut sf = surface(&mut px);
        let rect = menu_rect(1, (10, 10), &ITEMS, SCREEN);
        for (x, y) in [(15, 15), (300, 250)] {
            let mut ui = ui_at(x, y);
            ui.pressed = true;
            ui.down = true;
            let (under, _) = pass(&mut ui, &mut sf, rect);
            assert!(!under.hover && !under.started, "at {x},{y}");
            assert_ne!(ui.active, 5);
        }
        let mut ui = ui_at(300, 250);
        pass(&mut ui, &mut sf, rect);
        assert_eq!(ui.widget_at(300, 250), 0, "nothing on the panel answers");
        assert_eq!(ui.widget_at(15, 15), 100);
    }

    #[test]
    fn without_a_cover_the_panel_answers_as_before() {
        let mut ui = ui_at(15, 15);
        ui.begin();
        assert!(ui.interact(5, Rect::new(0, 0, 400, 300)).hover);
        assert_eq!(ui.widget_at(15, 15), 5);
    }

    #[test]
    fn a_click_on_an_item_chooses_it_and_a_press_outside_dismisses() {
        let mut px = Vec::new();
        let mut sf = surface(&mut px);
        let rect = menu_rect(1, (10, 10), &ITEMS, SCREEN);
        let second = (20, 10 + ROW + 5);
        let mut ui = ui_at(second.0, second.1);
        ui.pressed = true;
        ui.down = true;
        assert_eq!(pass(&mut ui, &mut sf, rect).1, MenuResult::Open);
        ui.down = false;
        ui.released = true;
        assert_eq!(pass(&mut ui, &mut sf, rect).1, MenuResult::Chosen(1));
        let mut ui = ui_at(300, 250);
        ui.pressed = true;
        ui.down = true;
        assert_eq!(pass(&mut ui, &mut sf, rect).1, MenuResult::Dismissed);
    }

    /// A row that cannot be chosen dims its key as well as its words (final review, 13).
    #[test]
    fn a_disabled_row_dims_its_key() {
        crate::theme::set(crate::theme::Theme::LIGHTBOX);
        let items = [MenuItem {
            label: "Paste	Ctrl V",
            enabled: false,
        }];
        let mut px = Vec::new();
        let mut sf = surface(&mut px);
        let rect = menu_rect(1, (10, 10), &items, SCREEN);
        let mut ui = ui_at(399, 299);
        ui.begin();
        menu(&mut ui, &mut sf, 100, rect, &items);
        let row = menu_row(rect, 0, 1);
        let right_half = (row.x + row.w / 2..row.right())
            .flat_map(|x| (row.y..row.bottom()).map(move |y| (x, y)))
            .map(|(x, y)| px[(y * 400 + x) as usize]);
        let colours: Vec<u32> = right_half.collect();
        assert!(colours.contains(&theme::disabled()), "the key is dimmed");
        assert!(!colours.contains(&theme::muted()), "no key at full Muted");
        crate::theme::set(crate::theme::Theme::NORMAL);
    }

    #[test]
    fn a_group_starts_under_a_one_unit_rule_and_no_row_moves() {
        let mut px = Vec::new();
        let mut sf = surface(&mut px);
        let rect = menu_rect(1, (10, 10), &ITEMS, SCREEN);
        let mut ui = ui_at(399, 299);
        ui.begin();
        menu_grouped(&mut ui, &mut sf, (100, rect), &ITEMS, 0, &[0, 2]);
        let at = |x: i32, y: i32| px[(y * 400 + x) as usize];
        let second = menu_row(rect, 2, 1);
        let mid = second.x + second.w / 2;
        assert_eq!(at(mid, second.y), theme::rule(), "a rule above the group");
        assert_ne!(at(mid, second.y + 1), theme::rule(), "one unit thick");
        assert_ne!(at(second.x, second.y), theme::rule(), "inset from the edge");
        let first = menu_row(rect, 1, 1);
        assert_ne!(
            at(mid, first.y),
            theme::rule(),
            "none where no group starts"
        );
        let top = menu_row(rect, 0, 1);
        assert_ne!(at(mid, top.y), theme::rule(), "none above the first row");
        assert_eq!(
            second,
            Rect::new(rect.x, rect.y + 48, rect.w, 24),
            "rows keep their place"
        );
    }

    #[test]
    fn room_for_a_picture_widens_the_menu_and_moves_only_the_words() {
        let long = [MenuItem {
            label: "Long enough to set it, and a good many more words to be sure it is wider",
            enabled: true,
        }];
        let plain = menu_rect(1, (10, 10), &long, SCREEN);
        let roomy = menu_rect_with_room(1, (10, 10), &long, SCREEN, 22);
        assert_eq!(roomy.w, plain.w + 22);
        assert_eq!(roomy.h, plain.h);
        assert_eq!(
            menu_row(roomy, 1, 1),
            Rect::new(roomy.x, roomy.y + 24, roomy.w, 24)
        );
        // The words start after the room: nothing is drawn in it.
        let mut px = Vec::new();
        let mut sf = surface(&mut px);
        let rect = menu_rect_with_room(1, (10, 10), &ITEMS, SCREEN, 22);
        let mut ui = ui_at(399, 299);
        ui.begin();
        menu_with_room(&mut ui, &mut sf, 100, rect, &ITEMS, 22);
        let row = menu_row(rect, 0, 1);
        let inked = |x0: i32, x1: i32| {
            (x0..x1)
                .any(|x| (row.y..row.bottom()).any(|y| px[(y * 400 + x) as usize] == theme::text()))
        };
        assert!(!inked(row.x, row.x + MENU_PAD + 22));
        assert!(inked(row.x + MENU_PAD + 22, row.right()));
    }

    #[test]
    fn a_disabled_item_cannot_be_chosen() {
        let mut px = Vec::new();
        let mut sf = surface(&mut px);
        let rect = menu_rect(1, (10, 10), &ITEMS, SCREEN);
        let mut ui = ui_at(20, 10 + 2 * ROW + 5);
        ui.pressed = true;
        ui.down = true;
        pass(&mut ui, &mut sf, rect);
        ui.down = false;
        ui.released = true;
        assert_eq!(pass(&mut ui, &mut sf, rect).1, MenuResult::Open);
    }
    const MIXED: [MenuItem<'static>; 5] = [
        MenuItem {
            label: "Alpha",
            enabled: false,
        },
        MenuItem {
            label: "Beta",
            enabled: true,
        },
        MenuItem {
            label: "Bravo",
            enabled: false,
        },
        MenuItem {
            label: "Charlie",
            enabled: true,
        },
        MenuItem {
            label: "beta again",
            enabled: true,
        },
    ];

    #[test]
    fn up_and_down_wrap_and_skip_disabled_rows() {
        use MenuKey::{Down, Up};
        use MenuNav::Highlight;
        assert_eq!(menu_key(&MIXED, None, Down), Highlight(1));
        assert_eq!(menu_key(&MIXED, None, Up), Highlight(4));
        assert_eq!(menu_key(&MIXED, Some(1), Down), Highlight(3));
        assert_eq!(
            menu_key(&MIXED, Some(4), Down),
            Highlight(1),
            "wraps past the end"
        );
        assert_eq!(
            menu_key(&MIXED, Some(1), Up),
            Highlight(4),
            "wraps past the top"
        );
        assert_eq!(menu_key(&MIXED, Some(3), Up), Highlight(1));
    }

    #[test]
    fn home_and_end_go_to_the_first_and_last_enabled_rows() {
        assert_eq!(
            menu_key(&MIXED, Some(3), MenuKey::Home),
            MenuNav::Highlight(1)
        );
        assert_eq!(
            menu_key(&MIXED, Some(1), MenuKey::End),
            MenuNav::Highlight(4)
        );
    }

    #[test]
    fn enter_chooses_only_an_enabled_highlighted_row() {
        assert_eq!(
            menu_key(&MIXED, Some(3), MenuKey::Enter),
            MenuNav::Chosen(3)
        );
        assert_eq!(menu_key(&MIXED, None, MenuKey::Enter), MenuNav::None);
        assert_eq!(menu_key(&MIXED, Some(0), MenuKey::Enter), MenuNav::None);
    }

    #[test]
    fn a_letter_jumps_to_the_next_enabled_row_that_starts_with_it() {
        let key = MenuKey::Letter;
        assert_eq!(menu_key(&MIXED, None, key('b')), MenuNav::Highlight(1));
        assert_eq!(menu_key(&MIXED, Some(1), key('B')), MenuNav::Highlight(4));
        assert_eq!(menu_key(&MIXED, Some(4), key('b')), MenuNav::Highlight(1));
        assert_eq!(menu_key(&MIXED, None, key('a')), MenuNav::None, "disabled");
        assert_eq!(menu_key(&MIXED, None, key('z')), MenuNav::None);
    }

    #[test]
    fn an_all_disabled_or_empty_menu_answers_nothing() {
        let off = [MenuItem {
            label: "x",
            enabled: false,
        }];
        assert_eq!(menu_key(&off, None, MenuKey::Down), MenuNav::None);
        assert_eq!(menu_key(&[], None, MenuKey::End), MenuNav::None);
    }

    #[test]
    fn the_highlighted_row_is_drawn_with_a_marker_bar() {
        let mut px = Vec::new();
        let mut sf = surface(&mut px);
        let rect = menu_rect(1, (10, 10), &ITEMS, SCREEN);
        let mut ui = ui_at(399, 299);
        ui.menu_highlight = Some(1);
        ui.begin();
        menu(&mut ui, &mut sf, 100, rect, &ITEMS);
        let row = menu_row(rect, 1, 1);
        let at = |x: i32, y: i32| px[(y * 400 + x) as usize];
        assert_eq!(at(row.x + 1, row.y + 5), crate::theme::accent(), "the bar");
        assert_eq!(
            at(row.right() - 2, row.y + 1),
            theme::hover(),
            "the row's fill"
        );
        let other = menu_row(rect, 0, 1);
        assert_eq!(at(other.x + 1, other.y + 5), theme::header());
    }

    #[test]
    fn a_key_after_a_tab_is_shown_at_the_right_and_is_not_the_rows_name() {
        assert_eq!(split_hint("Save\tCtrl S"), ("Save", "Ctrl S"));
        assert_eq!(split_hint("Save"), ("Save", ""));
        let plain = [MenuItem {
            label: "Save",
            enabled: true,
        }];
        let keyed = [MenuItem {
            label: "Save\tCtrl S",
            enabled: true,
        }];
        let wide = [MenuItem {
            label: "A long enough row to set the width of it",
            enabled: true,
        }];
        let a = menu_rect(1, (0, 0), &plain, SCREEN);
        let b = menu_rect(1, (0, 0), &keyed, SCREEN);
        assert!(b.w > a.w || a.w == MIN_W);
        let c = menu_rect(1, (0, 0), &wide, SCREEN);
        let mut px = Vec::new();
        let mut sf = surface(&mut px);
        let mut ui = ui_at(399, 299);
        ui.record_names();
        ui.begin();
        menu(&mut ui, &mut sf, 100, c, &keyed);
        let dump = ui.layout().unwrap_or_default();
        let name = dump.lines().next().and_then(|l| l.split('\t').nth(5));
        assert_eq!(name, Some("Save"));
        let row = menu_row(c, 0, 1);
        let right = (row.right() - PAD - 40..row.right() - PAD)
            .any(|x| (row.y..row.bottom()).any(|y| px[(y * 400 + x) as usize] == theme::muted()));
        assert!(right, "the key is drawn at the right edge");
    }

    #[test]
    fn a_key_widens_a_row_by_its_gap_and_its_own_width() {
        let long = |label| {
            [MenuItem {
                label,
                enabled: true,
            }]
        };
        let plain = menu_rect(
            1,
            (0, 0),
            &long("A long row here, long enough to be wider than the least width of a menu"),
            SCREEN,
        );
        let keyed = menu_rect(
            1,
            (0, 0),
            &long(
                "A long row here, long enough to be wider than the least width of a menu\tCtrl S",
            ),
            SCREEN,
        );
        assert_eq!(
            keyed.w,
            plain.w + HINT_GAP + text_width("Ctrl S", crate::font::text_scale(1))
        );
    }
}
