//! A rail: a folded dock area, shown as a 24-unit strip of its panels' 12-unit icons
//! (docs/ui-redesign.md 3.3). Activating an icon asks to unfold the area on that panel.
//!
//! Keys on a focused icon: the arrows along the rail move focus to the neighbour (wrapping),
//! Home and End to the ends; Enter and Space activate it.

use crate::chrome::{self, Look};
use crate::focus::WidgetKey;
use crate::icons::Art;
use crate::role::{Role, State};
use crate::theme;
use crate::widgets::Ui;
use pikslar_render::{Rect, Surface};

/// The rail's thickness, and each icon's place along it, in units: a 24-unit target.
pub const RAIL: i32 = 24;
/// The side of an icon's face inside its place, in units.
const FACE: i32 = 20;
/// The side of a colour swatch, in units.
const SWATCH: i32 = 12;

/// What an icon of the rail shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailIcon {
    Art(Art),
    /// A colour, edged: the Color panel shows the current colour.
    Swatch(u32),
}

/// One panel of the folded area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailItem<'a> {
    /// The panel's name, which is the icon's name too: never an icon alone.
    pub name: &'a str,
    pub icon: RailIcon,
}

/// Where icon `n` goes on a rail in `r`.
fn place(r: Rect, s: i32, n: i32, vertical: bool) -> Rect {
    let side = RAIL * s;
    if vertical {
        Rect::new(r.x, r.y + n * side, side, side)
    } else {
        Rect::new(r.x + n * side, r.y, side, side)
    }
}

/// How many icons fit along `r`.
fn fitting(r: Rect, s: i32, vertical: bool) -> usize {
    let along = if vertical { r.h } else { r.w };
    (along / (RAIL * s)).max(0) as usize
}

/// Draws the rail in `r`, its icons stacked down it when `vertical` or else side by side, the
/// icons taking ids from `id + 1`. Returns the panel to unfold the area on.
pub fn rail(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    r: Rect,
    vertical: bool,
    items: &[RailItem<'_>],
) -> Option<usize> {
    let s = ui.scale.max(1);
    let folded = State {
        expanded: Some(false),
        ..State::NONE
    };
    ui.container(id, r, Role::Group, folded);
    sf.fill(r, theme::header());
    let count = items.len().min(fitting(r, s, vertical));
    let mut chosen = None;
    for (n, item) in items.iter().take(count).enumerate() {
        let cell = place(r, s, n as i32, vertical);
        let item_id = id + 1 + n as u32;
        let state = ui.interact(item_id, cell);
        ui.focus.register_keys(item_id);
        ui.describe(Role::Button, folded);
        ui.name(item_id, item.name);
        let look = Look::of(state, false, false);
        let face = cell.inset((RAIL - FACE) / 2 * s);
        chrome::face(sf, face, s, theme::header(), look);
        draw_icon(sf, face, item.icon, look, s);
        ui.ring(sf, item_id, face);
        if let Some(p) = ui.focus.take_press(item_id) {
            if let Some(to) = rail_key(p.key, n, count, vertical) {
                ui.focus.set(id + 1 + to as u32);
            }
        }
        if state.clicked {
            chosen = Some(n);
        }
    }
    chosen
}

/// The icon a key on icon `n` of `count` moves focus to.
fn rail_key(key: WidgetKey, n: usize, count: usize, vertical: bool) -> Option<usize> {
    let back = (n + count - 1) % count;
    let next = (n + 1) % count;
    match (key, vertical) {
        (WidgetKey::Up, true) | (WidgetKey::Left, false) => Some(back),
        (WidgetKey::Down, true) | (WidgetKey::Right, false) => Some(next),
        (WidgetKey::Home, _) => Some(0),
        (WidgetKey::End, _) => Some(count - 1),
        _ => None,
    }
}

fn draw_icon(sf: &mut Surface<'_>, face: Rect, icon: RailIcon, look: Look, s: i32) {
    match icon {
        RailIcon::Art(art) => chrome::art_in(sf, face, art, look, s),
        RailIcon::Swatch(color) => {
            let side = SWATCH * s;
            let at = Rect::new(
                face.x + (face.w - side) / 2,
                face.y + (face.h - side) / 2 + look.drop() * s,
                side,
                side,
            );
            sf.fill(at, theme::edge());
            sf.fill(at.inset(s), color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::Press;
    use crate::icons::ActionIcon;
    use crate::theme::Theme;

    const W: i32 = 40;
    const H: i32 = 100;
    const R: Rect = Rect::new(8, 0, 24, 100);
    fn items() -> [RailItem<'static>; 3] {
        [
            RailItem {
                name: "Layers",
                icon: RailIcon::Art(ActionIcon::Stack.art()),
            },
            RailItem {
                name: "Color",
                icon: RailIcon::Swatch(0xff12_3456),
            },
            RailItem {
                name: "Palette",
                icon: RailIcon::Art(ActionIcon::LivePalette.art()),
            },
        ]
    }

    fn pass(ui: &mut Ui, px: &mut [u32]) -> Option<usize> {
        let mut sf = Surface { px, w: W, h: H };
        ui.begin();
        ui.focus.collect(true);
        let out = rail(ui, &mut sf, 50, R, true, &items());
        ui.focus.collect(false);
        ui.end();
        out
    }

    #[test]
    fn icons_sit_on_a_24_unit_pitch_and_a_click_asks_to_unfold_on_that_panel() {
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        pass(&mut ui, &mut px);
        assert_eq!(ui.widget_at(20, 30), 52, "the second place spans 24 to 47");
        assert_eq!(
            ui.widget_at(20, 90),
            50,
            "past the icons is the rail itself"
        );
        ui.mouse = (20, 60);
        ui.pressed = true;
        ui.released = true;
        assert_eq!(pass(&mut ui, &mut px), Some(2));
    }

    #[test]
    fn arrows_walk_the_icons_and_enter_activates_the_focused_one() {
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        pass(&mut ui, &mut px);
        ui.focus.set(51);
        for (key, want) in [
            (WidgetKey::Down, 52),
            (WidgetKey::Down, 53),
            (WidgetKey::Down, 51),
            (WidgetKey::Up, 53),
            (WidgetKey::Home, 51),
            (WidgetKey::End, 53),
        ] {
            ui.focus.request_press(Press::plain(key));
            assert_eq!(pass(&mut ui, &mut px), None, "moving is not choosing");
            assert_eq!(ui.focus.focused(), Some(want), "{key:?}");
        }
        ui.focus.request_activate();
        assert_eq!(pass(&mut ui, &mut px), Some(2));
    }

    #[test]
    fn the_colour_panel_shows_the_colour_and_focus_rings_in_cyan() {
        theme::set(Theme::LIGHTBOX);
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        pass(&mut ui, &mut px);
        let at = |px: &[u32], x: i32, y: i32| px[(y * W + x) as usize];
        assert_eq!(
            at(&px, 20, 36),
            0xff12_3456,
            "swatch in the middle of place 2"
        );
        assert_eq!(at(&px, 14, 30), theme::edge(), "edged");
        ui.focus.set(52);
        pass(&mut ui, &mut px);
        assert_eq!(
            at(&px, 20, 24),
            theme::focus(),
            "ring round the face, outside it"
        );
        theme::set(Theme::NORMAL);
    }

    #[test]
    fn the_rail_is_a_folded_group_of_named_buttons() {
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        ui.record_names();
        pass(&mut ui, &mut px);
        let text = ui.layout().unwrap_or_default();
        assert!(text.contains("\tgroup\tcollapsed\n"), "{text}");
        assert!(text.contains("\tColor\tbutton\tcollapsed\n"), "{text}");
    }

    #[test]
    fn icons_that_do_not_fit_are_left_out() {
        let mut px = vec![0; (W * H) as usize];
        let mut sf = Surface {
            px: &mut px,
            w: W,
            h: H,
        };
        let mut ui = Ui::new();
        ui.begin();
        rail(
            &mut ui,
            &mut sf,
            50,
            Rect::new(8, 0, 24, 50),
            true,
            &items(),
        );
        assert_eq!(ui.widgets().len(), 3, "the rail and two icons");
    }
}
