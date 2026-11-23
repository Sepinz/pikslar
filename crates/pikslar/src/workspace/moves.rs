//! Moving and hiding panels (docs/ui-redesign.md 3.4, the slot menu's Move to): pure changes
//! of an arrangement, each refused where it would break a rule. The Timeline stays in the
//! bottom dock, and Layers and Tiles, which need height, never go there.

use crate::workspace::model::{Area, Panel, Side, Slot, Workspace, MOST_SLOTS, PANEL_COUNT};

/// Where a panel goes: a new slot at the end of an area, or a tab of an existing slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    NewSlot(Side),
    Beside(Side, usize),
}

impl Target {
    fn side(self) -> Side {
        match self {
            Target::NewSlot(side) | Target::Beside(side, _) => side,
        }
    }
}

/// Whether `p` may be placed in `side`.
pub(crate) fn allowed(p: Panel, side: Side) -> bool {
    match p {
        Panel::Timeline => side == Side::Bottom,
        Panel::Layers | Panel::Tiles => side != Side::Bottom,
        _ => true,
    }
}

/// `ws` with `p` moved to `to`, or none when the move is not allowed, has no room or would
/// change nothing. A panel not placed (Tiles shown beside the Palette only for the view) is
/// simply placed.
pub(crate) fn move_panel(ws: Workspace, p: Panel, to: Target) -> Option<Workspace> {
    if !allowed(p, to.side()) {
        return None;
    }
    let from = ws.find(p);
    if let (Some((side, i, _)), Target::Beside(at, j)) = (from, to) {
        if (side, i) == (at, j) {
            return None;
        }
    }
    // The slot `p` leaves, and whether it goes with it, which moves a later target up one.
    let leaves = from.map(|(side, i, _)| {
        let alone = ws
            .area(side)
            .slots()
            .get(i)
            .is_some_and(|s| s.tabs.len() == 1);
        (side, i, alone)
    });
    let to = match (to, leaves) {
        (Target::NewSlot(at), Some((side, _, true))) if side == at => return None,
        (Target::Beside(at, j), Some((side, i, true))) if side == at && i < j => {
            Target::Beside(at, j - 1)
        }
        _ => to,
    };
    put(take(ws, p), p, to)
}

/// `ws` with `p` hidden by hand, or none when it cannot be hidden.
pub(crate) fn hide_panel(ws: Workspace, p: Panel) -> Option<Workspace> {
    if !p.can_hide() {
        return None;
    }
    let hidden = ws.hidden.with(p);
    Some(take(ws, p).with_hidden(hidden))
}

/// `ws` without `p`; a slot left without tabs goes too.
fn take(ws: Workspace, p: Panel) -> Workspace {
    let Some((side, i, _)) = ws.find(p) else {
        return ws;
    };
    let area = *ws.area(side);
    let slots: Vec<Slot> = area
        .slots()
        .iter()
        .enumerate()
        .map(|(n, &sl)| if n == i { without(sl, p) } else { sl })
        .filter(|sl| !sl.tabs.is_empty())
        .map(Slot::reachable)
        .collect();
    ws.with_area(side, Area::new(area.size, area.open, &slots))
}

/// A slot without `p`; its active tab stays on the panel it showed, or the one before.
fn without(sl: Slot, p: Panel) -> Slot {
    let shown = sl.active_panel();
    let tabs = sl.tabs.without(p);
    let active = shown
        .and_then(|q| tabs.as_slice().iter().position(|&t| t == q))
        .unwrap_or(usize::from(sl.active).saturating_sub(1))
        .min(tabs.len().saturating_sub(1));
    Slot {
        tabs,
        active: active as u8,
        ..sl
    }
}

/// `ws` with `p` placed at `to`, shown, and its area opened.
fn put(ws: Workspace, p: Panel, to: Target) -> Option<Workspace> {
    let area = *ws.area(to.side());
    let mut slots: Vec<Slot> = area.slots().to_vec();
    match to {
        Target::NewSlot(_) if slots.len() < MOST_SLOTS => slots.push(Slot::new(0, &[p])),
        Target::Beside(_, j) => {
            let sl = slots.get(j).copied()?;
            if sl.tabs.len() >= PANEL_COUNT {
                return None;
            }
            let tabs = sl.tabs.with(p);
            let active = tabs.as_slice().iter().position(|&t| t == p)?;
            slots[j] = Slot {
                tabs,
                active: active as u8,
                open: true,
                ..sl
            };
        }
        Target::NewSlot(_) => return None,
    }
    let hidden = ws.hidden.without(p);
    let area = Area::new(area.size, true, &slots);
    Some(ws.with_area(to.side(), area).with_hidden(hidden))
}

#[cfg(test)]
#[path = "moves_tests.rs"]
mod tests;
