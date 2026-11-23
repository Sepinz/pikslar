//! Flattens the layers of one frame into a cached image, one tile at a time.

use crate::onion::OnionStyle;
use crate::pixel::{blend_tile_in, to_screen_tile, Lut};
use crate::view::Rect;
use pikslar_core::tile::RawTile;
use pikslar_core::tilemap::{TileCache, TileEdit};
use pikslar_core::{
    onion_ghosts, Blending, ColorTable, Damage, Document, Editor, Ghost, Layer, Overlay, Span,
    TILE, TILE_AREA,
};

#[path = "compose_threads.rs"]
mod compose_threads;
pub use compose_threads::{BANDS_PER_THREAD, MAX_THREADS, PARALLEL_WORK};
#[path = "ghost_shares.rs"]
mod ghost_shares;
#[path = "visible.rs"]
mod visible;
pub use visible::shown_region;
use visible::TileSpan;

/// The stroke or floating pixels in progress, and the layer they belong to.
pub(crate) type InProgress<'a> = Option<(usize, &'a Overlay)>;

pub struct Compositor {
    width: usize,
    height: usize,
    tiles_wide: usize,
    tiles_high: usize,
    px: Vec<u32>,
    pub(crate) lut: Box<Lut>,
    lut_version: u64,
    seen: Vec<u32>,
    stamp: u32,
    /// Shows neighbouring frames faintly beneath the current one.
    pub onion: bool,
    /// How many frames to show each way.
    pub onion_depth: usize,
    /// The loop being worked on. Its ends count as neighbours.
    pub onion_loop: Option<Span>,
    /// Which neighbours show, and how.
    pub onion_style: OnionStyle,
    /// Ghosts would only blur an animation in motion.
    pub playing: bool,
    /// Which layers the screen draws, kept so that no frame makes a new list.
    drawn: Vec<bool>,
    /// The pixels of the tiles the maps place, decoded once and kept while they last.
    tiles: TileCache,
    /// The canvas tiles on screen, as first and past-the-last column and row; `None` for all.
    shown: Option<TileSpan>,
    /// Tiles that changed while off screen, composited once they show (`visible.rs`).
    stale: Vec<bool>,
    stale_count: usize,
    /// The document area around the tiles marked stale since [`Compositor::take_deferred`].
    deferred: Rect,
    /// Which layers each ghost left out, to find ghosts an edit shows or hides whole.
    shares: ghost_shares::GhostShares,
    /// Shown tiles an update composites at its end, perhaps on several threads.
    pending: Vec<u32>,
    /// The tile caches of the threads other than the calling one.
    workers: Vec<TileCache>,
    /// The most threads an update composites on, and the least work that uses more than one.
    threads: usize,
    parallel_work: usize,
    /// How many bands a threaded update is cut into for each thread; the threads share them.
    bands_per_thread: usize,
}

impl Compositor {
    pub fn new(width: usize, height: usize) -> Self {
        let tiles_wide = width.div_ceil(TILE);
        let tiles_high = height.div_ceil(TILE);
        Compositor {
            width,
            height,
            tiles_wide,
            tiles_high,
            px: vec![0; width * height],
            lut: Box::new([0; 65536]),
            lut_version: 0,
            seen: vec![0; tiles_wide * tiles_high],
            stamp: 0,
            onion: false,
            onion_depth: 1,
            onion_loop: None,
            onion_style: OnionStyle::default(),
            playing: false,
            drawn: Vec::new(),
            tiles: TileCache::new(),
            shown: None,
            stale: vec![false; tiles_wide * tiles_high],
            stale_count: 0,
            deferred: Rect::default(),
            shares: Default::default(),
            pending: Vec::new(),
            workers: Vec::new(),
            threads: compose_threads::default_threads(),
            parallel_work: PARALLEL_WORK,
            bands_per_thread: BANDS_PER_THREAD,
        }
    }

    /// A compositor for a canvas of another size, with this one's onion skin settings. It
    /// composites everything again on its first update.
    pub fn resized(&self, width: usize, height: usize) -> Self {
        Compositor {
            onion: self.onion,
            onion_depth: self.onion_depth,
            onion_loop: self.onion_loop,
            onion_style: self.onion_style,
            playing: self.playing,
            threads: self.threads,
            parallel_work: self.parallel_work,
            bands_per_thread: self.bands_per_thread,
            ..Compositor::new(width, height)
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Bytes of decoded map tile pixels kept for compositing tilemap layers, by every thread.
    pub fn tile_cache_bytes(&self) -> usize {
        let workers: usize = self.workers.iter().map(TileCache::held_bytes).sum();
        self.tiles.held_bytes() + workers
    }

    /// Composites on at most `threads` threads, and on more than one only from `min_work` on,
    /// in tiles times the layers drawn in them, ghosts included. The defaults are the machine's threads up to [`MAX_THREADS`], and
    /// [`PARALLEL_WORK`]. Measurements and tests set it; the program keeps the defaults.
    pub fn set_threads(&mut self, threads: usize, min_work: usize) {
        self.threads = threads.max(1);
        self.parallel_work = min_work;
    }

    /// Cuts a threaded update into `per_thread` bands for each thread (at least one, and never
    /// more than one a tile row), which the threads take in turn until none are left. The
    /// default is [`BANDS_PER_THREAD`]. Measurements and tests set it.
    pub fn set_bands_per_thread(&mut self, per_thread: usize) {
        self.bands_per_thread = per_thread.max(1);
    }

    /// The flattened image, as premultiplied BGRA.
    pub fn pixels(&self) -> &[u32] {
        &self.px
    }

    pub(crate) fn refresh_lut(&mut self, editor: &Editor, force: bool) {
        self.refresh_lut_from(&editor.doc().colors, force);
    }

    pub(crate) fn refresh_lut_from(&mut self, table: &ColorTable, force: bool) {
        let version = table.version();
        if force || version != self.lut_version {
            let colors = table.colors();
            self.lut[..colors.len()].copy_from_slice(colors);
            self.lut[colors.len()..].fill(0);
            self.lut_version = version;
        }
    }

    /// The frames to show faintly behind the current one, farthest first.
    pub(crate) fn ghosts(&self, editor: &Editor) -> Vec<Ghost> {
        if !self.onion || self.playing {
            return Vec::new();
        }
        let frames = editor.doc().frames.len();
        let (before, after) = self.onion_style.reach(self.onion_depth);
        onion_ghosts(editor.frame, frames, before, after, self.onion_loop)
    }

    /// Recomposites whatever changed and appends the changed document areas to `changed`.
    pub fn update(&mut self, editor: &Editor, damage: Damage, changed: &mut Vec<Rect>) {
        let ghosts = self.ghosts(editor);
        let (doc, now, over) = (editor.doc(), editor.frame, editor.overlay());
        let mut drawn = std::mem::take(&mut self.drawn);
        match damage {
            Damage::None if self.stale_count == 0 => drawn.clear(),
            _ => doc.screen_layers_into(&mut drawn),
        }
        let redrawn_all = matches!(damage, Damage::All);
        match damage {
            Damage::None => {
                self.refresh_lut(editor, false);
                self.stamp = self.stamp.wrapping_add(1);
            }
            Damage::All => {
                self.refresh_lut(editor, true);
                for index in 0..self.seen.len() {
                    self.compose_or_defer(index);
                }
                changed.push(Rect::new(0, 0, self.width as i32, self.height as i32));
            }
            Damage::Tiles(tiles) => {
                self.refresh_lut(editor, false);
                self.stamp = self.stamp.wrapping_add(1);
                for tile in tiles {
                    let index = tile as usize;
                    if index < self.seen.len() && self.seen[index] != self.stamp {
                        self.seen[index] = self.stamp;
                        if self.compose_or_defer(index) {
                            changed.push(self.tile_rect(index));
                        }
                    }
                }
            }
            Damage::Placed(edit) => {
                self.refresh_lut(editor, false);
                self.compose_placed(&edit, (doc, &drawn), (now, &ghosts), changed);
            }
        }
        if !drawn.is_empty() {
            let scene = (doc, drawn.as_slice());
            self.redraw_changed_ghosts(scene, (now, &ghosts), redrawn_all, changed);
        }
        let span = self.shown_span();
        self.catch_up(span, changed);
        self.compose_pending((doc, &drawn), over, (now, &ghosts));
        self.drawn = drawn;
    }

    /// Composites the canvas tiles where a tile edit shows: the maps of the frame shown, and
    /// of the frames the onion skin draws faintly beneath it, which place the tile too.
    fn compose_placed(
        &mut self,
        edit: &TileEdit,
        (doc, drawn): (&Document, &[bool]),
        (now, ghosts): (usize, &[Ghost]),
        changed: &mut Vec<Rect>,
    ) {
        self.stamp = self.stamp.wrapping_add(1);
        let frames = std::iter::once(now).chain(ghosts.iter().map(|g| g.frame));
        for frame in frames {
            doc.placed_canvas_tiles(edit, frame, drawn, |index| {
                if index < self.seen.len() && self.seen[index] != self.stamp {
                    self.seen[index] = self.stamp;
                    if self.compose_or_defer(index) {
                        changed.push(self.tile_rect(index));
                    }
                }
            });
        }
    }

    #[inline]
    fn tile_position(&self, index: usize) -> (usize, usize) {
        (index % self.tiles_wide, index / self.tiles_wide)
    }

    fn tile_rect(&self, index: usize) -> Rect {
        let (x, y) = (
            (index % self.tiles_wide * TILE) as i32,
            (index / self.tiles_wide * TILE) as i32,
        );
        Rect::new(x, y, TILE as i32, TILE as i32).intersect(Rect::new(
            0,
            0,
            self.width as i32,
            self.height as i32,
        ))
    }

    /// True while neighbouring frames are drawn into [`Self::pixels`].
    pub fn ghosting(&self) -> bool {
        self.onion && !self.playing
    }

    /// Composites part of the current frame, with work in progress but without onion skin, into
    /// `out` as premultiplied BGRA rows. `region` is x, y, width and height in document pixels and
    /// must lie inside the document; only the tiles it touches are composited.
    pub fn compose_plain(
        &mut self,
        editor: &Editor,
        (x0, y0, w, h): (usize, usize, usize, usize),
        out: &mut Vec<u32>,
    ) {
        self.refresh_lut(editor, false);
        out.clear();
        out.resize(w * h, 0);
        if w == 0 || h == 0 || x0 + w > self.width || y0 + h > self.height {
            return;
        }
        let (doc, frame, over) = (editor.doc(), editor.frame, editor.overlay());
        let drawn = doc.screen_layers();
        for ty in y0 / TILE..=(y0 + h - 1) / TILE {
            for tx in x0 / TILE..=(x0 + w - 1) / TILE {
                let mut tile = [0u32; TILE_AREA];
                let index = ty * self.tiles_wide + tx;
                let spot = (tx, ty, index);
                let kit = (&*self.lut, &mut self.tiles);
                blend_layers(&mut tile, (doc, frame, &drawn), over, spot, kit);
                if doc.blending == Blending::Aseprite {
                    to_screen_tile(&mut tile);
                }
                copy_overlap(&tile, (tx * TILE, ty * TILE), (x0, y0, w, h), out);
            }
        }
    }

    /// Composites a whole frame for export: no onion skin, no reference layers, and the color
    /// table already current. A document of Aseprite-compatible blending is left straight, as
    /// that blending builds it, so the export loses nothing to premultiplying.
    pub(crate) fn compose_frame(&mut self, doc: &Document, over: InProgress<'_>, frame: usize) {
        let picture = doc.picture_layers();
        for index in 0..self.seen.len() {
            self.compose_tile((doc, &picture), over, index, frame, &[]);
        }
    }

    /// `over` is drawn on `frame`; callers pass it only for the frame being edited. Only the
    /// layers `drawn` takes, by position from the bottom, are drawn.
    fn compose_tile(
        &mut self,
        (doc, drawn): (&Document, &[bool]),
        over: InProgress<'_>,
        index: usize,
        frame: usize,
        ghosts: &[Ghost],
    ) {
        let (tx, ty) = self.tile_position(index);
        let scene = compose_threads::Scene {
            doc,
            drawn,
            over,
            now: frame,
            ghosts,
            lut: &self.lut,
            style: &self.onion_style,
            screen: false,
        };
        let out = compose_threads::compose_one(&scene, &mut self.tiles, (tx, ty, index));
        self.store(index, &out);
    }

    fn store(&mut self, index: usize, out: &RawTile) {
        debug_assert!(index / self.tiles_wide < self.tiles_high);
        let (x0, y0) = (
            index % self.tiles_wide * TILE,
            index / self.tiles_wide * TILE,
        );
        let columns = TILE.min(self.width - x0);
        for y in 0..TILE.min(self.height - y0) {
            let start = (y0 + y) * self.width + x0;
            self.px[start..start + columns].copy_from_slice(&out[y * TILE..y * TILE + columns]);
        }
    }
}

/// Copies the part of a tile at document position `at` that falls inside `region` into `out`,
/// which holds `region` row by row.
fn copy_overlap(
    tile: &RawTile,
    at: (usize, usize),
    (x0, y0, w, h): (usize, usize, usize, usize),
    out: &mut [u32],
) {
    let (left, right) = (at.0.max(x0), (at.0 + TILE).min(x0 + w));
    let (top, bottom) = (at.1.max(y0), (at.1 + TILE).min(y0 + h));
    for y in top..bottom {
        let from = (y - at.1) * TILE + (left - at.0);
        let to = (y - y0) * w + (left - x0);
        out[to..to + right - left].copy_from_slice(&tile[from..from + right - left]);
    }
}

/// Blends the layers of `frame` that `drawn` takes, by position from the bottom, into canvas
/// tile (`tx`, `ty`), whose number is `index`. `over` is drawn on its layer.
pub(crate) fn blend_layers(
    out: &mut RawTile,
    (doc, frame, drawn): (&Document, usize, &[bool]),
    over: InProgress<'_>,
    (tx, ty, index): (usize, usize, usize),
    (lut, cache): (&Lut, &mut TileCache),
) {
    let include = |i: usize, _: &Layer| drawn.get(i).copied().unwrap_or(false);
    let at = (tx, ty, index);
    blend_chosen(out, (doc, frame), over, at, (lut, include), Some(cache));
}

/// [`blend_layers`] for the layers `include` takes, bottom up, whether shown or hidden.
pub(crate) fn blend_chosen(
    out: &mut RawTile,
    (doc, frame): (&Document, usize),
    over: InProgress<'_>,
    (tx, ty, index): (usize, usize, usize),
    (lut, include): (&Lut, impl Fn(usize, &Layer) -> bool),
    mut cache: Option<&mut TileCache>,
) {
    let layers = doc.layers().iter().enumerate();
    // Every layer drawn fills `raw` whole, so it is cleared only for a stroke over a blank tile.
    let mut raw = [0u32; TILE_AREA];
    for (position, layer) in layers.filter(|(i, l)| include(*i, l)) {
        let over_tile = over
            .filter(|(l, _)| *l == position)
            .and_then(|(_, o)| o.tile(index));
        let drawn = match cache.as_deref_mut() {
            Some(cache) => doc.layer_tile_cached((layer, frame), (tx, ty), &mut raw, cache),
            None => doc.layer_tile(layer, frame, tx, ty, &mut raw),
        };
        if !drawn {
            if over_tile.is_none() {
                continue;
            }
            raw.fill(0);
        }
        if let Some(tile) = over_tile {
            tile.apply(&mut raw);
        }
        blend_tile_in(doc.blending, out, &raw, layer, lut);
    }
}

#[cfg(test)]
#[path = "compose_tests.rs"]
mod tests;
