//! Contract: the bulk pixel operations write, erase and read whole blocks, each through the
//! editor's own commands, and leave the same document as the one-pixel operations would.

mod common;

use common::*;
use pikslar_api::{run_ops, ApiError, Op, Reply, API_VERSION};
use pikslar_core::{Editor, LayerCmd, LimitError};

const GREEN: u32 = 0xff20_c020;

fn cel(e: &Editor) -> (i64, i64) {
    (layer_ids(e)[0], frames(e)[0])
}

/// Every pixel of the bottom layer's first cel, row by row.
fn pixels(e: &mut Editor) -> Vec<u32> {
    let (layer, frame) = cel(e);
    let read = Op::Pixels {
        layer,
        frame,
        x: 0,
        y: 0,
        width: 16,
        height: 12,
    };
    match op(e, read) {
        Reply::Pixels(colors) => colors,
        other => panic!("expected pixels, got {other:?}"),
    }
}

#[test]
fn put_pixels_writes_colors_and_leaves_zeros_alone() {
    let mut e = project();
    let (layer, frame) = cel(&e);
    let colors = vec![i64::from(BLUE), 0, i64::from(GREEN), 0, 0, i64::from(BLUE)];
    let put = Op::PutPixels {
        layer,
        frame,
        x: 1,
        y: 3,
        width: 3,
        colors,
    };
    assert_eq!(op(&mut e, put), Reply::Done);
    let got = pixels(&mut e);
    assert_eq!(got[3 * 16 + 1], BLUE);
    assert_eq!(got[3 * 16 + 2], RED, "the dot under a 0 is kept");
    assert_eq!(got[3 * 16 + 3], GREEN);
    assert_eq!(got[4 * 16 + 3], BLUE);
    assert_eq!(got.iter().filter(|&&c| c != 0).count(), 4);
    undo(&mut e);
    assert_eq!(pixels(&mut e)[3 * 16 + 1], 0, "one undo step");
}

#[test]
fn put_pixels_saves_as_the_same_pixels_set_one_by_one() {
    let (mut by_block, mut by_pixel) = (project(), project());
    let (layer, frame) = cel(&by_block);
    let pf = cel(&by_pixel).1;
    let color = |x: i64, y: i64| match (x + y) % 3 {
        0 => 0,
        1 => i64::from(BLUE),
        _ => i64::from(0x80ff_00ff_u32),
    };
    let mut colors = Vec::new();
    let mut singles = Vec::new();
    for y in 2..9 {
        for x in 4..14 {
            colors.push(color(x, y));
            if color(x, y) != 0 {
                let c = color(x, y);
                singles.push(Op::SetPixel {
                    layer,
                    frame: pf,
                    x,
                    y,
                    color: c,
                });
            }
        }
    }
    let put = Op::PutPixels {
        layer,
        frame,
        x: 4,
        y: 2,
        width: 10,
        colors,
    };
    ops(&mut by_block, &[put]);
    ops(&mut by_pixel, &singles);
    assert_eq!(save(&by_block), save(&by_pixel));
}

#[test]
fn fill_rect_paints_or_erases_a_block() {
    let mut e = project();
    let (layer, frame) = cel(&e);
    let rect = |color: u32| Op::FillRect {
        layer,
        frame,
        x: 1,
        y: 2,
        width: 4,
        height: 3,
        color: i64::from(color),
    };
    op(&mut e, rect(GREEN));
    let got = pixels(&mut e);
    for y in 0..12 {
        for x in 0..16 {
            let inside = (1..5).contains(&x) && (2..5).contains(&y);
            assert_eq!(got[y * 16 + x] == GREEN, inside, "({x}, {y})");
        }
    }
    op(&mut e, rect(0));
    let got = pixels(&mut e);
    assert!(
        got.iter().all(|&c| c == 0),
        "the dot at (2, 3) was inside too"
    );
}

#[test]
fn fill_rect_saves_as_the_same_pixels_set_and_erased_one_by_one() {
    let (mut by_block, mut by_pixel) = (project(), project());
    let (layer, frame) = cel(&by_block);
    let pf = cel(&by_pixel).1;
    let mut singles = Vec::new();
    for y in 1..5 {
        for x in 0..16 {
            singles.push(Op::SetPixel {
                layer,
                frame: pf,
                x,
                y,
                color: i64::from(GREEN),
            });
        }
    }
    for y in 3..4 {
        for x in 2..9 {
            singles.push(Op::ErasePixel {
                layer,
                frame: pf,
                x,
                y,
            });
        }
    }
    let blocks = [
        Op::FillRect {
            layer,
            frame,
            x: 0,
            y: 1,
            width: 16,
            height: 4,
            color: i64::from(GREEN),
        },
        Op::FillRect {
            layer,
            frame,
            x: 2,
            y: 3,
            width: 7,
            height: 1,
            color: 0,
        },
    ];
    ops(&mut by_block, &blocks);
    ops(&mut by_pixel, &singles);
    assert_eq!(save(&by_block), save(&by_pixel));
}

#[test]
fn a_fill_after_a_block_is_not_held_inside_it() {
    let mut e = project();
    let (layer, frame) = cel(&e);
    let list = [
        Op::FillRect {
            layer,
            frame,
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            color: i64::from(GREEN),
        },
        Op::Fill {
            layer,
            frame,
            x: 15,
            y: 11,
            color: i64::from(BLUE),
        },
    ];
    ops(&mut e, &list);
    let got = pixels(&mut e);
    assert_eq!(got[11 * 16 + 15], BLUE, "no selection was left behind");
    assert_eq!(got.iter().filter(|&&c| c == BLUE).count(), 16 * 12 - 4 - 1);
    assert!(e.selection().is_none());
}

#[test]
fn pixels_reads_what_pixel_reads() {
    let mut e = project();
    let (layer, frame) = cel(&e);
    let block = match op(
        &mut e,
        Op::Pixels {
            layer,
            frame,
            x: 1,
            y: 2,
            width: 3,
            height: 2,
        },
    ) {
        Reply::Pixels(colors) => colors,
        other => panic!("expected pixels, got {other:?}"),
    };
    assert_eq!(block, vec![0, 0, 0, 0, RED, 0]);
}

fn refused(e: &mut Editor, bad: Op) -> ApiError {
    let before = save(e);
    let error = run_ops(e, API_VERSION, &[bad]).expect_err("refused");
    assert_eq!(save(e), before, "nothing changed");
    error
}

#[test]
fn blocks_outside_the_canvas_or_malformed_are_refused() {
    let mut e = project();
    let (layer, frame) = cel(&e);
    let put = |x, y, width, colors: Vec<i64>| Op::PutPixels {
        layer,
        frame,
        x,
        y,
        width,
        colors,
    };
    let fill = |x, y, width, height| Op::FillRect {
        layer,
        frame,
        x,
        y,
        width,
        height,
        color: 1 << 24,
    };
    let read = |x, y, width, height| Op::Pixels {
        layer,
        frame,
        x,
        y,
        width,
        height,
    };
    let c = i64::from(BLUE);
    for bad in [
        put(15, 0, 2, vec![c, c]),
        put(0, 11, 1, vec![c, c]),
        put(-1, 0, 1, vec![c]),
        put(0, 0, 0, vec![]),
        put(0, 0, 2, vec![c, c, c]),
        put(0, 0, 1, vec![0x00ff_ffff]),
        put(0, 0, 1, vec![1 << 32]),
        fill(0, 0, 17, 1),
        fill(0, 0, 1, 13),
        fill(0, 0, 0, 1),
        fill(5, 5, i64::MAX, 1),
        read(0, 0, 16, 13),
        read(i64::MIN, 0, 1, 1),
    ] {
        let error = refused(&mut e, bad.clone());
        assert!(
            matches!(
                error,
                ApiError::Outside { .. } | ApiError::OutOfRange { .. } | ApiError::Transparent(_)
            ),
            "{bad:?} gave {error:?}"
        );
    }
    let gone = put(0, 0, 1, vec![c]);
    let Op::PutPixels { colors, .. } = gone else {
        unreachable!()
    };
    let stale = Op::PutPixels {
        layer: 999,
        frame,
        x: 0,
        y: 0,
        width: 1,
        colors,
    };
    assert_eq!(refused(&mut e, stale), ApiError::NoSuchLayer(999));
}

#[test]
fn a_locked_layer_refuses_blocks_as_it_refuses_pixels() {
    let mut e = project();
    let (layer, frame) = cel(&e);
    let id = e.layer_id(0).expect("a layer");
    e.apply(LayerCmd::SetLocked {
        layer: id,
        locked: true,
    })
    .expect("locks");
    let set = Op::SetPixel {
        layer,
        frame,
        x: 0,
        y: 0,
        color: i64::from(BLUE),
    };
    let put = Op::PutPixels {
        layer,
        frame,
        x: 0,
        y: 0,
        width: 1,
        colors: vec![i64::from(BLUE)],
    };
    let fill = Op::FillRect {
        layer,
        frame,
        x: 0,
        y: 0,
        width: 2,
        height: 2,
        color: 0,
    };
    let expected = refused(&mut e, set);
    assert_eq!(expected, ApiError::Refused(LimitError::Locked));
    assert_eq!(refused(&mut e, put), expected);
    assert_eq!(refused(&mut e, fill), expected);
}

#[test]
fn on_a_palette_layer_a_block_saves_as_its_pixels_set_one_by_one() {
    let (mut by_block, mut by_pixel) = (project(), project());
    let (layer, frame) = (layer_ids(&by_block)[1], frames(&by_block)[1]);
    let pf = frames(&by_pixel)[1];
    let new = i64::from(0xff40_8020_u32);
    let colors = vec![
        i64::from(BLUE),
        new,
        0,
        new,
        i64::from(GREEN),
        i64::from(BLUE),
    ];
    let singles: Vec<Op> = colors
        .iter()
        .enumerate()
        .filter(|(_, &c)| c != 0)
        .map(|(i, &c)| Op::SetPixel {
            layer,
            frame: pf,
            x: 5 + i as i64 % 3,
            y: 6 + i as i64 / 3,
            color: c,
        })
        .collect();
    let put = Op::PutPixels {
        layer,
        frame,
        x: 5,
        y: 6,
        width: 3,
        colors,
    };
    ops(&mut by_block, &[put]);
    ops(&mut by_pixel, &singles);
    assert_eq!(save(&by_block), save(&by_pixel));
}

/// A block larger than one paste is written in strips, which save as one paste would: as the
/// same block written row by row.
#[test]
fn a_large_block_saves_as_the_same_block_row_by_row() {
    let large = || {
        let doc = pikslar_core::Document::new(400, 300, pikslar_core::ColorMode::Rgba);
        Editor::new(doc.expect("a canvas"))
    };
    let (mut whole, mut rows) = (large(), large());
    let fill = |e: &Editor, y: i64, height: i64| Op::FillRect {
        layer: layer_ids(e)[0],
        frame: frames(e)[0],
        x: 3,
        y,
        width: 397,
        height,
        color: i64::from(GREEN),
    };
    let all = fill(&whole, 0, 300);
    ops(&mut whole, &[all]);
    let each: Vec<Op> = (0..300).map(|y| fill(&rows, y, 1)).collect();
    ops(&mut rows, &each);
    assert_eq!(save(&whole), save(&rows));
    assert_eq!(whole.history().undo_depth(), 1);
}
