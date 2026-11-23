//! A Tiled map (TMJ, Tiled's JSON) of one frame's tilemap layers, with one atlas PNG for each
//! tileset they use.
//!
//! The map is orthogonal, drawn right-down, with its tilesets embedded. A cell's GID is its
//! tileset's first GID plus the tile's place in the panel, with the cell's three Tiled flip bits;
//! the pinned bit is never written, since Tiled reads bit 28 as a hexagonal turn. Each tilemap
//! layer becomes a `tilelayer`, groups become Tiled group layers, and paint and reference layers
//! are left out with a note. A cel offset of whole tiles moves the cells; what is left over, from
//! 0 to one tile less, becomes a negative `offsetx` and `offsety`, so cell column 0 always
//! reaches the canvas's left edge. The map is as many tiles as the canvas needs for every layer.

use crate::atlas::{atlas_layout, AtlasLayout};
use crate::{ExportError, Output};
use pikslar_core::tilemap::MapCell;
use pikslar_core::tileset::{Tileset, TilesetId};
use pikslar_core::{BlendMode, Document, Layer, LayerKind};
use std::path::{Path, PathBuf};

/// A map holds at most this many cells, its width times its height times its tilemap layers.
/// A map is held whole while its text is made, at about 4 bytes a cell and up to 11 more in the
/// text, so this keeps an export under about 64 MiB. It is 21 times the 256 by 256 map of three
/// layers the stress tests use; a canvas of 8,192 one-pixel tiles each way is refused.
pub const MAX_TMJ_CELLS: usize = 1 << 22;

/// A tileset as the map embeds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmjTileset {
    pub id: TilesetId,
    pub name: String,
    pub first_gid: u32,
    /// The atlas's file name, beside the map.
    pub image: String,
    pub layout: AtlasLayout,
    pub count: usize,
}

/// A tilemap layer as the map holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmjLayer {
    /// Its position in the document, from the bottom.
    pub index: usize,
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    /// Tiled's name for the blend mode, or none for normal.
    pub mode: Option<&'static str>,
    /// What is left of the cel offset after whole tiles: from one tile less than 0 up to 0.
    pub offset: (i32, i32),
    /// The map's GIDs, row by row, top first.
    pub data: Vec<u32>,
}

/// A layer of the map, bottom first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TmjNode {
    Tiles(TmjLayer),
    Group {
        name: String,
        visible: bool,
        layers: Vec<TmjNode>,
    },
}

/// Everything the map says, before it is written as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TmjMap {
    /// In tiles.
    pub width: usize,
    pub height: usize,
    pub tile: (usize, usize),
    pub tilesets: Vec<TmjTileset>,
    pub layers: Vec<TmjNode>,
    pub notes: Vec<String>,
}

/// Where the atlas of the `n`th exported tileset (from 1) goes, beside the map.
pub fn atlas_path(map: &Path, n: usize) -> PathBuf {
    let stem = map.file_stem().map(|s| s.to_string_lossy().into_owned());
    let name = format!("{}-tileset{n}.png", stem.unwrap_or_default());
    map.with_file_name(name)
}

/// The tilemap layers of `doc`, bottom first, each with its tileset.
fn map_layers(doc: &Document) -> Result<Vec<(usize, &Tileset)>, ExportError> {
    let mut found = Vec::new();
    for (index, layer) in doc.layers().iter().enumerate() {
        if let LayerKind::Tilemap(id) = layer.kind {
            found.push((index, doc.tileset(id)?));
        }
    }
    if found.is_empty() {
        return Err(ExportError::NoTilemaps);
    }
    Ok(found)
}

/// The one tile size every tilemap layer has, or the first two layers that differ.
fn shared_size(doc: &Document, maps: &[(usize, &Tileset)]) -> Result<(usize, usize), ExportError> {
    let (first, set) = maps[0];
    let size = set.tile_size();
    match maps.iter().find(|(_, s)| s.tile_size() != size) {
        None => Ok(size),
        Some(&(other, s)) => Err(ExportError::TileSizes {
            first: (doc.layers()[first].name.clone(), size),
            other: (doc.layers()[other].name.clone(), s.tile_size()),
        }),
    }
}

/// The tilesets the layers place tiles from, in the document's order, each with its first GID
/// and its atlas named for `map`. A tileset with no tiles has nothing to place and is left out.
fn used_tilesets(
    doc: &Document,
    maps: &[(usize, &Tileset)],
    map: &Path,
) -> Result<Vec<TmjTileset>, ExportError> {
    let mut out: Vec<TmjTileset> = Vec::new();
    let mut next_gid = 1u32;
    let used = |set: &Tileset| maps.iter().any(|(_, s)| s.id() == set.id());
    for set in doc.tilesets.iter().filter(|s| used(s) && !s.is_empty()) {
        let layout = atlas_layout(set)?;
        let image = atlas_path(map, out.len() + 1);
        let image = image.file_name().map(|n| n.to_string_lossy().into_owned());
        out.push(TmjTileset {
            id: set.id(),
            name: set.name().to_string(),
            first_gid: next_gid,
            image: image.unwrap_or_default(),
            layout,
            count: set.len(),
        });
        next_gid += set.len() as u32;
    }
    Ok(out)
}

/// A cel offset as whole tiles and what is left: `offset = whole * side + rest`, with `rest`
/// from `1 - side` up to 0.
fn split_offset(offset: i32, side: i32) -> (i32, i32) {
    let whole = -(-offset).div_euclid(side);
    (whole, offset - whole * side)
}

/// The tiles one layer needs across a canvas side of `canvas` pixels, its rest offset `rest`.
fn tiles_needed(canvas: usize, rest: i32, side: usize) -> usize {
    (canvas + rest.unsigned_abs() as usize).div_ceil(side)
}

/// Tiled's name for a blend mode, where Tiled has the same one.
fn tiled_mode(blend: BlendMode) -> Result<Option<&'static str>, ()> {
    Ok(match blend {
        BlendMode::Normal => None,
        BlendMode::Multiply => Some("multiply"),
        BlendMode::Screen => Some("screen"),
        BlendMode::Overlay => Some("overlay"),
        BlendMode::Darken => Some("darken"),
        BlendMode::Lighten => Some("lighten"),
        BlendMode::Difference => Some("difference"),
        // Not written: Tiled lacks the first two, and its own modes of the same names are not
        // promised to draw as Aseprite-compatible blending does.
        BlendMode::Addition
        | BlendMode::Subtract
        | BlendMode::ColorDodge
        | BlendMode::ColorBurn
        | BlendMode::HardLight
        | BlendMode::SoftLight
        | BlendMode::Exclusion
        | BlendMode::Hue
        | BlendMode::Saturation
        | BlendMode::Color
        | BlendMode::Luminosity
        | BlendMode::Divide => return Err(()),
    })
}

/// The GID that places what `cell` places, 0 for nothing.
fn gid(cell: MapCell, set: &Tileset, first_gid: u32) -> u32 {
    let Some(place) = cell.tile().and_then(|id| set.position(id)) else {
        return 0;
    };
    let orientation = cell.orientation();
    let (w, h) = set.tile_size();
    if orientation.swaps_axes() && w != h {
        return 0;
    }
    (first_gid + place as u32) | orientation.to_tiled_bits()
}

/// What the layer builder needs to know of the map.
struct Frame<'a> {
    doc: &'a Document,
    frame: usize,
    tile: (usize, usize),
    size: (usize, usize),
    tilesets: &'a [TmjTileset],
}

/// One tilemap layer's GIDs over the map, and how many placed cells fall outside it.
fn layer_of(at: &Frame<'_>, index: usize, set: &Tileset) -> (TmjLayer, usize) {
    let layer: &Layer = &at.doc.layers()[index];
    let cel = layer.cel(at.frame);
    let (ox, oy) = cel.position();
    let (qx, rx) = split_offset(i32::from(ox), at.tile.0 as i32);
    let (qy, ry) = split_offset(i32::from(oy), at.tile.1 as i32);
    let first = at.tilesets.iter().find(|t| t.id == set.id());
    let (w, h) = at.size;
    let mut data = vec![0u32; w * h];
    let mut outside = 0;
    if let (Some(first), Some((x0, y0, cw, ch))) = (first, cel.image().drawn_bounds()) {
        for cy in y0..y0 + ch {
            for cx in x0..x0 + cw {
                let cell = MapCell::from_bits(cel.image().get(cx, cy));
                let value = gid(cell, set, first.first_gid);
                let (mx, my) = (cx + qx, cy + qy);
                let inside = (0..w as i32).contains(&mx) && (0..h as i32).contains(&my);
                match (value, inside) {
                    (0, _) => {}
                    (v, true) => data[my as usize * w + mx as usize] = v,
                    (_, false) => outside += 1,
                }
            }
        }
    }
    let tiles = TmjLayer {
        index,
        name: layer.name.clone(),
        visible: layer.visible,
        opacity: layer.opacity,
        mode: tiled_mode(layer.blend).unwrap_or(None),
        offset: (rx, ry),
        data,
    };
    (tiles, outside)
}

/// The map of `doc`'s tilemap layers in `frame`, its atlases named for `map`. Nothing is
/// written.
#[inline(never)]
pub fn tmj_map(doc: &Document, frame: usize, map: &Path) -> Result<TmjMap, ExportError> {
    let frames = doc.frames.len();
    if frame >= frames {
        return Err(ExportError::NoSuchFrame { frame, frames });
    }
    let maps = map_layers(doc)?;
    let tile = shared_size(doc, &maps)?;
    let tilesets = used_tilesets(doc, &maps, map)?;
    let mut size = (1, 1);
    for (index, _) in &maps {
        let (ox, oy) = doc.layers()[*index].cel(frame).position();
        let rx = split_offset(i32::from(ox), tile.0 as i32).1;
        let ry = split_offset(i32::from(oy), tile.1 as i32).1;
        size.0 = size.0.max(tiles_needed(doc.width, rx, tile.0));
        size.1 = size.1.max(tiles_needed(doc.height, ry, tile.1));
    }
    let cells = (size.0 * size.1).saturating_mul(maps.len());
    if cells > MAX_TMJ_CELLS {
        return Err(ExportError::MapTooLarge { cells });
    }
    let at = Frame {
        doc,
        frame,
        tile,
        size,
        tilesets: &tilesets,
    };
    let mut built = Vec::new();
    let mut outside = 0;
    for (index, set) in &maps {
        let (layer, out) = layer_of(&at, *index, set);
        outside += out;
        built.push(layer);
    }
    let layers = crate::tmj_tree::tree(doc, built);
    let notes = notes(doc, outside);
    Ok(TmjMap {
        width: size.0,
        height: size.1,
        tile,
        tilesets,
        layers,
        notes,
    })
}

/// What the person should know: layers left out, blend modes Tiled lacks, cells off the map.
fn notes(doc: &Document, outside: usize) -> Vec<String> {
    let count = |kind: LayerKind| doc.layers().iter().filter(|l| l.kind == kind).count();
    let (paint, reference) = (count(LayerKind::Paint), count(LayerKind::Reference));
    let mut notes = Vec::new();
    if paint + reference > 0 {
        notes.push(format!(
            "A TMJ holds tilemap layers only: {paint} paint layer(s) and {reference} reference \
             layer(s) are left out"
        ));
    }
    let lacking = doc
        .layers()
        .iter()
        .filter(|l| matches!(l.kind, LayerKind::Tilemap(_)) && tiled_mode(l.blend).is_err())
        .count();
    if lacking > 0 {
        notes.push(format!(
            "{lacking} tilemap layer(s) use a blend mode not written to Tiled (Addition, Subtract \
             or one only Aseprite-compatible blending draws), and are written with the normal \
             blend mode"
        ));
    }
    if outside > 0 {
        notes.push(format!(
            "{outside} placed cell(s) lie wholly outside the canvas, which does not show them, \
             and are left out of the map"
        ));
    }
    notes
}

/// Every file the TMJ export makes, not yet written: an atlas for each tileset, then the map,
/// and anything the person should know.
pub struct TmjPlanned<'a> {
    pub outputs: Vec<Output<'a>>,
    pub notes: Vec<String>,
}

/// Plans the TMJ of `doc`'s `frame` at `path`, with its atlases beside it. Nothing is written.
#[inline(never)]
pub fn tmj_outputs<'a>(
    doc: &'a Document,
    frame: usize,
    path: PathBuf,
) -> Result<TmjPlanned<'a>, ExportError> {
    let map = tmj_map(doc, frame, &path)?;
    let text = crate::tmj_json::map_text(&map);
    let mut outputs: Vec<Output<'a>> = Vec::with_capacity(map.tilesets.len() + 1);
    for (n, entry) in map.tilesets.iter().enumerate() {
        let set = doc.tileset(entry.id)?;
        outputs.push(Output {
            path: atlas_path(&path, n + 1),
            write: Box::new(move |out: &mut dyn std::io::Write| {
                crate::atlas::write_atlas_png(set, &doc.colors, out).map_err(crate::io_error)
            }),
            ours_only: false,
        });
    }
    outputs.push(Output {
        path,
        write: Box::new(move |out: &mut dyn std::io::Write| out.write_all(text.as_bytes())),
        ours_only: false,
    });
    Ok(TmjPlanned {
        outputs,
        notes: map.notes,
    })
}
