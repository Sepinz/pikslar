//! The document: layers across frames, plus the shared color table.

use crate::animation::Tag;
use crate::color::{ColorTable, TableFull};
use crate::grid::Grid;
use crate::layer::{Cel, Layer, PersistedLayerId};
use std::sync::Arc;

pub const MAX_DIMENSION: usize = 8192;
pub const MAX_FRAMES: usize = 4096;
pub const MAX_LAYERS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitError {
    Dimensions,
    TooManyFrames,
    TooManyLayers,
    LastLayer,
    LastFrame,
    TableFull,
    NoSuchItem,
    NoMemory,
    /// A turn of the whole drawing would put pixels past what a layer can hold.
    OutOfReach,
    /// The frames of a hold sit in different places, so one turn cannot serve them all.
    UnequalHold,
    /// Cels may only land on layers of the same color mode.
    CelPlacement,
    /// A link joins layers of one color mode only.
    LinkModes,
    /// Only paint layers share a drawing: no reference or group is in a link with another layer.
    LinkKind,
    /// A drawing shared with a locked layer is not drawn on.
    SharedLocked,
    /// A new canvas would move a cel past the offset range.
    CanvasReach,
    /// The layer is locked, so it cannot be drawn on.
    Locked,
    /// The bottom layer has nothing to merge into.
    NothingBelow,
    /// A merge draws what is shown, so both layers must be shown.
    HiddenMerge,
    /// A reference layer is drawn from, not on.
    Reference,
    /// A group has no pixels or settings of its own.
    Group,
    /// Groups nest at most [`crate::groups::MAX_GROUP_DEPTH`] deep.
    TooDeep,
    /// Merging joins two layers in the same group.
    MergeAcrossGroups,
    /// A move that would split a group from what it holds.
    GroupOrder,
    /// A reference image past [`crate::reference::MAX_REFERENCE_SIDE`], or past what all of them
    /// may take together.
    ReferenceTooLarge,
    /// A frame rate past [`crate::timing::MAX_FRAME_RATE`], or 0.
    FrameRate,
    /// A document holds at most [`crate::slice::MAX_SLICES`] slices.
    TooManySlices,
    /// A tween needs two keys and a frame between them.
    TweenTooShort,
    /// A tween moves one held drawing, so every frame of the run must be in one hold.
    TweenUnheld,
    /// A brush is at most 256 by 256 pixels.
    BrushTooLarge,
    /// A brush is made from selected pixels, and nothing is selected.
    NothingToCapture,
    /// A palette holds at most 256 groups.
    TooManyGroups,
    /// A ramp runs between two different swatches, with 1 to
    /// [`crate::palette_ops::ramp::MAX_RAMP_STEPS`] colours between them.
    RampSteps,
    /// A palette file could not be read; the reader's own message says why.
    PaletteFile,
    /// Load as colours needs a list as long as the swatch list, since each swatch takes the
    /// colour at its own place.
    PaletteSize,
    /// There are no colours to load.
    PaletteNoColors,
    /// A document holds at most 64 tilesets.
    TooManyTilesets,
    /// A tileset holds at most 65,535 tiles.
    TooManyTiles,
    /// A tileset that layers use is not removed.
    TilesetInUse,
    /// A link joins tilemap layers of one tileset only.
    LinkTileset,
    /// A tilemap layer is edited in the Tiles or Pixels mode, not by the pixel commands.
    Tilemap,
    /// Turning tiles by a quarter turn needs tiles as wide as they are tall.
    SquareTiles,
    /// A pixel over an empty cell has no tile to draw on.
    NoTileHere,
    /// A tile is 1 to 256 pixels each way.
    TileSize,
    /// A terrain set has no such slot.
    TerrainSlot,
    /// A tileset holds at most 64 terrain sets, each of its own id.
    TooManyTerrains,
    /// A tilemap layer takes its colour mode from its tileset, so it is not converted.
    TilemapMode,
    /// Tilemap layers are not merged down, and nothing is merged onto one.
    TilemapMerge,
    /// Cels of a tilemap layer land on tilemap layers of the same tileset only.
    TilemapCels,
    /// The layer is not a tilemap layer.
    NotTilemap,
    /// Placing tiles needs a tile chosen, from the tileset of the layer.
    NoTileChosen,
    /// A stamp is at most 64 by 64 tiles.
    StampTooLarge,
    /// Every tile together, as a project file stores them, is at most
    /// [`crate::tileset::MAX_TILE_BYTES`].
    TileMemory,
    /// A script waits until the stroke in progress is finished and floating pixels are placed.
    Busy,
}

impl std::error::Error for LimitError {}

impl From<TableFull> for LimitError {
    fn from(_: TableFull) -> Self {
        LimitError::TableFull
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    /// Cells are IDs into the color table, so recoloring follows the palette.
    Palette,
    /// Cells are straight-alpha colors.
    Rgba,
}

pub use crate::blend_mode::{BlendMode, Blending};

pub use crate::ids::FrameInfo;
use crate::ids::IdFloor;

/// Data from a project file that this version does not read, carried along unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignSection {
    pub tag: [u8; 4],
    pub data: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub width: usize,
    pub height: usize,
    pub colors: Arc<ColorTable>,
    pub(crate) layers: crate::layers::Layers,
    pub frames: Vec<FrameInfo>,
    pub tags: Vec<Tag>,
    /// Frames a second, for a document timed by frame rate rather than in milliseconds.
    pub frame_rate: Option<u8>,
    pub slices: Vec<crate::slice::Slice>,
    /// The tiles tilemap layers place: one table for the whole document, shared until changed.
    pub tilesets: Arc<crate::tileset::Tilesets>,
    /// Kept so that saving does not lose what a newer version wrote.
    pub foreign: Vec<ForeignSection>,
    /// How the layers combine: Pikslar's blending, or Aseprite's for imported art that needs it.
    pub blending: Blending,
    /// The highest persisted layer id given while the document has been open: never given
    /// again. Saved when above the live ids, and not compared.
    pub(crate) layer_floor: IdFloor,
    /// The highest persisted frame id given while the document has been open, as for layers.
    pub(crate) frame_floor: IdFloor,
    /// Which saved document this is, for Rewind. Not compared: see [`crate::ids::DocumentId`].
    pub document_id: crate::ids::DocumentId,
}

/// Within the offset range, so every canvas pixel has a tile the cel can store.
fn placeable(cel: &Cel) -> bool {
    let (x, y) = cel.position();
    Cel::in_range(x.into(), y.into())
}

impl Document {
    /// A document from a file reader's parts, refused unless every layer has a cel for every
    /// frame, each placed within the offset range. Layers are numbered in order and their holds
    /// kept apart.
    pub fn from_parts(
        width: usize,
        height: usize,
        colors: Arc<ColorTable>,
        mut layers: Vec<Layer>,
        frames: Vec<FrameInfo>,
    ) -> Result<Self, LimitError> {
        if layers.is_empty() || frames.is_empty() {
            return Err(LimitError::NoSuchItem);
        }
        for layer in &layers {
            if layer.frame_count() != frames.len() {
                return Err(LimitError::NoSuchItem);
            }
            if !layer.cels().iter().all(placeable) {
                return Err(LimitError::Dimensions);
            }
        }
        let frames: Vec<FrameInfo> = frames
            .into_iter()
            .zip(1..)
            .map(|(info, id)| info.renewed(id))
            .collect();
        let mut frame_floor = IdFloor::default();
        frame_floor.raise(frames.len() as u32);
        let mut used = 0;
        for (index, layer) in layers.iter_mut().enumerate() {
            layer.set_persisted_id(PersistedLayerId(index as u32 + 1));
            used = layer.shift_holds(used);
        }
        Ok(Document {
            width,
            height,
            colors,
            layers: layers.into(),
            frames,
            tags: Vec::new(),
            frame_rate: None,
            slices: Vec::new(),
            tilesets: Arc::default(),
            foreign: Vec::new(),
            blending: Blending::Pikslar,
            layer_floor: IdFloor::default(),
            frame_floor,
            document_id: crate::ids::DocumentId::default(),
        }
        .with_floor_from_layers())
    }

    /// Raises the layer floor to the ids in use, as a document just made or opened has it.
    fn with_floor_from_layers(mut self) -> Self {
        let highest = self.layers.iter().map(|l| l.persisted_id().0).max();
        self.layer_floor.raise(highest.unwrap_or(0));
        self
    }

    /// The highest persisted layer id given while this document has been open.
    pub fn layer_floor(&self) -> u32 {
        self.layer_floor.get()
    }

    /// Takes on the ids another copy of this document has given, so none is given again.
    pub(crate) fn take_on_floor(&mut self, other: &Document) {
        self.layer_floor.raise(other.layer_floor.get());
        self.frame_floor.raise(other.frame_floor.get());
    }

    /// Gives the layers, from the bottom, the numbers a file saved for them. Refused, changing
    /// nothing, unless there is one per layer and they are distinct and not 0.
    pub fn keep_persisted_ids(&mut self, ids: &[u32]) -> Result<(), LimitError> {
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        let distinct = sorted.windows(2).all(|pair| pair[0] != pair[1]);
        let in_range = sorted.first() != Some(&0) && sorted.last() <= Some(&PersistedLayerId::MAX);
        if ids.len() != self.layers.len() || !in_range || !distinct {
            return Err(LimitError::NoSuchItem);
        }
        for (layer, &id) in self.layers.iter_mut().zip(ids) {
            layer.set_persisted_id(PersistedLayerId(id));
        }
        self.layer_floor.raise(sorted.last().copied().unwrap_or(0));
        Ok(())
    }

    pub fn new(width: usize, height: usize, mode: ColorMode) -> Result<Self, LimitError> {
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(LimitError::Dimensions);
        }
        let mut frame_floor = IdFloor::default();
        frame_floor.raise(1);
        let mut doc = Document {
            width,
            height,
            colors: Arc::new(ColorTable::new()),
            layers: Default::default(),
            frames: vec![FrameInfo::new(100).renewed(1)],
            tags: Vec::new(),
            frame_rate: None,
            slices: Vec::new(),
            tilesets: Arc::default(),
            foreign: Vec::new(),
            blending: Blending::Pikslar,
            layer_floor: IdFloor::default(),
            frame_floor,
            document_id: crate::ids::DocumentId::default(),
        };
        doc.add_layer(0, mode)?;
        Ok(doc)
    }

    pub(crate) fn blank_cel(&self) -> Cel {
        Cel::blank()
    }

    /// Every layer, from the bottom.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// Layers to adjust while building a document. An open document changes through commands.
    pub fn layers_mut(&mut self) -> &mut [Layer] {
        self.layers.as_mut_slice()
    }

    /// Replaces one cel with a canvas-sized image, a drawing of its own, so it leaves its link.
    pub fn set_cel(&mut self, layer: usize, frame: usize, grid: Grid) -> Result<(), LimitError> {
        if grid.width() != self.width || grid.height() != self.height {
            return Err(LimitError::Dimensions);
        }
        self.put_cel(layer, frame, Cel::new(grid))
    }

    /// Replaces one cel with another placed within the offset range, which may hold pixels past
    /// the canvas. A cel showing the same drawing keeps its link, at its own place; a cel with
    /// another drawing leaves its link, which ends if that leaves it one cel. So the cels of a
    /// link always show one drawing.
    pub fn put_cel(&mut self, layer: usize, frame: usize, cel: Cel) -> Result<(), LimitError> {
        if !placeable(&cel) {
            return Err(LimitError::Dimensions);
        }
        self.put_cel_in_link(layer, frame, cel)
    }

    /// Sets one layer's holds from ids of any size, then makes each hold really one drawing.
    /// Equal ids on two layers are two holds, so these are numbered clear of every other layer.
    pub fn set_holds(&mut self, layer: usize, ids: &[u32]) -> Result<(), LimitError> {
        let used = self
            .layers
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != layer)
            .flat_map(|(_, other)| other.links())
            .max()
            .unwrap_or(0);
        self.layers
            .get_mut(layer)
            .ok_or(LimitError::NoSuchItem)?
            .set_holds(ids, used)?;
        // The layer's old ids may have been links with other layers, now left with one cel.
        self.tidy_holds();
        Ok(())
    }

    /// Inserts a blank layer at `index`, counted from the bottom.
    pub fn add_layer(&mut self, index: usize, mode: ColorMode) -> Result<(), LimitError> {
        let name = format!("Layer {}", self.layers.len() + 1);
        self.insert_blank_layer(index, Layer::new(name, mode))
    }

    /// Inserts a layer of every frame blank at `index`, which may be of any kind but a group.
    pub(crate) fn insert_blank_layer(
        &mut self,
        index: usize,
        mut layer: Layer,
    ) -> Result<(), LimitError> {
        if self.layers.len() >= MAX_LAYERS {
            return Err(LimitError::TooManyLayers);
        }
        let blank = self.blank_cel();
        layer.set_persisted_id(self.new_persisted_id());
        let index = index.min(self.layers.len());
        layer.depth = self.depth_for(index);
        (0..self.frames.len()).for_each(|_| layer.push_cel(blank.clone()));
        self.layers.insert(index, layer);
        Ok(())
    }

    /// One past the largest number given while the document has been open, so a removed
    /// layer's number is never given again; once those run out, the smallest free one.
    pub(crate) fn new_persisted_id(&mut self) -> PersistedLayerId {
        let taken = |id: &u32| self.layers.iter().any(|l| l.persisted_id().0 == *id);
        let last = self.layers.iter().map(|l| l.persisted_id().0).max();
        let highest = last.unwrap_or(0).max(self.layer_floor.get());
        let next = Some(highest.saturating_add(1)).filter(|&id| id <= PersistedLayerId::MAX);
        // At most 256 layers, so one of the first 257 numbers is free.
        let free = || (1..).find(|id| !taken(id)).unwrap_or(1);
        let id = next.unwrap_or_else(free);
        self.layer_floor.raise(id);
        PersistedLayerId(id)
    }

    /// Removes one row, for tests; commands remove a group with its layers through
    /// `remove_block`.
    #[cfg(test)]
    pub(crate) fn remove_layer(&mut self, index: usize) -> Result<(), LimitError> {
        if self.layers.len() <= 1 {
            return Err(LimitError::LastLayer);
        }
        if index >= self.layers.len() {
            return Err(LimitError::NoSuchItem);
        }
        self.layers.remove(index);
        self.tidy_holds();
        Ok(())
    }

    /// Inserts a frame at `index`, copying the frame before it when `copy` is set.
    pub fn add_frame(&mut self, index: usize, copy: bool) -> Result<(), LimitError> {
        if self.frames.len() >= MAX_FRAMES {
            return Err(LimitError::TooManyFrames);
        }
        let at = index.min(self.frames.len());
        let source = at.saturating_sub(1);
        let blank = self.blank_cel();
        for layer in &mut self.layers {
            // A held layer stays held, so a background follows the animation as it grows.
            let link = layer.link(source);
            let cel = if copy || link != 0 {
                layer.cel(source).clone()
            } else {
                blank.clone()
            };
            layer.insert_frame(at, cel, link);
        }
        let info = self.new_frame_like(self.frames[source]);
        self.frames.insert(at, info);
        self.tags_after_insert(at);
        self.hold_references()
    }

    pub fn remove_frame(&mut self, index: usize) -> Result<(), LimitError> {
        if self.frames.len() <= 1 {
            return Err(LimitError::LastFrame);
        }
        if index >= self.frames.len() {
            return Err(LimitError::NoSuchItem);
        }
        self.frames.remove(index);
        for layer in &mut self.layers {
            layer.remove_frames(index..=index);
        }
        self.tags_after_remove(index);
        self.tidy_holds();
        Ok(())
    }

    /// Converts a layer between palette and RGBA without changing how it looks.
    pub fn set_layer_mode(&mut self, index: usize, mode: ColorMode) -> Result<(), LimitError> {
        let layer = self.layers.get(index).ok_or(LimitError::NoSuchItem)?;
        if layer.is_tilemap() {
            return Err(LimitError::TilemapMode);
        }
        let current = layer.mode;
        if current == mode {
            return Ok(());
        }
        // Converted on a copy, so a palette that overflows halfway changes nothing.
        let mut table = self.colors.clone();
        let colors = Arc::make_mut(&mut table);
        let mut converted = Vec::with_capacity(self.layers[index].frame_count());
        for cel in self.layers[index].cels() {
            let mut failed = false;
            let image = cel.image().map_cells(|cell| match mode {
                ColorMode::Rgba => colors.color(cell),
                ColorMode::Palette => colors.id_for(cell).unwrap_or_else(|_| {
                    failed = true;
                    0
                }),
            });
            if failed {
                return Err(LimitError::TableFull);
            }
            converted.push(image);
        }
        self.colors = table;
        self.unlink_from_other_layers(index);
        let layer = &mut self.layers[index];
        layer.replace_images(converted);
        layer.mode = mode;
        layer.share_held_cels();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::rgba;

    const RED: u32 = rgba(200, 10, 10, 255);

    fn doc() -> Document {
        Document::new(32, 32, ColorMode::Palette).expect("valid size")
    }

    #[test]
    fn rejects_absurd_sizes() {
        assert_eq!(
            Document::new(0, 5, ColorMode::Rgba),
            Err(LimitError::Dimensions)
        );
        assert_eq!(
            Document::new(5, MAX_DIMENSION + 1, ColorMode::Rgba),
            Err(LimitError::Dimensions)
        );
    }

    #[test]
    fn new_frames_cover_every_layer() {
        let mut d = doc();
        d.add_layer(1, ColorMode::Rgba).expect("room");
        d.add_frame(1, false).expect("room");
        assert!(d.layers.iter().all(|l| l.frame_count() == 2));
    }

    #[test]
    fn copied_frame_shares_storage_until_edited() {
        let mut d = doc();
        d.add_frame(1, true).expect("room");
        assert!(d.layers[0].cel(0).same_image(d.layers[0].cel(1)));
    }

    #[test]
    fn cannot_remove_last_layer_or_frame() {
        let mut d = doc();
        assert_eq!(d.remove_layer(0), Err(LimitError::LastLayer));
        assert_eq!(d.remove_frame(0), Err(LimitError::LastFrame));
    }

    #[test]
    fn a_cel_of_the_wrong_size_or_place_is_refused() {
        let mut d = doc();
        assert_eq!(
            d.set_cel(0, 0, Grid::new(31, 32)),
            Err(LimitError::Dimensions)
        );
        assert_eq!(
            d.set_cel(1, 0, Grid::new(32, 32)),
            Err(LimitError::NoSuchItem)
        );
        assert_eq!(
            d.set_cel(0, 1, Grid::new(32, 32)),
            Err(LimitError::NoSuchItem)
        );
    }

    #[test]
    fn a_drawing_that_reaches_past_the_canvas_is_kept_whole() {
        let mut d = Document::new(16, 16, ColorMode::Rgba).expect("valid size");
        let wide = Cel::new(Grid::from_cells(17, 1, &[1; 17]));
        d.put_cel(0, 0, wide).expect("within range");
        assert_eq!(
            d.layers[0].cel(0).get(16, 0),
            1,
            "the pixel past the edge is kept"
        );
        let moved = Cel::at(Grid::from_cells(2, 1, &[1, 1]).into_image(), -5, 40);
        d.put_cel(0, 0, moved.expect("in range"))
            .expect("within range");
        assert_eq!(d.layers[0].cel(0).get(-4, 40), 1);
    }

    #[test]
    fn building_a_document_from_its_parts_keeps_them() {
        let mut layer = Layer::new("only".into(), ColorMode::Rgba);
        layer.push_cel(Cel::new(Grid::new(4, 4)));
        let frames = vec![FrameInfo::new(50)];
        let d = Document::from_parts(4, 4, Arc::new(ColorTable::new()), vec![layer], frames)
            .expect("consistent parts");
        assert_eq!((d.width, d.height), (4, 4));
        assert_eq!(d.layers()[0].name, "only");
        assert_eq!(d.frames[0].duration_ms, 50);
    }

    #[test]
    fn parts_that_disagree_are_refused() {
        let colors = || Arc::new(ColorTable::new());
        let frames = |n| vec![FrameInfo::new(50); n];
        let layer = |size: usize| {
            let mut l = Layer::new("only".into(), ColorMode::Rgba);
            l.push_cel(Cel::new(Grid::from_cells(
                size,
                size,
                &vec![1; size * size],
            )));
            l
        };
        let build = |layers, n| Document::from_parts(4, 4, colors(), layers, frames(n)).err();
        assert_eq!(build(vec![layer(4)], 2), Some(LimitError::NoSuchItem));
        assert_eq!(
            build(vec![layer(17)], 1),
            None,
            "a drawing past the canvas is kept"
        );
        assert_eq!(build(vec![], 1), Some(LimitError::NoSuchItem));
        assert_eq!(build(vec![layer(4)], 0), Some(LimitError::NoSuchItem));
    }

    #[test]
    fn layers_are_numbered_for_saving_and_never_reuse_a_number_in_use() {
        let mut d = doc();
        d.add_layer(1, ColorMode::Rgba).expect("room");
        d.add_layer(0, ColorMode::Rgba).expect("room");
        let ids: Vec<u32> = d.layers.iter().map(|l| l.persisted_id().0).collect();
        assert_eq!(ids, [3, 1, 2], "a number stays with its layer");
        d.remove_layer(2).expect("removed");
        d.add_layer(0, ColorMode::Rgba).expect("room");
        assert_eq!(d.layers[0].persisted_id().0, 4);
    }

    #[test]
    fn parts_are_numbered_in_order_and_saved_numbers_do_not_change_equality() {
        let part = |name: &str| {
            let mut l = Layer::new(name.into(), ColorMode::Rgba);
            l.push_cel(Cel::blank());
            l
        };
        let frames = vec![FrameInfo::new(50)];
        let d = Document::from_parts(
            4,
            4,
            Arc::new(ColorTable::new()),
            vec![part("a"), part("b")],
            frames,
        )
        .expect("consistent parts");
        assert_eq!(d.layers[1].persisted_id(), PersistedLayerId(2));
        let mut other = d.layers[0].clone();
        other.set_persisted_id(PersistedLayerId(9));
        assert_eq!(other, d.layers[0]);
    }

    fn two_part_doc() -> Document {
        let part = |name: &str| {
            let mut l = Layer::new(name.into(), ColorMode::Rgba);
            l.push_cel(Cel::blank());
            l
        };
        let frames = vec![FrameInfo::new(50)];
        let colors = Arc::new(ColorTable::new());
        Document::from_parts(4, 4, colors, vec![part("a"), part("b")], frames).expect("parts")
    }

    #[test]
    fn saved_layer_numbers_are_kept_and_new_layers_number_past_them() {
        let mut d = two_part_doc();
        d.keep_persisted_ids(&[7, 3]).expect("distinct");
        let ids: Vec<u32> = d.layers.iter().map(|l| l.persisted_id().0).collect();
        assert_eq!(ids, [7, 3]);
        d.add_layer(0, ColorMode::Rgba).expect("room");
        assert_eq!(d.layers[0].persisted_id().0, 8);
    }

    #[test]
    fn a_new_layer_takes_a_free_number_when_none_is_left_above() {
        let mut d = two_part_doc();
        d.keep_persisted_ids(&[PersistedLayerId::MAX, 1])
            .expect("distinct");
        d.add_layer(0, ColorMode::Rgba).expect("room");
        assert_eq!(d.layers[0].persisted_id().0, 2);
    }

    #[test]
    fn saved_layer_numbers_must_be_distinct_nonzero_and_one_per_layer() {
        let mut d = two_part_doc();
        let over = PersistedLayerId::MAX + 1;
        for bad in [&[4, 4][..], &[0, 1], &[1], &[1, 2, 3], &[1, over]] {
            assert_eq!(
                d.keep_persisted_ids(bad),
                Err(LimitError::NoSuchItem),
                "{bad:?}"
            );
        }
        let ids: Vec<u32> = d.layers.iter().map(|l| l.persisted_id().0).collect();
        assert_eq!(ids, [1, 2], "a refusal changes nothing");
    }

    #[test]
    fn mode_conversion_keeps_a_hold_shared() {
        let mut d = Document::new(2, 1, ColorMode::Rgba).expect("valid size");
        d.add_frame(1, false).expect("room");
        d.set_cel(0, 0, Grid::from_cells(2, 1, &[RED, 0]))
            .expect("fits");
        d.hold_previous(0, 1).expect("frames");
        d.set_layer_mode(0, ColorMode::Palette).expect("fits");
        assert!(d.layers[0].cel(0).same_image(d.layers[0].cel(1)));
        assert_eq!(d.layers[0].link(1), d.layers[0].link(0));
        assert_ne!(d.layers[0].link(0), 0);
    }

    #[test]
    fn mode_conversion_round_trips_pixels() {
        let mut d = Document::new(2, 1, ColorMode::Rgba).expect("valid size");
        d.set_cel(0, 0, Grid::from_cells(2, 1, &[RED, 0]))
            .expect("fits");
        d.set_layer_mode(0, ColorMode::Palette).expect("fits");
        let ids = d.layers[0].cel(0).to_cells(d.width, d.height);
        assert_eq!(d.colors.color(ids[0]), RED);
        assert_eq!(ids[1], 0);
        d.set_layer_mode(0, ColorMode::Rgba).expect("fits");
        assert_eq!(d.layers[0].cel(0).to_cells(d.width, d.height), vec![RED, 0]);
    }
}
