//! Icon art as two-bit pixels, packed at compile time from rows of characters.
//!
//! The art is written in source as sheets: strips of icons side by side, one space between
//! icons, `size` rows per strip. `.` is clear, `#` ink, `+` shade and `*` signal. A `const fn`
//! packs each icon into `u32` words, sixteen pixels a word, row by row, so the strings never
//! reach the executable and a stray character stops the build.

/// What one icon pixel shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Clear,
    /// Text, or OnAccent on a chosen control.
    Ink,
    /// Muted; ink in high contrast and on a chosen control.
    Shade,
    /// Accent, used sparingly; ink in high contrast and on a chosen control.
    Signal,
}

/// Why a sheet does not pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackError {
    /// The word count does not fit the icon size.
    Words,
    /// The sheet does not have one strip of `size` rows for every `per` icons.
    Rows,
    /// A row is not as long as its strip's icons and gaps.
    Width { row: usize },
    /// The character between two icons is not a space.
    Gap { row: usize, col: usize },
    /// A character that is not `.`, `#`, `+` or `*`.
    Char { row: usize, col: usize },
}

/// The words one `size` by `size` icon takes.
pub const fn words(size: usize) -> usize {
    (size * size * 2).div_ceil(32)
}

const fn bits(ch: u8) -> Option<u32> {
    match ch {
        b'.' => Some(0),
        b'#' => Some(1),
        b'+' => Some(2),
        b'*' => Some(3),
        _ => None,
    }
}

/// Packs `N` icons of `size` pixels, laid out `per` to a strip, from `rows`.
pub const fn try_sheet<const N: usize, const W: usize>(
    size: usize,
    per: usize,
    rows: &[&str],
) -> Result<[[u32; W]; N], PackError> {
    if size == 0 || per == 0 || W != words(size) {
        return Err(PackError::Words);
    }
    if rows.len() != N.div_ceil(per) * size {
        return Err(PackError::Rows);
    }
    let mut out = [[0u32; W]; N];
    let mut row = 0;
    while row < rows.len() {
        let line = rows[row].as_bytes();
        let first = row / size * per;
        let count = if N - first < per { N - first } else { per };
        if line.len() != count * (size + 1) - 1 {
            return Err(PackError::Width { row });
        }
        let mut col = 0;
        while col < line.len() {
            let (icon, x) = (col / (size + 1), col % (size + 1));
            if x == size {
                if line[col] != b' ' {
                    return Err(PackError::Gap { row, col });
                }
            } else {
                let Some(tone) = bits(line[col]) else {
                    return Err(PackError::Char { row, col });
                };
                let i = (row % size) * size + x;
                out[first + icon][i / 16] |= tone << ((i % 16) * 2);
            }
            col += 1;
        }
        row += 1;
    }
    Ok(out)
}

/// [`try_sheet`] for art in source: a sheet that does not pack stops the build.
pub const fn sheet<const N: usize, const W: usize>(
    size: usize,
    per: usize,
    rows: &[&str],
) -> [[u32; W]; N] {
    match try_sheet(size, per, rows) {
        Ok(packed) => packed,
        Err(_) => panic!("an icon sheet does not pack"),
    }
}

/// One packed icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Art {
    size: i32,
    words: &'static [u32],
}

impl Art {
    pub const fn new(size: i32, words: &'static [u32]) -> Art {
        Art { size, words }
    }

    /// The icon is `size` by `size` pixels.
    pub const fn size(self) -> i32 {
        self.size
    }

    /// The tone at (`x`, `y`); clear outside the icon.
    pub fn tone(self, x: i32, y: i32) -> Tone {
        if x < 0 || y < 0 || x >= self.size || y >= self.size {
            return Tone::Clear;
        }
        let i = (y * self.size + x) as usize;
        let word = self.words.get(i / 16).copied().unwrap_or(0);
        match (word >> ((i % 16) * 2)) & 3 {
            1 => Tone::Ink,
            2 => Tone::Shade,
            3 => Tone::Signal,
            _ => Tone::Clear,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_counts_match_the_plan() {
        assert_eq!(words(16), 16);
        assert_eq!(words(12), 9);
        assert_eq!(words(8), 4);
    }

    #[rustfmt::skip]
    const TWO: [&str; 2] = [
        "#. +*",
        ".+ *.",
    ];

    static PACKED: [[u32; 1]; 2] = sheet(2, 2, &TWO);

    #[test]
    fn a_sheet_packs_every_tone_in_place() {
        let a = Art::new(2, &PACKED[0]);
        let b = Art::new(2, &PACKED[1]);
        assert_eq!(
            [a.tone(0, 0), a.tone(1, 0), a.tone(0, 1), a.tone(1, 1)],
            [Tone::Ink, Tone::Clear, Tone::Clear, Tone::Shade]
        );
        assert_eq!(
            [b.tone(0, 0), b.tone(1, 0), b.tone(0, 1), b.tone(1, 1)],
            [Tone::Shade, Tone::Signal, Tone::Signal, Tone::Clear]
        );
        assert_eq!(a.tone(-1, 0), Tone::Clear);
        assert_eq!(a.tone(0, 2), Tone::Clear);
    }

    #[test]
    fn a_short_last_strip_packs() {
        let rows = ["#. .#", "## ..", "+."];
        let rows = [rows[0], rows[1], rows[2], ".+"];
        let packed = try_sheet::<3, 1>(2, 2, &rows);
        let Ok(packed) = packed else {
            panic!("{packed:?}");
        };
        assert_eq!(packed[2][0], 2 | (2 << 6));
    }

    #[test]
    fn bad_sheets_are_refused() {
        assert_eq!(try_sheet::<2, 2>(2, 2, &TWO), Err(PackError::Words));
        assert_eq!(try_sheet::<2, 1>(2, 2, &TWO[..1]), Err(PackError::Rows));
        assert_eq!(
            try_sheet::<2, 1>(2, 2, &["#. +*", ".+ *"]),
            Err(PackError::Width { row: 1 })
        );
        assert_eq!(
            try_sheet::<2, 1>(2, 2, &["#.-+*", ".+ *."]),
            Err(PackError::Gap { row: 0, col: 2 })
        );
        assert_eq!(
            try_sheet::<2, 1>(2, 2, &["#. +*", ".x *."]),
            Err(PackError::Char { row: 1, col: 1 })
        );
    }
}
