//! Tests for the brush model, in both colour modes.

use super::*;
use crate::brush::{brush_origin, in_brush};
use crate::color::{rgba, ColorTable};
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};
use crate::mask::Mask;
use crate::orient::Orientation;
use crate::selection::SelectOp;
use crate::Turn;
use std::collections::BTreeMap;
use std::sync::Arc;

const RED: u32 = rgba(255, 0, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const GLASS: u32 = rgba(10, 20, 30, 77);
const MODES: [ColorMode; 2] = [ColorMode::Palette, ColorMode::Rgba];

fn editor(mode: ColorMode) -> Editor {
    let mut e = Editor::new(Document::new(32, 32, mode).expect("valid"));
    e.pixel_perfect = false;
    e
}

fn dot(e: &mut Editor, x: i32, y: i32, color: u32) {
    e.color = color;
    e.begin(Tool::Pencil, x, y).expect("begin");
    e.commit();
}

/// An L at (4, 4): red, blue to its right, translucent below; the fourth cell clear.
fn ell(mode: ColorMode) -> Editor {
    let mut e = editor(mode);
    dot(&mut e, 4, 4, RED);
    dot(&mut e, 5, 4, BLUE);
    dot(&mut e, 4, 5, GLASS);
    e.select(Mask::rect(4, 4, 2, 2), SelectOp::Replace);
    e
}

/// Every cell a stamp writes, by position relative to the stroke point.
fn cells(stamp: &Stamp) -> BTreeMap<(i32, i32), u32> {
    let mut out = BTreeMap::new();
    for run in stamp.runs() {
        for (i, &v) in run.values.iter().enumerate() {
            assert!(out.insert((run.dx + i as i32, run.dy), v).is_none());
        }
    }
    out
}

/// The values in the order the runs write them.
fn in_order(stamp: &Stamp) -> Vec<u32> {
    stamp.runs().flat_map(|r| r.values.to_vec()).collect()
}

fn table() -> Arc<ColorTable> {
    Arc::new(ColorTable::new())
}

struct Random(u64);

impl Random {
    fn next(&mut self, below: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) % below
    }
}

fn custom(e: &Editor) -> Arc<Brush> {
    e.brushes().active_custom().expect("custom brush").clone()
}

#[test]
fn a_brush_is_captured_from_the_selection_in_both_modes() {
    for mode in MODES {
        let mut e = ell(mode);
        e.capture_brush(BrushPaint::OwnColors).expect("capture");
        let b = custom(&e);
        assert_eq!(b.source_size(), (2, 2), "{mode:?}");
        assert_eq!(b.colors(), [RED, BLUE, GLASS, 0], "{mode:?}");
        assert_eq!(b.ids().is_some(), mode == ColorMode::Palette);
        assert_eq!(b.orientation(), Orientation::IDENTITY);
        assert_eq!(b.paint(), BrushPaint::OwnColors);
    }
}

#[test]
fn pixels_outside_an_irregular_selection_are_clear() {
    let mut e = ell(ColorMode::Rgba);
    e.select(Mask::rect(4, 4, 1, 2), SelectOp::Replace);
    e.capture_brush(BrushPaint::Shape).expect("capture");
    assert_eq!(custom(&e).colors(), [RED, GLASS]);
}

#[test]
fn floating_pixels_are_what_gets_captured() {
    for mode in MODES {
        let mut e = ell(mode);
        e.lift(false);
        e.flip_horizontal();
        e.capture_brush(BrushPaint::OwnColors).expect("capture");
        assert_eq!(custom(&e).colors(), [BLUE, RED, 0, GLASS], "{mode:?}");
    }
}

#[test]
fn nothing_to_capture_is_refused_and_changes_nothing() {
    let mut e = editor(ColorMode::Rgba);
    assert_eq!(
        e.capture_brush(BrushPaint::Shape),
        Err(LimitError::NothingToCapture)
    );
    e.select(Mask::rect(10, 10, 3, 3), SelectOp::Replace);
    assert_eq!(
        e.capture_brush(BrushPaint::Shape),
        Err(LimitError::NothingToCapture)
    );
    assert_eq!(*e.brushes(), BrushState::default());
}

#[test]
fn brushes_past_256_by_256_are_refused() {
    let mut e = Editor::new(Document::new(300, 300, ColorMode::Rgba).expect("valid"));
    dot(&mut e, 1, 1, RED);
    e.select(Mask::rect(0, 0, 257, 10), SelectOp::Replace);
    assert_eq!(
        e.capture_brush(BrushPaint::Shape),
        Err(LimitError::BrushTooLarge)
    );
    e.select(Mask::rect(0, 0, 256, 256), SelectOp::Replace);
    assert!(e.capture_brush(BrushPaint::Shape).is_ok());
}

#[test]
fn capturing_and_turning_are_not_undo_steps() {
    let mut e = ell(ColorMode::Palette);
    let (depth, revision) = (e.history.undo_depth(), e.revision());
    e.capture_brush(BrushPaint::OwnColors).expect("capture");
    assert!(e.turn_brush(Turn::Clockwise));
    assert!(e.set_brush_paint(BrushPaint::Shape));
    assert!(e.set_brush_handle(Handle::Corner(3)));
    e.set_brush(BrushChoice::Preset(Preset::Diamond));
    assert_eq!(e.history.undo_depth(), depth);
    assert_eq!(e.revision(), revision);
}

#[test]
fn only_a_custom_brush_can_be_turned() {
    let mut e = editor(ColorMode::Rgba);
    assert!(!e.turn_brush(Turn::Clockwise));
    assert!(!e.set_brush_paint(BrushPaint::Shape));
    assert!(!e.set_brush_handle(Handle::Centre));
}

#[test]
fn four_turns_give_back_the_source() {
    let all = [
        Turn::FlipHorizontal,
        Turn::FlipVertical,
        Turn::Clockwise,
        Turn::CounterClockwise,
    ];
    let mut rng = Random(7);
    let colors: Vec<u32> = (0..15)
        .map(|_| rgba(rng.next(256) as u8, 1, 2, 255))
        .collect();
    let source = Brush::from_parts("b", 5, 3, &colors, None, BrushPaint::OwnColors).expect("ok");
    for turn in all {
        let mut b = source.clone();
        let once = source.turned_by(turn);
        for _ in 0..4 {
            b = b.turned_by(turn);
        }
        assert_eq!(b, source, "{turn:?}");
        assert!(Arc::ptr_eq(&b.colors, &source.colors), "nothing is copied");
        let stamp = |b: &Brush| b.prepare(ColorMode::Rgba, &table(), 0).expect("ok").stamp;
        assert_eq!(stamp(&b), stamp(&source));
        assert_eq!(
            once.size(),
            if turn == Turn::Clockwise || turn == Turn::CounterClockwise {
                (3, 5)
            } else {
                (5, 3)
            }
        );
    }
    // Flipping twice and quarter-turning back are the identity too.
    let back = source
        .turned_by(Turn::Clockwise)
        .turned_by(Turn::CounterClockwise);
    assert_eq!(back, source);
    let twice = source
        .turned_by(Turn::FlipHorizontal)
        .turned_by(Turn::FlipHorizontal);
    assert_eq!(twice, source);
}

#[test]
fn a_clockwise_turn_puts_the_top_left_pixel_at_the_top_right() {
    let source =
        Brush::from_parts("b", 2, 1, &[RED, BLUE], None, BrushPaint::OwnColors).expect("ok");
    let turned = source.turned_by(Turn::Clockwise);
    let s = turned
        .prepare(ColorMode::Rgba, &table(), 0)
        .expect("ok")
        .stamp;
    let got = cells(&s);
    // 1 wide, 2 tall: red on top, blue below. Centre is (0, 0) of a 1 x 2 grid.
    assert_eq!(got, BTreeMap::from([((0, 0), RED), ((0, 1), BLUE)]));
}

#[test]
fn every_orientation_stamps_what_a_pixel_by_pixel_model_says() {
    let mut rng = Random(99);
    let (w, h) = (4usize, 3usize);
    let colors: Vec<u32> = (0..w * h)
        .map(|_| {
            if rng.next(3) == 0 {
                0
            } else {
                rgba(rng.next(250) as u8 + 1, 5, 6, 255)
            }
        })
        .collect();
    let source = Brush::from_parts("b", w, h, &colors, None, BrushPaint::OwnColors).expect("ok");
    for o in Orientation::ALL {
        let b = source.turned(o);
        let s = b.prepare(ColorMode::Rgba, &table(), 0).expect("ok").stamp;
        let (ow, oh) = o.turned_size(w, h);
        let (left, top) = (-((ow as i32 - 1) / 2), -((oh as i32 - 1) / 2));
        let mut want = BTreeMap::new();
        for y in 0..h {
            for x in 0..w {
                let c = colors[y * w + x];
                if c != 0 {
                    let (tx, ty) = o.apply(x, y, w, h);
                    want.insert((left + tx as i32, top + ty as i32), c);
                }
            }
        }
        assert_eq!(cells(&s), want, "orientation {}", o.index());
        assert_eq!(s.size(), (ow as i32, oh as i32));
        assert_eq!(s.offset(), (left, top));
    }
}

#[test]
fn the_handle_is_the_centre_or_a_corner_that_follows_its_pixel() {
    let colors = [1u32, 2, 3, 4, 5, 6];
    let b = Brush::from_parts("b", 3, 2, &colors, None, BrushPaint::OwnColors).expect("ok");
    let at = |b: &Brush, v: u32| -> (i32, i32) {
        let s = b.prepare(ColorMode::Rgba, &table(), 0).expect("ok").stamp;
        *cells(&s).iter().find(|(_, &c)| c == v).expect("present").0
    };
    // Centre by brush_origin's rule: a 3 x 2 brush has its origin at (-1, 0).
    assert_eq!(brush_origin(0, 0, 3), (-1, -1));
    assert_eq!(at(&b, 1), (-1, 0));
    // Corners: the chosen source pixel lands on the stroke point.
    for (n, pixel) in [(0u8, 1u32), (1, 3), (2, 4), (3, 6)] {
        for o in Orientation::ALL {
            let turned = b.with_handle(Handle::Corner(n)).turned(o);
            assert_eq!(
                at(&turned, pixel),
                (0, 0),
                "corner {n} orientation {}",
                o.index()
            );
        }
    }
}

#[test]
fn presets_and_round_have_the_expected_shapes() {
    for size in 1..=12 {
        let round = cells(
            &BrushChoice::Round
                .prepare(size, ColorMode::Rgba, &table(), 9)
                .expect("ok")
                .stamp,
        );
        let (left, top) = brush_origin(0, 0, size);
        let want: BTreeMap<_, _> = (0..size)
            .flat_map(|y| (0..size).map(move |x| (x, y)))
            .filter(|&(x, y)| in_brush(size, x, y))
            .map(|(x, y)| ((left + x, top + y), 9))
            .collect();
        assert_eq!(round, want, "round {size}");
        let n = size as usize;
        assert_eq!(Preset::Square.stamp(size, 9).cell_count(), n * n);
        assert_eq!(Preset::HorizontalLine.stamp(size, 9).size(), (size, 1));
        assert_eq!(Preset::VerticalLine.stamp(size, 9).size(), (1, size));
        let diamond = cells(&Preset::Diamond.stamp(size, 9));
        assert!(diamond.contains_key(&(0, 0)));
        for &(x, y) in diamond.keys() {
            assert!(
                diamond.contains_key(&(x, -y + 2 * top + size - 1)),
                "symmetric {size}"
            );
        }
    }
    assert_eq!(cells(&Preset::Diamond.stamp(3, 1)).len(), 5);
    assert_eq!(Preset::Square.stamp(1000, 1).size(), (256, 256));
    assert_eq!(Preset::Square.stamp(0, 1).size(), (1, 1));
}

#[test]
fn a_shape_brush_paints_the_fill_including_the_eraser_zero() {
    for mode in MODES {
        let mut e = ell(mode);
        e.capture_brush(BrushPaint::Shape).expect("capture");
        let b = custom(&e);
        for fill in [7u32, 0] {
            let s = b.prepare(mode, &e.doc.colors, fill).expect("ok");
            assert!(s.colors.is_none(), "a shape adds no colours");
            let got = cells(&s.stamp);
            assert_eq!(got.len(), 3);
            assert!(got.values().all(|&v| v == fill));
        }
    }
}

#[test]
fn own_colours_resolve_per_layer_mode() {
    // A palette brush painting on an RGBA layer paints colours.
    let mut p = ell(ColorMode::Palette);
    p.capture_brush(BrushPaint::OwnColors).expect("capture");
    let on_rgba = custom(&p)
        .prepare(ColorMode::Rgba, &table(), 0)
        .expect("ok");
    assert!(on_rgba.colors.is_none());
    assert_eq!(in_order(&on_rgba.stamp), [RED, BLUE, GLASS]);
    // An RGBA brush on a palette layer adds its colours to a copy of the table.
    let mut r = ell(ColorMode::Rgba);
    r.capture_brush(BrushPaint::OwnColors).expect("capture");
    let own = editor(ColorMode::Palette).doc.colors.clone();
    let on_palette = custom(&r).prepare(ColorMode::Palette, &own, 0).expect("ok");
    let grown = on_palette.colors.expect("grown");
    assert_eq!(own.len(), 1, "the document's table is untouched");
    let ids = in_order(&on_palette.stamp);
    let shown: Vec<u32> = ids.iter().map(|&id| grown.color(id)).collect();
    assert_eq!(shown, [RED, BLUE, GLASS]);
}

#[test]
fn a_recoloured_id_is_never_painted_with_the_wrong_colour() {
    let mut e = ell(ColorMode::Palette);
    e.capture_brush(BrushPaint::OwnColors).expect("capture");
    let red_id = custom(&e).ids().expect("ids")[0];
    Arc::make_mut(&mut e.doc.colors).set_color(red_id, rgba(1, 2, 3, 255));
    let p = custom(&e)
        .prepare(ColorMode::Palette, &e.doc.colors, 0)
        .expect("ok");
    let table = p.colors.as_ref().unwrap_or(&e.doc.colors);
    let shown: Vec<u32> = in_order(&p.stamp)
        .iter()
        .map(|&id| table.color(id))
        .collect();
    assert_eq!(shown, [RED, BLUE, GLASS]);
    assert_ne!(in_order(&p.stamp)[0], red_id);
}

#[test]
fn a_brush_too_colourful_for_the_table_is_refused_and_leaves_it_alone() {
    let colors: Vec<u32> = (1..=65_536u32)
        .map(|i| rgba((i >> 8) as u8, i as u8, 0, 255))
        .collect();
    let b = Brush::from_parts("big", 256, 256, &colors, None, BrushPaint::OwnColors).expect("ok");
    let t = table();
    assert!(b.prepare(ColorMode::Palette, &t, 0).is_err());
    assert_eq!(t.len(), 1);
    assert!(b.prepare(ColorMode::Rgba, &t, 0).is_ok());
}

/// A source layer with a random picture over a few colours, in a table of its own order.
fn random_source(rng: &mut Random, mode: ColorMode) -> Editor {
    let palette = [
        RED,
        BLUE,
        GLASS,
        rgba(1, 2, 3, 255),
        rgba(9, 9, 9, 128),
        rgba(200, 100, 5, 255),
    ];
    let mut e = editor(mode);
    for _ in 0..40 {
        let (x, y) = (rng.next(6) as i32, rng.next(5) as i32);
        dot(&mut e, x, y, palette[rng.next(6) as usize]);
    }
    e.select(Mask::rect(0, 0, 6, 5), SelectOp::Replace);
    e
}

/// A target whose table holds some of the same colours under other ids.
fn random_target(rng: &mut Random, mode: ColorMode) -> Editor {
    let palette = [BLUE, rgba(7, 7, 7, 255), RED, rgba(200, 100, 5, 255), GLASS];
    let mut e = editor(mode);
    for _ in 0..rng.next(6) {
        dot(
            &mut e,
            20 + rng.next(4) as i32,
            20,
            palette[rng.next(5) as usize],
        );
    }
    e
}

#[test]
fn the_indexed_rule_equals_pastes_on_random_clips() {
    let mut rng = Random(0x5eed);
    for round in 0..200 {
        let source_mode = MODES[rng.next(2) as usize];
        let source = random_source(&mut rng, source_mode);
        let Some(clip) = source.copy_selected() else {
            continue;
        };
        let Ok(brush) = Brush::from_clip("c", &clip, BrushPaint::OwnColors) else {
            continue;
        };
        for mode in MODES {
            let mut target = random_target(&mut rng, mode);
            let before = target.doc.colors.clone();
            let prepared = brush.prepare(mode, &before, 0).expect("fits");
            target.paste(&clip, 0, 0).expect("fits");
            target.anchor();
            let (w, h) = brush.source_size();
            let (left, top) = prepared.stamp.offset();
            let got = cells(&prepared.stamp);
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let painted = target.cel().get(x, y);
                    let want = got.get(&(left + x, top + y)).copied().unwrap_or(0);
                    assert_eq!(painted, want, "round {round} {mode:?} at ({x}, {y})");
                }
            }
            let after = prepared.colors.unwrap_or(before);
            assert_eq!(*after, *target.doc.colors, "round {round} {mode:?} table");
        }
    }
}

#[test]
fn runs_are_row_ordered_and_the_mask_covers_exactly_the_cells() {
    let b = Brush::from_parts(
        "b",
        4,
        2,
        &[1, 1, 0, 1, 0, 1, 1, 1],
        None,
        BrushPaint::OwnColors,
    )
    .expect("ok");
    let s = b.prepare(ColorMode::Rgba, &table(), 0).expect("ok").stamp;
    let runs: Vec<(i32, i32, usize)> = s.runs().map(|r| (r.dx, r.dy, r.values.len())).collect();
    assert_eq!(runs, [(-1, 0, 2), (2, 0, 1), (0, 1, 3)]);
    let mask = s.mask();
    assert_eq!(mask.count(), s.cell_count());
    for &(x, y) in cells(&s).keys() {
        assert!(mask.contains(x, y));
    }
    assert!(s.heap_bytes() > 0);
}

#[test]
fn recent_brushes_keep_the_last_eight_without_repeats() {
    let mut state = BrushState::default();
    let make = |i: u32| {
        Arc::new(Brush::from_parts("b", 1, 1, &[i + 1], None, BrushPaint::Shape).expect("ok"))
    };
    let all: Vec<_> = (0..10).map(make).collect();
    for b in &all {
        state.set_active(BrushChoice::Custom(b.clone()));
    }
    assert_eq!(state.recent().len(), MAX_RECENT_BRUSHES);
    assert!(Arc::ptr_eq(&state.recent()[0], &all[9]));
    state.set_active(BrushChoice::Custom(all[5].clone()));
    assert!(Arc::ptr_eq(&state.recent()[0], &all[5]));
    assert_eq!(state.recent().len(), MAX_RECENT_BRUSHES);
    state.set_active(BrushChoice::Round);
    assert_eq!(state.active(), &BrushChoice::Round);
    assert_eq!(state.recent().len(), MAX_RECENT_BRUSHES);
}

#[test]
fn bad_parts_are_refused() {
    let ok = |w, h, c: &[u32], ids: Option<&[u32]>| {
        Brush::from_parts("b", w, h, c, ids, BrushPaint::Shape)
    };
    assert_eq!(ok(0, 1, &[], None), Err(LimitError::BrushTooLarge));
    assert_eq!(ok(257, 1, &[1; 257], None), Err(LimitError::BrushTooLarge));
    assert_eq!(ok(2, 2, &[1, 1, 1], None), Err(LimitError::Dimensions));
    assert_eq!(ok(1, 2, &[1, 1], Some(&[1])), Err(LimitError::Dimensions));
    assert_eq!(ok(1, 2, &[0, 0], None), Err(LimitError::NothingToCapture));
}

#[test]
fn naming_and_editing_a_copy_leave_the_original() {
    let b = Brush::from_parts("one", 1, 1, &[5], None, BrushPaint::Shape).expect("ok");
    let c = b
        .with_name("two")
        .with_paint(BrushPaint::OwnColors)
        .with_handle(Handle::Corner(1));
    assert_eq!(
        (b.name(), b.paint(), b.handle()),
        ("one", BrushPaint::Shape, Handle::Centre)
    );
    assert_eq!(
        (c.name(), c.paint(), c.handle()),
        ("two", BrushPaint::OwnColors, Handle::Corner(1))
    );
    assert_eq!(b.pixel_bytes(), 4);
}

#[test]
fn the_footprint_covers_the_cells_the_stamp_writes_in_either_mode() {
    for mode in MODES {
        let mut e = ell(mode);
        e.capture_brush(BrushPaint::OwnColors).expect("captured");
        e.turn_brush(Turn::Clockwise);
        let footprint = e.brush_footprint(Tool::Pencil).expect("a stamp brush");
        let real = custom(&e)
            .prepare(mode, &table(), RED)
            .expect("stamp")
            .stamp;
        let keys = |s: &Stamp| cells(s).into_keys().collect::<Vec<_>>();
        assert_eq!(keys(&footprint), keys(&real), "{mode:?}");
        assert_eq!(footprint.offset(), real.offset());
        assert_eq!(footprint.cell_count(), 3, "the clear corner is not covered");
    }
}

#[test]
fn the_round_brush_and_a_size_one_preset_have_no_footprint_stamp() {
    let mut e = editor(ColorMode::Rgba);
    assert!(e.brush_footprint(Tool::Pencil).is_none());
    e.set_brush(BrushChoice::Preset(Preset::Square));
    e.brush_size = 1;
    assert!(e.brush_footprint(Tool::Pencil).is_none());
    e.brush_size = 3;
    assert_eq!(
        e.brush_footprint(Tool::Pencil).expect("stamp").cell_count(),
        9
    );
    assert!(e.brush_footprint(Tool::Fill).is_none());
}

#[test]
fn brushes_move_to_the_next_editor() {
    let mut e = ell(ColorMode::Rgba);
    e.capture_brush(BrushPaint::Shape).expect("captured");
    let mut next = editor(ColorMode::Rgba);
    next.use_brushes(e.brushes().clone());
    assert_eq!(next.brushes(), e.brushes());
}

#[test]
fn a_huge_selection_is_refused_by_its_bounds_and_the_size_is_known_without_pixels() {
    let mut e = Editor::new(Document::new(4000, 4000, ColorMode::Rgba).expect("valid"));
    assert_eq!(e.selection_size(), None);
    e.select(Mask::rect(0, 0, 4000, 4000), SelectOp::Replace);
    assert_eq!(e.selection_size(), Some((4000, 4000)));
    assert_eq!(
        e.capture_brush(BrushPaint::OwnColors),
        Err(LimitError::BrushTooLarge)
    );
    assert_eq!(*e.brushes(), BrushState::default());
}

#[test]
fn recent_brushes_are_unique_by_content_and_follow_a_turn() {
    let make =
        |i: u32| Brush::from_parts("b", 2, 1, &[i + 1, 0], None, BrushPaint::Shape).expect("ok");
    let mut state = BrushState::default();
    state.set_active(BrushChoice::Custom(Arc::new(make(1))));
    // The same brush picked again, as a new Arc (a library pick): still one entry.
    state.set_active(BrushChoice::Custom(Arc::new(make(1))));
    assert_eq!(state.recent().len(), 1);
    // A turn replaces the recent slot with the turned brush instead of keeping the old one.
    let turned = state
        .active_custom()
        .expect("custom")
        .turned_by(Turn::Clockwise);
    state.replace_active(turned.clone());
    assert_eq!(state.recent().len(), 1);
    assert_eq!(*state.recent()[0], turned);
    // Turning back onto a brush that is already recent leaves one copy of it.
    state.set_active(BrushChoice::Custom(Arc::new(make(2))));
    state.set_active(BrushChoice::Custom(Arc::new(turned.clone())));
    let back = turned.turned_by(Turn::CounterClockwise);
    state.replace_active(back);
    let all: Vec<_> = state.recent().iter().map(|b| (**b).clone()).collect();
    assert!(all.iter().enumerate().all(|(i, b)| !all[..i].contains(b)));
}
