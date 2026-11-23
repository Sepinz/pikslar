//! The rule engine: which slot of a terrain set a cell wants, given its neighbours.
//!
//! Everything here is a pure, allocation-free function of small integers.
//!
//! # Neighbour mask
//!
//! The eight neighbours of a cell form one byte, weights running clockwise from north (the
//! cr31 blob convention):
//! `N = 1, NE = 2, E = 4, SE = 8, S = 16, SW = 32, W = 64, NW = 128`.
//! A set bit means the neighbour belongs to the same terrain.
//!
//! # Slot order
//!
//! * [`RuleKind::Edge4`]: 16 slots; slot = `N=1 | E=2 | S=4 | W=8` over the four side neighbours.
//! * [`RuleKind::Corner4`]: 16 slots; slot = `NE=1 | SE=2 | SW=4 | NW=8` over the four corner
//!   states of the cell.
//! * [`RuleKind::Blob47`]: 47 slots, in ascending order of the reduced neighbour mask (the
//!   conventional blob order: slot 0 is the isolated tile with mask 0, slot 46 the full interior
//!   with mask 255). A corner bit survives the reduction only when both edge neighbours beside
//!   it are set.
//!
//! # Missing slots
//!
//! When the wanted slot is not defined, [`pick_slot`] takes the defined slot that agrees with
//! it on the most bits (ties to the lower slot number); with none defined it returns `None` and
//! the caller leaves the cell as it was.

/// North neighbour weight.
pub const N: u8 = 1;
/// North-east neighbour weight.
pub const NE: u8 = 2;
/// East neighbour weight.
pub const E: u8 = 4;
/// South-east neighbour weight.
pub const SE: u8 = 8;
/// South neighbour weight.
pub const S: u8 = 16;
/// South-west neighbour weight.
pub const SW: u8 = 32;
/// West neighbour weight.
pub const W: u8 = 64;
/// North-west neighbour weight.
pub const NW: u8 = 128;

/// The three rule kinds of a terrain set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleKind {
    /// 16 tiles from the four side neighbours.
    Edge4,
    /// 16 tiles from the four corner states.
    Corner4,
    /// 47 tiles from the eight neighbours.
    Blob47,
}

impl RuleKind {
    /// How many slots a set of this kind has.
    #[must_use]
    pub const fn slot_count(self) -> usize {
        match self {
            RuleKind::Edge4 | RuleKind::Corner4 => 16,
            RuleKind::Blob47 => BLOB_COUNT,
        }
    }
}

/// Number of distinct blob neighbourhoods after reduction.
pub const BLOB_COUNT: usize = 47;

/// Marks "no slot" in the lookup table.
const NONE: u8 = 255;

/// The reduced form of a neighbour mask: a corner is kept only when both its edges are set.
#[must_use]
pub const fn blob_reduce(mask: u8) -> u8 {
    let mut m = mask;
    if mask & (N | E) != (N | E) {
        m &= !NE;
    }
    if mask & (E | S) != (E | S) {
        m &= !SE;
    }
    if mask & (S | W) != (S | W) {
        m &= !SW;
    }
    if mask & (W | N) != (W | N) {
        m &= !NW;
    }
    m
}

/// The 47 reduced masks in slot order (ascending), built at compile time.
const BLOB_MASKS: [u8; BLOB_COUNT] = {
    let mut out = [0u8; BLOB_COUNT];
    let mut n = 0;
    let mut m = 0usize;
    while m < 256 {
        if blob_reduce(m as u8) as usize == m {
            out[n] = m as u8;
            n += 1;
        }
        m += 1;
    }
    out
};

/// Slot of every one of the 256 neighbourhoods.
const BLOB_SLOT_OF: [u8; 256] = {
    let mut out = [NONE; 256];
    let mut m = 0usize;
    while m < 256 {
        let r = blob_reduce(m as u8);
        let mut i = 0;
        while i < BLOB_COUNT {
            if BLOB_MASKS[i] == r {
                out[m] = i as u8;
            }
            i += 1;
        }
        m += 1;
    }
    out
};

/// Blob47 slot (0 to 46) for a full eight-neighbour mask.
#[must_use]
pub const fn blob_slot(mask: u8) -> u8 {
    BLOB_SLOT_OF[mask as usize]
}

/// The reduced neighbour mask a Blob47 slot stands for, or `None` past slot 46.
#[must_use]
pub const fn blob_mask_of_slot(slot: u8) -> Option<u8> {
    if (slot as usize) < BLOB_COUNT {
        Some(BLOB_MASKS[slot as usize])
    } else {
        None
    }
}

/// Edge4 slot (0 to 15) from a full eight-neighbour mask; corners are ignored.
#[must_use]
pub const fn edge4_slot(mask: u8) -> u8 {
    (mask & N) | ((mask & E) >> 1) | ((mask & S) >> 2) | ((mask & W) >> 3)
}

/// Corner4 slot (0 to 15) from a full eight-neighbour mask: the corner bits only.
#[must_use]
pub const fn corner4_slot(mask: u8) -> u8 {
    ((mask & NE) >> 1) | ((mask & SE) >> 2) | ((mask & SW) >> 3) | ((mask & NW) >> 4)
}

/// The slot a kind wants for a full eight-neighbour mask.
#[must_use]
pub const fn wanted_slot(kind: RuleKind, mask: u8) -> u8 {
    match kind {
        RuleKind::Edge4 => edge4_slot(mask),
        RuleKind::Corner4 => corner4_slot(mask),
        RuleKind::Blob47 => blob_slot(mask),
    }
}

/// The bit pattern a slot stands for, so that slots can be compared bit by bit.
const fn slot_bits(kind: RuleKind, slot: u8) -> u8 {
    match kind {
        RuleKind::Blob47 => match blob_mask_of_slot(slot) {
            Some(m) => m,
            None => 0,
        },
        _ => slot,
    }
}

/// Picks the slot to use. `defined` has bit `i` set when slot `i` has a tile.
///
/// Returns `wanted` when defined; otherwise the defined slot agreeing with it on the most
/// bits, ties to the lower slot number; `None` when nothing is defined (or `wanted` is out of
/// range for the kind).
#[must_use]
pub fn pick_slot(kind: RuleKind, wanted: u8, defined: u64) -> Option<u8> {
    let count = kind.slot_count();
    if usize::from(wanted) >= count {
        return None;
    }
    if (defined >> wanted) & 1 == 1 {
        return Some(wanted);
    }
    let want = slot_bits(kind, wanted);
    let mut best: Option<(u32, u8)> = None;
    for slot in 0..count as u8 {
        if (defined >> slot) & 1 == 0 {
            continue;
        }
        let diff = (slot_bits(kind, slot) ^ want).count_ones();
        if best.is_none_or(|(d, _)| diff < d) {
            best = Some((diff, slot));
        }
    }
    best.map(|(_, s)| s)
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
