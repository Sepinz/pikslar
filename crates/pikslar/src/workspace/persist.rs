//! The workspace lines of `settings.txt` (section 3.9 of docs/ui-redesign.md) and the repair
//! rules that turn whatever was read into a valid arrangement.
//!
//! ```text
//! workspace = animate
//! dock.draw.right = 248 open | 150 open 0 color | 112 open 0 palette | 0 open 0 layers lint
//! dock.draw.bottom = 28 open | 0 open 0 timeline
//! dock.draw.left = 240 closed
//! dock.draw.color = full
//! dock.draw.hidden = slices
//! timeline.names = 176
//! workspace.custom.name = Shading
//! dock.custom.right = ...
//! ```
//!
//! An area line is `size state | slot | slot ...`; a slot is `size state active-tab panels...`,
//! size 0 meaning "takes the rest". `color` and `hidden` are this module's additions to the
//! documented lines: Color's compact form and the panels hidden by hand are part of a preset's
//! arrangement too.
//!
//! Repair rules: a panel listed twice keeps its first place; a panel where it may not be (the
//! Timeline outside the bottom dock, Layers or Tiles in it) is dropped; a panel missing from
//! every area returns to its default area unless it was hidden by hand; unknown panel names are
//! dropped; sizes are clamped; more than three slots merge the extras into the last; a line that
//! cannot be read falls back to the preset's default for that line; a slot holding the Timeline
//! alone, which has no strip to unfold it by, is open. Only presets that differ from their
//! default are written, and nothing at all for someone who never changed a workspace.

use super::model::{
    clean_name, Area, Custom, Panel, PanelSet, Preset, Side, Slot, Tabs, Workspace, Workspaces,
    MOST_SLOTS, MOST_SLOT_SIZE, NAMES_DEFAULT,
};
use super::moves::allowed;
use super::presets::default_layout;

/// Whether a settings line belongs to the workspace.
pub(crate) fn is_workspace_line(name: &str) -> bool {
    name == "workspace"
        || name == "timeline.names"
        || name.starts_with("workspace.")
        || name.starts_with("dock.")
}

/// Reads the workspace lines, `(name, value)` trimmed and in file order; a later line wins.
pub(crate) fn read(lines: &[(&str, &str)]) -> Workspaces {
    let mut current = None;
    let mut names = NAMES_DEFAULT;
    let mut custom_name: Option<&str> = None;
    let mut custom_seen = false;
    let mut layouts = [
        Preset::Draw,
        Preset::Animate,
        Preset::Tiles,
        Preset::Palette,
        Preset::Custom,
    ]
    .map(default_layout);
    for &(name, value) in lines {
        match name {
            "workspace" => current = Preset::from_word(value).or(current),
            "timeline.names" => names = value.parse().unwrap_or(names),
            "workspace.custom.name" => custom_name = Some(value),
            _ => {
                let Some((preset, field)) = dock_name(name) else {
                    continue;
                };
                custom_seen |= preset == Preset::Custom;
                let i = preset_index(preset);
                layouts[i] = read_field(layouts[i], preset, field, value);
            }
        }
    }
    let mut w = Workspaces::default().with_names(names);
    for p in Preset::BUILT_IN {
        w = w.with_layout(p, repair(layouts[preset_index(p)], p));
    }
    if custom_seen || custom_name.is_some() {
        w.custom = Some(Custom {
            name: clean_name(custom_name.unwrap_or("")),
            layout: repair(layouts[preset_index(Preset::Custom)], Preset::Custom),
        });
    }
    w.choose(current.unwrap_or_default())
}

/// The workspace lines to write: only what differs from the defaults.
pub(crate) fn lines(w: &Workspaces) -> Vec<String> {
    let mut out = Vec::new();
    if w.current != Preset::Draw {
        out.push(format!("workspace = {}", w.current.word()));
    }
    for p in Preset::BUILT_IN {
        match w.layout(p) {
            Some(layout) if layout != default_layout(p) => push_layout(&mut out, p, &layout),
            _ => {}
        }
    }
    if w.timeline_names != NAMES_DEFAULT {
        out.push(format!("timeline.names = {}", w.timeline_names));
    }
    if let Some(c) = &w.custom {
        out.push(format!("workspace.custom.name = {}", c.name));
        push_layout(&mut out, Preset::Custom, &c.layout);
    }
    out
}

fn push_layout(out: &mut Vec<String>, p: Preset, layout: &Workspace) {
    let name = p.word();
    for side in [Side::Right, Side::Bottom, Side::Left] {
        out.push(format!(
            "dock.{name}.{} = {}",
            side.word(),
            area_text(layout.area(side))
        ));
    }
    let color = if layout.color_compact {
        "compact"
    } else {
        "full"
    };
    out.push(format!("dock.{name}.color = {color}"));
    let hidden: Vec<&str> = layout.hidden.iter().map(Panel::word).collect();
    out.push(
        format!("dock.{name}.hidden = {}", hidden.join(" "))
            .trim_end()
            .to_string(),
    );
}

fn area_text(a: &Area) -> String {
    let mut s = format!("{} {}", a.size, state_word(a.open));
    for sl in a.slots() {
        s.push_str(&format!(
            " | {} {} {}",
            sl.size,
            state_word(sl.open),
            sl.active
        ));
        for p in sl.tabs.as_slice() {
            s.push(' ');
            s.push_str(p.word());
        }
    }
    s
}

fn state_word(open: bool) -> &'static str {
    if open {
        "open"
    } else {
        "closed"
    }
}

fn preset_index(p: Preset) -> usize {
    p as usize
}

/// `dock.<preset>.<field>` split; none for an unknown preset.
fn dock_name(name: &str) -> Option<(Preset, &str)> {
    let rest = name.strip_prefix("dock.")?;
    let (preset, field) = rest.split_once('.')?;
    Some((Preset::from_word(preset)?, field))
}

/// One `dock.<preset>.<field>` line applied; a value that cannot be read gives the preset's
/// default for that field, and an unknown field changes nothing.
fn read_field(layout: Workspace, preset: Preset, field: &str, value: &str) -> Workspace {
    let default = default_layout(preset);
    if let Some(side) = Side::ALL.into_iter().find(|s| s.word() == field) {
        let area = parse_area(side, value).unwrap_or(*default.area(side));
        return layout.with_area(side, area);
    }
    match field {
        "color" => layout.with_compact(match value {
            "compact" => true,
            "full" => false,
            _ => default.color_compact,
        }),
        "hidden" => layout.with_hidden(
            value
                .split_whitespace()
                .filter_map(Panel::from_word)
                .fold(PanelSet::EMPTY, PanelSet::with),
        ),
        _ => layout,
    }
}

fn parse_state(word: &str) -> Option<bool> {
    match word {
        "open" => Some(true),
        "closed" => Some(false),
        _ => None,
    }
}

/// An area line; none when it cannot be read.
pub(crate) fn parse_area(side: Side, value: &str) -> Option<Area> {
    let mut parts = value.split('|');
    let mut head = parts.next()?.split_whitespace();
    let size: i32 = head.next()?.parse().ok()?;
    let open = parse_state(head.next()?)?;
    if head.next().is_some() {
        return None;
    }
    let mut slots: Vec<Slot> = Vec::with_capacity(MOST_SLOTS);
    for part in parts {
        let slot = parse_slot(part)?;
        if slot.tabs.is_empty() {
            continue;
        }
        if slots.len() < MOST_SLOTS {
            slots.push(slot);
        } else if let Some(last) = slots.last_mut() {
            last.tabs = slot
                .tabs
                .as_slice()
                .iter()
                .fold(last.tabs, |t, &p| t.with(p));
        }
    }
    let (lo, hi) = side.range();
    Some(Area::new(size.clamp(lo, hi), open, &slots))
}

/// A slot; none when its numbers or state cannot be read. Unknown panel names are dropped.
fn parse_slot(text: &str) -> Option<Slot> {
    let mut words = text.split_whitespace();
    let size: i32 = words.next()?.parse().ok()?;
    let open = parse_state(words.next()?)?;
    let active: u8 = words.next()?.parse().ok()?;
    let tabs = words
        .filter_map(Panel::from_word)
        .fold(Tabs::default(), Tabs::with);
    let last = u8::try_from(tabs.len().saturating_sub(1)).unwrap_or(u8::MAX);
    Some(Slot {
        size: size.clamp(0, MOST_SLOT_SIZE),
        open,
        active: active.min(last),
        tabs,
    })
}

/// The repair rules applied to a whole arrangement of `preset`; a valid arrangement comes back
/// as it was.
pub(crate) fn repair(layout: Workspace, preset: Preset) -> Workspace {
    let deduped = first_places(layout);
    let placed = deduped.placed();
    let hidden = layout
        .hidden
        .iter()
        .filter(|&p| p.can_hide() && !placed.contains(p))
        .fold(PanelSet::EMPTY, PanelSet::with);
    let deduped = deduped.with_hidden(hidden);
    // In the default's own reading order, so that panels put back keep their tab order.
    let default = default_layout(preset);
    let repaired = Side::ALL
        .into_iter()
        .flat_map(|side| default.area(side).slots().iter())
        .flat_map(|sl| sl.tabs.as_slice().iter().copied())
        .filter(|&p| !placed.contains(p) && !hidden.contains(p))
        .fold(deduped, |w, p| put_back(w, preset, p));
    reachable(repaired)
}

/// Every slot that holds the Timeline alone opened, since it has no strip to unfold it by.
fn reachable(w: Workspace) -> Workspace {
    Side::ALL.into_iter().fold(w, |w, side| {
        let area = *w.area(side);
        let slots: Vec<Slot> = area.slots().iter().map(|&sl| sl.reachable()).collect();
        w.with_area(side, Area::new(area.size, area.open, &slots))
    })
}

/// Each panel kept only where it is first listed and allowed (the Timeline below, Layers and
/// Tiles not), in reading order; slots left without a
/// panel are removed, and an active tab follows its panel.
fn first_places(layout: Workspace) -> Workspace {
    let mut seen = PanelSet::EMPTY;
    let mut out = layout;
    for side in Side::ALL {
        let area = layout.area(side);
        let mut slots: Vec<Slot> = Vec::with_capacity(MOST_SLOTS);
        for sl in area.slots() {
            let active = sl.active_panel();
            let kept: Vec<Panel> = sl
                .tabs
                .as_slice()
                .iter()
                .copied()
                .filter(|&p| !seen.contains(p) && allowed(p, side))
                .collect();
            seen = kept.iter().fold(seen, |s, &p| s.with(p));
            if kept.is_empty() {
                continue;
            }
            let at = active
                .and_then(|a| kept.iter().position(|&p| p == a))
                .unwrap_or(0);
            slots.push(Slot {
                active: u8::try_from(at).unwrap_or(0),
                tabs: Tabs::of(&kept),
                ..*sl
            });
        }
        out = out.with_area(side, Area::new(area.size, area.open, &slots));
    }
    out
}

/// `p` back in its default area: as a tab of the slot at its default position, or as a new
/// slot when the area has fewer slots than that. A default position is below three, so an area
/// with fewer slots always has room for one more.
fn put_back(w: Workspace, preset: Preset, p: Panel) -> Workspace {
    let default = default_layout(preset);
    let Some((side, i, _)) = default.find(p) else {
        return w;
    };
    let area = *w.area(side);
    let n = area.slots().len();
    let area = if i < n {
        let sl = area.slots()[i];
        area.with_slot(
            i,
            Slot {
                tabs: sl.tabs.with(p),
                ..sl
            },
        )
    } else {
        let mut slots = area.slots().to_vec();
        slots.push(Slot::new(default.area(side).slots()[i].size, &[p]));
        Area::new(area.size, area.open, &slots)
    };
    w.with_area(side, area)
}

#[cfg(test)]
#[path = "persist_tests.rs"]
mod tests;
