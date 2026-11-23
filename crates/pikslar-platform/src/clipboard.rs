//! Exchanging images with other programs through the Windows clipboard.
//!
//! Images are written as a bottom-up 32-bit DIBV5 with straight alpha. Windows derives
//! the older formats from that for programs that want them.

use crate::ffi::*;
use crate::window::Window;
use std::ptr::copy_nonoverlapping;

const V5_HEADER: usize = 124;
const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
const SRGB: u32 = 0x7352_4742;
const INTENT_IMAGES: u32 = 4;
/// Anything larger than this cannot be pasted into a sprite anyway.
const MAX_SIDE: i32 = 16_384;

/// Pixels in the same straight-alpha format as RGBA layers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardImage {
    pub width: usize,
    pub height: usize,
    pub px: Vec<u32>,
}

fn put_u32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn to_bgra(rgba: u32) -> u32 {
    let [r, g, b, a] = rgba.to_le_bytes();
    u32::from_le_bytes([b, g, r, a])
}

/// Builds the bytes of a DIBV5 for an image.
fn encode_dib(width: usize, height: usize, px: &[u32]) -> Vec<u8> {
    let image_bytes = width * height * 4;
    let mut out = vec![0u8; V5_HEADER + image_bytes];
    put_u32(&mut out, 0, V5_HEADER as u32);
    put_u32(&mut out, 4, width as u32);
    put_u32(&mut out, 8, height as u32);
    out[12] = 1;
    out[14] = 32;
    put_u32(&mut out, 16, BI_BITFIELDS);
    put_u32(&mut out, 20, image_bytes as u32);
    put_u32(&mut out, 40, 0x00ff_0000);
    put_u32(&mut out, 44, 0x0000_ff00);
    put_u32(&mut out, 48, 0x0000_00ff);
    put_u32(&mut out, 52, 0xff00_0000);
    put_u32(&mut out, 56, SRGB);
    put_u32(&mut out, 108, INTENT_IMAGES);
    for (row, target) in out[V5_HEADER..].chunks_exact_mut(width * 4).enumerate() {
        let source = &px[(height - 1 - row) * width..][..width];
        for (cell, &color) in target.chunks_exact_mut(4).zip(source) {
            cell.copy_from_slice(&to_bgra(color).to_le_bytes());
        }
    }
    out
}

fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// Channel positions of a 32-bit image: red, green, blue shifts and the alpha shift if any.
struct Layout {
    shifts: [u32; 3],
    alpha: Option<u32>,
    pixels_at: usize,
}

fn read_layout(data: &[u8], header: usize, compression: u32, colors_used: u32) -> Option<Layout> {
    let table = colors_used as usize * 4;
    if compression == BI_RGB {
        return Some(Layout {
            shifts: [16, 8, 0],
            alpha: Some(24),
            pixels_at: header + table,
        });
    }
    let masks_inline = header >= 56;
    let masks = if masks_inline { 40 } else { header };
    let pixels_at = header + table + if masks_inline { 0 } else { 12 };
    let shift = |at: usize| {
        read_u32(data, at)
            .filter(|&m| m != 0)
            .map(u32::trailing_zeros)
    };
    let alpha = if masks_inline {
        read_u32(data, masks + 12)
            .filter(|&m| m != 0)
            .map(u32::trailing_zeros)
    } else {
        None
    };
    Some(Layout {
        shifts: [shift(masks)?, shift(masks + 4)?, shift(masks + 8)?],
        alpha,
        pixels_at,
    })
}

/// Reads a 24 or 32-bit DIB. Anything malformed or unusual returns `None`.
fn decode_dib(data: &[u8]) -> Option<ClipboardImage> {
    let header = read_u32(data, 0)? as usize;
    let width = read_u32(data, 4)? as i32;
    let raw_height = read_u32(data, 8)? as i32;
    let bits = u16::from_le_bytes(data.get(14..16)?.try_into().ok()?) as usize;
    let compression = read_u32(data, 16)?;
    let colors_used = read_u32(data, 32)?;
    let height = raw_height.checked_abs()?;
    let usable = width > 0 && height > 0 && width <= MAX_SIDE && height <= MAX_SIDE;
    let known = matches!(
        (bits, compression),
        (24, BI_RGB) | (32, BI_RGB | BI_BITFIELDS)
    );
    if !usable || !known || header < 40 || colors_used > 256 {
        return None;
    }
    let layout = read_layout(data, header, compression, colors_used)?;
    let (w, h) = (width as usize, height as usize);
    let stride = (w * bits).div_ceil(32) * 4;
    let body = data.get(layout.pixels_at..layout.pixels_at.checked_add(stride * h)?)?;
    let bytes_per_pixel = bits / 8;
    let mut px = vec![0u32; w * h];
    for (row, line) in body.chunks_exact(stride).enumerate() {
        let target = if raw_height > 0 { h - 1 - row } else { row };
        for (cell, p) in px[target * w..][..w]
            .iter_mut()
            .zip(line.chunks_exact(bytes_per_pixel))
        {
            let mut raw = [0u8, 0, 0, 255];
            raw[..bytes_per_pixel].copy_from_slice(p);
            *cell = u32::from_le_bytes(raw);
        }
    }
    let has_alpha = bits == 32 && layout.alpha.is_some() && px.iter().any(|p| p >> 24 != 0);
    let channel = |p: u32, shift: u32| (p >> shift) as u8;
    for p in &mut px {
        let [r, g, b] = layout.shifts.map(|s| channel(*p, s));
        let a = match layout.alpha.filter(|_| has_alpha) {
            Some(shift) => channel(*p, shift),
            None => 255,
        };
        *p = if a == 0 {
            0
        } else {
            u32::from_le_bytes([r, g, b, a])
        };
    }
    Some(ClipboardImage {
        width: w,
        height: h,
        px,
    })
}

/// Changes whenever anything writes to the clipboard.
pub fn clipboard_stamp() -> u32 {
    // SAFETY: takes no arguments and only reads a system counter.
    unsafe { GetClipboardSequenceNumber() }
}

/// Places an image on the clipboard. Returns false if the clipboard was unavailable.
pub fn copy_image(window: Window, width: usize, height: usize, px: &[u32]) -> bool {
    if window.is_detached() || width == 0 || height == 0 || px.len() != width * height {
        return false;
    }
    let bytes = encode_dib(width, height, px);
    // SAFETY: the allocation is exactly `bytes.len()` long and is locked while written. The
    // clipboard takes ownership of it only when SetClipboardData succeeds; otherwise it is
    // freed here. The clipboard is opened and closed on this thread within this block.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if memory.is_null() {
            return false;
        }
        let target = GlobalLock(memory) as *mut u8;
        if target.is_null() {
            GlobalFree(memory);
            return false;
        }
        copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        GlobalUnlock(memory);
        if OpenClipboard(window.raw()) == 0 {
            GlobalFree(memory);
            return false;
        }
        EmptyClipboard();
        let stored = !SetClipboardData(CF_DIBV5, memory).is_null();
        if !stored {
            GlobalFree(memory);
        }
        CloseClipboard();
        stored
    }
}

/// Reads an image from the clipboard, if it holds one Pikslar can use.
pub fn paste_image(window: Window) -> Option<ClipboardImage> {
    if window.is_detached() {
        return None;
    }
    // SAFETY: the clipboard owns the handle and it stays valid while the clipboard is open.
    // The locked region is `GlobalSize` bytes long and is only read within that range, and
    // it is copied out before the clipboard is closed.
    unsafe {
        if OpenClipboard(window.raw()) == 0 {
            return None;
        }
        let format = [CF_DIBV5, CF_DIB]
            .into_iter()
            .find(|&f| IsClipboardFormatAvailable(f) != 0);
        let data = format.map_or(std::ptr::null_mut(), |f| GetClipboardData(f));
        let mut image = None;
        if !data.is_null() {
            let locked = GlobalLock(data) as *const u8;
            if !locked.is_null() {
                let bytes = std::slice::from_raw_parts(locked, GlobalSize(data));
                image = decode_dib(bytes);
                GlobalUnlock(data);
            }
        }
        CloseClipboard();
        image
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOLID: u32 = u32::from_le_bytes([10, 20, 30, 255]);
    const GLASS: u32 = u32::from_le_bytes([200, 100, 50, 128]);

    #[test]
    fn images_survive_a_round_trip_with_alpha() {
        let px = vec![SOLID, GLASS, 0, SOLID, GLASS, 0];
        let image = decode_dib(&encode_dib(3, 2, &px)).expect("readable");
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(image.px, px);
    }

    #[test]
    fn old_style_images_without_alpha_are_opaque() {
        let mut dib = vec![0u8; 40 + 8];
        put_u32(&mut dib, 0, 40);
        put_u32(&mut dib, 4, 1);
        put_u32(&mut dib, 8, 2);
        dib[12] = 1;
        dib[14] = 32;
        dib[40..44].copy_from_slice(&[30, 20, 10, 0]);
        dib[44..48].copy_from_slice(&[3, 2, 1, 0]);
        let image = decode_dib(&dib).expect("readable");
        assert_eq!(image.px[1], SOLID);
        assert_eq!(image.px[0], u32::from_le_bytes([1, 2, 3, 255]));
    }

    #[test]
    fn twenty_four_bit_rows_use_their_padding() {
        let mut dib = vec![0u8; 40 + 8];
        put_u32(&mut dib, 0, 40);
        put_u32(&mut dib, 4, 1);
        put_u32(&mut dib, 8, (-2i32) as u32);
        dib[12] = 1;
        dib[14] = 24;
        dib[40..43].copy_from_slice(&[30, 20, 10]);
        dib[44..47].copy_from_slice(&[3, 2, 1]);
        let image = decode_dib(&dib).expect("readable");
        assert_eq!(image.px[0], SOLID);
        assert_eq!(image.px[1], u32::from_le_bytes([1, 2, 3, 255]));
    }

    #[test]
    fn malformed_images_are_rejected_without_panicking() {
        assert!(decode_dib(&[]).is_none());
        assert!(decode_dib(&[40, 0, 0, 0]).is_none());
        let mut dib = encode_dib(2, 2, &[SOLID; 4]);
        dib.truncate(V5_HEADER + 3);
        assert!(decode_dib(&dib).is_none());
        let mut huge = encode_dib(1, 1, &[SOLID]);
        put_u32(&mut huge, 4, 0x7fff_ffff);
        assert!(decode_dib(&huge).is_none());
        let mut negative = encode_dib(1, 1, &[SOLID]);
        put_u32(&mut negative, 8, i32::MIN as u32);
        assert!(decode_dib(&negative).is_none());
    }

    #[test]
    fn detached_windows_never_touch_the_clipboard() {
        assert!(!copy_image(Window::detached(), 1, 1, &[SOLID]));
        assert!(paste_image(Window::detached()).is_none());
    }
}
