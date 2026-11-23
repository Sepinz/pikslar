//! Duplicating a layer, and merging one into the layer below it.
//!
//! A merge draws the two layers exactly as the screen shows them on their own, through the same
//! arithmetic in [`crate::blend`], and keeps every pixel, on the canvas or past it. Frames that
//! show the same pair of drawings at the same places share one merged drawing, so holds survive.

use crate::blend::{multiply, over, separable, separable_of, unpremultiply, Separable};
use crate::document::{BlendMode, Blending, ColorMode, Document, LimitError, MAX_LAYERS};
use crate::image::Image;
use crate::layer::{Cel, Layer, LayerKind};
use crate::tile::{Tile, TILE, TILE_AREA};
use crate::tilemap::{map_bounds, sample_window};
use crate::tileset::Tileset;
use std::sync::Arc;

/// What makes two frames' merges the same: each layer's drawing and where it sits.
type Pair = (usize, (i16, i16), usize, (i16, i16));

impl Document {
    /// Puts a copy of the layer at `index` at `to`, which is above it. The copy shares every
    /// drawing until one is drawn on, and has holds of its own across the same frames.
    pub(crate) fn duplicate_layer_to(&mut self, index: usize, to: usize) -> Result<(), LimitError> {
        if self.layers.len() >= MAX_LAYERS {
            return Err(LimitError::TooManyLayers);
        }
        let source = self.layers.get(index).ok_or(LimitError::NoSuchItem)?;
        let cels = source.cels().to_vec();
        // In no link until the caller gives the copies theirs.
        let mut copy = source
            .duplicate(format!("{} copy", source.name))
            .with_cels(cels);
        copy.set_persisted_id(self.new_persisted_id());
        self.layers.insert(to, copy);
        Ok(())
    }

    /// Replaces the layer at `upper` and the one below it with one layer: the lower one's name
    /// and place, Normal at full opacity, drawn as the two are shown.
    pub(crate) fn merge_down(&mut self, upper: usize) -> Result<(), LimitError> {
        let lower = upper.checked_sub(1).ok_or(LimitError::NothingBelow)?;
        let (bottom, top) = match (self.layers.get(lower), self.layers.get(upper)) {
            (Some(bottom), Some(top)) => (bottom, top),
            _ => return Err(LimitError::NoSuchItem),
        };
        if bottom.kind == LayerKind::Group || top.kind == LayerKind::Group {
            return Err(LimitError::Group);
        }
        // A tilemap layer is drawn as pixels onto a paint layer, but nothing is drawn onto one.
        if bottom.is_tilemap() {
            return Err(LimitError::TilemapMerge);
        }
        if bottom.depth != top.depth {
            return Err(LimitError::MergeAcrossGroups);
        }
        if !bottom.visible || !top.visible {
            return Err(LimitError::HiddenMerge);
        }
        let (made, group_of) = self.merged_drawings(bottom, top)?;
        let (made, mode) = self.as_palette(bottom, top, made);
        let holds = holds_of(&group_of);
        let cels = group_of.iter().map(|&g| made[g].clone()).collect();
        let mut merged = Layer::new(bottom.name.clone(), mode).with_cels(cels);
        merged.set_persisted_id(bottom.persisted_id());
        merged.depth = bottom.depth;
        self.layers.remove(upper);
        self.layers.remove(lower);
        self.layers.insert(lower, merged);
        self.set_holds(lower, &holds)
    }

    /// One merged cel for each pair of drawings at their places, and which one each frame shows.
    /// A short list searched in order: a layer has at most a few thousand frames.
    fn merged_drawings(
        &self,
        bottom: &Layer,
        top: &Layer,
    ) -> Result<(Vec<Cel>, Vec<usize>), LimitError> {
        let mut pairs: Vec<Pair> = Vec::new();
        let mut made: Vec<Cel> = Vec::new();
        let mut group_of = Vec::with_capacity(bottom.frame_count());
        for frame in 0..bottom.frame_count() {
            let (b, t) = (bottom.cel(frame), top.cel(frame));
            let pair = (b.image_id(), b.position(), t.image_id(), t.position());
            let group = match pairs.iter().position(|p| *p == pair) {
                Some(group) => group,
                None => {
                    made.push(self.merge_cels(bottom, top, frame)?);
                    pairs.push(pair);
                    made.len() - 1
                }
            };
            group_of.push(group);
        }
        Ok((made, group_of))
    }

    /// The two layers' cels of one frame, drawn together over every pixel either holds.
    fn merge_cels(&self, bottom: &Layer, top: &Layer, frame: usize) -> Result<Cel, LimitError> {
        let (b, t) = (bottom.cel(frame), top.cel(frame));
        // A tilemap layer is read through its tileset, as the screen reads it.
        let tiles = match top.tileset() {
            Some(set) => Some(self.tileset(set)?),
            None => None,
        };
        let top_bounds = match tiles {
            Some(set) => map_bounds(t, set),
            None => t.drawn_bounds(),
        };
        let Some((x, y, w, h)) = union(b.drawn_bounds(), top_bounds) else {
            return Ok(Cel::blank());
        };
        let origin = if b.is_blank() {
            t.position()
        } else {
            b.position()
        };
        let (ox, oy) = (i32::from(origin.0), i32::from(origin.1));
        let side = TILE as i32;
        let (tx0, ty0) = ((x - ox).div_euclid(side), (y - oy).div_euclid(side));
        let (tx1, ty1) = (
            (x + w - 1 - ox).div_euclid(side),
            (y + h - 1 - oy).div_euclid(side),
        );
        let mut cells = Vec::new();
        let mut placed = [0u32; TILE_AREA];
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let corner = (ox + tx * side, oy + ty * side);
                let raw = self.merged_tile((bottom, top), (b, t), tiles, corner, &mut placed);
                cells.push(Tile::pack(&raw).map(Arc::new));
            }
        }
        let start = |v: i32| i16::try_from(v).map_err(|_| LimitError::OutOfReach);
        let wide = u16::try_from(tx1 - tx0 + 1).map_err(|_| LimitError::OutOfReach)?;
        let image = Image::from_tiles(start(tx0)?, start(ty0)?, wide, cells);
        image
            .and_then(|image| Cel::at(image, ox, oy))
            .ok_or(LimitError::OutOfReach)
    }

    /// The storage tile of the merge whose corner is at `corner` on the canvas. A tilemap top
    /// layer is read through `tiles`, its tileset, into `placed`.
    fn merged_tile(
        &self,
        (bottom, top): (&Layer, &Layer),
        (b, t): (&Cel, &Cel),
        tiles: Option<&Tileset>,
        corner: (i32, i32),
        placed: &mut [u32; TILE_AREA],
    ) -> [u32; TILE_AREA] {
        let (below, above) = (b.sampler(), t.sampler());
        if let Some(set) = tiles {
            if !sample_window(t, set, corner, placed) {
                placed.fill(0);
            }
        }
        let mut raw = [0u32; TILE_AREA];
        for (i, px) in raw.iter_mut().enumerate() {
            let cx = corner.0 + (i % TILE) as i32;
            let cy = corner.1 + (i / TILE) as i32;
            let over = if tiles.is_some() {
                placed[i]
            } else {
                above(cx, cy)
            };
            *px = self.merged_pixel(bottom, top, below(cx, cy), over);
        }
        raw
    }

    /// One pixel of the two layers alone, as straight RGBA.
    fn merged_pixel(&self, bottom: &Layer, top: &Layer, below: u32, above: u32) -> u32 {
        let color = |layer: &Layer, cell: u32| match layer.mode {
            ColorMode::Rgba => cell,
            ColorMode::Palette => self.colors.color(cell & 0xffff),
        };
        let (below, above) = (color(bottom, below), color(top, above));
        if self.blending == Blending::Aseprite {
            return straight_once(straight_once(0, below, bottom), above, top);
        }
        let shown = mix_once(0, below, bottom);
        unpremultiply(mix_once(shown, above, top))
    }

    /// The merged drawings as palette ids, when both layers use the palette and every merged
    /// color is one of the table's. Otherwise they stay full color.
    fn as_palette(&self, bottom: &Layer, top: &Layer, made: Vec<Cel>) -> (Vec<Cel>, ColorMode) {
        if (bottom.mode, top.mode) != (ColorMode::Palette, ColorMode::Palette) {
            return (made, ColorMode::Rgba);
        }
        let mut missing = false;
        let ids: Vec<Cel> = made
            .iter()
            .map(|cel| {
                let image = cel.image().map_cells(|c| {
                    self.colors.id_for_existing(c).unwrap_or_else(|| {
                        missing = true;
                        0
                    })
                });
                cel.with_image(Arc::new(image))
            })
            .collect();
        match missing {
            true => (made, ColorMode::Rgba),
            false => (ids, ColorMode::Palette),
        }
    }
}

/// Hold ids from which drawing each frame shows: frames sharing one are held together. A
/// drawing only one frame shows is no hold once [`crate::animation::tidy_links`] has run.
fn holds_of(group_of: &[usize]) -> Vec<u32> {
    group_of.iter().map(|&g| g as u32 + 1).collect()
}

/// The screen's arithmetic for one pixel, kept out of line: a merge runs once, so one copy of
/// every mode serves it rather than one for each place it is used.
fn mix_once(dst: u32, color: u32, layer: &Layer) -> u32 {
    let opacity = u32::from(layer.opacity);
    match (layer.blend.stand_in(), separable_of(layer.blend)) {
        (_, Some(mode)) => separable_once(dst, color, opacity, mode),
        (BlendMode::Multiply, None) => multiply(dst, color, opacity),
        (_, None) => over(dst, color, opacity),
    }
}

/// Aseprite-compatible blending's arithmetic for one pixel, as the screen of such a document
/// builds it, kept out of line for the same reason.
#[inline(never)]
fn straight_once(dst: u32, color: u32, layer: &Layer) -> u32 {
    crate::blend_straight::mix(dst, color, layer.blend, u32::from(layer.opacity))
}

/// [`separable`] with the mode chosen at run time, so it is built once rather than per mode.
#[inline(never)]
fn separable_once(dst: u32, color: u32, opacity: u32, mode: Separable) -> u32 {
    separable(dst, color, opacity, mode)
}

/// The smallest rectangle holding both, either of which may be missing.
fn union(
    a: Option<(i32, i32, i32, i32)>,
    b: Option<(i32, i32, i32, i32)>,
) -> Option<(i32, i32, i32, i32)> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let (x, y) = (a.0.min(b.0), a.1.min(b.1));
            let right = (a.0 + a.2).max(b.0 + b.2);
            let bottom = (a.1 + a.3).max(b.1 + b.3);
            Some((x, y, right - x, bottom - y))
        }
        (a, b) => a.or(b),
    }
}
