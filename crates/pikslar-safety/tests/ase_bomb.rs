//! Worst-case Aseprite files held to the same memory and time limits as the PNG bombs of
//! `fuzz::decompression_bombs`. They are slow and big, so they are ignored by default; run them
//! in release: `cargo test -p pikslar-safety --release --test ase_bomb -- --ignored`.

#![allow(dead_code)]

#[path = "../../pikslar-format/src/ase_build.rs"]
mod ase_build;

use ase_build::{
    cel, cel_tilemap_packed, plain, tilemap_layer, tileset_chunk, zlib, Ase, MASKS_NEW,
};
use pikslar_format::{import_ase, FormatError};
use pikslar_safety::fuzz::{BOMB_PEAK_LIMIT, BOMB_TIME_LIMIT};
use pikslar_safety::tracking::measure;
use std::time::Instant;

const SIDE: usize = 8192;

/// A sprite of `frames` frames, each a cel of `side` by `side` pixels made of `pixels`.
fn bomb_file(depth: u16, pixels: &[u8], side: usize, frames: usize) -> Vec<u8> {
    let payload = {
        let mut p = Vec::new();
        p.extend_from_slice(&(side as u16).to_le_bytes());
        p.extend_from_slice(&(side as u16).to_le_bytes());
        p.extend_from_slice(&zlib(pixels));
        p
    };
    let mut ase = Ase::new(side as u16, side as u16, depth);
    ase.transparent = 255;
    ase = ase.frame(100, vec![plain("a"), cel(0, 0, 0, 255, 2, 0, &payload)]);
    for _ in 1..frames {
        ase = ase.frame(100, vec![cel(0, 0, 0, 255, 2, 0, &payload)]);
    }
    ase.bytes()
}

fn noisy_indexed() -> Vec<u8> {
    (0..SIDE * SIDE)
        .map(|i| ((i % SIDE + 16 * (i / SIDE)) % 256) as u8)
        .collect()
}

fn noisy_rgba() -> Vec<u8> {
    (0..SIDE * SIDE)
        .flat_map(|i| {
            let n = (i % SIDE + 16 * (i / SIDE)) as u32;
            [n as u8, (n >> 3) as u8, (n >> 5) as u8, 255]
        })
        .collect()
}

/// Reads the file against the limits: it may be refused as too large, but never past them.
fn held(name: &str, bytes: &[u8]) {
    let started = Instant::now();
    let (result, usage) = measure(|| import_ase(bytes).map(|(doc, _)| doc.layers().len()));
    let took = started.elapsed();
    let outcome = match &result {
        Ok(_) => "accepted".to_owned(),
        Err(e) => format!("refused ({e})"),
    };
    eprintln!(
        "{name}: file {} KiB, {outcome}, peak {} MiB, took {took:?}",
        bytes.len() >> 10,
        usage.peak >> 20
    );
    if let Err(e) = &result {
        assert!(matches!(e, FormatError::FileTooLarge), "{name}: {e}");
    }
    assert!(usage.peak <= BOMB_PEAK_LIMIT, "{name}: peak {}", usage.peak);
    assert!(took <= BOMB_TIME_LIMIT, "{name}: took {took:?}");
}

fn frames(default: usize) -> usize {
    std::env::var("F")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "slow and large: run in release"]
fn noisy_indexed_cels_are_held_to_the_png_limits() {
    held(
        "indexed noisy",
        &bomb_file(8, &noisy_indexed(), SIDE, frames(16)),
    );
}

#[test]
#[ignore = "slow and large: run in release"]
fn solid_indexed_cels_are_held_to_the_png_limits() {
    held(
        "indexed solid",
        &bomb_file(8, &vec![7u8; SIDE * SIDE], SIDE, frames(16)),
    );
}

#[test]
#[ignore = "slow and large: run in release"]
fn noisy_rgba_cels_are_held_to_the_png_limits() {
    held("rgba noisy", &bomb_file(32, &noisy_rgba(), SIDE, frames(4)));
}

#[test]
#[ignore = "slow and large: run in release"]
fn solid_rgba_cels_are_held_to_the_png_limits() {
    held(
        "rgba solid",
        &bomb_file(32, &vec![0x80u8; SIDE * SIDE * 4], SIDE, frames(4)),
    );
}

#[test]
#[ignore = "slow and large: run in release"]
fn one_noisy_indexed_cel_of_the_largest_size_still_opens() {
    let bytes = bomb_file(8, &noisy_indexed(), SIDE, 1);
    let (result, usage) = measure(|| import_ase(&bytes).map(|(doc, _)| doc.layers().len()));
    assert!(result.is_ok(), "{result:?}");
    assert!(usage.peak <= BOMB_PEAK_LIMIT, "peak {}", usage.peak);
}

/// A file with one tileset of `count` tiles (tile 0 counted) of `tile` pixels, packed from
/// `pixels`, and one map of one cell.
fn tileset_bomb(depth: u16, count: u32, tile: (u16, u16), pixels: &[u8]) -> Vec<u8> {
    let first = vec![
        tileset_chunk(0, 4, count, tile, "bomb", pixels),
        tilemap_layer(3, 0, "m", 0),
        ase_build::cel_tilemap(0, (0, 0), (1, 1), 32, MASKS_NEW, &[1]),
    ];
    Ase::new(8, 8, depth).frame(100, first).bytes()
}

fn tiles_noise(tiles: usize, side: usize, bytes: usize) -> Vec<u8> {
    (0..tiles * side * side * bytes)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect()
}

#[test]
#[ignore = "slow and large: run in release"]
fn noisy_rgba_tilesets_are_held_to_the_png_limits() {
    // 256 by 256 tiles: 1,500 of them are 393 MB of pixels, past what one file may unpack in
    // the memory left; 900 are 236 MB and fit.
    for count in [frames(1_500), 900] {
        let pixels = tiles_noise(count, 256, 4);
        let name = format!("rgba tileset noisy x{count}");
        held(&name, &tileset_bomb(32, count as u32, (256, 256), &pixels));
    }
}

#[test]
#[ignore = "slow and large: run in release"]
fn solid_and_indexed_tilesets_are_held_to_the_png_limits() {
    let solid = vec![0x80u8; 1_500 * 256 * 256 * 4];
    held(
        "rgba tileset solid",
        &tileset_bomb(32, 1_500, (256, 256), &solid),
    );
    let indexed = tiles_noise(4_000, 256, 1);
    held(
        "indexed tileset noisy",
        &tileset_bomb(8, 4_000, (256, 256), &indexed),
    );
    // More tiles than the pixels could be unpacked in the time allowed: refused before any is.
    held(
        "rgba tileset past the work",
        &tileset_bomb(32, 20_000, (256, 256), &[0]),
    );
}

#[test]
#[ignore = "slow and large: run in release"]
fn the_most_tiny_tiles_are_held_to_the_limits() {
    let pixels = tiles_noise(65_536, 1, 4);
    held(
        "65,536 tiles of one pixel",
        &tileset_bomb(32, 65_536, (1, 1), &pixels),
    );
}

#[test]
#[ignore = "slow and large: run in release"]
fn maps_of_the_most_cells_are_held_to_the_png_limits() {
    // 8192 by 8192 cells of 32 bits: 268 MB a map, one for each of the frames.
    let cells: Vec<u8> = (0..SIDE * SIDE)
        .flat_map(|i| (((i.wrapping_mul(2_654_435_761) >> 20) % 3 + 1) as u32).to_le_bytes())
        .collect();
    let side = SIDE as u16;
    let map = cel_tilemap_packed(0, (0, 0), (side, side), 32, MASKS_NEW, &zlib(&cells));
    let mut ase = Ase::new(8, 8, 32);
    let first = vec![
        tileset_chunk(0, 4, 4, (2, 2), "t", &[7; 64]),
        tilemap_layer(3, 0, "m", 0),
        map.clone(),
    ];
    ase = ase.frame(100, first);
    for _ in 1..frames(3) {
        ase = ase.frame(100, vec![map.clone()]);
    }
    held("maps of 8192 by 8192 cells", &ase.bytes());
}
