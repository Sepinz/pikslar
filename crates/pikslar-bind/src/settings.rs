//! The limits a run is held to: host settings, with the defaults the owner accepted.

use pikslar_core::MAX_DIMENSION;
use pikslar_script::{Limits, MAX_CALL_DEPTH};

/// Bytes of the largest cel a document can hold: the largest canvas, 4 bytes a pixel.
const LARGEST_CEL: usize = MAX_DIMENSION * MAX_DIMENSION * 4;

/// Fuel, memory, call depth and document growth for each run. Out-of-range values are
/// brought into range by [`ScriptLimits::clamped`], never refused, since they come from a
/// settings file. The script's memory and the document growth are separate limits: changing
/// one never changes the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptLimits {
    /// Units of work one run may do.
    pub fuel: u64,
    /// Bytes the script may hold at once.
    pub memory: usize,
    /// Deepest nesting of calls to script functions.
    pub call_depth: u32,
    /// Bytes of pixels and structure one run may add to the document. A run that grows the
    /// document by more stops and is taken back.
    pub growth: usize,
}

impl ScriptLimits {
    pub const DEFAULT: ScriptLimits = ScriptLimits {
        fuel: 50_000_000,
        memory: 16 << 20,
        call_depth: 128,
        growth: 256 << 20,
    };
    pub const FUEL_RANGE: (u64, u64) = (10_000, 2_000_000_000);
    pub const MEMORY_RANGE: (usize, usize) = (1 << 20, 256 << 20);
    pub const DEPTH_RANGE: (u32, u32) = (1, MAX_CALL_DEPTH);
    /// From 16 MiB, one 2048 by 2048 cel, to 2 GiB, eight cels of the largest canvas
    /// (8192 by 8192, 256 MiB each). A run that succeeds keeps what it added twice over for
    /// a moment (the document and the scratch copy it replaces), so more than 2 GiB would
    /// put a single run past what a typical machine can give one program.
    pub const GROWTH_RANGE: (usize, usize) = (16 << 20, 8 * LARGEST_CEL);

    /// Each limit within its range.
    pub fn clamped(self) -> ScriptLimits {
        ScriptLimits {
            fuel: self.fuel.clamp(Self::FUEL_RANGE.0, Self::FUEL_RANGE.1),
            memory: self
                .memory
                .clamp(Self::MEMORY_RANGE.0, Self::MEMORY_RANGE.1),
            call_depth: self
                .call_depth
                .clamp(Self::DEPTH_RANGE.0, Self::DEPTH_RANGE.1),
            growth: self
                .growth
                .clamp(Self::GROWTH_RANGE.0, Self::GROWTH_RANGE.1),
        }
    }

    /// Bytes of pixels and structure one run may add to the document, within its range.
    pub fn growth(self) -> usize {
        self.clamped().growth
    }

    pub(crate) fn interpreter(self) -> Limits {
        let s = self.clamped();
        Limits {
            fuel: s.fuel,
            memory: s.memory,
            call_depth: s.call_depth,
            ..Limits::default()
        }
    }
}

impl Default for ScriptLimits {
    fn default() -> ScriptLimits {
        ScriptLimits::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_accepted_ones() {
        let d = ScriptLimits::default();
        assert_eq!(
            (d.fuel, d.memory, d.call_depth, d.growth),
            (50_000_000, 16 << 20, 128, 256 << 20)
        );
        assert_eq!(d.clamped(), d);
        assert_eq!(d.growth(), 256 << 20);
    }

    #[test]
    fn settings_are_brought_into_range() {
        let wild = ScriptLimits {
            fuel: 0,
            memory: usize::MAX,
            call_depth: 0,
            growth: usize::MAX,
        };
        let c = wild.clamped();
        assert_eq!(c.fuel, 10_000);
        assert_eq!(c.memory, 256 << 20);
        assert_eq!(c.call_depth, 1);
        assert_eq!(c.growth, 2048 << 20);
        assert_eq!(wild.interpreter().call_depth, 1);
        let tiny = ScriptLimits {
            growth: 0,
            ..ScriptLimits::DEFAULT
        };
        assert_eq!(tiny.growth(), 16 << 20);
    }

    #[test]
    fn memory_and_growth_are_separate_limits() {
        for memory in [1 << 20, 16 << 20, 256 << 20] {
            let l = ScriptLimits {
                memory,
                ..ScriptLimits::DEFAULT
            };
            assert_eq!(l.growth(), 256 << 20, "memory {memory}");
        }
        let l = ScriptLimits {
            growth: 64 << 20,
            ..ScriptLimits::DEFAULT
        };
        assert_eq!(l.interpreter().memory, 16 << 20);
        assert_eq!(l.growth(), 64 << 20);
    }
}
