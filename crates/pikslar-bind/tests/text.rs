//! `draw_text` and `text_width`: built-in 5x7 text for scripts, priced before any pixel is made.

mod common;

use common::*;
use pikslar_api::ApiError;
use pikslar_bind::{functions, Capabilities, Capability, Failure, ScriptLimits};
use pikslar_core::{ColorMode, Document, Editor};
use pikslar_script::ErrorKind;

const RED: &str = "4278190335";

fn big() -> Editor {
    Editor::new(Document::new(512, 512, ColorMode::Rgba).expect("a canvas"))
}

/// The fuel given, and memory for the largest drawing's paste (14 bytes a pixel), so a test of
/// the price in fuel is not stopped by the price in memory.
fn limited(fuel: u64) -> ScriptLimits {
    ScriptLimits {
        fuel,
        memory: 64 << 20,
        ..ScriptLimits::default()
    }
}

#[test]
fn a_script_draws_text_and_measures_it() {
    let mut e = project();
    let out = ok(
        &mut e,
        &format!(
            "let l = layers()[0]; let f = frames()[0];\n\
             draw_text(l, f, 1, 2, \"A\", {RED}, 1);\n\
             return [get_pixel(l, f, 2, 3) == {RED}, get_pixel(l, f, 1, 3), text_width(\"Hi\", 1)];"
        ),
    );
    assert_eq!(out, "[true, 0, 11]");
}

#[test]
fn text_width_is_the_widest_line_at_the_scale() {
    let mut e = project();
    assert_eq!(ok(&mut e, "return text_width(\"\", 3);"), "0");
    assert_eq!(ok(&mut e, "return text_width(\"AB\", 2);"), "22");
    assert_eq!(ok(&mut e, "return text_width(\"a\\nbcd\", 2);"), "34");
}

#[test]
fn a_width_matches_the_pixels_drawn() {
    let mut e = project();
    let out = ok(
        &mut e,
        &format!(
            "let l = layers()[0]; let f = frames()[0];\n\
             let w = text_width(\"Hi\", 1);\n\
             draw_text(l, f, 0, 0, \"Hi\", {RED}, 1);\n\
             let last = 0;\n\
             for x in 0..16 {{ for y in 0..12 {{ if get_pixel(l, f, x, y) != 0 {{ last = x; }} }} }}\n\
             return [w, last];"
        ),
    );
    assert_eq!(out, "[11, 9]", "the i ends one column short of its cell");
}

#[test]
fn each_function_needs_its_own_capability() {
    let all = functions();
    let need = |name: &str| {
        all.iter()
            .find(|f| f.name == name)
            .map(|f| (f.arity, f.needs))
    };
    assert_eq!(need("draw_text"), Some((7, Some(Capability::EditDocument))));
    assert_eq!(
        need("text_width"),
        Some((2, Some(Capability::ReadDocument)))
    );
    let mut e = project();
    let src = "return text_width(\"A\", 1);";
    let edit_only = Capabilities::NONE.with(Capability::EditDocument);
    let out = run_with(&mut e, src, edit_only, ScriptLimits::default());
    assert!(out.result.is_err(), "no ReadDocument, no text_width");
}

#[test]
fn the_fuel_is_charged_before_any_pixel_is_made() {
    let mut e = big();
    let before = state(&e);
    let src = format!(
        "draw_text(layers()[0], frames()[0], 0, 0, \"{}\", {RED}, 8);",
        "W".repeat(200)
    );
    let out = run_with(&mut e, &src, Capabilities::ALL, limited(5_000));
    match out.result {
        Err(Failure::Script(error)) => assert_eq!(error.kind, ErrorKind::Fuel),
        other => panic!("expected the fuel to run out, got {other:?}"),
    }
    assert_eq!(state(&e).bytes, before.bytes, "nothing was drawn");
    assert_eq!(state(&e).undo, before.undo);
}

#[test]
fn the_price_follows_the_length_and_the_scale_and_is_the_drawing_made() {
    let used = |text: &str, scale: u32| {
        let mut e = big();
        let src = format!("draw_text(layers()[0], frames()[0], 0, 0, \"{text}\", {RED}, {scale});");
        let out = run(&mut e, &src);
        assert!(out.result.is_ok(), "{:?}", out.result);
        out.fuel_used
    };
    let (short, long, big_scale) = (used("A", 1), used(&"A".repeat(20), 1), used("A", 8));
    assert!(
        long > short + 20 * 40,
        "each character is paid for: {short} {long}"
    );
    assert!(
        big_scale > short + 40 * 40,
        "so is each pixel it is scaled to: {short} {big_scale}"
    );
    // 250 letters at scale 5: a drawing 7,495 by 45, wider than the canvas but all of it made.
    let drawing = 7_495 * 45;
    let most = used(&"W".repeat(250), 5);
    assert!(
        (drawing..drawing + 2_000).contains(&most),
        "the drawing's pixels and the command: {most}"
    );
}

#[test]
fn a_refused_draw_changes_nothing() {
    let mut e = project();
    let before = but_ids(&e);
    let cases = [
        ("draw_text(l, f, 99, 0, \"A\", C, 1);", "outside"),
        ("draw_text(l, f, 0, 0, \"A\", C, 9);", "scale"),
        ("draw_text(l, f, 0, 0, \"A\", 0, 1);", "color"),
        ("draw_text(l, f, 0, 0, \"AAAA\", C, 0);", "scale"),
    ];
    for (call, what) in cases {
        let src = format!(
            "let l = layers()[0]; let f = frames()[0]; {}",
            call.replace('C', RED)
        );
        match run(&mut e, &src).result {
            Err(Failure::Refused { error, .. }) => {
                assert!(
                    matches!(
                        error,
                        ApiError::Outside { .. } | ApiError::OutOfRange { .. }
                    ),
                    "{what}"
                );
            }
            other => panic!("{what}: expected a refusal, got {other:?}"),
        }
        assert_eq!(but_ids(&e).bytes, before.bytes, "{what}");
    }
    let long = format!(
        "let l = layers()[0]; let f = frames()[0]; draw_text(l, f, 0, 0, \"{}\", {RED}, 1);",
        "x".repeat(257)
    );
    assert!(matches!(
        run(&mut e, &long).result,
        Err(Failure::Refused { .. })
    ));
    assert_eq!(but_ids(&e).bytes, before.bytes);
    let wrong = "draw_text(layers()[0], frames()[0], 0, 0, 5, 1, 1);";
    assert!(run(&mut e, wrong).result.is_err());
    assert_eq!(but_ids(&e).bytes, before.bytes);
}

/// 40 letters over 40 line breaks at scale 4: 80 characters, but a drawing of 1.6 million pixels.
fn sparse_tall() -> String {
    format!("{}{}", "W".repeat(40), "\\n".repeat(40))
}

#[test]
fn the_price_is_the_drawing_made_not_the_letters_in_it() {
    let src =
        |text: &str| format!("draw_text(layers()[0], frames()[0], 0, 0, \"{text}\", {RED}, 4);");
    let mut e = big();
    let before = state(&e);
    let out = run_with(
        &mut e,
        &src(&sparse_tall()),
        Capabilities::ALL,
        limited(500_000),
    );
    match out.result {
        Err(Failure::Script(error)) => assert_eq!(error.kind, ErrorKind::Fuel),
        other => panic!("expected the fuel to run out, got {other:?}"),
    }
    assert_eq!(state(&e).bytes, before.bytes, "nothing was drawn");
    let out = run_with(
        &mut e,
        &src(&sparse_tall()),
        Capabilities::ALL,
        limited(5_000_000),
    );
    assert!(out.result.is_ok(), "{:?}", out.result);
    assert!(out.fuel_used > 1_500_000, "{}", out.fuel_used);
}

#[test]
fn a_drawing_too_large_is_refused_after_it_is_paid_for_and_changes_nothing() {
    let mut e = big();
    let before = but_ids(&e);
    let text = format!("{}{}", "W".repeat(144), "\\n".repeat(101));
    let src = format!("draw_text(layers()[0], frames()[0], 0, 0, \"{text}\", {RED}, 8);");
    match run(&mut e, &src).result {
        Err(Failure::Refused { error, .. }) => assert!(
            matches!(
                error,
                ApiError::OutOfRange {
                    what: "the area of the text",
                    ..
                }
            ),
            "{error:?}"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(but_ids(&e).bytes, before.bytes);
}

/// The paste behind a text draw holds the clip, the cells made from it and a mask at once, not
/// only the clip: that peak is what is charged (security review L1).
#[test]
fn a_text_draw_is_charged_for_the_peak_of_its_paste_not_only_its_pixels() {
    let one_text = |n: usize, memory: usize| {
        let mut e = big();
        let src = format!(
            "draw_text(layers()[0], frames()[0], 0, 0, \"{}\", {RED}, 8);",
            "A".repeat(n)
        );
        let limits = ScriptLimits {
            memory,
            ..ScriptLimits::default()
        };
        run_with(&mut e, &src, Capabilities::ALL, limits).result
    };
    // 100 letters at scale 8 are 268,800 pixels: 1.1 MB as a clip alone, over 3.7 MB in the paste.
    let limit = 2 << 20;
    match one_text(100, limit) {
        Err(f) => assert_eq!(f.kind(), Some(ErrorKind::Memory), "{f}"),
        Ok(v) => panic!("the paste's peak was not charged: {v}"),
    }
    assert!(
        one_text(40, limit).is_ok(),
        "a smaller text fits the same limit"
    );
}
