use super::*;

/// The 47 reduced masks of the published cr31 blob reference, written out by hand
/// (N=1, NE=2, E=4, SE=8, S=16, SW=32, W=64, NW=128), in slot order.
const BLOB_TABLE: [u8; 47] = [
    0, 1, 4, 5, 7, 16, 17, 20, 21, 23, 28, 29, 31, 64, 65, 68, 69, 71, 80, 81, 84, 85, 87, 92, 93,
    95, 112, 113, 116, 117, 119, 124, 125, 127, 193, 197, 199, 209, 213, 215, 221, 223, 241, 245,
    247, 253, 255,
];

/// Independent reference: the reduced mask built bit by bit from booleans.
fn reference_reduce(mask: u8) -> u8 {
    let has = |w: u8| mask & w != 0;
    let mut out = 0;
    for (w, keep) in [
        (N, true),
        (E, true),
        (S, true),
        (W, true),
        (NE, has(N) && has(E)),
        (SE, has(S) && has(E)),
        (SW, has(S) && has(W)),
        (NW, has(N) && has(W)),
    ] {
        if keep && has(w) {
            out |= w;
        }
    }
    out
}

#[test]
fn blob_masks_are_the_published_table() {
    for (slot, &m) in BLOB_TABLE.iter().enumerate() {
        assert_eq!(blob_mask_of_slot(slot as u8), Some(m));
    }
    assert_eq!(blob_mask_of_slot(47), None);
}

#[test]
fn all_256_neighbourhoods_reduce_to_the_table() {
    let mut seen = [false; 47];
    for mask in 0..=255u8 {
        let r = blob_reduce(mask);
        assert_eq!(r, reference_reduce(mask), "mask {mask}");
        let slot = blob_slot(mask);
        assert_eq!(BLOB_TABLE[usize::from(slot)], r, "mask {mask}");
        seen[usize::from(slot)] = true;
    }
    assert!(seen.iter().all(|&s| s), "all 47 classes are reached");
}

#[test]
fn reduction_yields_exactly_47_distinct_classes() {
    let mut classes: Vec<u8> = (0..=255u8).map(blob_reduce).collect();
    classes.sort_unstable();
    classes.dedup();
    assert_eq!(classes.len(), 47);
    assert_eq!(classes, BLOB_TABLE);
}

#[test]
fn reduction_is_idempotent() {
    for mask in 0..=255u8 {
        assert_eq!(blob_reduce(blob_reduce(mask)), blob_reduce(mask));
    }
}

#[test]
fn blob_known_cases() {
    assert_eq!(blob_slot(0), 0);
    assert_eq!(blob_slot(255), 46);
    // Corner alone does nothing.
    assert_eq!(blob_slot(NE), 0);
    // A corner next to only one of its edges is ignored.
    assert_eq!(blob_slot(N | NE), blob_slot(N));
    // Both edges set: the corner counts.
    assert_ne!(blob_slot(N | E | NE), blob_slot(N | E));
    assert_eq!(blob_reduce(N | E | NE), 7);
    assert_eq!(blob_reduce(N | E | S | W), 85);
}

#[test]
fn edge4_all_neighbourhoods() {
    for mask in 0..=255u8 {
        let expect = u8::from(mask & N != 0)
            | (u8::from(mask & E != 0) << 1)
            | (u8::from(mask & S != 0) << 2)
            | (u8::from(mask & W != 0) << 3);
        assert_eq!(edge4_slot(mask), expect, "mask {mask}");
        assert!(edge4_slot(mask) < 16);
    }
    assert_eq!(edge4_slot(N | E | S | W), 15);
}

#[test]
fn corner4_all_sixteen_states() {
    for state in 0..16u8 {
        let mask = (u8::from(state & 1 != 0) * NE)
            | (u8::from(state & 2 != 0) * SE)
            | (u8::from(state & 4 != 0) * SW)
            | (u8::from(state & 8 != 0) * NW);
        assert_eq!(corner4_slot(mask), state);
        // Edge bits never disturb it.
        assert_eq!(corner4_slot(mask | N | E | S | W), state);
    }
}

#[test]
fn wanted_slot_dispatches_by_kind() {
    let m = N | E | NE | SW;
    assert_eq!(wanted_slot(RuleKind::Edge4, m), edge4_slot(m));
    assert_eq!(wanted_slot(RuleKind::Corner4, m), corner4_slot(m));
    assert_eq!(wanted_slot(RuleKind::Blob47, m), blob_slot(m));
    assert_eq!(RuleKind::Blob47.slot_count(), 47);
    assert_eq!(RuleKind::Edge4.slot_count(), 16);
}

#[test]
fn defined_slot_is_used_as_is() {
    for kind in [RuleKind::Edge4, RuleKind::Corner4, RuleKind::Blob47] {
        for slot in 0..kind.slot_count() as u8 {
            assert_eq!(pick_slot(kind, slot, u64::MAX), Some(slot));
        }
    }
}

#[test]
fn nothing_defined_or_out_of_range_gives_none() {
    assert_eq!(pick_slot(RuleKind::Edge4, 3, 0), None);
    assert_eq!(pick_slot(RuleKind::Blob47, 46, 0), None);
    assert_eq!(pick_slot(RuleKind::Edge4, 16, u64::MAX), None);
    assert_eq!(pick_slot(RuleKind::Blob47, 47, u64::MAX), None);
}

#[test]
fn missing_slot_takes_the_closest_then_the_lower() {
    // Edge4: want 15 (all four), only 7 (three bits) and 13 defined: both differ by one bit,
    // the lower slot wins.
    assert_eq!(pick_slot(RuleKind::Edge4, 15, 1 << 7 | 1 << 13), Some(7));
    // Want 3; defined 0 (differs 2) and 1 (differs 1): 1 wins.
    assert_eq!(pick_slot(RuleKind::Edge4, 3, 1 | 1 << 1 | 1 << 12), Some(1));
    // Want 5 with only 10 defined (differs in all four): still chosen, never a hole.
    assert_eq!(pick_slot(RuleKind::Corner4, 5, 1 << 10), Some(10));
}

#[test]
fn blob_fallback_compares_neighbour_masks() {
    // Full interior (mask 255) missing; slot 45 is mask 253 (differs by one bit, E-less... bit 1).
    let defined = 1u64 << 45 | 1;
    assert_eq!(pick_slot(RuleKind::Blob47, 46, defined), Some(45));
    // Isolated (mask 0) missing; mask 1 (slot 1) is nearer than the full tile.
    let defined = 1u64 << 1 | 1u64 << 46;
    assert_eq!(pick_slot(RuleKind::Blob47, 0, defined), Some(1));
}

#[test]
fn fallback_always_finds_something_when_any_slot_is_defined() {
    for kind in [RuleKind::Edge4, RuleKind::Corner4, RuleKind::Blob47] {
        for wanted in 0..kind.slot_count() as u8 {
            for only in 0..kind.slot_count() as u8 {
                assert_eq!(pick_slot(kind, wanted, 1 << only), Some(only));
            }
        }
    }
}
