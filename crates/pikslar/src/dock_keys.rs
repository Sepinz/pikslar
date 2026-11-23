//! The docks' splitters and keys (docs/ui-redesign.md 3.3, 3.7): the gutters that size a side
//! dock, the slots in it and the timeline; F6 and Shift+F6 between the regions; Ctrl+Tab and
//! Ctrl+Shift+Tab between the tabs of the slot that has focus; and the keys of a focused tab,
//! splitter or rail icon handed to it through Focus.

use crate::app::App;
use crate::dock::{strip_id, with_slot, ID_DOCK};
use crate::layout::{strip, DOCK_EDGE, MATRIX_ROWS};
use crate::workspace::model::{Side, Slot};
use pikslar_platform::{Key, Mods};
use pikslar_render::{Rect, Surface};
use pikslar_ui::splitter::{splitter, Axis, Span, SplitChange, GUTTER};
use pikslar_ui::tabs::{cycle, TabEvent, TAB_IDS};
use pikslar_ui::{Press, WidgetKey};

/// The splitters: a dock's edge at `ID_SPLIT + side`, the gutter under slot `i` of a side dock
/// at `ID_SPLIT + 3 + side * 2 + i`.
pub(crate) const ID_SPLIT: u32 = ID_DOCK + 400;
/// The timeline's single line, and what each matrix row adds, in the stored size's units.
pub(crate) const LINE: i32 = crate::workspace::presets::TIMELINE_LINE;
const ROW: i32 = crate::workspace::presets::TIMELINE_ROW;

/// The id of the gutter under slot `i` of `side`.
pub(crate) fn slot_split_id(side: Side, i: usize) -> u32 {
    ID_SPLIT + 3 + side.index() as u32 * 2 + i as u32
}

/// The widget key a key is for a focused control that reads keys itself, if any.
fn widget_key(key: Key, mods: Mods) -> Option<WidgetKey> {
    Some(match key {
        Key::Left => WidgetKey::Left,
        Key::Right => WidgetKey::Right,
        Key::Up => WidgetKey::Up,
        Key::Down => WidgetKey::Down,
        Key::Home => WidgetKey::Home,
        Key::End => WidgetKey::End,
        Key::Enter => WidgetKey::Enter,
        Key::Menu => WidgetKey::Menu,
        Key::F(10) if mods.shift => WidgetKey::Menu,
        _ => return None,
    })
}

impl App {
    /// Draws the splitters, after the panes beside them.
    pub(crate) fn draw_splitters(&mut self, sf: &mut Surface<'_>) {
        for side in [Side::Right, Side::Left] {
            self.edge_splitter(sf, side);
            self.slot_splitters(sf, side);
        }
        self.timeline_splitter(sf);
    }

    fn edge_splitter(&mut self, sf: &mut Surface<'_>, side: Side) {
        let s = self.ui.scale;
        let placed = *self.layout.solved.area(side);
        let r = placed.rect;
        if placed.rail || r.w == 0 || placed.slots().is_empty() {
            return;
        }
        let x = match side {
            Side::Left => r.right() - GUTTER * s,
            _ => r.x,
        };
        let (least, most) = side.range();
        let span = Span {
            size: r.w / s,
            least,
            most,
            step: 4,
            after: side == Side::Right,
        };
        let id = ID_SPLIT + side.index() as u32;
        let gutter = Rect::new(x, r.y, GUTTER * s, r.h);
        let change = splitter(&mut self.ui, sf, id, gutter, Axis::Across, span);
        if self.ui.naming() {
            self.ui.name(id, &format!("Size the {} dock", side.word()));
        }
        let Some(change) = change else {
            return;
        };
        self.defer(move |a| {
            a.edit_layout(|ws| {
                let area = *ws.area(side);
                let area = match change {
                    SplitChange::Resize(n) => area.with_size(n),
                    SplitChange::Fold => area.with_open(false),
                };
                ws.with_area(side, area)
            })
        });
    }

    /// The gutters between open slots of a side dock, in the bottom of the upper slot's room.
    fn slot_splitters(&mut self, sf: &mut Surface<'_>, side: Side) {
        let s = self.ui.scale;
        let placed = *self.layout.solved.area(side);
        let slots = placed.slots();
        for i in 0..slots.len().saturating_sub(1) {
            let (upper, lower) = (slots[i], slots[i + 1]);
            if placed.rail || !upper.open || !lower.open {
                continue;
            }
            let r = upper.rect;
            let edge = if side == Side::Right {
                DOCK_EDGE * s
            } else {
                0
            };
            let gutter = Rect::new(
                r.x + edge,
                r.bottom() - GUTTER * s,
                r.w - DOCK_EDGE * s,
                GUTTER * s,
            );
            let size = r.h / s;
            let span = Span {
                size,
                least: strip(),
                most: size + lower.rect.h / s - strip(),
                step: 4,
                after: false,
            };
            let id = slot_split_id(side, i);
            let change = splitter(&mut self.ui, sf, id, gutter, Axis::Down, span);
            if self.ui.naming() {
                let name = upper.panel.map_or("", crate::dock::label);
                self.ui.name(id, &format!("Size {name}"));
            }
            match change {
                Some(SplitChange::Resize(n)) => self.defer(move |a| {
                    a.edit_layout(|ws| with_slot(ws, side, i, |sl| Slot { size: n, ..sl }))
                }),
                Some(SplitChange::Fold) => self.defer(move |a| a.fold_slot(side, i)),
                None => {}
            }
        }
    }

    /// The gutter along the top of the bottom dock: up opens matrix rows, a row a step.
    fn timeline_splitter(&mut self, sf: &mut Surface<'_>) {
        let s = self.ui.scale;
        let placed = *self.layout.solved.area(Side::Bottom);
        let r = placed.rect;
        if placed.rail || r.h == 0 {
            return;
        }
        let rows = self.layout.matrix_rows(s);
        let span = Span {
            size: LINE + ROW * rows,
            least: LINE,
            most: LINE + ROW * MATRIX_ROWS,
            step: ROW,
            after: true,
        };
        let id = ID_SPLIT + Side::Bottom.index() as u32;
        let gutter = Rect::new(r.x, r.y, r.w, GUTTER * s);
        let change = splitter(&mut self.ui, sf, id, gutter, Axis::Down, span);
        self.ui.name(id, "Size the timeline");
        match change {
            Some(SplitChange::Resize(n)) => self.defer(move |a| a.size_timeline(n)),
            Some(SplitChange::Fold) => self.defer(|a| a.toggle_matrix()),
            None => {}
        }
    }

    /// Sets the bottom dock's stored size: the single line, or the line and matrix rows.
    pub(crate) fn size_timeline(&mut self, size: i32) {
        let size = size.clamp(LINE, LINE + ROW * MATRIX_ROWS);
        self.matrix.open = size > LINE;
        self.edit_layout(|ws| ws.with_area(Side::Bottom, ws.area(Side::Bottom).with_size(size)));
    }

    /// The matrix rows the bottom dock's stored size asks for, or none when it leaves the
    /// rows to follow the layers.
    pub(crate) fn asked_rows(&self) -> Option<i32> {
        let size = self.workspaces.active().area(Side::Bottom).size;
        (size > LINE).then(|| ((size - LINE) / ROW).min(MATRIX_ROWS))
    }

    /// F6, Ctrl+Tab and the keys of a focused control that reads keys itself. Returns whether
    /// the key was used.
    pub(crate) fn dock_key(&mut self, key: Key, mods: Mods) -> bool {
        let used = match key {
            Key::F(6) if !mods.ctrl && !mods.alt => {
                self.step_region(mods.shift);
                true
            }
            Key::Tab if mods.ctrl => self.cycle_tabs(mods.shift),
            _ => self.press_focused(key, mods),
        };
        self.ui_dirty |= used;
        used
    }

    fn press_focused(&mut self, key: Key, mods: Mods) -> bool {
        let Some(at) = self.ui.focus.focused() else {
            return false;
        };
        let Some(wk) = widget_key(key, mods).filter(|_| self.ui.focus.takes_keys(at)) else {
            return false;
        };
        self.ui.focus.request_press(Press {
            key: wk,
            shift: mods.shift,
        });
        true
    }

    /// The regions F6 walks, in order: the top bar, the context bar, the tool strip, the docks.
    pub(crate) fn regions(&self) -> [Rect; 6] {
        let l = &self.layout;
        [l.top, l.context, l.side, l.left, l.right, l.bottom]
    }

    /// Moves focus to the first control of the next region that has one, or the previous.
    pub(crate) fn step_region(&mut self, back: bool) {
        let regions = self.regions();
        let rect_of = |id: u32| {
            self.ui
                .widgets()
                .iter()
                .rev()
                .find(|&&(_, w)| w == id)
                .map(|&(r, _)| r)
        };
        let region_of = |id: u32| {
            let r = rect_of(id)?;
            regions.iter().position(|g| g.contains(r.x, r.y))
        };
        let order: Vec<(u32, usize)> = self
            .ui
            .focus
            .order()
            .iter()
            .filter_map(|&id| Some((id, region_of(id)?)))
            .collect();
        let here = self.ui.focus.focused().and_then(region_of);
        let count = regions.len();
        let mut at = here.unwrap_or(if back { 0 } else { count - 1 });
        for _ in 0..count {
            at = cycle(at, count, back);
            if let Some(&(id, _)) = order.iter().find(|&&(_, g)| g == at) {
                self.focus_on(id);
                return;
            }
        }
    }

    /// Ctrl+Tab: the next tab of the slot that has focus, or the previous. Focus on a tab
    /// follows it. Returns whether a slot had focus.
    fn cycle_tabs(&mut self, back: bool) -> bool {
        let Some(at) = self.ui.focus.focused() else {
            return false;
        };
        let focus_rect = self
            .ui
            .widgets()
            .iter()
            .rev()
            .find(|&&(_, w)| w == at)
            .map(|&(r, _)| r);
        let Some(fr) = focus_rect else {
            return false;
        };
        let solved = self.layout.solved;
        for side in Side::ALL {
            let placed = solved.area(side);
            let shown = solved.effective.area(side).slots();
            for (i, (r, slot)) in placed.slots().iter().zip(shown).enumerate() {
                if r.rect.contains(fr.x, fr.y) && slot.tabs.len() > 1 {
                    let next = cycle(usize::from(slot.active), slot.tabs.len(), back);
                    let on_tab = (strip_id(side, i) + 1..strip_id(side, i) + TAB_IDS).contains(&at);
                    if on_tab {
                        self.focus_on(strip_id(side, i) + 1 + next as u32);
                    }
                    let panels = *slot;
                    self.slot_event((side, i), &panels, TabEvent::Choose(next));
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
#[path = "dock_keys_tests.rs"]
mod tests;
