//! The slot menu (docs/ui-redesign.md 3.4 and 3.7), opened from a tab strip's menu button, a
//! right click on a tab, or Shift+F10 and the Menu key on a focused tab: Move to another dock,
//! a new slot or beside another slot's panel; Fold; Hide the panel; Reset the workspace.

use crate::app::App;
use crate::dock::{label, with_slot};
use crate::frame_menu::{Entries, MenuKind, OpenMenu};
use crate::workspace::model::{Panel, Preset, Side, Slot, Workspace};
use crate::workspace::moves::{hide_panel, move_panel, Target};
use std::borrow::Cow;

/// What a row of the slot menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotOp {
    Move(Target),
    Fold,
    Hide,
    Reset,
}

/// The docks in the menu's order, with their words.
const DOCKS: [(Side, &str); 3] = [
    (Side::Right, "Move to the right dock"),
    (Side::Left, "Move to the left dock"),
    (Side::Bottom, "Move to the bottom dock"),
];

fn preset_name(p: Preset) -> &'static str {
    match p {
        Preset::Draw => "Draw",
        Preset::Animate => "Animate",
        Preset::Tiles => "Tiles",
        Preset::Palette => "Palette",
        Preset::Custom => "custom",
    }
}

impl App {
    /// Opens the slot menu for `panel`, shown in slot `slot` of `side`, at `at`.
    pub(crate) fn open_slot_menu(
        &mut self,
        (side, slot): (Side, usize),
        panel: Panel,
        at: (i32, i32),
    ) {
        self.finish_naming();
        self.playing = false;
        self.menu = Some(OpenMenu {
            at,
            kind: MenuKind::Slot { side, slot, panel },
        });
        self.ui_dirty = true;
    }

    /// A right click on a tab opens the slot menu for its panel. Returns whether it was one.
    /// The tab is the one drawn last frame; if its panel has left that slot since, as a preset
    /// chosen and not drawn yet does, the click is not taken as a tab's.
    pub(crate) fn open_tab_menu(&mut self, x: i32, y: i32) -> bool {
        let id = self.ui.widget_at(x, y);
        let Some(((side, i), panel)) = self.drawn_strips.tab_at(id) else {
            return false;
        };
        let shown = self.layout.solved.effective.area(side).slots().get(i);
        if !shown.is_some_and(|sl| sl.tabs.contains(panel)) {
            return false;
        }
        self.open_slot_menu((side, i), panel, (x, y));
        true
    }

    /// The rows: each with its label, whether it can be chosen, and what it does.
    fn slot_ops(
        &self,
        side: Side,
        slot: usize,
        panel: Panel,
    ) -> Vec<(Cow<'static, str>, bool, SlotOp)> {
        let ws = self.workspaces.active();
        let can = |op: SlotOp| self.slot_op_result(ws, (side, slot), panel, op).is_some();
        let mut rows: Vec<(Cow<'static, str>, SlotOp)> = DOCKS
            .iter()
            .map(|&(dock, word)| {
                let word = if dock == side {
                    "Move to a new slot"
                } else {
                    word
                };
                (Cow::Borrowed(word), SlotOp::Move(Target::NewSlot(dock)))
            })
            .collect();
        for (dock, _) in DOCKS {
            for (j, other) in ws.area(dock).slots().iter().enumerate() {
                let Some(beside) = other.active_panel().filter(|_| (dock, j) != (side, slot))
                else {
                    continue;
                };
                let word = format!("Move beside {}", label(beside));
                rows.push((Cow::Owned(word), SlotOp::Move(Target::Beside(dock, j))));
            }
        }
        let open = ws.area(side).slots().get(slot).is_none_or(|s| s.open);
        let fold = if open { "Fold" } else { "Unfold" };
        rows.push((Cow::Borrowed(fold), SlotOp::Fold));
        rows.push((Cow::Owned(format!("Hide {}", label(panel))), SlotOp::Hide));
        let reset = format!(
            "Reset the {} workspace",
            preset_name(self.workspaces.current)
        );
        rows.push((Cow::Owned(reset), SlotOp::Reset));
        rows.into_iter()
            .map(|(word, op)| (word, can(op), op))
            .collect()
    }

    pub(crate) fn dock_slot_entries(&self, side: Side, slot: usize, panel: Panel) -> Entries {
        self.slot_ops(side, slot, panel)
            .into_iter()
            .map(|(word, enabled, _)| (word, enabled))
            .collect()
    }

    /// The arrangement `op` leaves, or none when it cannot be done.
    fn slot_op_result(
        &self,
        ws: Workspace,
        at: (Side, usize),
        panel: Panel,
        op: SlotOp,
    ) -> Option<Workspace> {
        match op {
            SlotOp::Move(to) => move_panel(ws, panel, to),
            SlotOp::Hide => hide_panel(ws, panel),
            SlotOp::Fold => ws
                .area(at.0)
                .slots()
                .get(at.1)
                .filter(|sl| !sl.timeline_alone())
                .map(|_| ws),
            SlotOp::Reset => Some(ws),
        }
    }

    pub(crate) fn slot_menu_action(&mut self, (side, slot): (Side, usize), panel: Panel, n: usize) {
        let Some(&(_, true, op)) = self.slot_ops(side, slot, panel).get(n) else {
            return;
        };
        match op {
            SlotOp::Fold => self.fold_slot(side, slot),
            SlotOp::Reset => {
                self.workspaces = self.workspaces.reset(self.workspaces.current);
                self.follow_workspace();
            }
            SlotOp::Move(_) | SlotOp::Hide => {
                let ws = self.workspaces.active();
                if let Some(changed) = self.slot_op_result(ws, (side, slot), panel, op) {
                    self.tiles_shown |= panel == Panel::Tiles;
                    self.drawn_strips.refocus = true;
                    self.edit_layout(|_| unfolded(changed, panel));
                }
            }
        }
    }
}

/// A moved panel's area opened, so that it shows where it went.
fn unfolded(ws: Workspace, panel: Panel) -> Workspace {
    match ws.find(panel) {
        Some((side, i, _)) => {
            let ws = ws.with_area(side, ws.area(side).with_open(true));
            with_slot(ws, side, i, |sl| Slot { open: true, ..sl })
        }
        None => ws,
    }
}

#[cfg(test)]
#[path = "slot_menu_tests.rs"]
mod tests;
