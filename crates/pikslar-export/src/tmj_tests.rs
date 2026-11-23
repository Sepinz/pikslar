//! The TMJ export: GIDs and flags, offsets, groups, refusals, atlases, and a reconstruction
//! from the map and its atlases that must equal the PNG export of the same layers.

use crate::atlas::{write_atlas_png, AtlasLayout};
use crate::tmj::{tmj_map, TmjMap, TmjNode};
use crate::tmj_json::map_text;
use crate::{frame_png, ExportError};
use pikslar_core::orient::Orientation;
use pikslar_core::tilemap::{MapCell, PINNED};
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{rgba, BlendMode, Cel, ColorMode, Document, Grid, LayerKind};
use pikslar_format::png;
use std::path::Path;
use std::sync::Arc;

const MAP: &str = "out/map.tmj";

fn id(n: u32) -> TileId {
    TileId::new(n).expect("a tile id")
}

/// A tile of `w` by `h` whose every pixel differs, from `seed`, opaque or clear.
fn tile(w: usize, h: usize, seed: u32) -> Arc<pikslar_core::Image> {
    let px: Vec<u32> = (0..(w * h) as u32)
        .map(|i| match (i * 7 + seed) % 11 {
            0 => 0,
            v => rgba((seed * 40 + i) as u8, (v * 23) as u8, (i * 5) as u8, 255),
        })
        .collect();
    Arc::new(Grid::from_cells(w, h, &px).into_image())
}

/// A document of `w` by `h` with a tileset of `count` tiles of `side`, and no tilemap layer.
fn with_tileset(w: usize, h: usize, side: (usize, usize), count: u32) -> (Document, TilesetId) {
    let mut doc = Document::new(w, h, ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Walls", ColorMode::Rgba, side)
        .expect("room");
    for n in 0..count {
        doc.add_tile(set, tile(side.0, side.1, n + 1))
            .expect("fits");
    }
    (doc, set)
}

/// Adds a tilemap layer of `set` at the top, its frame 0 placing `cells` (`cw` a row) at `at`.
fn add_map(doc: &mut Document, set: TilesetId, cw: usize, cells: &[u32], at: (i32, i32)) -> usize {
    let index = doc.layers().len();
    doc.add_tilemap_layer(index, set).expect("room");
    let cel = Cel::new(Grid::from_cells(cw, cells.len() / cw, cells));
    let cel = cel.moved_to(at.0, at.1).expect("in range");
    doc.put_cel(index, 0, cel).expect("fits");
    index
}

fn place(n: u32, o: Orientation) -> u32 {
    MapCell::place(id(n), o).bits()
}

fn tiles(map: &TmjMap) -> Vec<&crate::tmj::TmjLayer> {
    fn walk<'a>(nodes: &'a [TmjNode], out: &mut Vec<&'a crate::tmj::TmjLayer>) {
        for node in nodes {
            match node {
                TmjNode::Tiles(l) => out.push(l),
                TmjNode::Group { layers, .. } => walk(layers, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(&map.layers, &mut out);
    out
}

#[test]
fn every_orientation_keeps_tileds_three_bits_and_never_the_pinned_one() {
    let (mut doc, set) = with_tileset(32, 8, (4, 4), 3);
    let cells: Vec<u32> = Orientation::ALL
        .iter()
        .map(|&o| place(3, o) | if o.index() % 2 == 0 { PINNED } else { 0 })
        .collect();
    add_map(&mut doc, set, 8, &cells, (0, 0));
    let map = tmj_map(&doc, 0, Path::new(MAP)).expect("a map");
    let data = &tiles(&map)[0].data;
    for (i, o) in Orientation::ALL.iter().enumerate() {
        assert_eq!(data[i], 3 | o.to_tiled_bits(), "{i}");
        assert_eq!(data[i] & PINNED, 0);
    }
    assert_eq!(data[8..], [0; 8], "the second row is empty");
}

#[test]
fn gids_follow_panel_order_and_each_tileset_starts_after_the_last() {
    let (mut doc, a) = with_tileset(8, 8, (4, 4), 3);
    let b = doc.add_tileset("B", ColorMode::Rgba, (4, 4)).expect("room");
    let empty = doc.add_tileset("C", ColorMode::Rgba, (4, 4)).expect("room");
    let b1 = doc.add_tile(b, tile(4, 4, 9)).expect("fits");
    let b2 = doc.add_tile(b, tile(4, 4, 10)).expect("fits");
    add_map(
        &mut doc,
        a,
        2,
        &[place(3, Orientation::IDENTITY), 0],
        (0, 0),
    );
    let cells = [b2, b1].map(|t| MapCell::place(t, Orientation::IDENTITY).bits());
    add_map(&mut doc, b, 2, &cells, (0, 4));
    add_map(&mut doc, empty, 1, &[0], (0, 0));
    let map = tmj_map(&doc, 0, Path::new(MAP)).expect("a map");
    let gids: Vec<_> = map
        .tilesets
        .iter()
        .map(|t| (t.name.as_str(), t.first_gid))
        .collect();
    assert_eq!(
        gids,
        [("Walls", 1), ("B", 4)],
        "a tileset with no tiles is left out"
    );
    let layers = tiles(&map);
    assert_eq!(layers[0].data, [3, 0, 0, 0]);
    assert_eq!(layers[1].data, [0, 0, 5, 4]);
    assert_eq!(layers[2].data, [0; 4]);
    assert_eq!(map.tilesets[1].image, "map-tileset2.png");
}

#[test]
fn whole_tile_offsets_move_cells_and_the_rest_is_a_negative_pixel_offset() {
    let (mut doc, set) = with_tileset(16, 12, (4, 4), 2);
    let one = place(1, Orientation::IDENTITY);
    add_map(&mut doc, set, 1, &[one], (8, 4));
    add_map(&mut doc, set, 1, &[one], (5, -3));
    add_map(&mut doc, set, 1, &[one], (-4, 0));
    let map = tmj_map(&doc, 0, Path::new(MAP)).expect("a map");
    assert_eq!(
        (map.width, map.height),
        (5, 4),
        "a column and row more for the rests"
    );
    let layers = tiles(&map);
    assert_eq!(layers[0].offset, (0, 0));
    assert_eq!(layers[0].data[5 + 2], 1, "two columns right, one row down");
    assert_eq!(layers[1].offset, (-3, -3));
    assert_eq!(layers[1].data[2], 1, "5 = 2 * 4 - 3, and -3 = 0 * 4 - 3");
    assert!(
        layers[2].data.iter().all(|&g| g == 0),
        "the cell left of the canvas"
    );
    assert!(map.notes.iter().any(|n| n.starts_with("1 placed cell(s)")));
    let text = map_text(&map);
    assert!(text.contains("\"offsetx\":-3"), "{text}");
    assert_eq!(
        text.matches("\"offsetx\"").count(),
        1,
        "only where not whole"
    );
}

#[test]
fn a_canvas_that_is_not_whole_tiles_gets_a_last_part_tile() {
    let (mut doc, set) = with_tileset(10, 9, (4, 4), 1);
    add_map(&mut doc, set, 1, &[0], (0, 0));
    let map = tmj_map(&doc, 0, Path::new(MAP)).expect("a map");
    assert_eq!((map.width, map.height, map.tile), (3, 3, (4, 4)));
}

#[test]
fn groups_become_group_layers_and_empty_ones_are_left_out() {
    let doc = grouped();
    let map = tmj_map(&doc, 0, Path::new(MAP)).expect("a map");
    assert_eq!(map.layers.len(), 2);
    let TmjNode::Group {
        name,
        visible,
        layers,
    } = &map.layers[0]
    else {
        panic!("the outer group first");
    };
    assert_eq!((name.as_str(), *visible), ("Outer", false));
    let TmjNode::Group {
        layers: inner,
        visible,
        ..
    } = &layers[0]
    else {
        panic!("the inner group");
    };
    assert!(*visible);
    assert!(matches!(&inner[..], [TmjNode::Tiles(l)] if l.index == 1));
    assert!(matches!(&map.layers[1], TmjNode::Tiles(l) if l.index == 6 && !l.visible));
    let text = map_text(&map);
    assert!(text.contains("\"name\":\"Inner \\\"quoted\\\"\""), "{text}");
    assert!(!text.contains("Paint only"));
    assert!(text.contains("\"nextlayerid\":5"), "{text}");
    for n in 1..=4 {
        assert!(text.contains(&format!("\"id\":{n},")), "{n}");
    }
    assert!(
        map.notes[0].contains("2 paint layer(s) and 0 reference"),
        "{:?}",
        map.notes
    );
}

/// Bottom to top: paint 0, map 1 (in group 2, in group 3), group 2, group 3 (hidden), paint 4
/// in group 5, group 5, map 6 (hidden).
fn grouped() -> Document {
    let (mut doc, set) = with_tileset(8, 8, (4, 4), 1);
    let one = place(1, Orientation::IDENTITY);
    add_map(&mut doc, set, 1, &[one], (0, 0));
    for _ in 0..2 {
        let at = doc.layers().len();
        doc.add_layer(at, ColorMode::Rgba).expect("room");
    }
    doc.add_layer(doc.layers().len(), ColorMode::Rgba)
        .expect("room");
    doc.add_layer(doc.layers().len(), ColorMode::Rgba)
        .expect("room");
    add_map(&mut doc, set, 1, &[one], (4, 4));
    {
        let layers = doc.layers_mut();
        layers[1].depth = 2;
        layers[2].kind = LayerKind::Group;
        layers[2].depth = 1;
        layers[2].name = "Inner \"quoted\"".into();
        layers[3].kind = LayerKind::Group;
        layers[3].visible = false;
        layers[3].name = "Outer".into();
        layers[4].depth = 1;
        layers[5].kind = LayerKind::Group;
        layers[5].name = "Paint only".into();
        layers[6].visible = false;
    }
    doc.check_groups().expect("a sound list");
    doc
}

#[test]
fn names_are_escaped_and_opacity_and_modes_written_as_tiled_reads_them() {
    let (mut doc, set) = with_tileset(4, 4, (4, 4), 1);
    let index = add_map(&mut doc, set, 1, &[0], (0, 0));
    let other = add_map(&mut doc, set, 1, &[0], (0, 0));
    {
        let layers = doc.layers_mut();
        layers[index].name = "a\"b\\c\n\u{1}é漢".into();
        layers[index].opacity = 128;
        layers[index].blend = BlendMode::Multiply;
        layers[other].blend = BlendMode::Addition;
        layers[0].kind = LayerKind::Reference;
    }
    let map = tmj_map(&doc, 0, Path::new("out/m\"ap.tmj")).expect("a map");
    let text = map_text(&map);
    assert!(
        text.contains(r#""name":"a\"b\\c\u000a\u0001é漢""#),
        "{text}"
    );
    assert!(text.contains(r#""image":"m\"ap-tileset1.png""#), "{text}");
    assert!(text.contains("\"opacity\":0.5019607843137255"), "{text}");
    assert!(text.contains("\"opacity\":1,"), "{text}");
    assert!(text.contains("\"mode\":\"multiply\""));
    assert_eq!(
        text.matches("\"mode\"").count(),
        1,
        "Addition is written as normal"
    );
    assert!(map.notes.iter().any(|n| n.contains("not written to Tiled")));
    assert!(
        map.notes[0].contains("0 paint layer(s) and 1 reference"),
        "{:?}",
        map.notes
    );
    assert!(text.ends_with("\"width\":1\n}\n"), "{text}");
}

#[test]
fn what_cannot_be_a_tiled_map_is_refused_with_a_reason() {
    let (doc, _) = with_tileset(8, 8, (4, 4), 1);
    assert!(matches!(
        tmj_map(&doc, 0, Path::new(MAP)),
        Err(ExportError::NoTilemaps)
    ));
    let (mut doc, set) = with_tileset(8, 8, (4, 4), 1);
    add_map(&mut doc, set, 1, &[0], (0, 0));
    assert!(matches!(
        tmj_map(&doc, 1, Path::new(MAP)),
        Err(ExportError::NoSuchFrame {
            frame: 1,
            frames: 1
        })
    ));
    let tall = doc
        .add_tileset("Tall", ColorMode::Rgba, (4, 8))
        .expect("room");
    add_map(&mut doc, tall, 1, &[0], (0, 0));
    let err = tmj_map(&doc, 0, Path::new(MAP)).expect_err("two sizes");
    assert!(matches!(err, ExportError::TileSizes { .. }));
    assert_eq!(
        err.to_string(),
        "A Tiled map has one tile size, and the tilemap layers differ: \"Tilemap 2\" has tiles \
         of 4 by 4 and \"Tilemap 3\" of 4 by 8. Give every tilemap layer tiles of one size, or \
         remove the others from a copy of the project"
    );
    let (mut doc, big) = with_tileset(8, 8, (256, 256), 0);
    for n in 0..1089 {
        doc.add_tile(big, tile(1, 1, n)).expect("fits");
    }
    add_map(&mut doc, big, 1, &[0], (0, 0));
    let err = tmj_map(&doc, 0, Path::new(MAP)).expect_err("too large");
    assert!(err.to_string().contains("atlas of 8448 by 8448"), "{err}");
}

#[test]
fn a_palette_tileset_becomes_rgba_atlas_pixels_in_panel_order() {
    let mut doc = Document::new(8, 8, ColorMode::Palette).expect("size");
    let colors = [
        rgba(200, 10, 10, 255),
        rgba(10, 200, 10, 128),
        rgba(1, 2, 3, 255),
    ];
    let ids: Vec<u32> = colors
        .iter()
        .map(|&c| Arc::make_mut(&mut doc.colors).id_for(c).expect("room"))
        .collect();
    let set = doc
        .add_tileset("P", ColorMode::Palette, (2, 2))
        .expect("room");
    for n in 0..3usize {
        let px = [ids[n], 0, ids[(n + 1) % 3], ids[n]];
        let image = Arc::new(Grid::from_cells(2, 2, &px).into_image());
        doc.add_tile(set, image).expect("fits");
    }
    let set = doc.tileset(set).expect("there");
    let mut bytes = Vec::new();
    write_atlas_png(set, &doc.colors, &mut bytes).expect("written");
    let atlas = png::decode(&bytes).expect("decodes");
    assert_eq!(AtlasLayout::new(3, (2, 2)).columns, 2);
    assert_eq!((atlas.width, atlas.height), (4, 4));
    let at = |x: usize, y: usize| atlas.px[y * 4 + x];
    for n in 0..3usize {
        let (x0, y0) = ((n % 2) * 2, (n / 2) * 2);
        assert_eq!(at(x0, y0), colors[n]);
        assert_eq!(at(x0 + 1, y0), 0);
        assert_eq!(at(x0, y0 + 1), colors[(n + 1) % 3]);
    }
    assert_eq!(
        [at(2, 2), at(3, 3)],
        [0, 0],
        "the place past the last tile is clear"
    );
}

/// Draws the map as Tiled is asked to: each shown tile layer, bottom first, its tiles read
/// from the atlases, turned by the GID's bits, at its pixel offset. Tiles here are opaque or
/// clear and layers opaque, so drawing over is replacing.
fn reconstruct(doc: &Document, map: &TmjMap) -> Vec<u32> {
    let atlases = atlases_of(doc, map);
    let mut canvas = vec![0u32; doc.width * doc.height];
    for layer in shown_of(map) {
        for (i, &gid) in layer.data.iter().enumerate() {
            let n = gid & 0x0fff_ffff;
            if let Some(atlas) = atlases.iter().rev().find(|a| a.0 <= n) {
                let at = ((i % map.width) * map.tile.0, (i / map.width) * map.tile.1);
                draw_tile(doc, map, (atlas, gid), (at, layer.offset), &mut canvas);
            }
        }
    }
    canvas
}

/// Each atlas as a PNG decodes: its first GID, its columns and its pixels.
fn atlases_of(doc: &Document, map: &TmjMap) -> Vec<(u32, usize, png::Image)> {
    map.tilesets
        .iter()
        .map(|t| {
            let mut bytes = Vec::new();
            let set = doc.tileset(t.id).expect("there");
            write_atlas_png(set, &doc.colors, &mut bytes).expect("written");
            (
                t.first_gid,
                t.layout.columns,
                png::decode(&bytes).expect("decodes"),
            )
        })
        .collect()
}

/// The tile layers drawn: shown themselves and in no hidden group, bottom first.
fn shown_of(map: &TmjMap) -> Vec<&crate::tmj::TmjLayer> {
    let mut shown = Vec::new();
    fn walk<'a>(nodes: &'a [TmjNode], on: bool, out: &mut Vec<&'a crate::tmj::TmjLayer>) {
        for node in nodes {
            match node {
                TmjNode::Tiles(l) if on && l.visible => out.push(l),
                TmjNode::Tiles(_) => {}
                TmjNode::Group {
                    layers, visible, ..
                } => walk(layers, on && *visible, out),
            }
        }
    }
    walk(&map.layers, true, &mut shown);
    shown
}

type Placed<'a> = (&'a (u32, usize, png::Image), u32);

/// Draws the tile `gid` names, turned by its bits, with its top left at map pixel `at` moved
/// by the layer's `offset`, over `canvas`, opaque pixels only.
fn draw_tile(
    doc: &Document,
    map: &TmjMap,
    ((first, columns, atlas), gid): Placed<'_>,
    (at, offset): ((usize, usize), (i32, i32)),
    canvas: &mut [u32],
) {
    let (tw, th) = map.tile;
    let k = ((gid & 0x0fff_ffff) - first) as usize;
    let o = Orientation::from_tiled_bits(gid & 0xe000_0000).inverse();
    for dy in 0..th {
        for dx in 0..tw {
            let (sx, sy) = o.apply(dx, dy, tw, th);
            let src = atlas.px[((k / columns) * th + sy) * atlas.width + (k % columns) * tw + sx];
            let x = (at.0 + dx) as i32 + offset.0;
            let y = (at.1 + dy) as i32 + offset.1;
            let on = (0..doc.width as i32).contains(&x) && (0..doc.height as i32).contains(&y);
            if on && src >> 24 == 255 {
                canvas[y as usize * doc.width + x as usize] = src;
            }
        }
    }
}

/// A small pseudo-random number source, the same on every run.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % u64::from(n)) as u32
    }
}

#[test]
fn the_map_and_its_atlases_draw_what_the_png_export_draws() {
    for seed in 1..60u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let side = [(4, 4), (5, 5), (3, 6), (8, 8)][rng.below(4) as usize];
        let square = side.0 == side.1;
        let (w, h) = (9 + rng.below(30) as usize, 7 + rng.below(30) as usize);
        let (mut doc, set) = with_tileset(w, h, side, 1 + rng.below(6));
        let count = doc.tileset(set).expect("there").len() as u32;
        for _ in 0..1 + rng.below(3) {
            let (cw, ch) = (1 + rng.below(9) as usize, 1 + rng.below(9) as usize);
            let cells: Vec<u32> = (0..cw * ch)
                .map(|_| match rng.below(4) {
                    0 => 0,
                    _ => {
                        let allowed: Vec<Orientation> = Orientation::ALL
                            .into_iter()
                            .filter(|o| square || !o.swaps_axes())
                            .collect();
                        let o = allowed[rng.below(allowed.len() as u32) as usize];
                        place(1 + rng.below(count), o)
                    }
                })
                .collect();
            let at = (rng.below(30) as i32 - 15, rng.below(30) as i32 - 15);
            let index = add_map(&mut doc, set, cw, &cells, at);
            doc.layers_mut()[index].visible = rng.below(5) > 0;
        }
        doc.layers_mut()[0].visible = false;
        let map = tmj_map(&doc, 0, Path::new(MAP)).expect("a map");
        let png = png::decode(&frame_png(&doc, 0).expect("png")).expect("decodes");
        assert_eq!(reconstruct(&doc, &map), png.px, "seed {seed}");
    }
}

#[test]
fn a_map_past_the_cell_bound_is_refused_before_it_is_built() {
    // 2,048 by 2,048 one-pixel tiles on two layers: twice the bound.
    let (mut doc, set) = with_tileset(2048, 2048, (1, 1), 1);
    add_map(&mut doc, set, 1, &[place(1, Orientation::IDENTITY)], (0, 0));
    let one = tmj_map(&doc, 0, Path::new(MAP)).expect("one layer is at the bound");
    assert_eq!(one.width * one.height, crate::tmj::MAX_TMJ_CELLS);
    add_map(&mut doc, set, 1, &[0], (0, 0));
    let refused = tmj_map(&doc, 0, Path::new(MAP));
    let Err(error @ ExportError::MapTooLarge { cells }) = refused else {
        panic!("refused: {refused:?}");
    };
    assert_eq!(cells, 2 * crate::tmj::MAX_TMJ_CELLS);
    assert!(error.to_string().contains("4,194,304"), "{error}");
    // The stress map of 256 by 256 cells on three layers is far inside it.
    const { assert!(256 * 256 * 3 * 16 <= crate::tmj::MAX_TMJ_CELLS) };
}
