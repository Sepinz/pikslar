//! `palette import` and `palette export`, run as a script would: what lands in the files, what
//! is refused, and that a refusal leaves every file as it was.

mod common;
use common::{cli, folder, listing};
use pikslar_core::{rgba, ColorMode, Document, DrawCmd, Editor, PaletteCmd, Tool};
use pikslar_format::palette_file::{self, PaletteFormat};
use pikslar_format::piks;
use std::fs;
use std::path::{Path, PathBuf};

const RED: u32 = rgba(255, 0, 0, 255);
const GREEN: u32 = rgba(0, 255, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const GOLD: u32 = rgba(255, 215, 0, 255);

/// A palette-mode project with swatches red, green, blue, and one pixel of each on a row.
fn project(dir: &Path, name: &str, swatches: &[u32]) -> PathBuf {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Palette).expect("size"));
    for &color in swatches {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    for (x, &color) in swatches.iter().enumerate() {
        e.color = color;
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(x as i32, 0)],
        })
        .expect("stroke");
    }
    let path = dir.join(name);
    fs::write(&path, piks::save(e.doc())).expect("write");
    path
}

fn load(path: &Path) -> Document {
    piks::load(&fs::read(path).expect("read")).expect("a project")
}

fn swatch_colours(doc: &Document) -> Vec<u32> {
    let table = &doc.colors;
    table.swatches().iter().map(|&id| table.color(id)).collect()
}

/// The first row as colours, and as ids.
fn row(doc: &Document) -> (Vec<u32>, Vec<u32>) {
    let cel = doc.layers()[0].cel(0);
    let ids: Vec<u32> = (0..3).map(|x| cel.get(x, 0)).collect();
    (ids.iter().map(|&i| doc.colors.color(i)).collect(), ids)
}

fn write_hex(dir: &Path, name: &str, colours: &[&str]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, colours.join("\n")).expect("write");
    path
}

#[test]
fn export_writes_the_swatches_in_order_in_each_format() {
    let d = folder("pal-export");
    let p = project(&d, "a.piks", &[RED, GREEN, BLUE]);
    let run = cli(&[&"palette", &"export", &p]);
    assert_eq!((run.code, run.err.as_str()), (0, ""), "{}", run.err);
    assert_eq!(run.out.trim(), d.join("a.gpl").display().to_string());
    let read = palette_file::read_any(&fs::read(d.join("a.gpl")).expect("file")).expect("a gpl");
    let rgb: Vec<[u8; 3]> = read
        .palette
        .colors
        .iter()
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    assert_eq!(rgb, [[255, 0, 0], [0, 255, 0], [0, 0, 255]]);
    for (args, name, format) in [
        (["--format", "hex"], "a.hex", PaletteFormat::Hex),
        (["--format", "pal"], "a.pal", PaletteFormat::Jasc),
    ] {
        let run = cli(&[&"palette", &"export", &p, &args[0], &args[1]]);
        assert_eq!(run.code, 0, "{}", run.err);
        let bytes = fs::read(d.join(name)).expect("file");
        assert_eq!(PaletteFormat::sniff(&bytes), format);
        let got = palette_file::read(&bytes, format).expect("reads back");
        assert_eq!(got.palette.colors.len(), 3);
    }
    let out = d.join("lospec.hex");
    let run = cli(&[&"palette", &"export", &p, &"-o", &out]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        fs::read_to_string(&out).expect("hex"),
        "ff0000\n00ff00\n0000ff\n"
    );
}

#[test]
fn export_does_not_replace_a_file_without_overwrite() {
    let d = folder("pal-export-exists");
    let p = project(&d, "a.piks", &[RED]);
    fs::write(d.join("a.gpl"), "mine").expect("write");
    let run = cli(&[&"palette", &"export", &p]);
    assert_eq!(run.code, 4, "{}", run.err);
    assert!(run.err.contains("--overwrite"));
    assert_eq!(fs::read_to_string(d.join("a.gpl")).expect("kept"), "mine");
    assert_eq!(cli(&[&"palette", &"export", &p, &"--overwrite"]).code, 0);
    assert!(fs::read_to_string(d.join("a.gpl"))
        .expect("new")
        .starts_with("GIMP Palette"));
}

#[test]
fn export_refuses_a_project_with_no_swatches_and_a_wrong_ending() {
    let d = folder("pal-export-refused");
    let empty = project(&d, "empty.piks", &[]);
    let run = cli(&[&"palette", &"export", &empty]);
    assert_eq!(run.code, 3, "{}", run.err);
    assert!(run.err.contains("no swatches"));
    let p = project(&d, "a.piks", &[RED]);
    let bad = d.join("a.txt");
    assert_eq!(cli(&[&"palette", &"export", &p, &"-o", &bad]).code, 1);
    let clash = d.join("a.gpl");
    assert_eq!(
        cli(&[
            &"palette",
            &"export",
            &p,
            &"--format",
            &"hex",
            &"-o",
            &clash
        ])
        .code,
        1
    );
    assert_eq!(listing(&d), ["a.piks", "empty.piks"]);
}

#[test]
fn append_joins_the_swatches_and_moves_no_pixel() {
    let d = folder("pal-append");
    let p = project(&d, "a.piks", &[RED, GREEN, BLUE]);
    let hex = write_hex(&d, "new.hex", &["ffd700", "00ff00"]);
    let run = cli(&[&"palette", &"import", &p, &hex]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.out.trim(), p.display().to_string());
    let doc = load(&p);
    assert_eq!(swatch_colours(&doc), [RED, GREEN, BLUE, GOLD]);
    assert_eq!(row(&doc), (vec![RED, GREEN, BLUE], vec![1, 2, 3]));
    assert_eq!(
        listing(&d),
        ["a.piks", "new.hex"],
        "no stray temporary file"
    );
}

#[test]
fn replace_keeps_every_pixel_and_says_what_is_no_longer_a_swatch() {
    let d = folder("pal-replace");
    let p = project(&d, "a.piks", &[RED, GREEN, BLUE]);
    let hex = write_hex(&d, "new.hex", &["ffd700", "0000ff"]);
    let run = cli(&[&"palette", &"import", &p, &hex, &"--how", &"replace"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("2 colour(s)"), "{}", run.err);
    let doc = load(&p);
    assert_eq!(swatch_colours(&doc), [GOLD, BLUE]);
    assert_eq!(row(&doc), (vec![RED, GREEN, BLUE], vec![1, 2, 3]));
}

#[test]
fn load_as_colours_recolours_the_swatches_and_keeps_every_id() {
    let d = folder("pal-colors");
    let p = project(&d, "a.piks", &[RED, GREEN, BLUE]);
    let hex = write_hex(&d, "swap.hex", &["ffd700", "ff0000", "00ff00"]);
    let run = cli(&[&"palette", &"import", &p, &hex, &"--how", &"colors"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.err.contains("changed"), "{}", run.err);
    let doc = load(&p);
    assert_eq!(row(&doc), (vec![GOLD, RED, GREEN], vec![1, 2, 3]));
    assert_eq!(swatch_colours(&doc), [GOLD, RED, GREEN]);
}

#[test]
fn load_as_colours_with_the_wrong_count_changes_nothing() {
    let d = folder("pal-count");
    let p = project(&d, "a.piks", &[RED, GREEN, BLUE]);
    let before = fs::read(&p).expect("read");
    let hex = write_hex(&d, "two.hex", &["ffd700", "ff0000"]);
    let run = cli(&[&"palette", &"import", &p, &hex, &"--how", &"colors"]);
    assert_eq!(run.code, 3, "{}", run.err);
    assert!(run.err.contains("has 3, the file has 2"), "{}", run.err);
    assert_eq!(fs::read(&p).expect("read"), before);
}

#[test]
fn out_writes_a_new_project_and_leaves_the_original() {
    let d = folder("pal-out");
    let p = project(&d, "a.piks", &[RED, GREEN, BLUE]);
    let before = fs::read(&p).expect("read");
    let hex = write_hex(&d, "swap.hex", &["ffd700", "ff0000", "00ff00"]);
    let out = d.join("b.piks");
    let args: [&dyn AsRef<std::ffi::OsStr>; 8] = [
        &"palette", &"import", &p, &hex, &"--how", &"colors", &"-o", &out,
    ];
    let run = cli(&args);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(fs::read(&p).expect("read"), before);
    assert_eq!(row(&load(&out)).0, [GOLD, RED, GREEN]);
    // It exists now: not replaced without --overwrite, and the project itself never is.
    assert_eq!(cli(&args).code, 4);
    assert_eq!(
        cli(&[&"palette", &"import", &p, &hex, &"-o", &p, &"--overwrite"]).code,
        4
    );
    assert_eq!(fs::read(&p).expect("read"), before);
    let mut again = args.to_vec();
    again.push(&"--overwrite");
    assert_eq!(cli(&again).code, 0);
}

#[test]
fn a_palette_that_already_matches_writes_nothing() {
    let d = folder("pal-same");
    let p = project(&d, "a.piks", &[RED, GREEN]);
    let hex = write_hex(&d, "same.hex", &["ff0000", "00ff00"]);
    let before = fs::read(&p).expect("read");
    let run = cli(&[&"palette", &"import", &p, &hex]);
    assert_eq!((run.code, run.out.as_str()), (0, ""), "{}", run.err);
    assert!(run.err.contains("nothing changed"));
    assert_eq!(fs::read(&p).expect("read"), before);
}

#[test]
fn a_palette_file_with_a_bad_line_is_refused_with_its_number() {
    let d = folder("pal-bad");
    let p = project(&d, "a.piks", &[RED]);
    let before = fs::read(&p).expect("read");
    let hex = write_hex(&d, "bad.hex", &["ff0000", "nope"]);
    let run = cli(&[&"palette", &"import", &p, &hex]);
    assert_eq!(run.code, 2, "{}", run.err);
    assert!(run.err.contains("Line 2"), "{}", run.err);
    assert_eq!(fs::read(&p).expect("read"), before);
    let missing = d.join("missing.gpl");
    assert_eq!(cli(&[&"palette", &"import", &p, &missing]).code, 2);
    assert_eq!(listing(&d), ["a.piks", "bad.hex"]);
}

#[test]
fn a_file_with_no_ending_is_read_by_its_first_line() {
    let d = folder("pal-sniff");
    let p = project(&d, "a.piks", &[RED]);
    let file = d.join("palette");
    fs::write(&file, "JASC-PAL\r\n0100\r\n1\r\n255 215 0\r\n").expect("write");
    assert_eq!(cli(&[&"palette", &"import", &p, &file]).code, 0);
    assert_eq!(swatch_colours(&load(&p)), [RED, GOLD]);
}

#[test]
fn arguments_that_make_no_sense_exit_1() {
    let d = folder("pal-args");
    let p = project(&d, "a.piks", &[RED]);
    let hex = write_hex(&d, "a.hex", &["ff0000"]);
    let cases: Vec<Vec<&dyn AsRef<std::ffi::OsStr>>> = vec![
        vec![&"palette"],
        vec![&"palette", &"bogus"],
        vec![&"palette", &"export"],
        vec![&"palette", &"import", &p],
        vec![&"palette", &"import", &p, &hex, &"--how", &"merge"],
        vec![&"palette", &"import", &p, &hex, &"--how"],
        vec![&"palette", &"export", &p, &"--how", &"append"],
        vec![&"palette", &"export", &p, &"--format", &"png"],
        vec![&"palette", &"import", &p, &hex, &"--bogus"],
    ];
    for args in cases {
        let run = cli(&args);
        assert_eq!(run.code, 1, "{}", run.err);
        assert!(run.err.contains("See: pikslar-cli help"), "{}", run.err);
    }
}

#[test]
fn help_names_the_ways_and_the_exit_codes() {
    for args in [["help", "palette"], ["palette", "--help"]] {
        let run = cli(&[&args[0], &args[1]]);
        assert_eq!(run.code, 0);
        for word in ["--how", "append", "replace", "colors", "--overwrite"] {
            assert!(run.out.contains(word), "{word}");
        }
    }
    assert!(cli(&[]).out.contains("palette import"));
}

/// Writing a version 1 or 2 project back in place would make it version 3 with no copy of the
/// original, and older versions could not open it: that needs `--out`.
#[test]
fn import_does_not_write_over_an_older_version_project_in_place() {
    let d = folder("pal-older");
    let p = d.join("old.piks");
    fs::copy(common::corpus("v2-palette.piks"), &p).expect("copy");
    let before = fs::read(&p).expect("read");
    let hex = write_hex(&d, "add.hex", &["ffd700"]);
    let run = cli(&[&"palette", &"import", &p, &hex]);
    assert_eq!(run.code, 3, "{}", run.err);
    assert!(
        run.err.contains("older version") && run.err.contains("--out"),
        "{}",
        run.err
    );
    assert_eq!(fs::read(&p).expect("read"), before);
    assert_eq!(listing(&d), ["add.hex", "old.piks"]);
    let out = d.join("new.piks");
    let run = cli(&[&"palette", &"import", &p, &hex, &"--out", &out]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(fs::read(&p).expect("read"), before);
    assert!(swatch_colours(&load(&out)).contains(&GOLD));
}
