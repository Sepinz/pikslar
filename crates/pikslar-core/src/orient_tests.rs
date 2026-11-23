//! Orientations against an independent reference: whole-grid turns written as the named
//! operations (transpose, quarter turns, mirrors), with Tiled's published flag table.

use crate::orient::{Orientation, FLIPPED_DIAGONALLY, FLIPPED_HORIZONTALLY, FLIPPED_VERTICALLY};

type Grid = Vec<Vec<u32>>;

/// A `w` by `h` grid whose cells are all different: `y * 100 + x`.
fn labelled(w: usize, h: usize) -> Grid {
    (0..h)
        .map(|y| (0..w).map(|x| (y * 100 + x) as u32).collect())
        .collect()
}

fn mirror_h(g: &Grid) -> Grid {
    g.iter()
        .map(|row| row.iter().rev().copied().collect())
        .collect()
}

fn mirror_v(g: &Grid) -> Grid {
    g.iter().rev().cloned().collect()
}

fn transpose(g: &Grid) -> Grid {
    let (w, h) = (g[0].len(), g.len());
    (0..w).map(|x| (0..h).map(|y| g[y][x]).collect()).collect()
}

fn turn_cw(g: &Grid) -> Grid {
    mirror_h(&transpose(g))
}

fn turn_ccw(g: &Grid) -> Grid {
    mirror_v(&transpose(g))
}

fn turn_half(g: &Grid) -> Grid {
    mirror_h(&mirror_v(g))
}

fn anti_transpose(g: &Grid) -> Grid {
    turn_half(&transpose(g))
}

/// What Tiled draws for each combination of flags, as published: the three flags in
/// 0x80000000 (horizontal), 0x40000000 (vertical) and 0x20000000 (diagonal), by the named
/// operation each combination equals.
fn reference(flags: u32, g: &Grid) -> Grid {
    match flags >> 29 {
        0b000 => g.clone(),
        0b100 => mirror_h(g),
        0b010 => mirror_v(g),
        0b110 => turn_half(g),
        0b001 => transpose(g),
        0b101 => turn_cw(g),
        0b011 => turn_ccw(g),
        0b111 => anti_transpose(g),
        _ => unreachable!(),
    }
}

/// The grid after moving every cell where `o` says.
fn drawn(o: Orientation, g: &Grid) -> Grid {
    let (w, h) = (g[0].len(), g.len());
    let (tw, th) = o.turned_size(w, h);
    let mut out = vec![vec![u32::MAX; tw]; th];
    for (y, row) in g.iter().enumerate() {
        for (x, &cell) in row.iter().enumerate() {
            let (nx, ny) = o.apply(x, y, w, h);
            assert!(nx < tw && ny < th, "{o:?} sent ({x},{y}) outside {tw}x{th}");
            assert_eq!(out[ny][nx], u32::MAX, "{o:?} sent two cells to one place");
            out[ny][nx] = cell;
        }
    }
    out
}

fn all() -> impl Iterator<Item = Orientation> {
    (0..8).map(|i| Orientation::from_index(i).expect("0 to 7 are orientations"))
}

#[test]
fn flag_masks_are_tiled_s() {
    assert_eq!(FLIPPED_HORIZONTALLY, 0x8000_0000);
    assert_eq!(FLIPPED_VERTICALLY, 0x4000_0000);
    assert_eq!(FLIPPED_DIAGONALLY, 0x2000_0000);
}

#[test]
fn there_are_eight_and_no_ninth() {
    assert_eq!(all().count(), 8);
    assert!(Orientation::from_index(8).is_none());
    assert!(Orientation::from_index(255).is_none());
}

#[test]
fn every_orientation_matches_the_reference_on_square_and_oblong_grids() {
    for (w, h) in [(1, 1), (3, 3), (2, 3), (3, 2), (1, 4), (5, 1)] {
        let g = labelled(w, h);
        for o in all() {
            assert_eq!(
                drawn(o, &g),
                reference(o.to_tiled_bits(), &g),
                "{o:?} on {w}x{h}"
            );
        }
    }
}

#[test]
fn tiled_bits_round_trip_and_ignore_the_other_bits() {
    for o in all() {
        let bits = o.to_tiled_bits();
        assert_eq!(bits & 0x1FFF_FFFF, 0);
        assert_eq!(Orientation::from_tiled_bits(bits), o);
        assert_eq!(
            Orientation::from_tiled_bits(bits | 0x1000_0000 | 0x0FFF_FFFF),
            o
        );
    }
    let seen: std::collections::HashSet<u32> = all().map(Orientation::to_tiled_bits).collect();
    assert_eq!(seen.len(), 8);
}

#[test]
fn named_constructors_are_what_they_say() {
    let g = labelled(2, 3);
    assert_eq!(drawn(Orientation::IDENTITY, &g), g);
    assert_eq!(drawn(Orientation::flip_horizontal(), &g), mirror_h(&g));
    assert_eq!(drawn(Orientation::flip_vertical(), &g), mirror_v(&g));
    assert_eq!(drawn(Orientation::rotate90(), &g), turn_cw(&g));
    assert_eq!(drawn(Orientation::rotate180(), &g), turn_half(&g));
    assert_eq!(drawn(Orientation::rotate270(), &g), turn_ccw(&g));
    assert_eq!(drawn(Orientation::transpose(), &g), transpose(&g));
    assert_eq!(drawn(Orientation::anti_transpose(), &g), anti_transpose(&g));
    assert_eq!(Orientation::default(), Orientation::IDENTITY);
}

#[test]
fn a_quarter_turn_is_clockwise() {
    // The top left cell of a 2 wide, 3 tall grid goes to the top right of the 3 wide result.
    assert_eq!(Orientation::rotate90().apply(0, 0, 2, 3), (2, 0));
}

#[test]
fn all_sixty_four_compositions_match_doing_one_then_the_other() {
    for (w, h) in [(2, 3), (3, 3), (4, 1)] {
        let g = labelled(w, h);
        for a in all() {
            for b in all() {
                let step_by_step = drawn(b, &drawn(a, &g));
                let composed = a.compose(b);
                assert_eq!(
                    drawn(composed, &g),
                    step_by_step,
                    "{a:?} then {b:?} on {w}x{h}"
                );
            }
        }
    }
}

#[test]
fn compositions_match_the_reference_without_using_apply() {
    for a in all() {
        for b in all() {
            let g = labelled(2, 3);
            let want = reference(b.to_tiled_bits(), &reference(a.to_tiled_bits(), &g));
            let got = reference(a.compose(b).to_tiled_bits(), &g);
            assert_eq!(got, want, "{a:?} then {b:?}");
        }
    }
}

#[test]
fn identity_inverse_and_associativity() {
    for a in all() {
        assert_eq!(a.compose(Orientation::IDENTITY), a);
        assert_eq!(Orientation::IDENTITY.compose(a), a);
        assert_eq!(a.compose(a.inverse()), Orientation::IDENTITY);
        assert_eq!(a.inverse().compose(a), Orientation::IDENTITY);
        assert_eq!(a.inverse().inverse(), a);
        for b in all() {
            for c in all() {
                assert_eq!(a.compose(b).compose(c), a.compose(b.compose(c)));
            }
        }
    }
}

#[test]
fn four_quarter_turns_give_the_start() {
    let q = Orientation::rotate90();
    assert_eq!(q.compose(q).compose(q).compose(q), Orientation::IDENTITY);
    assert_eq!(q.compose(q), Orientation::rotate180());
    assert_eq!(q.inverse(), Orientation::rotate270());
}

#[test]
fn inverse_undoes_apply_on_every_cell() {
    let (w, h) = (3, 5);
    for o in all() {
        let (tw, th) = o.turned_size(w, h);
        for y in 0..h {
            for x in 0..w {
                let (nx, ny) = o.apply(x, y, w, h);
                assert_eq!(o.inverse().apply(nx, ny, tw, th), (x, y), "{o:?}");
            }
        }
    }
}

#[test]
fn only_the_diagonal_ones_swap_the_axes() {
    for o in all() {
        let swaps = o.to_tiled_bits() & FLIPPED_DIAGONALLY != 0;
        assert_eq!(o.swaps_axes(), swaps);
        assert_eq!(o.turned_size(2, 7), if swaps { (7, 2) } else { (2, 7) });
    }
    assert_eq!(all().filter(|o| !o.swaps_axes()).count(), 4);
}

#[test]
fn turned_size_of_a_square_never_changes() {
    for o in all() {
        assert_eq!(o.turned_size(6, 6), (6, 6));
    }
}
