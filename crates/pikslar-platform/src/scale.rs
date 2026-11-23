//! The interface size a person chose, and the scale it gives in a window of a given size.

use crate::window::interface_scale;
use std::sync::atomic::{AtomicI32, Ordering};

/// The fewest units a chosen size must leave. The interface needs 640 across, and from 478
/// down it keeps two layer rows, the tool strip and the frame strip.
pub const SMALLEST_UNITS: (i32, i32) = (640, 480);
/// The largest size offered.
pub const LARGEST_SIZE: i32 = 3;

/// The chosen size, or 0 for automatic.
static CHOSEN: AtomicI32 = AtomicI32::new(0);
/// Windows' text size, in percent.
static TEXT_PERCENT: AtomicI32 = AtomicI32::new(100);

/// The text size Settings, Accessibility, Text size sets, from the registry value it writes, in
/// its range of 100 to 225 percent. Not set means 100.
pub fn text_percent_from(value: Option<u32>) -> i32 {
    value.map_or(100, |v| v.clamp(100, 225) as i32)
}

pub(crate) fn text_percent() -> i32 {
    TEXT_PERCENT.load(Ordering::Relaxed)
}

/// Keeps `percent` as the text size. Returns whether it changed.
pub(crate) fn set_text_percent(percent: i32) -> bool {
    TEXT_PERCENT.swap(percent.clamp(100, 225), Ordering::Relaxed) != percent.clamp(100, 225)
}

/// Whether `scale` leaves the interface its room in a client of `w` by `h` pixels.
pub fn size_fits(scale: i32, w: i32, h: i32) -> bool {
    scale >= 1 && w / scale >= SMALLEST_UNITS.0 && h / scale >= SMALLEST_UNITS.1
}

/// The scale to use: automatic follows the display while it leaves enough room; a chosen size
/// is kept while it fits, and gives way to the largest smaller size that does when the window
/// shrinks.
pub fn chosen_scale(chosen: Option<i32>, dpi: i32, w: i32, h: i32) -> i32 {
    chosen_scale_with_text(chosen, dpi, 100, w, h)
}

/// [`chosen_scale`], with Windows' text size in percent. The interface font is a bitmap drawn at
/// whole multiples, so a larger text size asks for a larger automatic scale, text and the
/// controls holding it together, as if the display had that many more dots per inch. A chosen
/// size is the person's own and is kept.
pub fn chosen_scale_with_text(chosen: Option<i32>, dpi: i32, text: i32, w: i32, h: i32) -> i32 {
    match chosen {
        None => interface_scale(dpi * text.clamp(100, 225) / 100, w, h).min(LARGEST_SIZE),
        Some(size) => (1..=size.clamp(1, LARGEST_SIZE))
            .rev()
            .find(|&s| size_fits(s, w, h))
            .unwrap_or(1),
    }
}

pub(crate) fn chosen() -> Option<i32> {
    let size = CHOSEN.load(Ordering::Relaxed);
    (size > 0).then_some(size)
}

pub(crate) fn set_chosen(size: Option<i32>) {
    let size = size.map_or(0, |s| s.clamp(1, LARGEST_SIZE));
    CHOSEN.store(size, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_hd_laptop_at_150_can_choose_2x_but_not_3x() {
        assert_eq!(
            chosen_scale(None, 144, 1920, 1003),
            2,
            "automatic follows the display while 640 by 480 units fit"
        );
        assert_eq!(chosen_scale(Some(2), 144, 1920, 1003), 2);
        assert!(size_fits(2, 1920, 1003));
        assert!(!size_fits(3, 1920, 1003), "640 by 334 units");
        assert_eq!(
            chosen_scale(Some(3), 144, 1920, 1003),
            2,
            "the largest that fits"
        );
    }

    #[test]
    fn a_chosen_size_gives_way_in_a_small_window_and_never_goes_below_1x() {
        assert_eq!(chosen_scale(Some(2), 96, 1279, 1003), 1);
        assert_eq!(chosen_scale(Some(2), 96, 1280, 960), 2);
        assert_eq!(chosen_scale(Some(2), 96, 1280, 959), 1);
        assert_eq!(chosen_scale(Some(3), 96, 300, 200), 1);
        assert_eq!(
            chosen_scale(Some(1), 288, 3840, 2050),
            1,
            "1x on a 4K display"
        );
        assert_eq!(
            chosen_scale(Some(9), 96, 3840, 2160),
            3,
            "no size beyond 3x"
        );
    }

    #[test]
    fn windows_text_size_raises_the_automatic_scale_only() {
        assert_eq!(chosen_scale_with_text(None, 96, 100, 1920, 1003), 1);
        assert_eq!(chosen_scale_with_text(None, 96, 125, 1920, 1003), 1);
        assert_eq!(chosen_scale_with_text(None, 96, 150, 1920, 1003), 2);
        assert_eq!(
            chosen_scale_with_text(None, 96, 225, 1920, 1003),
            2,
            "3x does not fit"
        );
        assert_eq!(
            chosen_scale_with_text(None, 96, 225, 3840, 2050),
            2,
            "2.25 rounds to 2"
        );
        assert_eq!(chosen_scale_with_text(None, 144, 225, 3840, 2050), 3);
        assert_eq!(
            chosen_scale_with_text(None, 192, 200, 3840, 2050),
            3,
            "no more than 3x"
        );
        assert_eq!(
            chosen_scale_with_text(None, 96, 200, 1000, 700),
            1,
            "too small for 2x"
        );
        assert_eq!(
            chosen_scale_with_text(Some(1), 96, 200, 1920, 1003),
            1,
            "a chosen size is kept"
        );
    }

    #[test]
    fn the_text_size_is_read_as_a_percentage_in_its_range() {
        assert_eq!(text_percent_from(None), 100, "not set");
        assert_eq!(text_percent_from(Some(100)), 100);
        assert_eq!(text_percent_from(Some(175)), 175);
        assert_eq!(text_percent_from(Some(40)), 100, "below the range");
        assert_eq!(text_percent_from(Some(900)), 225, "above it");
    }

    #[test]
    fn the_choice_is_remembered_and_clamped() {
        set_chosen(Some(2));
        assert_eq!(chosen(), Some(2));
        set_chosen(Some(7));
        assert_eq!(chosen(), Some(LARGEST_SIZE));
        set_chosen(None);
        assert_eq!(chosen(), None);
    }
}
