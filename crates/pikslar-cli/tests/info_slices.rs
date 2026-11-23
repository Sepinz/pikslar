//! `info` and slices: a count and a line each in the text, always a list in the JSON.

mod common;
use common::{cli, corpus, folder};
use pikslar_core::{ColorMode, Document, Slice, SliceRect};
use pikslar_format::piks;
use std::fs;

fn rect(x: i32, y: i32, width: u32, height: u32) -> SliceRect {
    SliceRect {
        x,
        y,
        width,
        height,
    }
}

fn sliced() -> Document {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let plain = Slice {
        name: "hit \"box\"".into(),
        color: 0xff_00_80_ff,
        bounds: rect(-2, 3, 8, 5),
        pivot: None,
        center: None,
    };
    let full = Slice {
        name: "panel".into(),
        color: 0x01_02_03_04,
        bounds: rect(1, 1, 10, 10),
        pivot: Some((4, -1)),
        center: Some(rect(2, 3, 5, 4)),
    };
    doc.add_slice(plain).expect("fits");
    doc.add_slice(full).expect("fits");
    doc
}

#[test]
fn the_text_counts_the_slices_and_describes_each() {
    let d = folder("info-slices-text");
    let project = d.join("s.piks");
    fs::write(&project, piks::save(&sliced())).expect("write");
    let out = cli(&[&"info", &project]).out;
    assert!(out.contains("Slices: 2\n"), "{out}");
    assert!(out.contains("  hit \"box\": at -2, 3, 8 x 5\n"), "{out}");
    assert!(
        out.contains("  panel: at 1, 1, 10 x 10, pivot 4, -1, center 2, 3, 5 x 4\n"),
        "{out}"
    );
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn the_json_lists_the_slices_with_pivot_and_center_when_set() {
    let d = folder("info-slices-json");
    let project = d.join("s.piks");
    fs::write(&project, piks::save(&sliced())).expect("write");
    let json = cli(&[&"info", &project, &"--json"]).out;
    assert!(
        json.contains(
            "\"slices\":[{\"name\":\"hit \\\"box\\\"\",\"color\":\"#ff0080ff\",\
             \"bounds\":{\"x\":-2,\"y\":3,\"w\":8,\"h\":5}},\
             {\"name\":\"panel\",\"color\":\"#01020304\",\
             \"bounds\":{\"x\":1,\"y\":1,\"w\":10,\"h\":10},\
             \"pivot\":{\"x\":4,\"y\":-1},\
             \"center\":{\"x\":2,\"y\":3,\"w\":5,\"h\":4}}]}\n"
        ),
        "{json}"
    );
    assert_eq!(json.lines().count(), 1);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn without_slices_the_text_is_unchanged_and_the_json_has_an_empty_list() {
    let p = corpus("v3-animated.piks");
    let text = cli(&[&"info", &p]).out;
    assert!(!text.contains("Slices"), "{text}");
    let json = cli(&[&"info", &p, &"--json"]).out;
    assert!(json.ends_with("],\"slices\":[]}\n"), "{json}");
}

#[test]
fn a_slice_name_with_control_characters_is_shown_not_obeyed() {
    let d = folder("info-slices-escape");
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("size");
    let s = Slice {
        name: "a\u{1b}[2Jb".into(),
        color: 0,
        bounds: rect(0, 0, 2, 2),
        pivot: None,
        center: None,
    };
    doc.add_slice(s).expect("fits");
    let project = d.join("s.piks");
    fs::write(&project, piks::save(&doc)).expect("write");
    let out = cli(&[&"info", &project]).out;
    assert!(out.contains("  a\\u{1b}[2Jb: at 0, 0, 2 x 2\n"), "{out}");
    assert!(!out.contains('\u{1b}'), "{out:?}");
    fs::remove_dir_all(d).expect("cleanup");
}
