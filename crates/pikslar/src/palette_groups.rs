//! Groups and ramps in the palette menu: made from the selected swatch, shown by marking their
//! members on the swatches, renamed in the header, and changed one member at a time. A group
//! never changes a color or an id.

use crate::frame_menu::{Entries, MenuKind};
use crate::palette_menu::row_of;
use crate::App;
use pikslar_core::{GroupKind, PaletteCmd, MAX_GROUPS};
use std::sync::Arc;

/// Groups per page of the list.
const PAGE: usize = 8;

impl App {
    /// The selected swatch, if it is one of the swatches shown.
    fn chosen_swatch(&self) -> Option<u32> {
        let swatches = self.editor.doc().colors.swatches();
        self.selected_swatch.filter(|id| swatches.contains(id))
    }

    /// The first "Group N" no group is called.
    fn default_group_name(&self) -> String {
        let groups = self.editor.doc().colors.groups();
        (1..)
            .map(|n| format!("Group {n}"))
            .find(|name| groups.iter().all(|g| &g.name != name))
            .unwrap_or_default()
    }

    pub(crate) fn group_list_entries(&self, page: usize) -> Entries {
        let groups = self.editor.doc().colors.groups();
        let room = groups.len() < MAX_GROUPS;
        let mut rows = vec![row_of(
            "New group from the selected swatch",
            room && self.chosen_swatch().is_some(),
        )];
        let start = page * PAGE;
        rows.extend(groups.iter().skip(start).take(PAGE).map(|g| {
            let ramp = if g.kind == GroupKind::Ramp {
                ", ramp"
            } else {
                ""
            };
            row_of(format!("{}  ({}{ramp})", g.name, g.ids.len()), true)
        }));
        if groups.len() > start + PAGE {
            rows.push(row_of("More...", true));
        }
        rows
    }

    pub(crate) fn group_entries(&self, group: usize) -> Entries {
        let Some(found) = self.editor.doc().colors.groups().get(group) else {
            return Vec::new();
        };
        let chosen = self.chosen_swatch();
        let member = chosen.is_some_and(|id| found.ids.contains(&id));
        let shown = self.palette_ui.marked == Some(group);
        vec![
            row_of(
                if shown {
                    "Hide members"
                } else {
                    "Show members"
                },
                true,
            ),
            row_of("Rename...", true),
            row_of("Add the selected swatch", chosen.is_some() && !member),
            row_of("Take out the selected swatch", member),
            row_of("Remove the group (swatches stay)", true),
        ]
    }

    pub(crate) fn group_list_action(&mut self, page: usize, n: usize, at: (i32, i32)) {
        if n == 0 {
            return self.new_group();
        }
        let index = page * PAGE + n - 1;
        if n == PAGE + 1 {
            return self.open_palette_kind(at, MenuKind::PaletteGroups(page + 1));
        }
        if index < self.editor.doc().colors.groups().len() {
            self.open_palette_kind(at, MenuKind::PaletteGroup(index));
        }
    }

    /// A group of the selected swatch, named at once so it can be typed over.
    fn new_group(&mut self) {
        let Some(id) = self.chosen_swatch() else {
            return;
        };
        let name = self.default_group_name();
        let ids: Arc<[u32]> = Arc::from([id]);
        let made = self.run(PaletteCmd::AddGroup { name, ids });
        if let Some(group) = made.and_then(|o| o.created) {
            self.palette_ui.marked = Some(group as usize);
            self.start_group_naming(group as usize);
        }
    }

    pub(crate) fn group_action(&mut self, group: usize, n: usize) {
        let chosen = self.chosen_swatch();
        match (n, chosen) {
            (0, _) => {
                let shown = self.palette_ui.marked == Some(group);
                self.palette_ui.marked = (!shown).then_some(group);
            }
            (1, _) => self.start_group_naming(group),
            (2, Some(id)) => {
                self.run(PaletteCmd::AddToGroup { group, id });
            }
            (3, Some(id)) => {
                self.run(PaletteCmd::RemoveFromGroup { group, id });
            }
            (4, _) => self.remove_group(group),
            _ => {}
        }
    }

    fn remove_group(&mut self, group: usize) {
        if self.run(PaletteCmd::RemoveGroup { group }).is_none() {
            return;
        }
        // Groups after it moved up one place.
        self.palette_ui.marked = match self.palette_ui.marked {
            Some(m) if m == group => None,
            Some(m) if m > group => Some(m - 1),
            other => other,
        };
    }
}
