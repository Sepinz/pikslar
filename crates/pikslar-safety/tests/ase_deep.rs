//! Every Aseprite file the importer accepts out of many damaged ones must also be a document
//! the editor can be used on: it saves and loads, and survives the safety crate's whole
//! exercise of commands. Heavier than the check in `pikslar-format`'s own robustness test, so
//! it is ignored by default: `cargo test -p pikslar-safety --release --test ase_deep -- --ignored`
//! (set `PIKSLAR_ASE_DEEP` for the number of files; 3,000 by default).

#![allow(dead_code)]

#[path = "../../pikslar-format/src/ase_build.rs"]
mod ase_build;

use pikslar_core::{Document, Editor};
use pikslar_format::{import_ase, piks};
use pikslar_safety::deep_use::{check_shape, exercise};
use pikslar_safety::mutate::{mutate, Rng};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn deep(doc: Document, seed: u64) -> Result<(), String> {
    doc.check_groups().map_err(|e| format!("groups: {e:?}"))?;
    check_shape(&doc).map_err(|e| format!("shape: {e}"))?;
    let saved = piks::save(&doc);
    let again = piks::load(&saved).map_err(|e| format!("its own save is refused: {e}"))?;
    if piks::save(&again) != saved {
        return Err("a save is not stable".to_owned());
    }
    let mut rng = Rng::new(seed);
    let mut editor = Editor::new(doc);
    match catch_unwind(AssertUnwindSafe(|| exercise(&mut editor, &mut rng))) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(format!("exercise: {e}")),
        Err(_) => Err("a panic in the exercise".to_owned()),
    }
}

#[test]
#[ignore = "heavy: run in release"]
fn accepted_damaged_files_survive_the_whole_exercise() {
    let seeds = [ase_build::rich(), ase_build::rich_indexed()];
    let count: u64 = std::env::var("PIKSLAR_ASE_DEEP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3_000);
    let (mut accepted, mut failures) = (0u64, Vec::new());
    for n in 0..count {
        let mut rng = Rng::new(n * 7919 + 3);
        let seed = &seeds[rng.below(seeds.len())];
        let file = mutate(&mut rng, seed);
        let Ok(read) = catch_unwind(|| import_ase(&file)) else {
            failures.push(format!("mutation {n}: the import panicked"));
            continue;
        };
        let Ok((doc, _)) = read else { continue };
        accepted += 1;
        if let Err(e) = deep(doc, n) {
            failures.push(format!("mutation {n}: {e}"));
        }
    }
    eprintln!(
        "{count} files, {accepted} accepted, {} failed",
        failures.len()
    );
    assert!(failures.is_empty(), "{failures:#?}");
}
