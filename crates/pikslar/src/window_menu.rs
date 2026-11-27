//! The Window menu (docs/ui-redesign.md 3.6): the workspaces, every panel with Show or Hide (the
//! way back for a panel hidden by hand), Reset, Save as custom and Canvas only. Each row says
//! its state in words, never by colour alone: "in use", "Show", "Hide", "always shown".

use crate::app::App;
use crate::dock::label;
use crate::frame_menu::Entries;
use crate::shortcuts::Action;
use crate::workspace::model::{Panel, Preset, Side, Workspace};
use crate::workspace::moves::{hide_panel, move_panel, Target};
use crate::workspace::presets::default_layout;
use std::borrow::Cow;

/// What a row of the Window menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowRow {
    Preset(Preset),
    Panel(Panel),
    Reset,
    SaveCustom,
    CanvasOnly,
}

fn preset_action(p: Preset) -> Option<Action> {
    Some(match p {
        Preset::Draw => Action::WorkspaceDraw,
        Preset::Animate => Action::WorkspaceAnimate,
        Preset::Tiles => Action::WorkspaceTiles,
        Preset::Palette => Action::WorkspacePalette,
        Preset::Custom => return None,
    })
}

fn preset_word(p: Preset) -> &'static str {
    match p {
        Preset::Draw => "Draw",
        Preset::Animate => "Animate",
        Preset::Tiles => "Tiles",
        Preset::Palette => "Palette",
        Preset::Custom => "Custom",
    }
}

impl App {
    fn preset_rows(&self) -> Vec<(Cow<'static, str>, bool, WindowRow)> {
        let current = self.workspaces.current;
        let mut presets: Vec<(Preset, String)> = Preset::BUILT_IN
            .iter()
            .map(|&p| (p, preset_word(p).to_string()))
            .collect();
        if let Some(custom) = &self.workspaces.custom {
            presets.push((Preset::Custom, custom.name.clone()));
        }
        presets
            .into_iter()
            .map(|(p, name)| {
                let state = if p == current { ", in use" } else { "" };
                let text = format!("{name} workspace{state}");
                let label = match preset_action(p) {
                    Some(a) => self.keymap.tab_hint(&text, a),
                    None => text,
                };
                (Cow::Owned(label), true, WindowRow::Preset(p))
            })
            .collect()
    }

    fn panel_rows(&self) -> Vec<(Cow<'static, str>, bool, WindowRow)> {
        let placed = self.workspaces.active().placed();
        Panel::ALL
            .iter()
            .map(|&p| {
                let (text, enabled) = match (p.can_hide(), placed.contains(p)) {
                    (false, _) => (format!("{}, always shown", label(p)), false),
                    (true, true) => (format!("Hide {}", label(p)), true),
                    (true, false) => (format!("Show {}", label(p)), true),
                };
                (Cow::Owned(text), enabled, WindowRow::Panel(p))
            })
            .collect()
    }

    fn window_rows(&self) -> Vec<(Cow<'static, str>, bool, WindowRow)> {
        let key = |text: String, a: Action| Cow::Owned(self.keymap.tab_hint(&text, a));
        let mut rows = self.preset_rows();
        rows.extend(self.panel_rows());
        let current = self.workspaces.current;
        let reset = format!("Reset the {} workspace", preset_word(current));
        rows.push((
            key(reset, Action::ResetWorkspace),
            current != Preset::Custom,
            WindowRow::Reset,
        ));
        let save = "Save as custom workspace".to_string();
        rows.push((
            key(save, Action::SaveCustomWorkspace),
            true,
            WindowRow::SaveCustom,
        ));
        let canvas = if self.window_ui.canvas_only {
            "Show the docks"
        } else {
            "Canvas only"
        };
        rows.push((
            key(canvas.to_string(), Action::CanvasOnly),
            true,
            WindowRow::CanvasOnly,
        ));
        rows
    }

    /// How many workspace rows the Window menu starts with.
    pub(crate) fn preset_count(&self) -> usize {
        self.preset_rows().len()
    }

    /// How many panel rows follow them.
    pub(crate) fn panel_count(&self) -> usize {
        self.panel_rows().len()
    }

    pub(crate) fn window_entries(&self) -> Entries {
        self.window_rows()
            .into_iter()
            .map(|(l, e, _)| (l, e))
            .collect()
    }

    pub(crate) fn window_menu_action(&mut self, n: usize) {
        let Some(&(_, true, row)) = self.window_rows().get(n) else {
            return;
        };
        match row {
            WindowRow::Preset(p) => self.choose_preset(p),
            WindowRow::Panel(p) => self.toggle_panel(p),
            WindowRow::Reset => self.reset_workspace(),
            WindowRow::SaveCustom => self.save_custom_workspace(),
            WindowRow::CanvasOnly => self.toggle_canvas_only(),
        }
        self.ui_dirty = true;
    }

    /// Hides `p` if it is shown, and shows it again, where it came from, if it is not.
    pub(crate) fn toggle_panel(&mut self, p: Panel) {
        let ws = self.workspaces.active();
        let showing = !ws.placed().contains(p);
        let changed = if showing {
            self.show_panel(ws, p)
        } else {
            hide_panel(ws, p)
        };
        let Some(changed) = changed else {
            return;
        };
        self.tiles_shown = showing && p == Panel::Tiles;
        self.drawn_strips.refocus = true;
        self.window_ui.canvas_only = false;
        self.edit_layout(|_| changed);
    }

    /// `ws` with `p` placed: beside the panels it shared a slot with in the preset's own
    /// arrangement, else in a new slot of the dock it has there, else wherever it is allowed.
    fn show_panel(&self, ws: Workspace, p: Panel) -> Option<Workspace> {
        show_targets(ws, p, self.workspaces.current)
            .into_iter()
            .find_map(|target| move_panel(ws, p, target))
    }
}

/// Where `p` may be put, best first.
fn show_targets(ws: Workspace, p: Panel, current: Preset) -> Vec<Target> {
    let preset = if current == Preset::Custom {
        Preset::Draw
    } else {
        current
    };
    let mut out = Vec::new();
    let home = default_layout(preset);
    if let Some((side, i, _)) = home.find(p) {
        let mates = home.area(side).slots()[i].tabs;
        let beside = mates
            .as_slice()
            .iter()
            .filter(|&&q| q != p)
            .find_map(|&q| ws.find(q).filter(|&(s, ..)| s == side));
        out.push(match beside {
            Some((_, j, _)) => Target::Beside(side, j),
            None => Target::NewSlot(side),
        });
    }
    for side in [Side::Right, Side::Left, Side::Bottom] {
        out.extend((0..ws.area(side).slots().len()).map(|j| Target::Beside(side, j)));
        out.push(Target::NewSlot(side));
    }
    out
}

#[cfg(test)]
#[path = "window_menu_tests.rs"]
mod tests;
