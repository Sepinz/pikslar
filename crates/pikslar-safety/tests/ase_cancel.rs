//! How long a read of the largest Aseprite cels takes to end once asked to stop. They are slow
//! and big, so they are ignored by default; run them in release and read the times printed:
//! `cargo test -p pikslar-safety --release --test ase_cancel -- --ignored --nocapture`.

#![allow(dead_code)]

#[path = "../../pikslar-format/src/ase_build.rs"]
mod ase_build;

use ase_build::{cel, plain, zlib, Ase};
use pikslar_format::{import_ase, import_ase_until, FormatError};
use std::time::{Duration, Instant};

const SIDE: usize = 8192;

/// A sprite of one frame holding one packed cel of `SIDE` by `SIDE` pixels made of `pixels`.
fn one_cel(depth: u16, pixels: &[u8]) -> Vec<u8> {
    let mut payload = [(SIDE as u16).to_le_bytes(), (SIDE as u16).to_le_bytes()].concat();
    payload.extend_from_slice(&zlib(pixels));
    let mut ase = Ase::new(SIDE as u16, SIDE as u16, depth);
    ase.transparent = 255;
    ase.frame(100, vec![plain("a"), cel(0, 0, 0, 255, 2, 0, &payload)])
        .bytes()
}

fn noisy_rgba() -> Vec<u8> {
    (0..SIDE * SIDE)
        .flat_map(|i| {
            let n = (i % SIDE + 16 * (i / SIDE)) as u32;
            [n as u8, (n >> 3) as u8, (n >> 5) as u8, 255]
        })
        .collect()
}

/// The longest a read of `bytes` went on past a stop asked for at each of `after` that comes
/// before the read would end. The read may end refused for its size; a stop asked before then
/// still ends it.
fn worst_delay(name: &str, bytes: &[u8], after: &[Duration]) -> Duration {
    let started = Instant::now();
    let whole = import_ase(bytes).map(|(doc, _)| doc.layers().len());
    let took = started.elapsed();
    assert!(
        matches!(whole, Ok(_) | Err(FormatError::FileTooLarge)),
        "{name}: {whole:?}"
    );
    let mut worst = Duration::ZERO;
    for &wait in after.iter().filter(|&&wait| wait < took) {
        let started = Instant::now();
        let stop = || started.elapsed() >= wait;
        let result = import_ase_until(bytes, &stop);
        let delay = started.elapsed().saturating_sub(wait);
        assert!(matches!(result, Err(FormatError::Stopped)), "{name}");
        worst = worst.max(delay);
        if std::env::var("EACH").is_ok() {
            eprintln!("  {name}: stop at {wait:?}, ended {delay:?} later");
        }
    }
    eprintln!("{name}: whole read {took:?}, longest past a stop {worst:?}");
    worst
}

fn waits() -> Vec<Duration> {
    (1..=20).map(|n| Duration::from_millis(n * 25)).collect()
}

#[test]
#[ignore = "slow and large: run in release"]
fn the_largest_solid_cel_stops_soon_after_it_is_asked() {
    let bytes = one_cel(32, &vec![0x80u8; SIDE * SIDE * 4]);
    worst_delay("rgba solid", &bytes, &waits());
}

#[test]
#[ignore = "slow and large: run in release"]
fn the_largest_noisy_cel_stops_soon_after_it_is_asked() {
    let bytes = one_cel(32, &noisy_rgba());
    worst_delay("rgba noisy", &bytes, &waits());
}
