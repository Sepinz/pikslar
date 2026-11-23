//! The palette menu, opened by a right click on the Palette header. The three kinds of change
//! are named apart: "Indices" rows change ids and never how anything looks, "Pixels" rows change
//! the art, and what a swatch looks like is changed by the sliders (see `docs/palette.md`).

use crate::app::App;
use crate::color_ui::hex;
use crate::frame_menu::{Entries, MenuKind, OpenMenu};
use pikslar_core::{LoadHow, PaletteCmd, RemapKind, SortBy};
use pikslar_platform::Window;
use std::borrow::Cow;

/// The steps a ramp can be made with.
pub(crate) const RAMP_STEPS: [u32; 4] = [3, 5, 7, 9];
const SORTS: [(&str, SortBy); 3] = [
    ("Sort by hue", SortBy::Hue),
    ("Sort by lightness", SortBy::Lightness),
    ("Sort by use, most first", SortBy::Usage),
];
const IMPORTS: [(&str, LoadHow); 3] = [
    ("Append to the swatches", LoadHow::Append),
    ("Replace the swatches", LoadHow::Replace),
    ("Load as colors (palette swap)", LoadHow::AsColors),
];
const REMAPS: [(&str, RemapKind, &str); 3] = [
    (
        "Indices: renumber to palette order",
        RemapKind::ToSwatchOrder,
        "Renumbered the colors to palette order. Nothing looks different",
    ),
    (
        "Indices: merge duplicate colors",
        RemapKind::MergeDuplicates,
        "Merged duplicate colors. Nothing looks different",
    ),
    (
        "Indices: remove unused colors",
        RemapKind::RemoveUnused,
        "Removed unused colors. Nothing looks different",
    ),
];

/// Where the main menu's rows are.
mod row {
    pub const SORT: usize = 0;
    pub const GROUPS: usize = 1;
    pub const RAMP: usize = 2;
    pub const REMAP: usize = 3;
    pub const REPLACE: usize = 6;
    pub const IMPORT: usize = 7;
    pub const EXPORT: usize = 8;
}

pub(crate) fn row_of(label: impl Into<Cow<'static, str>>, on: bool) -> (Cow<'static, str>, bool) {
    (label.into(), on)
}

impl App {
    /// Opens the palette menu if a right click at `(x, y)` is on the Palette header.
    pub(crate) fn open_palette_menu(&mut self, x: i32, y: i32) -> bool {
        if !self.palette_ui.header.contains(x, y) {
            return false;
        }
        self.open_palette_kind((x, y), MenuKind::Palette);
        true
    }

    pub(crate) fn open_palette_kind(&mut self, at: (i32, i32), kind: MenuKind) {
        self.menu = Some(OpenMenu { at, kind });
        self.ui_dirty = true;
    }

    /// The two swatches last clicked, earlier first: where a ramp starts and ends, or the color
    /// whose pixels change and the one they become. None unless both are swatches.
    pub(crate) fn swatch_pair(&self) -> Option<(u32, u32)> {
        let swatches = self.editor.doc().colors.swatches();
        let (from, to) = (self.palette_ui.previous?, self.selected_swatch?);
        let both = swatches.contains(&from) && swatches.contains(&to);
        (both && from != to).then_some((from, to))
    }

    fn pair_text(&self) -> String {
        let colors = &self.editor.doc().colors;
        self.swatch_pair().map_or_else(String::new, |(from, to)| {
            format!("{} to {}", hex(colors.color(from)), hex(colors.color(to)))
        })
    }

    pub(crate) fn palette_entries(&self, kind: MenuKind) -> Entries {
        match kind {
            MenuKind::PaletteSort => SORTS.iter().map(|&(l, _)| row_of(l, true)).collect(),
            MenuKind::PaletteImport => IMPORTS.iter().map(|&(l, _)| row_of(l, true)).collect(),
            MenuKind::PaletteRamp => RAMP_STEPS
                .iter()
                .map(|n| row_of(format!("{n} steps, {}", self.pair_text()), true))
                .collect(),
            MenuKind::PaletteReplace => self.replace_entries(),
            MenuKind::PaletteGroups(page) => self.group_list_entries(page),
            MenuKind::PaletteGroup(group) => self.group_entries(group),
            _ => self.palette_main_entries(),
        }
    }

    fn palette_main_entries(&self) -> Entries {
        let swatches = self.editor.doc().colors.swatches().len();
        let pair = self.swatch_pair().is_some();
        let mut rows = vec![
            row_of("Sort swatches...", swatches > 1),
            row_of("Groups and ramps...", true),
            row_of("New ramp...", pair),
        ];
        rows.extend(REMAPS.iter().map(|&(label, _, _)| row_of(label, true)));
        rows.push(row_of("Pixels: replace a color...", pair));
        rows.push(row_of("Import palette...", true));
        rows.push(row_of("Export palette...", swatches > 0));
        rows
    }

    fn replace_entries(&self) -> Entries {
        let text = self.pair_text();
        vec![
            row_of(format!("This layer: {text}"), true),
            row_of(format!("All layers: {text}"), true),
        ]
    }

    pub(crate) fn palette_menu_action(
        &mut self,
        kind: MenuKind,
        n: usize,
        at: (i32, i32),
        window: Window,
    ) {
        match kind {
            MenuKind::PaletteSort => self.sort_action(n),
            MenuKind::PaletteImport => {
                if let Some(&(_, how)) = IMPORTS.get(n) {
                    self.import_palette(how, window);
                }
            }
            MenuKind::PaletteRamp => self.ramp_action(n),
            MenuKind::PaletteReplace => self.replace_action(n),
            MenuKind::PaletteGroups(page) => self.group_list_action(page, n, at),
            MenuKind::PaletteGroup(group) => self.group_action(group, n),
            _ => self.palette_main_action(n, at, window),
        }
        self.ui_dirty = true;
    }

    fn palette_main_action(&mut self, n: usize, at: (i32, i32), window: Window) {
        match n {
            row::SORT => self.open_palette_kind(at, MenuKind::PaletteSort),
            row::GROUPS => self.open_palette_kind(at, MenuKind::PaletteGroups(0)),
            row::RAMP => self.open_palette_kind(at, MenuKind::PaletteRamp),
            row::REPLACE => self.open_palette_kind(at, MenuKind::PaletteReplace),
            row::IMPORT => self.open_palette_kind(at, MenuKind::PaletteImport),
            row::EXPORT => self.export_palette(window),
            _ => {
                if let Some(&(_, kind, said)) =
                    n.checked_sub(row::REMAP).and_then(|i| REMAPS.get(i))
                {
                    self.remap_action(kind, said);
                }
            }
        }
    }

    fn sort_action(&mut self, n: usize) {
        if let Some(&(_, by)) = SORTS.get(n) {
            self.run(PaletteCmd::Sort { by });
        }
    }

    /// Renumbers colors. Ids change, so a swatch chosen before no longer means what it did.
    fn remap_action(&mut self, kind: RemapKind, said: &'static str) {
        let Some(outcome) = self.run(PaletteCmd::Remap(kind)) else {
            return;
        };
        if outcome.changed {
            self.selected_swatch = None;
            self.palette_ui.previous = None;
            self.notify(said.to_string());
        } else {
            self.notify("Already so: nothing to change".to_string());
        }
    }

    fn ramp_action(&mut self, n: usize) {
        let (Some((from, to)), Some(&steps)) = (self.swatch_pair(), RAMP_STEPS.get(n)) else {
            return;
        };
        if let Some(outcome) = self.run(PaletteCmd::AddRamp { from, to, steps }) {
            self.palette_ui.marked = outcome.created.map(|g| g as usize);
            self.notify(format!("Made a ramp of {} colors", steps + 2));
        }
    }

    /// Changes pixels: the earlier swatch's pixels take the later swatch's id.
    fn replace_action(&mut self, n: usize) {
        let Some((from, to)) = self.swatch_pair() else {
            return;
        };
        let layer = match n {
            0 => self.editor.layer_id(self.editor.layer),
            _ => None,
        };
        if n == 0 && layer.is_none() {
            return;
        }
        let Some(outcome) = self.run(PaletteCmd::ReplaceColor { from, to, layer }) else {
            return;
        };
        let colors = &self.editor.doc().colors;
        let said = match outcome.changed {
            true => format!(
                "Pixels of {} now use {}",
                hex(colors.color(from)),
                hex(colors.color(to))
            ),
            false => format!("No pixels use {} there", hex(colors.color(from))),
        };
        self.notify(said);
    }
}
