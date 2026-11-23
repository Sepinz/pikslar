//! The limits a script is held to, and the cost model for memory and work.
//!
//! See `docs/scripting-language.md`, "Limits" and "Memory accounting".

/// Deepest nesting of blocks and expressions together, counted while parsing. It bounds the
/// interpreter's recursion within one function call, and the recursion of dropping a tree.
pub const MAX_NEST: u32 = 64;
/// Most local variable slots one function may use.
pub const MAX_LOCALS: usize = 256;
/// Most arguments one call may pass.
pub const MAX_ARGS: usize = 16;
/// Most functions one script may define.
pub const MAX_FUNCTIONS: usize = 1024;
/// Highest call depth a host may set.
pub const MAX_CALL_DEPTH: u32 = 256;
/// The cancel flag is read at least once every this many fuel units.
pub const CANCEL_INTERVAL: u64 = 1024;
/// Builtins burn one fuel unit for every this many bytes they read, write or compare.
pub const BYTES_PER_FUEL: usize = 16;
/// Native stack a run may use. Before each call to a script function the interpreter
/// measures how deep its own stack has grown since the run began, and stops the run with a
/// call depth error past this budget, whatever the build. One function, nested as deeply as
/// the parser allows, adds far less than the margin to [`RECOMMENDED_STACK`].
pub const STACK_BUDGET: usize = 8 << 20;
/// Thread stack a host should give a script run: the budget plus a margin of 8 MiB. A test
/// runs the deepest script the limits allow on a thread of this size in an unoptimised
/// build, where frames are largest.
pub const RECOMMENDED_STACK: usize = 16 << 20;

/// Limits for compiling and running one script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Units of work allowed for one run.
    pub fuel: u64,
    /// Bytes of memory the program and everything it allocates may hold at once.
    pub memory: usize,
    /// Deepest nesting of calls to script functions. Capped at [`MAX_CALL_DEPTH`].
    pub call_depth: u32,
    /// Largest source accepted, in bytes.
    pub source_bytes: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            fuel: 50_000_000,
            memory: 16 << 20,
            call_depth: 128,
            source_bytes: 256 << 10,
        }
    }
}

impl Limits {
    pub(crate) fn depth(&self) -> u32 {
        self.call_depth.min(MAX_CALL_DEPTH)
    }
}

/// Bookkeeping the system heap adds to every allocation: a header, and rounding up to its
/// granule. The Windows heap uses 16-byte granules and an 8 to 16 byte header.
pub const ALLOC_OVERHEAD: usize = 32;
const GRANULE: usize = 16;
/// From this size the Windows heap takes whole pages from the system for each allocation.
const LARGE: usize = 256 << 10;
const PAGE: usize = 4096;

/// What an allocation of `bytes` is charged: never less than the heap really uses for it.
/// `None` when the size cannot be represented, which callers treat as over the cap.
pub fn alloc_charge(bytes: usize) -> Option<usize> {
    if bytes < LARGE {
        round_up(bytes.checked_add(ALLOC_OVERHEAD)?, GRANULE)
    } else {
        round_up(bytes.checked_add(PAGE)?, PAGE)
    }
}

fn round_up(n: usize, to: usize) -> Option<usize> {
    Some(n.checked_add(to - 1)? / to * to)
}

/// Fuel for a builtin that touches `bytes` bytes: one unit, plus one per [`BYTES_PER_FUEL`].
pub fn byte_fuel(bytes: usize) -> u64 {
    1 + (bytes / BYTES_PER_FUEL) as u64
}

/// Fuel for a builtin that touches `items` list elements: one unit, plus one per element.
pub fn item_fuel(items: usize) -> u64 {
    (items as u64).saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charges_cover_header_and_granule() {
        assert_eq!(alloc_charge(0), Some(32));
        assert_eq!(alloc_charge(1), Some(48));
        assert_eq!(alloc_charge(16), Some(48));
        assert_eq!(alloc_charge(17), Some(64));
    }

    #[test]
    fn large_charges_round_to_pages_with_a_spare_page() {
        let n = 1 << 20;
        assert_eq!(alloc_charge(n), Some(n + 4096));
        assert_eq!(alloc_charge(n + 1), Some(n + 8192));
    }

    #[test]
    fn impossible_sizes_have_no_charge() {
        assert_eq!(alloc_charge(usize::MAX), None);
        assert_eq!(alloc_charge(usize::MAX - 20), None);
    }

    #[test]
    fn work_fuel_grows_with_size() {
        assert_eq!(byte_fuel(0), 1);
        assert_eq!(byte_fuel(32), 3);
        assert_eq!(item_fuel(10), 11);
        assert!(item_fuel(usize::MAX) >= usize::MAX as u64);
    }

    #[test]
    fn call_depth_is_capped() {
        let l = Limits {
            call_depth: 10_000,
            ..Limits::default()
        };
        assert_eq!(l.depth(), MAX_CALL_DEPTH);
    }
}
