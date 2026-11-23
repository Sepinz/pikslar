//! Palette chunks: the new one (0x2019) and the two old ones (0x0004 and 0x0011).
//! Written from the public file specification only; see [`crate::ase_read`].

use crate::ase_chunks::{Bytes, Depth, Sprite};
use crate::ase_read::AseWarning;
use crate::FormatError;
use pikslar_core::rgba;

/// The most colors a palette brought in has: what the color table holds besides clear.
const MAX_PALETTE: u32 = 65_535;
/// The least bytes one entry of a new palette chunk takes.
const ENTRY_BYTES: u64 = 6;
/// Old palettes number their colors with a byte, from 0 to 255.
const OLD_COLORS: usize = 256;

/// A 6-bit color channel (0 to 63) as 8 bits.
fn widen(channel: u8) -> u8 {
    ((u32::from(channel.min(63)) * 255 + 31) / 63) as u8
}

impl Sprite<'_> {
    /// A new palette chunk (0x2019).
    pub fn palette_chunk(&mut self, frame: usize, data: &[u8]) -> Result<(), FormatError> {
        let mut r = Bytes(data);
        let (size, first, last) = (r.u32()?, r.u32()?, r.u32()?);
        r.skip(8)?;
        if size > MAX_PALETTE {
            return Err(FormatError::Unsupported("a palette of over 65,535 colors"));
        }
        if first > last || last >= size {
            return Err(FormatError::Corrupt(
                "a palette range lies outside the palette",
            ));
        }
        let entries = u64::from(last - first) + 1;
        if entries * ENTRY_BYTES > r.len() as u64 {
            return Err(FormatError::Corrupt("a palette chunk is cut short"));
        }
        if frame > 0 {
            self.warnings.add(AseWarning::PaletteChanges);
        }
        let palette = self.palette.get_or_insert_with(Vec::new);
        // A later chunk may add colors, but a smaller one does not take any away.
        if palette.len() < size as usize {
            palette.resize(size as usize, 0);
        }
        for at in first..=last {
            let flags = r.u16()?;
            let [red, green, blue, alpha] = [r.u8()?, r.u8()?, r.u8()?, r.u8()?];
            if flags & 1 != 0 {
                r.string()?;
            }
            if let Some(slot) = palette.get_mut(at as usize) {
                *slot = rgba(red, green, blue, alpha);
            }
        }
        Ok(())
    }

    /// An old palette chunk: `six_bit` for 0x0011, whose channels go from 0 to 63.
    pub fn old_palette_chunk(
        &mut self,
        frame: usize,
        data: &[u8],
        six_bit: bool,
    ) -> Result<(), FormatError> {
        if frame > 0 {
            self.warnings.add(AseWarning::PaletteChanges);
        }
        let mut r = Bytes(data);
        let packets = r.u16()?;
        let mut at = 0usize;
        for _ in 0..packets {
            let skip = usize::from(r.u8()?);
            let count = match r.u8()? {
                0 => OLD_COLORS,
                n => usize::from(n),
            };
            at += skip;
            for _ in 0..count {
                let [red, green, blue] = [r.u8()?, r.u8()?, r.u8()?];
                let [red, green, blue] = match six_bit {
                    true => [widen(red), widen(green), widen(blue)],
                    false => [red, green, blue],
                };
                if at >= OLD_COLORS {
                    return Err(FormatError::Corrupt("an old palette runs past 256 colors"));
                }
                if self.old_palette.len() <= at {
                    self.old_palette.resize(at + 1, 0);
                }
                if let Some(slot) = self.old_palette.get_mut(at) {
                    *slot = rgba(red, green, blue, u8::MAX);
                }
                at += 1;
            }
        }
        Ok(())
    }

    /// Warns of what the palette cannot answer for: no palette in an indexed file, and indexed
    /// pixels that name a color it does not have.
    pub fn check_palette(&mut self) {
        if self.depth != Depth::Indexed {
            return;
        }
        let colors = self
            .palette
            .as_ref()
            .map_or(self.old_palette.len(), Vec::len);
        if colors == 0 {
            self.warnings.add(AseWarning::NoPalette);
        } else if self.highest.is_some_and(|i| usize::from(i) >= colors) {
            self.warnings.add(AseWarning::IndexPastPalette);
        }
    }

    /// The palette the file ends with: the new one if it has one, else the old one.
    pub fn final_palette(&self) -> Vec<u32> {
        self.palette
            .clone()
            .unwrap_or_else(|| self.old_palette.clone())
    }
}
