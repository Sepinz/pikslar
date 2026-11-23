//! How much a run has grown the document, against the most it may (`ScriptLimits::growth`).
//!
//! The script's own memory is capped by the interpreter, but what it makes in the document,
//! pixels in new tiles and the cels of new layers and frames, is the document's. Before the
//! first edit the document is measured ([`pikslar_api::Script::held`]). Edits note the fuel
//! they paid, and no edit adds more than [`MOST_BYTES_PER_UNIT`] bytes for each unit it paid,
//! so the document is measured again only once the paid fuel could have used up what is left
//! of the budget, or once it is [`MEASURE_EVERY`] times what a measure costs, whichever is
//! later: measuring adds a few percent to a run at most. At the end of the run it is measured
//! again if the edits since the last measure could have passed the budget. Growth past the
//! budget stops the run, which is taken back like any failure.

use crate::binding::Binding;
use pikslar_script::HostError;

/// The most document bytes an edit adds per fuel unit it pays: about 4 a pixel written at 2
/// units, and 8 to 32 a cel entry or tile reference for edits priced by layers and frames.
pub const MOST_BYTES_PER_UNIT: u64 = 64;
/// Fuel paid by edits since the last measure, per cel or tile one measure may visit, before
/// a measure is taken even with room left in the budget.
pub const MEASURE_EVERY: u64 = 8;

#[derive(Default)]
pub struct Growth {
    /// What the document held before the run's first edit, once measured.
    start: Option<usize>,
    /// How much it had grown at the last measure.
    grown: usize,
    /// Fuel paid by edits since the last measure.
    owed: u64,
    pub budget: usize,
}

impl Growth {
    /// True when the edits paid for since the last measure could have used up the budget.
    fn could_have_passed(&self) -> bool {
        let left = self.budget.saturating_sub(self.grown) as u64;
        self.owed.saturating_mul(MOST_BYTES_PER_UNIT) > left
    }

    /// True when enough has been paid since the last measure to measure again.
    fn due(&self, measure_cost: u64) -> bool {
        let left = self.budget.saturating_sub(self.grown) as u64;
        let could_fill = left / MOST_BYTES_PER_UNIT;
        self.owed >= could_fill.max(measure_cost.saturating_mul(MEASURE_EVERY))
    }
}

impl Binding<'_, '_> {
    /// Measures the document before the run's first edit.
    pub fn before_edit(&mut self) {
        if self.growth.start.is_none() {
            self.growth.start = Some(self.script_held());
        }
    }

    /// Notes an edit that paid `paid` fuel, and measures when enough has been paid.
    pub fn after_edit(&mut self, name: &str, paid: u64) -> Result<(), HostError> {
        self.growth.owed = self.growth.owed.saturating_add(paid);
        if !self.growth.due(self.script_held_cost() as u64) {
            return Ok(());
        }
        self.check_growth(name)
    }

    /// At the end of the run: measures again only if the budget could have been passed.
    pub fn check_growth_at_end(&mut self) -> Result<(), HostError> {
        match self.growth.could_have_passed() {
            true => self.check_growth("the end of the script"),
            false => Ok(()),
        }
    }

    /// Measures now, and stops the run if it grew the document past its budget.
    pub fn check_growth(&mut self, name: &str) -> Result<(), HostError> {
        let Some(start) = self.growth.start else {
            return Ok(());
        };
        self.growth.owed = 0;
        self.growth.grown = self.script_held().saturating_sub(start);
        match self.growth.grown > self.growth.budget {
            false => Ok(()),
            true => Err(HostError::new(format!(
                "{name}: the run grew the document by more than {} MiB, the most a run may add",
                self.growth.budget >> 20
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_come_when_the_budget_could_be_used_up_or_they_are_cheap_enough() {
        let mut g = Growth {
            budget: 64 << 20,
            ..Growth::default()
        };
        g.owed = (64 << 20) / MOST_BYTES_PER_UNIT - 1;
        assert!(!g.due(1));
        g.owed += 1;
        assert!(g.due(1));
        g.grown = 64 << 20;
        g.owed = 7;
        assert!(!g.due(1), "a measure costs more than was paid");
        g.owed = 8;
        assert!(g.due(1));
    }
}
