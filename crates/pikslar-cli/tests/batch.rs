//! `batch`: a list of scripting API operations applied to a project as one transaction, run
//! as a build script would. All or nothing: a refused operation or a malformed file writes
//! nothing, and a done batch saves the bytes the same API calls would.

mod common;
use common::{bottom_layer, cli, cli_input, folder, listing, project};
use pikslar_api::{run_ops, Mode, Op, API_VERSION};
use pikslar_core::Editor;
use pikslar_format::piks;
use std::fs;
use std::path::Path;

#[path = "../../pikslar-format/src/ase_build.rs"]
mod ase_build;
use ase_build::*;

/// An edit touching layers, frames, pixels and the palette. Frames are named through the
/// first operation's `document` result, and the new frame through `add_frame`'s.
fn script(bottom: i64) -> String {
    let place = |at: u32| format!(r#"{{"result":0,"place":{at}}}"#);
    let (first, second) = (place(0), place(1));
    format!(
        r#"{{"api":1,"ops":[
  {{"op":"document"}},
  {{"op":"add_layer","mode":"palette","name":"Ink"}},
  {{"op":"set_pixel","layer":{bottom},"frame":{second},"x":3,"y":2,"color":4278190335}},
  {{"op":"draw_line","layer":{bottom},"frame":{first},"from":[0,7],"to":[7,0],"color":4294901760}},
  {{"op":"set_opacity","layer":{bottom},"opacity":77}},
  {{"op":"add_frame","at":2,"copy":true}},
  {{"op":"set_duration","frame":{{"result":5}},"ms":40}},
  {{"op":"add_tag","name":"loop","from":{first},"to":{{"result":5}}}},
  {{"op":"add_swatch","color":2131763248}},
  {{"op":"pixel","layer":{bottom},"frame":{second},"x":3,"y":2}},
  {{"op":"document"}}
]}}"#
    )
}

/// The bytes the project would save as after the same edit through the API directly, with
/// the frame handles read from the document.
fn by_api(project: &Path, bottom: i64) -> Vec<u8> {
    let doc = piks::load(&fs::read(project).expect("read")).expect("opens");
    let mut editor = Editor::new(doc);
    let h = |e: &Editor, at: usize| i64::try_from(e.doc().frames[at].key().get()).expect("small");
    let (first, second) = (h(&editor, 0), h(&editor, 1));
    let ops = [
        Op::AddLayer {
            above: None,
            mode: Mode::Palette,
            name: Some("Ink".into()),
        },
        Op::SetPixel {
            layer: bottom,
            frame: second,
            x: 3,
            y: 2,
            color: 4_278_190_335,
        },
        Op::DrawLine {
            layer: bottom,
            frame: first,
            from: (0, 7),
            to: (7, 0),
            color: 4_294_901_760,
        },
        Op::SetOpacity {
            layer: bottom,
            opacity: 77,
        },
        Op::AddFrame { at: 2, copy: true },
    ];
    run_ops(&mut editor, API_VERSION, &ops).expect("the API runs it");
    let made = h(&editor, 2);
    let rest = [
        Op::SetDuration {
            frame: made,
            ms: 40,
        },
        Op::AddTag {
            name: "loop".into(),
            from: first,
            to: made,
        },
        Op::AddSwatch {
            color: 2_131_763_248,
        },
    ];
    run_ops(&mut editor, API_VERSION, &rest).expect("the API runs it");
    piks::try_save(editor.doc()).expect("saves")
}

#[test]
fn a_batch_saves_what_the_same_api_calls_would_byte_for_byte() {
    let dir = folder("batch-bytes");
    let p = project(&dir.join("walk.piks"), 8, 2, 3);
    let bottom = bottom_layer(&p);
    fs::write(dir.join("ops.json"), script(bottom)).expect("ops");
    let before = fs::read(&p).expect("read");
    let out = dir.join("out.piks");
    let run = cli(&[&"batch", &p, &dir.join("ops.json"), &"--out", &out]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(fs::read(&out).expect("written"), by_api(&p, bottom));
    assert_eq!(
        fs::read(&p).expect("read"),
        before,
        "the source is untouched"
    );
    assert_eq!(run.out.lines().count(), 1, "{}", run.out);
    assert!(run.out.contains(r#""changed":true"#), "{}", run.out);
    assert!(run.out.contains(r#"{"color":4278190335}"#), "{}", run.out);
    assert!(run.out.contains(r#""durations":[20,30,40]"#) || run.out.contains("durations"));
}

#[test]
fn a_batch_from_standard_input_changes_the_project_in_place() {
    let dir = folder("batch-stdin");
    let p = project(&dir.join("walk.piks"), 8, 2, 4);
    let bottom = bottom_layer(&p);
    let (json, want) = (script(bottom), by_api(&p, bottom));
    let run = cli_input(&[&"batch", &p, &"-"], json.as_bytes());
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(fs::read(&p).expect("read"), want);
    assert_eq!(listing(&dir), ["walk.piks"], "no temporary file is left");
}

#[test]
fn a_refused_operation_writes_nothing() {
    let dir = folder("batch-refused");
    let p = project(&dir.join("walk.piks"), 8, 2, 5);
    let bottom = bottom_layer(&p);
    let before = fs::read(&p).expect("read");
    let late_failure = [
        format!(r#"{{"op":"set_opacity","layer":{bottom},"opacity":300}}"#),
        r#"{"op":"layer","layer":999}"#.to_string(),
        format!(
            r#"{{"op":"set_pixel","layer":{bottom},"frame":0,"x":8,"y":0,"color":4278190335}}"#
        ),
        r#"{"op":"set_duration","frame":0,"ms":5}"#.to_string(),
        format!(r#"{{"op":"set_blend","layer":{bottom},"blend":"multiply"}}"#),
        r#"{"op":"add_swatch","color":5}"#.to_string(),
    ];
    for bad in late_failure {
        let ops = format!(
            r#"{{"api":1,"ops":[{{"op":"add_frame","at":0}},{{"op":"set_visible","layer":{bottom},"visible":false}},{bad}]}}"#
        );
        for out in [false, true] {
            let target = dir.join("new.piks");
            let run = match out {
                true => cli_input(&[&"batch", &p, &"-", &"--out", &target], ops.as_bytes()),
                false => cli_input(&[&"batch", &p, &"-"], ops.as_bytes()),
            };
            assert_eq!(run.code, 3, "{bad}: {}", run.err);
            assert!(run.err.contains("operation 2"), "{bad}: {}", run.err);
            assert!(run.out.is_empty(), "{}", run.out);
            assert_eq!(fs::read(&p).expect("read"), before, "{bad}");
            assert_eq!(listing(&dir), ["walk.piks"], "{bad}");
        }
    }
}

#[test]
fn a_reference_that_cannot_be_resolved_writes_nothing() {
    let dir = folder("batch-references");
    let p = project(&dir.join("walk.piks"), 8, 2, 7);
    let bottom = bottom_layer(&p);
    let before = fs::read(&p).expect("read");
    let edit = format!(r#"{{"op":"set_visible","layer":{bottom},"visible":false}}"#);
    let cases = [
        // Forward, to itself, out of range, the wrong type: refused before anything runs.
        (
            r#"{"op":"remove_frame","frame":{"result":3}}"#,
            "only an earlier",
        ),
        (
            r#"{"op":"remove_frame","frame":{"result":2}}"#,
            "only an earlier",
        ),
        (
            r#"{"op":"remove_frame","frame":{"result":99}}"#,
            "only an earlier",
        ),
        (
            r#"{"op":"remove_frame","frame":{"result":0}}"#,
            "gives a document",
        ),
        (
            r#"{"op":"remove_layer","layer":{"result":1}}"#,
            "which gives a frame handle",
        ),
        // Past the end of the document's frames: found as the list runs, and taken back.
        (
            r#"{"op":"remove_frame","frame":{"result":0,"place":2}}"#,
            "no place 2",
        ),
    ];
    for (bad, says) in cases {
        let ops = format!(
            r#"{{"api":1,"ops":[{{"op":"document"}},{{"op":"add_frame","at":0}},{bad},{edit}]}}"#
        );
        for out in [false, true] {
            let target = dir.join("new.piks");
            let run = match out {
                true => cli_input(&[&"batch", &p, &"-", &"--out", &target], ops.as_bytes()),
                false => cli_input(&[&"batch", &p, &"-"], ops.as_bytes()),
            };
            assert_ne!(run.code, 0, "{bad}");
            assert!(run.err.contains("operation 2"), "{bad}: {}", run.err);
            assert!(run.err.contains(says), "{bad}: {}", run.err);
            assert!(run.out.is_empty(), "{}", run.out);
            assert_eq!(fs::read(&p).expect("read"), before, "{bad}");
            assert_eq!(listing(&dir), ["walk.piks"], "{bad}");
        }
    }
}

#[test]
fn malformed_operation_files_are_refused_before_anything_runs() {
    let dir = folder("batch-malformed");
    let p = project(&dir.join("walk.piks"), 8, 1, 6);
    let before = fs::read(&p).expect("read");
    let ok = r#"{"op":"add_frame","at":0}"#;
    let many = format!(r#"{{"api":1,"ops":[{}]}}"#, vec![ok; 10_001].join(","));
    let cases: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"{".to_vec(),
        br#"{"api":1,"ops":[{"op":"add_frame","at":0}"#.to_vec(),
        br#"{"api":1}"#.to_vec(),
        br#"{"ops":[]}"#.to_vec(),
        br#"{"api":"1","ops":[]}"#.to_vec(),
        br#"{"api":1.0,"ops":[]}"#.to_vec(),
        br#"{"api":1,"ops":{}}"#.to_vec(),
        br#"{"api":1,"ops":[],"api":1}"#.to_vec(),
        br#"{"api":1,"ops":[],"extra":true}"#.to_vec(),
        br#"{"api":1,"ops":[]} trailing"#.to_vec(),
        b"\xEF\xBB\xBF{\"api\":1,\"ops\":[]}".to_vec(),
        b"{\"api\":1,\"ops\":[{\"op\":\"add_tag\",\"name\":\"\xff\",\"from\":0,\"to\":0}]}"
            .to_vec(),
        format!(r#"{{"api":1,"ops":[{ok},{{"op":"add_frame","at":0,"copy":1}}]}}"#).into_bytes(),
        format!(r#"{{"api":1,"ops":[{ok},{{"op":"warp"}}]}}"#).into_bytes(),
        format!(r#"{{"api":1,"ops":[{ok},{{"op":"add_frame","at":1e400}}]}}"#).into_bytes(),
        format!(r#"{{"api":1,"ops":[{ok},{{"op":"add_frame","at":NaN}}]}}"#).into_bytes(),
        many.into_bytes(),
        format!(r#"{{"api":1,"ops":[{ok}]}}{}"#, " ".repeat(1 << 20)).into_bytes(),
        format!("{}{}", "[".repeat(100_000), "]".repeat(100_000)).into_bytes(),
    ];
    for (i, case) in cases.iter().enumerate() {
        let run = cli_input(&[&"batch", &p, &"-"], case);
        assert_eq!(run.code, 2, "case {i}: {}", run.err);
        assert!(
            run.err.starts_with("pikslar-cli: "),
            "case {i}: {}",
            run.err
        );
        assert_eq!(fs::read(&p).expect("read"), before, "case {i}");
        assert_eq!(listing(&dir), ["walk.piks"], "case {i}");
    }
}

#[test]
fn another_api_version_is_refused() {
    let dir = folder("batch-version");
    let p = project(&dir.join("walk.piks"), 8, 1, 7);
    let before = fs::read(&p).expect("read");
    let run = cli_input(
        &[&"batch", &p, &"-"],
        br#"{"api":2,"ops":[{"op":"add_frame","at":0}]}"#,
    );
    assert_eq!(run.code, 3, "{}", run.err);
    assert!(run.err.contains("version 2"), "{}", run.err);
    assert_eq!(fs::read(&p).expect("read"), before);
}

#[test]
fn a_batch_that_changes_nothing_does_not_write() {
    let dir = folder("batch-nothing");
    let p = project(&dir.join("walk.piks"), 8, 1, 8);
    let stamp = fs::metadata(&p).and_then(|m| m.modified()).expect("time");
    let run = cli_input(
        &[&"batch", &p, &"-"],
        br#"{"api":1,"ops":[{"op":"version"},{"op":"swatches"}]}"#,
    );
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        run.out.trim(),
        r#"{"results":[{"version":1},{"swatches":[]}],"changed":false,"written":null}"#
    );
    assert_eq!(
        fs::metadata(&p).and_then(|m| m.modified()).expect("time"),
        stamp
    );
}

#[test]
fn targets_are_checked_as_other_commands_do() {
    let dir = folder("batch-targets");
    let p = project(&dir.join("walk.piks"), 8, 1, 9);
    let other = project(&dir.join("other.piks"), 8, 1, 10);
    let before = fs::read(&other).expect("read");
    let ops = br#"{"api":1,"ops":[{"op":"add_frame","at":0}]}"#;
    let run = cli_input(&[&"batch", &p, &"-", &"--out", &other], ops);
    assert_eq!(run.code, 4, "{}", run.err);
    assert_eq!(fs::read(&other).expect("read"), before);
    let run = cli_input(&[&"batch", &p, &"-", &"--out", &dir.join("x.png")], ops);
    assert_eq!(run.code, 1, "{}", run.err);
    let run = cli_input(&[&"batch", &p, &"-", &"--out", &other, &"--overwrite"], ops);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_ne!(fs::read(&other).expect("read"), before);
    for args in [
        vec!["batch"],
        vec!["batch", "a.piks"],
        vec!["batch", "a", "b", "c"],
    ] {
        let args: Vec<&dyn AsRef<std::ffi::OsStr>> = args.iter().map(|a| a as _).collect();
        assert_eq!(cli(&args).code, 1);
    }
}

#[test]
fn an_aseprite_file_is_only_read() {
    let dir = folder("batch-ase");
    let ase = dir.join("hero.ase");
    let bytes = Ase::new(4, 3, 32).frame(100, vec![plain("ink")]).bytes();
    fs::write(&ase, &bytes).expect("write");
    let ops = br#"{"api":1,"ops":[{"op":"add_frame","at":0}]}"#;
    let run = cli_input(&[&"batch", &ase, &"-"], ops);
    assert_eq!(run.code, 3, "{}", run.err);
    assert!(run.err.contains("--out"), "{}", run.err);
    assert_eq!(fs::read(&ase).expect("read"), bytes);
    let out = dir.join("from-ase.piks");
    let run = cli_input(&[&"batch", &ase, &"-", &"--out", &out], ops);
    assert_eq!(run.code, 0, "{}", run.err);
    let saved = piks::load(&fs::read(&out).expect("written")).expect("a project");
    assert_eq!(saved.frames.len(), 2);
    assert_eq!(fs::read(&ase).expect("read"), bytes);
}
