//! Rename, duplicate and merge down: what each keeps, what each refuses, and their undo.

use crate::blend::{mix, unpremultiply};
use crate::color::rgba;
use crate::command::{LayerCmd, StepCmd, MAX_LAYER_NAME};
use crate::document::{BlendMode, ColorMode, Document, LimitError, MAX_LAYERS};
use crate::editor::Editor;
use crate::layer::Cel;
use crate::offset_tests::image_of;
use std::collections::HashMap;
use std::sync::Arc;

const RED: u32 = rgba(200, 10, 10, 255);
const BLUE: u32 = rgba(10, 40, 220, 160);

fn cel_of(pixels: &[((i32, i32), u32)]) -> Cel {
    Cel::from_image(image_of(&pixels.iter().copied().collect::<HashMap<_, _>>()))
}

/// Two RGBA layers over three frames. Frames 0 and 1 hold one drawing on both layers; frame 2
/// has drawings of its own. Layer 1 has a pixel far past the 32 by 32 canvas.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(32, 32, ColorMode::Rgba).expect("valid"));
    e.doc.add_layer(1, ColorMode::Rgba).expect("layer");
    for _ in 0..2 {
        e.doc.add_frame(e.doc.frames.len(), false).expect("frame");
    }
    let lower = cel_of(&[((2, 2), RED), ((5, 5), RED), ((-5, -20), RED)]);
    let upper = cel_of(&[((5, 5), BLUE), ((60, 70), BLUE), ((-3, 3), BLUE)]);
    for frame in 0..3 {
        let own = |c: &Cel| {
            if frame == 2 {
                c.with_own_image()
            } else {
                c.clone()
            }
        };
        e.doc.put_cel(0, frame, own(&lower)).expect("cel");
        e.doc.put_cel(1, frame, own(&upper)).expect("cel");
    }
    e.doc.set_holds(0, &[1, 1, 0]).expect("holds");
    e.doc.set_holds(1, &[1, 1, 0]).expect("holds");
    e.doc.layers[1].blend = BlendMode::Screen;
    e.doc.layers[1].opacity = 200;
    e.layer = 1;
    e
}

fn apply(e: &mut Editor, make: impl Fn(crate::command::LayerId) -> LayerCmd, index: usize) {
    let layer = e.layer_id(index).expect("layer");
    e.apply(make(layer)).expect("applied");
}

fn merge(e: &mut Editor, index: usize) -> Result<(), LimitError> {
    let layer = e.layer_id(index).expect("layer");
    e.apply(LayerCmd::MergeDown { layer }).map(|_| ())
}

#[test]
fn merging_draws_each_pixel_as_the_two_layers_show_it_on_and_off_the_canvas() {
    let mut e = editor();
    let (lower, upper) = (e.doc.layers[0].clone(), e.doc.layers[1].clone());
    merge(&mut e, 1).expect("merged");
    assert_eq!(e.doc.layers.len(), 1);
    assert_eq!(e.layer, 0);
    let merged = &e.doc.layers[0];
    for frame in 0..3 {
        for (x, y) in [(2, 2), (5, 5), (60, 70), (9, 9), (-5, -20), (-3, 3)] {
            let below = mix(0, lower.cel(frame).get(x, y), lower.blend, 255);
            let shown = mix(below, upper.cel(frame).get(x, y), upper.blend, 200);
            assert_eq!(merged.cel(frame).get(x, y), unpremultiply(shown), "{x},{y}");
        }
    }
    assert_ne!(
        merged.cel(0).get(60, 70),
        0,
        "a pixel past the canvas is kept"
    );
}

#[test]
fn the_merged_layer_is_the_lower_ones_and_plain() {
    let mut e = editor();
    e.doc.layers[0].name = "Ink".into();
    let id = e.doc.layers[0].persisted_id();
    merge(&mut e, 1).expect("merged");
    let merged = &e.doc.layers[0];
    assert_eq!((merged.name.as_str(), merged.persisted_id()), ("Ink", id));
    assert_eq!((merged.blend, merged.opacity), (BlendMode::Normal, 255));
    assert!(merged.visible && !merged.locked);
    assert_eq!(merged.mode, ColorMode::Rgba);
}

#[test]
fn frames_that_showed_one_pair_of_drawings_share_the_merged_drawing() {
    let mut e = editor();
    merge(&mut e, 1).expect("merged");
    let merged = &e.doc.layers[0];
    assert!(merged.cel(0).same_image(merged.cel(1)));
    assert!(!merged.cel(0).same_image(merged.cel(2)));
    assert_ne!(merged.link(0), 0);
    assert_eq!(merged.link(0), merged.link(1));
    assert_eq!(merged.link(2), 0);
}

#[test]
fn a_hold_after_a_lone_frame_is_kept_too() {
    let mut e = editor();
    let lone = (
        e.doc.layers[0].cel(2).clone(),
        e.doc.layers[1].cel(2).clone(),
    );
    let held = (
        e.doc.layers[0].cel(0).clone(),
        e.doc.layers[1].cel(0).clone(),
    );
    for (layer, (first, rest)) in [(0, (&lone.0, &held.0)), (1, (&lone.1, &held.1))] {
        e.doc.put_cel(layer, 0, first.clone()).expect("cel");
        e.doc.put_cel(layer, 2, rest.clone()).expect("cel");
        e.doc.set_holds(layer, &[0, 1, 1]).expect("holds");
    }
    merge(&mut e, 1).expect("merged");
    let merged = &e.doc.layers[0];
    assert!(merged.cel(1).same_image(merged.cel(2)));
    assert!(!merged.cel(0).same_image(merged.cel(1)));
    assert_eq!((merged.link(0), merged.link(1) != 0), (0, true));
    assert_eq!(merged.link(1), merged.link(2));
}

#[test]
fn a_merge_holds_its_numbers_clear_of_the_other_layers() {
    let mut e = editor();
    e.doc.add_layer(0, ColorMode::Rgba).expect("layer");
    e.doc.set_holds(0, &[1, 1, 0]).expect("holds");
    merge(&mut e, 2).expect("merged");
    assert_ne!(e.doc.layers[0].link(0), e.doc.layers[1].link(0));
}

#[test]
fn the_merged_layer_becomes_the_active_one() {
    let mut e = editor();
    e.doc.add_layer(2, ColorMode::Rgba).expect("layer");
    e.layer = 1;
    merge(&mut e, 1).expect("merged");
    assert_eq!((e.doc.layers.len(), e.layer), (2, 0));
}

#[test]
fn merge_is_one_undo_step_back_to_the_same_document() {
    let mut e = editor();
    let before = e.doc.clone();
    merge(&mut e, 1).expect("merged");
    let after = e.doc.clone();
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.doc, before);
    assert_eq!(e.layer, 1);
    e.apply(StepCmd::Redo).expect("redo");
    assert_eq!(e.doc, after);
}

#[test]
fn merge_is_refused_with_nothing_changed() {
    let mut e = editor();
    let before = e.doc.clone();
    assert_eq!(merge(&mut e, 0), Err(LimitError::NothingBelow));
    e.doc.layers[0].visible = false;
    assert_eq!(merge(&mut e, 1), Err(LimitError::HiddenMerge));
    e.doc.layers[0].visible = true;
    e.doc.layers[1].visible = false;
    assert_eq!(merge(&mut e, 1), Err(LimitError::HiddenMerge));
    e.doc.layers[1].visible = true;
    e.doc.layers[0].locked = true;
    assert_eq!(merge(&mut e, 1), Err(LimitError::Locked));
    e.doc.layers[0].locked = false;
    e.doc.layers[1].locked = true;
    assert_eq!(merge(&mut e, 1), Err(LimitError::Locked));
    e.doc.layers[1].locked = false;
    assert_eq!(e.doc, before);
    assert_eq!(e.history.undo_depth(), 0);
}

/// Two palette layers of opaque table colours, the upper one at `opacity`.
fn palette_editor(opacity: u8) -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Palette).expect("valid"));
    let table = Arc::make_mut(&mut e.doc.colors);
    let (a, b) = (
        table.id_for(RED).expect("room"),
        table.id_for(rgba(1, 2, 3, 255)).expect("room"),
    );
    e.doc.add_layer(1, ColorMode::Palette).expect("layer");
    e.doc
        .put_cel(0, 0, cel_of(&[((1, 1), a), ((2, 2), a)]))
        .expect("cel");
    e.doc
        .put_cel(1, 0, cel_of(&[((2, 2), b), ((3, 3), b)]))
        .expect("cel");
    e.doc.layers[1].opacity = opacity;
    e
}

#[test]
fn two_palette_layers_merge_to_a_palette_layer_when_every_colour_is_in_the_table() {
    let mut e = palette_editor(255);
    let colors = e.doc.colors.clone();
    merge(&mut e, 1).expect("merged");
    let merged = &e.doc.layers[0];
    assert_eq!(merged.mode, ColorMode::Palette);
    let shown = |x, y| colors.color(merged.cel(0).get(x, y));
    assert_eq!(shown(1, 1), RED);
    assert_eq!(shown(2, 2), rgba(1, 2, 3, 255));
    assert_eq!(merged.cel(0).get(9, 9), 0);
    assert_eq!(e.doc.colors, colors, "the table is untouched");
}

#[test]
fn a_colour_outside_the_table_makes_the_merge_full_colour() {
    let mut e = palette_editor(100);
    merge(&mut e, 1).expect("merged");
    let merged = &e.doc.layers[0];
    assert_eq!(merged.mode, ColorMode::Rgba);
    assert_eq!(merged.cel(0).get(1, 1), RED);
    assert_eq!(merged.cel(0).get(3, 3) >> 24, 100);
}

#[test]
fn a_duplicate_shares_every_drawing_and_holds_the_same_frames_apart() {
    let mut e = editor();
    e.layer = 0;
    e.doc.layers[0].name = "Ink".into();
    apply(&mut e, |layer| LayerCmd::Duplicate { layer }, 0);
    assert_eq!(e.doc.layers.len(), 3);
    assert_eq!(e.layer, 1);
    let (source, copy) = (&e.doc.layers[0], &e.doc.layers[1]);
    assert_eq!(copy.name, "Ink copy");
    assert_ne!(copy.persisted_id(), source.persisted_id());
    assert_ne!(copy.id(), source.id());
    for frame in 0..3 {
        assert!(
            copy.cel(frame).same_image(source.cel(frame)),
            "no pixel copied"
        );
    }
    assert_eq!(copy.link(0), copy.link(1));
    assert_eq!(copy.link(2), 0);
    let others: Vec<u32> = [0, 2].iter().map(|&i| e.doc.layers[i].link(0)).collect();
    assert!(!others.contains(&copy.link(0)), "its hold is its own");
}

#[test]
fn duplicate_undoes_in_one_step_and_is_refused_at_the_layer_limit() {
    let mut e = editor();
    let before = e.doc.clone();
    apply(&mut e, |layer| LayerCmd::Duplicate { layer }, 1);
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.doc, before);
    while e.doc.layers.len() < MAX_LAYERS {
        e.doc.add_layer(0, ColorMode::Rgba).expect("layer");
    }
    let full = e.doc.clone();
    let layer = e.layer_id(0).expect("layer");
    let refused = e.apply(LayerCmd::Duplicate { layer });
    assert_eq!(refused.map(|_| ()), Err(LimitError::TooManyLayers));
    assert_eq!(e.doc, full);
}

#[test]
fn rename_is_one_step_trimmed_and_ignores_an_empty_name() {
    let mut e = editor();
    let before = e.doc.clone();
    let rename = |name: &'static str| {
        move |layer| LayerCmd::Rename {
            layer,
            name: name.into(),
        }
    };
    apply(&mut e, rename("  Shadows \t"), 0);
    assert_eq!(e.doc.layers[0].name, "Shadows");
    assert_eq!(e.history.undo_depth(), 1);
    apply(&mut e, rename("   "), 0);
    apply(&mut e, rename("Shadows"), 0);
    assert_eq!(e.history.undo_depth(), 1, "no step for nothing");
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.doc, before);
}

#[test]
fn a_long_name_is_cut_and_control_characters_are_dropped() {
    let mut e = editor();
    let long: String = "é".repeat(MAX_LAYER_NAME + 9) + "\n";
    let layer = e.layer_id(0).expect("layer");
    e.apply(LayerCmd::Rename {
        layer,
        name: format!("a\u{7}{long}"),
    })
    .expect("renamed");
    let name = &e.doc.layers[0].name;
    assert_eq!(name.chars().count(), MAX_LAYER_NAME);
    assert!(name.starts_with("aé") && !name.chars().any(char::is_control));
}
