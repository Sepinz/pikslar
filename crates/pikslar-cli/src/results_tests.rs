//! References to earlier results: what they resolve to when a list runs, and every way one
//! is refused, before anything runs or while the list runs, leaving the document as it was.

use crate::apply::{read_ops, run};
use crate::json::{parse, Value, LIMITS};
use pikslar_api::{run_ops, Op, API_VERSION};
use pikslar_core::{ColorMode, Document, Editor};
use pikslar_format::piks;

fn list(text: &str) -> Vec<Value> {
    match parse(text.as_bytes(), &LIMITS).expect("JSON") {
        Value::Array(items) => items,
        other => panic!("not a list: {other:?}"),
    }
}

/// A 4 by 4 document with one layer (id 1) and two frames.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(4, 4, ColorMode::Rgba).expect("size"));
    e.apply(pikslar_core::FrameCmd::Add { at: 1, copy: false })
        .expect("room");
    e
}

fn handles(e: &Editor) -> Vec<i64> {
    e.doc()
        .frames
        .iter()
        .map(|f| i64::try_from(f.key().get()).expect("small"))
        .collect()
}

const RED: i64 = 4_278_190_335;

#[test]
fn references_name_what_earlier_operations_made_or_listed() {
    let ops = list(&format!(
        r#"[
        {{"op":"document"}},
        {{"op":"add_frame","at":2}},
        {{"op":"add_layer","above":{{"result":0,"place":0}},"name":"Ink"}},
        {{"op":"set_pixel","layer":{{"result":2}},"frame":{{"result":1}},"x":1,"y":1,"color":{RED}}},
        {{"op":"duplicate_layer","layer":{{"result":2}}}},
        {{"op":"set_duration","frame":{{"result":0,"place":1}},"ms":40}},
        {{"op":"add_tag","name":"all","from":{{"result":0,"place":0}},"to":{{"result":1}}}},
        {{"op":"pixel","layer":{{"result":4}},"frame":{{"result":1}},"x":1,"y":1}}
    ]"#
    ));
    let planned = read_ops(&ops).expect("every reference is to an earlier, fitting result");
    let mut e = editor();
    let mut same = Editor::new(piks::load(&piks::save(e.doc())).expect("opens"));
    let ran = run(&mut e, 1, &planned).expect("runs");
    assert_eq!(
        ran.results[7],
        Value::Object(vec![("color".into(), Value::Int(RED))])
    );

    // The same edit through the API with the ids written out gives the same document.
    let h = handles(&same);
    let added = run_ops(
        &mut same,
        API_VERSION,
        &[Op::AddFrame { at: 2, copy: false }],
    )
    .expect("adds");
    let new_frame = match added[0] {
        pikslar_api::Reply::Frame(handle) => i64::try_from(handle).expect("small"),
        ref other => panic!("{other:?}"),
    };
    let rest = [
        Op::AddLayer {
            above: Some(1),
            mode: pikslar_api::Mode::Rgba,
            name: Some("Ink".into()),
        },
        Op::SetPixel {
            layer: 2,
            frame: new_frame,
            x: 1,
            y: 1,
            color: RED,
        },
        Op::DuplicateLayer { layer: 2 },
        Op::SetDuration {
            frame: h[1],
            ms: 40,
        },
        Op::AddTag {
            name: "all".into(),
            from: h[0],
            to: new_frame,
        },
    ];
    run_ops(&mut same, API_VERSION, &rest).expect("runs");
    assert_eq!(piks::save(e.doc()), piks::save(same.doc()));
}

#[test]
fn references_that_cannot_fit_are_refused_before_anything_runs() {
    let cases = [
        // Forward, to itself, out of range.
        (
            r#"[{"op":"remove_frame","frame":{"result":1}},{"op":"add_frame","at":0}]"#,
            0,
        ),
        (
            r#"[{"op":"add_frame","at":0},{"op":"remove_frame","frame":{"result":1}}]"#,
            1,
        ),
        (
            r#"[{"op":"add_frame","at":0},{"op":"remove_frame","frame":{"result":99}}]"#,
            1,
        ),
        // Wrong type: a layer for a frame, a frame for a layer, a swatch, nothing.
        (
            r#"[{"op":"add_layer"},{"op":"remove_frame","frame":{"result":0}}]"#,
            1,
        ),
        (
            r#"[{"op":"add_frame","at":0},{"op":"remove_layer","layer":{"result":0}}]"#,
            1,
        ),
        (
            r#"[{"op":"add_swatch","color":4278190335},{"op":"remove_layer","layer":{"result":0}}]"#,
            1,
        ),
        (
            r#"[{"op":"version"},{"op":"remove_layer","layer":{"result":0}}]"#,
            1,
        ),
        // A document needs a place; one id takes none.
        (
            r#"[{"op":"document"},{"op":"remove_frame","frame":{"result":0}}]"#,
            1,
        ),
        (
            r#"[{"op":"add_frame","at":0},{"op":"remove_frame","frame":{"result":0,"place":0}}]"#,
            1,
        ),
    ];
    for (text, at) in cases {
        let refusal = read_ops(&list(text)).expect_err(text);
        assert_eq!(
            (refusal.code, refusal.at),
            ("bad_result", Some(at)),
            "{text}: {refusal:?}"
        );
    }
}

#[test]
fn malformed_references_are_bad_requests() {
    let cases = [
        r#"[{"op":"add_frame","at":0},{"op":"remove_frame","frame":{"result":-1}}]"#,
        r#"[{"op":"add_frame","at":0},{"op":"remove_frame","frame":{"result":"0"}}]"#,
        r#"[{"op":"add_frame","at":0},{"op":"remove_frame","frame":{}}]"#,
        r#"[{"op":"add_frame","at":0},{"op":"remove_frame","frame":{"result":0,"of":1}}]"#,
        r#"[{"op":"document"},{"op":"remove_frame","frame":{"result":0,"place":-2}}]"#,
        r#"[{"op":"add_frame","at":0},{"op":"remove_frame","frame":[0]}]"#,
        // Places and other numbers take no references.
        r#"[{"op":"add_frame","at":0},{"op":"add_frame","at":{"result":0}}]"#,
        r#"[{"op":"add_layer"},{"op":"move_layer","layer":1,"to":{"result":0}}]"#,
        r#"[{"op":"add_layer"},{"op":"set_pixel","layer":1,"frame":0,"x":{"result":0},"y":0,"color":0}]"#,
        r#"[{"op":"add_swatch","color":1},{"op":"recolor","swatch":{"result":0},"color":2}]"#,
    ];
    for text in cases {
        let refusal = read_ops(&list(text)).expect_err(text);
        assert_eq!(
            (refusal.code, refusal.at),
            ("bad_request", Some(1)),
            "{text}: {refusal:?}"
        );
    }
}

#[test]
fn a_place_past_a_documents_end_fails_the_run_and_takes_it_back() {
    let planned = read_ops(&list(
        r#"[{"op":"add_frame","at":0},{"op":"document"},{"op":"remove_frame","frame":{"result":1,"place":3}}]"#,
    ))
    .expect("fits until it runs");
    let mut e = editor();
    let before = (piks::save_without_ids(e.doc()), e.revision());
    let floor = e.doc().frame_floor();
    let refusal = run(&mut e, 1, &planned).err().expect("refused");
    assert_eq!(
        (refusal.code, refusal.at),
        ("bad_result", Some(2)),
        "{refusal:?}"
    );
    assert!(refusal.message.contains("3 frames"), "{}", refusal.message);
    // Everything but the ids is as before; the frame the run added has used up its id.
    assert_eq!((piks::save_without_ids(e.doc()), e.revision()), before);
    assert!(e.doc().frame_floor() > floor);
}
