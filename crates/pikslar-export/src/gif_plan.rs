//! What each frame of a GIF draws, and what it leaves for the next, so every frame shows exactly
//! its pixels while drawing as few as it safely can.
//!
//! A frame draws, in one rectangle, the pixels that differ from what the canvas shows when it
//! starts; the rest of the canvas is kept. A drawn pixel cannot make the canvas clear again, so
//! when the next frame clears pixels this frame shows, this frame's rectangle grows to cover them
//! and is cleared after it is shown, and the next frame draws again whatever it keeps there. A
//! loop's last frame is followed by its first: the first frame draws every pixel it shows, so the
//! last clears only what the first does not show, and the first frame is the same whether a
//! player clears the canvas between plays or not.
//!
//! The frames are compared at the document's own size, a band at a time in play order, holding
//! three bands (the first frame's, kept for the wrap) and what was found in each frame.

use crate::Picture;
use pikslar_format::gif_write::{Area, Disposal};
use pikslar_render::BAND_ROWS;

/// The smallest rectangle holding some pixels, as left, top, right and bottom, the last two
/// past the edge; None for no pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bounds(Option<[usize; 4]>);

impl Bounds {
    pub(crate) fn add(&mut self, x: usize, y: usize) {
        let [l, t, r, b] = self.0.unwrap_or([x, y, x + 1, y + 1]);
        self.0 = Some([l.min(x), t.min(y), r.max(x + 1), b.max(y + 1)]);
    }

    /// Adds the pixels of row `y` from `first` to `last`, both included, if there are any.
    pub(crate) fn add_row(&mut self, y: usize, span: Option<(usize, usize)>) {
        if let Some((first, last)) = span {
            self.add(first, y);
            self.add(last, y);
        }
    }

    fn union(self, other: Bounds) -> Bounds {
        match (self.0, other.0) {
            (Some([l, t, r, b]), Some([ol, ot, or, ob])) => {
                Bounds(Some([l.min(ol), t.min(ot), r.max(or), b.max(ob)]))
            }
            _ => Bounds(self.0.or(other.0)),
        }
    }

    /// The part of `self` inside `other`: a rectangle holding every pixel the two share.
    fn meet(self, other: Bounds) -> Bounds {
        let (Some([l, t, r, b]), Some([ol, ot, or, ob])) = (self.0, other.0) else {
            return Bounds(None);
        };
        let [l, t, r, b] = [l.max(ol), t.max(ot), r.min(or), b.min(ob)];
        Bounds((l < r && t < b).then_some([l, t, r, b]))
    }

    pub(crate) fn is_empty(self) -> bool {
        self.0.is_none()
    }
}

/// What one frame has, against the frame shown before it in play order (a clear canvas for the
/// first).
#[derive(Clone, Copy, Debug, Default)]
struct Seen {
    /// Opaque pixels that are not the colour the frame before shows there.
    changed: Bounds,
    /// Pixels the frame before shows and this one does not.
    vanished: Bounds,
    /// Every opaque pixel.
    opaque: Bounds,
    /// Whether any pixel is clear.
    clear: bool,
}

/// One frame of a GIF as it is drawn, at the document's own size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) area: Area,
    pub(crate) after: Disposal,
}

/// Records in `seen` how `now`, rows from `top` of a frame `width` wide, differs from
/// `before`, the same rows of the frame shown before it. Each row is searched from both ends.
fn compare(now: &[u32], before: &[u32], (width, top): (usize, usize), seen: &mut Seen) {
    let rows = now.chunks_exact(width).zip(before.chunks_exact(width));
    for (y, (now, before)) in (top..).zip(rows) {
        let pairs = || now.iter().zip(before);
        seen.opaque.add_row(y, span(pairs(), |n, _| n >> 24 != 0));
        seen.changed
            .add_row(y, span(pairs(), |n, b| n >> 24 != 0 && n != b));
        seen.vanished
            .add_row(y, span(pairs(), |n, b| n >> 24 == 0 && b >> 24 != 0));
        seen.clear = seen.clear || now.iter().any(|&n| n >> 24 == 0);
    }
}

/// The first and last places in a row where `hit` holds of the pixel now and before.
pub(crate) fn span<'r>(
    mut pairs: impl DoubleEndedIterator<Item = (&'r u32, &'r u32)> + ExactSizeIterator,
    hit: impl Fn(u32, u32) -> bool,
) -> Option<(usize, usize)> {
    let count = pairs.len();
    let first = pairs.position(|(&n, &b)| hit(n, b))?;
    // What is left is after `first`: a hit found from the end is counted back from the last.
    let last = match pairs.rev().position(|(&n, &b)| hit(n, b)) {
        Some(back) => count - 1 - back,
        None => first,
    };
    Some((first, last))
}

/// What each frame of `order` has, against the one before it, and the first frame against the
/// last, as a loop plays it again.
fn look(picture: &Picture<'_>, order: &[usize]) -> (Vec<Seen>, Seen) {
    let width = picture.doc().width.max(1);
    let mut seen = vec![Seen::default(); order.len()];
    let mut again = Seen::default();
    let (mut band, mut before, mut first) = (Vec::new(), Vec::new(), Vec::new());
    for index in 0..picture.bands() {
        let top = index * BAND_ROWS;
        before.clear();
        for (at, &frame) in order.iter().enumerate() {
            picture.band(frame, index, &mut band);
            before.resize(band.len(), 0);
            compare(&band, &before, (width, top), &mut seen[at]);
            if at == 0 {
                first.clone_from(&band);
            }
            std::mem::swap(&mut band, &mut before);
        }
        compare(&first, &before, (width, top), &mut again);
    }
    (seen, again)
}

/// How each frame of `order` is drawn. `looped` if it plays more than once.
pub(crate) fn gif_plan(picture: &Picture<'_>, order: &[usize], looped: bool) -> Vec<Step> {
    let (seen, again) = look(picture, order);
    let mut drawn: Vec<(Bounds, Disposal)> = Vec::with_capacity(seen.len());
    for (at, now) in seen.iter().enumerate() {
        let mut bounds = match drawn.last() {
            // The frame before cleared its area: what this one shows there is drawn again.
            Some(&(area, Disposal::Clear)) => now.changed.union(now.opaque.meet(area)),
            _ => now.changed,
        };
        let cleared = match seen.get(at + 1) {
            Some(next) => next.vanished,
            // The first frame again: it draws all it shows, so only what it does not is cleared.
            None if looped => again.vanished,
            None => Bounds(None),
        };
        let after = match cleared.is_empty() {
            true => Disposal::Keep,
            false => Disposal::Clear,
        };
        bounds = bounds.union(cleared);
        drawn.push((bounds, after));
    }
    drawn
        .into_iter()
        .map(|(bounds, after)| Step {
            area: area_of(bounds),
            after,
        })
        .collect()
}

/// The area that draws `bounds`. A frame that draws nothing draws its top left pixel, which is
/// already what the canvas shows: a GIF frame is at least one pixel.
pub(crate) fn area_of(bounds: Bounds) -> Area {
    let [x, y, right, bottom] = bounds.0.unwrap_or([0, 0, 1, 1]);
    Area {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(l: usize, t: usize, r: usize, bottom: usize) -> Bounds {
        Bounds(Some([l, t, r, bottom]))
    }

    #[test]
    fn bounds_grow_join_and_meet() {
        let mut one = Bounds::default();
        one.add(3, 2);
        assert_eq!(one, b(3, 2, 4, 3));
        one.add(1, 5);
        assert_eq!(one, b(1, 2, 4, 6));
        assert_eq!(one.union(Bounds(None)), one);
        assert_eq!(Bounds(None).union(one), one);
        assert_eq!(b(0, 0, 2, 2).union(b(5, 1, 6, 9)), b(0, 0, 6, 9));
        assert_eq!(b(0, 0, 4, 4).meet(b(2, 3, 9, 9)), b(2, 3, 4, 4));
        assert_eq!(
            b(0, 0, 2, 2).meet(b(2, 0, 4, 2)),
            Bounds(None),
            "touching only"
        );
        assert_eq!(b(0, 0, 2, 2).meet(Bounds(None)), Bounds(None));
        assert_eq!(Bounds(None).meet(b(0, 0, 2, 2)), Bounds(None));
    }

    #[test]
    fn a_row_is_searched_from_both_ends() {
        let now = [0, 0xff00_0001, 0, 0xff00_0002, 0xff00_0002, 0];
        let before = [0xff00_0009, 0xff00_0001, 0, 0, 0xff00_0002, 0xff00_0009];
        let pairs = || now.iter().zip(&before);
        assert_eq!(span(pairs(), |n, _| n >> 24 != 0), Some((1, 4)));
        assert_eq!(
            span(pairs(), |n, b| n >> 24 != 0 && n != b),
            Some((3, 3)),
            "one pixel"
        );
        assert_eq!(
            span(pairs(), |n, b| n >> 24 == 0 && b >> 24 != 0),
            Some((0, 5))
        );
        assert_eq!(span(pairs(), |n, _| n == 7), None);
        let one = [5u32];
        assert_eq!(span(one.iter().zip(&one), |n, _| n == 5), Some((0, 0)));
    }

    #[test]
    fn nothing_drawn_is_the_top_left_pixel() {
        let area = area_of(Bounds(None));
        assert_eq!((area.x, area.y, area.width, area.height), (0, 0, 1, 1));
        let area = area_of(b(2, 3, 7, 4));
        assert_eq!((area.x, area.y, area.width, area.height), (2, 3, 5, 1));
    }
}
