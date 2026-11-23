//! Carrying an id map to every image that holds palette ids: cels on every layer and frame, and
//! the images of palette-mode tilesets.
//!
//! Each distinct image is mapped once and each distinct storage tile once, so cels that share a
//! drawing (holds, links, a copied frame) still share it afterwards, and tiles that frames
//! share still cost one. Floating pixels are not touched here: a command that changes the
//! document puts them down first, so they are cels by the time this runs.

use crate::document::{ColorMode, Document};
use crate::grid::Cell;
use crate::image::Image;
use crate::layer::{Layer, LayerKind};
use crate::tile::{RawTile, Tile, TILE_AREA};
use crate::tileset::{TileId, TilesetId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Which cels an id map reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Every palette image of the document, tilesets included.
    Everywhere,
    /// The cels of one layer, and the cels linked to them on other layers, since a link is one
    /// drawing. Tilesets are not reached.
    Layer(usize),
}

/// True for a layer whose images hold palette ids. A tilemap layer's images hold map cells, not
/// colours, so only the kinds named here do.
pub(crate) fn holds_ids(layer: &Layer) -> bool {
    layer.mode == ColorMode::Palette
        && matches!(layer.kind, LayerKind::Paint | LayerKind::Reference)
}

/// For each layer and frame, whether the cel is in `scope`. A layer that holds no ids has no
/// row.
pub(crate) fn scoped_cels(doc: &Document, scope: Scope) -> Vec<Vec<bool>> {
    let layers = doc.layers();
    let linked: HashSet<u32> = match scope {
        Scope::Layer(at) => layers
            .get(at)
            .map(|l| l.links().filter(|&id| id != 0).collect())
            .unwrap_or_default(),
        Scope::Everywhere => HashSet::new(),
    };
    layers
        .iter()
        .enumerate()
        .map(|(index, layer)| {
            if !holds_ids(layer) {
                return Vec::new();
            }
            let own = scope == Scope::Everywhere || scope == Scope::Layer(index);
            layer
                .links()
                .map(|link| own || (link != 0 && linked.contains(&link)))
                .collect()
        })
        .collect()
}

/// Every distinct image that holds palette ids, once: the drawings of cels and the images of
/// palette tilesets.
pub(crate) fn palette_images(doc: &Document) -> Vec<&Arc<Image>> {
    let mut seen = HashSet::new();
    let cels = doc.layers().iter().filter(|l| holds_ids(l));
    let drawings = cels.flat_map(|l| (0..l.frame_count()).filter_map(move |f| l.drawing(f)));
    let tiles = doc
        .tilesets
        .iter()
        .filter(|set| set.mode() == ColorMode::Palette)
        .flat_map(|set| set.tiles().iter().map(|entry| &entry.image));
    drawings
        .chain(tiles)
        .filter(|image| seen.insert(Arc::as_ptr(image) as usize))
        .collect()
}

/// Which ids a pixel shows anywhere in the document.
pub(crate) fn used_ids(doc: &Document) -> Vec<bool> {
    let mut used = vec![false; doc.colors.len()];
    let mut seen = HashSet::new();
    let mut raw: RawTile = [0; TILE_AREA];
    for image in palette_images(doc) {
        for tile in image
            .drawn_tiles()
            .filter(|t| seen.insert(Arc::as_ptr(t) as usize))
        {
            tile.decode(&mut raw);
            for &id in raw.iter() {
                if let Some(slot) = used.get_mut(id as usize) {
                    *slot = true;
                }
            }
        }
    }
    used
}

/// The cels in `scope` that show `id`, as layer and frame.
pub(crate) fn cels_showing(doc: &Document, id: u32, scope: Scope) -> Vec<(usize, usize)> {
    // Images looked at, by address, as those that show the id and those that do not: sets of
    // plain numbers, a kind the program has anyway.
    let (mut shows, mut hides) = (HashSet::new(), HashSet::new());
    let mut found = Vec::new();
    for (index, row) in scoped_cels(doc, scope).iter().enumerate() {
        for (frame, _) in row.iter().enumerate().filter(|(_, &in_scope)| in_scope) {
            let Some(image) = doc.layers()[index].drawing(frame) else {
                continue;
            };
            let key = Arc::as_ptr(image) as usize;
            let shown = shows.contains(&key)
                || (!hides.contains(&key) && {
                    let shown = image_shows(image, id);
                    match shown {
                        true => shows.insert(key),
                        false => hides.insert(key),
                    };
                    shown
                });
            if shown {
                found.push((index, frame));
            }
        }
    }
    found
}

pub(crate) fn image_shows(image: &Image, id: u32) -> bool {
    image.drawn_tiles().any(|tile| tile.shows(id))
}

/// Maps images through an id map, each image and each tile once.
struct Mapper<'a> {
    map: &'a [u32],
    images: HashMap<usize, Arc<Image>>,
    tiles: HashMap<usize, (Cell, bool)>,
}

impl<'a> Mapper<'a> {
    fn new(map: &'a [u32]) -> Self {
        Mapper {
            map,
            images: HashMap::new(),
            tiles: HashMap::new(),
        }
    }

    /// The image with its ids moved: the very same image if none moves.
    fn image(&mut self, image: &Arc<Image>) -> Arc<Image> {
        let key = Arc::as_ptr(image) as usize;
        if let Some(done) = self.images.get(&key) {
            return done.clone();
        }
        let mut changed = false;
        let mapped = image.map_tiles(|tile| {
            let (cell, moved) = self.tile(tile);
            changed |= moved;
            cell
        });
        let out = if changed {
            Arc::new(mapped)
        } else {
            image.clone()
        };
        self.images.insert(key, out.clone());
        out
    }

    fn tile(&mut self, tile: &Arc<Tile>) -> (Cell, bool) {
        let key = Arc::as_ptr(tile) as usize;
        if let Some(done) = self.tiles.get(&key) {
            return done.clone();
        }
        let (cell, moved) = match tile.map_values(self.map) {
            Some(new) => (new.map(Arc::new), true),
            None => (Some(tile.clone()), false),
        };
        self.tiles.insert(key, (cell.clone(), moved));
        (cell, moved)
    }
}

/// The new drawings of each layer that changes, one per frame, as layer index and drawings.
type LayerDrawings = Vec<(usize, Vec<Arc<Image>>)>;
/// The new images of the tiles that change.
type TileImages = Vec<(TilesetId, TileId, Arc<Image>)>;

/// Moves every palette id in `scope` through `map`. A drawing that shows no moved id is left
/// as it is, so a command that changes nothing leaves the document equal and records no step.
pub(crate) fn rewrite(doc: &mut Document, map: &[u32], scope: Scope) {
    let mut mapper = Mapper::new(map);
    let drawings = new_drawings(doc, scope, &mut mapper);
    let tiles = match scope {
        Scope::Everywhere => new_tile_images(doc, &mut mapper),
        Scope::Layer(_) => Vec::new(),
    };
    drop(mapper);
    for (index, images) in drawings {
        let layer = &mut doc.layers_mut()[index];
        // One entry in the step's log for the whole list, then each frame shows its drawing:
        // frames of one hold or link get the one mapped drawing, so they stay shared.
        layer.replace_images(vec![Image::blank(); images.len()]);
        for (frame, image) in images.iter().enumerate() {
            layer.show_drawing(frame, image);
        }
    }
    if !tiles.is_empty() {
        let table = Arc::make_mut(&mut doc.tilesets);
        for (set, tile, image) in tiles {
            // A remap sends each value to one value, so no storage tile gains colours and the
            // tiles never take more room in a file than before.
            if let Some(found) = table.get_mut(set) {
                found.set_image(tile, image);
            }
        }
    }
}

fn new_drawings(doc: &Document, scope: Scope, mapper: &mut Mapper) -> LayerDrawings {
    let mut out = Vec::new();
    for (index, row) in scoped_cels(doc, scope).iter().enumerate() {
        let layer = &doc.layers()[index];
        let old: Vec<&Arc<Image>> = (0..row.len()).filter_map(|f| layer.drawing(f)).collect();
        let new: Vec<Arc<Image>> = old
            .iter()
            .zip(row)
            .map(|(&image, &in_scope)| match in_scope {
                true => mapper.image(image),
                false => image.clone(),
            })
            .collect();
        if old.iter().zip(&new).any(|(a, b)| !Arc::ptr_eq(a, b)) {
            out.push((index, new));
        }
    }
    out
}

fn new_tile_images(doc: &Document, mapper: &mut Mapper) -> TileImages {
    let palette = |set: &&crate::tileset::Tileset| set.mode() == ColorMode::Palette;
    let mut out = Vec::new();
    for set in doc.tilesets.iter().filter(palette) {
        for entry in set.tiles() {
            let image = mapper.image(&entry.image);
            if !Arc::ptr_eq(&image, &entry.image) {
                out.push((set.id(), entry.id, image));
            }
        }
    }
    out
}
