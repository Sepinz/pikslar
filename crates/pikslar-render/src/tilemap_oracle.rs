//! A brute-force picture of a tilemap document for the render tests, worked out from the
//! definition of a cell and sharing no code with the sampler.
//!
//! [`Scene`] holds a random document with paint and tilemap layers, and `truth`, the same
//! document with every tilemap layer replaced by a paint layer holding the pixels the oracle
//! says its cells draw. The compositor, preview and export are already trusted on paint layers,
//! so a tilemap document must come out exactly as its `truth` does.

use pikslar_core::tileset::TilesetId;
use pikslar_core::{rgba, BlendMode, Cel, ColorMode, Document, Grid, Layer, LayerKind, TILE};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next() >> 11) % u64::from(n.max(1))) as u32
    }

    pub fn chance(&mut self, in_n: u32) -> bool {
        self.below(in_n) == 0
    }
}

const SIZES: [(usize, usize); 9] = [
    (8, 8),
    (16, 16),
    (4, 4),
    (1, 1),
    (5, 3),
    (3, 7),
    (17, 17),
    (12, 20),
    (2, 2),
];

/// One tileset as the oracle knows it.
pub struct TilesetSpec {
    pub id: TilesetId,
    pub mode: ColorMode,
    pub size: (usize, usize),
    /// Pixels of tile `n` (id `n + 1`).
    pub tiles: Vec<Vec<u32>>,
}

/// What one frame of a map layer holds.
#[derive(Clone)]
pub struct MapCelSpec {
    pub cells: HashMap<(i32, i32), u32>,
    pub offset: (i32, i32),
    /// Frames of one hold share a key, as they share a drawing.
    pub key: usize,
}

pub struct MapLayerSpec {
    /// Position of the layer in the document, from the bottom.
    pub at: usize,
    pub tileset: usize,
    pub frames: Vec<MapCelSpec>,
}

pub struct Scene {
    pub doc: Document,
    pub truth: Document,
    pub tilesets: Vec<TilesetSpec>,
    pub maps: Vec<MapLayerSpec>,
}

/// The tile's pixels placed as Tiled says: the diagonal flip first, then horizontal, then
/// vertical, from each source pixel to where it lands.
fn place(source: &[u32], (w, h): (usize, usize), bits: u32) -> Vec<u32> {
    let (diagonal, horizontal, vertical) = (
        bits & 0x2000_0000 != 0,
        bits & 0x8000_0000 != 0,
        bits & 0x4000_0000 != 0,
    );
    let mut placed = vec![0u32; w * h];
    for sy in 0..h {
        for sx in 0..w {
            let (mut x, mut y, mut width, mut height) = (sx, sy, w, h);
            if diagonal {
                (x, y, width, height) = (y, x, height, width);
            }
            if horizontal {
                x = width - 1 - x;
            }
            if vertical {
                y = height - 1 - y;
            }
            placed[y * width + x] = source[sy * w + sx];
        }
    }
    placed
}

/// The pixel a map cel draws at canvas (`x`, `y`).
fn map_pixel(cel: &MapCelSpec, set: &TilesetSpec, x: i32, y: i32) -> u32 {
    let (w, h) = (set.size.0 as i32, set.size.1 as i32);
    let (lx, ly) = (x - cel.offset.0, y - cel.offset.1);
    let bits = cel
        .cells
        .get(&(lx.div_euclid(w), ly.div_euclid(h)))
        .copied()
        .unwrap_or(0);
    let id = (bits & 0x0fff_ffff) as usize;
    if id == 0 || id > set.tiles.len() {
        return 0;
    }
    let placed = place(&set.tiles[id - 1], set.size, bits);
    placed[ly.rem_euclid(h) as usize * set.size.0 + lx.rem_euclid(w) as usize]
}

impl MapCelSpec {
    /// True if this map places one of `ids` over canvas pixel (`x`, `y`).
    pub fn places_at(&self, set: &TilesetSpec, x: i32, y: i32, ids: &[u32]) -> bool {
        let (w, h) = (set.size.0 as i32, set.size.1 as i32);
        let at = (
            (x - self.offset.0).div_euclid(w),
            (y - self.offset.1).div_euclid(h),
        );
        let id = self.cells.get(&at).copied().unwrap_or(0) & 0x0fff_ffff;
        id != 0 && ids.contains(&id)
    }

    fn cel(&self) -> Cel {
        let (cols, rows) = (
            self.cells.keys().map(|k| k.0).max(),
            self.cells.keys().map(|k| k.1).max(),
        );
        let (cols, rows) = (
            cols.map_or(1, |c| c as usize + 1),
            rows.map_or(1, |r| r as usize + 1),
        );
        let mut cells = vec![0u32; cols * rows];
        for (&(cx, cy), &bits) in &self.cells {
            cells[cy as usize * cols + cx as usize] = bits;
        }
        let image = Grid::from_cells(cols, rows, &cells).into_image();
        Cel::at(image, self.offset.0, self.offset.1).expect("in reach")
    }
}

fn random_pixel(rng: &mut Rng, mode: ColorMode, palette: &[u32]) -> u32 {
    match mode {
        ColorMode::Palette => palette[rng.below(palette.len() as u32) as usize],
        ColorMode::Rgba => {
            let alpha = [255, 255, 255, 128, 40, 0][rng.below(6) as usize];
            rgba(
                rng.next() as u8,
                (rng.next() >> 9) as u8,
                (rng.next() >> 17) as u8,
                alpha,
            )
        }
    }
}

fn random_tileset(rng: &mut Rng, doc: &mut Document, palette: &[u32]) -> TilesetSpec {
    let size = SIZES[rng.below(SIZES.len() as u32) as usize];
    let mode = if rng.chance(2) {
        ColorMode::Rgba
    } else {
        ColorMode::Palette
    };
    let id = doc.add_tileset("T", mode, size).expect("a tileset");
    let mut tiles = Vec::new();
    for _ in 0..1 + rng.below(6) {
        let blank = rng.chance(6);
        let pixels: Vec<u32> = (0..size.0 * size.1)
            .map(|_| {
                if blank || rng.chance(4) {
                    0
                } else {
                    random_pixel(rng, mode, palette)
                }
            })
            .collect();
        let image = Grid::from_cells(size.0, size.1, &pixels).into_image();
        doc.add_tile(id, Arc::new(image)).expect("a tile");
        tiles.push(pixels);
    }
    TilesetSpec {
        id,
        mode,
        size,
        tiles,
    }
}

fn random_cells(rng: &mut Rng, set: &TilesetSpec) -> MapCelSpec {
    let (cols, rows) = (1 + rng.below(40) as i32, 1 + rng.below(40) as i32);
    let mut cells = HashMap::new();
    for cy in 0..rows {
        for cx in 0..cols {
            if rng.chance(3) {
                continue;
            }
            let id = 1 + rng.below(set.tiles.len() as u32);
            let mut turn = rng.below(8);
            if set.size.0 != set.size.1 {
                turn &= 0b110;
            }
            let pinned = if rng.chance(5) { 1 << 28 } else { 0 };
            cells.insert((cx, cy), id | turn << 29 | pinned);
        }
    }
    let offset = (rng.below(70) as i32 - 35, rng.below(70) as i32 - 35);
    MapCelSpec {
        cells,
        offset,
        key: 0,
    }
}

fn random_paint_cel(
    rng: &mut Rng,
    mode: ColorMode,
    palette: &[u32],
    (w, h): (usize, usize),
) -> Cel {
    let mut px = vec![0u32; w * h];
    for p in px.iter_mut() {
        if rng.chance(5) {
            *p = random_pixel(rng, mode, palette);
        }
    }
    Cel::new(Grid::from_cells(w, h, &px))
}

/// What a layer is, apart from its drawings.
fn style(rng: &mut Rng, layer: &mut Layer) {
    layer.opacity = [255, 255, 255, 200, 90][rng.below(5) as usize];
    layer.blend = if rng.chance(2) {
        BlendMode::Normal
    } else {
        BlendMode::ALL[rng.below(9) as usize]
    };
    layer.visible = !rng.chance(6);
}

const PALETTE_COLORS: [u32; 5] = [
    rgba(200, 30, 30, 255),
    rgba(30, 200, 30, 255),
    rgba(30, 30, 200, 160),
    rgba(240, 240, 20, 255),
    rgba(10, 10, 10, 90),
];

/// Wraps a run of layers below the top one in a group, in both documents, which may be hidden.
/// Returns the run's last layer: the group sits above it, and layers above that move up one.
fn add_group(rng: &mut Rng, docs: [&mut Document; 2], count: usize) -> usize {
    let first = rng.below(count as u32 - 1) as usize;
    let last = first + rng.below((count - 1 - first) as u32) as usize;
    let visible = !rng.chance(4);
    for doc in docs {
        for member in &mut doc.layers_mut()[first..=last] {
            member.depth = 1;
        }
        doc.add_layer(last + 1, ColorMode::Rgba).expect("room");
        let group = &mut doc.layers_mut()[last + 1];
        group.kind = LayerKind::Group;
        group.depth = 0;
        group.visible = visible;
    }
    last
}

/// A random scene from `seed`. Layers, frames, offsets, tile sizes, opacity, blend modes,
/// visibility, holds and a group all vary.
pub fn random_scene(seed: u64) -> Scene {
    random_tall_scene(seed, 0)
}

/// [`random_scene`] with `extra` more pixel rows, for tests that need many tile rows.
pub fn random_tall_scene(seed: u64, extra: usize) -> Scene {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let (w, h) = (
        18 + rng.below(50) as usize,
        18 + rng.below(40) as usize + extra,
    );
    let frames = 1 + rng.below(4) as usize;
    let mut doc = Document::new(w, h, ColorMode::Rgba).expect("a canvas");
    for color in PALETTE_COLORS {
        Arc::make_mut(&mut doc.colors).id_for(color).expect("room");
    }
    let palette: Vec<u32> = (1..=PALETTE_COLORS.len() as u32).collect();
    for at in 1..frames {
        doc.add_frame(at, false).expect("room");
    }
    let count = 2 + rng.below(4) as usize;
    let mut tilesets: Vec<TilesetSpec> = Vec::new();
    let mut maps: Vec<MapLayerSpec> = Vec::new();
    let mut paints: Vec<(usize, ColorMode)> = vec![(0, ColorMode::Rgba)];
    for at in 1..count {
        if rng.chance(2) {
            if tilesets.is_empty() || rng.chance(2) {
                tilesets.push(random_tileset(&mut rng, &mut doc, &palette));
            }
            let tileset = rng.below(tilesets.len() as u32) as usize;
            doc.add_tilemap_layer(at, tilesets[tileset].id)
                .expect("room");
            maps.push(MapLayerSpec {
                at,
                tileset,
                frames: Vec::new(),
            });
        } else {
            let mode = if rng.chance(2) {
                ColorMode::Rgba
            } else {
                ColorMode::Palette
            };
            doc.add_layer(at, mode).expect("room");
            paints.push((at, mode));
        }
    }
    let mut truth = doc.clone();
    truth.tilesets = Default::default();
    for map in &maps {
        truth.layers_mut()[map.at].kind = LayerKind::Paint;
    }
    fill_layers(
        &mut rng,
        (&mut doc, &mut truth),
        (&paints, &mut maps, &tilesets),
        &palette,
        frames,
    );
    if count > 2 && rng.chance(2) {
        let last = add_group(&mut rng, [&mut doc, &mut truth], count);
        for map in maps.iter_mut().filter(|m| m.at > last) {
            map.at += 1;
        }
    }
    Scene {
        doc,
        truth,
        tilesets,
        maps,
    }
}

/// Gives every layer its frames, and gives both documents the same style.
fn fill_layers(
    rng: &mut Rng,
    (doc, truth): (&mut Document, &mut Document),
    (paints, maps, tilesets): (&[(usize, ColorMode)], &mut [MapLayerSpec], &[TilesetSpec]),
    palette: &[u32],
    frames: usize,
) {
    let (w, h) = (doc.width, doc.height);
    for &(at, mode) in paints {
        let mut cels: Vec<Cel> = Vec::new();
        for f in 0..frames {
            let held = f > 0 && rng.chance(4);
            cels.push(if held {
                cels[f - 1].clone()
            } else {
                random_paint_cel(rng, mode, palette, (w, h))
            });
        }
        for (f, cel) in cels.into_iter().enumerate() {
            doc.put_cel(at, f, cel.clone()).expect("a cel");
            truth.put_cel(at, f, cel).expect("a cel");
        }
    }
    for map in maps.iter_mut() {
        let set = &tilesets[map.tileset];
        let mut truth_cels: HashMap<usize, Cel> = HashMap::new();
        for f in 0..frames {
            let spec = if f > 0 && rng.chance(4) {
                map.frames[f - 1].clone()
            } else {
                MapCelSpec {
                    key: f,
                    ..random_cells(rng, set)
                }
            };
            let cel = spec.cel();
            let held = f > 0 && spec.key == map.frames[f - 1].key;
            let cel = if held {
                doc.layers()[map.at].cel(f - 1).clone()
            } else {
                cel
            };
            doc.put_cel(map.at, f, cel).expect("a cel");
            let image = truth_cels
                .entry(spec.key)
                .or_insert_with(|| truth_cel(&spec, set, (w, h)))
                .clone();
            truth.put_cel(map.at, f, image).expect("a cel");
            map.frames.push(spec);
        }
    }
    for at in 0..doc.layers().len() {
        let mut layer = doc.layers()[at].clone();
        style(rng, &mut layer);
        for target in [&mut *doc, &mut *truth] {
            let l = &mut target.layers_mut()[at];
            (l.opacity, l.blend, l.visible) = (layer.opacity, layer.blend, layer.visible);
        }
    }
}

/// The map's pixels over the canvas, drawn as a paint cel.
fn truth_cel(spec: &MapCelSpec, set: &TilesetSpec, (w, h): (usize, usize)) -> Cel {
    let mut px = vec![0u32; w * h];
    for y in 0..h {
        for x in 0..w {
            px[y * w + x] = map_pixel(spec, set, x as i32, y as i32);
        }
    }
    Cel::new(Grid::from_cells(w, h, &px))
}

/// The canvas tiles, counted along rows, where `spec` places one of `ids`.
pub fn placed_tiles(
    spec: &MapCelSpec,
    set: &TilesetSpec,
    ids: &[u32],
    (w, h): (usize, usize),
) -> Vec<usize> {
    let across = w.div_ceil(TILE);
    let mut found = vec![false; across * h.div_ceil(TILE)];
    for y in 0..h {
        for x in 0..w {
            if spec.places_at(set, x as i32, y as i32, ids) {
                found[y / TILE * across + x / TILE] = true;
            }
        }
    }
    (0..found.len()).filter(|&i| found[i]).collect()
}
