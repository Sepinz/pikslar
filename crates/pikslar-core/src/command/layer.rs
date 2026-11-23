//! Layer commands.

use super::{LayerCmd, Outcome};
use crate::document::LimitError;

/// The most characters a layer's name keeps.
pub const MAX_LAYER_NAME: usize = 64;
use crate::editor::Editor;

impl Editor {
    pub(super) fn apply_layer(&mut self, command: LayerCmd) -> Result<Outcome, LimitError> {
        match command {
            LayerCmd::Add { at, mode } => {
                self.transact(|d| d.add_layer(at, mode))?;
                self.layer = at.min(self.doc.layers.len() - 1);
            }
            LayerCmd::Remove { layer } => {
                let index = self.index_of(layer)?;
                self.transact(|d| d.remove_block(index))?;
            }
            LayerCmd::Move { layer, to } => self.move_layer(self.index_of(layer)?, to)?,
            LayerCmd::SetOpacity { layer, opacity } => {
                self.refuse_group(self.index_of(layer)?)?;
                return Ok(super::did(self.set_opacity(layer, opacity)?));
            }
            LayerCmd::SetVisible { layer, visible } => {
                return self.set_visible(self.index_of(layer)?, visible);
            }
            LayerCmd::SetLocked { layer, locked } => {
                return self.set_locked(self.index_of(layer)?, locked);
            }
            LayerCmd::SetBlend { layer, blend } => {
                let index = self.index_of(layer)?;
                self.refuse_group(index)?;
                self.transact(|d| {
                    d.layers[index].blend = blend;
                    Ok(())
                })?;
            }
            LayerCmd::SetMode { layer, mode } => {
                let index = self.index_of(layer)?;
                self.transact(|d| d.set_layer_mode(index, mode))?;
            }
            LayerCmd::Rename { layer, name } => return self.rename(self.index_of(layer)?, name),
            LayerCmd::Duplicate { layer } => {
                let index = self.index_of(layer)?;
                self.layer = self.transact(|d| d.duplicate_block(index))?;
            }
            LayerCmd::Group { layer } => {
                let index = self.index_of(layer)?;
                self.layer = self.transact(|d| d.group_layer(index))?;
            }
            LayerCmd::Ungroup { layer } => {
                let index = self.index_of(layer)?;
                self.transact(|d| d.ungroup(index))?;
                self.layer = index.saturating_sub(1).min(self.doc.layers.len() - 1);
            }
            LayerCmd::Step { layer, up } => {
                let index = self.index_of(layer)?;
                self.layer = self.transact(|d| d.step_layer(index, up))?;
            }
            LayerCmd::SetFolded { layer, folded } => {
                return self.set_folded(self.index_of(layer)?, folded);
            }
            LayerCmd::PlaceReference { below, image, name } => {
                self.layer = self.transact(|d| d.place_reference(below, image, name))?;
            }
            LayerCmd::MergeDown { layer } => {
                let index = self.index_of(layer)?;
                self.transact(|d| d.merge_down(index))?;
                self.layer = index - 1;
            }
        }
        Ok(Outcome::CHANGED)
    }

    /// Moves one row. With groups in the list, a row moved alone could take layers into
    /// another group, so it is refused there: `Step` moves through groups.
    fn move_layer(&mut self, from: usize, to: usize) -> Result<(), LimitError> {
        if to >= self.doc.layers.len() {
            return Err(LimitError::NoSuchItem);
        }
        if self
            .doc
            .layers
            .iter()
            .any(|l| l.kind == crate::layer::LayerKind::Group)
        {
            return Err(LimitError::GroupOrder);
        }
        self.transact(|d| {
            d.layers.move_to(from, to);
            d.check_groups()
        })?;
        self.layer = to;
        Ok(())
    }

    fn rename(&mut self, index: usize, name: String) -> Result<Outcome, LimitError> {
        let kept = crate::names::without_hidden(name.trim());
        let name: String = kept.chars().take(MAX_LAYER_NAME).collect();
        if name.is_empty() || self.doc.layers[index].name == name {
            return Ok(Outcome::UNCHANGED);
        }
        self.transact(|d| {
            d.layers[index].name = name;
            Ok(())
        })?;
        Ok(Outcome::CHANGED)
    }

    /// Changes the file, so it counts as a modification, but is never a step of undo.
    fn set_visible(&mut self, index: usize, visible: bool) -> Result<Outcome, LimitError> {
        if self.doc.layers[index].visible == visible {
            return Ok(Outcome::UNCHANGED);
        }
        self.doc.layers[index].visible = visible;
        self.end_join();
        self.touched();
        self.damage_all();
        Ok(Outcome::CHANGED)
    }

    fn refuse_group(&self, index: usize) -> Result<(), LimitError> {
        match self.doc.layers[index].kind {
            crate::layer::LayerKind::Group => Err(LimitError::Group),
            _ => Ok(()),
        }
    }

    /// Like visibility, never undone; only a group folds.
    fn set_folded(&mut self, index: usize, folded: bool) -> Result<Outcome, LimitError> {
        let layer = &mut self.doc.layers[index];
        if layer.kind != crate::layer::LayerKind::Group || layer.folded == folded {
            return Ok(Outcome::UNCHANGED);
        }
        layer.folded = folded;
        self.end_join();
        self.touched();
        Ok(Outcome::CHANGED)
    }

    /// Like visibility: it changes the file and is never undone. What is being drawn on the
    /// layer, or on a layer in the group, is put down first.
    fn set_locked(&mut self, index: usize, locked: bool) -> Result<Outcome, LimitError> {
        if self.doc.layers[index].locked == locked {
            return Ok(Outcome::UNCHANGED);
        }
        let (low, high) = self.doc.block(index);
        let shares = |(layer, _): &(usize, usize)| (low..=high).contains(layer);
        let members = self.doc.link_members(self.layer, self.frame);
        if locked && (members.iter().any(shares) || (low..=high).contains(&self.layer)) {
            self.commit();
            self.anchor();
        }
        self.doc.layers[index].locked = locked;
        self.end_join();
        self.touched();
        Ok(Outcome::CHANGED)
    }
}
