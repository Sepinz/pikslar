//! The context bar's actions (docs/ui-redesign.md phase 5): focus goes to the bar.

use crate::app::App;
use crate::shortcuts::Action;

impl App {
    /// Does `action` if it is one of this phase's. Returns whether it was.
    pub(crate) fn context_bar_action(&mut self, action: Action) -> bool {
        if action != Action::ContextBar {
            return false;
        }
        self.focus_context_bar();
        true
    }

    /// Moves keyboard focus to the first control of the context bar, if it has one.
    pub(crate) fn focus_context_bar(&mut self) {
        let bar = self.layout.context;
        let widgets = self.ui.widgets();
        let first = self.ui.focus.order().iter().copied().find(|&id| {
            widgets
                .iter()
                .any(|&(r, w)| w == id && bar.contains(r.x, r.y))
        });
        if let Some(id) = first {
            self.focus_on(id);
        }
    }
}
