//! Layer groups. A group is a row with no pixels; it holds the layers straight below it that
//! are deeper than it. Hiding or locking a group hides or locks everything in it.
//!
//! Layers are counted from the bottom, so a group's contents have lower numbers than its row.
//! The list keeps one rule: the top layer is in no group, and each layer is at most as deep as
//! the one above it, or one deeper when the one above is a group.

use crate::document::{ColorMode, Document, LimitError, MAX_LAYERS};
use crate::layer::{Layer, LayerKind};

/// How deep groups may go.
pub const MAX_GROUP_DEPTH: u8 = 4;

/// A run of rows, lowest and highest.
pub type Block = (usize, usize);

impl Document {
    /// The layer at `index` together with everything in it, if it is a group.
    pub fn block(&self, index: usize) -> Block {
        let Some(layer) = self.layers.get(index) else {
            return (index, index);
        };
        if layer.kind != LayerKind::Group {
            return (index, index);
        }
        let inside = self
            .layers
            .get(..index)
            .unwrap_or_default()
            .iter()
            .rev()
            .take_while(|l| l.depth > layer.depth)
            .count();
        (index - inside, index)
    }

    /// The group the layer at `index` is in, if any.
    pub fn parent(&self, index: usize) -> Option<usize> {
        let depth = self.layers.get(index)?.depth;
        (depth > 0)
            .then(|| (index + 1..self.layers.len()).find(|&j| self.layers[j].depth < depth))
            .flatten()
    }

    /// The rows the layer list shows, top first: every layer not inside a folded group.
    pub fn listed_layers(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.layers.len());
        let mut fold: Option<u8> = None;
        for (index, layer) in self.layers.iter().enumerate().rev() {
            if fold.is_some_and(|depth| layer.depth > depth) {
                continue;
            }
            let folded = layer.kind == LayerKind::Group && layer.folded;
            fold = folded.then_some(layer.depth);
            out.push(index);
        }
        out
    }

    /// For every layer, whether it is shown: its own eye and every group it is in.
    pub fn shown_layers(&self) -> Vec<bool> {
        self.through_groups(|l| l.visible, true)
    }

    /// For every layer, whether it is locked by its own lock or by a group it is in.
    pub fn locked_layers(&self) -> Vec<bool> {
        self.through_groups(|l| l.locked, false)
    }

    /// For every layer, whether it is part of the picture that is exported.
    pub fn picture_layers(&self) -> Vec<bool> {
        let shown = self.shown_layers();
        let drawn = |(i, l): (usize, &Layer)| {
            shown[i] && matches!(l.kind, LayerKind::Paint | LayerKind::Tilemap(_))
        };
        self.layers.iter().enumerate().map(drawn).collect()
    }

    /// For every layer, whether the screen draws it: shown paint, reference and tilemap layers.
    pub fn screen_layers(&self) -> Vec<bool> {
        let mut out = Vec::new();
        self.screen_layers_into(&mut out);
        out
    }

    /// [`Document::screen_layers`] written into `out`, so that a buffer kept from frame to
    /// frame is reused rather than a new one made for every frame.
    pub fn screen_layers_into(&self, out: &mut Vec<bool>) {
        self.through_groups_into(|l| l.visible, true, out);
        for (drawn, layer) in out.iter_mut().zip(self.layers.iter()) {
            *drawn &= layer.kind != LayerKind::Group;
        }
    }

    /// True when the layer at `index` is locked, by itself or by a group it is in. Asked for
    /// every command that draws, so it walks up the groups instead of making a list.
    pub fn locked_through(&self, index: usize) -> bool {
        let mut at = Some(index);
        while let Some(i) = at {
            match self.layers.get(i) {
                Some(layer) if layer.locked => return true,
                Some(_) => at = self.parent(i),
                None => return false,
            }
        }
        false
    }

    /// `own` of each layer joined with that of every group it is in: with `and`, all of them
    /// must hold; otherwise any one is enough.
    fn through_groups(&self, own: impl Fn(&Layer) -> bool, and: bool) -> Vec<bool> {
        let mut out = Vec::new();
        self.through_groups_into(own, and, &mut out);
        out
    }

    fn through_groups_into(&self, own: impl Fn(&Layer) -> bool, and: bool, out: &mut Vec<bool>) {
        let mut chain = [and; MAX_GROUP_DEPTH as usize + 2];
        out.clear();
        out.resize(self.layers.len(), and);
        for (index, layer) in self.layers.iter().enumerate().rev() {
            let depth = usize::from(layer.depth).min(MAX_GROUP_DEPTH as usize);
            let value = match and {
                true => chain[depth] && own(layer),
                false => chain[depth] || own(layer),
            };
            out[index] = value;
            if layer.kind == LayerKind::Group {
                chain[depth + 1] = value;
            }
        }
    }

    /// Checks the rule the list keeps, and that no layer is too deep.
    pub fn check_groups(&self) -> Result<(), LimitError> {
        let layers = &self.layers;
        if layers.iter().any(|l| l.depth > MAX_GROUP_DEPTH) {
            return Err(LimitError::TooDeep);
        }
        if layers.last().is_some_and(|l| l.depth != 0) {
            return Err(LimitError::GroupOrder);
        }
        let fits = |pair: &[Layer]| {
            let opens = u8::from(pair[1].kind == LayerKind::Group);
            pair[0].depth <= pair[1].depth + opens
        };
        match layers.windows(2).all(fits) {
            true => Ok(()),
            false => Err(LimitError::GroupOrder),
        }
    }

    /// The depth a layer put in at `at` takes: that of the layer it lands above.
    pub(crate) fn depth_for(&self, at: usize) -> u8 {
        let below = at.checked_sub(1).and_then(|i| self.layers.get(i));
        below.or(self.layers.first()).map_or(0, |l| l.depth)
    }

    /// Puts the layer at `index`, with everything in it, into a new group. Returns the group.
    pub(crate) fn group_layer(&mut self, index: usize) -> Result<usize, LimitError> {
        if index >= self.layers.len() {
            return Err(LimitError::NoSuchItem);
        }
        if self.layers.len() >= MAX_LAYERS {
            return Err(LimitError::TooManyLayers);
        }
        let (low, high) = self.block(index);
        let rows = low..=high;
        if self
            .layers
            .get(rows.clone())
            .unwrap_or_default()
            .iter()
            .any(|l| l.depth >= MAX_GROUP_DEPTH)
        {
            return Err(LimitError::TooDeep);
        }
        let depth = self.layers[high].depth;
        for row in rows {
            self.layers[row].depth += 1;
        }
        let groups = self.layers.iter().filter(|l| l.kind == LayerKind::Group);
        let mut group = Layer::new(format!("Group {}", groups.count() + 1), ColorMode::Rgba)
            .with_cels(vec![self.blank_cel(); self.frames.len()]);
        group.kind = LayerKind::Group;
        group.depth = depth;
        group.set_persisted_id(self.new_persisted_id());
        self.layers.insert(high + 1, group);
        Ok(high + 1)
    }

    /// Takes the layers out of the group at `index` and removes the group. Each keeps its own
    /// eye and lock: those are never undone, so ungrouping must not change them.
    pub(crate) fn ungroup(&mut self, index: usize) -> Result<(), LimitError> {
        let group = self.layers.get(index).ok_or(LimitError::NoSuchItem)?;
        if group.kind != LayerKind::Group {
            return Err(LimitError::NoSuchItem);
        }
        if self.layers.len() == 1 {
            return Err(LimitError::LastLayer);
        }
        let (low, _) = self.block(index);
        for row in low..index {
            self.layers[row].depth -= 1;
        }
        self.layers.remove(index);
        Ok(())
    }

    /// Removes the layer at `index` and everything in it.
    pub(crate) fn remove_block(&mut self, index: usize) -> Result<(), LimitError> {
        if index >= self.layers.len() {
            return Err(LimitError::NoSuchItem);
        }
        let (low, high) = self.block(index);
        if high - low + 1 >= self.layers.len() {
            return Err(LimitError::LastLayer);
        }
        for row in (low..=high).rev() {
            self.layers.remove(row);
        }
        self.tidy_holds();
        Ok(())
    }

    /// Puts a copy of the layer at `index`, with everything in it, above it. Returns the copy.
    pub(crate) fn duplicate_block(&mut self, index: usize) -> Result<usize, LimitError> {
        if index >= self.layers.len() {
            return Err(LimitError::NoSuchItem);
        }
        let (low, high) = self.block(index);
        let count = high - low + 1;
        if self.layers.len() + count > MAX_LAYERS {
            return Err(LimitError::TooManyLayers);
        }
        for k in 0..count {
            self.duplicate_layer_to(low + k, high + 1 + k)?;
        }
        self.link_copies((low, high), high + 1);
        Ok(high + count)
    }

    /// Moves the layer at `index`, with everything in it, one place up or down the list, into
    /// and out of groups on the way. A folded group is passed over whole. Returns where the
    /// layer is now.
    pub(crate) fn step_layer(&mut self, index: usize, up: bool) -> Result<usize, LimitError> {
        let id = self.layers.get(index).ok_or(LimitError::NoSuchItem)?.id();
        let block = self.block(index);
        match up {
            true => self.step_up(block)?,
            false => self.step_down(block)?,
        }
        self.check_groups()?;
        let now = self.layers.iter().position(|l| l.id() == id);
        now.ok_or(LimitError::NoSuchItem)
    }

    fn step_up(&mut self, (low, high): Block) -> Result<(), LimitError> {
        let depth = self.layers[high].depth;
        let above = self.layers.get(high + 1).ok_or(LimitError::NoSuchItem)?;
        if above.depth < depth {
            // The top of a group: out over its row.
            self.layers.move_to(high + 1, low);
            return self.shift_depth((low + 1, high + 1), false);
        }
        let sibling = (high + 1..self.layers.len())
            .find(|&j| self.layers[j].depth == depth)
            .ok_or(LimitError::GroupOrder)?;
        if self.opens(sibling) {
            // In at the bottom of the group above.
            return self.shift_depth((low, high), true);
        }
        for k in 0..sibling - high {
            self.layers.move_to(high + 1 + k, low + k);
        }
        Ok(())
    }

    fn step_down(&mut self, (low, high): Block) -> Result<(), LimitError> {
        let depth = self.layers[high].depth;
        let below = low.checked_sub(1).map(|i| &self.layers[i]);
        match below {
            None if depth == 0 => return Err(LimitError::NoSuchItem),
            // The bottom of a group: out below it, in place.
            Some(b) if b.depth < depth => return self.shift_depth((low, high), false),
            None => return self.shift_depth((low, high), false),
            Some(_) => {}
        }
        let next = low - 1;
        if self.opens(next) {
            // In at the top of the group below, under its row.
            self.layers.move_to(next, high);
            return self.shift_depth((next, high - 1), true);
        }
        let (first, _) = self.block(next);
        for k in 0..high - low + 1 {
            self.layers.move_to(low + k, first + k);
        }
        Ok(())
    }

    /// True when the row at `index` is a group a layer stepping past goes into.
    fn opens(&self, index: usize) -> bool {
        let layer = &self.layers[index];
        layer.kind == LayerKind::Group && !layer.folded
    }

    /// Makes every row of the block one deeper, or one shallower.
    fn shift_depth(&mut self, (low, high): Block, deeper: bool) -> Result<(), LimitError> {
        let rows = self.layers.get(low..=high).unwrap_or_default();
        if deeper && rows.iter().any(|l| l.depth >= MAX_GROUP_DEPTH) {
            return Err(LimitError::TooDeep);
        }
        for row in low..=high {
            let layer = &mut self.layers[row];
            layer.depth = match deeper {
                true => layer.depth + 1,
                false => layer.depth.saturating_sub(1),
            };
        }
        Ok(())
    }
}
