//! What a random run counts.

/// How much a run really did, so a run that quietly did nothing cannot look like a pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub steps: u64,
    pub undos: u64,
    pub redos: u64,
    pub floating: u64,
    /// Times the oldest steps were gone because the history budget was reached.
    pub dropped: u64,
    /// Commands turned down for memory, each checked to have changed nothing.
    pub refused: u64,
    /// Slice changes that became steps, and slice changes or resizes under slices turned down.
    pub slices: u64,
    pub slices_refused: u64,
    /// Position tweens that became steps.
    pub tweens: u64,
    /// Links across layers that became steps.
    pub links: u64,
    /// Brushes captured, turns and paint or handle changes of one, and strokes made with a custom
    /// brush that became steps.
    pub brushes: u64,
    pub brush_turns: u64,
    pub brush_strokes: u64,
    /// Palette actions that became steps, remaps checked to change no colour, palette files
    /// loaded, saves reopened in the middle of a run and tile strokes on a palette map.
    pub palette: u64,
    pub remaps: u64,
    pub loads: u64,
    pub saves: u64,
    pub map_strokes: u64,
    /// Tilemap actions that became steps (M10), pixel strokes drawn on tiles through the canvas,
    /// terrain set commands and terrain strokes that became steps.
    pub tiles: u64,
    pub tile_pixels: u64,
    pub terrains: u64,
    pub terrain_strokes: u64,
    /// The terrain runs (11.5): cells changed by terrain strokes checked against the model's
    /// own rules, previews checked against their commits, cells pinned by hand placements,
    /// terrain tiles removed and strokes repeated on a document saved and opened again.
    pub terrain_cells: u64,
    pub previews: u64,
    pub pins: u64,
    pub removed_tiles: u64,
    pub reloaded_strokes: u64,
    /// Scrubs through history that moved, and the steps they played, each checked against the
    /// state recorded where it stopped.
    pub scrubs: u64,
    pub scrubbed_steps: u64,
    /// Changes of the document's blending that became steps.
    pub blendings: u64,
}

impl Counts {
    pub fn merge(&mut self, other: &Counts) {
        self.steps += other.steps;
        self.undos += other.undos;
        self.redos += other.redos;
        self.floating += other.floating;
        self.dropped += other.dropped;
        self.refused += other.refused;
        self.slices += other.slices;
        self.slices_refused += other.slices_refused;
        self.tweens += other.tweens;
        self.links += other.links;
        self.brushes += other.brushes;
        self.brush_turns += other.brush_turns;
        self.brush_strokes += other.brush_strokes;
        self.palette += other.palette;
        self.remaps += other.remaps;
        self.loads += other.loads;
        self.saves += other.saves;
        self.map_strokes += other.map_strokes;
        self.tiles += other.tiles;
        self.tile_pixels += other.tile_pixels;
        self.terrains += other.terrains;
        self.terrain_strokes += other.terrain_strokes;
        self.terrain_cells += other.terrain_cells;
        self.previews += other.previews;
        self.pins += other.pins;
        self.removed_tiles += other.removed_tiles;
        self.reloaded_strokes += other.reloaded_strokes;
        self.scrubs += other.scrubs;
        self.scrubbed_steps += other.scrubbed_steps;
        self.blendings += other.blendings;
    }
}
