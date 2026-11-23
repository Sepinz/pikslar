//! `Tile::map_values` gives exactly what decoding, moving every value and packing again gives,
//! including whether anything moved, for tiles of every encoding and maps of every kind.

use crate::tile::{RawTile, Tile, TILE_AREA};

/// The slow way: every cell decoded, moved (0 stays 0, a value past the map stays), packed.
fn reference(tile: &Tile, map: &[u32]) -> Option<Option<Tile>> {
    let mut raw: RawTile = [0; TILE_AREA];
    tile.decode(&mut raw);
    let mut moved = false;
    for px in raw.iter_mut().filter(|px| **px != 0) {
        if let Some(&to) = map.get(*px as usize).filter(|&&to| to != *px) {
            *px = to;
            moved = true;
        }
    }
    moved.then(|| Tile::pack(&raw))
}

struct Dice(u64);

impl Dice {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

/// A tile of up to `colors` values (0 among them), in blocks or noise.
fn tile(dice: &mut Dice, colors: usize) -> Option<Tile> {
    let values: Vec<u32> = (0..colors).map(|_| dice.below(40) as u32).collect();
    let blocky = dice.below(2) == 0;
    let mut raw: RawTile = [0; TILE_AREA];
    for (i, px) in raw.iter_mut().enumerate() {
        let pick = if blocky {
            (i / 37) % colors
        } else {
            dice.below(colors)
        };
        *px = values[pick];
    }
    Tile::pack(&raw)
}

/// A map over ids 0 to 39 of one of the remaps' kinds: a permutation, merges, removals to 0,
/// or a single replacement. Some maps are shorter than the ids, which leave the rest alone.
fn map(dice: &mut Dice) -> Vec<u32> {
    let len = 20 + dice.below(21);
    let mut to: Vec<u32> = (0..len as u32).collect();
    match dice.below(4) {
        0 => {
            for i in (2..len).rev() {
                to.swap(i, 1 + dice.below(i - 1));
            }
        }
        1 => to.iter_mut().skip(1).for_each(|v| *v = 1 + *v / 3),
        2 => to.iter_mut().skip(1).step_by(3).for_each(|v| *v = 0),
        _ => {
            let from = 1 + dice.below(len - 1);
            to[from] = dice.below(len) as u32;
        }
    }
    to
}

#[test]
fn mapping_values_equals_decoding_moving_and_packing() {
    let mut dice = Dice(0x5EED_1234_ABCD_0001);
    for case in 0..20_000 {
        let colors = [1, 2, 3, 4, 5, 9, 16, 17, 40][dice.below(9)];
        let Some(t) = tile(&mut dice, colors) else {
            continue;
        };
        let m = map(&mut dice);
        assert_eq!(
            t.map_values(&m),
            reference(&t, &m),
            "case {case}: {t:?} {m:?}"
        );
    }
}

#[test]
fn a_packed_tile_not_as_pack_makes_it_comes_out_as_pack_makes_it() {
    // Index 2 is never used, and the order is not that of first appearance.
    let mut data = vec![0u8; TILE_AREA / 4];
    data.iter_mut().step_by(2).for_each(|b| *b = 0b01_01_01_01);
    let odd = Tile::Packed {
        palette: vec![5, 6, 7].into_boxed_slice(),
        bits: 2,
        data: data.into_boxed_slice(),
    };
    let swap = [0, 1, 2, 3, 4, 6, 5, 9];
    assert_eq!(odd.map_values(&swap), reference(&odd, &swap));
    assert!(odd.map_values(&swap).is_some());
}

#[test]
fn nothing_moving_is_none_and_everything_cleared_is_no_tile() {
    let raw: RawTile = std::array::from_fn(|i| 1 + (i % 3) as u32);
    let t = Tile::pack(&raw).expect("a tile");
    assert_eq!(t.map_values(&[0, 1, 2, 3]), None);
    assert_eq!(t.map_values(&[0, 0, 0, 0]), Some(None));
    assert_eq!(
        Tile::Solid(2).map_values(&[0, 1, 5]),
        Some(Some(Tile::Solid(5)))
    );
    assert_eq!(
        Tile::Solid(2).map_values(&[0, 1]),
        None,
        "past the map stays"
    );
}

#[test]
fn shows_agrees_with_decoding_for_every_value() {
    let mut dice = Dice(0x0BAD_CAFE_0000_0007);
    let mut odd_data = vec![0u8; TILE_AREA / 4];
    odd_data
        .iter_mut()
        .step_by(2)
        .for_each(|b| *b = 0b01_01_01_01);
    let odd = Tile::Packed {
        palette: vec![5, 6, 7].into_boxed_slice(),
        bits: 2,
        data: odd_data.into_boxed_slice(),
    };
    let mut tiles = vec![odd, Tile::Solid(3)];
    for _ in 0..3_000 {
        let colors = [1, 2, 3, 5, 16, 17, 40][dice.below(7)];
        tiles.extend(tile(&mut dice, colors));
    }
    let mut raw: RawTile = [0; TILE_AREA];
    for t in &tiles {
        t.decode(&mut raw);
        for value in 0..41 {
            assert_eq!(t.shows(value), raw.contains(&value), "{t:?} {value}");
        }
    }
}
