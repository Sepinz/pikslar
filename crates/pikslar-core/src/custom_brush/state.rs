//! The brushes an editor holds.

use super::brush::Brush;
use super::preset::{round_stamp, Preset};
use super::stamp::Prepared;
use crate::color::ColorTable;
use crate::document::{ColorMode, LimitError};
use std::sync::Arc;

/// Session brushes kept, newest first.
pub const MAX_RECENT_BRUSHES: usize = 8;

/// Which brush paints.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BrushChoice {
    /// Today's round brush.
    #[default]
    Round,
    Preset(Preset),
    Custom(Arc<Brush>),
}

impl BrushChoice {
    /// The stamp for a layer of `mode`. Round brushes and presets are shapes painting `fill`, at
    /// `size`; a custom brush ignores `size`.
    pub fn prepare(
        &self,
        size: i32,
        mode: ColorMode,
        table: &Arc<ColorTable>,
        fill: u32,
    ) -> Result<Prepared, LimitError> {
        let stamp = match self {
            BrushChoice::Round => round_stamp(size, fill),
            BrushChoice::Preset(p) => p.stamp(size, fill),
            BrushChoice::Custom(brush) => return brush.prepare(mode, table, fill),
        };
        Ok(Prepared {
            stamp,
            colors: None,
        })
    }
}

/// The brush in use and the recent ones. View state: never an undo step.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrushState {
    active: BrushChoice,
    recent: Vec<Arc<Brush>>,
    captured: u32,
}

impl BrushState {
    pub fn active(&self) -> &BrushChoice {
        &self.active
    }

    /// The brush in use, if it is a custom one.
    pub fn active_custom(&self) -> Option<&Arc<Brush>> {
        match &self.active {
            BrushChoice::Custom(b) => Some(b),
            _ => None,
        }
    }

    /// Recent brushes, newest first, at most [`MAX_RECENT_BRUSHES`].
    pub fn recent(&self) -> &[Arc<Brush>] {
        &self.recent
    }

    pub(crate) fn set_active(&mut self, choice: BrushChoice) {
        if let BrushChoice::Custom(brush) = &choice {
            self.remember(brush.clone());
        }
        self.active = choice;
    }

    /// Replaces the active custom brush with a changed copy, without adding a recent entry.
    pub(crate) fn replace_active(&mut self, brush: Brush) {
        let BrushChoice::Custom(old) = &self.active else {
            return;
        };
        let new = Arc::new(brush);
        // The recent list holds what is in hand now, not what it was before the turn.
        if let Some(slot) = self.recent.iter_mut().find(|b| Arc::ptr_eq(b, old)) {
            *slot = new.clone();
        }
        self.active = BrushChoice::Custom(new);
        let mut kept: Vec<Arc<Brush>> = Vec::with_capacity(self.recent.len());
        for b in self.recent.drain(..) {
            if !kept.iter().any(|k| **k == *b) {
                kept.push(b);
            }
        }
        self.recent = kept;
    }

    /// The name the next captured brush gets.
    pub(crate) fn next_name(&mut self) -> String {
        self.captured = self.captured.saturating_add(1);
        format!("Brush {}", self.captured)
    }

    fn remember(&mut self, brush: Arc<Brush>) {
        self.recent.retain(|b| **b != *brush);
        self.recent.insert(0, brush);
        self.recent.truncate(MAX_RECENT_BRUSHES);
    }
}

#[path = "remap.rs"]
mod remap;
