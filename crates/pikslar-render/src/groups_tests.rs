//! A hidden group hides what it holds, on the screen and in everything made from the file.

use crate::bands::BandSource;
use crate::compose::Compositor;
use crate::flatten::flatten_document;
use pikslar_core::{rgba, Cel, ColorMode, Damage, Document, Editor, Grid, LayerCmd};

const SIDE: usize = 16;
const INK: u32 = rgba(200, 30, 30, 255);
const LEAF: u32 = rgba(30, 160, 30, 255);

fn dot(color: u32, at: usize) -> Cel {
    let mut px = vec![0u32; SIDE * SIDE];
    px[at] = color;
    Cel::new(Grid::from_cells(SIDE, SIDE, &px))
}

/// Two frames. Layer 0 has a dot at 0; layer 1, in a group, a dot at 5 that moves to 6.
fn editor(hidden: bool) -> Editor {
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid");
    doc.add_frame(1, false).expect("frame");
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    doc.put_cel(0, 0, dot(INK, 0)).expect("cel");
    doc.put_cel(0, 1, dot(INK, 0)).expect("cel");
    doc.put_cel(1, 0, dot(LEAF, 5)).expect("cel");
    doc.put_cel(1, 1, dot(LEAF, 6)).expect("cel");
    let mut e = Editor::new(doc);
    let layer = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::Group { layer }).expect("grouped");
    let group = e.layer_id(2).expect("group");
    let visible = !hidden;
    e.apply(LayerCmd::SetVisible {
        layer: group,
        visible,
    })
    .expect("set");
    e.frame = 1;
    e
}

fn screen(e: &Editor, onion: bool) -> Vec<u32> {
    let mut comp = Compositor::new(SIDE, SIDE);
    comp.onion = onion;
    comp.onion_depth = 1;
    comp.update(e, Damage::All, &mut Vec::new());
    comp.pixels().to_vec()
}

#[test]
fn a_shown_group_shows_what_it_holds_everywhere() {
    let e = editor(false);
    assert_ne!(screen(&e, false)[6], 0);
    let mut comp = Compositor::new(SIDE, SIDE);
    assert_eq!(flatten_document(e.doc(), 1, &mut comp)[6], LEAF);
    let mut band = Vec::new();
    BandSource::new(e.doc()).band(1, 0, &mut band);
    assert_eq!(band[6], LEAF);
}

#[test]
fn a_hidden_group_hides_what_it_holds_everywhere() {
    let e = editor(true);
    let (plain, ghosted) = (screen(&e, false), screen(&e, true));
    assert_eq!(plain[6], 0, "not on the screen");
    assert_eq!(ghosted[5], 0, "no ghost");
    assert_ne!(plain[0], 0, "the layer outside still shows");
    let mut comp = Compositor::new(SIDE, SIDE);
    let flat = flatten_document(e.doc(), 1, &mut comp);
    assert_eq!((flat[0], flat[6]), (INK, 0), "nor in a flattened frame");
    let mut band = Vec::new();
    BandSource::new(e.doc()).band(1, 0, &mut band);
    assert_eq!(band[6], 0, "nor in a band");
}

#[test]
fn a_plain_layer_beside_an_empty_group_is_still_read_straight() {
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid");
    doc.put_cel(0, 0, dot(rgba(10, 20, 30, 77), 2))
        .expect("cel");
    let mut e = Editor::new(doc);
    let layer = e.layer_id(0).expect("layer");
    e.apply(LayerCmd::Group { layer }).expect("grouped");
    let mut comp = Compositor::new(SIDE, SIDE);
    let flat = flatten_document(e.doc(), 0, &mut comp);
    assert_eq!(
        flat[2],
        rgba(10, 20, 30, 77),
        "exact, not through premultiplying"
    );
}
