//! The side dock panels' heights, and which part of a long list is in view.

use pikslar_render::Rect;

pub(crate) const FULL_COLOR: i32 = 150;
pub(crate) const COMPACT_COLOR: i32 = 74;
/// A folded color section: the swatch of the current color and its value, on one row.
pub(crate) const FOLDED_COLOR: i32 = 22;
/// The active layer's settings under the list, with the gap above them.
/// The opacity bar, the gap under it and the two buttons: 54 units, more where the bar grows.
pub(crate) fn layer_details() -> i32 {
    38 + pikslar_ui::fit_h(16)
}
pub(crate) const PALETTE_ROW: i32 = 22;
pub(crate) const LAYER_ROW: i32 = 26;

/// Which part of the layer and palette lists is in view, and where the wheel moves them.
#[derive(Debug, Default)]
pub(crate) struct PanelScroll {
    layer_first: usize,
    /// The active row and list shape last brought into view.
    followed: Option<(usize, usize, usize)>,
    palette_first: usize,
    /// The selected palette row last brought into view.
    palette_followed: Option<usize>,
    tiles_first: usize,
    layers: Rect,
    palette: Rect,
    tiles: Rect,
}

/// The lists of the right panel that scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum List {
    Layers,
    Palette,
    Tiles,
}

fn scrolled(first: usize, ticks: i32) -> usize {
    first.saturating_add_signed(-(ticks as isize))
}

impl PanelScroll {
    /// The first layer row to show. The active row is shown whenever it or the list changes,
    /// so the wheel can still look elsewhere in between.
    pub fn layer_window(
        &mut self,
        area: Rect,
        active: usize,
        visible: usize,
        count: usize,
    ) -> usize {
        self.layers = area;
        if self.followed != Some((active, visible, count)) {
            self.followed = Some((active, visible, count));
            self.layer_first = self
                .layer_first
                .min(active)
                .max((active + 1).saturating_sub(visible));
        }
        self.layer_first = self.layer_first.min(count.saturating_sub(visible));
        self.layer_first
    }

    /// The first palette row to show. A newly selected swatch is brought into view.
    pub fn palette_window(
        &mut self,
        area: Rect,
        rows: usize,
        total: usize,
        selected: Option<usize>,
    ) -> usize {
        self.palette = area;
        if self.palette_followed != selected {
            self.palette_followed = selected;
            if let Some(row) = selected {
                self.palette_first = self
                    .palette_first
                    .min(row)
                    .max((row + 1).saturating_sub(rows));
            }
        }
        self.palette_first = self.palette_first.min(total.saturating_sub(rows));
        self.palette_first
    }

    /// The first row of tiles to show, kept within the list.
    pub fn tiles_window(&mut self, area: Rect, rows: usize, total: usize) -> usize {
        self.tiles = area;
        self.tiles_first = self.tiles_first.min(total.saturating_sub(rows));
        self.tiles_first
    }

    /// Shows `list` from row `first`, as a drag on its scroll bar asks. The next draw keeps it
    /// within the list.
    pub fn set_first(&mut self, list: List, first: usize) {
        match list {
            List::Layers => self.layer_first = first,
            List::Palette => self.palette_first = first,
            List::Tiles => self.tiles_first = first,
        }
    }

    /// Takes `list` out of the wheel's reach while its section is folded.
    pub fn hide(&mut self, list: List) {
        match list {
            List::Layers => self.layers = Rect::default(),
            List::Palette => self.palette = Rect::default(),
            List::Tiles => self.tiles = Rect::default(),
        }
    }

    /// Scrolls the list under the pointer. Returns false if there is none.
    pub fn wheel(&mut self, ticks: i32, x: i32, y: i32) -> bool {
        if self.layers.contains(x, y) {
            self.layer_first = scrolled(self.layer_first, ticks);
            true
        } else if self.palette.contains(x, y) {
            self.palette_first = scrolled(self.palette_first, ticks);
            true
        } else if self.tiles.contains(x, y) {
            self.tiles_first = scrolled(self.tiles_first, ticks);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0, 0, 100, 100);

    #[test]
    fn a_newly_selected_swatch_is_brought_into_view() {
        let mut s = PanelScroll::default();
        assert_eq!(s.palette_window(AREA, 2, 10, None), 0);
        assert_eq!(s.palette_window(AREA, 2, 10, Some(7)), 6);
        s.wheel(3, 5, 5);
        assert_eq!(s.palette_window(AREA, 2, 10, Some(7)), 3);
    }

    #[test]
    fn scrolling_stops_at_both_ends() {
        let mut s = PanelScroll::default();
        assert_eq!(s.layer_window(AREA, 0, 3, 10), 0);
        assert!(s.wheel(-100, 5, 5));
        assert_eq!(s.layer_window(AREA, 0, 3, 10), 7);
        assert!(s.wheel(100, 5, 5));
        assert_eq!(s.layer_window(AREA, 0, 3, 10), 0);
    }

    #[test]
    fn the_wheel_can_look_away_from_the_active_layer() {
        let mut s = PanelScroll::default();
        s.layer_window(AREA, 0, 3, 10);
        s.wheel(-4, 5, 5);
        assert_eq!(s.layer_window(AREA, 0, 3, 10), 4);
    }

    #[test]
    fn changing_the_active_layer_brings_it_back_into_view() {
        let mut s = PanelScroll::default();
        s.layer_window(AREA, 0, 3, 10);
        s.wheel(-4, 5, 5);
        s.layer_window(AREA, 0, 3, 10);
        assert_eq!(s.layer_window(AREA, 9, 3, 10), 7);
        assert_eq!(s.layer_window(AREA, 2, 3, 10), 2);
    }

    #[test]
    fn a_shrinking_list_keeps_the_active_row_in_view() {
        let mut s = PanelScroll::default();
        s.layer_window(AREA, 8, 6, 10);
        assert_eq!(s.layer_window(AREA, 8, 2, 10), 7);
    }

    #[test]
    fn a_palette_that_fits_never_scrolls() {
        let mut s = PanelScroll::default();
        assert_eq!(s.palette_window(AREA, 2, 2, None), 0);
        assert!(s.wheel(-3, 5, 5));
        assert_eq!(s.palette_window(AREA, 2, 2, None), 0);
    }

    #[test]
    fn the_wheel_elsewhere_is_left_for_others() {
        let mut s = PanelScroll::default();
        s.layer_window(AREA, 0, 3, 10);
        assert!(!s.wheel(1, 500, 500));
    }
}
