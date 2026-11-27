use super::*;
use crate::focus::Press;
use crate::theme::Theme;

const W: i32 = 300;
const H: i32 = 40;
const R: Rect = Rect::new(0, 10, 300, 20);
const TABS: [&str; 3] = ["Layers", "Palette", "Color"];

fn strip(active: usize) -> Strip<'static> {
    Strip {
        tabs: &TABS,
        active,
        folded: false,
        icons: &[],
    }
}

fn pass(ui: &mut Ui, px: &mut [u32], s: Strip<'_>) -> StripOut {
    let mut sf = Surface { px, w: W, h: H };
    ui.begin();
    ui.focus.collect(true);
    let out = tab_strip(ui, &mut sf, 100, R, s);
    ui.focus.collect(false);
    ui.end();
    out
}

fn tab_rect(ui: &Ui, n: u32) -> Rect {
    ui.widgets()
        .iter()
        .find(|(_, id)| *id == 101 + n)
        .map(|&(r, _)| r)
        .unwrap_or_default()
}

#[test]
fn a_click_on_a_tab_chooses_it() {
    let mut px = vec![0; (W * H) as usize];
    let mut ui = Ui::new();
    pass(&mut ui, &mut px, strip(0));
    let second = tab_rect(&ui, 1);
    ui.mouse = (second.x + 3, second.y + 3);
    ui.pressed = true;
    ui.released = true;
    assert_eq!(
        pass(&mut ui, &mut px, strip(0)).event,
        Some(TabEvent::Choose(1))
    );
}

#[test]
fn arrows_choose_the_neighbour_and_take_focus_with_them_wrapping() {
    let mut px = vec![0; (W * H) as usize];
    let mut ui = Ui::new();
    pass(&mut ui, &mut px, strip(0));
    ui.focus.set(101);
    let mut press = |ui: &mut Ui, key, active| {
        ui.focus.request_press(Press::plain(key));
        pass(ui, &mut px, strip(active)).event
    };
    assert_eq!(
        press(&mut ui, WidgetKey::Right, 0),
        Some(TabEvent::Choose(1))
    );
    assert_eq!(ui.focus.focused(), Some(102));
    assert_eq!(
        press(&mut ui, WidgetKey::Right, 1),
        Some(TabEvent::Choose(2))
    );
    assert_eq!(
        press(&mut ui, WidgetKey::Right, 2),
        Some(TabEvent::Choose(0)),
        "wraps"
    );
    assert_eq!(ui.focus.focused(), Some(101));
    assert_eq!(
        press(&mut ui, WidgetKey::Left, 0),
        Some(TabEvent::Choose(2))
    );
    assert_eq!(
        press(&mut ui, WidgetKey::Home, 2),
        Some(TabEvent::Choose(0))
    );
    assert_eq!(press(&mut ui, WidgetKey::End, 0), Some(TabEvent::Choose(2)));
    ui.focus.request_activate();
    assert_eq!(
        pass(&mut ui, &mut px, strip(0)).event,
        Some(TabEvent::Choose(2)),
        "Enter shows the focused tab"
    );
}

#[test]
fn the_menu_key_and_the_menu_button_ask_for_the_slot_menu() {
    let mut px = vec![0; (W * H) as usize];
    let mut ui = Ui::new();
    pass(&mut ui, &mut px, strip(1));
    ui.focus.set(103);
    ui.focus.request_press(Press::plain(WidgetKey::Menu));
    let third = tab_rect(&ui, 2);
    assert_eq!(
        pass(&mut ui, &mut px, strip(1)).event,
        Some(TabEvent::Menu {
            tab: 2,
            at: (third.x, third.bottom())
        }),
        "the menu of the focused tab, under it"
    );
    ui.focus.set(100 + MENU);
    ui.focus.request_activate();
    assert!(matches!(
        pass(&mut ui, &mut px, strip(1)).event,
        Some(TabEvent::Menu { tab: 1, .. })
    ));
    ui.focus.set(100 + FOLD);
    ui.focus.request_activate();
    assert_eq!(pass(&mut ui, &mut px, strip(1)).event, Some(TabEvent::Fold));
}

#[test]
fn the_tools_room_lies_between_the_tabs_and_the_menu_button() {
    let mut px = vec![0; (W * H) as usize];
    let mut ui = Ui::new();
    let out = pass(&mut ui, &mut px, strip(0));
    let last = tab_rect(&ui, 2);
    assert_eq!(out.tools.x, last.right());
    assert_eq!(out.tools.right(), R.right() - 48);
    assert_eq!(out.tools.h, R.h);
}

#[test]
fn tabs_that_do_not_fit_share_the_room_and_stay_inside_it() {
    let mut px = vec![0; (W * H) as usize];
    let mut ui = Ui::new();
    let many = ["Layers"; 8];
    let s = Strip {
        tabs: &many,
        active: 0,
        folded: false,
        icons: &[],
    };
    let out = pass(&mut ui, &mut px, s);
    let last = tab_rect(&ui, 7);
    assert!(last.right() <= R.right() - 40, "{last:?}");
    assert_eq!(tab_rect(&ui, 0).w, last.w);
    assert!(out.tools.w < 40);
}

#[test]
fn the_shown_tab_joins_the_panel_with_an_accent_line_and_others_stand_on_the_edge() {
    theme::set(Theme::LIGHTBOX);
    let mut px = vec![0; (W * H) as usize];
    let mut ui = Ui::new();
    pass(&mut ui, &mut px, strip(0));
    let (first, second) = (tab_rect(&ui, 0), tab_rect(&ui, 1));
    let at = |x: i32, y: i32| px[(y * W + x) as usize];
    let mid = first.x + first.w / 2;
    assert_eq!(at(mid, first.y), theme::edge(), "top edge");
    assert_eq!(at(mid, first.y + 1), theme::accent(), "accent under it");
    assert_eq!(
        at(mid, first.bottom() - 1),
        theme::panel(),
        "no line below: it joins"
    );
    assert_eq!(at(first.x, first.y), theme::header(), "notched corner");
    let other = second.x + 2;
    assert_eq!(
        at(other, second.bottom() - 1),
        theme::edge(),
        "the strip's line"
    );
    assert_eq!(at(other, second.y), theme::header());
    theme::set(Theme::NORMAL);
}

#[test]
fn focus_rings_the_tab_in_the_focus_colour() {
    theme::set(Theme::LIGHTBOX);
    let mut px = vec![0; (W * H) as usize];
    let mut ui = Ui::new();
    pass(&mut ui, &mut px, strip(0));
    ui.focus.set(102);
    pass(&mut ui, &mut px, strip(0));
    let second = tab_rect(&ui, 1);
    let at = |x: i32, y: i32| px[(y * W + x) as usize];
    assert_eq!(at(second.x + 5, second.y - 2), theme::focus());
    assert_eq!(at(second.x + 5, second.bottom() + 1), theme::focus());
    theme::set(Theme::NORMAL);
}

#[test]
fn the_strip_is_a_tab_list_of_tabs_with_named_buttons() {
    let mut px = vec![0; (W * H) as usize];
    let mut ui = Ui::new();
    ui.record_names();
    pass(&mut ui, &mut px, strip(1));
    let text = ui.layout().unwrap_or_default();
    for line in [
        "100\t0\t10\t300\t20\tPalette tabs\ttab-list\texpanded\n",
        "\tPalette\ttab\tchosen\n",
        "\tLayers\ttab\t\n",
        "\tPalette menu\tbutton\t\n",
        "\tFold Palette\tbutton\texpanded\n",
    ] {
        assert!(text.contains(line), "{line:?} in {text}");
    }
    let folded = Strip {
        folded: true,
        ..strip(1)
    };
    pass(&mut ui, &mut px, folded);
    assert!(ui
        .layout()
        .unwrap_or_default()
        .contains("\tUnfold Palette\tbutton\tcollapsed\n"));
}

#[test]
fn ctrl_tab_cycles_both_ways() {
    assert_eq!(cycle(2, 3, false), 0);
    assert_eq!(cycle(0, 3, true), 2);
    assert_eq!(cycle(0, 0, false), 0);
}

#[test]
fn the_enter_key_pressed_on_a_tab_shows_it() {
    let mut px = vec![0; (W * H) as usize];
    let mut ui = Ui::new();
    pass(&mut ui, &mut px, strip(0));
    ui.focus.set(102);
    ui.focus.request_press(Press::plain(WidgetKey::Enter));
    assert_eq!(
        pass(&mut ui, &mut px, strip(0)).event,
        Some(TabEvent::Choose(1)),
        "Enter on the second tab, whichever is shown"
    );
    assert_eq!(ui.focus.focused(), Some(102), "focus stays on that tab");
}

const CROWDED: [&str; 3] = ["Layers", "Slices", "Pixel Lint"];

/// A strip of `CROWDED` in a `w` wide row, with icons for the tabs.
fn crowded(ui: &mut Ui, w: i32, active: usize, with_icons: bool) -> StripOut {
    let art = crate::icons::ActionIcon::Stack.art();
    let icons = [Some(art), Some(art), Some(art)];
    let s = Strip {
        tabs: &CROWDED,
        active,
        folded: false,
        icons: if with_icons { &icons } else { &[] },
    };
    let mut px = vec![0; (w * 40) as usize];
    let mut sf = Surface {
        px: &mut px,
        w,
        h: 40,
    };
    ui.begin();
    ui.focus.collect(true);
    let out = tab_strip(ui, &mut sf, 100, Rect::new(0, 10, w, 20), s);
    ui.focus.collect(false);
    ui.end();
    out
}

#[test]
fn a_crowded_strip_keeps_the_shown_tab_whole_and_the_others_as_icons() {
    let mut ui = Ui::new();
    let ts = ui.text_scale();
    // Room for the shown name but not for all three names, even with less padding.
    let out = crowded(&mut ui, snug_width(), 0, true);
    assert_eq!(out.compact, 0b110, "Slices and Pixel Lint are icons");
    let shown = tab_rect(&ui, 0);
    assert!(
        shown.w >= text_width("Layers", ts) + 8,
        "whole name: {shown:?}"
    );
    assert_eq!(tab_rect(&ui, 1).w, 24, "an icon tab is a 24-unit target");
    assert_eq!(tab_rect(&ui, 2).w, 24);
    let last = tab_rect(&ui, 2);
    assert!(
        last.right() <= snug_width() - 48,
        "inside the room for the buttons"
    );
    // The shown tab can be any of them.
    let out = crowded(&mut ui, snug_width(), 2, true);
    assert_eq!(out.compact, 0b011);
    // Too snug for "Pixel Lint" beside two icons: the shown tab keeps its short word whole.
    assert!(tab_rect(&ui, 2).w >= text_width("Lint", ts) + 8);
}

#[test]
fn a_shown_tab_beside_icons_keeps_its_whole_name_where_that_fits() {
    let ts = Ui::new().text_scale();
    // Long one-word neighbours: no short word helps them, so they become icons, and the room
    // left is enough for the shown tab's whole name.
    let tabs = ["Pixel Lint", "Palettebrowser", "Referenceimages"];
    let w = text_width("Pixel Lint", ts) + 8 + 2 * 24 + 48;
    let mut ui = Ui::new();
    let out = narrow(&mut ui, &tabs, (w, 0), true);
    assert_eq!(out.compact, 0b110);
    assert!(tab_rect(&ui, 0).w >= text_width("Pixel Lint", ts) + 8);
}

#[test]
fn a_roomy_strip_has_whole_names_and_a_snug_one_keeps_them_with_less_padding() {
    let mut ui = Ui::new();
    assert_eq!(crowded(&mut ui, 300, 0, true).compact, 0);
    let ts = ui.text_scale();
    let natural: i32 = CROWDED.iter().map(|t| text_width(t, ts) + 16).sum();
    // Not enough for the usual padding, enough for four units a side.
    let out = crowded(&mut ui, natural - 8 + 48, 0, true);
    assert_eq!(out.compact, 0, "whole names with narrower tabs");
    assert!(tab_rect(&ui, 2).right() <= natural - 8);
}

#[test]
fn without_icons_a_crowded_strip_shares_the_room_as_before() {
    let mut ui = Ui::new();
    let out = crowded(&mut ui, snug_width(), 0, false);
    assert_eq!(out.compact, 0);
    assert_eq!(tab_rect(&ui, 0).w, tab_rect(&ui, 2).w);
}

#[test]
fn a_tab_is_never_two_letters_and_dots() {
    // Whatever the room, a tab with an icon is a whole name or an icon alone.
    let ts = Ui::new().text_scale();
    for w in (60..=260).step_by(4) {
        let mut ui = Ui::new();
        let out = crowded(&mut ui, w, 1, true);
        for n in 0..3u32 {
            let r = tab_rect(&ui, n);
            let whole = text_width(short_label(CROWDED[n as usize]), ts) + 2;
            let icon = out.compact >> n & 1 == 1;
            assert!(
                icon || r.w >= whole,
                "{w}: tab {n} is {} wide, not whole",
                r.w
            );
        }
    }
}

/// A strip width whose room is a little under what even the short words of the names need
/// with half padding, so icons are the next step.
fn snug_width() -> i32 {
    let ts = Ui::new().text_scale();
    CROWDED
        .iter()
        .map(|t| text_width(short_label(t), ts) + 8)
        .sum::<i32>()
        - 4
        + 48
}

#[test]
fn a_strip_too_narrow_for_whole_names_shows_the_short_word_before_any_icon() {
    let ts = Ui::new().text_scale();
    let whole: i32 = CROWDED.iter().map(|t| text_width(t, ts) + 8).sum();
    let short: i32 = CROWDED
        .iter()
        .map(|t| text_width(short_label(t), ts) + 8)
        .sum();
    assert!(short < whole);
    let mut ui = Ui::new();
    crate::font::log_text(true);
    let out = crowded(&mut ui, short + 48, 0, true);
    let log = crate::font::take_text_log();
    crate::font::log_text(false);
    assert_eq!(out.compact, 0, "no icons while the short words fit");
    let words: Vec<&str> = log.texts.iter().map(|(_, t)| t.as_str()).collect();
    assert!(
        words.contains(&"Lint"),
        "Pixel Lint shows as Lint: {words:?}"
    );
    assert!(words.contains(&"Layers") && words.contains(&"Slices"));
    let last = tab_rect(&ui, 2);
    assert!(last.right() <= short, "inside the room: {last:?}");
    assert_eq!(short_label("Pixel Lint"), "Lint");
    assert_eq!(short_label("Layers"), "Layers");
}

/// A strip of `tabs` (each with an icon when `icons`) in a row `w` wide, drawn once.
fn narrow(ui: &mut Ui, tabs: &[&str], (w, active): (i32, usize), icons: bool) -> StripOut {
    let art = crate::icons::ActionIcon::Stack.art();
    let all = [Some(art); 4];
    let s = Strip {
        tabs,
        active,
        folded: false,
        icons: if icons { &all[..tabs.len()] } else { &[] },
    };
    let (sw, sh) = (w.max(1), 40);
    let mut px = vec![0; (sw * sh) as usize];
    let mut sf = Surface {
        px: &mut px,
        w: sw,
        h: sh,
    };
    ui.begin();
    ui.focus.collect(true);
    let out = tab_strip(ui, &mut sf, 100, Rect::new(0, 10, w, 20), s);
    ui.focus.collect(false);
    ui.end();
    out
}

#[test]
fn a_dock_too_narrow_for_its_buttons_gives_every_tab_no_width_not_a_negative_one() {
    for icons in [false, true] {
        for w in [0, 1, 20, 47, 48, 49, 60] {
            let mut ui = Ui::new();
            narrow(&mut ui, &CROWDED, (w, 0), icons);
            for n in 0..3 {
                let r = tab_rect(&ui, n);
                assert!(r.w >= 0, "w {w}, icons {icons}: tab {n} is {r:?}");
                assert!(r.x >= 0 && r.right() <= w.max(0) + 24, "{r:?} in {w}");
            }
        }
    }
}

#[test]
fn one_tab_takes_its_name_or_what_there_is() {
    let mut ui = Ui::new();
    let ts = ui.text_scale();
    let out = narrow(&mut ui, &["Layers"], (300, 0), true);
    assert_eq!(out.compact, 0);
    assert_eq!(tab_rect(&ui, 0).w, text_width("Layers", ts) + 16);
    for w in [0, 30, 50, 80] {
        let mut ui = Ui::new();
        let out = narrow(&mut ui, &["Layers"], (w, 0), true);
        assert!(tab_rect(&ui, 0).w >= 0, "{w}");
        assert_eq!(out.compact & !1, 0, "only the one tab can be an icon");
    }
}

#[test]
fn an_active_tab_past_the_end_makes_every_tab_an_icon_without_overflowing() {
    let mut ui = Ui::new();
    let out = narrow(&mut ui, &CROWDED, (snug_width(), 7), true);
    assert_eq!(out.compact, 0b111, "no tab is the shown one");
    for n in 0..3 {
        assert_eq!(tab_rect(&ui, n).w, 24);
    }
    let mut ui = Ui::new();
    let out = narrow(&mut ui, &CROWDED, (48 + 40, 9), true);
    assert_eq!(out.compact, 0b111);
    assert!((0..3).all(|n| tab_rect(&ui, n).w == 40 / 3));
}

#[test]
fn a_tab_narrower_than_its_icon_draws_no_art() {
    // 16 units for three tabs is five each, under the 8-unit icon.
    let mut ui = Ui::new();
    let (w, tabs) = (48 + 16, &CROWDED);
    let art = crate::icons::ActionIcon::Stack.art();
    let icons = [Some(art); 3];
    let s = Strip {
        tabs,
        active: 9,
        folded: false,
        icons: &icons,
    };
    let mut px = vec![0u32; (w * 40) as usize];
    px.iter_mut().for_each(|p| *p = 0x00ab_cdef);
    let mut sf = Surface {
        px: &mut px,
        w,
        h: 40,
    };
    ui.begin();
    tab_strip(&mut ui, &mut sf, 100, Rect::new(0, 10, w, 20), s);
    ui.end();
    let tabs_area = Rect::new(0, 10, 15, 19);
    let drawn = (tabs_area.y..tabs_area.bottom())
        .flat_map(|y| (0..tabs_area.w).map(move |x| (x, y)))
        .any(|(x, y)| px[(y * w + x) as usize] != theme::header());
    assert!(!drawn, "nothing but the strip's own colour under the tabs");
}

#[test]
fn a_focused_icon_tab_still_answers_the_arrows() {
    let mut ui = Ui::new();
    let w = snug_width();
    narrow(&mut ui, &CROWDED, (w, 0), true);
    ui.focus.set(102);
    ui.focus.request_press(Press::plain(WidgetKey::Right));
    let out = narrow(&mut ui, &CROWDED, (w, 0), true);
    assert_eq!(out.event, Some(TabEvent::Choose(2)));
    assert_eq!(ui.focus.focused(), Some(103));
}

/// An icon tab that is not shown is drawn in Muted, like a word tab, so it never looks shown
/// (final review, defect 9).
#[test]
fn an_icon_tab_that_is_not_shown_is_muted() {
    crate::theme::set(crate::theme::Theme::LIGHTBOX);
    let mut ui = Ui::new();
    let w = snug_width();
    let art = crate::icons::ActionIcon::Stack.art();
    let icons = [Some(art), Some(art), Some(art)];
    let s = Strip {
        tabs: &CROWDED,
        active: 0,
        folded: false,
        icons: &icons,
    };
    let mut px = vec![0; (w * 40) as usize];
    let mut sf = Surface {
        px: &mut px,
        w,
        h: 40,
    };
    ui.begin();
    let out = tab_strip(&mut ui, &mut sf, 100, Rect::new(0, 10, w, 20), s);
    ui.end();
    assert_eq!(out.compact, 0b110);
    let r = tab_rect(&ui, 1);
    let inks: Vec<u32> = (r.y..r.bottom())
        .flat_map(|y| (r.x..r.right()).map(move |x| (x, y)))
        .map(|(x, y)| px[(y * w + x) as usize])
        .collect();
    assert!(inks.contains(&theme::muted()), "drawn in Muted");
    assert!(!inks.contains(&theme::text()), "not in Text");
    crate::theme::set(crate::theme::Theme::NORMAL);
}
