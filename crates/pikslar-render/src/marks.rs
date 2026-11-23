//! Lines drawn over the canvas: the selection's marching ants and small frames.
//!
//! These write straight to the screen and are never part of the composited image.

use crate::view::{Rect, Surface, View};
use pikslar_core::Edge;

const ANT_LIGHT: u32 = 0xffff_ffff;
const ANT_DARK: u32 = 0xff00_0000;
const DASH_SHIFT: i32 = 2;

#[inline]
fn screen_x(view: &View, x: i32) -> i32 {
    (x * view.num).div_euclid(view.den) + view.x
}

#[inline]
fn screen_y(view: &View, y: i32) -> i32 {
    (y * view.num).div_euclid(view.den) + view.y
}

#[inline]
fn put(sf: &mut Surface<'_>, clip: Rect, x: i32, y: i32, color: u32) {
    if clip.contains(x, y) {
        sf.px[(y * sf.w + x) as usize] = color;
    }
}

/// Draws an outline as black and white dashes, which show up on any art.
pub fn draw_ants(sf: &mut Surface<'_>, view: &View, area: Rect, edges: &[Edge], phase: i32) {
    let clip = area.intersect(sf.bounds());
    if clip.is_empty() {
        return;
    }
    for edge in edges {
        let (x, y) = (screen_x(view, edge.x), screen_y(view, edge.y));
        let (end_x, end_y) = if edge.horizontal {
            (screen_x(view, edge.x + edge.len), y)
        } else {
            (x, screen_y(view, edge.y + edge.len))
        };
        // Skip lines entirely outside the area without walking them.
        if end_x < clip.x || x >= clip.right() || end_y < clip.y || y >= clip.bottom() {
            continue;
        }
        let (from, to) = if edge.horizontal {
            (x.max(clip.x), (end_x + 1).min(clip.right()))
        } else {
            (y.max(clip.y), (end_y + 1).min(clip.bottom()))
        };
        for t in from..to {
            let (px, py) = if edge.horizontal { (t, y) } else { (x, t) };
            let dark = ((px + py + phase) >> DASH_SHIFT) & 1 == 0;
            put(sf, clip, px, py, if dark { ANT_DARK } else { ANT_LIGHT });
        }
    }
}

/// Draws a one-pixel frame around a document area.
pub fn draw_frame(sf: &mut Surface<'_>, view: &View, area: Rect, doc: Rect, color: u32) {
    let (x0, y0) = (screen_x(view, doc.x), screen_y(view, doc.y));
    let (x1, y1) = (
        screen_x(view, doc.right()).max(x0 + 2),
        screen_y(view, doc.bottom()).max(y0 + 2),
    );
    let screen = Rect::new(x0, y0, x1 - x0, y1 - y0);
    for line in [
        Rect::new(screen.x, screen.y, screen.w, 1),
        Rect::new(screen.x, screen.bottom() - 1, screen.w, 1),
        Rect::new(screen.x, screen.y, 1, screen.h),
        Rect::new(screen.right() - 1, screen.y, 1, screen.h),
    ] {
        sf.fill(line.intersect(area), color);
    }
}

/// Draws a freehand path while it is still being traced.
pub fn draw_path(sf: &mut Surface<'_>, view: &View, area: Rect, path: &[(i32, i32)], phase: i32) {
    let clip = area.intersect(sf.bounds());
    let half = (view.num / view.den).max(1) / 2;
    let centers = path
        .iter()
        .map(|&(x, y)| (screen_x(view, x) + half, screen_y(view, y) + half));
    let mut previous: Option<(i32, i32)> = None;
    for point in centers {
        let start = previous.unwrap_or(point);
        pikslar_core::editor::for_line(start.0, start.1, point.0, point.1, |x, y| {
            let dark = ((x + y + phase) >> DASH_SHIFT) & 1 == 0;
            put(sf, clip, x, y, if dark { ANT_DARK } else { ANT_LIGHT });
        });
        previous = Some(point);
    }
}

/// The screen rectangle of a document area, for hit tests and redraw regions.
pub fn screen_rect(view: &View, x: i32, y: i32, w: i32, h: i32) -> Rect {
    let (x0, y0) = (screen_x(view, x), screen_y(view, y));
    Rect::new(
        x0,
        y0,
        screen_x(view, x + w) - x0,
        screen_y(view, y + h) - y0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::Mask;

    const VIEW: View = View {
        num: 4,
        den: 1,
        x: 10,
        y: 10,
    };

    #[test]
    fn ants_land_on_the_shape_boundary_and_never_leave_the_area() {
        let mut px = vec![0u32; 200 * 200];
        let mut sf = Surface {
            px: &mut px,
            w: 200,
            h: 200,
        };
        let edges = Mask::rect(0, 0, 5, 5).outline();
        let area = Rect::new(0, 0, 20, 200);
        draw_ants(&mut sf, &VIEW, area, &edges, 0);
        assert_ne!(px[10 * 200 + 12], 0, "top edge inside the area");
        assert_eq!(px[10 * 200 + 25], 0, "clipped at the area's edge");
        assert_eq!(px[12 * 200 + 12], 0, "interior untouched");
    }

    #[test]
    fn marks_far_off_screen_are_harmless() {
        let mut px = vec![0u32; 100];
        let mut sf = Surface {
            px: &mut px,
            w: 10,
            h: 10,
        };
        let area = sf.bounds();
        let edges = Mask::rect(-9000, 9000, 50, 50).outline();
        draw_ants(&mut sf, &VIEW, area, &edges, 3);
        draw_frame(&mut sf, &VIEW, area, Rect::new(5000, 5000, 3, 3), 1);
        draw_path(&mut sf, &VIEW, area, &[(-500, -500), (-499, 900)], 0);
        assert!(px.iter().all(|&p| p == 0));
    }

    #[test]
    fn a_frame_stays_visible_when_zoomed_far_out() {
        let far = View {
            num: 1,
            den: 8,
            x: 0,
            y: 0,
        };
        let r = screen_rect(&far, 0, 0, 8, 8);
        assert_eq!((r.w, r.h), (1, 1));
        let mut px = vec![0u32; 100];
        let mut sf = Surface {
            px: &mut px,
            w: 10,
            h: 10,
        };
        let area = sf.bounds();
        draw_frame(&mut sf, &far, area, Rect::new(0, 0, 1, 1), 9);
        assert_eq!(px[0], 9);
        assert_eq!(px[1], 9);
    }
}
