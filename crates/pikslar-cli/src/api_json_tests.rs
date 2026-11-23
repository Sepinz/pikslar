use super::*;
use crate::json::{parse, to_text, LIMITS};
use pikslar_api::{DocumentInfo, LayerInfo};

fn op_of(text: &str) -> Result<Op, Refusal> {
    op(
        &parse(text.as_bytes(), &LIMITS).expect("JSON"),
        &crate::results::Plan::new(&[]),
    )
}

/// Every operation's name. The match has no catch-all, so a new operation fails to build here
/// until it is given a name and a row in [`every_op`].
fn name(op: &Op) -> &'static str {
    match op {
        Op::Version => "version",
        Op::Document => "document",
        Op::Layer { .. } => "layer",
        Op::Pixel { .. } => "pixel",
        Op::Swatches => "swatches",
        Op::AddLayer { .. } => "add_layer",
        Op::RemoveLayer { .. } => "remove_layer",
        Op::RenameLayer { .. } => "rename_layer",
        Op::MoveLayer { .. } => "move_layer",
        Op::DuplicateLayer { .. } => "duplicate_layer",
        Op::SetOpacity { .. } => "set_opacity",
        Op::SetVisible { .. } => "set_visible",
        Op::SetBlend { .. } => "set_blend",
        Op::AddFrame { .. } => "add_frame",
        Op::RemoveFrame { .. } => "remove_frame",
        Op::SetDuration { .. } => "set_duration",
        Op::AddTag { .. } => "add_tag",
        Op::SetPixel { .. } => "set_pixel",
        Op::ErasePixel { .. } => "erase_pixel",
        Op::DrawLine { .. } => "draw_line",
        Op::Fill { .. } => "fill",
        Op::PutPixels { .. } => "put_pixels",
        Op::FillRect { .. } => "fill_rect",
        Op::DrawText { .. } => "draw_text",
        Op::Pixels { .. } => "pixels",
        Op::AddSwatch { .. } => "add_swatch",
        Op::Recolor { .. } => "recolor",
        Op::RemoveSwatch { .. } => "remove_swatch",
    }
}

fn reading_ops() -> Vec<(&'static str, Op)> {
    vec![
        (r#"{"op":"version"}"#, Op::Version),
        (r#"{"op":"document"}"#, Op::Document),
        (r#"{"op":"layer","layer":1}"#, Op::Layer { layer: 1 }),
        (
            r#"{"op":"pixel","layer":1,"frame":0,"x":2,"y":3}"#,
            Op::Pixel {
                layer: 1,
                frame: 0,
                x: 2,
                y: 3,
            },
        ),
        (r#"{"op":"swatches"}"#, Op::Swatches),
    ]
}

fn layer_ops() -> Vec<(&'static str, Op)> {
    vec![
        (
            r#"{"op":"add_layer","above":1,"mode":"palette","name":"Ink"}"#,
            Op::AddLayer {
                above: Some(1),
                mode: Mode::Palette,
                name: Some("Ink".into()),
            },
        ),
        (
            r#"{"op":"remove_layer","layer":2}"#,
            Op::RemoveLayer { layer: 2 },
        ),
        (
            r#"{"op":"rename_layer","layer":1,"name":"Base"}"#,
            Op::RenameLayer {
                layer: 1,
                name: "Base".into(),
            },
        ),
        (
            r#"{"op":"move_layer","layer":1,"to":1}"#,
            Op::MoveLayer { layer: 1, to: 1 },
        ),
        (
            r#"{"op":"duplicate_layer","layer":1}"#,
            Op::DuplicateLayer { layer: 1 },
        ),
        (
            r#"{"op":"set_opacity","layer":1,"opacity":77}"#,
            Op::SetOpacity {
                layer: 1,
                opacity: 77,
            },
        ),
        (
            r#"{"op":"set_visible","layer":1,"visible":false}"#,
            Op::SetVisible {
                layer: 1,
                visible: false,
            },
        ),
        (
            r#"{"op":"set_blend","layer":1,"blend":"Screen"}"#,
            Op::SetBlend {
                layer: 1,
                blend: "Screen".into(),
            },
        ),
    ]
}

fn frame_ops() -> Vec<(&'static str, Op)> {
    vec![
        (
            r#"{"op":"add_frame","at":2,"copy":true}"#,
            Op::AddFrame { at: 2, copy: true },
        ),
        (
            r#"{"op":"remove_frame","frame":1}"#,
            Op::RemoveFrame { frame: 1 },
        ),
        (
            r#"{"op":"set_duration","frame":0,"ms":40}"#,
            Op::SetDuration { frame: 0, ms: 40 },
        ),
        (
            r#"{"op":"add_tag","name":"loop","from":0,"to":1}"#,
            Op::AddTag {
                name: "loop".into(),
                from: 0,
                to: 1,
            },
        ),
    ]
}

fn pixel_ops() -> Vec<(&'static str, Op)> {
    vec![
        (
            r#"{"op":"set_pixel","layer":1,"frame":0,"x":7,"y":2,"color":4278190335}"#,
            Op::SetPixel {
                layer: 1,
                frame: 0,
                x: 7,
                y: 2,
                color: 4_278_190_335,
            },
        ),
        (
            r#"{"op":"erase_pixel","layer":1,"frame":0,"x":7,"y":2}"#,
            Op::ErasePixel {
                layer: 1,
                frame: 0,
                x: 7,
                y: 2,
            },
        ),
        (
            r#"{"op":"draw_line","layer":1,"frame":0,"from":[0,11],"to":[15,0],"color":4278190335}"#,
            Op::DrawLine {
                layer: 1,
                frame: 0,
                from: (0, 11),
                to: (15, 0),
                color: 4_278_190_335,
            },
        ),
        (
            r#"{"op":"fill","layer":1,"frame":0,"x":0,"y":0,"color":4294901760}"#,
            Op::Fill {
                layer: 1,
                frame: 0,
                x: 0,
                y: 0,
                color: 4_294_901_760,
            },
        ),
        (
            r#"{"op":"add_swatch","color":2131763248}"#,
            Op::AddSwatch {
                color: 2_131_763_248,
            },
        ),
        (
            r#"{"op":"recolor","swatch":1,"color":4278190335}"#,
            Op::Recolor {
                swatch: 1,
                color: 4_278_190_335,
            },
        ),
        (
            r#"{"op":"remove_swatch","swatch":1}"#,
            Op::RemoveSwatch { swatch: 1 },
        ),
    ]
}

/// One valid object for every operation, with the operation it reads as.
pub(crate) fn every_op() -> Vec<(&'static str, Op)> {
    [
        reading_ops(),
        layer_ops(),
        frame_ops(),
        pixel_ops(),
        block_ops(),
    ]
    .concat()
}

#[test]
fn every_op_is_read_by_its_name() {
    let table = every_op();
    let mut names: Vec<&str> = table.iter().map(|(_, op)| name(op)).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), table.len(), "each op once");
    assert_eq!(names.len(), 28, "every op of API version 1");
    for (text, want) in table {
        assert_eq!(op_of(text).expect(text), want, "{text}");
        assert!(text.contains(&format!("\"op\":\"{}\"", name(&want))));
    }
}

#[test]
fn optional_fields_take_their_defaults() {
    let plain = Op::AddLayer {
        above: None,
        mode: Mode::Rgba,
        name: None,
    };
    assert_eq!(op_of(r#"{"op":"add_layer"}"#).expect("bare"), plain);
    let nulls = r#"{"op":"add_layer","above":null,"mode":null,"name":null}"#;
    assert_eq!(op_of(nulls).expect("nulls"), plain);
    let copy = op_of(r#"{"op":"add_frame","at":0}"#).expect("no copy");
    assert_eq!(copy, Op::AddFrame { at: 0, copy: false });
}

#[test]
fn malformed_ops_are_refused_with_a_reason() {
    for (text, says) in [
        (r#"[]"#, "must be an object"),
        (r#"{}"#, "needs \"op\""),
        (r#"{"op":1}"#, "must be a string"),
        (r#"{"op":"explode"}"#, "no operation"),
        (r#"{"op":"Version"}"#, "no operation"),
        (r#"{"op":"layer"}"#, "needs \"layer\""),
        (r#"{"op":"layer","layer":"1"}"#, "whole number"),
        (r#"{"op":"layer","layer":true}"#, "whole number"),
        (r#"{"op":"layer","layer":null}"#, "needs \"layer\""),
        (
            r#"{"op":"layer","layer":1,"extra":0}"#,
            "no field \"extra\"",
        ),
        (r#"{"op":"version","layer":1}"#, "no field \"layer\""),
        (
            r#"{"op":"set_visible","layer":1,"visible":1}"#,
            "true or false",
        ),
        (r#"{"op":"add_layer","mode":"cmyk"}"#, "rgba or palette"),
        (r#"{"op":"add_layer","name":5}"#, "must be a string"),
        (
            r#"{"op":"draw_line","layer":1,"frame":0,"from":[1],"to":[1,2],"color":0}"#,
            "[x, y]",
        ),
        (
            r#"{"op":"draw_line","layer":1,"frame":0,"from":[1,2,3],"to":[1,2],"color":0}"#,
            "[x, y]",
        ),
        (
            r#"{"op":"draw_line","layer":1,"frame":0,"from":{"x":1},"to":[1,2],"color":0}"#,
            "[x, y]",
        ),
    ] {
        let refusal = op_of(text).expect_err(text);
        assert_eq!(
            refusal.code,
            if says == "no operation" {
                "unknown_op"
            } else {
                "bad_request"
            }
        );
        assert!(
            refusal.message.contains(says),
            "{text}: {}",
            refusal.message
        );
    }
}

#[test]
fn replies_are_written_as_json() {
    let cases = [
        (Reply::Done, "null"),
        (Reply::Version(1), r#"{"version":1}"#),
        (Reply::Color(255), r#"{"color":255}"#),
        (Reply::Created(4), r#"{"id":4}"#),
        (Reply::Frame(9), r#"{"frame":9}"#),
        (
            Reply::Swatches(vec![(1, 2), (3, 4)]),
            r#"{"swatches":[{"id":1,"color":2},{"id":3,"color":4}]}"#,
        ),
        (
            Reply::Document(DocumentInfo {
                width: 16,
                height: 12,
                durations: vec![100, 40],
                frames: vec![7, 3],
                layers: vec![1, 2],
            }),
            r#"{"width":16,"height":12,"durations":[100,40],"frames":[7,3],"layers":[1,2]}"#,
        ),
        (
            Reply::Layer(LayerInfo {
                id: 2,
                index: 1,
                name: "A \"b\"".into(),
                kind: "paint",
                mode: Mode::Palette,
                visible: true,
                locked: false,
                opacity: 255,
                blend: "Normal",
            }),
            r#"{"id":2,"index":1,"name":"A \"b\"","kind":"paint","mode":"palette","visible":true,"locked":false,"opacity":255,"blend":"Normal"}"#,
        ),
    ];
    for (reply_given, want) in cases {
        assert_eq!(to_text(&reply(&reply_given)), want);
    }
}

#[test]
fn api_errors_have_stable_codes() {
    use pikslar_api::ApiError;
    let cases = [
        (
            ApiError::Version {
                wanted: 2,
                supported: 1,
            },
            "version",
        ),
        (ApiError::NoSuchLayer(9), "no_such_layer"),
        (ApiError::NoSuchFrame(9), "no_such_frame"),
        (ApiError::NoSuchSwatch(9), "no_such_swatch"),
        (
            ApiError::OutOfRange {
                what: "opacity",
                value: 300,
                min: 0,
                max: 255,
            },
            "out_of_range",
        ),
        (ApiError::Outside { x: -1, y: 0 }, "outside"),
        (ApiError::Transparent(5), "transparent"),
        (
            ApiError::BadName {
                name: String::new(),
                why: "is empty",
            },
            "bad_name",
        ),
        (ApiError::UnknownBlend("x".into()), "unknown_blend"),
        (
            ApiError::Refused(pikslar_core::LimitError::NoSuchItem),
            "refused",
        ),
        (ApiError::Cancelled, "cancelled"),
    ];
    for (error, code) in cases {
        let refusal = api_error(&error);
        assert_eq!(refusal.code, code);
        assert_eq!(refusal.message, error.to_string());
    }
}

fn block_ops() -> Vec<(&'static str, Op)> {
    vec![
        (
            r#"{"op":"put_pixels","layer":1,"frame":0,"x":1,"y":2,"width":2,"colors":[0,4278190335]}"#,
            Op::PutPixels {
                layer: 1,
                frame: 0,
                x: 1,
                y: 2,
                width: 2,
                colors: vec![0, 4_278_190_335],
            },
        ),
        (
            r#"{"op":"fill_rect","layer":1,"frame":0,"x":1,"y":2,"width":3,"height":4,"color":0}"#,
            Op::FillRect {
                layer: 1,
                frame: 0,
                x: 1,
                y: 2,
                width: 3,
                height: 4,
                color: 0,
            },
        ),
        (
            r#"{"op":"draw_text","layer":1,"frame":0,"x":3,"y":4,"text":"Hi\nyou","color":4278190335,"scale":2}"#,
            Op::DrawText {
                layer: 1,
                frame: 0,
                x: 3,
                y: 4,
                text: "Hi\nyou".into(),
                color: 4_278_190_335,
                scale: 2,
            },
        ),
        (
            r#"{"op":"pixels","layer":1,"frame":0,"x":0,"y":0,"width":2,"height":1}"#,
            Op::Pixels {
                layer: 1,
                frame: 0,
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
        ),
    ]
}

#[test]
fn put_pixels_refuses_colors_that_are_not_numbers() {
    let text = r#"{"op":"put_pixels","layer":1,"frame":0,"x":0,"y":0,"width":1,"colors":["red"]}"#;
    assert!(op_of(text).is_err());
}

#[test]
fn pixels_replies_with_a_list() {
    let text = to_text(&reply(&Reply::Pixels(vec![0, 255])));
    assert_eq!(text, r#"{"pixels":[0,255]}"#);
}
