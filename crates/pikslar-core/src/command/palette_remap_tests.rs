//! The lossless remaps and Replace colour through the command core: one step each, the same
//! colours on every layer, and the lock rule for Replace colour.

use crate::color::rgba;
use crate::command::{CelCmd, DrawCmd, FloatingCmd, FrameCmd, LayerCmd, PaletteCmd, SelectionCmd};
use crate::custom_brush::{Brush, BrushChoice, BrushPaint};
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};
use crate::palette_ops::remap::RemapKind;
use crate::{CelDrag, DragKind, Landing};
use std::sync::Arc;

const RED: u32 = rgba(255, 0, 0, 255);
const GREEN: u32 = rgba(0, 255, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const GREY: u32 = rgba(128, 128, 128, 255);
const ORANGE: u32 = rgba(255, 128, 0, 255);

/// Swatches grey 1, blue 2, red 3, green 4, and an orange 5 that is no swatch. Layer 0 has a
/// row of each colour: red at y 0, green at y 1, blue at y 2, orange at y 3.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Palette).expect("valid"));
    for color in [GREY, BLUE, RED, GREEN] {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    for (y, color) in [RED, GREEN, BLUE, ORANGE].into_iter().enumerate() {
        e.color = color;
        let path = vec![(0, y as i32), (3, y as i32)];
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        })
        .expect("stroke");
    }
    e
}

/// The colour each pixel of a layer's frame shows.
fn looks(e: &Editor, layer: usize, frame: usize) -> Vec<u32> {
    let cel = e.doc.layers[layer].cel(frame);
    (0..16 * 16)
        .map(|i| e.doc.colors.color(cel.get(i % 16, i / 16)))
        .collect()
}

fn ids_at(e: &Editor, layer: usize, y: i32) -> u32 {
    e.doc.layers[layer].cel(0).get(0, y)
}

#[test]
fn renumbering_gives_swatches_ids_in_order_and_keeps_every_colour() {
    let mut e = editor();
    e.apply(PaletteCmd::MoveSwatch { from: 0, to: 3 })
        .expect("moved");
    let (before, version) = (looks(&e, 0, 0), e.doc.colors.version());
    let depth = e.history.undo_depth();
    let out = e
        .apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder))
        .expect("remapped");
    assert!(out.changed);
    assert_eq!(e.history.undo_depth(), depth + 1, "one step");
    // Swatches were blue, red, green, grey; orange follows.
    let t = &e.doc.colors;
    assert_eq!(t.swatches(), [1, 2, 3, 4]);
    assert_eq!(&t.colors()[1..], [BLUE, RED, GREEN, GREY, ORANGE]);
    assert_eq!(looks(&e, 0, 0), before);
    assert_eq!(ids_at(&e, 0, 0), 2, "the red row");
    assert!(t.version() > version, "cached drawings are stale");
    assert!(e.undo());
    assert_eq!(e.doc.colors.swatches(), [2, 3, 4, 1]);
    assert_eq!(ids_at(&e, 0, 0), 3);
    assert_eq!(looks(&e, 0, 0), before);
}

#[test]
fn renumbering_follows_groups_and_keeps_their_order() {
    let mut e = editor();
    e.apply(PaletteCmd::MoveSwatch { from: 0, to: 3 })
        .expect("moved");
    let ids: Arc<[u32]> = [4, 3].into_iter().collect();
    e.apply(PaletteCmd::AddGroup {
        name: "g".into(),
        ids,
    })
    .expect("group");
    e.apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder))
        .expect("remapped");
    // Green and red were ids 4 and 3; they are now 3 and 2.
    assert_eq!(e.doc.colors.groups()[0].ids, [3, 2]);
}

#[test]
fn merging_moves_pixels_swatches_and_groups_to_the_lowest_id() {
    let mut e = editor();
    // Blue 2 becomes red: ids 2 and 3 both show red.
    e.apply(PaletteCmd::Recolor { id: 2, color: RED })
        .expect("recolored");
    let ids: Arc<[u32]> = [3, 4].into_iter().collect();
    e.apply(PaletteCmd::AddGroup {
        name: "g".into(),
        ids,
    })
    .expect("group");
    let before = looks(&e, 0, 0);
    let colors = e.doc.colors.colors().to_vec();
    let out = e
        .apply(PaletteCmd::Remap(RemapKind::MergeDuplicates))
        .expect("merged");
    assert!(out.changed);
    assert_eq!(looks(&e, 0, 0), before);
    assert_eq!(ids_at(&e, 0, 0), 2, "red pixels moved to the lowest id");
    assert_eq!(ids_at(&e, 0, 2), 2, "so did what was blue");
    assert_eq!(e.doc.colors.colors(), colors, "no colour moves or goes");
    assert_eq!(e.doc.colors.swatches(), [1, 2, 4]);
    assert_eq!(e.doc.colors.groups()[0].ids, [2, 4]);
    let again = e
        .apply(PaletteCmd::Remap(RemapKind::MergeDuplicates))
        .expect("ok");
    assert!(!again.changed, "nothing left to merge");
}

#[test]
fn removing_unused_closes_ids_up_and_keeps_swatches_groups_and_pixels() {
    let mut e = editor();
    // Grey (1) is a swatch with no pixel: it stays. Take its swatch away and it goes.
    e.apply(PaletteCmd::RemoveSwatch { id: 1 })
        .expect("removed");
    let before = looks(&e, 0, 0);
    let out = e
        .apply(PaletteCmd::Remap(RemapKind::RemoveUnused))
        .expect("removed");
    assert!(out.changed);
    let t = &e.doc.colors;
    assert_eq!(&t.colors()[1..], [BLUE, RED, GREEN, ORANGE]);
    assert_eq!(t.swatches(), [1, 2, 3]);
    assert_eq!(looks(&e, 0, 0), before);
    assert!(e.undo());
    assert_eq!(e.doc.colors.len(), 6);
    assert_eq!(looks(&e, 0, 0), before);
}

#[test]
fn a_swatch_nothing_draws_with_survives_remove_unused() {
    let mut e = editor();
    e.apply(PaletteCmd::AddSwatch {
        color: rgba(1, 2, 3, 255),
    })
    .expect("room");
    e.apply(PaletteCmd::Remap(RemapKind::RemoveUnused))
        .expect("ok");
    assert_eq!(e.doc.colors.len(), 7, "grey and the new swatch stay");
}

#[test]
fn a_remap_that_changes_nothing_is_no_step() {
    let mut e = editor();
    e.apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder))
        .expect("ok");
    for kind in [
        RemapKind::ToSwatchOrder,
        RemapKind::MergeDuplicates,
        RemapKind::RemoveUnused,
    ] {
        let (depth, revision) = (e.history.undo_depth(), e.revision);
        let out = e.apply(PaletteCmd::Remap(kind)).expect("ok");
        assert!(!out.changed, "{kind:?}");
        assert_eq!((e.history.undo_depth(), e.revision), (depth, revision));
    }
}

/// Layer 0 is locked and draws red; layer 1 draws red and green.
fn two_layers() -> Editor {
    let mut e = editor();
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Palette,
    })
    .expect("layer");
    for (y, color) in [RED, GREEN].into_iter().enumerate() {
        e.color = color;
        let path = vec![(5, y as i32), (6, y as i32)];
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        })
        .expect("stroke");
    }
    e.doc.layers[0].locked = true;
    e
}

#[test]
fn a_remap_reaches_a_locked_layer_because_it_changes_no_colour() {
    let mut e = two_layers();
    e.apply(PaletteCmd::MoveSwatch { from: 0, to: 3 })
        .expect("moved");
    let (a, b) = (looks(&e, 0, 0), looks(&e, 1, 0));
    e.apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder))
        .expect("a lock does not refuse it");
    assert_eq!((looks(&e, 0, 0), looks(&e, 1, 0)), (a, b));
    assert_eq!(ids_at(&e, 0, 0), 2, "the locked layer was renumbered too");
}

#[test]
fn replace_everywhere_is_refused_if_a_locked_layer_shows_the_colour() {
    let mut e = two_layers();
    let (depth, before) = (e.history.undo_depth(), looks(&e, 0, 0));
    let replace = PaletteCmd::ReplaceColor {
        from: 3,
        to: 4,
        layer: None,
    };
    assert_eq!(e.apply(replace), Err(LimitError::Locked));
    assert_eq!(e.history.undo_depth(), depth, "no step, nothing half done");
    assert_eq!(looks(&e, 0, 0), before);
    assert_eq!(
        looks(&e, 1, 0)[5],
        RED,
        "the open layer is not changed either"
    );
}

#[test]
fn replace_everywhere_passes_a_locked_layer_that_lacks_the_colour() {
    let mut e = two_layers();
    // Blue is only on the locked layer 0 as well; use a colour only layer 1 has.
    e.color = rgba(9, 9, 9, 255);
    e.layer = 1;
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(9, 9), (9, 9)],
    })
    .expect("stroke");
    let from = e
        .doc
        .colors
        .id_for_existing(rgba(9, 9, 9, 255))
        .expect("id");
    e.apply(PaletteCmd::ReplaceColor {
        from,
        to: 4,
        layer: None,
    })
    .expect("only layer 1 shows it");
    assert_eq!(looks(&e, 1, 0)[9 * 16 + 9], GREEN);
}

#[test]
fn replace_is_refused_when_the_group_is_locked_and_when_a_link_is() {
    let mut e = two_layers();
    e.doc.layers[0].locked = false;
    let layer = e.layer_id(0).expect("layer");
    e.apply(LayerCmd::Group { layer }).expect("grouped");
    let group = e
        .doc
        .layers
        .iter()
        .position(|l| l.kind == crate::LayerKind::Group);
    e.doc.layers[group.expect("a group")].locked = true;
    let replace = PaletteCmd::ReplaceColor {
        from: 3,
        to: 4,
        layer: None,
    };
    assert_eq!(e.apply(replace), Err(LimitError::Locked));

    let mut e = linked_layers();
    e.doc.layers[1].locked = true;
    let own = Some(e.layer_id(0).expect("layer"));
    let replace = PaletteCmd::ReplaceColor {
        from: 3,
        to: 4,
        layer: own,
    };
    assert_eq!(e.apply(replace), Err(LimitError::SharedLocked));
}

/// Layers 0 and 1 show one drawing of red, linked.
fn linked_layers() -> Editor {
    let mut e = editor();
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Palette,
    })
    .expect("layer");
    e.apply(CelCmd::Drag(CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 1,
        to_frame: 0,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    }))
    .expect("linked");
    e
}

#[test]
fn replacing_on_a_layer_reaches_its_links_but_not_a_copy_that_shares_storage() {
    let mut e = linked_layers();
    e.apply(LayerCmd::Duplicate {
        layer: e.layer_id(1).expect("layer"),
    })
    .expect("copy");
    let own = e.layer_id(0);
    e.apply(PaletteCmd::ReplaceColor {
        from: 3,
        to: 4,
        layer: own,
    })
    .expect("replaced");
    assert_eq!(looks(&e, 0, 0)[0], GREEN);
    assert_eq!(looks(&e, 1, 0)[0], GREEN, "a link is one drawing");
    let copies = e.doc.layers.len() - 1;
    assert_eq!(looks(&e, copies, 0)[0], RED, "the duplicate is its own");
}

#[test]
fn replace_colour_changes_pixels_and_never_a_colour() {
    let mut e = editor();
    let (colors, version) = (e.doc.colors.colors().to_vec(), e.doc.colors.version());
    let depth = e.history.undo_depth();
    e.apply(PaletteCmd::ReplaceColor {
        from: 3,
        to: 4,
        layer: None,
    })
    .expect("replaced");
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert_eq!(ids_at(&e, 0, 0), 4);
    assert_eq!(looks(&e, 0, 0)[0], GREEN, "the pixels changed");
    assert_eq!(
        (e.doc.colors.colors(), e.doc.colors.version()),
        (&colors[..], version)
    );
    assert!(e.undo());
    assert_eq!(ids_at(&e, 0, 0), 3);
}

#[test]
fn replace_colour_by_the_rule_of_bad_ids_and_nothing_to_do() {
    let mut e = editor();
    let go = |e: &mut Editor, from, to| {
        e.apply(PaletteCmd::ReplaceColor {
            from,
            to,
            layer: None,
        })
    };
    assert_eq!(go(&mut e, 0, 1), Err(LimitError::NoSuchItem));
    assert_eq!(go(&mut e, 99, 1), Err(LimitError::NoSuchItem));
    assert_eq!(go(&mut e, 1, 99), Err(LimitError::NoSuchItem));
    let depth = e.history.undo_depth();
    assert!(!go(&mut e, 3, 3).expect("ok").changed);
    // Grey has no pixel: nothing shows it, so there is no step.
    assert!(!go(&mut e, 1, 4).expect("ok").changed);
    assert_eq!(e.history.undo_depth(), depth);
    // To 0 clears the pixels.
    go(&mut e, 3, 0).expect("cleared");
    assert_eq!(ids_at(&e, 0, 0), 0);
}

#[test]
fn replace_colour_on_a_layer_that_is_not_a_palette_layer_is_refused() {
    let mut e = editor();
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("layer");
    let layer = e.layer_id(1);
    let out = e.apply(PaletteCmd::ReplaceColor {
        from: 3,
        to: 4,
        layer,
    });
    assert_eq!(out, Err(LimitError::NoSuchItem));
}

#[test]
fn a_remap_carries_floating_pixels_with_it() {
    let mut e = editor();
    e.apply(PaletteCmd::MoveSwatch { from: 0, to: 3 })
        .expect("moved");
    let shape = Box::new(crate::mask::Mask::rect(0, 0, 4, 1));
    let op = crate::selection::SelectOp::Replace;
    e.apply(SelectionCmd::Select { shape, op })
        .expect("selected");
    e.apply(FloatingCmd::Lift { copy: false }).expect("lifted");
    e.apply(FloatingCmd::Move {
        dx: 8,
        dy: 8,
        join: false,
    })
    .expect("moved");
    e.apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder))
        .expect("remapped");
    assert!(e.floating.is_none());
    let cel = e.doc.layers[0].cel(0);
    assert_eq!(e.doc.colors.color(cel.get(8, 8)), RED);
    assert_eq!(cel.get(8, 8), 2);
    assert_eq!(cel.get(0, 0), 0, "lifted away");
}

#[test]
fn session_brushes_follow_a_remap_and_a_brush_stays_one_brush() {
    let mut e = editor();
    e.apply(PaletteCmd::MoveSwatch { from: 0, to: 3 })
        .expect("moved");
    let ids = [3, 4, 0, 2];
    let colors: Vec<u32> = ids.iter().map(|&id| e.doc.colors.color(id)).collect();
    let brush =
        Brush::from_parts("b", 2, 2, &colors, Some(&ids), BrushPaint::OwnColors).expect("ok");
    e.brushes.set_active(BrushChoice::Custom(Arc::new(brush)));
    e.apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder))
        .expect("remapped");
    let active = e.brushes().active_custom().expect("brush");
    // Blue, red, green, grey became 1, 2, 3, 4.
    assert_eq!(active.ids(), Some(&[2, 3, 0, 1][..]));
    assert!(Arc::ptr_eq(active, &e.brushes().recent()[0]));
    assert!(e.undo());
    let active = e.brushes().active_custom().expect("brush");
    assert_eq!(
        active.ids(),
        Some(&[2, 3, 0, 1][..]),
        "view state: undo leaves it"
    );
}

#[test]
fn a_remap_asks_for_memory_first_and_a_refusal_changes_nothing() {
    let mut e = editor();
    e.apply(PaletteCmd::MoveSwatch { from: 0, to: 3 })
        .expect("moved");
    let before = (e.doc.colors.colors().to_vec(), looks(&e, 0, 0));
    let depth = e.history.undo_depth();
    e.set_memory_check(|_| false);
    let out = e.apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder));
    assert_eq!(out, Err(LimitError::NoMemory));
    let out = e.apply(PaletteCmd::ReplaceColor {
        from: 3,
        to: 4,
        layer: None,
    });
    assert_eq!(out, Err(LimitError::NoMemory));
    assert_eq!(e.history.undo_depth(), depth);
    assert_eq!((e.doc.colors.colors().to_vec(), looks(&e, 0, 0)), before);
}

#[test]
fn the_memory_estimate_covers_every_distinct_palette_drawing_once() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("copy");
    let one = e.doc.layers[0].cel(0).heap_size();
    static ASKED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    e.set_memory_check(|bytes| {
        ASKED.store(bytes, std::sync::atomic::Ordering::SeqCst);
        true
    });
    e.apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder))
        .expect("ok");
    let bytes = ASKED.load(std::sync::atomic::Ordering::SeqCst);
    assert!(bytes >= one, "the drawing is counted: {bytes} < {one}");
    assert!(
        bytes < 2 * one + (1 << 16),
        "a shared drawing is counted once"
    );
}

#[test]
fn a_hold_and_a_copied_frame_stay_shared_after_a_remap() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("copy");
    let layer = e.layer_id(0).expect("layer");
    e.apply(FrameCmd::Add { at: 2, copy: false })
        .expect("frame");
    e.apply(FrameCmd::HoldPrevious { layer, frame: 2 })
        .expect("hold");
    e.apply(PaletteCmd::MoveSwatch { from: 0, to: 3 })
        .expect("moved");
    e.apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder))
        .expect("remapped");
    let cels = e.doc.layers[0].cels();
    assert!(cels[0].same_image(&cels[1]), "the copy still shares");
    assert!(cels[2].link() != 0 && cels[1].link() == cels[2].link());
    assert!(cels[1].same_image(&cels[2]), "the hold still shares");
    assert_eq!(ids_at(&e, 0, 0), 2);
}

#[test]
fn storage_tiles_two_drawings_share_are_rebuilt_once() {
    let mut e = Editor::new(Document::new(48, 16, ColorMode::Palette).expect("valid"));
    for color in [GREY, BLUE, RED] {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    e.color = RED;
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(0, 0), (3, 0), (40, 0), (41, 1)],
    })
    .expect("stroke");
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("copy");
    // A stroke on the copy splits it off, but it keeps the tiles it did not touch.
    (e.layer, e.frame) = (0, 1);
    e.color = rgba(5, 5, 5, 255);
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(1, 5), (2, 5)],
    })
    .expect("stroke");
    e.apply(PaletteCmd::MoveSwatch { from: 0, to: 2 })
        .expect("moved");
    let shared = |e: &Editor| {
        let cels = e.doc.layers[0].cels();
        let first: Vec<_> = cels[0].image().drawn_tiles().map(Arc::as_ptr).collect();
        cels[1]
            .image()
            .drawn_tiles()
            .filter(|t| first.contains(&Arc::as_ptr(t)))
            .count()
    };
    let before = shared(&e);
    assert!(before > 0);
    e.apply(PaletteCmd::Remap(RemapKind::ToSwatchOrder))
        .expect("remapped");
    assert_eq!(shared(&e), before);
}

/// A swatch made clear is still a swatch: merging keeps it and its pixels, so giving it a
/// colour again brings them back.
#[test]
fn merging_keeps_a_clear_swatch_and_its_pixels_linked() {
    let mut e = editor();
    // Red (3) made clear; its row still holds id 3.
    e.apply(PaletteCmd::Recolor { id: 3, color: 0 })
        .expect("recoloured");
    e.apply(crate::command::StepCmd::EndDrag).expect("end");
    e.apply(PaletteCmd::Remap(RemapKind::MergeDuplicates))
        .expect("merged");
    assert_eq!(ids_at(&e, 0, 0), 3, "the row keeps its id");
    assert!(e.doc.colors.swatches().contains(&3), "the swatch stays");
    e.apply(PaletteCmd::Recolor { id: 3, color: RED })
        .expect("recoloured");
    assert_eq!(
        e.doc.colors.color(ids_at(&e, 0, 0)),
        RED,
        "the row shows again"
    );
}
