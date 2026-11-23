//! The growth budget measures the document only when the fuel paid since the last measure
//! could have filled it, which holds only while no edit adds more than
//! `MOST_BYTES_PER_UNIT` bytes for each fuel unit it pays. These runs make the edits that add
//! the most cels at once and check that bound with the document's own measure.

mod common;

use pikslar_bind::MOST_BYTES_PER_UNIT;
use pikslar_core::{ColorMode, Document, Editor, FrameCmd, LayerCmd, LayerKind};

fn editor(frames: usize) -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("a canvas"));
    for at in 1..frames {
        e.apply(FrameCmd::Add { at, copy: false }).expect("room");
    }
    e
}

/// Runs `source` and checks what it added to the document against the fuel it used.
fn check_bound(e: &mut Editor, source: &str) {
    let before = e.doc().held_bytes();
    let out = common::run(e, source);
    assert!(out.result.is_ok(), "{:?}", out.result);
    let added = e.doc().held_bytes().saturating_sub(before) as u64;
    assert!(added > 0, "the edit added cels");
    assert!(
        added <= out.fuel_used * MOST_BYTES_PER_UNIT,
        "{added} bytes for {} fuel units",
        out.fuel_used
    );
}

#[test]
fn duplicating_the_largest_group_pays_for_every_cel_it_copies() {
    let mut e = editor(512);
    let layer = e.layer_id(0).expect("a layer");
    e.apply(LayerCmd::Group { layer }).expect("grouped");
    // Each new layer goes in below the group's row, so into the group: 128 rows in all,
    // half of what a document may hold, so the copy fills it.
    for _ in 0..126 {
        e.apply(LayerCmd::Add {
            at: 1,
            mode: ColorMode::Rgba,
        })
        .expect("room");
    }
    let doc = e.doc();
    let group = doc.layers().iter().find(|l| l.kind == LayerKind::Group);
    let group = group.expect("the group").persisted_id().get();
    assert_eq!(doc.block(doc.layers().len() - 1), (0, 127));
    check_bound(&mut e, &format!("duplicate_layer({group});"));
    assert_eq!(e.doc().layers().len(), 256);
}

#[test]
fn a_new_layer_on_the_longest_animation_stays_within_the_bound() {
    let mut e = editor(pikslar_core::MAX_FRAMES);
    check_bound(&mut e, "add_layer(\"x\");");
}

#[test]
fn a_copied_frame_on_the_most_layers_stays_within_the_bound() {
    let mut e = editor(1);
    for _ in 1..256 {
        e.apply(LayerCmd::Add {
            at: 0,
            mode: ColorMode::Rgba,
        })
        .expect("room");
    }
    check_bound(&mut e, "copy_frame(1);");
}
