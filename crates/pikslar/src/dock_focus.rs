//! Keyboard focus when what has it goes away. A tab with focus follows its panel when the
//! arrangement changes; after a preset, a move or a hide, a control that is no longer drawn
//! hands focus to the first control of the region it was in. Otherwise (a slot folded by hand)
//! focus is dropped, so the arrows go back to the frames. Either way focus never points at a
//! widget that is not drawn.

use crate::app::App;
use crate::dock::strip_id;
use crate::workspace::model::Panel;
use pikslar_render::Rect;

impl App {
    /// The panel whose tab has focus, if a tab has it.
    pub(crate) fn focused_tab_panel(&self) -> Option<Panel> {
        let id = self.ui.focus.focused()?;
        self.drawn_strips.tab_at(id).map(|(_, p)| p)
    }

    /// After the arrangement changed: focus that was on `panel`'s tab goes to its tab where the
    /// panel is now. A panel no longer shown leaves it to [`App::refocus_after_draw`].
    pub(crate) fn refocus_tab(&mut self, panel: Option<Panel>) {
        let Some(found) = panel.and_then(|p| self.view_layout().find(p)) else {
            return;
        };
        let (side, i, t) = found;
        self.ui.focus.set(strip_id(side, i) + 1 + t as u32);
    }

    /// Where the control with focus was last drawn, before a draw pass.
    pub(crate) fn focus_before_draw(&self) -> Option<Rect> {
        let id = self.ui.focus.focused()?;
        self.ui
            .widgets()
            .iter()
            .rev()
            .find(|&&(_, w)| w == id)
            .map(|&(r, _)| r)
    }

    /// After a draw pass that dropped focus because its control was not drawn, when the
    /// arrangement changed as a whole: the first control drawn in the region the old one was
    /// in takes it.
    pub(crate) fn refocus_after_draw(&mut self, was: Option<Rect>) {
        let wanted = std::mem::take(&mut self.drawn_strips.refocus);
        let lost = self.ui.focus.focused().is_none();
        let Some(was) = was.filter(|_| wanted && lost) else {
            return;
        };
        let (cx, cy) = (was.x + was.w / 2, was.y + was.h / 2);
        let Some(region) = self.regions().into_iter().find(|g| g.contains(cx, cy)) else {
            return;
        };
        let widgets = self.ui.widgets();
        let first = self.ui.focus.order().iter().copied().find(|&id| {
            widgets
                .iter()
                .any(|&(r, w)| w == id && region.contains(r.x, r.y))
        });
        if let Some(id) = first {
            self.ui.focus.set(id);
            self.ui_dirty = true;
        }
    }
}
