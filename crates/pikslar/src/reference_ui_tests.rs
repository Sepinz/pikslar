//! Placing a reference picture from a PNG: what it becomes, what it refuses before decoding,
//! and that drawing on it says why it cannot.

use crate::test_app::*;
use pikslar_core::{rgba, LayerKind, StepCmd};
use pikslar_format::png;
use std::path::PathBuf;

const PHOTO: u32 = rgba(10, 120, 240, 255);

fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-ref-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("folder");
    let path = dir.join(format!("{name}.png"));
    std::fs::write(&path, bytes).expect("written");
    path
}

fn photo(width: usize, height: usize) -> Vec<u8> {
    let image = png::Image {
        width,
        height,
        px: vec![PHOTO; width * height],
    };
    png::encode(&image).expect("encoded")
}

/// The PNG checksum, bit by bit: a test needs it only for one header.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A PNG whose header says `width` by `height` and whose data is not an image at all.
fn header_only(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = photo(1, 1);
    bytes[16..20].copy_from_slice(&width.to_be_bytes());
    bytes[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = crc32(&bytes[12..29]);
    bytes[29..33].copy_from_slice(&crc.to_be_bytes());
    bytes.truncate(33);
    bytes.extend_from_slice(&[0; 64]);
    bytes
}

#[test]
fn a_png_becomes_a_reference_above_the_active_layer_named_after_the_file() {
    let mut a = app(3);
    let path = temp_file("street", &photo(40, 20));
    a.place_reference_from(&path).expect("placed");
    let doc = a.editor.doc();
    assert_eq!(doc.layers().len(), 2);
    assert_eq!(a.editor.layer, 1);
    let reference = &doc.layers()[1];
    assert_eq!(reference.kind, LayerKind::Reference);
    assert_eq!(reference.name, "street");
    assert_eq!(
        reference.cel(2).get(39, 19),
        PHOTO,
        "every frame, past the canvas"
    );
    a.editor.apply(StepCmd::Undo).expect("undo");
    assert_eq!(a.editor.doc().layers().len(), 1);
}

#[test]
fn a_png_too_large_is_refused_from_its_header_before_decoding() {
    let mut a = app(1);
    for (w, h) in [(5000, 10), (10, 4097), (9000, 9000)] {
        let path = temp_file(&format!("huge{w}x{h}"), &header_only(w, h));
        let refused = a.place_reference_from(&path).expect_err("refused");
        assert!(refused.to_string().contains("4,096"), "{w}x{h}: {refused}");
    }
    assert_eq!(a.editor.doc().layers().len(), 1);
}

#[test]
fn a_damaged_png_says_so_and_changes_nothing() {
    let mut a = app(1);
    let path = temp_file("broken", &header_only(16, 16));
    assert!(a.place_reference_from(&path).is_err());
    let path = temp_file("notpng", b"hello");
    assert!(a.place_reference_from(&path).is_err());
    assert_eq!(a.editor.doc().layers().len(), 1);
}

#[test]
fn drawing_on_the_reference_says_why_it_cannot() {
    let mut a = app(1);
    let path = temp_file("trace", &photo(8, 8));
    a.place_reference_from(&path).expect("placed");
    let before = a.editor.doc().layers()[1].cel(0).image_id();
    let on_canvas = screen(&a, 3, 3);
    click(&mut a, on_canvas, pikslar_platform::Mods::default());
    assert_eq!(a.editor.doc().layers()[1].cel(0).image_id(), before);
    let said = a.message.as_ref().map(|(text, _)| text.as_str());
    assert_eq!(said, Some("Error: A reference layer is not drawn on"));
}

#[test]
fn the_open_menu_ends_with_placing_a_reference() {
    let a = app(1);
    let items = a.open_items();
    let last = items.last().expect("items");
    assert_eq!(last.0, crate::settings_ui::PLACE_REFERENCE);
    assert!(a.is_place_reference_item(items.len() - 1));
    assert!(!a.is_place_reference_item(items.len() - 2));
    assert!(!a.is_place_reference_item(items.len()));
}

#[test]
fn a_header_with_a_bad_checksum_is_damaged_not_too_large() {
    let mut bytes = header_only(5000, 10);
    bytes[29] ^= 1;
    let refused = png::dimensions(&bytes).expect_err("refused");
    assert!(matches!(refused, pikslar_format::FormatError::Corrupt(_)));
}

/// One lone pixel on clear, which Pixel Lint marks as a stray on a paint layer.
fn speckled() -> Vec<u8> {
    let mut px = vec![0; 16 * 16];
    px[8 * 16 + 8] = PHOTO;
    png::encode(&png::Image {
        width: 16,
        height: 16,
        px,
    })
    .expect("encoded")
}

#[test]
fn pixel_lint_finds_nothing_on_a_reference() {
    let mut a = app(1);
    a.lint.enabled = true;
    let path = temp_file("speck", &speckled());
    a.place_reference_from(&path).expect("placed");
    a.refresh_lint();
    assert!(a.lint.findings.is_empty());
    // The same picture opened as a sprite is marked, so the check above means something.
    let mut b = app(1);
    b.lint.enabled = true;
    let opened = crate::files::open(&path).expect("opens");
    b.replace_document(opened.doc, None);
    b.refresh_lint();
    assert!(!b.lint.findings.is_empty());
}
