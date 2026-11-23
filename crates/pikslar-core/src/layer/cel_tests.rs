use super::*;
use crate::grid::Cell;
use crate::tile::{Tile, TILE_AREA};
use std::collections::HashMap;

/// A small deterministic generator, so a failure names the seed that made it.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn between(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u64) as i32
    }
}

/// An image whose table starts at a random local tile, with tiles of every encoding and gaps,
/// and the same pixels written out by hand in local coordinates.
fn random_image(rng: &mut Rng) -> (Image, HashMap<(i32, i32), u32>) {
    let (x0, y0) = (rng.between(-6, 4) as i16, rng.between(-6, 4) as i16);
    let (tw, th) = (rng.between(1, 4) as u16, rng.between(1, 4) as usize);
    let mut cells: Vec<Cell> = Vec::new();
    let mut pixels = HashMap::new();
    for row in 0..th as i32 {
        for column in 0..tw as i32 {
            let mut raw = [0u32; TILE_AREA];
            let kind = rng.below(5);
            for (i, p) in raw.iter_mut().enumerate() {
                *p = match kind {
                    0 => 0,
                    1 => 7,
                    2 => (rng.below(3) as u32) * 5,
                    3 => rng.below(40) as u32,
                    _ => (i as u32 + 1) * 3 + rng.below(2) as u32,
                };
            }
            let (tx, ty) = (x0 as i32 + column, y0 as i32 + row);
            for (i, &p) in raw.iter().enumerate() {
                if p != 0 {
                    let (x, y) = (i as i32 % 16, i as i32 / 16);
                    pixels.insert((tx * 16 + x, ty * 16 + y), p);
                }
            }
            cells.push(Tile::pack(&raw).map(Arc::new));
        }
    }
    let image = Image::from_tiles(x0, y0, tw, cells).expect("within reach");
    (image, pixels)
}

fn oracle(pixels: &HashMap<(i32, i32), u32>, (cx, cy): (i32, i32), x: i32, y: i32) -> u32 {
    pixels.get(&(x - cx, y - cy)).copied().unwrap_or(0)
}

fn random_offset(rng: &mut Rng) -> (i32, i32) {
    match rng.below(4) {
        0 => (rng.between(-3, 3) * 16, rng.between(-3, 3) * 16),
        1 => (rng.between(-40, 40), rng.between(-40, 40)),
        2 => (rng.between(-15, 15), rng.between(-15, 15)),
        _ => (0, 0),
    }
}

#[test]
fn every_read_agrees_with_moving_the_pixels_by_hand() {
    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (image, pixels) = random_image(&mut rng);
        let offset = random_offset(&mut rng);
        let cel = Cel::at(image, offset.0, offset.1).expect("in range");
        let (w, h) = (rng.between(1, 70) as usize, rng.between(1, 70) as usize);
        let mut reader = cel.reader();
        let mut raw = [0u32; TILE_AREA];
        for ty in 0..h.div_ceil(16) {
            for tx in 0..w.div_ceil(16) {
                cel.decode(tx, ty, &mut raw);
                let mut any = false;
                for (i, &got) in raw.iter().enumerate() {
                    let (x, y) = ((tx * 16 + i % 16) as i32, (ty * 16 + i / 16) as i32);
                    let want = oracle(&pixels, offset, x, y);
                    assert_eq!(got, want, "seed {seed} decode ({x}, {y}) at {offset:?}");
                    any |= want != 0;
                }
                assert!(!any || cel.has_tile(tx, ty), "seed {seed} has_tile");
            }
        }
        for y in -20..h as i32 + 20 {
            for x in -20..w as i32 + 20 {
                let want = oracle(&pixels, offset, x, y);
                assert_eq!(cel.get(x, y), want, "seed {seed} get ({x}, {y})");
                assert_eq!(reader.get(x, y), want, "seed {seed} reader ({x}, {y})");
            }
        }
        let (wx, wy) = (rng.between(-30, 30), rng.between(-30, 30));
        let window = cel.window(wx, wy, w, h);
        for (i, &got) in window.iter().enumerate() {
            let (x, y) = (wx + (i % w) as i32, wy + (i / w) as i32);
            assert_eq!(got, oracle(&pixels, offset, x, y), "seed {seed} window");
        }
        let canvas = cel.to_cells(w, h);
        assert_eq!(canvas, cel.window(0, 0, w, h), "seed {seed} to_cells");
    }
}

#[test]
fn a_tile_with_nothing_under_it_is_empty() {
    let (image, _) = random_image(&mut Rng(77));
    let cel = Cel::at(image, 5000, 5000).expect("in range");
    assert!(!cel.has_tile(0, 0));
    let mut raw = [9u32; TILE_AREA];
    cel.decode(0, 0, &mut raw);
    assert!(raw.iter().all(|&p| p == 0));
}

#[test]
fn positions_past_the_limit_are_refused_and_the_limit_itself_is_kept() {
    let limit = OFFSET_LIMIT;
    assert!(Cel::at(Image::blank(), limit, -limit).is_some());
    assert!(Cel::at(Image::blank(), limit + 1, 0).is_none());
    assert!(Cel::at(Image::blank(), 0, -limit - 1).is_none());
    let cel = Cel::at(Image::blank(), -limit, limit).expect("in range");
    assert_eq!(cel.position(), (-limit as i16, limit as i16));
    assert!(Cel::at(Image::blank(), i32::MIN, 0).is_none());
}

#[test]
fn every_canvas_pixel_has_a_storable_tile_at_the_limits() {
    let largest = crate::document::MAX_DIMENSION as i32;
    for offset in [-OFFSET_LIMIT, OFFSET_LIMIT] {
        for x in [0, largest - 1] {
            let tile = (x - offset) >> TILE_SHIFT;
            assert!((-crate::image::MAX_TILE_REACH..crate::image::MAX_TILE_REACH).contains(&tile));
        }
    }
}

#[test]
fn reads_far_off_the_canvas_do_not_overflow() {
    let (image, _) = random_image(&mut Rng(5));
    let cel = Cel::at(image, -OFFSET_LIMIT, OFFSET_LIMIT).expect("in range");
    assert_eq!(cel.get(i32::MAX / 2, i32::MIN / 2), 0);
    let far = cel.window(i32::MAX - 10, i32::MIN + 10, 4, 4);
    assert!(far.iter().all(|&p| p == 0));
}

#[test]
fn only_a_cel_at_the_origin_fits_a_grid() {
    let cel = Cel::new(Grid::from_cells(4, 4, &[1; 16]));
    assert!(cel.fits_grid(4, 4));
    assert!(!Cel {
        x: 1,
        ..cel.clone()
    }
    .fits_grid(4, 4));
}

#[test]
fn looking_the_same_needs_the_same_place_too() {
    let cel = Cel::new(Grid::from_cells(4, 4, &[1; 16]));
    let moved = Cel {
        y: 3,
        ..cel.clone()
    };
    assert!(moved.same_image(&cel) && !moved.looks_the_same(&cel));
    assert!(cel.clone().looks_the_same(&cel));
}
