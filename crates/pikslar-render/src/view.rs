//! The mapping between document and screen, and drawing the canvas through it.

use crate::blend::on_backdrop;

use std::cell::Cell;

const CHECKER_A: u32 = 0xff3d_3d3d;
const CHECKER_B: u32 = 0xff34_3434;
const CHECKER_SHIFT: i32 = 3;
/// The surround, until [`set_backdrop`] says otherwise: neutral grey, so colours read true.
pub const WORKSPACE: u32 = 0xff2b_2b2b;

/// What the canvas is drawn on: the surround outside the sprite, the two squares of the
/// checkerboard behind transparency, and the line that marks where the document ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backdrop {
    pub surround: u32,
    pub checker_a: u32,
    pub checker_b: u32,
    /// The document's edge: a line this many screen pixels wide, drawn on the surround just
    /// outside the document so it never covers a pixel of it. None when 0.
    pub edge_width: i32,
    pub edge: u32,
}

impl Backdrop {
    pub const NEUTRAL: Backdrop = Backdrop {
        surround: WORKSPACE,
        checker_a: CHECKER_A,
        checker_b: CHECKER_B,
        edge_width: 0,
        edge: WORKSPACE,
    };
}

thread_local! {
    static BACKDROP: Cell<Backdrop> = const { Cell::new(Backdrop::NEUTRAL) };
}

/// Draws canvases on `backdrop` from now on, on this thread (the one that draws the interface).
pub fn set_backdrop(backdrop: Backdrop) {
    BACKDROP.with(|b| b.set(backdrop));
}

/// The backdrop canvases are drawn on now, on this thread.
pub fn current_backdrop() -> Backdrop {
    BACKDROP.with(Cell::get)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn intersect(&self, o: Rect) -> Rect {
        let (x, y) = (self.x.max(o.x), self.y.max(o.y));
        let (r, b) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        Rect::new(x, y, (r - x).max(0), (b - y).max(0))
    }

    pub fn union(&self, o: Rect) -> Rect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return *self;
        }
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        Rect::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }

    pub fn inset(&self, amount: i32) -> Rect {
        Rect::new(
            self.x + amount,
            self.y + amount,
            self.w - amount * 2,
            self.h - amount * 2,
        )
    }
}

/// A mutable block of 32-bit pixels.
pub struct Surface<'a> {
    pub px: &'a mut [u32],
    pub w: i32,
    pub h: i32,
}

impl Surface<'_> {
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    pub fn fill(&mut self, rect: Rect, color: u32) {
        let r = rect.intersect(self.bounds());
        if r.is_empty() {
            return;
        }
        for row in self
            .px
            .chunks_exact_mut(self.w as usize)
            .skip(r.y as usize)
            .take(r.h as usize)
        {
            row[r.x as usize..r.right() as usize].fill(color);
        }
    }
}

/// Where the document sits on screen. One of `num` and `den` is always 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub num: i32,
    pub den: i32,
    pub x: i32,
    pub y: i32,
}

pub const ZOOM_STEPS: [(i32, i32); 12] = [
    (1, 8),
    (1, 4),
    (1, 2),
    (1, 1),
    (2, 1),
    (3, 1),
    (4, 1),
    (6, 1),
    (8, 1),
    (12, 1),
    (16, 1),
    (32, 1),
];

impl View {
    pub fn to_doc(&self, sx: i32, sy: i32) -> (i32, i32) {
        (
            ((sx - self.x) * self.den).div_euclid(self.num),
            ((sy - self.y) * self.den).div_euclid(self.num),
        )
    }

    pub fn to_screen(&self, r: Rect) -> Rect {
        let scale_down = |v: i32| v * self.num / self.den;
        let (x0, y0) = (
            (r.x * self.num).div_euclid(self.den) + self.x,
            (r.y * self.num).div_euclid(self.den) + self.y,
        );
        let (w, h) = (scale_down(r.w).max(1) + 1, scale_down(r.h).max(1) + 1);
        Rect::new(x0, y0, w, h)
    }

    /// The zoom step nearest the current scale.
    fn step(&self) -> usize {
        ZOOM_STEPS
            .iter()
            .position(|&(n, d)| n * self.den >= self.num * d)
            .unwrap_or(ZOOM_STEPS.len() - 1)
    }

    /// Moves `steps` zoom levels, keeping the document point under (sx, sy) in place.
    pub fn zoomed(&self, steps: i32, sx: i32, sy: i32) -> View {
        let last = ZOOM_STEPS.len() as i32 - 1;
        let (num, den) = ZOOM_STEPS[(self.step() as i32 + steps).clamp(0, last) as usize];
        let (dx, dy) = self.to_doc(sx, sy);
        View {
            num,
            den,
            x: sx - dx * num / den,
            y: sy - dy * num / den,
        }
    }

    /// The largest zoom that fits a document in `area`, centered.
    pub fn fit(doc_w: i32, doc_h: i32, area: Rect) -> View {
        let fits = |&&(n, d): &&(i32, i32)| doc_w * n / d <= area.w && doc_h * n / d <= area.h;
        let (num, den) = ZOOM_STEPS
            .iter()
            .rev()
            .find(fits)
            .copied()
            .unwrap_or(ZOOM_STEPS[0]);
        View {
            num,
            den,
            x: area.x + (area.w - doc_w * num / den) / 2,
            y: area.y + (area.h - doc_h * num / den) / 2,
        }
    }
}

/// Where a `src_w` by `src_h` document lies on screen through `view`: every screen pixel whose
/// document pixel is inside it.
pub fn document_on_screen(view: View, src_w: i32, src_h: i32) -> Rect {
    let span = |n: i32| (n * view.num + view.den - 1) / view.den;
    Rect::new(view.x, view.y, span(src_w), span(src_h))
}

/// A column of the map outside the document: on its edge line, or on the plain surround.
const ON_EDGE: i32 = -2;
const OUTSIDE: i32 = -1;

/// For each screen column of `area`, its document column, or [`ON_EDGE`] or [`OUTSIDE`].
fn column_map(xmap: &mut Vec<i32>, area: Rect, view: View, src_w: i32, ring: Rect) {
    xmap.clear();
    xmap.extend((area.x..area.right()).map(|sx| {
        let dx = view.to_doc(sx, 0).0;
        if dx >= 0 && dx < src_w {
            dx
        } else if sx >= ring.x && sx < ring.right() {
            ON_EDGE
        } else {
            OUTSIDE
        }
    }));
}

/// A screen row above or below the document: surround, with the edge line across the
/// document's width (and the line's corners) when the row is on it.
fn outside_row(row: &mut [u32], sy: i32, area: Rect, ring: Rect, back: Backdrop) {
    row.fill(back.surround);
    if sy >= ring.y && sy < ring.bottom() {
        let (from, to) = (ring.x.max(area.x), ring.right().min(area.right()));
        if from < to {
            row[(from - area.x) as usize..(to - area.x) as usize].fill(back.edge);
        }
    }
}

/// Draws the part of the canvas inside `clip`, with checkers behind transparency and the
/// backdrop's edge line around the document.
///
/// `src` is premultiplied BGRA of `src_w * src_h` pixels. `xmap` is a reusable buffer.
pub fn blit_canvas(
    src: &[u32],
    src_w: i32,
    src_h: i32,
    view: View,
    target: &mut Surface<'_>,
    clip: Rect,
    xmap: &mut Vec<i32>,
) {
    let document = document_on_screen(view, src_w, src_h);
    blit_framed(src, (src_w, src_h), view, target, (clip, document), xmap);
}

/// [`blit_canvas`] for a picture that is part of a larger document: the edge line goes around
/// `document`, the whole document's place on screen, rather than around the picture.
pub fn blit_framed(
    src: &[u32],
    (src_w, src_h): (i32, i32),
    view: View,
    target: &mut Surface<'_>,
    (clip, document): (Rect, Rect),
    xmap: &mut Vec<i32>,
) {
    let area = clip.intersect(target.bounds());
    if area.is_empty() {
        return;
    }
    let back = current_backdrop();
    let ring = document.inset(-back.edge_width.max(0));
    column_map(xmap, area, view, src_w, ring);
    let stride = target.w as usize;
    let mut previous = None;
    for sy in area.y..area.bottom() {
        let dy = view.to_doc(0, sy).1;
        let start = sy as usize * stride + area.x as usize;
        let same_checker = (sy >> CHECKER_SHIFT) == ((sy - 1) >> CHECKER_SHIFT);
        if dy < 0 || dy >= src_h {
            previous = None;
            let row = &mut target.px[start..start + area.w as usize];
            outside_row(row, sy, area, ring, back);
            continue;
        }
        if previous == Some(dy) && same_checker {
            target
                .px
                .copy_within(start - stride..start - stride + area.w as usize, start);
            continue;
        }
        previous = Some(dy);
        let row = &mut target.px[start..start + area.w as usize];
        let source = &src[dy as usize * src_w as usize..][..src_w as usize];
        for ((out, &dx), sx) in row.iter_mut().zip(xmap.iter()).zip(area.x..) {
            *out = if dx == ON_EDGE {
                back.edge
            } else if dx < 0 {
                back.surround
            } else {
                let checker = ((sx >> CHECKER_SHIFT) ^ (sy >> CHECKER_SHIFT)) & 1;
                on_backdrop(
                    source[dx as usize],
                    if checker == 0 {
                        back.checker_a
                    } else {
                        back.checker_b
                    },
                )
            };
        }
    }
}

/// Draws `w` by `h` premultiplied pixels at true size, one screen pixel for each, on the
/// canvas checkerboard with its top left at `at`. Row `y` of the picture starts at
/// `src[y * stride]`. Whatever falls outside `target` is left out.
pub fn blit_true_size(
    src: &[u32],
    stride: usize,
    (w, h): (usize, usize),
    target: &mut Surface<'_>,
    at: (i32, i32),
) {
    let area = Rect::new(at.0, at.1, w as i32, h as i32).intersect(target.bounds());
    let backdrop = current_backdrop();
    let tw = target.w as usize;
    for sy in area.y..area.bottom() {
        let row = &src[(sy - at.1) as usize * stride..];
        let start = sy as usize * tw;
        for sx in area.x..area.right() {
            let checker = ((sx >> CHECKER_SHIFT) ^ (sy >> CHECKER_SHIFT)) & 1;
            let back = if checker == 0 {
                backdrop.checker_a
            } else {
                backdrop.checker_b
            };
            target.px[start + sx as usize] = on_backdrop(row[(sx - at.0) as usize], back);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: u32 = 0xffff_0000;

    #[test]
    fn screen_and_document_agree_at_every_zoom() {
        for (num, den) in ZOOM_STEPS {
            let v = View {
                num,
                den,
                x: 13,
                y: -7,
            };
            let r = v.to_screen(Rect::new(40, 16, 1, 1));
            assert_eq!(v.to_doc(r.x, r.y), (40, 16), "{num}/{den}");
        }
    }

    #[test]
    fn positions_left_of_the_canvas_are_negative() {
        let v = View {
            num: 4,
            den: 1,
            x: 100,
            y: 0,
        };
        assert_eq!(v.to_doc(99, 0).0, -1);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let v = View {
            num: 4,
            den: 1,
            x: 100,
            y: 50,
        };
        let zoomed = v.zoomed(1, 300, 200);
        assert_eq!(zoomed.num, 6);
        assert_eq!(zoomed.to_doc(300, 200), v.to_doc(300, 200));
    }

    #[test]
    fn fit_centers_within_the_area() {
        let v = View::fit(100, 50, Rect::new(10, 10, 800, 600));
        assert_eq!((v.num, v.den), (8, 1));
        assert_eq!(v.x, 10);
        assert_eq!(v.y, 10 + (600 - 400) / 2);
    }

    fn render(view: View, clip: Rect) -> Vec<u32> {
        let mut px = vec![0u32; 16];
        let src = [RED, 0, 0, RED];
        let mut target = Surface {
            px: &mut px,
            w: 4,
            h: 4,
        };
        blit_canvas(&src, 2, 2, view, &mut target, clip, &mut Vec::new());
        px
    }

    #[test]
    fn magnified_pixels_cover_whole_blocks() {
        let px = render(
            View {
                num: 2,
                den: 1,
                x: 0,
                y: 0,
            },
            Rect::new(0, 0, 4, 4),
        );
        assert_eq!(&px[0..2], &[RED, RED]);
        assert_eq!(&px[4..6], &[RED, RED]);
        assert_eq!(&px[10..12], &[RED, RED]);
        assert_eq!(px[2], CHECKER_A);
    }

    #[test]
    fn nothing_is_drawn_outside_the_clip() {
        let px = render(
            View {
                num: 2,
                den: 1,
                x: 0,
                y: 0,
            },
            Rect::new(0, 0, 1, 1),
        );
        assert_eq!(px[0], RED);
        assert_eq!(px[1], 0);
        assert_eq!(px[4], 0);
    }

    #[test]
    fn the_backdrop_can_be_chosen_and_is_per_thread() {
        let (surround, a) = (0xff00_0000, 0xff12_3456);
        set_backdrop(Backdrop {
            surround,
            checker_a: a,
            checker_b: a,
            ..Backdrop::NEUTRAL
        });
        let view = View {
            num: 1,
            den: 1,
            x: 1,
            y: 1,
        };
        let px = render(view, Rect::new(0, 0, 4, 4));
        assert_eq!(px[0], surround, "outside the document");
        assert_eq!(px[6], a, "a transparent pixel shows the checker");
        let other = std::thread::spawn(current_backdrop).join().ok();
        assert_eq!(other, Some(Backdrop::NEUTRAL));
        set_backdrop(Backdrop::NEUTRAL);
    }

    /// A `w` by `h` opaque red document through `view` on a 40 by 40 surface, with an edge of
    /// `width` pixels, drawn whole or in the two halves a partial redraw would use.
    fn with_edge(view: View, (w, h): (i32, i32), width: i32, halves: bool) -> Vec<u32> {
        const EDGE: u32 = 0xff0b_0b0d;
        set_backdrop(Backdrop {
            edge_width: width,
            edge: EDGE,
            ..Backdrop::NEUTRAL
        });
        let src = vec![RED; (w * h) as usize];
        let mut px = vec![0u32; 1600];
        let mut target = Surface {
            px: &mut px,
            w: 40,
            h: 40,
        };
        let clips = if halves {
            vec![Rect::new(0, 0, 40, 17), Rect::new(0, 17, 40, 23)]
        } else {
            vec![Rect::new(0, 0, 40, 40)]
        };
        for clip in clips {
            blit_canvas(&src, w, h, view, &mut target, clip, &mut Vec::new());
        }
        set_backdrop(Backdrop::NEUTRAL);
        px
    }

    #[test]
    fn the_edge_rings_the_document_and_never_covers_it_at_any_zoom() {
        const EDGE: u32 = 0xff0b_0b0d;
        for (num, den) in [(1, 1), (3, 1), (1, 2)] {
            for width in [1, 2] {
                for halves in [false, true] {
                    let view = View {
                        num,
                        den,
                        x: 9,
                        y: 11,
                    };
                    let px = with_edge(view, (5, 4), width, halves);
                    let doc = document_on_screen(view, 5, 4);
                    let ring = doc.inset(-width);
                    for y in 0..40 {
                        for x in 0..40 {
                            let want = if doc.contains(x, y) {
                                RED
                            } else if ring.contains(x, y) {
                                EDGE
                            } else {
                                WORKSPACE
                            };
                            let got = px[(y * 40 + x) as usize];
                            assert_eq!(got, want, "{num}/{den} width {width} at {x},{y}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_document_on_screen_is_where_its_pixels_are_drawn() {
        for (num, den) in ZOOM_STEPS {
            let view = View {
                num,
                den,
                x: 3,
                y: 5,
            };
            let r = document_on_screen(view, 37, 21);
            assert_eq!(view.to_doc(r.x, r.y), (0, 0), "{num}/{den}");
            let before = view.to_doc(r.x - 1, r.y - 1);
            assert!(
                before.0 < 0 && before.1 < 0,
                "{num}/{den}: before {before:?}"
            );
            let last = view.to_doc(r.right() - 1, r.bottom() - 1);
            assert!(last.0 < 37 && last.1 < 21, "{num}/{den}: last {last:?}");
            let past = view.to_doc(r.right(), r.bottom());
            assert!(past.0 >= 37 && past.1 >= 21, "{num}/{den}: past {past:?}");
        }
    }

    #[test]
    fn outside_the_document_is_workspace() {
        let px = render(
            View {
                num: 1,
                den: 1,
                x: 1,
                y: 1,
            },
            Rect::new(0, 0, 4, 4),
        );
        assert_eq!(px[0], WORKSPACE);
        assert_eq!(px[5], RED);
        assert_eq!(px[15], WORKSPACE);
    }

    #[test]
    fn rect_operations() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(b), Rect::new(5, 5, 5, 5));
        assert_eq!(a.union(b), Rect::new(0, 0, 15, 15));
        assert!(a.intersect(Rect::new(20, 0, 1, 1)).is_empty());
    }

    #[test]
    fn a_true_size_picture_is_one_screen_pixel_each_on_the_checkerboard() {
        let mut px = vec![7u32; 20 * 20];
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 20,
        };
        // A 3 by 2 picture taken out of rows 4 wide; the empty pixel shows the checkerboard.
        let src = [
            0xff00_00ff,
            0,
            0xff00_ff00,
            99,
            0xffff_0000,
            0xffff_0000,
            0xffff_0000,
            99,
        ];
        blit_true_size(&src, 4, (3, 2), &mut sf, (18, 9));
        assert_eq!(px[9 * 20 + 18], on_backdrop(0xff00_00ff, CHECKER_B));
        assert_eq!(px[9 * 20 + 19], on_backdrop(0, CHECKER_B), "an empty pixel");
        assert_eq!(px[10 * 20 + 19], on_backdrop(0xffff_0000, CHECKER_B));
        assert_eq!(px[9 * 20 + 17], 7, "nothing drawn before it");
        assert_eq!(px[11 * 20 + 18], 7, "nor below it");
    }
}
