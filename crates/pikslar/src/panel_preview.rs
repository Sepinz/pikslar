//! The Preview panel: the frame at true size, centred over the transparency checker, so an
//! empty sprite shows the checker and a drawn one shows itself, one screen pixel to a pixel.

use crate::app::App;
use crate::true_preview::shown_part;
use pikslar_render::{current_backdrop, Rect, Surface};

/// Square size of the checker, in screen pixels (the canvas draws eight).
const SQUARE: i32 = 8;

/// The checker over `r`, in the backdrop's two greys. No allocation.
pub(crate) fn fill_checker(sf: &mut Surface<'_>, r: Rect) {
    let area = r.intersect(sf.bounds());
    let back = current_backdrop();
    let w = sf.w as usize;
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let dark = ((x - r.x) / SQUARE + (y - r.y) / SQUARE) % 2 != 0;
            sf.px[y as usize * w + x as usize] = if dark { back.checker_b } else { back.checker_a };
        }
    }
}

impl App {
    /// Draws the Preview panel in `body`.
    pub(crate) fn draw_preview_panel(&mut self, sf: &mut Surface<'_>, body: Rect) {
        if body.w <= 0 || body.h <= 0 {
            return;
        }
        fill_checker(sf, body);
        let doc = self.editor.doc();
        let part = shown_part((doc.width, doc.height), (body.w as usize, body.h as usize));
        if part.2 == 0 || part.3 == 0 {
            return;
        }
        let at = Rect::new(
            body.x + (body.w - part.2 as i32) / 2,
            body.y + (body.h - part.3 as i32) / 2,
            part.2 as i32,
            part.3 as i32,
        );
        self.blit_preview(sf, at, part);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_app::app;
    use crate::workspace::model::Preset;

    #[test]
    fn an_empty_sprite_shows_the_checker_in_the_preview_panel() {
        let (w, h) = (1366, 705);
        let mut a = app(1);
        a.resized(w, h, 1);
        a.choose_preset(Preset::Animate);
        let mut px = vec![0u32; (w * h) as usize];
        for _ in 0..2 {
            let mut surface = Surface { px: &mut px, w, h };
            a.draw_interface(
                &mut surface,
                pikslar_platform::Window::detached(),
                &mut Vec::new(),
            );
        }
        let back = current_backdrop();
        let right = a.layout.right;
        let count = |c: u32| {
            (right.y..right.bottom())
                .flat_map(|y| (right.x..right.right()).map(move |x| (x, y)))
                .filter(|&(x, y)| px[(y * w + x) as usize] == c)
                .count()
        };
        assert!(count(back.checker_a) > 500, "light squares in the dock");
        assert!(count(back.checker_b) > 500, "dark squares in the dock");
    }

    #[test]
    fn the_checker_alternates_every_eight_pixels() {
        let mut px = vec![0u32; 20 * 20];
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 20,
        };
        fill_checker(&mut sf, Rect::new(2, 2, 16, 16));
        let back = current_backdrop();
        assert_eq!(px[2 * 20 + 2], back.checker_a);
        assert_eq!(px[2 * 20 + 10], back.checker_b);
        assert_eq!(px[10 * 20 + 2], back.checker_b);
        assert_eq!(px[10 * 20 + 10], back.checker_a);
        assert_eq!(px[0], 0, "outside is untouched");
    }
}
