//! Contract: malformed input is refused at the boundary with a clear error, and leaves the
//! document as it was.

mod common;

use common::*;
use pikslar_api::{run_ops, ApiError, Mode, Op, API_VERSION};

fn range(what: &'static str, value: i64, min: i64, max: i64) -> ApiError {
    ApiError::OutOfRange {
        what,
        value,
        min,
        max,
    }
}

/// The error for a bad name: a long one is quoted as its first 64 characters and an ellipsis.
fn bad_name(name: &str, why: &'static str) -> ApiError {
    let mut quoted: String = name.chars().take(64).collect();
    if name.chars().count() > 64 {
        quoted.push('\u{2026}');
    }
    ApiError::BadName { name: quoted, why }
}

/// Every malformed operation, with the error it must give, on the project's layers.
fn cases(bottom: i64, swatch: i64, f: &[i64]) -> Vec<(Op, ApiError)> {
    let gone = f[1] + 1_000_000;
    let pixel = |x, y| Op::SetPixel {
        layer: bottom,
        frame: f[0],
        x,
        y,
        color: i64::from(RED),
    };
    let colored = |color| Op::SetPixel {
        layer: bottom,
        frame: f[0],
        x: 0,
        y: 0,
        color,
    };
    let renamed = |name: &str| Op::RenameLayer {
        layer: bottom,
        name: name.into(),
    };
    let long = "x".repeat(65);
    let tag = "t".repeat(33);
    vec![
        (Op::Layer { layer: 999 }, ApiError::NoSuchLayer(999)),
        (Op::Layer { layer: -1 }, ApiError::NoSuchLayer(-1)),
        (Op::Layer { layer: 0 }, ApiError::NoSuchLayer(0)),
        (
            Op::RemoveLayer { layer: i64::MAX },
            ApiError::NoSuchLayer(i64::MAX),
        ),
        (
            Op::AddLayer {
                above: Some(77),
                mode: Mode::Rgba,
                name: None,
            },
            ApiError::NoSuchLayer(77),
        ),
        (Op::RemoveFrame { frame: gone }, ApiError::NoSuchFrame(gone)),
        (Op::RemoveFrame { frame: -1 }, ApiError::NoSuchFrame(-1)),
        (Op::RemoveFrame { frame: 0 }, ApiError::NoSuchFrame(0)),
        (
            Op::Pixel {
                layer: bottom,
                frame: gone,
                x: 0,
                y: 0,
            },
            ApiError::NoSuchFrame(gone),
        ),
        (
            Op::AddFrame { at: 3, copy: false },
            range("a new frame's place", 3, 0, 2),
        ),
        (pixel(16, 0), ApiError::Outside { x: 16, y: 0 }),
        (pixel(0, 12), ApiError::Outside { x: 0, y: 12 }),
        (pixel(-1, 0), ApiError::Outside { x: -1, y: 0 }),
        (
            pixel(i64::MIN, i64::MAX),
            ApiError::Outside {
                x: i64::MIN,
                y: i64::MAX,
            },
        ),
        (
            Op::DrawLine {
                layer: bottom,
                frame: f[0],
                from: (0, 0),
                to: (40, 3),
                color: i64::from(RED),
            },
            ApiError::Outside { x: 40, y: 3 },
        ),
        (colored(-1), range("a color", -1, 0, 0xffff_ffff)),
        (colored(1 << 32), range("a color", 1 << 32, 0, 0xffff_ffff)),
        (colored(0x00ff_ffff), ApiError::Transparent(0x00ff_ffff)),
        (
            Op::SetOpacity {
                layer: bottom,
                opacity: 256,
            },
            range("opacity", 256, 0, 255),
        ),
        (
            Op::SetOpacity {
                layer: bottom,
                opacity: -1,
            },
            range("opacity", -1, 0, 255),
        ),
        (
            Op::SetDuration { frame: f[0], ms: 9 },
            range("a duration in milliseconds", 9, 10, 60_000),
        ),
        (
            Op::SetDuration {
                frame: f[0],
                ms: 60_001,
            },
            range("a duration in milliseconds", 60_001, 10, 60_000),
        ),
        (renamed(""), bad_name("", "is empty")),
        (renamed("   "), bad_name("   ", "is empty")),
        (
            renamed(&long),
            bad_name(&long, "is longer than 64 characters"),
        ),
        (
            renamed("a\nb"),
            bad_name("a\nb", "holds a control character"),
        ),
        (
            Op::AddTag {
                name: tag.clone(),
                from: f[0],
                to: f[1],
            },
            bad_name(&tag, "is longer than 32 characters"),
        ),
        (
            Op::AddTag {
                name: "back".into(),
                from: f[1],
                to: f[0],
            },
            range("a tag's last frame's place", 0, 1, 1),
        ),
        (
            Op::SetBlend {
                layer: bottom,
                blend: "Bogus".into(),
            },
            ApiError::UnknownBlend("Bogus".into()),
        ),
        (
            Op::SetBlend {
                layer: bottom,
                blend: "multiply".into(),
            },
            ApiError::UnknownBlend("multiply".into()),
        ),
        (
            Op::RemoveSwatch { swatch: 999 },
            ApiError::NoSuchSwatch(999),
        ),
        (Op::RemoveSwatch { swatch: 0 }, ApiError::NoSuchSwatch(0)),
        (
            Op::Recolor {
                swatch: swatch + 1,
                color: 0,
            },
            ApiError::NoSuchSwatch(swatch + 1),
        ),
        (
            Op::MoveLayer {
                layer: bottom,
                to: 2,
            },
            range("a layer's place", 2, 0, 1),
        ),
    ]
}

#[test]
fn malformed_operations_are_refused_and_change_nothing() {
    let mut e = project();
    let before = save(&e);
    let depth = e.history().undo_depth();
    let (bottom, swatch) = (layer_ids(&e)[0], i64::from(e.doc().colors.swatches()[0]));
    for (op, error) in cases(bottom, swatch, &frames(&e)) {
        // A good edit first, so a refusal must also take that back.
        let good = Op::SetPixel {
            layer: bottom,
            frame: frames(&e)[1],
            x: 9,
            y: 9,
            color: i64::from(BLUE),
        };
        let result = run_ops(&mut e, API_VERSION, &[good, op.clone()]);
        assert_eq!(result, Err(error), "{op:?}");
        assert_eq!(save(&e), before, "{op:?} left a change");
        assert_eq!(e.history().undo_depth(), depth);
    }
}

#[test]
fn every_error_says_what_was_wrong() {
    let e = project();
    let (bottom, swatch) = (layer_ids(&e)[0], i64::from(e.doc().colors.swatches()[0]));
    for (_, error) in cases(bottom, swatch, &frames(&e)) {
        let text = error.to_string();
        assert!(text.len() > 10, "{error:?} says only {text:?}");
    }
    let text = range("opacity", 256, 0, 255).to_string();
    assert_eq!(text, "opacity must be from 0 to 255, not 256");
}

#[test]
fn script_text_in_errors_is_cut_short() {
    let mut e = project();
    let bottom = layer_ids(&e)[0];
    let long = "n".repeat(1_000_000);
    for op in [
        Op::RenameLayer {
            layer: bottom,
            name: long.clone(),
        },
        Op::SetBlend {
            layer: bottom,
            blend: long.clone(),
        },
        Op::AddLayer {
            above: None,
            mode: Mode::Rgba,
            name: Some(long.clone()),
        },
    ] {
        let error = run_ops(&mut e, API_VERSION, &[op]).expect_err("refused");
        let quoted = match &error {
            ApiError::BadName { name, .. } | ApiError::UnknownBlend(name) => name.clone(),
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(quoted.chars().count(), 65, "{error:?}");
        assert!(quoted.ends_with('\u{2026}'));
        assert!(error.to_string().len() < 200, "{error}");
    }
}

#[test]
fn reading_a_block_is_held_to_a_size_of_its_own() {
    let mut e = pikslar_core::Editor::new(
        pikslar_core::Document::new(8192, 8192, pikslar_core::ColorMode::Rgba).expect("fits"),
    );
    let (layer, frame) = (layer_ids(&e)[0], frames(&e)[0]);
    let read = Op::Pixels {
        layer,
        frame,
        x: 0,
        y: 0,
        width: 8192,
        height: 8192,
    };
    let error = run_ops(&mut e, API_VERSION, &[read]).expect_err("too large to read at once");
    assert!(
        matches!(
            error,
            ApiError::OutOfRange {
                what: "a block's area",
                ..
            }
        ),
        "{error:?}"
    );
}
