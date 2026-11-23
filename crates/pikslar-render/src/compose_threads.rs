//! Compositing the tiles an update collected, on the calling thread or, for many tiles, on
//! scoped threads.
//!
//! An update first collects the canvas tiles it has to composite (`Compositor::pending`), then
//! composites them here. While the work, the tiles times the layers drawn in them (ghosts
//! included), stays below [`PARALLEL_WORK`], it does so on the calling thread and starts no
//! thread. From there on, it runs on at most [`MAX_THREADS`] threads, and no more than the
//! machine runs at once: the calling thread and a scoped thread for each other. The tiles are
//! cut into [`BANDS_PER_THREAD`] bands for each thread, of whole canvas tile rows with about as
//! many tiles each, and every thread takes the next band left (a shared counter) until none
//! are. A thread the system puts on a slower core so takes fewer bands, and the others do not
//! wait for it. Each band writes only the canvas rows of its own tiles. Each thread keeps a
//! tile cache of its own between updates. The threads end before the update returns, so none
//! exists between operations.
//!
//! Every tile is composited by the same function from the same document whichever thread takes
//! it, and a tile cache only holds decoded tile pixels, so the picture is the same bit for bit.

use super::{Compositor, InProgress};
use crate::onion::OnionStyle;
use crate::pixel::{blend_tile, blend_tile_in, from_screen_tile, ghost_tile, to_screen_tile, Lut};
use pikslar_core::tile::RawTile;
use pikslar_core::tilemap::TileCache;
use pikslar_core::{Blending, Document, Ghost, TILE, TILE_AREA};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// The most threads one update composites on, the calling thread included. Six, since bands
/// are shared: on a hybrid processor (6 performance and 8 efficiency cores here) the stress
/// map's window step waited 3.2 ms at the median and 3.7 ms at p95 on six, against 3.7 and
/// 4.3 ms on four, for about 0.7 ms more processor time a step. Eight waited less again but
/// spent about 1.3 ms more (`docs/tilemap-performance.md`, section 10).
pub const MAX_THREADS: usize = 6;
/// How many bands a threaded update is cut into for each thread. The threads take them in
/// turn, so one on a slower core takes fewer and the others do not wait for it. Two to eight
/// measured alike on performance cores; one (bands fixed to threads) waited longest when
/// threads landed on efficiency cores.
pub const BANDS_PER_THREAD: usize = 4;
/// The least work, in tiles times the layers drawn in each, composited on several threads; less
/// is composited on the calling thread, which then starts no thread. 4,096 layer tiles, about a
/// million layer pixels, take about 1.2 ms on one thread. Starting three threads after an idle
/// moment costs 0.35 to 0.5 ms of processor time whatever they do, so below this they save
/// little of the wait and add a third or more to the processor time; at 512, a 64 tile redraw
/// of 12 layers took longer and twice the processor time (`pikslar-bench tile-cpu`, see
/// `docs/tilemap-performance.md`, section 9). Tiles times layers predicts the time better than
/// the area does, so the area adds no rule of its own.
pub const PARALLEL_WORK: usize = 4_096;

/// What compositing a tile reads, the same for every thread.
pub(super) struct Scene<'a> {
    pub doc: &'a Document,
    pub drawn: &'a [bool],
    pub over: InProgress<'a>,
    pub now: usize,
    pub ghosts: &'a [Ghost],
    pub lut: &'a Lut,
    pub style: &'a OnionStyle,
    /// Premultiplied for the screen. Only Aseprite-compatible blending, which builds a
    /// straight picture, reads it: its exports take the straight one.
    pub screen: bool,
}

/// The canvas, in pixels and tiles.
#[derive(Clone, Copy)]
struct Grid {
    width: usize,
    height: usize,
    tiles_wide: usize,
}

/// One neighbouring frame of `now` over `out`. Layers holding the current drawing add nothing.
/// `STRAIGHT` is the document's blending, Aseprite-compatible when true, made a constant so
/// that Pikslar's ghost loop is compiled as it was, with no test of the blending in it.
fn blend_ghost<const STRAIGHT: bool>(
    scene: &Scene,
    cache: &mut TileCache,
    out: &mut RawTile,
    (tx, ty): (usize, usize),
    ghost: Ghost,
) {
    let mut flat = [0u32; TILE_AREA];
    let mut any = false;
    // A reference shows one drawing on every frame, so like any hold it leaves no ghost.
    let layers = scene.doc.layers().iter().enumerate();
    let shown = |i: &usize| scene.drawn.get(*i).copied().unwrap_or(false);
    let mut raw = [0u32; TILE_AREA];
    for (_, layer) in layers.filter(|(i, _)| shown(i)) {
        let at = (layer, ghost.frame);
        if layer.cel(ghost.frame).looks_the_same(layer.cel(scene.now))
            || !scene.doc.layer_tile_cached(at, (tx, ty), &mut raw, cache)
        {
            continue;
        }
        if STRAIGHT {
            blend_tile_in(Blending::Aseprite, &mut flat, &raw, layer, scene.lut);
        } else {
            blend_tile(&mut flat, &raw, layer, scene.lut);
        }
        any = true;
    }
    if any {
        // A ghost is drawn on the screen's picture, whichever blending built it.
        if STRAIGHT {
            to_screen_tile(&mut flat);
        }
        ghost_tile(out, &flat, ghost.distance, scene.style);
    }
}

/// Canvas tile `index` composited: ghosts first, then the layers of the frame.
pub(super) fn compose_one(
    scene: &Scene,
    cache: &mut TileCache,
    (tx, ty, index): (usize, usize, usize),
) -> RawTile {
    if scene.doc.blending == Blending::Aseprite {
        return compose_one_straight(scene, cache, (tx, ty, index));
    }
    let mut out = [0u32; TILE_AREA];
    for ghost in scene.ghosts {
        blend_ghost::<false>(scene, cache, &mut out, (tx, ty), *ghost);
    }
    let kit = (scene.lut, cache);
    let at = (scene.doc, scene.now, scene.drawn);
    super::blend_layers(&mut out, at, scene.over, (tx, ty, index), kit);
    out
}

/// [`compose_one`] for Aseprite-compatible blending: the ghosts, drawn on the screen's
/// picture, made straight for the frame's layers to be laid on, and the result made
/// premultiplied again for the screen. Out of line, so Pikslar's path is compiled as it was.
#[inline(never)]
fn compose_one_straight(
    scene: &Scene,
    cache: &mut TileCache,
    (tx, ty, index): (usize, usize, usize),
) -> RawTile {
    let mut out = [0u32; TILE_AREA];
    for ghost in scene.ghosts {
        blend_ghost::<true>(scene, cache, &mut out, (tx, ty), *ghost);
    }
    if !scene.ghosts.is_empty() {
        from_screen_tile(&mut out);
    }
    let kit = (scene.lut, cache);
    let at = (scene.doc, scene.now, scene.drawn);
    super::blend_layers(&mut out, at, scene.over, (tx, ty, index), kit);
    if scene.screen {
        to_screen_tile(&mut out);
    }
    out
}

/// Writes tile `(tx, ty)` into `rows`, the canvas rows from tile row `first_row` on.
fn store(rows: &mut [u32], grid: Grid, first_row: usize, (tx, ty): (usize, usize), out: &RawTile) {
    let (x0, y0) = (tx * TILE, ty * TILE);
    let columns = TILE.min(grid.width - x0);
    let top = first_row * TILE;
    for y in 0..TILE.min(grid.height - y0) {
        let start = (y0 + y - top) * grid.width + x0;
        rows[start..start + columns].copy_from_slice(&out[y * TILE..y * TILE + columns]);
    }
}

/// Composites `tiles` (indices, by rows) into `rows`, which start at tile row `first_row`.
fn compose_band(
    scene: &Scene,
    cache: &mut TileCache,
    (grid, first_row): (Grid, usize),
    tiles: &[u32],
    rows: &mut [u32],
) {
    for &index in tiles {
        let index = index as usize;
        let (tx, ty) = (index % grid.tiles_wide, index / grid.tiles_wide);
        let out = compose_one(scene, cache, (tx, ty, index));
        store(rows, grid, first_row, (tx, ty), &out);
    }
}

/// `tiles`, sorted, cut into at most `count` runs of whole tile rows with about as many tiles
/// each, as (first tile row, past-the-last tile row, the run's tiles).
pub(super) fn bands(tiles: &[u32], tiles_wide: usize, count: usize) -> Vec<(usize, usize, &[u32])> {
    let row = |i: usize| tiles[i] as usize / tiles_wide;
    let mut out = Vec::with_capacity(count.min(tiles.len()));
    let mut start = 0;
    while start < tiles.len() {
        let left = count.saturating_sub(out.len()).max(1);
        let mut end = (start + (tiles.len() - start).div_ceil(left)).min(tiles.len());
        while end < tiles.len() && row(end) == row(end - 1) {
            end += 1;
        }
        out.push((row(start), row(end - 1) + 1, &tiles[start..end]));
        start = end;
    }
    out
}

/// How many threads an update of `work` (tiles times the layers drawn in each, ghosts
/// included) composites on, given at most `threads` and the least work that uses more than one.
pub(super) fn threads_for(work: usize, threads: usize, min_work: usize) -> usize {
    if work >= min_work {
        threads.max(1)
    } else {
        1
    }
}

/// How many threads an update may composite on: the machine's, at most [`MAX_THREADS`].
pub(super) fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_THREADS)
}

/// The canvas rows of tile rows `first` up to `past`, from `px`, which starts at tile row
/// `taken`.
fn band_rows(
    px: &mut [u32],
    grid: Grid,
    (taken, first, past): (usize, usize, usize),
) -> (&mut [u32], &mut [u32]) {
    let skip = (first - taken) * TILE * grid.width;
    let rows = ((past - first) * TILE).min(grid.height - first * TILE) * grid.width;
    let (_, after) = px.split_at_mut(skip);
    after.split_at_mut(rows)
}

/// One band left to composite: its first tile row, its tiles and its canvas rows. Whichever
/// thread draws its number takes it out, so each band is composited once.
type Job<'a> = Mutex<Option<(usize, &'a [u32], &'a mut [u32])>>;

/// `px` cut into the canvas rows of each of `parts`, as jobs.
fn jobs<'a>(grid: Grid, parts: &[(usize, usize, &'a [u32])], px: &'a mut [u32]) -> Vec<Job<'a>> {
    let mut rest = px;
    let mut taken = 0;
    let mut jobs = Vec::with_capacity(parts.len());
    for &(first, past, tiles) in parts {
        let (band, after) = band_rows(rest, grid, (taken, first, past));
        rest = after;
        taken = past;
        jobs.push(Mutex::new(Some((first, tiles, band))));
    }
    jobs
}

/// Takes the next band from `next` and composites it, until none are left.
fn take_bands(
    scene: &Scene,
    cache: &mut TileCache,
    grid: Grid,
    (jobs, next): (&[Job], &AtomicUsize),
) {
    while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
        let taken = job.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some((first, tiles, band)) = taken {
            compose_band(scene, cache, (grid, first), tiles, band);
        }
    }
}

/// Composites `parts` (from [`bands`]) into `px` on `threads` threads: the calling thread with
/// `cache` and scoped threads with the caches of `workers`. Each takes the next band left, so
/// a thread on a slower core takes fewer bands and nobody waits long for it.
fn compose_on_threads(
    shared: &Scene,
    (grid, threads): (Grid, usize),
    parts: &[(usize, usize, &[u32])],
    px: &mut [u32],
    (cache, workers): (&mut TileCache, &mut Vec<TileCache>),
) {
    let helpers = threads.min(parts.len()).saturating_sub(1);
    while workers.len() < helpers {
        workers.push(TileCache::new());
    }
    let jobs = jobs(grid, parts, px);
    let next = AtomicUsize::new(0);
    let shares = (jobs.as_slice(), &next);
    std::thread::scope(|scope| {
        for cache in workers.iter_mut().take(helpers) {
            scope.spawn(move || take_bands(shared, cache, grid, shares));
        }
        take_bands(shared, cache, grid, shares);
    });
}

impl Compositor {
    /// Composites the tiles collected in `pending` and empties it. Below the threshold this
    /// allocates nothing and starts no thread.
    pub(super) fn compose_pending(
        &mut self,
        scene: (&Document, &[bool]),
        over: InProgress<'_>,
        (now, ghosts): (usize, &[Ghost]),
    ) {
        if self.pending.is_empty() {
            return;
        }
        let mut pending = std::mem::take(&mut self.pending);
        pending.sort_unstable();
        let grid = Grid {
            width: self.width,
            height: self.height,
            tiles_wide: self.tiles_wide,
        };
        let shared = Scene {
            doc: scene.0,
            drawn: scene.1,
            over,
            now,
            ghosts,
            lut: &self.lut,
            style: &self.onion_style,
            screen: true,
        };
        let layers = scene.1.iter().filter(|&&drawn| drawn).count().max(1);
        let work = pending.len() * layers * (1 + ghosts.len());
        let threads = threads_for(work, self.threads, self.parallel_work);
        let parts = match threads {
            1 => Vec::new(),
            _ => {
                let count = threads.saturating_mul(self.bands_per_thread);
                bands(&pending, grid.tiles_wide, count)
            }
        };
        if parts.len() > 1 {
            let caches = (&mut self.tiles, &mut self.workers);
            compose_on_threads(&shared, (grid, threads), &parts, &mut self.px, caches);
        } else {
            // One band, every pending row: no thread is started and nothing allocated.
            let row = |i: u32| i as usize / grid.tiles_wide;
            let (first, past) = (row(pending[0]), row(pending[pending.len() - 1]) + 1);
            let (band, _) = band_rows(&mut self.px, grid, (0, first, past));
            compose_band(&shared, &mut self.tiles, (grid, first), &pending, band);
        }
        pending.clear();
        self.pending = pending;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_redraws_stay_on_the_calling_thread_and_large_ones_go_on_threads() {
        // The paint stress at zoom 4: 64 canvas tiles of 12 layers. Threads made it slower and
        // doubled its processor time (`pikslar-bench tile-cpu`).
        assert_eq!(threads_for(64 * 12, 4, PARALLEL_WORK), 1);
        // The realistic map fitted: 256 animated tiles of four layers.
        assert_eq!(threads_for(256 * 4, 4, PARALLEL_WORK), 1);
        // The stress map at zoom 1: 3,400 tiles of four layers.
        assert_eq!(threads_for(3_400 * 4, 4, PARALLEL_WORK), 4);
        assert_eq!(threads_for(PARALLEL_WORK, 3, PARALLEL_WORK), 3);
        assert_eq!(threads_for(PARALLEL_WORK - 1, 4, PARALLEL_WORK), 1);
        assert_eq!(threads_for(usize::MAX, 1, PARALLEL_WORK), 1);
        assert_eq!(
            threads_for(0, 4, 0),
            4,
            "a threshold of 0 always uses threads"
        );
    }

    #[test]
    fn bands_are_whole_rows_with_about_as_many_tiles_each() {
        // A canvas 5 tiles wide: rows 0 to 3 whole, then two tiles of row 4.
        let tiles: Vec<u32> = (0..20).chain([21, 23]).collect();
        let parts = bands(&tiles, 5, 3);
        assert_eq!(parts.len(), 3);
        let mut next = 0;
        for &(first, past, run) in &parts {
            assert_eq!(first, run[0] as usize / 5, "starts on its first tile's row");
            assert_eq!(past, *run.last().expect("tiles") as usize / 5 + 1);
            assert!(first >= next, "rows in order, none shared");
            next = past;
        }
        let all: Vec<u32> = parts.iter().flat_map(|p| p.2.iter().copied()).collect();
        assert_eq!(all, tiles);
        assert_eq!(bands(&tiles, 5, 1).len(), 1);
        assert_eq!(bands(&[7], 5, 4), vec![(1, 2, &[7u32][..])]);
        assert!(bands(&[], 5, 4).is_empty());
        // More threads than rows: one band a row.
        assert_eq!(bands(&[0, 1, 5], 5, 4).len(), 2);
    }

    #[test]
    fn shared_bands_are_several_a_thread_and_at_most_one_a_row() {
        // 20 whole rows of 5 tiles, cut for 4 threads at 4 bands each: 16 bands of whole rows.
        let tiles: Vec<u32> = (0..100).collect();
        let parts = bands(&tiles, 5, 4 * BANDS_PER_THREAD);
        assert_eq!(parts.len(), 4 * BANDS_PER_THREAD);
        let sizes: Vec<usize> = parts.iter().map(|p| p.2.len()).collect();
        assert!(sizes.iter().all(|&n| n % 5 == 0 && (5..=10).contains(&n)));
        // Asking for any number of bands, however large, gives one a row and allocates as much.
        let parts = bands(&tiles, 5, usize::MAX);
        assert_eq!(parts.len(), 20);
        assert!(parts.iter().all(|&(first, past, _)| past == first + 1));
    }
}
