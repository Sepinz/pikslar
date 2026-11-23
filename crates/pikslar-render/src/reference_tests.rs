//! Reference layers show on the screen and in nothing made from the document.

use crate::bands::BandSource;
use crate::compose::Compositor;
use crate::flatten::flatten_document;
use pikslar_core::{rgba, Cel, ColorMode, Damage, Document, Editor, Grid, LayerCmd};
use std::sync::Arc;

const SIDE: usize = 16;
const INK: u32 = rgba(200, 30, 30, 255);
const PHOTO: u32 = rgba(30, 30, 200, 255);

fn filled(color: u32, at: usize) -> Cel {
    let mut px = vec![0u32; SIDE * SIDE];
    px[at] = color;
    Cel::new(Grid::from_cells(SIDE, SIDE, &px))
}

/// A paint layer with one dot and a reference covering another, over two frames, the paint
/// layer different on each so the onion skin has something to show.
fn editor() -> Editor {
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid");
    doc.add_frame(1, false).expect("frame");
    doc.put_cel(0, 0, filled(INK, 0)).expect("cel");
    doc.put_cel(0, 1, filled(INK, 3)).expect("cel");
    let mut e = Editor::new(doc);
    let image = Arc::new(filled(PHOTO, 5).image().clone());
    let place = LayerCmd::PlaceReference {
        below: 0,
        image,
        name: "Photo".into(),
    };
    e.apply(place).expect("placed");
    e.frame = 1;
    e
}

fn hide(e: &mut Editor, index: usize) {
    let layer = e.layer_id(index).expect("layer");
    let hide = LayerCmd::SetVisible {
        layer,
        visible: false,
    };
    e.apply(hide).expect("hidden");
}

fn screen(e: &Editor, onion: bool) -> Vec<u32> {
    let mut comp = Compositor::new(SIDE, SIDE);
    comp.onion = onion;
    comp.onion_depth = 1;
    comp.update(e, Damage::All, &mut Vec::new());
    comp.pixels().to_vec()
}

#[test]
fn the_screen_shows_a_reference_and_export_leaves_it_out() {
    let e = editor();
    assert_ne!(screen(&e, false)[5], 0, "shown on the screen");
    let mut comp = Compositor::new(SIDE, SIDE);
    let flat = flatten_document(e.doc(), 1, &mut comp);
    assert_eq!(flat[5], 0, "left out of a flattened frame");
    assert_eq!(flat[3], INK);
    for source in [
        BandSource::new(e.doc()),
        BandSource::with_layers(e.doc(), &[0, 1]),
    ] {
        let mut band = Vec::new();
        source.band(1, 0, &mut band);
        assert_eq!(band[5], 0, "left out of a band");
        assert_eq!(band[3], INK);
    }
}

#[test]
fn a_reference_alone_exports_as_an_empty_picture() {
    let mut e = editor();
    hide(&mut e, 0);
    let mut comp = Compositor::new(SIDE, SIDE);
    assert!(flatten_document(e.doc(), 0, &mut comp)
        .iter()
        .all(|&p| p == 0));
}

#[test]
fn a_hidden_reference_is_not_shown_on_the_screen() {
    let mut e = editor();
    hide(&mut e, 1);
    assert_eq!(screen(&e, false)[5], 0);
}

#[test]
fn the_onion_skin_ghosts_paint_layers_only() {
    let e = editor();
    let (plain, ghosted) = (screen(&e, false), screen(&e, true));
    assert_ne!(
        ghosted[0], plain[0],
        "the paint layer's last frame is ghosted"
    );
    assert_eq!(ghosted[5], plain[5], "the reference adds no ghost");
}
