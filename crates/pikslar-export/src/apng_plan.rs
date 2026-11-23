//! What each frame of an APNG draws: the rectangle of cells that differ, exactly, from the frame
//! shown before it. A frame replaces what it covers (SOURCE) and leaves it (NONE), so a cell
//! outside its rectangle is already what it should be, clear or not, and one inside is written
//! as it is. The first frame is the whole image, so a loop starts the same however a player
//! restarts it.
//!
//! The frames are compared at the document's own size, a band at a time in play order,
//! holding two bands and one rectangle per frame.

use crate::gif_plan::{area_of, span, Bounds};
use crate::Picture;
use pikslar_format::apng_write::Area;
use pikslar_render::BAND_ROWS;

/// Records in `changed` the cells of `now`, rows from `top` of a frame `width` wide, that are
/// not `before`'s.
fn compare(now: &[u32], before: &[u32], (width, top): (usize, usize), changed: &mut Bounds) {
    let rows = now.chunks_exact(width).zip(before.chunks_exact(width));
    for (y, (now, before)) in (top..).zip(rows) {
        changed.add_row(y, span(now.iter().zip(before), |n, b| n != b));
    }
}

/// The area each frame of `order` draws, in document pixels.
pub(crate) fn apng_areas(picture: &Picture<'_>, order: &[usize]) -> Vec<Area> {
    let doc = picture.doc();
    let width = doc.width.max(1);
    let mut changed = vec![Bounds::default(); order.len()];
    let (mut band, mut before) = (Vec::new(), Vec::new());
    // A single frame is the whole image, with nothing before it to compare.
    let bands = if order.len() > 1 { picture.bands() } else { 0 };
    for index in 0..bands {
        let top = index * BAND_ROWS;
        for (at, &frame) in order.iter().enumerate() {
            picture.band(frame, index, &mut band);
            if at > 0 {
                compare(&band, &before, (width, top), &mut changed[at]);
            }
            std::mem::swap(&mut band, &mut before);
        }
    }
    let whole = Area {
        x: 0,
        y: 0,
        width: doc.width,
        height: doc.height,
    };
    let rest = changed.into_iter().skip(1).map(area_of);
    std::iter::once(whole)
        .chain(rest)
        .take(order.len())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_records_only_cells_that_differ_in_any_channel() {
        let now = [0xff00_0001u32, 0x0000_0005, 0x8000_0002, 7];
        let before = [0xff00_0001u32, 0x0000_0006, 0x8000_0002, 7];
        let mut changed = Bounds::default();
        compare(&now, &before, (2, 4), &mut changed);
        assert_eq!(
            area_of(changed),
            Area {
                x: 1,
                y: 4,
                width: 1,
                height: 1
            },
            "a hidden colour counts"
        );
        let mut none = Bounds::default();
        compare(&now, &now, (4, 0), &mut none);
        assert!(none.is_empty());
    }
}
