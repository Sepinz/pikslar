//! Panels as slot bodies (docs/ui-redesign.md 3.2): each panel draws itself into the rectangle
//! its slot gives it, as it was drawn before the redesign.

use crate::app::App;
use crate::workspace::model::Panel;
use pikslar_render::{Rect, Surface};

impl App {
    /// Draws `panel` in `body`, in its folded form when `folded`.
    pub(crate) fn draw_panel(
        &mut self,
        sf: &mut Surface<'_>,
        panel: Panel,
        body: Rect,
        folded: bool,
    ) {
        match panel {
            Panel::Color => self.draw_color(sf, body, folded),
            Panel::Palette => self.draw_palette(sf, body, folded),
            Panel::Layers => self.draw_layers(sf, body, folded),
            Panel::Lint => self.draw_lint_panel(sf, body, folded),
            Panel::Tiles => self.draw_tiles(sf, body),
            Panel::Timeline => self.draw_timeline(sf, body),
            Panel::History => self.draw_history(sf, body),
            Panel::Preview => self.draw_preview_panel(sf, body),
            Panel::Slices | Panel::Scripts => {}
        }
    }
}
