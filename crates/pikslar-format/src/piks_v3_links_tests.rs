//! Links across layers in version 3: one image record however many layers show it, read back
//! as it was, and a file that only reloads to itself.

use super::*;
use crate::piks::{load, load_recovery, save, save_recovery};
use pikslar_core::{CelDrag, DragKind, Grid, Landing};

/// Three layers of three frames, each cel drawn, with the first cel of layer 0 linked to frame
/// 1 of layer 2, moved apart there, and a hold on layer 1.
fn linked() -> Document {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    for at in 1..3 {
        doc.add_layer(at, ColorMode::Rgba).expect("room");
        doc.add_frame(at, false).expect("room");
    }
    for layer in 0..3 {
        for frame in 0..3 {
            let value = 0xff00_0000 | (layer * 10 + frame + 1) as u32;
            doc.set_cel(layer, frame, Grid::from_cells(8, 8, &[value; 64]))
                .expect("fits");
        }
    }
    doc.drag_cels(&CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 2,
        to_frame: 1,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    })
    .expect("linked");
    let apart = doc.layers()[2].cel(1).moved_to(-2, 3).expect("in range");
    doc.put_cel(2, 1, apart).expect("frame");
    doc.hold_across(1, 0, 0, 2).expect("frames");
    doc
}

#[test]
fn a_drawing_shown_on_two_layers_is_one_image_record() {
    let doc = linked();
    let (images, _) = number_images(&doc);
    // Nine cels: layer 1 holds one drawing on three frames, layers 0 and 2 share one.
    assert_eq!(images.len(), 9 - 2 - 1);
    let back = load(&save(&doc)).expect("opens");
    assert_eq!(back.layers(), doc.layers(), "every cel, link and place");
    assert!(back.layers()[0].cel(0).same_image(back.layers()[2].cel(1)));
    assert_eq!(back.check_links(), Ok(()));
}

#[test]
fn a_linked_project_reloads_to_the_same_bytes() {
    let saved = save(&linked());
    let again = save(&load(&saved).expect("opens"));
    assert_eq!(again, saved);
}

#[test]
fn a_cel_given_a_drawing_of_its_own_is_written_alone() {
    let mut doc = linked();
    let own = Grid::from_cells(8, 8, &[0xff00_00aa; 64]).into_image();
    let cel = pikslar_core::Cel::at(own, -2, 3).expect("in range");
    doc.put_cel(2, 1, cel).expect("frame");
    assert_eq!(doc.layers()[2].link(1), 0, "it leaves the link");
    assert_eq!(doc.check_links(), Ok(()));
    let back = load(&save(&doc)).expect("opens");
    assert_eq!(back.layers()[2].link(1), 0);
    assert_eq!(
        back.layers()[0].link(0),
        0,
        "a link left with one cel is none"
    );
    assert_eq!(back.layers()[2].cel(1).get(-2, 3), 0xff00_00aa);
}

#[test]
fn a_recovery_copy_keeps_links_across_layers() {
    let doc = linked();
    let back = load_recovery(&save_recovery(&doc, None)).expect("opens");
    assert_eq!(back.doc.layers(), doc.layers());
}

/// The bytes of `doc` saved, with each cel record's link number replaced, resealed.
fn with_links(doc: &Document, links: &[Vec<u32>]) -> Vec<u8> {
    let mut bytes = save(doc);
    let (layers, frames) = (doc.layers().len(), doc.frames.len());
    let end = bytes.len() - 8;
    let start = end - layers * frames * 14;
    for (layer, ids) in links.iter().enumerate() {
        for (frame, id) in ids.iter().enumerate() {
            let at = start + (layer * frames + frame) * 14;
            assert_eq!(bytes[at], 13, "a cel record");
            bytes[at + 10..at + 14].copy_from_slice(&id.to_le_bytes());
        }
    }
    let n = bytes.len() - 4;
    let crc = crate::crc32(&[&bytes[..n]]);
    bytes[n..].copy_from_slice(&crc.to_le_bytes());
    bytes
}

/// An RGBA, a palette and an RGBA layer of three frames, each cel with a drawing of its own
/// but the first two of layer 0, which share one, as do the last two of layer 1.
fn mixed() -> Document {
    let modes = [ColorMode::Rgba, ColorMode::Palette, ColorMode::Rgba];
    let mut doc = Document::new(8, 8, modes[0]).expect("valid");
    for (at, &mode) in modes.iter().enumerate().skip(1) {
        doc.add_layer(at, mode).expect("room");
    }
    for at in 1..3 {
        doc.add_frame(at, false).expect("room");
    }
    for layer in [0, 2] {
        for frame in 0..3 {
            let value = 0xff00_0000 | (layer * 10 + frame + 1) as u32;
            let drawing = Grid::from_cells(8, 8, &[value; 64]);
            doc.set_cel(layer, frame, drawing).expect("fits");
        }
    }
    let shared = doc.layers()[0].cel(0).clone();
    doc.put_cel(0, 1, shared).expect("frame");
    doc.set_cel(1, 1, Grid::from_cells(8, 8, &[1; 64]))
        .expect("fits");
    let shared = doc.layers()[1].cel(1).clone();
    doc.put_cel(1, 2, shared).expect("frame");
    doc
}

fn links_of(doc: &Document) -> Vec<Vec<u32>> {
    doc.layers().iter().map(|l| l.links().collect()).collect()
}

#[test]
fn a_link_joining_two_modes_keeps_each_layers_own_hold() {
    let doc = mixed();
    let bytes = with_links(&doc, &[vec![7, 7, 0], vec![7, 7, 7], vec![0, 0, 0]]);
    let (back, notes) = crate::piks::load_noted(&bytes).expect("opens");
    assert_eq!(notes.damaged, ["holds"], "something was wrong");
    assert_eq!(
        links_of(&back),
        [vec![1, 1, 0], vec![0, 2, 2], vec![0, 0, 0]],
        "each layer keeps the frames that share one drawing, as a hold"
    );
    assert_eq!(back.check_links(), Ok(()));
    let pixels = |d: &Document| -> Vec<Vec<u32>> {
        let cels = d.layers().iter().flat_map(|l| l.cels());
        cels.map(|c| c.to_cells(8, 8)).collect()
    };
    assert_eq!(pixels(&back), pixels(&doc), "every drawing kept");
}

#[test]
fn a_broken_link_leaves_the_sound_ones_alone() {
    let doc = mixed();
    let bytes = with_links(&doc, &[vec![7, 0, 9], vec![7, 0, 0], vec![9, 0, 0]]);
    let mut doc = doc;
    let shared = doc.layers()[0].cel(2).clone();
    doc.put_cel(2, 0, shared).expect("frame");
    let bytes_shared = with_links(&doc, &[vec![7, 0, 9], vec![7, 0, 0], vec![9, 0, 0]]);
    let (back, notes) = crate::piks::load_noted(&bytes_shared).expect("opens");
    assert_eq!(notes.damaged, ["holds"]);
    assert_eq!(links_of(&back), [vec![0, 0, 1], vec![0; 3], vec![1, 0, 0]]);
    assert!(back.layers()[0].cel(2).same_image(back.layers()[2].cel(0)));
    let (back, notes) = crate::piks::load_noted(&bytes).expect("opens");
    assert_eq!(notes.damaged, ["holds"], "two drawings under one number");
    assert!(links_of(&back).iter().flatten().all(|&id| id == 0));
}

#[test]
fn a_file_with_only_sound_links_is_not_damaged() {
    let doc = mixed();
    let bytes = with_links(&doc, &[vec![7, 7, 3], vec![0, 4, 4], vec![0, 0, 0]]);
    let (back, notes) = crate::piks::load_noted(&bytes).expect("opens");
    assert!(notes.damaged.is_empty(), "{:?}", notes.damaged);
    assert_eq!(links_of(&back), [vec![2, 2, 0], vec![0, 1, 1], vec![0; 3]]);
}
