//! One frame's drawing on a layer, and the only code that maps canvas positions to it.
//!
//! The rule, used for every read and write: a cel's offset is where its image's local (0, 0)
//! lands on the canvas, so canvas `(x, y)` is local `(x - cx, y - cy)`. Arithmetic is in `i32`.

use crate::grid::Grid;
use crate::image::{Image, Reader};
use crate::tile::{RawTile, TILE, TILE_SHIFT};
use std::sync::Arc;

/// How far a cel may sit from the canvas origin on either axis, in pixels: three times the
/// largest canvas. Every canvas pixel then has a local tile within the image's reach.
pub const OFFSET_LIMIT: i32 = 24_576;

const IN_TILE: i32 = TILE as i32 - 1;

/// One frame's drawing on a layer. Cloning shares the image until one of them is edited.
///
/// Reads take canvas positions, whatever the offset.
///
/// There are two ways to copy a cel, and each caller must mean one of them:
///
/// - **The whole cel** ([`Clone`]): the drawing and where it sits. Starting a hold does this.
/// - **The drawing only** ([`Cel::show_drawing_of`]): where the cel sits and which hold it is in
///   stay as they were. Keeping the frames of a hold on one drawing does this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cel {
    pub(super) image: Arc<Image>,
    pub(super) x: i16,
    pub(super) y: i16,
    /// Which hold this frame belongs to, or 0. An id means one hold in the whole document.
    pub(super) link: u32,
}

/// Reads canvas pixels of one cel through the tile it read last.
pub struct CelReader<'a> {
    inner: Reader<'a>,
    x: i32,
    y: i32,
}

impl CelReader<'_> {
    /// The same answer as [`Cel::get`].
    #[inline]
    pub fn get(&mut self, x: i32, y: i32) -> u32 {
        self.inner.get(x - self.x, y - self.y)
    }
}

impl Cel {
    pub fn new(grid: Grid) -> Self {
        Cel::from_image(grid.into_image())
    }

    /// A cel with nothing drawn.
    pub fn blank() -> Self {
        Cel::from_image(Image::blank())
    }

    /// A cel at the canvas origin showing this image, in no hold.
    pub fn from_image(image: Image) -> Self {
        Cel::placed(image, 0, 0)
    }

    /// A cel showing this image with its local origin at canvas (`x`, `y`), in no hold.
    /// `None` when the position is past [`OFFSET_LIMIT`].
    pub fn at(image: Image, x: i32, y: i32) -> Option<Self> {
        Cel::in_range(x, y).then(|| Cel::placed(image, x as i16, y as i16))
    }

    fn placed(image: Image, x: i16, y: i16) -> Self {
        Cel {
            image: Arc::new(image),
            x,
            y,
            link: 0,
        }
    }

    /// True when a cel may sit at this position.
    pub fn in_range(x: i32, y: i32) -> bool {
        let range = -OFFSET_LIMIT..=OFFSET_LIMIT;
        range.contains(&x) && range.contains(&y)
    }

    /// Where this cel's image sits on the canvas.
    pub fn position(&self) -> (i16, i16) {
        (self.x, self.y)
    }

    /// This cel showing the same shared image from another place, in no hold. `None` when the
    /// position is past [`OFFSET_LIMIT`].
    pub fn moved_to(&self, x: i32, y: i32) -> Option<Cel> {
        Cel::in_range(x, y).then(|| Cel {
            image: self.image.clone(),
            x: x as i16,
            y: y as i16,
            link: 0,
        })
    }

    /// How far this cel can move along each axis and stay in range: least and most.
    pub(crate) fn room_to_move(&self) -> ((i32, i32), (i32, i32)) {
        let (x, y) = self.origin();
        (
            (-OFFSET_LIMIT - x, OFFSET_LIMIT - x),
            (-OFFSET_LIMIT - y, OFFSET_LIMIT - y),
        )
    }

    /// Moves the cel. The caller keeps the result in range, through `room_to_move`.
    pub(super) fn move_by(&mut self, dx: i32, dy: i32) {
        let (x, y) = (self.x as i32 + dx, self.y as i32 + dy);
        debug_assert!(Cel::in_range(x, y), "a move past the range");
        let clamp = |v: i32| v.clamp(-OFFSET_LIMIT, OFFSET_LIMIT) as i16;
        (self.x, self.y) = (clamp(x), clamp(y));
    }

    #[inline]
    fn origin(&self) -> (i32, i32) {
        (self.x as i32, self.y as i32)
    }

    /// Which hold this cel belongs to, or 0.
    #[inline]
    pub fn link(&self) -> u32 {
        self.link
    }

    /// Shows another cel's drawing here, keeping this cel's own position and hold.
    pub(super) fn show_drawing_of(&mut self, other: &Cel) {
        self.image = other.image.clone();
    }

    /// Reads one canvas position, returning 0 where nothing is drawn. Any position may be
    /// asked, on the canvas or off it.
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> u32 {
        let (cx, cy) = self.origin();
        self.image.get(x - cx, y - cy)
    }

    /// Reads canvas positions as [`Cel::get`] does, holding the image itself, for loops that
    /// need a plain function and read many pixels.
    pub fn sampler(&self) -> impl Fn(i32, i32) -> u32 + '_ {
        let image: &Image = &self.image;
        let (cx, cy) = self.origin();
        move |x, y| image.get(x - cx, y - cy)
    }

    /// A way to read many nearby canvas pixels quickly.
    pub fn reader(&self) -> CelReader<'_> {
        let (x, y) = self.origin();
        CelReader {
            inner: self.image.reader(),
            x,
            y,
        }
    }

    /// True when nothing has been drawn here.
    pub fn is_blank(&self) -> bool {
        self.image.is_blank()
    }

    /// Writes a canvas of this size to a top-to-bottom buffer.
    pub fn to_cells(&self, width: usize, height: usize) -> Vec<u32> {
        self.window(0, 0, width, height)
    }

    /// Writes the canvas rectangle at (`x0`, `y0`) of this size to a top-to-bottom buffer. It
    /// may reach past the canvas, which reads what is drawn there.
    pub fn window(&self, x0: i32, y0: i32, width: usize, height: usize) -> Vec<u32> {
        let mut px = vec![0u32; width * height];
        let (cx, cy) = self.origin();
        let mut raw = [0u32; crate::tile::TILE_AREA];
        let (table_x, table_y, tw, th) = self.image.table();
        let (w, h) = (width as i64, height as i64);
        for ty in table_y..table_y + th as i32 {
            for tx in table_x..table_x + tw as i32 {
                // The tile's corner relative to the window, in i64 so no window can overflow.
                let left = (tx as i64) * TILE as i64 + cx as i64 - x0 as i64;
                let top = (ty as i64) * TILE as i64 + cy as i64 - y0 as i64;
                let size = TILE as i64;
                if left >= w || top >= h || left + size <= 0 || top + size <= 0 {
                    continue;
                }
                let Some(tile) = self.image.tile(tx, ty) else {
                    continue;
                };
                tile.decode(&mut raw);
                let (from_x, to_x) = (left.max(0), (left + size).min(w));
                for row in top.max(0)..(top + size).min(h) {
                    let source = ((row - top) * size + (from_x - left)) as usize;
                    let start = (row * w + from_x) as usize;
                    let count = (to_x - from_x) as usize;
                    px[start..start + count].copy_from_slice(&raw[source..source + count]);
                }
            }
        }
        px
    }

    /// True when this cel can be stored as a canvas-sized grid of this size: at the origin, with
    /// every tile inside the canvas, counting partial tiles at the right and bottom as inside.
    pub fn fits_grid(&self, width: usize, height: usize) -> bool {
        self.origin() == (0, 0) && self.image.fits(width, height)
    }

    /// What growing this cel's table to take a write anywhere on a canvas of this size could
    /// allocate beyond what a table of the canvas's size holds, which every estimate already
    /// counts: the larger table's slots, or 0 when it stays within that.
    pub fn growth_bytes(&self, width: usize, height: usize) -> usize {
        if width == 0 || height == 0 {
            return 0;
        }
        let (cx, cy) = self.origin();
        let first = |at: i32| at >> TILE_SHIFT;
        let (left, top) = (first(-cx), first(-cy));
        let (right, bottom) = (first(width as i32 - 1 - cx), first(height as i32 - 1 - cy));
        let (x0, y0, tw, th) = self.image.table();
        let ((x0, x1), (y0, y1)) = if self.image.is_blank() {
            ((left, right + 1), (top, bottom + 1))
        } else {
            (
                (x0.min(left), (x0 + tw as i32).max(right + 1)),
                (y0.min(top), (y0 + th as i32).max(bottom + 1)),
            )
        };
        let slots = (x1 - x0) as usize * (y1 - y0) as usize;
        let canvas = width.div_ceil(TILE) * height.div_ceil(TILE);
        if slots <= (tw * th).max(canvas) {
            return 0;
        }
        slots.saturating_mul(size_of::<crate::grid::Cell>())
    }

    /// The canvas tile's corner in the image's own coordinates.
    #[inline]
    fn local_corner(&self, tx: usize, ty: usize) -> (i32, i32) {
        let (cx, cy) = self.origin();
        (
            (tx as i32) * TILE as i32 - cx,
            (ty as i32) * TILE as i32 - cy,
        )
    }

    /// True when something may be drawn in a canvas tile. Exact when the offset is a multiple of
    /// 16, otherwise true if any image tile it overlaps holds something.
    #[inline]
    pub fn has_tile(&self, tx: usize, ty: usize) -> bool {
        let (lx, ly) = self.local_corner(tx, ty);
        let (ix, iy) = (lx >> TILE_SHIFT, ly >> TILE_SHIFT);
        let has = |dx: i32, dy: i32| self.image.tile(ix + dx, iy + dy).is_some();
        let (across, down) = ((lx & IN_TILE) != 0, (ly & IN_TILE) != 0);
        has(0, 0) || (across && has(1, 0)) || (down && has(0, 1)) || (across && down && has(1, 1))
    }

    /// Decodes a canvas tile into `out`, clearing what nothing is drawn on. At an offset that is
    /// not a multiple of 16 it is put together from the parts of up to four image tiles.
    #[inline]
    pub fn decode(&self, tx: usize, ty: usize, out: &mut RawTile) {
        let (lx, ly) = self.local_corner(tx, ty);
        let (ix, iy) = (lx >> TILE_SHIFT, ly >> TILE_SHIFT);
        let (rx, ry) = ((lx & IN_TILE) as usize, (ly & IN_TILE) as usize);
        if rx == 0 && ry == 0 {
            self.image.decode(ix, iy, out);
            return;
        }
        out.fill(0);
        // Each part: which image tile, where its piece starts in that tile, and how big it is.
        let parts = [
            ((0, 0), (rx, ry), (TILE - rx, TILE - ry), (0, 0)),
            ((1, 0), (0, ry), (rx, TILE - ry), (TILE - rx, 0)),
            ((0, 1), (rx, 0), (TILE - rx, ry), (0, TILE - ry)),
            ((1, 1), (0, 0), (rx, ry), (TILE - rx, TILE - ry)),
        ];
        for ((dx, dy), from, size, to) in parts {
            if size.0 == 0 || size.1 == 0 {
                continue;
            }
            if let Some(tile) = self.image.tile(ix + dx, iy + dy) {
                tile.decode_rect(from, size, out, to);
            }
        }
    }

    /// True when both cels show one shared image, so an edit made through a link reaches both.
    #[inline]
    pub fn same_image(&self, other: &Cel) -> bool {
        Arc::ptr_eq(&self.image, &other.image)
    }

    /// Tells images apart without looking at them.
    pub(crate) fn image_key(&self) -> *const Image {
        Arc::as_ptr(&self.image)
    }

    /// How many cels anywhere, in documents or history, hold this cel's image.
    pub(crate) fn image_holders(&self) -> usize {
        Arc::strong_count(&self.image)
    }

    /// True when both cels show one shared image at one place, so they look the same.
    #[inline]
    pub fn looks_the_same(&self, other: &Cel) -> bool {
        self.same_image(other) && self.origin() == other.origin()
    }

    /// A number equal for cels sharing one image, for telling images apart while saving.
    pub fn image_id(&self) -> usize {
        Arc::as_ptr(&self.image) as usize
    }

    /// What this cel's image holds in memory, counting every tile as its own.
    pub fn heap_size(&self) -> usize {
        self.image.heap_size()
    }

    /// What this cel's image keeps alive while nothing else holds it: what it holds, and the
    /// shared allocation it lives in with its two counts.
    pub(crate) fn kept_bytes(&self) -> usize {
        kept_bytes(&self.image)
    }

    /// What this cel's image holds in its table of tile references alone.
    pub fn table_bytes(&self) -> usize {
        self.image.table_bytes()
    }

    /// The same picture with an image of its own, sharing only the immutable tiles.
    pub fn with_own_image(&self) -> Cel {
        Cel {
            image: Arc::new((*self.image).clone()),
            ..self.clone()
        }
    }

    /// The smallest rectangle holding every drawn pixel, on the canvas or past it, in canvas
    /// positions: x, y, width and height.
    pub fn drawn_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let (x, y, w, h) = self.image.drawn_bounds()?;
        let (cx, cy) = self.origin();
        Some((x + cx, y + cy, w, h))
    }

    /// This cel showing another image from the same place, in the same hold.
    pub(crate) fn with_image(&self, image: Arc<Image>) -> Cel {
        Cel {
            image,
            ..self.clone()
        }
    }

    /// The drawing this cel shows, in the image's own coordinates.
    pub fn image(&self) -> &Image {
        &self.image
    }

    /// The image to edit, copied first if another cel or undo step still shares it.
    pub(super) fn image_mut(&mut self) -> &mut Image {
        Arc::make_mut(&mut self.image)
    }
}

/// What an image keeps alive while nothing else holds it: what it holds, and the shared
/// allocation it lives in with its two counts.
pub(crate) fn kept_bytes(image: &Image) -> usize {
    2 * size_of::<usize>() + size_of::<Image>() + image.heap_size()
}

#[cfg(test)]
#[path = "cel_tests.rs"]
mod tests;
