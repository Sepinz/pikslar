//! The menu bar's and the Window menu's actions (docs/ui-redesign.md phase 7).

use crate::app::App;
use crate::menu_bar::WINDOW;
use crate::shortcuts::Action;
use crate::workspace::model::{Side, Workspace};

impl App {
    /// Does `action` if it is one of this phase's. Returns whether it was.
    pub(crate) fn window_action(&mut self, action: Action) -> bool {
        match action {
            Action::CanvasOnly => self.toggle_canvas_only(),
            Action::MenuBar => self.menu_bar_toggle(),
            Action::WindowMenu => self.open_bar_menu(WINDOW, true),
            Action::SaveCustomWorkspace => self.save_custom_workspace(),
            Action::ResetWorkspace => self.reset_workspace(),
            Action::LargeText => self.toggle_large_text(),
            _ => return false,
        }
        true
    }

    /// F11: every dock area shown as its rail, or the docks back as they were.
    pub(crate) fn toggle_canvas_only(&mut self) {
        self.window_ui.canvas_only = !self.window_ui.canvas_only;
        let hint = if self.window_ui.canvas_only {
            "Canvas only: a rail icon or the Window menu brings the docks back"
        } else {
            "The docks are back"
        };
        self.notify(hint.to_string());
        self.drawn_strips.refocus = true;
        self.ui_dirty = true;
        self.full_canvas = true;
        self.sync_matrix_layout();
    }

    /// Window > Reset: the arrangement in use goes back to its preset's own.
    pub(crate) fn reset_workspace(&mut self) {
        self.workspaces = self.workspaces.reset(self.workspaces.current);
        self.follow_workspace();
    }

    /// Window > Save as custom workspace: the arrangement in use is kept as the custom one.
    pub(crate) fn save_custom_workspace(&mut self) {
        self.workspaces = self.workspaces.save_custom("Custom");
        self.follow_workspace();
        self.notify("Saved as the custom workspace".to_string());
    }

    /// The arrangement as shown: the one in use, or, while the canvas is alone, every dock
    /// area folded to its rail. Nothing stored changes, so nothing has to be put back.
    pub(crate) fn shown_arrangement(&self, ws: Workspace) -> Workspace {
        if !self.window_ui.canvas_only {
            return ws;
        }
        Side::ALL.into_iter().fold(ws, |ws, side| {
            ws.with_area(side, ws.area(side).with_open(false))
        })
    }
}
