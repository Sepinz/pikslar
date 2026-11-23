//! Feeds damaged files to every parser and holds each to memory and time limits.

use crate::craft;
use crate::deep_use::exercise;
use crate::mutate::{mutate, Rng};
use crate::tracking::{measure, Usage};
use pikslar_core::{ColorMode, Document, Editor, Tool};
use pikslar_core::{DrawCmd, FloatingCmd};
use pikslar_format::brush_lib;
use pikslar_format::palette_file::{self, PaletteFormat};
use pikslar_format::piks::{self, Recovered};
use pikslar_format::png::{self, Image};
use pikslar_format::{deflate, inflate};
use std::cell::Cell;
use std::ops::Range;
use std::time::{Duration, Instant};

/// Longest any one parse of these small files may take.
pub const MAX_PARSE: Duration = Duration::from_millis(250);
/// Longest one read of a brush library may take. A library is at most 16 MiB.
pub const MAX_LIBRARY_READ: Duration = Duration::from_millis(100);
/// Most a read of a brush library may hold at once.
pub const LIBRARY_PEAK_LIMIT: usize = 32 * MEGABYTE;
/// Longest one read of a palette file may take. A palette file is at most 4 MiB.
pub const MAX_PALETTE_READ: Duration = Duration::from_millis(100);
/// Most a read of a palette file may hold at once.
pub const PALETTE_PEAK_LIMIT: usize = 32 * MEGABYTE;
const MEGABYTE: usize = 1 << 20;
/// Most an import of a damaged Aseprite file may hold at once, as `pikslar-format`'s own
/// robustness test allows.
pub const ASE_PEAK_LIMIT: usize = 192 * MEGABYTE;
/// `inflate` is asked for at most this much: past the mebibyte at which a stop is asked inside
/// a block, so that the stop checks reach it.
const INFLATE_LIMIT: usize = 3 * MEGABYTE;
/// Larger pictures are only drawn and edited now and then, to keep runs short.
const ROUTINE_AREA: usize = 1 << 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Project,
    Recovery,
    Png,
    Inflate,
    BrushLibrary,
    PaletteGpl,
    PaletteJasc,
    PaletteHex,
    TileProject,
    AseTiles,
    /// Project files whose terrain section, `Atil`, is damaged (M11).
    Terrain,
}

impl Target {
    pub const ALL: [Target; 11] = [
        Target::Project,
        Target::Recovery,
        Target::Png,
        Target::Inflate,
        Target::BrushLibrary,
        Target::PaletteGpl,
        Target::PaletteJasc,
        Target::PaletteHex,
        Target::TileProject,
        Target::AseTiles,
        Target::Terrain,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Target::Project => "project",
            Target::Recovery => "recovery",
            Target::Png => "png",
            Target::Inflate => "inflate",
            Target::BrushLibrary => "brushlib",
            Target::PaletteGpl => "gpl",
            Target::PaletteJasc => "jasc",
            Target::PaletteHex => "hex",
            Target::TileProject => "tiles",
            Target::AseTiles => "asetiles",
            Target::Terrain => "terrain",
        }
    }

    /// The most a parse may hold at once for an input of `len` bytes.
    pub fn peak_limit(self, len: usize) -> usize {
        match self {
            Target::Project | Target::Recovery | Target::TileProject | Target::Terrain => {
                16 * MEGABYTE + 64 * len
            }
            Target::AseTiles => ASE_PEAK_LIMIT,
            Target::Png => 64 * MEGABYTE,
            Target::Inflate => 4 * MEGABYTE,
            Target::BrushLibrary => LIBRARY_PEAK_LIMIT,
            Target::PaletteGpl | Target::PaletteJasc | Target::PaletteHex => PALETTE_PEAK_LIMIT,
        }
    }

    /// The longest one parse may take.
    pub fn max_parse(self) -> Duration {
        match self {
            Target::BrushLibrary => MAX_LIBRARY_READ,
            Target::PaletteGpl | Target::PaletteJasc | Target::PaletteHex => MAX_PALETTE_READ,
            _ => MAX_PARSE,
        }
    }
}

/// The valid files that damage starts from.
pub struct Seeds {
    pub projects: Vec<Vec<u8>>,
    pub recoveries: Vec<Vec<u8>>,
    pub images: Vec<Vec<u8>>,
    pub streams: Vec<Vec<u8>>,
    pub libraries: Vec<Vec<u8>>,
    /// GPL, JASC and HEX files, in that order.
    pub palettes: [Vec<Vec<u8>>; 3],
    /// Project files with tilesets, and Aseprite files with tilemaps.
    pub tiled: [Vec<Vec<u8>>; 2],
    /// Project files with autotiled maps.
    pub terrain: Vec<Vec<u8>>,
}

fn sample_project() -> Result<Vec<u8>, String> {
    let mut editor =
        Editor::new(Document::new(20, 20, ColorMode::Palette).map_err(|e| e.to_string())?);
    editor
        .apply(DrawCmd::Begin {
            tool: Tool::Line,
            x: 0,
            y: 0,
        })
        .map_err(|e| e.to_string())?;
    editor.apply(DrawCmd::To { x: 19, y: 12 }).expect("drag");
    editor.apply(DrawCmd::Commit).expect("commit");
    editor
        .apply(pikslar_core::TagCmd::Add {
            name: "walk".into(),
            from: 0,
            to: 0,
        })
        .map_err(|e| e.to_string())?;
    Ok(piks::save(editor.doc()))
}

/// Three layers sharing drawings: a cel of the bottom layer linked to two frames of the top
/// one, one of them moved apart, and a hold on the middle layer.
fn linked_document() -> Result<Document, String> {
    use pikslar_core::{CelDrag, DragKind, Grid, Landing};
    let fail = |e: pikslar_core::LimitError| e.to_string();
    let mut doc = Document::new(24, 16, ColorMode::Rgba).map_err(fail)?;
    for at in 1..3 {
        doc.add_layer(at, ColorMode::Rgba).map_err(fail)?;
        doc.add_frame(at, false).map_err(fail)?;
    }
    for (layer, frame) in [(0, 0), (1, 0), (2, 2)] {
        let ink = 0xff00_0000 | (40 * layer + frame + 9) as u32;
        let cells: Vec<u32> = (0..24 * 16)
            .map(|i| if i % 5 == 0 { ink } else { 0 })
            .collect();
        doc.set_cel(layer, frame, Grid::from_cells(24, 16, &cells))
            .map_err(fail)?;
    }
    doc.drag_cels(&CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 2,
        to_frame: 0,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    })
    .map_err(fail)?;
    doc.hold_across(2, 0, 0, 1).map_err(fail)?;
    let apart = doc.layers()[2].cel(1).moved_to(5, -3);
    doc.put_cel(2, 1, apart.ok_or("in range")?).map_err(fail)?;
    doc.hold_across(1, 0, 0, 2).map_err(fail)?;
    Ok(doc)
}

/// The linked document with a frame removed and put back and its top layer removed, so its
/// file has a `Nids` section of frame ids and id floors.
fn numbered_document() -> Result<Document, String> {
    use pikslar_core::{FrameCmd, LayerCmd};
    let fail = |e: pikslar_core::LimitError| e.to_string();
    let mut editor = Editor::new(linked_document()?);
    editor
        .apply(FrameCmd::Remove { from: 0, to: 0 })
        .map_err(fail)?;
    editor
        .apply(FrameCmd::Add { at: 0, copy: true })
        .map_err(fail)?;
    let top = editor.layer_id(2).ok_or("three layers")?;
    editor
        .apply(LayerCmd::Remove { layer: top })
        .map_err(fail)?;
    Ok(editor.doc().clone())
}

/// Two and a half mebibytes that pack small, in one block of `deflate`.
fn long_run() -> Vec<u8> {
    (0..5 * MEGABYTE / 2).map(|i| (i / 5) as u8).collect()
}

impl Seeds {
    pub fn load() -> Result<Seeds, String> {
        let entries = crate::corpus::entries()?;
        let read = |entry: &crate::corpus::Entry| {
            entry.bytes().map_err(|e| format!("{}: {e}", entry.name))
        };
        let mut projects = entries.iter().map(read).collect::<Result<Vec<_>, _>>()?;
        projects.push(sample_project()?);
        let linked = linked_document()?;
        projects.push(piks::save(&linked));
        projects.push(crate::fuzz_palette::grouped_project()?);
        let numbered = numbered_document()?;
        projects.push(piks::save(&numbered));
        let mut recoveries = entries
            .iter()
            .filter(|entry| entry.floating)
            .map(read)
            .collect::<Result<Vec<_>, _>>()?;
        recoveries.push(piks::save_recovery(&linked, None));
        recoveries.push(piks::save_recovery(&numbered, None));
        let mut images = craft::png_seeds();
        images.push(
            png::encode(&Image {
                width: 9,
                height: 5,
                px: (0..45).map(|i| 0xff00_0000 | (i * 5000)).collect(),
            })
            .map_err(|e| e.to_string())?,
        );
        let raw: Vec<u8> = (0..4000u32).map(|i| (i / 7) as u8).collect();
        let mut noise = Rng::new(3);
        let random: Vec<u8> = (0..700).map(|_| noise.next_u64() as u8).collect();
        let streams = vec![
            deflate::deflate(&raw),
            deflate::deflate(&random),
            craft::stored(&raw),
            // One block of more than two mebibytes, whose stop is asked inside it.
            deflate::deflate(&long_run()),
        ];
        Ok(Seeds {
            projects,
            recoveries,
            images,
            streams,
            libraries: crate::fuzz_brush::seeds()?,
            palettes: [
                crate::fuzz_palette::seeds(PaletteFormat::Gpl)?,
                crate::fuzz_palette::seeds(PaletteFormat::Jasc)?,
                crate::fuzz_palette::seeds(PaletteFormat::Hex)?,
            ],
            tiled: [
                crate::fuzz_tiles::project_seeds()?,
                crate::fuzz_tiles::ase_seeds()?,
            ],
            terrain: crate::fuzz_terrain::seeds()?,
        })
    }

    fn for_target(&self, target: Target) -> &[Vec<u8>] {
        match target {
            Target::Project => &self.projects,
            Target::Recovery => &self.recoveries,
            Target::Png => &self.images,
            Target::Inflate => &self.streams,
            Target::BrushLibrary => &self.libraries,
            Target::PaletteGpl => &self.palettes[0],
            Target::PaletteJasc => &self.palettes[1],
            Target::PaletteHex => &self.palettes[2],
            Target::TileProject => &self.tiled[0],
            Target::AseTiles => &self.tiled[1],
            Target::Terrain => &self.terrain,
        }
    }
}

/// What a run found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub cases: u64,
    pub accepted: u64,
    pub exercised: u64,
    /// Recovery copies whose floating pixels were good enough to be put back.
    pub restored: u64,
    pub peak: usize,
    pub largest: usize,
    pub slowest: Duration,
}

impl Tally {
    fn record(&mut self, usage: Usage, took: Duration) {
        self.cases += 1;
        self.peak = self.peak.max(usage.peak);
        self.largest = self.largest.max(usage.largest);
        self.slowest = self.slowest.max(took);
    }

    pub fn merge(&mut self, other: &Tally) {
        self.cases += other.cases;
        self.accepted += other.accepted;
        self.exercised += other.exercised;
        self.restored += other.restored;
        self.peak = self.peak.max(other.peak);
        self.largest = self.largest.max(other.largest);
        self.slowest = self.slowest.max(other.slowest);
    }
}

thread_local! {
    static CURRENT: Cell<(Target, u64, u64)> = const { Cell::new((Target::Project, 0, 0)) };
}

/// Makes a crash name the case that caused it, even when the process aborts.
pub fn report_panics_with_case() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let (target, seed, case) = CURRENT.with(Cell::get);
        eprintln!(
            "CRASH while checking {}: --seed {seed} --only {case}",
            target.name()
        );
        default(info);
    }));
}

/// How many seeds at the end of a target's list take long to use (a library at the pixel
/// budget, a palette of every colour a file holds). They are drawn one case in
/// [`HEAVY_ONE_IN`], so a long run still reaches them but a short one stays quick.
fn heavy_seeds(target: Target) -> usize {
    match target {
        Target::BrushLibrary => crate::fuzz_brush::HEAVY_LIBRARY_SEEDS,
        Target::PaletteGpl => crate::fuzz_palette::HEAVY_PALETTE_SEEDS,
        _ => 0,
    }
}

const HEAVY_ONE_IN: usize = 200;

fn pick_seed<'a>(target: Target, inputs: &'a [Vec<u8>], rng: &mut Rng) -> &'a Vec<u8> {
    let heavy = heavy_seeds(target).min(inputs.len().saturating_sub(1));
    if heavy == 0 {
        return &inputs[rng.below(inputs.len())];
    }
    let light = inputs.len() - heavy;
    if rng.chance(HEAVY_ONE_IN) {
        &inputs[light + rng.below(heavy)]
    } else {
        &inputs[rng.below(light)]
    }
}

fn damaged(target: Target, seeds: &Seeds, rng: &mut Rng) -> Vec<u8> {
    let inputs = seeds.for_target(target);
    let input = pick_seed(target, inputs, rng);
    if target == Target::Terrain {
        return crate::fuzz_terrain::damaged(rng, input);
    }
    let bytes = mutate(rng, input);
    match target {
        // Most damage must get past the checksum, or only the checksum is being tested.
        Target::Project | Target::Recovery | Target::TileProject if !rng.chance(20) => {
            craft::resealed(bytes)
        }
        Target::Png if !rng.chance(20) => craft::resealed_png(bytes),
        Target::BrushLibrary if !rng.chance(20) => crate::fuzz_brush::resealed(bytes),
        _ => bytes,
    }
}

fn worth_exercising(doc: &Document, case: u64) -> bool {
    doc.width * doc.height <= ROUTINE_AREA || case % 64 == 0
}

fn use_project(doc: Document, tally: &mut Tally, rng: &mut Rng, case: u64) -> Result<(), String> {
    tally.accepted += 1;
    if worth_exercising(&doc, case) {
        tally.exercised += 1;
        exercise(&mut Editor::new(doc), rng)?;
    }
    Ok(())
}

/// Opens a damaged project with tilesets, or imports a damaged Aseprite file with tilemaps.
fn tiled_case(
    target: Target,
    id: (u64, usize),
    tally: &mut Tally,
    bytes: &[u8],
    rng: &mut Rng,
) -> Result<(), String> {
    let read = match target {
        Target::AseTiles => within_limits(target, id, tally, || pikslar_format::import_ase(bytes))?
            .map(|(doc, _)| doc),
        _ => within_limits(target, id, tally, || piks::load(bytes))?,
    };
    match read {
        Ok(doc) => {
            if target == Target::TileProject {
                crate::fuzz_limits::charge_holds(bytes, id.0)?;
            }
            use_tiled(doc, tally, rng, id.0)
        }
        Err(_) => Ok(()),
    }
}

fn use_tiled(doc: Document, tally: &mut Tally, rng: &mut Rng, case: u64) -> Result<(), String> {
    tally.accepted += 1;
    let deep = worth_exercising(&doc, case) && case % 8 == 0;
    tally.exercised += u64::from(deep);
    crate::fuzz_tiles::use_tiled(doc, rng, deep)
}

/// An opened file with damaged terrain keeps its maps' rules and saves stably; each with
/// terrain sets left also takes terrain strokes of every shape, undone to the start. Most damage
/// inside `Atil` rightly drops the section, so those are the few that test the sets read back.
fn use_terrain(doc: Document, tally: &mut Tally, rng: &mut Rng) -> Result<(), String> {
    tally.accepted += 1;
    let strokes = crate::fuzz_terrain::terrain_sets(&doc) > 0;
    let kept = strokes.then(|| doc.clone());
    crate::fuzz_tiles::use_tiled(doc, rng, false)?;
    match kept {
        Some(doc) => {
            tally.exercised += 1;
            crate::fuzz_terrain::use_terrain(doc, rng)
        }
        None => Ok(()),
    }
}

fn use_recovered(
    found: Recovered,
    tally: &mut Tally,
    rng: &mut Rng,
    case: u64,
) -> Result<(), String> {
    tally.accepted += 1;
    if !worth_exercising(&found.doc, case) {
        return Ok(());
    }
    tally.exercised += 1;
    let mut editor = Editor::new(found.doc);
    // A floating snapshot that does not fit is refused here, as the app refuses it.
    if let Some(snapshot) = found.floating {
        if editor.restore_floating(snapshot).is_ok() {
            tally.restored += 1;
            editor.settle_floating();
            editor.take_damage();
            editor.apply(FloatingCmd::Anchor).expect("anchor");
        }
    }
    // Started afresh, so that "undo everything" means everything since this point.
    exercise(&mut Editor::new(editor.doc().clone()), rng)
}

fn use_image(image: Image, tally: &mut Tally) -> Result<(), String> {
    tally.accepted += 1;
    if image.px.len() != image.width * image.height {
        return Err("decoded pixels do not match the image size".to_owned());
    }
    let encoded = png::encode(&image).map_err(|e| e.to_string())?;
    let again = png::decode(&encoded).map_err(|e| e.to_string())?;
    if again.px != image.px {
        return Err("an accepted image did not survive being written".to_owned());
    }
    Ok(())
}

/// Runs one parse and holds it to its limits.
fn within_limits<R>(
    target: Target,
    (case, len): (u64, usize),
    tally: &mut Tally,
    parse: impl FnOnce() -> R,
) -> Result<R, String> {
    let started = Instant::now();
    let (result, usage) = measure(parse);
    let took = started.elapsed();
    tally.record(usage, took);
    let name = target.name();
    if usage.peak > target.peak_limit(len) {
        return Err(format!("{name} case {case}: held {} bytes", usage.peak));
    }
    if took > target.max_parse() {
        return Err(format!("{name} case {case}: took {took:?}"));
    }
    Ok(result)
}

fn one_case(
    target: Target,
    seeds: &Seeds,
    tally: &mut Tally,
    seed: u64,
    case: u64,
) -> Result<(), String> {
    CURRENT.with(|current| current.set((target, seed, case)));
    let mut rng = Rng::new(seed ^ case.wrapping_mul(0x1000_0000_01b3) ^ target as u64);
    let bytes = damaged(target, seeds, &mut rng);
    let id = (case, bytes.len());
    let used = match target {
        Target::Project => match within_limits(target, id, tally, || piks::load(&bytes))? {
            Ok(doc) => {
                crate::fuzz_limits::charge_holds(&bytes, case)?;
                use_project(doc, tally, &mut rng, case)
            }
            Err(_) => Ok(()),
        },
        Target::Recovery => {
            match within_limits(target, id, tally, || piks::load_recovery(&bytes))? {
                Ok(found) => use_recovered(found, tally, &mut rng, case),
                Err(_) => Ok(()),
            }
        }
        Target::Png => match within_limits(target, id, tally, || png::decode(&bytes))? {
            Ok(image) => use_image(image, tally),
            Err(_) => Ok(()),
        },
        Target::Inflate => {
            match within_limits(target, id, tally, || {
                inflate::inflate(&bytes, INFLATE_LIMIT)
            })? {
                Ok(out) if out.len() > INFLATE_LIMIT => Err("went past its limit".to_owned()),
                Ok(out) => {
                    tally.accepted += 1;
                    crate::fuzz_limits::stop_holds(&bytes, INFLATE_LIMIT, &out, rng.below(4))
                }
                Err(_) => Ok(()),
            }
        }
        Target::BrushLibrary => {
            match within_limits(target, id, tally, || brush_lib::read_library(&bytes))? {
                Ok(library) => {
                    tally.accepted += 1;
                    tally.exercised += 1;
                    crate::fuzz_brush::use_library(&library, &mut rng)
                }
                Err(_) => Ok(()),
            }
        }
        Target::PaletteGpl | Target::PaletteJasc | Target::PaletteHex => {
            palette_case(target, id, tally, &bytes, &mut rng)
        }
        Target::TileProject | Target::AseTiles => tiled_case(target, id, tally, &bytes, &mut rng),
        Target::Terrain => match within_limits(target, id, tally, || piks::load(&bytes))? {
            Ok(doc) => use_terrain(doc, tally, &mut rng),
            Err(_) => Ok(()),
        },
    };
    used.map_err(|e| format!("{} seed {seed} case {case}: {e}", target.name()))
}

/// Reads a damaged palette file as its own format, then, as the app does for a file whose
/// extension is unknown, as whatever its first line says.
fn palette_case(
    target: Target,
    id: (u64, usize),
    tally: &mut Tally,
    bytes: &[u8],
    rng: &mut Rng,
) -> Result<(), String> {
    let format = match target {
        Target::PaletteGpl => PaletteFormat::Gpl,
        Target::PaletteJasc => PaletteFormat::Jasc,
        _ => PaletteFormat::Hex,
    };
    let read = within_limits(target, id, tally, || palette_file::read(bytes, format))?;
    let sniffed = within_limits(target, id, tally, || palette_file::read_any(bytes))?;
    tally.cases -= 1;
    if let Ok(found) = &sniffed {
        crate::fuzz_palette::use_palette(found, PaletteFormat::sniff(bytes), rng)?;
    }
    match read {
        Ok(found) => {
            tally.accepted += 1;
            tally.exercised += 1;
            crate::fuzz_palette::use_palette(&found, format, rng)
        }
        Err(_) => Ok(()),
    }
}

/// Checks a range of cases. Each case depends only on its numbers, so any one can be replayed.
pub fn run(target: Target, seeds: &Seeds, seed: u64, cases: Range<u64>) -> Result<Tally, String> {
    let mut tally = Tally::default();
    for case in cases {
        one_case(target, seeds, &mut tally, seed, case)?;
    }
    Ok(tally)
}

/// The largest image a PNG may claim, from a file a few hundred kilobytes long.
pub struct Bomb {
    pub file_bytes: usize,
    pub usage: Usage,
    pub took: Duration,
}

/// Most a worst-case image may hold: its compressed rows, its decoded rows and its pixels.
pub const BOMB_PEAK_LIMIT: usize = 700 * MEGABYTE;
/// The same for 16 bits a channel, whose decoded rows are twice as long: 512 MiB of rows and
/// 256 MiB of pixels. Rows are unfiltered where they lie, so there is no second copy of them.
pub const BOMB_16_PEAK_LIMIT: usize = 800 * MEGABYTE;
/// Slowest a worst-case image may decode.
pub const BOMB_TIME_LIMIT: Duration = Duration::from_secs(10);

fn one_bomb(interlaced: bool, depth: u8) -> Result<Bomb, String> {
    const SIDE: usize = 8192;
    let raw = vec![0u8; craft::stream_size(SIDE, SIDE, depth, 6, interlaced)];
    let side = SIDE as u32;
    let bytes = craft::png_of_depth(side, side, depth, interlaced, &deflate::deflate(&raw));
    drop(raw);
    let started = Instant::now();
    let (decoded, usage) = measure(|| png::decode(&bytes));
    let took = started.elapsed();
    let image = decoded.map_err(|e| format!("the worst-case image was refused: {e}"))?;
    if image.px.len() != SIDE * SIDE {
        return Err("the worst-case image has the wrong size".to_owned());
    }
    let limit = match depth {
        8 => BOMB_PEAK_LIMIT,
        _ => BOMB_16_PEAK_LIMIT,
    };
    if usage.peak > limit {
        return Err(format!(
            "the worst-case {depth}-bit image (interlaced {interlaced}) held {} bytes",
            usage.peak
        ));
    }
    if took > BOMB_TIME_LIMIT {
        return Err(format!("the worst-case image took {took:?}"));
    }
    Ok(Bomb {
        file_bytes: bytes.len(),
        usage,
        took,
    })
}

/// The names of [`decompression_bombs`], in order.
pub const BOMB_NAMES: [&str; 4] = ["plain", "interlaced", "16-bit plain", "16-bit interlaced"];

/// Decodes 8192 by 8192 true-color images of zeros, plain and interlaced, at 8 and 16 bits a
/// channel, against the limits.
pub fn decompression_bombs() -> Result<[Bomb; 4], String> {
    Ok([
        one_bomb(false, 8)?,
        one_bomb(true, 8)?,
        one_bomb(false, 16)?,
        one_bomb(true, 16)?,
    ])
}

#[cfg(test)]
mod tests {
    use pikslar_format::inflate::{inflate, STOP_EVERY_BYTES};

    #[test]
    fn a_seed_stream_is_stopped_inside_its_block() {
        let seeds = super::Seeds::load().expect("seeds");
        let limit = super::INFLATE_LIMIT;
        let long = seeds.streams.iter().find_map(|s| {
            let out = inflate(s, limit).ok()?;
            (out.len() > 2 * STOP_EVERY_BYTES).then_some((s, out))
        });
        let (long, whole) = long.expect("a stream past two mebibytes within the limit");
        for after in 1..3 {
            let held = crate::fuzz_limits::stop_holds(long, limit, &whole, after);
            assert_eq!(held, Ok(()));
        }
    }

    #[test]
    fn the_numbered_seed_saves_frame_ids_and_floors() {
        let doc = super::numbered_document().expect("made");
        let bytes = super::piks::save(&doc);
        assert!(bytes.windows(4).any(|w| w == b"Nids"));
        let back = super::piks::load(&bytes).expect("opens");
        assert_eq!(back.layer_floor(), doc.layer_floor());
        assert_eq!(back.frame_floor(), doc.frame_floor());
    }

    #[test]
    fn the_linked_seed_shares_drawings_across_layers_at_two_places() {
        let doc = super::linked_document().expect("made");
        assert_eq!(doc.check_links(), Ok(()));
        let id = doc.layers()[0].link(0);
        assert!(id != 0 && doc.layers()[2].link(1) == id);
        assert_ne!(
            doc.layers()[2].cel(1).position(),
            doc.layers()[0].cel(0).position()
        );
        assert!(doc.layers()[1].links().all(|link| link != 0 && link != id));
    }
}
