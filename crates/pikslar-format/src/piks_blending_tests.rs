//! Aseprite-compatible blending in the project file: required feature bit 3, set exactly when
//! the document uses it, so an older version refuses the file rather than drawing it wrongly.

use crate::piks::{load_noted, save};
use pikslar_core::{BlendMode, Blending, ColorMode, Document};

/// The header's required features, right after the magic and the version.
fn features(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]])
}

/// What versions before Aseprite-compatible blending knew: references, groups and tilemaps.
const KNOWN_BEFORE: u32 = 1 | 2 | 4;

fn with_every_mode() -> Document {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    for at in 1..BlendMode::ALL.len() {
        doc.add_layer(at, ColorMode::Rgba).expect("room");
    }
    for (layer, mode) in doc.layers_mut().iter_mut().zip(BlendMode::ALL) {
        layer.blend = mode;
    }
    doc
}

#[test]
fn aseprite_blending_and_every_mode_survive_save_and_load() {
    let mut doc = with_every_mode();
    doc.blending = Blending::Aseprite;
    let bytes = save(&doc);
    assert_eq!(features(&bytes) & 8, 8, "bit 3 is set");
    assert_ne!(
        features(&bytes) & !KNOWN_BEFORE,
        0,
        "an older version refuses it"
    );
    let (back, notes) = load_noted(&bytes).expect("loads");
    assert_eq!(back.blending, Blending::Aseprite);
    let modes: Vec<BlendMode> = back.layers().iter().map(|l| l.blend).collect();
    assert_eq!(modes, BlendMode::ALL);
    assert!(notes.can_save_over() && notes.message().is_none());
    assert_eq!(save(&back), bytes, "saving again writes the same file");
}

#[test]
fn pikslar_blending_sets_no_bit_and_keeps_an_aseprite_mode_in_use() {
    // A document switched back to Pikslar's blending keeps each layer's mode; only how it is
    // drawn changes, and an older version reads a mode it does not know as Normal.
    let doc = with_every_mode();
    let bytes = save(&doc);
    assert_eq!(features(&bytes) & 8, 0);
    let (back, notes) = load_noted(&bytes).expect("loads");
    assert_eq!(back.blending, Blending::Pikslar);
    assert_eq!(back.layers()[14].blend, BlendMode::Hue);
    assert!(notes.can_save_over());
}

#[test]
fn a_new_document_is_pikslar_blending_and_its_file_is_as_before() {
    let doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    assert_eq!(doc.blending, Blending::Pikslar);
    assert_eq!(features(&save(&doc)), 0);
}
