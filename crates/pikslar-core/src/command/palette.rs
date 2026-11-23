//! Palette commands: adding, removing and recolouring swatches, and putting swatches in order
//! and in groups. Order and groups never change an id or a pixel, so they repaint nothing.

use super::{Outcome, PaletteCmd};
use crate::color::{
    clean_group_name, hue_order, light_order, GroupKind, PaletteGroup, SortBy, MAX_GROUPS,
};
use crate::document::{Document, LimitError};
use crate::editor::Editor;
use crate::palette_ops::ramp::{ramp_colors, MAX_RAMP_STEPS};
use crate::palette_ops::rewrite::palette_images;
use crate::steps::Dragged;
use crate::tile::{RawTile, TILE_AREA};
use std::collections::HashSet;
use std::sync::Arc;

impl Editor {
    pub(super) fn apply_palette(&mut self, command: PaletteCmd) -> Result<Outcome, LimitError> {
        match command {
            PaletteCmd::AddSwatch { color } => {
                let id = self.transact(|d| Ok(Arc::make_mut(&mut d.colors).add_swatch(color)?))?;
                Ok(Outcome {
                    changed: true,
                    created: Some(id),
                })
            }
            PaletteCmd::RemoveSwatch { id } => {
                self.transact(|d| {
                    Arc::make_mut(&mut d.colors).remove_swatch(id);
                    Ok(())
                })?;
                Ok(Outcome::CHANGED)
            }
            PaletteCmd::Recolor { id, color } => self.recolor(id, color),
            PaletteCmd::MoveSwatch { from, to } => self.move_swatch(from, to),
            PaletteCmd::Sort { by } => self.sort_swatches(by),
            PaletteCmd::AddGroup { name, ids } => self.add_group(&name, &ids),
            PaletteCmd::RenameGroup { group, name } => self.rename_group(group, &name),
            PaletteCmd::RemoveGroup { group } => self.remove_group(group),
            PaletteCmd::AddToGroup { group, id } => self.add_to_group(group, id),
            PaletteCmd::RemoveFromGroup { group, id } => self.remove_from_group(group, id),
            PaletteCmd::AddRamp { from, to, steps } => self.add_ramp(from, to, steps),
            PaletteCmd::Remap(kind) => self.remap_palette(kind),
            PaletteCmd::ReplaceColor { from, to, layer } => self.replace_color(from, to, layer),
            PaletteCmd::Load { colors, how } => self.load_palette(&colors, how),
        }
    }

    fn recolor(&mut self, id: u32, color: u32) -> Result<Outcome, LimitError> {
        let table = &self.doc.colors;
        if id == 0 || id as usize >= table.len() {
            return Err(LimitError::NoSuchItem);
        }
        if table.color(id) == color {
            return Ok(Outcome::UNCHANGED);
        }
        self.join_step(Dragged::Recolor(id), (self.layer, self.frame));
        Arc::make_mut(&mut self.doc.colors).set_color(id, color);
        self.damage_all();
        self.amended();
        Ok(Outcome::CHANGED)
    }

    fn move_swatch(&mut self, from: usize, to: usize) -> Result<Outcome, LimitError> {
        let len = self.doc.colors.swatches().len();
        if from >= len || to >= len {
            return Err(LimitError::NoSuchItem);
        }
        if from == to {
            return Ok(Outcome::UNCHANGED);
        }
        self.transact(|d| {
            Arc::make_mut(&mut d.colors).move_swatch(from, to);
            Ok(())
        })?;
        Ok(Outcome::CHANGED)
    }

    fn sort_swatches(&mut self, by: SortBy) -> Result<Outcome, LimitError> {
        let table = &self.doc.colors;
        let color = |id: u32| table.color(id);
        let order = match by {
            SortBy::Hue => table.sorted_swatches(|id| hue_order(color(id))),
            SortBy::Lightness => table.sorted_swatches(|id| light_order(color(id))),
            SortBy::Usage => {
                let counts = usage_counts(&self.doc);
                let used = |id: u32| counts.get(id as usize).copied().unwrap_or(0);
                // Most used first: the count turned around.
                table.sorted_swatches(|id| u64::MAX - used(id))
            }
        };
        // Already in order: nothing changes, so there is no step.
        if order == table.swatches() {
            return Ok(Outcome::UNCHANGED);
        }
        self.transact(|d| {
            Arc::make_mut(&mut d.colors).set_swatch_order(order);
            Ok(())
        })?;
        Ok(Outcome::CHANGED)
    }

    fn add_group(&mut self, name: &str, ids: &[u32]) -> Result<Outcome, LimitError> {
        let table = &self.doc.colors;
        if table.groups().len() >= MAX_GROUPS {
            return Err(LimitError::TooManyGroups);
        }
        let mut seen = HashSet::new();
        let ids: Vec<u32> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
        if ids.is_empty() || ids.iter().any(|id| !table.swatches().contains(id)) {
            return Err(LimitError::NoSuchItem);
        }
        let group = PaletteGroup {
            name: clean_group_name(name),
            kind: GroupKind::Group,
            ids,
        };
        let index = table.groups().len() as u32;
        self.transact(|d| {
            Arc::make_mut(&mut d.colors).push_group(group);
            Ok(())
        })?;
        Ok(Outcome {
            changed: true,
            created: Some(index),
        })
    }

    /// Typing joins one step, so a name typed in full is undone in one go.
    fn rename_group(&mut self, group: usize, name: &str) -> Result<Outcome, LimitError> {
        let found = self.doc.colors.groups().get(group);
        let found = found.ok_or(LimitError::NoSuchItem)?;
        let name = clean_group_name(name);
        if found.name == name {
            return Ok(Outcome::UNCHANGED);
        }
        self.join_step(Dragged::GroupName(group), (self.layer, self.frame));
        if let Some(found) = Arc::make_mut(&mut self.doc.colors)
            .groups_mut()
            .get_mut(group)
        {
            found.name = name;
        }
        self.amended();
        Ok(Outcome::CHANGED)
    }

    fn remove_group(&mut self, group: usize) -> Result<Outcome, LimitError> {
        if group >= self.doc.colors.groups().len() {
            return Err(LimitError::NoSuchItem);
        }
        self.transact(|d| {
            Arc::make_mut(&mut d.colors).groups_mut().remove(group);
            Ok(())
        })?;
        Ok(Outcome::CHANGED)
    }

    fn add_to_group(&mut self, group: usize, id: u32) -> Result<Outcome, LimitError> {
        let table = &self.doc.colors;
        let found = table.groups().get(group).ok_or(LimitError::NoSuchItem)?;
        if !table.swatches().contains(&id) {
            return Err(LimitError::NoSuchItem);
        }
        if found.ids.contains(&id) {
            return Ok(Outcome::UNCHANGED);
        }
        self.transact(|d| {
            if let Some(found) = Arc::make_mut(&mut d.colors).groups_mut().get_mut(group) {
                found.ids.push(id);
            }
            Ok(())
        })?;
        Ok(Outcome::CHANGED)
    }

    fn remove_from_group(&mut self, group: usize, id: u32) -> Result<Outcome, LimitError> {
        let found = self.doc.colors.groups().get(group);
        let found = found.ok_or(LimitError::NoSuchItem)?;
        if !found.ids.contains(&id) {
            return Ok(Outcome::UNCHANGED);
        }
        self.transact(|d| {
            if let Some(found) = Arc::make_mut(&mut d.colors).groups_mut().get_mut(group) {
                found.ids.retain(|&member| member != id);
            }
            Ok(())
        })?;
        Ok(Outcome::CHANGED)
    }

    /// The colours between two swatches join the palette by the usual rules (a colour the
    /// table has keeps its id), and the whole is one step, or nothing if the table is full. A
    /// colour between that comes out fully clear (only between two clear ends) is left out of
    /// the ramp, so the group holds only real swatches; its two ends are always in it.
    fn add_ramp(&mut self, from: u32, to: u32, steps: u32) -> Result<Outcome, LimitError> {
        let table = &self.doc.colors;
        let (start, end) = (table.color(from), table.color(to));
        let is_swatch = |id: u32| table.swatches().contains(&id);
        if from == to || !(1..=MAX_RAMP_STEPS).contains(&steps) {
            return Err(LimitError::RampSteps);
        }
        if !is_swatch(from) || !is_swatch(to) {
            return Err(LimitError::NoSuchItem);
        }
        if table.groups().len() >= MAX_GROUPS {
            return Err(LimitError::TooManyGroups);
        }
        let colors = ramp_colors(start, end, steps);
        let index = table.groups().len() as u32;
        self.transact(|d| {
            let table = Arc::make_mut(&mut d.colors);
            let mut ids = vec![from];
            for color in colors {
                ids.push(table.add_swatch(color)?);
            }
            ids.push(to);
            // A clear step is id 0, which is never a swatch: it is left out.
            let mut seen = HashSet::new();
            ids.retain(|&id| id != 0 && seen.insert(id));
            table.push_group(PaletteGroup {
                name: "Ramp".into(),
                kind: GroupKind::Ramp,
                ids,
            });
            Ok(())
        })?;
        Ok(Outcome {
            changed: true,
            created: Some(index),
        })
    }
}

/// How many pixels show each colour id, over every image that holds palette ids: palette
/// drawings and palette tileset images, as Remove unused looks at. An image that frames, holds,
/// links or tiles share is counted once.
fn usage_counts(doc: &Document) -> Vec<u64> {
    let mut counts = vec![0u64; doc.colors.len()];
    let mut raw: RawTile = [0; TILE_AREA];
    for image in palette_images(doc) {
        for tile in image.drawn_tiles() {
            tile.decode(&mut raw);
            for &id in raw.iter().filter(|&&id| id != 0) {
                if let Some(count) = counts.get_mut(id as usize) {
                    *count += 1;
                }
            }
        }
    }
    counts
}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ramp_tests.rs"]
mod ramp_tests;
