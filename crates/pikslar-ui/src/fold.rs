//! The header of a panel section that folds: an arrow and the section's title, which together
//! open or fold it.

use crate::font::{cap_height, draw_text, text_width};
use crate::theme;
use crate::widgets::Ui;
use pikslar_render::{Rect, Surface};

/// Units the arrow takes before the title.
pub const ARROW_ROOM: i32 = 14;

/// Draws a section header in `r`: an arrow pointing down while `open` and right while folded,
/// then `title`, cut short with an ellipsis when it does not fit, so what is drawn beside the
/// header is never drawn over. Returns true when it was clicked, to fold or open the section.
pub fn fold_header(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    r: Rect,
    title: &str,
    open: bool,
) -> bool {
    let state = ui.interact(id, r);
    ui.describe(
        crate::role::Role::Button,
        crate::role::State {
            expanded: Some(open),
            ..crate::role::State::NONE
        },
    );
    ui.name(id, if title.is_empty() { "Fold" } else { title });
    if state.hover {
        sf.fill(r, theme::raised());
    }
    let s = ui.scale;
    let ink = if state.hover {
        theme::text()
    } else {
        theme::muted()
    };
    // Drawn at the text's scale, so it reads beside the title.
    draw_arrow(sf, (r.x + 2 * s, r.y + r.h / 2), open, ink, ui.text_scale());
    let ts = ui.text_scale();
    let y = r.y + (r.h - cap_height() * ts) / 2;
    let x = title_x(r.x, s, ts);
    let room = r.right() - x - 3 * s;
    // The title is whole or left out: a stub of dots says nothing, and the tab names the panel.
    if text_width(title, ts) <= room {
        draw_text(sf, x, y, title, ink, ts);
    }
    state.clicked
}

/// Where the title starts: [`ARROW_ROOM`] units in, or further when a large face draws a larger
/// arrow, so a gap of four units always parts them.
pub fn title_x(x: i32, s: i32, ts: i32) -> i32 {
    x + (ARROW_ROOM * s).max(2 * s + 5 * ts + 4 * s)
}

/// A small solid triangle, five steps of `s` across, whose left edge is at `x` and middle at `y`.
pub fn draw_arrow(sf: &mut Surface<'_>, (x, y): (i32, i32), open: bool, ink: u32, s: i32) {
    for step in 0..3 {
        let long = (5 - 2 * step) * s;
        let r = if open {
            Rect::new(x + step * s, y - s + step * s, long, s)
        } else {
            Rect::new(x + step * s, y - 2 * s + step * s, s, long)
        };
        sf.fill(r, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(px: &[u32], w: i32) -> Vec<(i32, i32)> {
        (0..px.len() as i32)
            .filter(|&i| px[i as usize] == 7)
            .map(|i| (i % w, i / w))
            .collect()
    }

    #[test]
    fn the_arrow_points_down_when_open_and_right_when_folded() {
        let mut px = vec![0u32; 10 * 10];
        let mut sf = Surface {
            px: &mut px,
            w: 10,
            h: 10,
        };
        draw_arrow(&mut sf, (1, 5), true, 7, 1);
        let open = lit(&px, 10);
        assert_eq!(open.len(), 9);
        assert!(
            open.contains(&(1, 4)) && open.contains(&(5, 4)),
            "wide at the top"
        );
        assert!(open.contains(&(3, 6)), "a point at the bottom");
        let mut px = vec![0u32; 10 * 10];
        let mut sf = Surface {
            px: &mut px,
            w: 10,
            h: 10,
        };
        draw_arrow(&mut sf, (1, 5), false, 7, 1);
        let folded = lit(&px, 10);
        assert_eq!(folded.len(), 9);
        assert!(
            folded.contains(&(1, 3)) && folded.contains(&(1, 7)),
            "tall at the left"
        );
        assert!(folded.contains(&(3, 5)), "a point at the right");
    }

    #[test]
    fn a_click_on_the_header_reports_it() {
        let mut ui = Ui::new();
        let mut px = vec![0u32; 100 * 30];
        let mut sf = Surface {
            px: &mut px,
            w: 100,
            h: 30,
        };
        let r = Rect::new(0, 0, 80, 22);
        ui.mouse = (5, 5);
        ui.pressed = true;
        ui.down = true;
        ui.begin();
        assert!(!fold_header(&mut ui, &mut sf, 9, r, "Palette", true));
        ui.end();
        ui.down = false;
        ui.released = true;
        ui.begin();
        assert!(fold_header(&mut ui, &mut sf, 9, r, "Palette", true));
        ui.end();
    }

    /// The arrow grows with the face; the title keeps four units from it (final review, 10).
    #[test]
    fn the_title_keeps_a_gap_from_the_arrow_in_every_face() {
        for s in [1, 2, 3] {
            for ts in [s, 2 * s] {
                let arrow_right = 2 * s + 5 * ts;
                assert!(title_x(0, s, ts) - arrow_right >= 4 * s, "{s} {ts}");
            }
        }
        assert_eq!(
            title_x(10, 1, 1),
            10 + ARROW_ROOM,
            "the Text face keeps its place"
        );
    }

    #[test]
    fn a_title_wider_than_its_header_is_cut_short_inside_it() {
        let mut ui = Ui::new();
        let mut px = vec![0u32; 100 * 30];
        let mut sf = Surface {
            px: &mut px,
            w: 100,
            h: 30,
        };
        let r = Rect::new(0, 0, 40, 22);
        ui.begin();
        fold_header(&mut ui, &mut sf, 9, r, "Layers and more", true);
        ui.end();
        let outside = (0..30).flat_map(|y| (40..100).map(move |x| (x, y)));
        let inked: Vec<(i32, i32)> = outside
            .filter(|&(x, y)| px[(y * 100 + x) as usize] != 0)
            .collect();
        assert!(inked.is_empty(), "ink past the header: {inked:?}");
    }
}
