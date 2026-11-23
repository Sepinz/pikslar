//! Replays recorded input, with frame boundaries, through the whole application.
//!
//! To change the record on purpose, set `PIKSLAR_RECORD_REPLAY` and run this test, then say in
//! the commit why the states differ.

use crate::app::App;
use pikslar_core::Document;
use pikslar_platform::{Button, Event, Handler, Key, Mods, Window};
use pikslar_render::Surface;
use std::fmt::Write;

const RECORD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../perf/replay-app.txt");
const SEQUENCES: u32 = 8;
const EVENTS: u32 = 3_000;
const LINE_EVERY: u32 = 100;
/// Letters that do the same thing every run. Others open dialogs or use the shared clipboard.
const LETTERS: &[u8] = b"BEGILUMQWVRHAZYD0TKNF";

fn keys() -> Vec<Key> {
    let letters = LETTERS.iter().map(|&c| Key::Char(c));
    letters
        .chain([
            Key::Escape,
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::Home,
            Key::End,
            Key::Comma,
            Key::Period,
            Key::Delete,
        ])
        .collect()
}

fn mix(hash: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        *hash = (*hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn hash_document(hash: &mut u64, doc: &Document) {
    doc.colors
        .colors()
        .iter()
        .for_each(|&c| mix(hash, c as u64));
    doc.frames
        .iter()
        .for_each(|f| mix(hash, f.duration_ms as u64));
    for tag in &doc.tags {
        tag.name.bytes().for_each(|b| mix(hash, b as u64));
        mix(hash, tag.from as u64);
        mix(hash, tag.to as u64);
    }
    for layer in doc.layers() {
        mix(hash, layer.visible as u64);
        mix(hash, layer.opacity as u64);
        layer.links().for_each(|l| mix(hash, l as u64));
        // Where each cel sits and every pixel it keeps, past the canvas too.
        for cel in layer.cels() {
            let (cx, cy) = cel.position();
            mix(hash, (cx as u16 as u64) << 16 | cy as u16 as u64);
            let (x0, y0, tw, th) = cel.image().table();
            for y in y0 * 16..(y0 + th as i32) * 16 {
                for x in x0 * 16..(x0 + tw as i32) * 16 {
                    mix(hash, cel.image().get(x, y) as u64);
                }
            }
        }
    }
}

fn describe(a: &App) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    let e = &a.editor;
    hash_document(&mut hash, e.doc());
    format!(
        "{hash:016x} {} {} {} {} {}",
        e.history().undo_depth(),
        e.history().redo_depth(),
        e.layer,
        e.frame,
        e.is_floating() as u8
    )
}

fn play(seed: u32, out: &mut String) {
    let window = Window::detached();
    let (w, h) = (800, 600);
    let mut px = vec![0u32; (w * h) as usize];
    let keys = keys();
    let mut a = App::launch(None, Some(32), None);
    a.resized(w, h, 1);
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(11);
    let mut next = move |limit: u32| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) % limit
    };
    let start = std::time::Instant::now();
    for index in 0..EVENTS {
        let (x, y) = (next(w as u32) as i32, next(h as u32) as i32);
        let key = keys[next(keys.len() as u32) as usize];
        let may_ctrl = matches!(key, Key::Char(b'A' | b'D' | b'Z' | b'Y'));
        let mods = Mods {
            ctrl: may_ctrl && next(2) == 0,
            shift: next(4) == 0,
            alt: next(6) == 0,
        };
        let button = [Button::Left, Button::Left, Button::Right, Button::Middle][next(4) as usize];
        let event = match next(10) {
            0..=3 => Event::Move { x, y },
            4 | 5 => Event::Down { x, y, button, mods },
            6 | 7 => Event::Up { x, y, button },
            _ => Event::KeyDown {
                key,
                mods,
                repeat: false,
            },
        };
        a.event(event.clone(), window);
        // A key that types sends its character after it, as Windows does.
        if let Event::KeyDown { key, mods, .. } = event {
            if let Some(c) = crate::test_app::typed_char(key, mods) {
                a.event(Event::Text(c), window);
            }
        }
        // A scrub held past the strip's end steps on a clock: this one moves a millisecond an
        // event, so a slow run steps exactly where a fast one does.
        a.timeline.clock = Some(start + std::time::Duration::from_millis(u64::from(index)));
        // Playback runs on the clock, which no record can repeat.
        a.playing = false;
        if next(4) == 0 {
            let mut surface = Surface { px: &mut px, w, h };
            a.frame(&mut surface, window, &mut Vec::new());
        }
        if index % LINE_EVERY == 0 {
            let _ = writeln!(out, "{seed} {index} {}", describe(&a));
        }
    }
}

#[test]
fn input_replays_to_the_recorded_states() {
    let mut played = String::new();
    (0..SEQUENCES).for_each(|seed| play(seed, &mut played));
    if std::env::var_os("PIKSLAR_RECORD_REPLAY").is_some() {
        std::fs::write(RECORD, &played).expect("recorded");
        return;
    }
    let recorded = std::fs::read_to_string(RECORD).expect("record exists");
    let recorded = recorded.replace("\r\n", "\n");
    let first = played
        .lines()
        .zip(recorded.lines())
        .find(|(now, before)| now != before);
    assert_eq!(first, None, "first state that differs (now, recorded)");
    assert_eq!(played.lines().count(), recorded.lines().count());
}
