//! The plans of the three remaps on tables alone.

use super::*;
use crate::color::{rgba, GroupKind, PaletteGroup};

const A: u32 = rgba(1, 0, 0, 255);
const B: u32 = rgba(2, 0, 0, 255);
const C: u32 = rgba(3, 0, 0, 255);

/// Ids 1 to 3 hold A, B, C; swatches are 3 then 1; one group holds 1 and 3.
fn table() -> ColorTable {
    let mut t = ColorTable::from_parts(&[0, A, B, C], &[3, 1]);
    t.set_groups(vec![PaletteGroup {
        name: "g".into(),
        kind: GroupKind::Ramp,
        ids: vec![1, 3],
    }]);
    t
}

#[test]
fn swatch_order_puts_swatches_first_and_the_rest_after_in_old_order() {
    let plan = plan(RemapKind::ToSwatchOrder, &table(), Vec::new);
    assert_eq!(plan.map.as_slice(), [0, 2, 3, 1]);
    assert_eq!(plan.table.colors(), [0, C, A, B]);
    assert_eq!(plan.table.swatches(), [1, 2]);
    assert_eq!(plan.table.groups()[0].ids, [2, 1]);
    assert_eq!(plan.table.groups()[0].kind, GroupKind::Ramp);
    assert_eq!(plan.table.id_for_existing(B), Some(3));
}

#[test]
fn merging_maps_to_the_lowest_id_and_keeps_the_table_whole() {
    let mut t = table();
    t.set_color(3, A);
    let plan = plan(RemapKind::MergeDuplicates, &t, Vec::new);
    assert_eq!(plan.map.as_slice(), [0, 1, 2, 1]);
    assert_eq!(plan.table.colors(), t.colors());
    assert_eq!(
        plan.table.version(),
        t.version(),
        "no colour changes its id"
    );
    assert_eq!(plan.table.swatches(), [1], "3 and 1 are one swatch");
    assert_eq!(plan.table.groups()[0].ids, [1]);
}

#[test]
fn merging_never_sends_an_id_to_zero_and_clear_ids_merge_only_among_themselves() {
    let mut t = table();
    t.set_color(1, 0);
    t.set_color(3, 0);
    let merged = plan(RemapKind::MergeDuplicates, &t, Vec::new);
    assert_eq!(merged.map.as_slice(), [0, 1, 2, 1]);
    assert_eq!(merged.table.swatches(), [1], "the clear swatch stays");
    assert_eq!(merged.table.groups()[0].ids, [1]);
    // One clear id alone is left as it is.
    let mut t = table();
    t.set_color(2, 0);
    let alone = plan(RemapKind::MergeDuplicates, &t, Vec::new);
    assert!(alone.map.is_identity());
}

#[test]
fn removing_unused_keeps_what_a_pixel_a_swatch_or_a_group_uses() {
    let mut t = ColorTable::from_parts(&[0, A, B, C], &[]);
    t.set_groups(Vec::new());
    let plan = plan(RemapKind::RemoveUnused, &t, || {
        vec![false, false, true, false]
    });
    assert_eq!(plan.map.as_slice(), [0, 0, 1, 0]);
    assert_eq!(plan.table.colors(), [0, B]);
    // With swatches 3 and 1 and the group, only 2 is unused.
    let plan = super::plan(RemapKind::RemoveUnused, &table(), || vec![false; 4]);
    assert_eq!(plan.map.as_slice(), [0, 1, 0, 2]);
    assert_eq!(plan.table.swatches(), [2, 1]);
}

#[test]
fn an_id_map_leaves_ids_past_the_table_alone() {
    let map = IdMap::replacing(4, 2, 3);
    assert_eq!((map.get(2), map.get(1), map.get(9)), (3, 1, 9));
    assert!(!map.is_identity());
    assert!(IdMap::replacing(4, 9, 3).is_identity());
}
