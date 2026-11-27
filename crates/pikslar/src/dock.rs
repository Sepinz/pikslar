//! The dock areas (docs/ui-redesign.md 3.3): each slot's tab strip over its active panel's
//! body, and a folded area's rail. What the strips and rails ask changes the arrangement in
//! use, through [`App::edit_layout`].

use crate::app::App;
use crate::layout::{bottom_strip, BODY_X, BODY_Y, DOCK_EDGE};
use crate::panel_fit::List;
use crate::shortcuts::Action;
use crate::workspace::model::{Panel, Preset, Side, Slot, Workspace, PANEL_COUNT};
use crate::workspace::solve::SlotRect;
use pikslar_render::{Rect, Surface};
use pikslar_ui::icons::ActionIcon;
use pikslar_ui::rail::{rail, RailIcon, RailItem};
use pikslar_ui::tabs::{tab_strip, Strip, TabEvent, BUTTON_W as FOLD_W, TAB_IDS};
use pikslar_ui::theme;
use std::borrow::Cow;

/// The docks' controls: a tab strip per slot from here, [`TAB_IDS`] each.
pub(crate) const ID_DOCK: u32 = 760_000;
/// The rails, [`RAIL_IDS`] each, after the strips.
const ID_RAIL: u32 = ID_DOCK + 9 * TAB_IDS;
const RAIL_IDS: u32 = 16;

/// The id of the tab strip of slot `slot` in `side`.
pub(crate) fn strip_id(side: Side, slot: usize) -> u32 {
    ID_DOCK + (side.index() as u32 * 3 + slot as u32) * TAB_IDS
}

/// The slots as their strips were last drawn, by strip, so that a click on last frame's tabs
/// means the panel that was shown there, whatever the layout has become since.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DrawnStrips {
    slots: [Option<Slot>; 9],
    /// The arrangement changed as a whole (a preset, a panel moved or hidden): a control with
    /// focus that is not drawn any more hands focus on within its region.
    pub(crate) refocus: bool,
}

impl DrawnStrips {
    /// The slot and panel of the tab drawn as widget `id`, if it was one.
    pub(crate) fn tab_at(&self, id: u32) -> Option<((Side, usize), Panel)> {
        let at = id.checked_sub(ID_DOCK)?;
        let (k, tab) = ((at / TAB_IDS) as usize, (at % TAB_IDS) as usize);
        let slot = (*self.slots.get(k)?)?;
        let panel = *slot.tabs.as_slice().get(tab.checked_sub(1)?)?;
        Some(((Side::ALL[k / 3], k % 3), panel))
    }
}

/// The words a panel's tab, rail icon and menus show.
pub(crate) fn label(p: Panel) -> &'static str {
    match p {
        Panel::Color => "Color",
        Panel::Palette => "Palette",
        Panel::Layers => "Layers",
        Panel::Timeline => "Timeline",
        Panel::History => "History",
        Panel::Tiles => "Tiles",
        Panel::Preview => "Preview",
        Panel::Lint => "Pixel Lint",
        Panel::Slices => "Slices",
        Panel::Scripts => "Scripts",
    }
}

fn art(p: Panel) -> ActionIcon {
    match p {
        Panel::Color => ActionIcon::Ramp,
        Panel::Palette => ActionIcon::LivePalette,
        Panel::Layers => ActionIcon::Stack,
        Panel::Timeline => ActionIcon::Play,
        Panel::Preview => ActionIcon::TrueSize,
        Panel::History => ActionIcon::Loop,
        Panel::Tiles => ActionIcon::TileModeTiles,
        Panel::Slices => ActionIcon::Grid,
        Panel::Lint => ActionIcon::Warning,
        Panel::Scripts => ActionIcon::ScriptRun,
    }
}

/// The labels of a slot's tabs, without allocating.
fn labels(slot: &Slot) -> ([&'static str; PANEL_COUNT], usize) {
    let mut out = [""; PANEL_COUNT];
    let tabs = slot.tabs.as_slice();
    for (o, &p) in out.iter_mut().zip(tabs) {
        *o = label(p);
    }
    (out, tabs.len())
}

/// `ws` with slot `i` of `side` changed by `f`.
pub(crate) fn with_slot(
    ws: Workspace,
    side: Side,
    i: usize,
    f: impl FnOnce(Slot) -> Slot,
) -> Workspace {
    let area = *ws.area(side);
    match area.slots().get(i) {
        Some(&slot) => ws.with_area(side, area.with_slot(i, f(slot))),
        None => ws,
    }
}

/// Where a slot's tab strip and its panel's body go in the slot's rectangle.
fn slot_parts(side: Side, r: Rect, strip: bool, s: i32) -> (Rect, Rect) {
    let strip_h = if strip { crate::layout::strip() * s } else { 0 };
    let (x, w) = match side {
        Side::Right => (r.x + DOCK_EDGE * s, r.w - DOCK_EDGE * s),
        Side::Left => (r.x, r.w - DOCK_EDGE * s),
        Side::Bottom => (r.x, r.w),
    };
    let strip_r = Rect::new(x, r.y, w.max(0), strip_h.min(r.h));
    let body = match side {
        Side::Bottom => Rect::new(r.x, r.y + strip_h, r.w, (r.h - strip_h).max(0)),
        _ => Rect::new(
            r.x + BODY_X * s,
            strip_r.bottom() + BODY_Y * s,
            (r.w - 2 * BODY_X * s).max(0),
            (r.h - strip_h - 2 * BODY_Y * s).max(0),
        ),
    };
    (strip_r, body)
}

impl App {
    /// Draws the side docks and the bottom dock.
    pub(crate) fn draw_docks(&mut self, sf: &mut Surface<'_>) {
        self.sync_color();
        self.drawn_strips.slots = Default::default();
        // A list drawn last pass may not be drawn this one: its wheel must not stay live.
        for list in [List::Layers, List::Palette, List::Tiles] {
            self.panel_scroll.hide(list);
        }
        // Right clicks open the menu of the header drawn this pass, not of one a tab now hides.
        self.palette_ui.header = Default::default();
        self.tile_ui.header = Default::default();
        let s = self.ui.scale;
        let right = self.layout.right;
        sf.fill(
            Rect::new(right.x, right.y, s.min(right.w), right.h),
            theme::rule(),
        );
        let left = self.layout.left;
        if left.w > 0 {
            sf.fill(
                Rect::new(left.right() - s, left.y, s, left.h),
                theme::rule(),
            );
        }
        for side in [Side::Right, Side::Left, Side::Bottom] {
            self.draw_area(sf, side);
        }
        self.draw_splitters(sf);
    }

    pub(crate) fn draw_area(&mut self, sf: &mut Surface<'_>, side: Side) {
        let solved = self.layout.solved;
        let placed = *solved.area(side);
        let area = *solved.effective.area(side);
        if placed.rect.w == 0 || placed.rect.h == 0 {
            return;
        }
        if placed.rail {
            self.draw_rail(sf, side, placed.rect, area.slots());
            return;
        }
        for (i, (r, slot)) in placed.slots().iter().zip(area.slots()).enumerate() {
            self.draw_dock_slot(sf, (side, i), *r, slot);
        }
    }

    fn draw_dock_slot(
        &mut self,
        sf: &mut Surface<'_>,
        (side, i): (Side, usize),
        r: SlotRect,
        slot: &Slot,
    ) {
        let s = self.ui.scale;
        let strip = side != Side::Bottom || bottom_strip(slot);
        let (strip_r, body) = slot_parts(side, r.rect, strip, s);
        if strip {
            let (words, count) = labels(slot);
            let id = strip_id(side, i);
            let active = usize::from(slot.active).min(count.saturating_sub(1));
            let mut icons = [None; PANEL_COUNT];
            for (o, &p) in icons.iter_mut().zip(slot.tabs.as_slice()) {
                *o = Some(art(p).art());
            }
            let shown = Strip {
                tabs: &words[..count],
                active,
                folded: !r.open,
                icons: &icons[..count],
            };
            let out = tab_strip(&mut self.ui, sf, id, strip_r, shown);
            // The strip's two icon buttons say what they do; they keep the names the strip
            // gave them. A slot folded for room says so on its fold button instead.
            let fold = if r.open {
                "Fold this panel"
            } else {
                "Unfold this panel"
            };
            for (offset, hint) in [
                (pikslar_ui::tabs::MENU, "Panel menu"),
                (pikslar_ui::tabs::FOLD, fold),
            ] {
                let at = self.ui.widgets().iter().find(|(_, w)| *w == id + offset);
                if let Some(&(rect, _)) = at {
                    self.say_hint_over(rect, Cow::Borrowed(hint));
                }
            }
            self.hint_room_fold((side, i), r, strip_r);
            // A tab drawn as its icon alone says its name when the pointer rests on it, or the
            // keyboard focus.
            for n in (0..count).filter(|n| out.compact >> n & 1 == 1) {
                let tab_id = id + 1 + n as u32;
                let rect = self.ui.widgets().iter().find(|(_, w)| *w == tab_id);
                if let Some(&(rect, _)) = rect {
                    self.note_hint(rect, words[n]);
                }
            }
            if let Some(kept) = self.drawn_strips.slots.get_mut(side.index() * 3 + i) {
                *kept = Some(*slot);
            }
            if self.ui.naming() {
                // Tabs are named apart from the panels' own headers, which scripts find by name.
                for (n, w) in words[..count].iter().enumerate() {
                    self.ui.name(id + 1 + n as u32, &format!("{w} tab"));
                }
            }
            if let Some(event) = out.event {
                let panels = *slot;
                self.defer(move |a| a.slot_event((side, i), &panels, event));
            }
        }
        // A folded slot shows its panel's header, if it has one, and else its strip alone.
        match r.panel {
            Some(panel) if r.open || crate::layout::folds_to_header(panel) => {
                self.draw_panel(sf, panel, body, !r.open);
            }
            Some(Panel::Palette) => self.release_folded_palette(),
            _ => {}
        }
    }

    /// A slot the solver folded for want of room says so on its fold button, which would not
    /// show it: only a taller window does.
    fn hint_room_fold(&mut self, (side, i): (Side, usize), r: SlotRect, strip: Rect) {
        let stored = self
            .workspaces
            .active()
            .area(side)
            .slots()
            .get(i)
            .map(|sl| sl.open);
        if r.open || stored != Some(true) {
            return;
        }
        let s = self.ui.scale;
        let button = Rect::new(strip.right() - FOLD_W * s, strip.y, FOLD_W * s, strip.h);
        self.note_hint(button, "Folded for room. A taller window shows it");
    }

    fn draw_rail(&mut self, sf: &mut Surface<'_>, side: Side, r: Rect, slots: &[Slot]) {
        let mut panels = [Panel::Color; PANEL_COUNT];
        let mut count = 0;
        for &p in slots.iter().flat_map(|sl| sl.tabs.as_slice()) {
            if count < PANEL_COUNT {
                panels[count] = p;
                count += 1;
            }
        }
        let color = crate::color_ui::to_screen(self.editor.color);
        let items = panels.map(|p| RailItem {
            name: label(p),
            icon: match p {
                Panel::Color => RailIcon::Swatch(color),
                p => RailIcon::Art(art(p).art()),
            },
        });
        let id = ID_RAIL + side.index() as u32 * RAIL_IDS;
        let chosen = rail(
            &mut self.ui,
            sf,
            id,
            r,
            side != Side::Bottom,
            &items[..count],
        );
        if self.ui.naming() {
            for (n, item) in items[..count].iter().enumerate() {
                self.ui
                    .name(id + 1 + n as u32, &format!("Show {}", item.name));
            }
        }
        if let Some(n) = chosen {
            let panel = panels[n];
            self.defer(move |a| a.unfold_area(side, panel));
        }
    }

    /// What a slot's strip asked. `shown` is the slot as drawn, which may hold a panel the
    /// arrangement does not (Tiles, while a tilemap layer is active).
    pub(crate) fn slot_event(&mut self, (side, i): (Side, usize), shown: &Slot, event: TabEvent) {
        match event {
            TabEvent::Choose(n) => {
                let Some(&panel) = shown.tabs.as_slice().get(n) else {
                    return;
                };
                self.tiles_shown = panel == Panel::Tiles;
                self.edit_layout(|ws| {
                    with_slot(ws, side, i, |sl| Slot {
                        open: true,
                        active: sl
                            .tabs
                            .as_slice()
                            .iter()
                            .position(|&p| p == panel)
                            .map_or(sl.active, |t| t as u8),
                        ..sl
                    })
                });
            }
            TabEvent::Fold => self.fold_slot(side, i),
            TabEvent::Menu { tab, at } => {
                if let Some(&panel) = shown.tabs.as_slice().get(tab) {
                    self.open_slot_menu((side, i), panel, at);
                }
            }
        }
    }

    /// Folds slot `i` of `side`, or opens it. A slot folded for room stays as it is: only a
    /// larger window shows it.
    pub(crate) fn fold_slot(&mut self, side: Side, i: usize) {
        let stored = self.workspaces.active().area(side).slots().get(i).copied();
        let shown = self.layout.solved.area(side).slots().get(i).copied();
        let (Some(stored), Some(shown)) = (stored, shown) else {
            return;
        };
        if (stored.open && !shown.open) || stored.timeline_alone() {
            return;
        }
        self.edit_layout(|ws| {
            with_slot(ws, side, i, |sl| Slot {
                open: !sl.open,
                ..sl
            })
        });
    }

    /// Opens the folded area `side` on `panel`.
    fn unfold_area(&mut self, side: Side, panel: Panel) {
        self.window_ui.canvas_only = false;
        self.edit_layout(|ws| {
            let area = ws.area(side).with_open(true);
            let ws = ws.with_area(side, area);
            match ws.find(panel) {
                Some((at, i, t)) => with_slot(ws, at, i, |sl| Slot {
                    open: true,
                    active: t as u8,
                    ..sl
                }),
                None => ws,
            }
        });
    }

    /// Ctrl+1 to Ctrl+4: the preset. Returns whether `action` was one.
    pub(crate) fn workspace_action(&mut self, action: Action) -> bool {
        let preset = match action {
            Action::WorkspaceDraw => Preset::Draw,
            Action::WorkspaceAnimate => Preset::Animate,
            Action::WorkspaceTiles => Preset::Tiles,
            Action::WorkspacePalette => Preset::Palette,
            _ => return false,
        };
        self.choose_preset(preset);
        true
    }

    /// Shows `preset`'s arrangement; the document is left as it is.
    pub(crate) fn choose_preset(&mut self, preset: Preset) {
        self.workspaces = self.workspaces.choose(preset);
        self.follow_workspace();
    }

    /// After the arrangement in use changed as a whole: the matrix opens as the bottom dock's
    /// stored size asks, and the layout follows.
    pub(crate) fn follow_workspace(&mut self) {
        self.window_ui.canvas_only = false;
        let tab = self.focused_tab_panel();
        self.drawn_strips.refocus = true;
        self.matrix.open = self.asked_rows().is_some();
        self.ui_dirty = true;
        self.full_canvas = true;
        self.sync_matrix_layout();
        self.refocus_tab(tab);
    }

    /// Changes the arrangement in use and lays the window out again.
    pub(crate) fn edit_layout(&mut self, f: impl FnOnce(Workspace) -> Workspace) {
        let tab = self.focused_tab_panel();
        let current = self.workspaces.current;
        let changed = f(self.workspaces.active());
        self.workspaces = self.workspaces.with_layout(current, changed);
        self.ui_dirty = true;
        self.sync_matrix_layout();
        self.refocus_tab(tab);
    }

    /// The arrangement as shown: the one in use, with Tiles as a tab beside Palette (or in
    /// the right dock's first slot) while a tilemap layer is active and Tiles has no place
    /// of its own. Shown, Tiles' slot takes the height its rows ask for.
    pub(crate) fn view_layout(&self) -> Workspace {
        self.shown_arrangement(self.arranged_layout())
    }

    /// [`App::view_layout`] before the canvas-only form is applied.
    fn arranged_layout(&self) -> Workspace {
        let ws = self.workspaces.active();
        if ws.placed().contains(Panel::Tiles) || ws.hidden.contains(Panel::Tiles) {
            return ws;
        }
        let host = ws
            .find(Panel::Palette)
            .filter(|&(side, _, _)| side != Side::Bottom)
            .map(|(side, i, _)| (side, i))
            .or_else(|| (!ws.area(Side::Right).is_empty()).then_some((Side::Right, 0)));
        let Some((side, i)) = host else {
            return ws;
        };
        let s = self.ui.scale;
        let width = (ws.area(side).size - 2 * BODY_X - DOCK_EDGE) * s;
        let Some(body) = self.tiles_wanted(width) else {
            return ws;
        };
        let shown = self.tiles_shown;
        with_slot(ws, side, i, |sl| {
            let tabs = sl.tabs.with(Panel::Tiles);
            match tabs.as_slice().iter().position(|&p| p == Panel::Tiles) {
                Some(t) if shown => Slot {
                    tabs,
                    active: t as u8,
                    size: crate::layout::strip() + 2 * BODY_Y + body,
                    ..sl
                },
                _ => Slot { tabs, ..sl },
            }
        })
    }
}

#[cfg(test)]
#[path = "dock_tests.rs"]
mod tests;
