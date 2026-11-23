//! Glyphs from the system's fonts, for characters the interface font does not draw.
//!
//! Each glyph is drawn once by GDI without smoothing, in Segoe UI with Windows' own font
//! linking for scripts it lacks, and read back as one byte a pixel. The interface keeps what
//! comes back, so this runs once for each character and size shown.

use crate::ffi::*;
use std::ptr::null_mut;

/// Draws `ch` into a `w` by `h` box with its baseline `baseline` rows from the top, as
/// `pikslar_ui::fallback` asks. A glyph wider than the box is narrowed to fit. Returns `None`
/// when the system draws nothing for a character that is not a space.
pub fn rasterize_glyph(ch: char, w: i32, h: i32, baseline: i32) -> Option<Vec<u8>> {
    if w <= 0 || h <= 0 || w > 1024 || h > 1024 {
        return None;
    }
    let text: Vec<u16> = ch.encode_utf16(&mut [0; 2]).to_vec();
    // SAFETY: every handle made here is checked and released before returning, and the
    // pixels are read only while the bitmap they belong to is alive.
    let wide = unsafe { draw(&text, w, h, baseline)? };
    let mask = narrow(&wide.pixels, wide.w, w, h);
    (mask.contains(&1) || ch.is_whitespace()).then_some(mask)
}

struct Drawn {
    pixels: Vec<u8>,
    w: i32,
}

/// GDI objects released when dropped: the device context first, which lets go of the font and
/// bitmap selected into it, then those.
struct Gdi {
    dc: H,
    font: H,
    bitmap: H,
}

impl Drop for Gdi {
    fn drop(&mut self) {
        // SAFETY: the handles are null or were made by this module and not yet released.
        unsafe {
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
            for o in [self.bitmap, self.font] {
                if !o.is_null() {
                    DeleteObject(o);
                }
            }
        }
    }
}

/// Draws the text at its natural width, at least `w` wide, and returns its pixels as 0 or 1.
/// The em is a quarter more than the baseline height, which puts Latin capitals about seven
/// rows of eight high, as tall as the interface font's.
unsafe fn draw(text: &[u16], w: i32, h: i32, baseline: i32) -> Option<Drawn> {
    let mut gdi = Gdi {
        dc: CreateCompatibleDC(null_mut()),
        font: segoe(baseline),
        bitmap: null_mut(),
    };
    if gdi.dc.is_null() || gdi.font.is_null() {
        return None;
    }
    SelectObject(gdi.dc, gdi.font);
    let n = text.len() as i32;
    let mut size = SIZE::default();
    GetTextExtentPoint32W(gdi.dc, text.as_ptr(), n, &mut size);
    let full = size.cx.clamp(w, 4 * w);
    let bits = section(&mut gdi, full, h)?;
    SetBkMode(gdi.dc, TRANSPARENT);
    SetTextColor(gdi.dc, 0x00ff_ffff);
    SetTextAlign(gdi.dc, TA_BASELINE);
    let left = (full - size.cx).max(0) / 2;
    TextOutW(gdi.dc, left, baseline, text.as_ptr(), n);
    GdiFlush();
    // SAFETY: the section holds `full * h` pixels and lives until `gdi` is dropped below.
    let px = std::slice::from_raw_parts(bits, (full * h) as usize);
    let pixels = px.iter().map(|&p| u8::from(p & 0xff > 0x7f)).collect();
    Some(Drawn { pixels, w: full })
}

/// Segoe UI without smoothing, its em a quarter more than `baseline`. Null if none was made.
///
/// # Safety
/// The caller releases the font it returns.
unsafe fn segoe(baseline: i32) -> H {
    let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    CreateFontW(
        -(baseline * 5 / 4).max(1),
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        1,
        0,
        0,
        NONANTIALIASED_QUALITY,
        0,
        face.as_ptr(),
    )
}

/// A `w` by `h` 32-bit bitmap selected into `gdi`'s device context, which `gdi` releases.
/// Returns where its pixels are, or None if it could not be made.
///
/// # Safety
/// `gdi.dc` is a live device context.
unsafe fn section(gdi: &mut Gdi, w: i32, h: i32) -> Option<*mut u32> {
    let info = BITMAPINFOHEADER {
        size: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        w,
        h: -h,
        planes: 1,
        bits: 32,
        comp: 0,
        img: 0,
        xp: 0,
        yp: 0,
        used: 0,
        imp: 0,
    };
    let mut bits: *mut u32 = null_mut();
    gdi.bitmap = CreateDIBSection(gdi.dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if gdi.bitmap.is_null() || bits.is_null() {
        return None;
    }
    SelectObject(gdi.dc, gdi.bitmap);
    Some(bits)
}

/// Narrows pixels `from` wide to `w`, keeping a pixel wherever any of the columns it covers
/// had one, so thin strokes survive.
fn narrow(pixels: &[u8], from: i32, w: i32, h: i32) -> Vec<u8> {
    let mut out = vec![0u8; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x * from / w, ((x + 1) * from / w).max(x * from / w + 1));
            let row = &pixels[(y * from) as usize..((y + 1) * from) as usize];
            out[(y * w + x) as usize] = u8::from(row[a as usize..b as usize].contains(&1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrowing_keeps_thin_strokes() {
        // One row, eight wide, a single lit column: still lit at half the width.
        let row = [0, 0, 0, 0, 0, 1, 0, 0];
        assert_eq!(narrow(&row, 8, 4, 1), vec![0, 0, 1, 0]);
        assert_eq!(narrow(&row, 8, 8, 1), row.to_vec());
    }

    #[test]
    fn the_system_draws_cyrillic_and_chinese_inside_the_box() {
        let zhe = rasterize_glyph('Ж', 10, 18, 16).expect("Segoe UI has Cyrillic");
        assert_eq!(zhe.len(), 180);
        assert!(zhe.iter().filter(|&&p| p == 1).count() > 10);
        assert!(
            rasterize_glyph('漢', 22, 18, 16).is_some(),
            "font linking for Chinese"
        );
        assert_eq!(
            rasterize_glyph('\u{3000}', 22, 18, 16),
            Some(vec![0; 22 * 18])
        );
        assert_eq!(rasterize_glyph('Ж', 0, 18, 16), None);
    }

    #[test]
    fn a_character_with_nothing_to_draw_is_none() {
        assert_eq!(
            rasterize_glyph('\u{200b}', 10, 18, 16),
            None,
            "zero width space"
        );
    }
}
