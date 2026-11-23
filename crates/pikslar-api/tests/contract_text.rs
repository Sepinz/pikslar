//! Contract: `DrawText` draws built-in 5x7 text on one cel as the text tool does, clipped to the
//! canvas, and a refused one changes nothing.

mod common;

use common::*;
use pikslar_api::{run_ops, ApiError, Op, Reply, API_VERSION};
use pikslar_core::glyphs::{rasterize, Font};
use pikslar_core::{Editor, LayerCmd, LimitError};

fn cel(e: &Editor) -> (i64, i64) {
    (layer_ids(e)[0], frames(e)[0])
}

fn draw(e: &Editor, (x, y): (i64, i64), text: &str, color: u32, scale: i64) -> Op {
    let (layer, frame) = cel(e);
    Op::DrawText {
        layer,
        frame,
        x,
        y,
        text: text.into(),
        color: i64::from(color),
        scale,
    }
}

fn pixel(e: &mut Editor, x: i64, y: i64) -> u32 {
    let (layer, frame) = cel(e);
    match op(e, Op::Pixel { layer, frame, x, y }) {
        Reply::Color(c) => c,
        other => panic!("expected a color, got {other:?}"),
    }
}

fn refused(e: &mut Editor, bad: Op) -> ApiError {
    let before = save(e);
    let error = run_ops(e, API_VERSION, &[bad]).expect_err("refused");
    assert_eq!(save(e), before, "nothing changed");
    error
}

#[test]
fn text_lands_where_the_rasterised_glyphs_are() {
    let mut e = project();
    let text = draw(&e, (1, 2), "A", RED, 1);
    assert_eq!(op(&mut e, text), Reply::Done);
    // "A" is lit from its second row: ".###." then "#...#".
    assert_eq!(pixel(&mut e, 2, 3), RED);
    assert_eq!(pixel(&mut e, 1, 3), 0);
    assert_eq!(pixel(&mut e, 1, 4), RED);
    assert_eq!(pixel(&mut e, 1, 2), 0, "the row above the letters is clear");
}

#[test]
fn text_saves_as_the_same_pixels_set_one_by_one() {
    let (mut drawn, mut set) = (project(), project());
    let (layer, frame) = cel(&set);
    let text = draw(&drawn, (2, 1), "Hi\nB", BLUE, 1);
    op(&mut drawn, text);
    let (clip, _) = rasterize(Font::Pikslar5x7, 1, 1, "Hi\nB", BLUE);
    let mut writes = Vec::new();
    for (i, &c) in clip.colors.iter().enumerate() {
        let (x, y) = (2 + (i % clip.width) as i64, 1 + (i / clip.width) as i64);
        if c != 0 && x < 16 && y < 12 {
            writes.push(Op::SetPixel {
                layer,
                frame,
                x,
                y,
                color: i64::from(c),
            });
        }
    }
    ops(&mut set, &writes);
    assert_eq!(save_but_ids(&drawn), save_but_ids(&set));
}

#[test]
fn text_running_off_the_canvas_is_clipped_not_refused() {
    let mut e = project();
    let text = draw(&e, (14, 6), "WW", RED, 3);
    assert_eq!(op(&mut e, text), Reply::Done);
    assert_ne!(pixel(&mut e, 14, 10), 0, "the part that fits is drawn");
    assert_eq!(pixel(&mut e, 13, 9), 0);
}

#[test]
fn what_the_font_lacks_is_a_question_mark_and_empty_text_does_nothing() {
    let (mut lacking, mut marks) = (project(), project());
    let (a, b) = (
        draw(&lacking, (1, 1), "\u{416}", RED, 1),
        draw(&marks, (1, 1), "?", RED, 1),
    );
    op(&mut lacking, a);
    op(&mut marks, b);
    assert_eq!(save_but_ids(&lacking), save_but_ids(&marks));
    let mut e = project();
    let before = (save(&e), e.history().undo_depth());
    let nothing = draw(&e, (1, 1), "", RED, 1);
    assert_eq!(op(&mut e, nothing), Reply::Done);
    assert_eq!((save(&e), e.history().undo_depth()), before);
}

#[test]
fn a_text_draw_is_one_undo_step() {
    let mut e = project();
    let (before, depth) = (save(&e), e.history().undo_depth());
    let text = draw(&e, (1, 1), "Hello", RED, 1);
    op(&mut e, text);
    assert_eq!(e.history().undo_depth(), depth + 1);
    undo(&mut e);
    assert_eq!(save_but_ids(&e), save_but_ids(&reopen(&before)));
}

#[test]
fn bad_places_scales_lengths_colors_and_layers_are_refused_and_change_nothing() {
    let mut e = project();
    let bad_x = draw(&e, (16, 0), "A", RED, 1);
    assert!(matches!(refused(&mut e, bad_x), ApiError::Outside { .. }));
    let bad_y = draw(&e, (0, -1), "A", RED, 1);
    assert!(matches!(refused(&mut e, bad_y), ApiError::Outside { .. }));
    for scale in [0, 9, -1] {
        let bad = draw(&e, (0, 0), "A", RED, scale);
        assert!(
            matches!(refused(&mut e, bad), ApiError::OutOfRange { .. }),
            "{scale}"
        );
    }
    let long = draw(&e, (0, 0), &"x".repeat(257), RED, 1);
    assert!(matches!(refused(&mut e, long), ApiError::OutOfRange { .. }));
    let wide = draw(&e, (0, 0), &"W".repeat(200), RED, 8);
    assert!(
        matches!(refused(&mut e, wide), ApiError::OutOfRange { .. }),
        "wider than any canvas"
    );
    let clear = draw(&e, (0, 0), "A", 0, 1);
    assert!(matches!(
        refused(&mut e, clear),
        ApiError::OutOfRange { .. }
    ));
    let (layer, frame) = cel(&e);
    let stale = Op::DrawText {
        layer: 999,
        frame,
        x: 0,
        y: 0,
        text: "A".into(),
        color: i64::from(RED),
        scale: 1,
    };
    assert_eq!(refused(&mut e, stale), ApiError::NoSuchLayer(999));
    let _ = layer;
}

#[test]
fn a_locked_layer_refuses_text_as_it_refuses_pixels() {
    let mut e = project();
    let id = e.layer_id(0).expect("a layer");
    e.apply(LayerCmd::SetLocked {
        layer: id,
        locked: true,
    })
    .expect("locks");
    let text = draw(&e, (0, 0), "A", RED, 1);
    assert_eq!(refused(&mut e, text), ApiError::Refused(LimitError::Locked));
}

#[test]
fn text_whose_drawing_would_be_too_large_is_refused() {
    let mut e = project();
    // Few characters, but one long line over many empty ones makes a drawing of 56 million
    // pixels, which is refused before it is made.
    let mut tall = "W".repeat(144);
    tall.push_str(&"\n".repeat(101));
    let bad = draw(&e, (0, 0), &tall, RED, 8);
    let error = refused(&mut e, bad);
    assert!(
        matches!(
            error,
            ApiError::OutOfRange {
                what: "the area of the text",
                ..
            }
        ),
        "{error:?}"
    );
}
