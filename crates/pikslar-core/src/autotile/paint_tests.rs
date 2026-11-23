//! The rules of terrain painting against small maps worked out by hand.

use super::*;
use crate::tileset::TileId;
use std::collections::HashMap;

/// Slot `s` of every test set holds tile `100 + s`.
fn tile_of(slot: u8) -> u32 {
    100 + u32::from(slot)
}

/// A set of a kind with the given slots filled.
fn set(kind: RuleKind, slots: impl IntoIterator<Item = u8>) -> Rule {
    let mut set = TerrainSet::new(1, "T".into(), kind);
    for slot in slots {
        let tile = TileId::new(tile_of(slot)).expect("an id");
        set.set_slot(usize::from(slot), Some(tile)).expect("a slot");
    }
    Rule::new(&set)
}

fn full(kind: RuleKind) -> Rule {
    set(kind, 0..kind.slot_count() as u8)
}

type Map = HashMap<At, u32>;

fn read(map: &Map) -> impl Fn(At) -> u32 + '_ {
    |c| map.get(&c).copied().unwrap_or(0)
}

fn paint(points: &[At], clear: bool) -> Paint {
    Paint {
        clear,
        vertices: false,
        points: points.iter().copied().collect(),
    }
}

/// The map after the changes.
fn after(rule: &Rule, paint: &Paint, map: &Map) -> Map {
    let mut out = map.clone();
    for (c, v) in changes(rule, paint, &read(map), |_| true) {
        out.insert(c, v);
    }
    out.retain(|_, v| *v != 0);
    out
}

/// The slot each cell of a map holds, as `(x, y, slot)`, in row order.
fn slots(map: &Map) -> Vec<(i32, i32, u32)> {
    let mut out: Vec<_> = map.iter().map(|(&(x, y), &v)| (x, y, v - 100)).collect();
    out.sort_unstable_by_key(|&(x, y, _)| (y, x));
    out
}

#[test]
fn edge4_paints_a_plus_with_the_slots_of_its_sides() {
    let rule = full(RuleKind::Edge4);
    let plus = [(1, 0), (0, 1), (1, 1), (2, 1), (1, 2)];
    let out = after(&rule, &paint(&plus, false), &Map::new());
    // N = 1, E = 2, S = 4, W = 8.
    assert_eq!(
        slots(&out),
        [(1, 0, 4), (0, 1, 2), (1, 1, 15), (2, 1, 8), (1, 2, 1)]
    );
}

#[test]
fn blob47_paints_blocks_with_the_conventional_slots() {
    let rule = full(RuleKind::Blob47);
    // The 47 reduced masks in order: slot 4 is 7, slot 10 is 28, slot 26 is 112, slot 34 is 193.
    let block = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let out = after(&rule, &paint(&block, false), &Map::new());
    assert_eq!(slots(&out), [(0, 0, 10), (1, 0, 26), (0, 1, 4), (1, 1, 34)]);
    // The middle of a 3 by 3 block is the full interior, slot 46 (mask 255).
    let nine: Vec<At> = (0..3).flat_map(|y| (0..3).map(move |x| (x, y))).collect();
    let out = after(&rule, &paint(&nine, false), &Map::new());
    assert_eq!(out[&(1, 1)], tile_of(46));
    // A corner neighbour alone is dropped by the reduction: slot 0, the lone tile.
    let out = after(&rule, &paint(&[(0, 0), (1, 1)], false), &Map::new());
    assert_eq!((out[&(0, 0)], out[&(1, 1)]), (tile_of(0), tile_of(0)));
}

#[test]
fn corner4_paints_a_cell_with_its_corners_shared_by_its_neighbours() {
    let rule = full(RuleKind::Corner4);
    let out = after(&rule, &paint(&[(1, 1)], false), &Map::new());
    // NE = 1, SE = 2, SW = 4, NW = 8.
    assert_eq!(
        slots(&out),
        [
            (0, 0, 2),
            (1, 0, 6),
            (2, 0, 4),
            (0, 1, 3),
            (1, 1, 15),
            (2, 1, 12),
            (0, 2, 1),
            (1, 2, 9),
            (2, 2, 8),
        ]
    );
}

#[test]
fn corner4_painted_by_a_vertex_changes_the_four_cells_around_it() {
    let rule = full(RuleKind::Corner4);
    let mut by_vertex = paint(&[(1, 1)], false);
    by_vertex.vertices = true;
    let out = after(&rule, &by_vertex, &Map::new());
    assert_eq!(slots(&out), [(0, 0, 2), (1, 0, 4), (0, 1, 1), (1, 1, 8)]);
    // Clearing the vertex again takes the corners back: slot 0 is the tile with none.
    let mut clear = by_vertex.clone();
    clear.clear = true;
    let back = after(&rule, &clear, &out);
    assert!(back.values().all(|&v| v == tile_of(0)), "{back:?}");
    let no_slot_0 = set(RuleKind::Corner4, 1..16);
    let back = after(&no_slot_0, &clear, &out);
    assert!(back.is_empty(), "without slot 0 the cells become empty");
}

#[test]
fn a_missing_slot_takes_the_defined_one_sharing_most_bits_ties_to_the_lower() {
    // A cell wanting N | E | S (7) of slots 0, 3 (N | E) and 5 (N | S): 3 and 5 both differ by
    // one bit, so 3 is chosen.
    let rule = set(RuleKind::Edge4, [0, 3, 5]);
    let shape = [(1, 0), (1, 1), (2, 1), (1, 2)];
    let out = after(&rule, &paint(&shape, false), &Map::new());
    assert_eq!(out[&(1, 1)], tile_of(3));
    // With no slot defined nothing is placed, and a cell holding something keeps it.
    let none = set(RuleKind::Edge4, []);
    let map = Map::from([((5, 5), 7)]);
    assert_eq!(after(&none, &paint(&[(5, 5), (0, 0)], false), &map), map);
}

#[test]
fn erasing_clears_membership_and_the_neighbours_follow() {
    let rule = full(RuleKind::Edge4);
    let row = Map::from([
        ((0, 0), tile_of(2)),
        ((1, 0), tile_of(10)),
        ((2, 0), tile_of(8)),
    ]);
    let out = after(&rule, &paint(&[(1, 0)], true), &row);
    assert_eq!(slots(&out), [(0, 0, 0), (2, 0, 0)]);
    // A cell that was not a member is left as it is.
    let other = Map::from([((4, 4), 7)]);
    assert_eq!(after(&rule, &paint(&[(4, 4)], true), &other), other);
}

#[test]
fn a_pinned_neighbour_keeps_its_tile_and_still_counts_as_a_member() {
    let rule = full(RuleKind::Edge4);
    let pinned = tile_of(0) | PINNED;
    let map = Map::from([((1, 0), pinned)]);
    let out = after(&rule, &paint(&[(0, 0)], false), &map);
    assert_eq!(out[&(1, 0)], pinned, "never changed as a neighbour");
    assert_eq!(out[&(0, 0)], tile_of(2), "the pinned cell is east of it");
    // Painting the pinned cell itself unpins it.
    let out = after(&rule, &paint(&[(1, 0)], false), &map);
    assert_eq!(out[&(1, 0)], tile_of(0));
}

#[test]
fn retiling_a_cell_follows_its_neighbours_and_unpins_it() {
    let rule = full(RuleKind::Edge4);
    let map = Map::from([((0, 0), tile_of(7) | PINNED), ((1, 0), tile_of(8))]);
    assert_eq!(retile(&rule, &read(&map), (0, 0)), Some(tile_of(2)));
    assert_eq!(retile(&rule, &read(&map), (5, 5)), None, "not a member");
    // Corner4: each corner takes what most of the other three cells at its vertex say.
    let rule = full(RuleKind::Corner4);
    let around = Map::from([
        ((0, 0), tile_of(2)),
        ((1, 0), tile_of(4)),
        ((0, 1), tile_of(1)),
        ((1, 1), tile_of(15) | PINNED),
    ]);
    assert_eq!(retile(&rule, &read(&around), (1, 1)), Some(tile_of(8)));
}

#[test]
fn the_affected_cells_are_each_painted_cell_and_its_neighbours_once_in_row_order() {
    let cells = paint(&[(0, 0), (1, 0)], false).affected();
    assert_eq!(cells.len(), 12);
    assert_eq!(cells.first(), Some(&(-1, -1)));
    assert_eq!(cells.last(), Some(&(2, 1)));
    assert!(cells
        .windows(2)
        .all(|w| (w[0].1, w[0].0) < (w[1].1, w[1].0)));
}
