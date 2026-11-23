//! A short version of the long run, with every debug check on.

use pikslar_format::png;
use pikslar_safety::craft::{png_from_stream, stored};
use pikslar_safety::fuzz::{self, Seeds, Tally, Target};
use pikslar_safety::tracking::measure;

const CASES: u64 = 3_000;

fn check(target: Target) -> Tally {
    fuzz::report_panics_with_case();
    let seeds = Seeds::load().expect("seeds");
    let tally = fuzz::run(target, &seeds, 1, 0..CASES).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(tally.cases, CASES);
    assert!(
        tally.accepted > CASES / 50,
        "only {} damaged files got through, so little deep was tested",
        tally.accepted
    );
    tally
}

#[test]
fn damaged_projects_are_safe_to_open_and_use() {
    let tally = check(Target::Project);
    assert!(tally.exercised > CASES / 50, "{}", tally.exercised);
}

#[test]
fn damaged_recovery_copies_are_safe_to_open_and_use() {
    let tally = check(Target::Recovery);
    assert!(tally.exercised > CASES / 50, "{}", tally.exercised);
    assert!(tally.restored > CASES / 100, "{}", tally.restored);
}

#[test]
fn damaged_images_are_safe_to_open() {
    check(Target::Png);
}

#[test]
fn damaged_compressed_streams_are_safe_to_read() {
    check(Target::Inflate);
}

#[test]
fn a_tiny_file_claiming_a_huge_image_costs_little_memory() {
    let file = png_from_stream(8192, 8192, false, &stored(&[0; 100]));
    let (result, usage) = measure(|| png::decode(&file));
    assert!(result.is_err());
    assert!(usage.peak < 32 << 20, "held {} bytes", usage.peak);
}

#[test]
fn damaged_brush_libraries_are_safe_to_read_and_use() {
    let tally = check(Target::BrushLibrary);
    assert!(tally.exercised > CASES / 2, "{}", tally.exercised);
    assert!(tally.peak < 32 << 20, "{}", tally.peak);
}

#[test]
fn the_brush_library_seeds_are_valid_and_resealing_keeps_them_so() {
    use pikslar_format::brush_lib::read_library;
    let seeds = pikslar_safety::fuzz_brush::seeds().expect("seeds");
    assert!(seeds.len() >= 4);
    let mut over_budget = 0;
    for seed in seeds {
        let library = read_library(&seed).expect("reads");
        assert!(!library.brushes.is_empty());
        // One seed is over the pixel budget on purpose, to reach the reader's refusal of it.
        if library.report.over_limit {
            over_budget += 1;
            assert!(library.report.lost > 0);
        } else {
            assert!(library.report.is_clean());
        }
        let sealed = pikslar_safety::fuzz_brush::resealed(seed.clone());
        assert_eq!(sealed, seed, "resealing a good library changes nothing");
    }
    assert_eq!(over_budget, 1, "the seed over the pixel budget");
}

#[test]
fn damaged_palette_files_of_each_format_are_safe_to_read_and_use() {
    for target in [Target::PaletteGpl, Target::PaletteJasc, Target::PaletteHex] {
        let tally = check(target);
        assert!(
            tally.exercised > CASES / 50,
            "{target:?}: {}",
            tally.exercised
        );
        assert!(tally.peak < 32 << 20, "{target:?}: {}", tally.peak);
    }
}

#[test]
fn damaged_projects_with_tilesets_are_safe_to_open_save_and_use() {
    let tally = check(Target::TileProject);
    assert!(tally.exercised > CASES / 100, "{}", tally.exercised);
}

/// Damage mostly inside `Atil` (M11): a file that opens keeps its maps' rules, saves stably,
/// and takes terrain strokes of every shape that undo to the start.
#[test]
fn damaged_terrain_sections_are_safe_to_open_paint_and_undo() {
    let tally = check(Target::Terrain);
    // About a fifth of the cases keep terrain sets to paint with.
    assert!(tally.exercised > CASES / 10, "{}", tally.exercised);
}

#[test]
fn damaged_aseprite_files_with_tilemaps_are_safe_to_import_save_and_use() {
    let tally = check(Target::AseTiles);
    assert!(tally.exercised > CASES / 100, "{}", tally.exercised);
    assert!(tally.peak < fuzz::ASE_PEAK_LIMIT, "{}", tally.peak);
}
