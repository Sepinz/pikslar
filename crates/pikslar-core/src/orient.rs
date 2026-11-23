//! The eight ways a rectangle of pixels can be turned and mirrored.
//!
//! Brushes and tiles both use these, and they follow Tiled's definition so that exported flags
//! mean the same thing there: the **diagonal** flip (swap x and y) is applied first, then the
//! **horizontal** flip, then the **vertical** one. Tiled stores the three as the top bits of a
//! tile id: bit 31 horizontal, bit 30 vertical, bit 29 diagonal.
//!
//! An [`Orientation`] is only the way of turning; the pixels are never resampled, so four
//! quarter turns give back exactly the start.
//!
//! ```
//! use pikslar_core::orient::Orientation;
//! let turn = Orientation::rotate90();
//! // A 2 wide, 3 tall picture becomes 3 wide, 2 tall, and its top left goes to the top right.
//! assert_eq!(turn.turned_size(2, 3), (3, 2));
//! assert_eq!(turn.apply(0, 0, 2, 3), (2, 0));
//! assert_eq!(turn.compose(turn).compose(turn).compose(turn), Orientation::IDENTITY);
//! ```

/// Tiled's flag for a horizontal flip.
pub const FLIPPED_HORIZONTALLY: u32 = 0x8000_0000;
/// Tiled's flag for a vertical flip.
pub const FLIPPED_VERTICALLY: u32 = 0x4000_0000;
/// Tiled's flag for the diagonal flip (x and y swapped).
pub const FLIPPED_DIAGONALLY: u32 = 0x2000_0000;

const HORIZONTAL: u8 = 0b100;
const VERTICAL: u8 = 0b010;
const DIAGONAL: u8 = 0b001;

/// One of the eight orientations. Bit 2 is the horizontal flip, bit 1 the vertical flip and bit
/// 0 the diagonal flip, so the number is the top three bits of Tiled's flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Orientation(u8);

impl Orientation {
    /// Left as it is.
    pub const IDENTITY: Orientation = Orientation(0);

    /// All eight, in the order of their numbers.
    pub const ALL: [Orientation; 8] = [
        Orientation(0),
        Orientation(1),
        Orientation(2),
        Orientation(3),
        Orientation(4),
        Orientation(5),
        Orientation(6),
        Orientation(7),
    ];

    /// The orientation with this number, 0 to 7.
    pub const fn from_index(index: u8) -> Option<Orientation> {
        if index < 8 {
            Some(Orientation(index))
        } else {
            None
        }
    }

    /// Its number, 0 to 7.
    pub const fn index(self) -> u8 {
        self.0
    }

    /// Mirrored left to right.
    pub const fn flip_horizontal() -> Orientation {
        Orientation(HORIZONTAL)
    }

    /// Mirrored top to bottom.
    pub const fn flip_vertical() -> Orientation {
        Orientation(VERTICAL)
    }

    /// Turned a quarter turn clockwise.
    pub const fn rotate90() -> Orientation {
        Orientation(HORIZONTAL | DIAGONAL)
    }

    /// Turned half way round.
    pub const fn rotate180() -> Orientation {
        Orientation(HORIZONTAL | VERTICAL)
    }

    /// Turned a quarter turn counter-clockwise.
    pub const fn rotate270() -> Orientation {
        Orientation(VERTICAL | DIAGONAL)
    }

    /// Mirrored along the top-left to bottom-right diagonal: x and y swapped.
    pub const fn transpose() -> Orientation {
        Orientation(DIAGONAL)
    }

    /// Mirrored along the other diagonal.
    pub const fn anti_transpose() -> Orientation {
        Orientation(HORIZONTAL | VERTICAL | DIAGONAL)
    }

    /// The orientation Tiled's flags stand for. Bits other than the top three are ignored.
    pub const fn from_tiled_bits(bits: u32) -> Orientation {
        Orientation((bits >> 29) as u8)
    }

    /// Tiled's three flag bits for this orientation, and nothing else.
    pub const fn to_tiled_bits(self) -> u32 {
        (self.0 as u32) << 29
    }

    /// True if width and height trade places. Only these need a square to fit.
    pub const fn swaps_axes(self) -> bool {
        self.0 & DIAGONAL != 0
    }

    /// The size of a `w` by `h` picture after this turn.
    pub const fn turned_size(self, w: usize, h: usize) -> (usize, usize) {
        if self.swaps_axes() {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// Where the pixel at (`x`, `y`) of a `w` by `h` picture lands, within
    /// [`turned_size`](Self::turned_size). The diagonal flip comes first, then horizontal, then
    /// vertical. `x < w` and `y < h` are the caller's to keep.
    pub const fn apply(self, x: usize, y: usize, w: usize, h: usize) -> (usize, usize) {
        let (mut x, mut y, mut w, mut h) = (x, y, w, h);
        if self.0 & DIAGONAL != 0 {
            (x, y, w, h) = (y, x, h, w);
        }
        if self.0 & HORIZONTAL != 0 {
            x = w - 1 - x;
        }
        if self.0 & VERTICAL != 0 {
            y = h - 1 - y;
        }
        (x, y)
    }

    /// The orientation of doing `self` and then `then`.
    pub const fn compose(self, then: Orientation) -> Orientation {
        // The result is the one orientation that sends two probe points of an oblong grid
        // where the two turns in a row send them. The points tell all eight apart.
        let (w, h) = (2, 3);
        let (tw, th) = self.turned_size(w, h);
        let (ax, ay) = self.apply(0, 0, w, h);
        let (bx, by) = self.apply(1, 0, w, h);
        let want_a = then.apply(ax, ay, tw, th);
        let want_b = then.apply(bx, by, tw, th);
        let mut i = 0;
        while i < 8 {
            let o = Orientation(i);
            let a = o.apply(0, 0, w, h);
            let b = o.apply(1, 0, w, h);
            if a.0 == want_a.0 && a.1 == want_a.1 && b.0 == want_b.0 && b.1 == want_b.1 {
                return o;
            }
            i += 1;
        }
        // Unreachable: the eight orientations are closed under composition.
        Orientation::IDENTITY
    }

    /// The orientation that undoes this one.
    pub const fn inverse(self) -> Orientation {
        let mut i = 0;
        while i < 8 {
            let o = Orientation(i);
            if self.compose(o).0 == 0 {
                return o;
            }
            i += 1;
        }
        Orientation::IDENTITY
    }
}
