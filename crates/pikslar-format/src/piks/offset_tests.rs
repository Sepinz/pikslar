//! Version 3 files keep every offset, every pixel past the canvas and every hold exactly, and the
//! version 2 writer refuses what it cannot hold rather than lose it.

use super::v2_writer::encode_v2;
use super::*;
use pikslar_core::{ColorTable, FrameInfo, Grid, Image, Layer, OFFSET_LIMIT};
use std::sync::Arc;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }

    fn between(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u64) as i32
    }
}

/// A drawing of `w` by `h` with a few colors, placed so part of it may lie past the canvas.
fn random_cel(rng: &mut Rng, w: usize, h: usize, palette: bool) -> Cel {
    let (gw, gh) = (rng.between(1, 40) as usize, rng.between(1, 40) as usize);
    let cells: Vec<u32> = (0..gw * gh)
        .map(|_| match rng.below(3) {
            0 => 0,
            _ if palette => 1 + rng.below(3) as u32,
            _ => 0xff00_0000 | rng.below(0xff_ffff) as u32,
        })
        .collect();
    let image = Grid::from_cells(gw, gh, &cells).into_image();
    let reach = (w.max(h) as i32) * 2;
    let at = match rng.below(4) {
        0 => (0, 0),
        1 => (rng.between(-4, 4) * 16, rng.between(-4, 4) * 16),
        _ => (rng.between(-reach, reach), rng.between(-reach, reach)),
    };
    Cel::at(image, at.0, at.1).expect("in range")
}

/// A random document: layers in both modes, holds, and now and then a held frame sat apart
/// from its hold, as a file may place one.
fn random_doc(seed: u64) -> Document {
    let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
    let (w, h) = (rng.between(1, 64) as usize, rng.between(1, 64) as usize);
    let frames = rng.between(1, 4) as usize;
    let layers = (0..rng.between(1, 3))
        .map(|_| {
            let palette = rng.below(2) == 0;
            let mode = if palette {
                ColorMode::Palette
            } else {
                ColorMode::Rgba
            };
            let cels = (0..frames)
                .map(|_| random_cel(&mut rng, w, h, palette))
                .collect();
            let mut holds = vec![0; frames];
            if frames > 1 && rng.below(2) == 0 {
                (holds[0], holds[1]) = (1, 1);
            }
            Layer::new("layer".into(), mode)
                .with_cels(cels)
                .with_holds(&holds)
                .expect("one per frame")
        })
        .collect();
    let colors = Arc::new(ColorTable::from_parts(
        &[0, 0xff11_2233, 0xff44_5566, 0x8077_8899],
        &[],
    ));
    let durations = vec![FrameInfo::new(100); frames];
    let mut doc = Document::from_parts(w, h, colors, layers, durations).expect("consistent");
    if frames > 1 && rng.below(3) == 0 {
        let held = &doc.layers()[0];
        if held.link(1) != 0 {
            let apart = held
                .cel(1)
                .moved_to(rng.between(-30, 30), 7)
                .expect("in range");
            doc.put_cel(0, 1, apart).expect("frame");
        }
    }
    doc
}

/// Where a frame's cel sits, its table and pixels, and the first frame it is held with.
type Seen = ((i16, i16), Vec<u32>, Option<usize>);

/// What each frame shows and from where, with holds as which frames share a number.
fn shape(doc: &Document) -> Vec<Vec<Seen>> {
    doc.layers()
        .iter()
        .map(|layer| {
            let links: Vec<u32> = layer.links().collect();
            (0..layer.frame_count())
                .map(|f| {
                    let cel = layer.cel(f);
                    let (x0, y0, tw, th) = cel.image().table();
                    let mut pixels = vec![x0 as u32, y0 as u32];
                    for ly in y0 * 16..(y0 + th as i32) * 16 {
                        for lx in x0 * 16..(x0 + tw as i32) * 16 {
                            pixels.push(cel.image().get(lx, ly));
                        }
                    }
                    let held_with = (links[f] != 0)
                        .then(|| links.iter().position(|&l| l == links[f]).expect("itself"));
                    (cel.position(), pixels, held_with)
                })
                .collect()
        })
        .collect()
}

#[test]
fn random_documents_with_offsets_round_trip_exactly() {
    for seed in 1..=500u64 {
        let doc = random_doc(seed);
        let again = load(&save(&doc)).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(shape(&again), shape(&doc), "seed {seed}");
        for (a, b) in doc.layers().iter().zip(again.layers()) {
            for f in 0..a.frame_count() {
                for g in 0..a.frame_count() {
                    assert_eq!(
                        a.cel(f).same_image(a.cel(g)),
                        b.cel(f).same_image(b.cel(g)),
                        "seed {seed}: which frames share a drawing"
                    );
                }
            }
        }
        assert_eq!(
            save(&again),
            save(&doc),
            "seed {seed}: saved again, byte for byte"
        );
    }
}

/// An image holding one pixel in its first tile and one in its last, along one axis, as far
/// apart as an image can reach.
fn spanning(across: bool) -> Image {
    let reach = pikslar_core::MAX_TILE_REACH;
    let mut first = [0u32; pikslar_core::tile::TILE_AREA];
    let mut last = first;
    first[0] = 0xff00_00ff;
    last[pikslar_core::tile::TILE_AREA - 1] = 0xff00_ff00;
    let pack = |raw: &[u32; pikslar_core::tile::TILE_AREA]| {
        pikslar_core::tile::Tile::pack(raw).map(Arc::new)
    };
    let mut cells = vec![None; 2 * reach as usize];
    cells[0] = pack(&first);
    cells[2 * reach as usize - 1] = pack(&last);
    let start = -(reach as i16);
    let image = if across {
        Image::from_tiles(start, 0, 2 * reach as u16, cells)
    } else {
        Image::from_tiles(0, start, 1, cells)
    };
    image.expect("within reach")
}

#[test]
fn drawings_at_both_ends_of_the_range_save_and_reopen() {
    let limit = OFFSET_LIMIT;
    for at in [
        (limit, limit),
        (-limit, limit),
        (limit, -limit),
        (-limit, -limit),
    ] {
        let mut doc = Document::new(8192, 8192, ColorMode::Rgba).expect("valid");
        doc.add_frame(1, false).expect("room");
        for (frame, across) in [(0, true), (1, false)] {
            let cel = Cel::at(spanning(across), at.0, at.1).expect("in range");
            doc.put_cel(0, frame, cel).expect("frame");
        }
        let cel = doc.layers()[0].cel(0);
        assert_eq!(cel.get(-32_768 + at.0, at.1), 0xff00_00ff);
        assert_eq!(cel.get(32_767 + at.0, 15 + at.1), 0xff00_ff00);
        let again = load(&save(&doc)).expect("reopens");
        assert_eq!(shape(&again), shape(&doc), "at {at:?}");
    }
}

#[test]
fn version_2_refuses_what_it_cannot_hold() {
    let image = || Grid::from_cells(2, 2, &[1, 1, 1, 1]).into_image();
    let moved = Cel::at(image(), 3, -1).expect("in range");
    let beyond = Cel::new(Grid::from_cells(40, 1, &[1; 40]));
    for cel in [moved, beyond] {
        let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("valid");
        doc.put_cel(0, 0, cel).expect("frame");
        assert!(matches!(
            encode_v2(&doc, &[]),
            Err(FormatError::Unsupported(_))
        ));
    }
    let mut held = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    held.put_cel(0, 0, Cel::at(image(), 0, 0).expect("origin"))
        .expect("frame");
    held.add_frame(1, true).expect("room");
    held.set_holds(0, &[1, 1]).expect("hold");
    let apart = held.layers()[0].cel(1).moved_to(4, 0).expect("in range");
    held.put_cel(0, 1, apart).expect("frame");
    assert!(matches!(
        encode_v2(&held, &[]),
        Err(FormatError::Unsupported(_))
    ));
    held.put_cel(0, 1, Cel::at(image(), 0, 0).expect("origin"))
        .expect("frame");
    assert!(
        encode_v2(&held, &[]).is_ok(),
        "the same drawing at the origin is fine"
    );
    let mut across = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    across.add_layer(1, ColorMode::Rgba).expect("room");
    across
        .put_cel(0, 0, Cel::at(image(), 0, 0).expect("origin"))
        .expect("frame");
    let link = pikslar_core::CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 1,
        to_frame: 0,
        kind: pikslar_core::DragKind::Link,
        landing: pikslar_core::Landing::Overwrite,
    };
    across.drag_cels(&link).expect("linked");
    assert!(matches!(
        encode_v2(&across, &[]),
        Err(FormatError::Unsupported(_))
    ));
}

#[test]
fn a_recovery_copy_of_pixels_lifted_from_a_moved_cel_comes_back_the_same() {
    use pikslar_core::{CelCmd, Editor, FloatingCmd, Mask, SelectOp, SelectionCmd, StepCmd};
    let cells: Vec<u32> = (0..30 * 20)
        .map(|i| 0xff00_0000 | (i as u32 * 77))
        .collect();
    let mut doc = Document::new(24, 16, ColorMode::Rgba).expect("valid");
    doc.put_cel(0, 0, Cel::new(Grid::from_cells(30, 20, &cells)))
        .expect("frame");
    let mut e = Editor::new(doc);
    let layer = e.layer_id(0).expect("layer");
    e.apply(CelCmd::Move {
        layer,
        frame: 0,
        dx: -9,
        dy: 5,
    })
    .expect("move");
    e.apply(StepCmd::EndDrag).expect("end");
    let picked = Box::new(Mask::rect(2, 3, 8, 6));
    e.apply(SelectionCmd::Select {
        shape: picked,
        op: SelectOp::Replace,
    })
    .expect("select");
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    e.apply(FloatingCmd::Move {
        dx: 7,
        dy: -2,
        join: false,
    })
    .expect("float");
    let snapshot = e.floating_snapshot().expect("floating");
    let bytes = save_recovery(e.doc(), Some(&snapshot));
    let back = load_recovery(&bytes).expect("valid");
    assert!(!back.floating_damaged);
    let mut restored = Editor::new(back.doc);
    restored
        .restore_floating(back.floating.expect("floating"))
        .expect("the lifted pixels still match the moved cel");
    for editor in [&mut e, &mut restored] {
        editor.apply(FloatingCmd::Anchor).expect("anchor");
    }
    assert_eq!(shape(e.doc()), shape(restored.doc()));
    assert_eq!(e.doc().layers()[0].cel(0).position(), (-9, 5));
}

#[test]
fn a_tweened_hold_saves_its_offsets_and_its_drawing_once() {
    use pikslar_core::{CelCmd, Ease, Editor};
    let cells: Vec<u32> = (0..40 * 30).map(|i| 0xff00_0000 | (i * 911)).collect();
    let mut doc = Document::new(32, 24, ColorMode::Rgba).expect("valid");
    doc.put_cel(0, 0, Cel::new(Grid::from_cells(40, 30, &cells)))
        .expect("frame");
    for at in 1..5 {
        doc.add_frame(at, true).expect("room");
    }
    doc.set_holds(0, &[1; 5]).expect("hold");
    let held = save(&doc);
    let last = doc.layers()[0].cel(4).moved_to(-7, 9).expect("in range");
    doc.put_cel(0, 4, last).expect("frame");
    let mut e = Editor::new(doc);
    let layer = e.layer_id(0).expect("layer");
    let command = CelCmd::Tween {
        layer,
        from: 0,
        to: 4,
        ease: Ease::InOut,
    };
    assert!(e.apply(command).expect("tween").changed);
    let tweened = e.doc().clone();
    let places: Vec<(i16, i16)> = tweened.layers()[0]
        .cels()
        .iter()
        .map(|c| c.position())
        .collect();
    assert_eq!(places, [(0, 0), (-1, 1), (-4, 5), (-6, 8), (-7, 9)]);

    let bytes = save(&tweened);
    assert_eq!(
        bytes.len(),
        held.len(),
        "the same records as the hold before it moved: one image, five cels"
    );
    let again = load(&bytes).expect("reopens");
    assert_eq!(shape(&again), shape(&tweened));
    let layer = &again.layers()[0];
    assert!(
        (0..5).all(|f| layer.cel(f).same_image(layer.cel(0))),
        "one drawing in memory"
    );
    assert!(
        layer.links().all(|l| l != 0 && l == layer.link(0)),
        "still one hold"
    );
    assert_eq!(save(&again), bytes, "saved again, byte for byte");
}
