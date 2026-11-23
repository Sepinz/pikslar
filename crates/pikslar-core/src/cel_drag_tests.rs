//! Timeline drags against a model written apart from the code: thousands of random documents
//! and drags, each one step that undoes and redoes exactly.

use crate::cel_drag::{CelDrag, DragKind, Landing};
use crate::color::rgba;
use crate::command::{CelCmd, StepCmd};
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::Editor;
use crate::layer::Cel;
use crate::offset_tests::image_of;
use std::collections::HashMap;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// What the model knows of a cel: which drawing it shows (0 for none) and its link (0 for none),
/// named alike on every layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slot {
    drawing: usize,
    hold: u32,
}

/// The model: per layer, a slot per frame; plus each frame's time.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Model {
    layers: Vec<Vec<Slot>>,
    times: Vec<u32>,
}

/// Reads the document into the model's terms. Drawings are named by what they show, in order of
/// first appearance, and holds by their members, so documents that look alike read the same.
/// A drag that would change nothing to see is no step, as for every command.
fn read(doc: &Document) -> Model {
    let mut names: HashMap<((i16, i16), Vec<u32>), usize> = HashMap::new();
    let mut holds: HashMap<u32, u32> = HashMap::new();
    let layers = doc
        .layers
        .iter()
        .map(|layer| {
            layer
                .cels()
                .iter()
                .map(|cel| {
                    let drawing = if cel.is_blank() {
                        0
                    } else {
                        let next = names.len() + 1;
                        let shows = (cel.position(), cel.to_cells(doc.width, doc.height));
                        *names.entry(shows).or_insert(next)
                    };
                    let hold = if cel.link() == 0 {
                        0
                    } else {
                        let next = holds.len() as u32 + 1;
                        *holds.entry(cel.link()).or_insert(next)
                    };
                    Slot { drawing, hold }
                })
                .collect()
        })
        .collect();
    let times = doc.frames.iter().map(|f| f.duration_ms).collect();
    Model { layers, times }
}

/// The rule, written out again from the description of a drag, on the model.
fn expected(before: &Model, drag: &CelDrag) -> Model {
    let mut m = before.clone();
    // A block dropped where it was, of any kind, is no drag.
    let home = (drag.to_layer, drag.to_frame) == (drag.layers.0, drag.frames.0);
    if home && drag.landing == Landing::Overwrite {
        return m;
    }
    let width = drag.frames.1 - drag.frames.0 + 1;
    let block: Vec<Vec<Slot>> = (drag.layers.0..=drag.layers.1)
        .map(|l| before.layers[l][drag.frames.0..=drag.frames.1].to_vec())
        .collect();
    let at = |f: usize| match drag.landing {
        Landing::Insert if f >= drag.to_frame => f + width,
        _ => f,
    };
    if drag.landing == Landing::Insert {
        for layer in &mut m.layers {
            for _ in 0..width {
                layer.insert(
                    drag.to_frame,
                    Slot {
                        drawing: 0,
                        hold: 0,
                    },
                );
            }
        }
        let times = before.times[drag.frames.0..=drag.frames.1].to_vec();
        m.times.splice(drag.to_frame..drag.to_frame, times);
    }
    let blank = Slot {
        drawing: 0,
        hold: 0,
    };
    if drag.kind == DragKind::Move {
        for l in drag.layers.0..=drag.layers.1 {
            for f in drag.frames.0..=drag.frames.1 {
                m.layers[l][at(f)] = blank;
            }
        }
    }
    let mut fresh = 1_000;
    for (row, slots) in block.iter().enumerate() {
        let (source, target) = (drag.layers.0 + row, drag.to_layer + row);
        for (column, slot) in slots.iter().enumerate() {
            let hold = match drag.kind {
                DragKind::Move if source == target => slot.hold,
                DragKind::Move | DragKind::Copy => 0,
                DragKind::Link if slot.hold != 0 => slot.hold,
                DragKind::Link => {
                    fresh += 1;
                    let from = at(drag.frames.0 + column);
                    let s = &mut m.layers[source][from];
                    if s.drawing == slot.drawing && s.hold == 0 {
                        s.hold = fresh;
                    }
                    fresh
                }
            };
            m.layers[target][drag.to_frame + column] = Slot {
                drawing: slot.drawing,
                hold,
            };
        }
    }
    // A link of one cel, anywhere in the document, is no link.
    let ids: Vec<u32> = m.layers.iter().flatten().map(|s| s.hold).collect();
    for layer in &mut m.layers {
        for slot in layer.iter_mut() {
            if slot.hold != 0 && ids.iter().filter(|&&id| id == slot.hold).count() < 2 {
                slot.hold = 0;
            }
        }
    }
    renamed(&m)
}

/// The model with drawings and holds named by first appearance, as [`read`] names them.
fn renamed(m: &Model) -> Model {
    let mut names: HashMap<usize, usize> = HashMap::new();
    let mut holds: HashMap<u32, u32> = HashMap::new();
    let mut out = m.clone();
    for layer in out.layers.iter_mut() {
        for slot in layer.iter_mut() {
            if slot.drawing != 0 {
                let next = names.len() + 1;
                slot.drawing = *names.entry(slot.drawing).or_insert(next);
            }
            if slot.hold != 0 {
                let next = holds.len() as u32 + 1;
                slot.hold = *holds.entry(slot.hold).or_insert(next);
            }
        }
    }
    out
}

/// A random document: up to 4 layers, some of them palette layers, up to 6 frames, cels drawn
/// on or not, a few holds.
fn document(rng: &mut Rng) -> Editor {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    for at in 1..=rng.below(4) {
        let mode = if rng.below(4) == 0 {
            ColorMode::Palette
        } else {
            ColorMode::Rgba
        };
        doc.add_layer(at, mode).expect("layer");
    }
    for at in 1..=rng.below(6) {
        doc.add_frame(at, false).expect("frame");
        doc.frames[at].duration_ms = 50 + 10 * at as u32;
    }
    let (layers, frames) = (doc.layers.len(), doc.frames.len());
    for layer in 0..layers {
        for frame in 0..frames {
            if rng.below(3) > 0 {
                let dot = HashMap::from([((rng.below(8) as i32, 1), rgba(9, 9, 9, 255))]);
                let cel = Cel::from_image(image_of(&dot));
                doc.layers[layer].put_cel(frame, cel).expect("frame");
            }
        }
        if frames > 1 && rng.below(2) == 0 {
            let (a, b) = (rng.below(frames), rng.below(frames));
            let source = rng.below(frames);
            doc.hold_across(layer, source, a.min(b), a.max(b))
                .expect("hold");
        }
    }
    Editor::new(doc)
}

/// A random drag that may or may not fit the document.
fn drag(rng: &mut Rng, doc: &Document) -> CelDrag {
    let (layers, frames) = (doc.layers.len(), doc.frames.len());
    let (l0, f0) = (rng.below(layers), rng.below(frames));
    let (l1, f1) = (l0 + rng.below(layers - l0), f0 + rng.below(frames - f0));
    let kinds = [DragKind::Move, DragKind::Copy, DragKind::Link];
    CelDrag {
        layers: (l0, l1),
        frames: (f0, f1),
        to_layer: rng.below(layers),
        to_frame: rng.below(frames + 1),
        kind: kinds[rng.below(3)],
        landing: if rng.below(3) == 0 {
            Landing::Insert
        } else {
            Landing::Overwrite
        },
    }
}

#[test]
fn every_drag_matches_the_model_and_is_one_step_that_undoes_and_redoes() {
    let (mut made, mut refused) = (0, 0);
    for seed in 1..=2_500u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut e = document(&mut rng);
        for step in 0..3 {
            let d = drag(&mut rng, &e.doc);
            let what = format!("seed {seed} step {step}: {d:?}");
            let before_doc = e.doc.clone();
            let before = read(&e.doc);
            let depth = e.history().undo_depth();
            match e.apply(CelCmd::Drag(d)) {
                Err(error) => {
                    refused += 1;
                    assert!(
                        matches!(
                            error,
                            LimitError::NoSuchItem
                                | LimitError::CelPlacement
                                | LimitError::LinkModes
                        ),
                        "{what}: {error:?}"
                    );
                    assert_eq!(e.doc, before_doc, "{what}: a refused drag changes nothing");
                    assert_eq!(e.history().undo_depth(), depth, "{what}");
                    continue;
                }
                Ok(outcome) => {
                    made += 1;
                    let after = read(&e.doc);
                    assert_eq!(after, expected(&before, &d), "{what}");
                    every_hold_shows_one_drawing(&e.doc, &what);
                    assert_eq!(outcome.changed, e.doc != before_doc, "{what}");
                    let steps = usize::from(outcome.changed);
                    assert_eq!(e.history().undo_depth(), depth + steps, "{what}");
                    if outcome.changed {
                        assert_eq!((e.layer, e.frame), (d.to_layer, d.to_frame), "{what}");
                        let after_doc = e.doc.clone();
                        e.apply(StepCmd::Undo).expect("undo");
                        assert_eq!(e.doc, before_doc, "{what}: undo");
                        assert_eq!(read(&e.doc), before, "{what}: undo, drawings");
                        e.apply(StepCmd::Redo).expect("redo");
                        assert_eq!(e.doc, after_doc, "{what}: redo");
                    }
                }
            }
        }
    }
    assert!(
        made > 3_000 && refused > 500,
        "{made} made, {refused} refused"
    );
}

fn every_hold_shows_one_drawing(doc: &Document, what: &str) {
    for layer in doc.layers.iter() {
        let cels = layer.cels();
        for (a, cel) in cels.iter().enumerate() {
            for other in cels.iter().skip(a + 1) {
                if cel.link() != 0 && cel.link() == other.link() {
                    assert!(cel.same_image(other), "{what}: a hold shows two drawings");
                }
            }
        }
    }
}

fn three_by_two() -> Editor {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    doc.add_frame(1, false).expect("frame");
    doc.add_frame(2, false).expect("frame");
    for layer in 0..2 {
        for frame in 0..3 {
            let dot = HashMap::from([((layer as i32, frame as i32), rgba(1, 2, 3, 255))]);
            let cel = Cel::from_image(image_of(&dot));
            doc.layers[layer].put_cel(frame, cel).expect("frame");
        }
    }
    Editor::new(doc)
}

fn simple(layers: (usize, usize), frames: (usize, usize), to: (usize, usize)) -> CelDrag {
    CelDrag {
        layers,
        frames,
        to_layer: to.0,
        to_frame: to.1,
        kind: DragKind::Move,
        landing: Landing::Overwrite,
    }
}

#[test]
fn a_move_leaves_empty_cels_and_shares_the_drawing() {
    let mut e = three_by_two();
    let image = e.doc.layers[0].cel(0).image_id();
    e.apply(CelCmd::Drag(simple((0, 0), (0, 0), (0, 2))))
        .expect("drag");
    assert!(e.doc.layers[0].cel(0).is_blank());
    assert_eq!(e.doc.layers[0].cel(2).image_id(), image, "no pixel copied");
    assert_eq!((e.layer, e.frame), (0, 2));
}

#[test]
fn a_link_makes_a_hold_and_an_insert_adds_frames_with_their_times() {
    let mut e = three_by_two();
    e.doc.frames[0].duration_ms = 70;
    let d = CelDrag {
        kind: DragKind::Link,
        landing: Landing::Insert,
        ..simple((0, 1), (0, 0), (0, 3))
    };
    e.apply(CelCmd::Drag(d)).expect("drag");
    assert_eq!(e.doc.frames.len(), 4);
    assert_eq!(e.doc.frames[3].duration_ms, 70);
    for layer in 0..2 {
        let l = &e.doc.layers[layer];
        assert!(l.link(0) != 0 && l.link(0) == l.link(3), "layer {layer}");
    }
}

#[test]
fn links_reach_layers_of_one_mode_and_cels_land_on_their_own_mode() {
    let mut e = three_by_two();
    let d = CelDrag {
        kind: DragKind::Link,
        ..simple((0, 0), (0, 0), (1, 1))
    };
    assert!(e.apply(CelCmd::Drag(d)).is_ok(), "a link across layers");
    let l = |layer: usize, frame: usize| e.doc.layers[layer].link(frame);
    assert!(l(0, 0) != 0 && l(0, 0) == l(1, 1));
    e.doc.add_layer(2, ColorMode::Palette).expect("layer");
    let d = simple((1, 1), (0, 0), (2, 0));
    assert_eq!(e.apply(CelCmd::Drag(d)), Err(LimitError::CelPlacement));
    let d = CelDrag {
        kind: DragKind::Link,
        ..d
    };
    assert_eq!(e.apply(CelCmd::Drag(d)), Err(LimitError::LinkModes));
}

#[test]
fn a_block_given_back_to_front_is_refused() {
    let e = three_by_two();
    let d = simple((0, 0), (2, 1), (0, 0));
    assert_eq!(e.doc.check_cel_drag(&d), Err(LimitError::NoSuchItem));
}

#[test]
fn a_link_over_its_own_block_holds_only_what_still_shows_its_source() {
    let mut e = three_by_two();
    let cels = e.doc.layers[0].cels().to_vec();
    let d = CelDrag {
        kind: DragKind::Link,
        ..simple((0, 0), (0, 1), (0, 1))
    };
    e.apply(CelCmd::Drag(d)).expect("drag");
    let l = &e.doc.layers[0];
    assert!(l.cel(1).same_image(&cels[0]) && l.cel(2).same_image(&cels[1]));
    assert!(
        l.link(0) != 0 && l.link(0) == l.link(1),
        "frame 1 holds frame 0's drawing"
    );
    assert_eq!(
        l.link(2),
        0,
        "frame 1's own drawing is gone, so frame 2 holds nothing"
    );
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.doc.layers[0].links().collect::<Vec<_>>(), [0, 0, 0]);
}

#[test]
fn a_drag_onto_itself_is_no_step() {
    let mut e = three_by_two();
    let outcome = e
        .apply(CelCmd::Drag(simple((0, 1), (0, 2), (0, 0))))
        .expect("drag");
    assert!(!outcome.changed);
    assert_eq!(e.history().undo_depth(), 0);
}
