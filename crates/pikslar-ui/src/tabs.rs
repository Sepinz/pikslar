//! A slot's tab strip (docs/ui-redesign.md 2.5, 3.3, 3.7): the slot's panels as tabs, room
//! for the shown panel's header tools, then the slot menu button and the fold chevron.
//!
//! Keys on a focused tab: Left and Right choose the previous and next tab (wrapping), Home and
//! End the first and last, Enter and Space show it, and Shift+F10 or the menu key ask for the
//! slot menu. Ctrl+Tab is the caller's, with [`cycle`], since focus may be in the panel.

use crate::chrome::{self, Look};
use crate::focus::WidgetKey;
use crate::font::{cap_height, draw_text, text_width};
use crate::icons::{ActionIcon, Art, MarkIcon};
use crate::role::{Role, State};
use crate::text_fit::fit_to_width;
use crate::theme;
use crate::widgets::Ui;
use pikslar_render::{Rect, Surface};

/// The strip's least height, in units; see [`strip_h`].
pub const STRIP_H: i32 = 20;

/// The strip's height, in units: 20, or as tall as the face needs (`fit_h`), so *Large text*
/// grows it.
pub fn strip_h() -> i32 {
    crate::font::fit_h(STRIP_H)
}
/// The most tabs a strip shows; more are left to the slot menu.
pub const MAX_TABS: usize = 16;
/// The ids a strip takes from the `id` it is given: `id` itself, a tab each from `id + 1`, the
/// menu button at `id + 17` and the fold chevron at `id + 18`.
pub const TAB_IDS: u32 = 19;
const MENU: u32 = 17;
/// The fold chevron's offset from the strip's id.
pub const FOLD: u32 = 18;
/// Space either side of a tab's words, in units.
const PAD: i32 = 8;
/// The width of the menu button and of the chevron, in units: 20-unit controls on a 24-unit
/// pitch (docs/ui-redesign.md 2.5).
pub const BUTTON_W: i32 = 24;
const BUTTON: i32 = BUTTON_W;

/// What a strip shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strip<'a> {
    pub tabs: &'a [&'a str],
    pub active: usize,
    /// The slot is folded to its strip.
    pub folded: bool,
    /// An icon for each tab, in step with `tabs`, or none. With icons, a crowded strip shows the
    /// tabs that are not shown as their icons alone rather than cutting every name short; the
    /// name stays on the tab for the layout dump and for the caller's hint.
    pub icons: &'a [Option<Art>],
}

/// What a strip asks for this pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabEvent {
    /// Show this tab (and unfold the slot).
    Choose(usize),
    /// Open the slot menu for this tab, at this point.
    Menu { tab: usize, at: (i32, i32) },
    /// Fold the slot, or unfold it.
    Fold,
}

/// A strip's answer: what it asks, and the room between the tabs and the menu button for the
/// shown panel's header tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StripOut {
    pub event: Option<TabEvent>,
    pub tools: Rect,
    /// Bit `n` is set when tab `n` is drawn as its icon alone.
    pub compact: u16,
}

/// The tab after `active` of `count`, or before it going `back`, wrapping: Ctrl+Tab.
pub fn cycle(active: usize, count: usize, back: bool) -> usize {
    match (count, back) {
        (0, _) => 0,
        (_, true) => (active + count - 1) % count,
        (_, false) => (active + 1) % count,
    }
}

/// The width of a tab drawn as its icon alone, in units: a 24-unit target.
const ICON_TAB: i32 = 24;

/// How the tabs share `room`: each tab's width, and the bits of those drawn as icons alone.
/// Whole names with the usual padding if they fit; else with half of it; else, when every tab
/// has an icon, the shown tab whole and the others as icons; else the room shared evenly (as
/// icons if every tab has one). At most [`MAX_TABS`] tabs; no allocation.
fn plan(strip: Strip<'_>, tabs: &[&str], ts: i32, s: i32, room: i32) -> ([i32; MAX_TABS], u16) {
    // A dock narrower than its two buttons leaves no room, never a negative one.
    let room = room.max(0);
    let mut out = [0; MAX_TABS];
    for pad in [PAD, PAD / 2] {
        for (w, t) in out.iter_mut().zip(tabs) {
            *w = text_width(t, ts) + 2 * pad * s;
        }
        if out.iter().sum::<i32>() <= room || tabs.is_empty() {
            return (out, 0);
        }
    }
    let has_icon = |n: usize| strip.icons.get(n).is_some_and(Option::is_some);
    if !(0..tabs.len()).all(has_icon) {
        let even = room / tabs.len() as i32;
        out.iter_mut().take(tabs.len()).for_each(|w| *w = even);
        return (out, 0);
    }
    let all = ((1u32 << tabs.len()) - 1) as u16;
    let others = all & !(1u32 << strip.active.min(31)) as u16;
    for (n, w) in out.iter_mut().take(tabs.len()).enumerate() {
        if others >> n & 1 == 1 {
            *w = ICON_TAB * s;
        }
    }
    if out.iter().sum::<i32>() <= room {
        return (out, others);
    }
    let each = (ICON_TAB * s).min(room / tabs.len() as i32);
    out.iter_mut().take(tabs.len()).for_each(|w| *w = each);
    (out, all)
}

/// Draws the strip in `r` and handles its pointer and keys.
pub fn tab_strip(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    r: Rect,
    strip: Strip<'_>,
) -> StripOut {
    let s = ui.scale.max(1);
    let tabs = &strip.tabs[..strip.tabs.len().min(MAX_TABS)];
    let shown = tabs.get(strip.active).copied().unwrap_or("");
    let expanded = State {
        expanded: Some(!strip.folded),
        ..State::NONE
    };
    ui.container(id, r, Role::TabList, expanded);
    if ui.naming() {
        ui.name(id, &format!("{shown} tabs"));
    }
    sf.fill(r, theme::header());
    let fold = Rect::new(r.right() - BUTTON * s, r.y, BUTTON * s, r.h);
    let menu = Rect::new(fold.x - BUTTON * s, r.y, BUTTON * s, r.h);
    let ts = ui.text_scale();
    // The line the panel hangs from, broken under the shown tab, which draws over it.
    sf.fill(Rect::new(r.x, r.bottom() - s, r.w, s), theme::edge());
    let mut x = r.x;
    let mut event = None;
    let (sizes, compact) = plan(strip, tabs, ts, s, menu.x - r.x);
    for (n, &w) in sizes.iter().take(tabs.len()).enumerate() {
        let tab = Rect::new(x, r.y, w, r.h);
        let icon = (compact >> n & 1 == 1).then(|| strip.icons[n]).flatten();
        event = draw_tab(ui, sf, (id, n), tab, (strip, icon), tabs.len()).or(event);
        x += w;
    }
    let tools = Rect::new(x, r.y, (menu.x - x).max(0), r.h);
    event = strip_buttons(ui, sf, id, (menu, fold), strip, shown).or(event);
    StripOut {
        event,
        tools,
        compact,
    }
}

/// A tab's words, cut with an ellipsis only when nothing better fits.
fn draw_label(sf: &mut Surface<'_>, (r, s, ts): (Rect, i32, i32), label: &str, look: Look) {
    let words = fit_to_width(label, r.w - 2 * s, ts);
    let ink = if look.chosen || look.hover {
        theme::text()
    } else {
        theme::muted()
    };
    let tx = r.x + (r.w - text_width(&words, ts)) / 2;
    let ty = r.y + (r.h - cap_height() * ts) / 2 + look.drop() * s;
    draw_text(sf, tx, ty, &words, ink, ts);
}

/// One tab: its look, its keys and its click.
fn draw_tab(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    (strip_id, n): (u32, usize),
    r: Rect,
    (strip, icon): (Strip<'_>, Option<Art>),
    count: usize,
) -> Option<TabEvent> {
    let s = ui.scale.max(1);
    let id = strip_id + 1 + n as u32;
    let label = strip.tabs[n];
    let chosen = n == strip.active;
    let state = ui.interact(id, r);
    ui.focus.register_keys(id);
    ui.describe(Role::Tab, State::chosen(chosen));
    ui.name(id, label);
    let look = Look::of(state, chosen, false);
    paint_tab(sf, r, s, look);
    match icon {
        Some(art) => draw_tab_art(sf, (r, s), art, look),
        None => draw_label(sf, (r, s, ui.text_scale()), label, look),
    }
    ui.ring(sf, id, r);
    let keyed = ui
        .focus
        .take_press(id)
        .and_then(|p| tab_key(p.key, n, count));
    if let Some(TabEvent::Choose(to)) = keyed {
        ui.focus.set(strip_id + 1 + to as u32);
    }
    let keyed = keyed.map(|e| match e {
        TabEvent::Menu { tab, .. } => TabEvent::Menu {
            tab,
            at: (r.x, r.bottom()),
        },
        e => e,
    });
    state.clicked.then_some(TabEvent::Choose(n)).or(keyed)
}

/// An icon tab's art. One too narrow for its art shows nothing rather than art spilling over
/// its neighbours.
fn draw_tab_art(sf: &mut Surface<'_>, (r, s): (Rect, i32), art: Art, look: Look) {
    if r.w < art.size() * s {
        return;
    }
    if look.chosen || look.hover {
        chrome::art_in(sf, r, art, look, s);
    } else {
        // Muted, as a tab's words are while it is not shown.
        let side = art.size() * s;
        let (x, y) = (r.x + (r.w - side) / 2, r.y + (r.h - side) / 2);
        let tones = crate::icons::Tones::one(theme::muted());
        crate::icons::draw_art(sf, x, y, art, tones, s);
    }
}

/// What a key on tab `n` of `count` asks. The menu's place is filled in by the caller.
fn tab_key(key: WidgetKey, n: usize, count: usize) -> Option<TabEvent> {
    match key {
        WidgetKey::Left => Some(TabEvent::Choose(cycle(n, count, true))),
        WidgetKey::Right => Some(TabEvent::Choose(cycle(n, count, false))),
        WidgetKey::Home => Some(TabEvent::Choose(0)),
        WidgetKey::End => Some(TabEvent::Choose(count.saturating_sub(1))),
        WidgetKey::Menu => Some(TabEvent::Menu { tab: n, at: (0, 0) }),
        WidgetKey::Enter => Some(TabEvent::Choose(n)),
        WidgetKey::Up | WidgetKey::Down => None,
    }
}

/// The shown tab joins the panel: an edge over and beside it, the panel's face, a unit of
/// Accent under its top edge and notched top corners. Others are words on the strip, outlined
/// under the pointer.
fn paint_tab(sf: &mut Surface<'_>, r: Rect, s: i32, look: Look) {
    if look.chosen {
        sf.fill(r, theme::edge());
        let inner = Rect::new(r.x + s, r.y + s, r.w - 2 * s, r.h - s);
        sf.fill(inner, theme::panel());
        sf.fill(Rect::new(inner.x, inner.y, inner.w, s), theme::accent());
    } else if look.hover {
        sf.fill(r, theme::edge_hover());
        sf.fill(r.inset(s), theme::header());
    } else {
        return;
    }
    sf.fill(Rect::new(r.x, r.y, s, s), theme::header());
    sf.fill(Rect::new(r.right() - s, r.y, s, s), theme::header());
}

/// The slot menu button and the fold chevron at the strip's right end.
fn strip_buttons(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    (menu, fold): (Rect, Rect),
    strip: Strip<'_>,
    shown: &str,
) -> Option<TabEvent> {
    let more = chrome::art_button(
        ui,
        sf,
        (id + MENU, menu),
        ActionIcon::More.art(),
        theme::header(),
        State::NONE,
    );
    if ui.naming() {
        ui.name(id + MENU, &format!("{shown} menu"));
    }
    let chevron = if strip.folded {
        MarkIcon::FoldRight
    } else {
        MarkIcon::ChevronDown
    };
    let open = State {
        expanded: Some(!strip.folded),
        ..State::NONE
    };
    let folded = chrome::art_button(
        ui,
        sf,
        (id + FOLD, fold),
        chevron.art(),
        theme::header(),
        open,
    );
    if ui.naming() {
        let verb = if strip.folded { "Unfold" } else { "Fold" };
        ui.name(id + FOLD, &format!("{verb} {shown}"));
    }
    let at = (menu.x, menu.bottom());
    let menu_event = more.then_some(TabEvent::Menu {
        tab: strip.active,
        at,
    });
    folded.then_some(TabEvent::Fold).or(menu_event)
}

#[cfg(test)]
#[path = "tabs_tests.rs"]
mod tests;
