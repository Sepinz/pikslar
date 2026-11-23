//! Sheet layouts and descriptions chosen on the command line.

mod common;
use common::{cli, corpus, folder, listing};
use pikslar_export::{sheet_plan, write_sheet_png, JsonShape, Layout, Picture, SheetOptions};
use pikslar_format::{piks, png};
use std::fs;

fn sheet(args: &[&str], out: &std::path::Path) -> common::Run {
    let project = corpus("v3-animated.piks");
    let mut all: Vec<&dyn AsRef<std::ffi::OsStr>> =
        vec![&"export", &project, &"--format", &"sheet", &"-o", &out];
    all.extend(args.iter().map(|a| a as &dyn AsRef<std::ffi::OsStr>));
    cli(&all)
}

#[test]
fn columns_padding_and_a_hash_are_the_bytes_the_library_makes() {
    let d = folder("sheet-options");
    let doc = piks::load(&fs::read(corpus("v3-animated.piks")).expect("read")).expect("loads");
    let last = doc.frames.len() - 1;
    let picture = Picture::visible(&doc);
    for (columns, padding, json) in [(2, 1, "hash"), (1, 0, "array"), (3, 4, "hash")] {
        let out = d.join(format!("s{columns}-{padding}.png"));
        let (c, p) = (columns.to_string(), padding.to_string());
        let run = sheet(&["--columns", &c, "--padding", &p, "--json", json], &out);
        assert_eq!(run.code, 0, "{}", run.err);
        let options = SheetOptions {
            layout: Layout {
                columns: Some(columns),
                padding,
            },
            json: if json == "hash" {
                JsonShape::Hash
            } else {
                JsonShape::Array
            },
        };
        let stem = format!("s{columns}-{padding}");
        let name = format!("{stem}.png");
        let plan = sheet_plan(&picture, 0..=last, (&name, &stem), &options).expect("planned");
        let mut image = Vec::new();
        write_sheet_png(&picture, (&plan.sheet, 0), None, &mut image).expect("made");
        assert_eq!(fs::read(&out).expect("image"), image, "{columns} {padding}");
        let text = fs::read_to_string(out.with_extension("json")).expect("description");
        assert_eq!(text, plan.description);
        let decoded = png::decode(&image).expect("decodes");
        let rows = (last + 1).div_ceil(columns);
        let wide = columns.min(last + 1);
        assert_eq!(
            (decoded.width, decoded.height),
            (
                wide * doc.width + (wide - 1) * padding,
                rows * doc.height + (rows - 1) * padding
            )
        );
    }
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn the_defaults_given_by_name_are_the_bytes_of_none_given() {
    let d = folder("sheet-defaults");
    let plain = d.join("plain.png");
    let named = d.join("named.png");
    assert_eq!(sheet(&[], &plain).code, 0);
    let run = sheet(&["--padding", "0", "--json", "array"], &named);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(fs::read(&plain).expect("a"), fs::read(&named).expect("b"));
    let text = |p: &std::path::Path| {
        fs::read_to_string(p.with_extension("json"))
            .expect("description")
            .replace("plain", "named")
    };
    assert_eq!(text(&plain), text(&named));
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_layout_that_does_not_fit_exits_3_and_writes_nothing() {
    let d = folder("sheet-too-wide");
    let run = sheet(&["--padding", "8192"], &d.join("s.png"));
    assert_eq!(run.code, 3, "{}", run.err);
    assert!(run.err.contains("columns and padding"), "{}", run.err);
    assert!(listing(&d).is_empty());
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn the_help_names_the_sheet_options() {
    let help = cli(&[&"help", &"export"]).out;
    for flag in ["--columns", "--padding", "--json hash|array"] {
        assert!(help.contains(flag), "{flag}");
    }
}
