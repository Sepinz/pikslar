//! A segmented control: a row of choices of which one is chosen, such as Tiles / Pixels or a
//! selection mode (docs/ui-redesign.md 3.4). The chosen segment is filled in Accent.
//!
//! Keys on a focused segment: Left and Right choose the neighbour (wrapping) and take focus
//! along, Home and End the ends; Enter and Space choose the focused one.

use crate::chrome::{self, Look};
use crate::focus::WidgetKey;
use crate::font::{cap_height, draw_text, text_width};
use crate::icons::Art;
use crate::role::{Role, State};
use crate::text_fit::fit_to_width;
use crate::theme;
use crate::widgets::Ui;
use pikslar_render::{Rect, Surface};

/// What a segment shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show<'a> {
    Words(&'a str),
    Art(Art),
}

/// One choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment<'a> {
    /// Its name, listed for scripts whatever it shows.
    pub name: &'a str,
    pub show: Show<'a>,
}

/// Where segment `n` of `count` goes in `r`: even shares, the last taking what is left. Each
/// shares its one-unit edge with the next.
fn place(r: Rect, s: i32, n: i32, count: i32) -> Rect {
    let share = (r.w - s) / count.max(1);
    let x = r.x + n * share;
    let right = if n + 1 == count {
        r.right()
    } else {
        x + share + s
    };
    Rect::new(x, r.y, right - x, r.h)
}

/// Draws the segments in `r` over `parent`, `chosen` filled, their ids from `id + 1`. Returns
/// the segment chosen this pass, if it is another.
pub fn segmented(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    (id, r): (u32, Rect),
    parent: u32,
    segments: &[Segment<'_>],
    chosen: usize,
) -> Option<usize> {
    let s = ui.scale.max(1);
    ui.container(id, r, Role::Group, State::NONE);
    let count = segments.len();
    let mut out = None;
    for (n, seg) in segments.iter().enumerate() {
        let cell = place(r, s, n as i32, count as i32);
        let seg_id = id + 1 + n as u32;
        let state = ui.interact(seg_id, cell);
        ui.focus.register_keys(seg_id);
        ui.describe(Role::Toggle, State::chosen(n == chosen));
        ui.name(seg_id, seg.name);
        let look = Look::of(state, n == chosen, false);
        paint(sf, cell, s, look);
        show(ui, sf, cell, seg.show, look);
        if state.clicked {
            out = Some(n);
        }
        if let Some(to) = ui
            .focus
            .take_press(seg_id)
            .and_then(|p| key(p.key, n, count))
        {
            ui.focus.set(id + 1 + to as u32);
            out = Some(to);
        }
    }
    chrome::notch(sf, r, s, parent);
    // Rings last, so the next segment's edge does not cut them.
    for n in 0..count {
        ui.ring(sf, id + 1 + n as u32, place(r, s, n as i32, count as i32));
    }
    out.filter(|&n| n != chosen)
}

/// One segment's face: an edge that lightens under the pointer, the face, and the lip while
/// raised.
fn paint(sf: &mut Surface<'_>, cell: Rect, s: i32, look: Look) {
    let edge = if look.hover {
        theme::edge_hover()
    } else {
        theme::edge()
    };
    sf.fill(cell, edge);
    let inner = cell.inset(s);
    let face = look.surface();
    sf.fill(inner, face);
    if !look.chosen && !look.pressed && theme::lip() != face {
        sf.fill(Rect::new(inner.x, inner.y, inner.w, s), theme::lip());
    }
}

fn show(ui: &Ui, sf: &mut Surface<'_>, cell: Rect, what: Show<'_>, look: Look) {
    let s = ui.scale.max(1);
    match what {
        Show::Art(art) => chrome::art_in(sf, cell, art, look, s),
        Show::Words(words) => {
            let ts = ui.text_scale();
            let words = fit_to_width(words, cell.w - 4 * s, ts);
            let x = cell.x + (cell.w - text_width(&words, ts)) / 2;
            let y = cell.y + (cell.h - cap_height() * ts) / 2 + look.drop() * s;
            draw_text(sf, x, y, &words, look.ink(), ts);
        }
    }
}

/// The segment a key on segment `n` of `count` chooses.
fn key(key: WidgetKey, n: usize, count: usize) -> Option<usize> {
    match key {
        WidgetKey::Left => Some((n + count - 1) % count),
        WidgetKey::Right => Some((n + 1) % count),
        WidgetKey::Home => Some(0),
        WidgetKey::End => Some(count - 1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::Press;
    use crate::icons::ActionIcon;
    use crate::theme::Theme;

    const W: i32 = 130;
    const H: i32 = 40;
    const R: Rect = Rect::new(10, 10, 91, 20);

    fn segs() -> [Segment<'static>; 3] {
        [
            Segment {
                name: "Replace",
                show: Show::Words("Rep"),
            },
            Segment {
                name: "Add",
                show: Show::Art(ActionIcon::Plus.art()),
            },
            Segment {
                name: "Subtract",
                show: Show::Art(ActionIcon::Minus.art()),
            },
        ]
    }

    fn pass(ui: &mut Ui, px: &mut [u32], chosen: usize) -> Option<usize> {
        let mut sf = Surface { px, w: W, h: H };
        ui.begin();
        ui.focus.collect(true);
        let out = segmented(ui, &mut sf, (20, R), 7, &segs(), chosen);
        ui.focus.collect(false);
        ui.end();
        out
    }

    #[test]
    fn segments_share_the_width_and_their_edges() {
        let s = 1;
        let a = place(R, s, 0, 3);
        let b = place(R, s, 1, 3);
        let c = place(R, s, 2, 3);
        assert_eq!((a.x, a.right()), (10, 41));
        assert_eq!(b.x, a.right() - 1, "one shared unit of edge");
        assert_eq!(c.right(), R.right());
    }

    #[test]
    fn a_click_chooses_another_segment_but_not_the_chosen_one_again() {
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        ui.mouse = (55, 20);
        ui.pressed = true;
        ui.released = true;
        assert_eq!(pass(&mut ui, &mut px, 0), Some(1));
        ui.pressed = true;
        ui.released = true;
        assert_eq!(pass(&mut ui, &mut px, 1), None);
    }

    #[test]
    fn arrows_choose_the_neighbour_and_move_focus_with_it() {
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        pass(&mut ui, &mut px, 0);
        ui.focus.set(21);
        ui.focus.request_press(Press::plain(WidgetKey::Right));
        assert_eq!(pass(&mut ui, &mut px, 0), Some(1));
        assert_eq!(ui.focus.focused(), Some(22));
        ui.focus.request_press(Press::plain(WidgetKey::Left));
        assert_eq!(pass(&mut ui, &mut px, 1), Some(0));
        ui.focus.request_press(Press::plain(WidgetKey::Left));
        assert_eq!(pass(&mut ui, &mut px, 0), Some(2), "wraps");
        ui.focus.request_press(Press::plain(WidgetKey::Home));
        assert_eq!(pass(&mut ui, &mut px, 2), Some(0));
        ui.focus.set(23);
        ui.focus.request_activate();
        assert_eq!(pass(&mut ui, &mut px, 0), Some(2), "Enter chooses");
    }

    #[test]
    fn the_chosen_segment_is_filled_the_outer_corners_notched_and_focus_ringed() {
        theme::set(Theme::LIGHTBOX);
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        pass(&mut ui, &mut px, 1);
        let at = |px: &[u32], x: i32, y: i32| px[(y * W + x) as usize];
        assert_eq!(at(&px, 45, 12), theme::accent(), "filled, no lip");
        assert_eq!(
            at(&px, 15, 11),
            theme::lip(),
            "the others raised with a lip"
        );
        assert_eq!(at(&px, 10, 10), 7, "outer corner in the parent's colour");
        assert_eq!(
            at(&px, 40, 10),
            theme::edge(),
            "the shared edge is not notched"
        );
        ui.focus.set(21);
        pass(&mut ui, &mut px, 1);
        assert_eq!(at(&px, 20, 8), theme::focus());
        theme::set(Theme::NORMAL);
    }

    #[test]
    fn segments_are_toggles_in_a_group() {
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        ui.record_names();
        pass(&mut ui, &mut px, 2);
        let text = ui.layout().unwrap_or_default();
        assert!(text.contains("\t\tgroup\t\n"), "{text}");
        assert!(text.contains("\tSubtract\ttoggle\tchosen\n"), "{text}");
        assert!(text.contains("\tAdd\ttoggle\t\n"), "{text}");
    }
}
