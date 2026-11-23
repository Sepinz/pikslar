//! The JSON lines session, in process: the handshake, every request, the folder it is kept
//! in, and a corpus of malformed and mutated lines, each of which gets one reply and changes
//! nothing.

use super::*;
use crate::api_json::{self, tests::every_op};
use crate::json::LIMITS;
use pikslar_api::{run_ops, Op, API_VERSION};
use pikslar_core::{rgba, ColorMode, Document, DrawCmd, Editor, FrameCmd, LayerCmd, PaletteCmd};
use pikslar_format::piks;
use std::fs;
use std::path::PathBuf;

const HELLO: &str = r#"{"id":0,"type":"hello","api":1}"#;
const OPEN: &str = r#"{"id":1,"type":"open","path":"walk.piks"}"#;

/// A fresh folder holding `walk.piks`: two layers (ids 1 and 2), two frames, a swatch, a dot.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-session-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let mut e = Editor::new(Document::new(16, 12, ColorMode::Rgba).expect("size"));
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Palette,
    })
    .expect("room");
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.apply(PaletteCmd::AddSwatch {
        color: rgba(20, 20, 220, 255),
    })
    .expect("room");
    (e.layer, e.frame, e.color) = (0, 0, rgba(220, 20, 20, 255));
    e.apply(DrawCmd::Stroke {
        tool: pikslar_core::Tool::Pencil,
        path: vec![(2, 3)],
    })
    .expect("a dot");
    fs::write(dir.join("walk.piks"), piks::save(e.doc())).expect("write");
    dir
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("list")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Runs a session over `input` in `dir`, giving back each reply line read as JSON.
fn talk(dir: &Path, input: &[u8]) -> Vec<Value> {
    let session = Session::new(Sandbox::new(dir).expect("a folder"));
    let mut out = Vec::new();
    serve(&mut io::Cursor::new(input), &mut out, session).expect("the session ends cleanly");
    let text = String::from_utf8(out).expect("replies are UTF-8");
    text.lines()
        .map(|l| json::parse(l.as_bytes(), &LIMITS).expect("each reply is JSON"))
        .collect()
}

fn lines(list: &[&str]) -> Vec<u8> {
    list.iter()
        .flat_map(|l| format!("{l}\n").into_bytes())
        .collect()
}

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    match v {
        Value::Object(m) => m
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .unwrap_or(&Value::Null),
        _ => &Value::Null,
    }
}

fn ok(v: &Value) -> bool {
    *get(v, "ok") == Value::Bool(true)
}

fn code(v: &Value) -> String {
    match get(get(v, "error"), "code") {
        Value::Str(s) => s.clone(),
        _ => String::new(),
    }
}

#[test]
fn hello_comes_first_and_names_the_version() {
    let dir = folder("hello");
    let replies = talk(
        &dir,
        &lines(&[
            OPEN,
            r#"{"id":"a","type":"hello","api":2}"#,
            r#"{"id":"b","type":"hello","api":1,"extra":1}"#,
            HELLO,
            OPEN,
        ]),
    );
    assert_eq!(replies.len(), 5);
    assert_eq!(code(&replies[0]), "hello_first");
    assert_eq!(*get(&replies[0], "id"), Value::Int(1));
    assert_eq!(code(&replies[1]), "version");
    assert_eq!(*get(&replies[1], "id"), Value::Str("a".into()));
    assert_eq!(code(&replies[2]), "bad_request");
    assert!(ok(&replies[3]), "{:?}", replies[3]);
    assert_eq!(*get(get(&replies[3], "result"), "api"), Value::Int(1));
    let limits = get(get(&replies[3], "result"), "limits");
    assert_eq!(*get(limits, "reply_cells"), Value::Int(1 << 20));
    assert_eq!(*get(limits, "pixel_work"), Value::Int(1 << 28));
    assert!(ok(&replies[4]), "{:?}", replies[4]);
}

/// The frame handles of a document, in order.
fn handles(e: &Editor) -> Vec<i64> {
    e.doc()
        .frames
        .iter()
        .map(|f| i64::try_from(f.key().get()).expect("small"))
        .collect()
}

#[test]
fn a_session_saves_what_the_same_api_calls_would_byte_for_byte() {
    let dir = folder("bytes");
    let second = r#"{"result":0,"place":1}"#;
    let run = format!(
        r#"{{"id":2,"type":"run","ops":[{{"op":"document"}},{{"op":"add_layer","name":"Ink"}},{{"op":"set_pixel","layer":1,"frame":{second},"x":7,"y":2,"color":4278190335}},{{"op":"add_frame","at":2,"copy":true}},{{"op":"add_tag","name":"loop","from":{{"result":0,"place":0}},"to":{{"result":3}}}},{{"op":"pixel","layer":1,"frame":{second},"x":7,"y":2}}]}}"#
    );
    let replies = talk(
        &dir,
        &lines(&[
            HELLO,
            OPEN,
            &run,
            r#"{"id":3,"type":"save","path":"out.piks"}"#,
            r#"{"id":4,"type":"close"}"#,
        ]),
    );
    assert!(replies.iter().all(ok), "{replies:?}");
    let results = get(get(&replies[2], "result"), "results");
    let Value::Array(results) = results else {
        panic!("results")
    };
    assert_eq!(
        results[1],
        Value::Object(vec![("id".into(), Value::Int(3))])
    );
    assert!(matches!(get(&results[3], "frame"), Value::Int(h) if *h > 0));
    assert_eq!(
        results[5],
        Value::Object(vec![("color".into(), Value::Int(4_278_190_335))])
    );

    let mut e =
        Editor::new(piks::load(&fs::read(dir.join("walk.piks")).expect("read")).expect("opens"));
    let h = handles(&e);
    let ops = [
        Op::AddLayer {
            above: None,
            mode: pikslar_api::Mode::Rgba,
            name: Some("Ink".into()),
        },
        Op::SetPixel {
            layer: 1,
            frame: h[1],
            x: 7,
            y: 2,
            color: 4_278_190_335,
        },
        Op::AddFrame { at: 2, copy: true },
    ];
    run_ops(&mut e, API_VERSION, &ops).expect("runs");
    let tag = Op::AddTag {
        name: "loop".into(),
        from: h[0],
        to: handles(&e)[2],
    };
    run_ops(&mut e, API_VERSION, &[tag]).expect("runs");
    let want = piks::try_save(e.doc()).expect("saves");
    assert_eq!(fs::read(dir.join("out.piks")).expect("saved"), want);
    assert_eq!(listing(&dir), ["out.piks", "walk.piks"]);
}

#[test]
fn a_refused_run_leaves_the_document_as_it_was() {
    let dir = folder("rollback");
    let original = fs::read(dir.join("walk.piks")).expect("read");
    let replies = talk(
        &dir,
        &lines(&[
            HELLO,
            OPEN,
            r#"{"id":2,"type":"run","ops":[{"op":"add_frame","at":0},{"op":"set_visible","layer":1,"visible":false},{"op":"set_opacity","layer":1,"opacity":256}]}"#,
            r#"{"id":3,"type":"run","ops":[{"op":"add_frame","at":0},{"op":"warp"}]}"#,
            r#"{"id":4,"type":"close"}"#,
            r#"{"id":5,"type":"save"}"#,
        ]),
    );
    assert_eq!(code(&replies[2]), "out_of_range");
    assert_eq!(*get(get(&replies[2], "error"), "at"), Value::Int(2));
    assert_eq!(code(&replies[3]), "unknown_op");
    assert_eq!(*get(get(&replies[3], "error"), "at"), Value::Int(1));
    assert!(
        ok(&replies[4]),
        "nothing changed, so close needs no discard: {:?}",
        replies[4]
    );
    assert_eq!(code(&replies[5]), "not_open");
    assert_eq!(fs::read(dir.join("walk.piks")).expect("read"), original);
}

/// The table's operations name frames by place, 0 or 1. As JSON each such field refers to
/// the place in a `document` operation put first; as an API call it is that frame's handle.
fn frame_fields(value: &Value) -> Value {
    let Value::Object(members) = value else {
        panic!("an operation")
    };
    let tag = members
        .iter()
        .any(|(k, v)| k == "op" && *v == Value::Str("add_tag".into()));
    let members = members.iter().map(|(k, v)| match (k.as_str(), v) {
        ("frame", Value::Int(place)) | ("from" | "to", Value::Int(place))
            if tag || k == "frame" =>
        {
            let place = Value::Int(*place);
            let refer = vec![("result".into(), Value::Int(0)), ("place".into(), place)];
            (k.clone(), Value::Object(refer))
        }
        _ => (k.clone(), v.clone()),
    });
    Value::Object(members.collect())
}

fn with_handles(op: Op, h: &[i64]) -> Op {
    let at = |place: i64| h[usize::try_from(place).expect("a place")];
    match op {
        Op::Pixel { layer, frame, x, y } => Op::Pixel {
            layer,
            frame: at(frame),
            x,
            y,
        },
        Op::RemoveFrame { frame } => Op::RemoveFrame { frame: at(frame) },
        Op::PutPixels {
            frame,
            x,
            y,
            width,
            colors,
            layer,
        } => Op::PutPixels {
            frame: at(frame),
            x,
            y,
            width,
            colors,
            layer,
        },
        Op::FillRect {
            frame,
            x,
            y,
            width,
            height,
            color,
            layer,
        } => Op::FillRect {
            frame: at(frame),
            x,
            y,
            width,
            height,
            color,
            layer,
        },
        Op::DrawText {
            frame,
            x,
            y,
            text,
            color,
            scale,
            layer,
        } => Op::DrawText {
            frame: at(frame),
            x,
            y,
            text,
            color,
            scale,
            layer,
        },
        Op::Pixels {
            frame,
            x,
            y,
            width,
            height,
            layer,
        } => Op::Pixels {
            frame: at(frame),
            x,
            y,
            width,
            height,
            layer,
        },
        Op::SetDuration { frame, ms } => Op::SetDuration {
            frame: at(frame),
            ms,
        },
        Op::AddTag { name, from, to } => Op::AddTag {
            name,
            from: at(from),
            to: at(to),
        },
        Op::SetPixel {
            layer,
            frame,
            x,
            y,
            color,
        } => Op::SetPixel {
            layer,
            frame: at(frame),
            x,
            y,
            color,
        },
        Op::ErasePixel { layer, frame, x, y } => Op::ErasePixel {
            layer,
            frame: at(frame),
            x,
            y,
        },
        Op::DrawLine {
            layer,
            frame,
            from,
            to,
            color,
        } => Op::DrawLine {
            layer,
            frame: at(frame),
            from,
            to,
            color,
        },
        Op::Fill {
            layer,
            frame,
            x,
            y,
            color,
        } => Op::Fill {
            layer,
            frame: at(frame),
            x,
            y,
            color,
        },
        other => other,
    }
}

/// A reply without its frame handles (`document`'s list, `add_frame`'s new one), which differ
/// between two openings of a file.
fn timing_only(value: &Value) -> Value {
    match value {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| k != "frames" && k != "frame")
                .cloned()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(timing_only).collect()),
        other => other.clone(),
    }
}

#[test]
fn every_op_gives_what_the_api_gives() {
    let dir = folder("every-op");
    let bytes = fs::read(dir.join("walk.piks")).expect("read");
    let mut done = 0;
    for (text, op) in every_op() {
        let parsed = json::parse(text.as_bytes(), &LIMITS).expect("JSON");
        let text = json::to_text(&frame_fields(&parsed));
        let run = format!(r#"{{"id":2,"type":"run","ops":[{{"op":"document"}},{text}]}}"#);
        let replies = talk(&dir, &lines(&[HELLO, OPEN, &run]));
        let mut e = Editor::new(piks::load(&bytes).expect("opens"));
        let op = with_handles(op, &handles(&e));
        match run_ops(&mut e, API_VERSION, &[Op::Document, op]) {
            Ok(want) => {
                done += 1;
                let results = get(get(&replies[2], "result"), "results");
                assert_eq!(
                    timing_only(results),
                    timing_only(&Value::Array(want.iter().map(api_json::reply).collect())),
                    "{text}"
                );
            }
            Err(error) => assert_eq!(
                code(&replies[2]),
                api_json::api_error(&error).code,
                "{text}"
            ),
        }
    }
    assert!(done >= 20, "most operations apply to the project: {done}");
}

#[test]
fn save_close_and_open_keep_their_rules() {
    let dir = folder("rules");
    fs::write(dir.join("taken.piks"), b"keep").expect("write");
    let edit = r#"{"id":2,"type":"run","ops":[{"op":"add_frame","at":0}]}"#;
    let replies = talk(
        &dir,
        &lines(&[
            HELLO,
            OPEN,
            OPEN,
            edit,
            r#"{"id":3,"type":"close"}"#,
            r#"{"id":4,"type":"save","path":"taken.piks"}"#,
            r#"{"id":5,"type":"save","path":"new.png"}"#,
            r#"{"id":6,"type":"save","path":"taken.piks","overwrite":true}"#,
            r#"{"id":7,"type":"close"}"#,
            OPEN,
            edit,
            r#"{"id":8,"type":"close","discard":true}"#,
            r#"{"id":9,"type":"open","path":"taken.piks"}"#,
            r#"{"id":10,"type":"save"}"#,
            r#"{"id":11,"type":"launch"}"#,
        ]),
    );
    let codes: Vec<String> = replies.iter().map(code).collect();
    assert_eq!(
        codes,
        [
            "",
            "",
            "already_open",
            "",
            "unsaved",
            "exists",
            "bad_path",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
            "unknown_type",
        ]
    );
    assert_ne!(fs::read(dir.join("taken.piks")).expect("read"), b"keep");
    assert_eq!(listing(&dir), ["taken.piks", "walk.piks"]);
}

#[test]
fn paths_stay_inside_the_folder() {
    let dir = folder("paths");
    let inner = dir.join("inner");
    fs::create_dir_all(&inner).expect("inner");
    fs::copy(dir.join("walk.piks"), inner.join("walk.piks")).expect("copy");
    let outside = dir
        .join("walk.piks")
        .display()
        .to_string()
        .replace('\\', "\\\\");
    let bad = [
        "",
        "..",
        "../walk.piks",
        "inner/../../walk.piks",
        "./walk.piks",
        "/walk.piks",
        "\\\\walk.piks",
        "C:walk.piks",
        "C:\\\\walk.piks",
        "\\\\\\\\server\\\\share\\\\a.piks",
        "\\\\\\\\?\\\\C:\\\\a.piks",
        "con.piks",
        "walk.piks:stream",
        "walk.piks.",
        "walk.piks ",
        "a\\u0000b.piks",
        "inner",
    ];
    for given in bad.iter().copied().chain([outside.as_str()]) {
        let open = format!(r#"{{"id":1,"type":"open","path":"{given}"}}"#);
        let save = format!(r#"{{"id":3,"type":"save","path":"{given}"}}"#);
        let replies = talk(&inner, &lines(&[HELLO, &open, OPEN, &save]));
        assert_eq!(replies.len(), 4, "{given}");
        assert!(!ok(&replies[1]), "open {given}: {:?}", replies[1]);
        assert!(ok(&replies[2]), "{given}");
        assert!(!ok(&replies[3]), "save {given}: {:?}", replies[3]);
    }
    assert_eq!(listing(&inner), ["walk.piks"]);
    let replies = talk(
        &inner,
        &lines(&[
            HELLO,
            r#"{"id":1,"type":"open","path":"./walk.piks"}"#,
            r#"{"id":2,"type":"open","path":"missing.piks"}"#,
            r#"{"id":3,"type":"open","path":"../walk.piks"}"#,
        ]),
    );
    assert_eq!(code(&replies[1]), "bad_path");
    assert_eq!(code(&replies[2]), "not_found");
    assert_eq!(code(&replies[3]), "bad_path");
}

/// Lines no reader should take, each answered with one error and the session going on.
fn malformed() -> Vec<Vec<u8>> {
    let deep = format!(
        r#"{{"id":5,"type":"run","ops":{}{}}}"#,
        "[".repeat(20),
        "]".repeat(20)
    );
    let many = format!(
        r#"{{"id":5,"type":"run","ops":[{}]}}"#,
        vec![r#"{"op":"version"}"#; MAX_OPS_PLUS_ONE].join(",")
    );
    let mut cases: Vec<Vec<u8>> = [
        "", " ", "{", r#"{"id":5"#, r#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":0}"#,
        "[]", "5", "null", r#""text""#, r#"{"type":"hello","api":1}"#,
        r#"{"id":null,"type":"hello","api":1}"#, r#"{"id":1.5,"type":"hello","api":1}"#,
        r#"{"id":[1],"type":"hello","api":1}"#, r#"{"id":5}"#, r#"{"id":5,"type":7}"#,
        r#"{"id":5,"type":"run"}"#, r#"{"id":5,"type":"run","ops":{}}"#,
        r#"{"id":5,"type":"run","ops":[1]}"#, r#"{"id":5,"type":"run","ops":[],"x":1}"#,
        r#"{"id":5,"id":6,"type":"run","ops":[]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":0,"at":1}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":NaN}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":Infinity}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":-0}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":1e400}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":0.0}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":99999999999999999999}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":"0"}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_tag","name":"\ud800","from":0,"to":0}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"add_tag","name":"\udc00x","from":0,"to":0}]}"#,
        r#"{"id":5,"type":"run","ops":[]} trailing"#, r#"{"id":5,"type":"run","ops":[]}}"#,
        r#"{"id":5,"type":"run","ops":[],}"#, r#"{'id':5}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"set_pixel","layer":1,"frame":0,"x":1,"y":1,"color":true}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"draw_line","layer":1,"frame":0,"from":[1,1,1],"to":[1,1],"color":0}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"rename_layer","layer":1,"name":"a\u0007b"}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"set_pixel","layer":1,"frame":0,"x":-9223372036854775808,"y":1,"color":0}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"set_pixel","layer":1,"frame":0,"x":1,"y":1,"color":-1}]}"#,
        r#"{"id":5,"type":"run","ops":[{"op":"set_duration","frame":9223372036854775807,"ms":40}]}"#,
        &deep, &many,
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect();
    cases.push(b"{\"id\":5,\"type\":\"run\",\"ops\":[{\"op\":\"add_tag\",\"name\":\"a\x01\",\"from\":0,\"to\":0}]}".to_vec());
    cases.push(b"{\"id\":5,\"type\":\"run\",\"ops\":[{\"op\":\"add_tag\",\"name\":\"\xff\",\"from\":0,\"to\":0}]}".to_vec());
    cases.push(b"\xEF\xBB\xBF{\"id\":5,\"type\":\"run\",\"ops\":[]}".to_vec());
    cases.push(b"{\"id\":5,\"type\":\"run\",\"ops\":[]}\x00".to_vec());
    cases.push(format!(r#"{{"id":"{}","type":"hello","api":1}}"#, "x".repeat(257)).into_bytes());
    cases.push(
        format!(
            r#"{{"id":5,"type":"run","ops":[{{"op":"add_tag","name":"{}","from":0,"to":0}}]}}"#,
            "x".repeat(5000)
        )
        .into_bytes(),
    );
    cases.push(vec![b'['; MAX_LINE_BYTES + 10]);
    cases
}

const MAX_OPS_PLUS_ONE: usize = crate::apply::MAX_OPS + 1;

#[test]
fn malformed_lines_get_one_error_each_and_change_nothing() {
    let dir = folder("malformed");
    let original = fs::read(dir.join("walk.piks")).expect("read");
    let cases = malformed();
    let mut input = lines(&[HELLO, OPEN]);
    for case in &cases {
        input.extend_from_slice(case);
        input.push(b'\n');
    }
    input.extend_from_slice(&lines(&[
        r#"{"id":"last","type":"run","ops":[{"op":"document"}]}"#,
        r#"{"id":"end","type":"close"}"#,
    ]));
    let replies = talk(&dir, &input);
    assert_eq!(replies.len(), cases.len() + 4);
    for (i, reply) in replies[2..2 + cases.len()].iter().enumerate() {
        let shown = String::from_utf8_lossy(&cases[i][..cases[i].len().min(80)]).into_owned();
        assert!(!ok(reply), "case {i} {shown}: {reply:?}");
        assert!(!code(reply).is_empty(), "case {i} {shown}: {reply:?}");
    }
    let n = cases.len();
    assert!(ok(&replies[n + 2]), "{:?}", replies[n + 2]);
    assert!(
        ok(&replies[n + 3]),
        "nothing was changed: {:?}",
        replies[n + 3]
    );
    assert_eq!(fs::read(dir.join("walk.piks")).expect("read"), original);
    assert_eq!(listing(&dir), ["walk.piks"]);
}

/// Every one-byte change to valid requests: deleted, doubled, replaced, or the line cut short.
#[test]
fn mutated_requests_never_crash_and_never_write() {
    let dir = folder("mutated");
    let original = fs::read(dir.join("walk.piks")).expect("read");
    let valid = [
        r#"{"id":2,"type":"run","ops":[{"op":"draw_line","layer":1,"frame":0,"from":[0,0],"to":[3,9],"color":4278190335},{"op":"add_frame","at":1,"copy":true}]}"#,
        r#"{"id":3,"type":"close","discard":true}"#,
    ];
    let mut sessions = 0;
    for line in valid {
        let bytes = line.as_bytes();
        for i in 0..bytes.len() {
            let mut variants = vec![
                [&bytes[..i], &bytes[i + 1..]].concat(),
                [&bytes[..=i], &bytes[i..]].concat(),
                bytes[..i].to_vec(),
            ];
            for with in *b"\"{]0\\\xff " {
                let mut changed = bytes.to_vec();
                changed[i] = with;
                variants.push(changed);
            }
            for variant in variants {
                let mut input = lines(&[HELLO, OPEN]);
                input.extend_from_slice(&variant);
                input.push(b'\n');
                let replies = talk(&dir, &input);
                assert_eq!(replies.len(), 3, "{}", String::from_utf8_lossy(&variant));
                sessions += 1;
            }
        }
    }
    assert!(sessions > 1000, "{sessions}");
    assert_eq!(fs::read(dir.join("walk.piks")).expect("read"), original);
    assert_eq!(listing(&dir), ["walk.piks"]);
}

#[test]
fn input_ending_inside_a_line_runs_nothing() {
    let dir = folder("unended");
    let original = fs::read(dir.join("walk.piks")).expect("read");
    let mut input = lines(&[
        HELLO,
        OPEN,
        r#"{"id":2,"type":"run","ops":[{"op":"add_frame","at":0}]}"#,
    ]);
    input.extend_from_slice(br#"{"id":3,"type":"save"}"#);
    let replies = talk(&dir, &input);
    assert_eq!(replies.len(), 4);
    assert!(ok(&replies[2]));
    assert_eq!(code(&replies[3]), "unended");
    assert_eq!(fs::read(dir.join("walk.piks")).expect("read"), original);
    assert!(talk(&dir, b"").is_empty(), "no input, no replies");
}

struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_closed_output_ends_the_session_with_a_write_failure() {
    let dir = folder("broken");
    let session = Session::new(Sandbox::new(&dir).expect("a folder"));
    let input = lines(&[HELLO, OPEN]);
    let ended = serve(&mut io::Cursor::new(input), &mut Broken, session);
    assert_eq!(ended.map_err(|f| f.code()), Err(4));
}

#[test]
fn error_text_never_holds_where_the_folder_is() {
    let dir = folder("nopath");
    fs::write(dir.join("junk.piks"), b"not a project").expect("junk");
    fs::create_dir(dir.join("box.piks")).expect("a folder");
    let outside = dir.join("outside");
    fs::create_dir(&outside).expect("outside");
    let inner = dir.join("inner");
    fs::create_dir(&inner).expect("inner");
    // A junction out of the folder, to something there and to something that is not.
    let made = |name: &str, target: &Path| {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(inner.join(name))
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let linked = made("there", &outside) && made("gone", &outside.join("nothing"));
    let opens = [
        r#"{"id":1,"type":"open","path":"junk.piks"}"#,
        r#"{"id":2,"type":"open","path":"box.piks"}"#,
        r#"{"id":3,"type":"open","path":"missing.piks"}"#,
        r#"{"id":4,"type":"open","path":"inner/there/a.piks"}"#,
        r#"{"id":5,"type":"open","path":"inner/gone/a.piks"}"#,
    ];
    let replies = talk(&dir, &lines(&[&[HELLO][..], &opens].concat()));
    let real = fs::canonicalize(&dir).expect("real").display().to_string();
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "\u{0}".into());
    for reply in &replies[1..] {
        let text = json::to_text(reply);
        assert!(!ok(reply), "{text}");
        for leak in [dir.display().to_string(), real.clone(), user.clone()] {
            assert!(
                !text.contains(&leak.replace('\\', "\\\\")),
                "{text} holds {leak}"
            );
        }
    }
    if linked {
        assert_eq!(
            code(&replies[4]),
            code(&replies[5]),
            "a link out says the same either way"
        );
    }
}
