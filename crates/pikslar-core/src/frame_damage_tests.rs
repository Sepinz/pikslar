use crate::document::{ColorMode, Document};
use crate::editor::{Damage, Editor};
use crate::grid::Grid;
use crate::layer::Cel;
use crate::tile::{TILE, TILE_AREA};
use std::collections::HashSet;
use std::ops::ControlFlow;
use std::sync::Arc;

const SIZE: (usize, usize) = (70, 55);
const FRAMES: usize = 4;
const TILE_SIZES: [(usize, usize); 6] = [(8, 8), (16, 16), (5, 3), (17, 17), (3, 7), (1, 1)];

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) % u64::from(n.max(1))) as u32
    }
}

/// `base` with a few values changed, or none.
fn touched(base: &[u32], rng: &mut Rng, value: impl Fn(&mut Rng) -> u32) -> Vec<u32> {
    let mut out = base.to_vec();
    for _ in 0..rng.below(4) {
        let at = rng.below(out.len() as u32) as usize;
        out[at] = value(rng);
    }
    out
}

/// A random cel: `cells` mostly as the previous frame left them, sometimes moved.
fn cel(cells: &[u32], (w, h): (usize, usize), rng: &mut Rng) -> Cel {
    let image = Grid::from_cells(w, h, cells).into_image();
    match rng.below(4) {
        0 => {
            Cel::at(image, rng.below(40) as i32 - 20, rng.below(40) as i32 - 20).expect("in range")
        }
        _ => Cel::from_image(image),
    }
}

fn pixel(rng: &mut Rng) -> u32 {
    match rng.below(3) {
        0 => 0,
        _ => 0xff00_0000 | rng.below(4),
    }
}

/// Paint and map layers whose frames differ a little, held, moved, hidden, and a map whose
/// tileset is missing.
fn random_doc(seed: u64) -> Document {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut doc = Document::new(SIZE.0, SIZE.1, ColorMode::Rgba).expect("a canvas");
    for frame in 1..FRAMES {
        doc.add_frame(frame, false).expect("room");
    }
    let size = TILE_SIZES[rng.below(TILE_SIZES.len() as u32) as usize];
    let set = doc.add_tileset("T", ColorMode::Rgba, size).expect("a set");
    for n in 0..4u32 {
        let px: Vec<u32> = (0..size.0 * size.1)
            .map(|i| 0xff00_0000 | (n * 977 + i as u32))
            .collect();
        let image = Grid::from_cells(size.0, size.1, &px).into_image();
        doc.add_tile(set, Arc::new(image)).expect("a tile");
    }
    for layer in 1..4 {
        doc.add_layer(layer, ColorMode::Rgba).expect("room");
    }
    doc.add_tilemap_layer(4, set).expect("room");
    let mut px: Vec<u32> = (0..SIZE.0 * SIZE.1).map(|_| pixel(&mut rng)).collect();
    let map_size = (SIZE.0.div_ceil(size.0) + 2, SIZE.1.div_ceil(size.1) + 2);
    let map_value =
        |rng: &mut Rng| rng.below(6) | ((rng.below(8) << 29) * u32::from(size.0 == size.1));
    let mut map: Vec<u32> = (0..map_size.0 * map_size.1)
        .map(|_| map_value(&mut rng))
        .collect();
    for frame in 0..FRAMES {
        px = touched(&px, &mut rng, pixel);
        map = touched(&map, &mut rng, map_value);
        for layer in [0, 1, 3] {
            doc.put_cel(layer, frame, cel(&px, SIZE, &mut rng))
                .expect("fits");
        }
        doc.put_cel(4, frame, cel(&map, map_size, &mut rng))
            .expect("fits");
    }
    doc.hold_across(1, 0, 0, FRAMES - 1).expect("a hold");
    doc.layers_mut()[3].visible = false;
    doc
}

fn changed_tiles(doc: &Document, from: usize, to: usize) -> HashSet<usize> {
    let mut marked = HashSet::new();
    doc.frame_change_tiles((from, to), &doc.screen_layers(), |i| {
        marked.insert(i);
        ControlFlow::Continue(())
    });
    marked
}

fn tile_of(
    doc: &Document,
    layer: usize,
    frame: usize,
    (tx, ty): (usize, usize),
) -> [u32; TILE_AREA] {
    let mut out = [0u32; TILE_AREA];
    doc.layer_tile(&doc.layers()[layer], frame, tx, ty, &mut out);
    // A canvas tile at the edge holds pixels past the canvas, which nothing shows.
    for (i, pixel) in out.iter_mut().enumerate() {
        if tx * TILE + i % TILE >= doc.width || ty * TILE + i / TILE >= doc.height {
            *pixel = 0;
        }
    }
    out
}

#[test]
fn every_tile_left_out_looks_the_same_in_both_frames() {
    let (wide, high) = (SIZE.0.div_ceil(TILE), SIZE.1.div_ceil(TILE));
    let mut left_out = 0;
    for seed in 1..=150 {
        let doc = random_doc(seed);
        let shown = doc.screen_layers();
        for (from, to) in [(0, 1), (1, 2), (2, 3), (3, 0), (1, 3)] {
            let marked = changed_tiles(&doc, from, to);
            assert!(
                marked.iter().all(|&i| i < wide * high),
                "seed {seed}: on the canvas"
            );
            for ty in 0..high {
                for tx in 0..wide {
                    if marked.contains(&(ty * wide + tx)) {
                        continue;
                    }
                    left_out += 1;
                    for layer in (0..doc.layers().len()).filter(|&l| shown[l]) {
                        assert!(
                            tile_of(&doc, layer, from, (tx, ty))
                                == tile_of(&doc, layer, to, (tx, ty)),
                            "seed {seed} frames {from}->{to} layer {layer} tile {tx},{ty}"
                        );
                    }
                }
            }
        }
    }
    assert!(left_out > 1000, "the check left tiles out: {left_out}");
}

/// One map layer of 8 by 8 tiles at the origin, its frames alike but for one cell.
fn one_cell_apart() -> Document {
    let mut doc = Document::new(64, 64, ColorMode::Rgba).expect("a canvas");
    doc.add_frame(1, false).expect("room");
    let set = doc
        .add_tileset("T", ColorMode::Rgba, (8, 8))
        .expect("a set");
    let px = vec![0xff11_2233; 64];
    let image = Grid::from_cells(8, 8, &px).into_image();
    doc.add_tile(set, Arc::new(image)).expect("a tile");
    doc.add_tilemap_layer(1, set).expect("room");
    let mut cells = vec![1u32; 64];
    doc.put_cel(1, 0, Cel::new(Grid::from_cells(8, 8, &cells)))
        .expect("fits");
    cells[2 * 8 + 5] = 0;
    doc.put_cel(1, 1, Cel::new(Grid::from_cells(8, 8, &cells)))
        .expect("fits");
    doc
}

#[test]
fn one_changed_cell_marks_only_the_canvas_tile_it_covers() {
    let doc = one_cell_apart();
    // The cell at (5, 2) covers pixels 40..48 by 16..24: canvas tile (2, 1) of 4 wide.
    assert_eq!(changed_tiles(&doc, 0, 1), HashSet::from([4 + 2]));
    assert_eq!(changed_tiles(&doc, 1, 0), HashSet::from([4 + 2]));
    assert!(changed_tiles(&doc, 0, 0).is_empty());
}

#[test]
fn looking_stops_when_told_to() {
    for seed in 1..=20 {
        let doc = random_doc(seed);
        let mut calls = 0;
        doc.frame_change_tiles((0, 1), &doc.screen_layers(), |_| {
            calls += 1;
            ControlFlow::Break(())
        });
        assert!(calls <= 1, "seed {seed}: {calls} calls");
    }
}

#[test]
fn the_editor_damages_the_changed_tiles_or_everything() {
    let mut editor = Editor::new(one_cell_apart());
    editor.take_damage();
    editor.frame = 1;
    editor.damage_frame_change(0);
    assert!(matches!(editor.take_damage(), Damage::Tiles(t) if t == vec![6]));
    editor.damage_frame_change(1);
    assert!(
        matches!(editor.take_damage(), Damage::None),
        "the same frame"
    );
    // A frame past the end is treated as anything at all.
    editor.damage_frame_change(9);
    assert!(matches!(editor.take_damage(), Damage::All));
    // When most of the canvas changes, everything is drawn.
    let mut doc = one_cell_apart();
    doc.put_cel(1, 1, Cel::blank()).expect("fits");
    let mut editor = Editor::new(doc);
    editor.take_damage();
    editor.frame = 1;
    editor.damage_frame_change(0);
    assert!(matches!(editor.take_damage(), Damage::All));
}

#[test]
fn a_cels_area_covers_what_it_draws() {
    let doc = one_cell_apart();
    // The map draws 8 by 8 cells of 8 pixels at the origin; frame 1 leaves one cell empty.
    assert_eq!(doc.cel_area(1, 0), Some((0, 0, 64, 64)));
    assert_eq!(doc.cel_area(0, 0), None, "the blank paint layer");
    assert_eq!(doc.cel_area(9, 0), None, "no such layer");
    for seed in 1..=40 {
        let doc = random_doc(seed);
        let (wide, high) = (SIZE.0.div_ceil(TILE), SIZE.1.div_ceil(TILE));
        for layer in 0..doc.layers().len() {
            let area = doc.cel_area(layer, 2);
            for ty in 0..high {
                for tx in 0..wide {
                    let pixels = tile_of(&doc, layer, 2, (tx, ty));
                    if pixels.iter().all(|&p| p == 0) {
                        continue;
                    }
                    let (x, y, w, h) = area.expect("something drawn has an area");
                    let (px, py) = ((tx * TILE) as i32, (ty * TILE) as i32);
                    let side = TILE as i32;
                    let meets = px < x + w && x < px + side && py < y + h && y < py + side;
                    assert!(meets, "seed {seed} layer {layer} tile {tx},{ty}");
                }
            }
        }
    }
}
