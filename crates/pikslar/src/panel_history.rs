//! The History panel: Rewind's bar, which drags through every edit of this session. It has the
//! bar in the arrangements that place it where it can be reached (Animate keeps it as a tab
//! beside the Timeline); where it cannot be, the Timeline draws the bar itself in its status row.

use crate::app::App;
use crate::workspace::model::Panel;
use pikslar_render::{Rect, Surface};

impl App {
    /// Whether the History panel can be reached as the window is laid out now: placed in a slot
    /// that is open, in a dock that is not folded to its rail. Hidden behind another tab, it
    /// still counts, one click away.
    pub(crate) fn history_placed(&self) -> bool {
        let solved = &self.layout.solved;
        let Some((side, slot, _)) = solved.effective.find(Panel::History) else {
            return false;
        };
        let area = solved.area(side);
        !area.rail
            && area.rect.w > 0
            && area
                .slots()
                .get(slot)
                .is_some_and(|r| r.open && r.rect.h > 0)
    }

    /// The History panel in `body`: the Rewind bar and its counter, with the status plate.
    pub(crate) fn draw_history(&mut self, sf: &mut Surface<'_>, body: Rect) {
        let s = self.ui.scale;
        let bar = Rect::new(
            body.x + 8 * s,
            body.y + 6 * s,
            (body.w - 16 * s).max(0),
            20 * s,
        );
        self.draw_rewind(sf, bar);
    }
}
