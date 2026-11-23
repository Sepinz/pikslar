//! Tileset chunks in numbers no real file has: read in time linear in the file, stopped when
//! asked, and charged for every tile they make, even blank ones.

use super::{MAX_KEPT_TILESETS, TILE_OVERHEAD};
use crate::ase_build::*;
use crate::ase_cel::Budget;
use crate::ase_chunks::{Depth, Sprite};
use crate::{import_ase, import_ase_until, AseWarning, FormatError};
use std::cell::Cell;
use std::time::{Duration, Instant};

/// One frame of `count` tileset chunks, each kept in another file under its own number.
fn many_tilesets(count: u32) -> Vec<u8> {
    let tilesets = (0..count).map(|n| tileset_external(n, 1, (2, 2), "t"));
    let chunks = std::iter::once(plain("art")).chain(tilesets).collect();
    Ase::new(4, 4, 32).frame(100, chunks).bytes()
}

#[test]
fn many_tileset_chunks_are_read_in_linear_time_and_only_a_few_kept() {
    let bytes = many_tilesets(100_000);
    let started = Instant::now();
    let (doc, warnings) = import_ase(&bytes).expect("opens");
    let took = started.elapsed();
    // Kept and searched linearly, 100,000 chunks took tens of seconds; read in order they take
    // well under one, even unoptimised.
    assert!(took < Duration::from_secs(10), "{took:?}");
    assert!(doc.tilesets.is_empty(), "kept in other files");
    let past = AseWarning::Skipped("tilesets past what a document holds");
    assert!(warnings.contains(&past), "{warnings:?}");
}

#[test]
fn tilesets_past_the_bound_are_not_kept() {
    let bytes = many_tilesets(MAX_KEPT_TILESETS as u32 + 50);
    let mut sprite = Sprite::new(4, 4, Depth::Rgba, 1, budget());
    // The chunks of the frame, past its header, each read as the importer reads it.
    for n in 0..MAX_KEPT_TILESETS as u32 + 50 {
        let chunk = tileset_external(n, 1, (2, 2), "t");
        sprite.tileset_chunk(&chunk[6..]).expect("read");
    }
    assert_eq!(sprite.tilesets.len(), MAX_KEPT_TILESETS);
    assert!(import_ase(&bytes).is_ok());
}

#[test]
fn a_frame_of_many_chunks_asks_whether_to_stop_as_it_goes() {
    let bytes = many_tilesets(50_000);
    let asked = Cell::new(0);
    let never = || {
        asked.set(asked.get() + 1);
        false
    };
    import_ase_until(&bytes, &never).expect("opens");
    assert!(asked.get() > 10, "asked {} times", asked.get());
    let at_once = Cell::new(0);
    let soon = || {
        at_once.set(at_once.get() + 1);
        at_once.get() > 2
    };
    assert!(matches!(
        import_ase_until(&bytes, &soon),
        Err(FormatError::Stopped)
    ));
}

fn budget() -> Budget {
    Budget {
        work: 1 << 30,
        held: 512 << 20,
    }
}

#[test]
fn blank_tiles_of_a_damaged_picture_are_charged_too() {
    let count = 1_000u32;
    let chunk = tileset_packed(0, 4, count + 1, (1, 1), "broken", &[0x78, 0x9c, 0xff]);
    let mut sprite = Sprite::new(4, 4, Depth::Rgba, 1, budget());
    let held = sprite.budget.held;
    sprite.tileset_chunk(&chunk[6..]).expect("read");
    let charged = held - sprite.budget.held;
    assert!(
        charged >= u64::from(count) * TILE_OVERHEAD,
        "{charged} bytes for {count} tiles"
    );
    // Too many of them for what is left is refused, not made.
    let mut poor = Sprite::new(4, 4, Depth::Rgba, 1, budget());
    poor.budget.held = u64::from(count) * TILE_OVERHEAD - 1;
    let refused = poor.tileset_chunk(&chunk[6..]);
    assert!(
        matches!(refused, Err(FormatError::FileTooLarge)),
        "{refused:?}"
    );
}

/// One plain layer and 255 tilemap layers over one tileset of `count` opaque 64 by 64 tiles
/// that differ, so none packs to one colour and every pixel is looked at.
fn layers_over_one_tileset(count: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(count as usize * 4096 * 4);
    for t in 0..count {
        for p in 0..4096u32 {
            pixels.extend_from_slice(&[(p + t) as u8, (p * 7) as u8, (t >> 8) as u8, 255]);
        }
    }
    let mut chunks = vec![plain("art")];
    chunks.extend((0..255).map(|n| tilemap_layer(0, 0, &format!("map {n}"), 0)));
    chunks.push(tileset_chunk(0, 0, count, (64, 64), "opaque", &pixels));
    Ase::new(16, 16, 32).frame(100, chunks).bytes()
}

/// Release only: unoptimised, looking at the pixels takes far longer than either bound.
#[test]
#[cfg_attr(debug_assertions, ignore = "timed; run with --release")]
fn many_tilemap_layers_over_one_tileset_are_looked_at_quickly_and_stop_when_asked() {
    let bytes = layers_over_one_tileset(4096);
    let last = Cell::new(Instant::now());
    let longest = Cell::new(Duration::ZERO);
    let never = || {
        let now = Instant::now();
        longest.set(longest.get().max(now - last.replace(now)));
        false
    };
    let started = Instant::now();
    let (doc, _) = import_ase_until(&bytes, &never).expect("opens");
    let took = started.elapsed();
    let tail = last.get().elapsed();
    assert_eq!(doc.layers().len(), 256);
    assert_eq!(doc.blending, pikslar_core::Blending::Pikslar, "all opaque");
    // Looked at once for each of its 255 layers, the 16 million pixels took seconds, without
    // a question between.
    assert!(took < Duration::from_secs(1), "{took:?}");
    let gap = longest.get().max(tail);
    assert!(gap < Duration::from_millis(100), "a stop waited {gap:?}");
}
