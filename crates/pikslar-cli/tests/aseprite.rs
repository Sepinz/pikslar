//! Aseprite files as input: read through the importer, never written, warnings on stderr.

mod common;
use common::{cli, folder, listing};
use pikslar_core::rgba;
use pikslar_format::png;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "../../pikslar-format/src/ase_build.rs"]
mod ase_build;
use ase_build::*;

fn clean() -> Vec<u8> {
    let pic = rgba_picture(2, 2, 10);
    Ase::new(4, 3, 32)
        .frame(100, vec![plain("ink"), cel_raw(0, 1, 1, 2, 2, &pic)])
        .bytes()
}

fn skewed() -> Vec<u8> {
    let mut ase = Ase::new(4, 3, 32);
    ase.ratio = (2, 1);
    ase.frame(100, vec![plain("ink")]).bytes()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).expect("write");
    path
}

#[test]
fn an_aseprite_file_exports_to_png_with_its_pixels() {
    let d = folder("ase-export");
    for (name, ending) in [("a.ase", "ase"), ("b.ASEPRITE", "aseprite")] {
        let input = write(&d, name, &clean());
        let out = d.join(format!("{ending}.png"));
        let run = cli(&[&"export", &input, &"--out", &out]);
        assert_eq!((run.code, run.err.as_str()), (0, ""), "{name}");
        let image = png::decode(&fs::read(&out).expect("written")).expect("a png");
        assert_eq!((image.width, image.height), (4, 3));
        let at = |x: usize, y: usize| image.px[y * 4 + x];
        assert_eq!(at(1, 1), rgba(10, 0, 0, 255));
        assert_eq!(at(0, 0), 0);
        assert_eq!(fs::read(&input).expect("kept"), clean(), "never written");
    }
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn the_default_output_is_a_png_beside_the_aseprite_file() {
    let d = folder("ase-default");
    let input = write(&d, "hero.ase", &clean());
    let run = cli(&[&"export", &input]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(listing(&d), ["hero.ase", "hero.png"]);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn info_describes_an_aseprite_file() {
    let d = folder("ase-info");
    let input = write(&d, "a.ase", &clean());
    let json = cli(&[&"info", &input, &"--json"]);
    assert_eq!(json.code, 0, "{}", json.err);
    assert!(
        json.out.starts_with("{\"width\":4,\"height\":3,"),
        "{}",
        json.out
    );
    assert!(json.out.contains("\"name\":\"ink\""), "{}", json.out);
    assert_eq!(json.out.lines().count(), 1);
    let text = cli(&[&"info", &input]);
    assert_eq!(text.code, 0);
    assert!(text.out.contains("ink"), "{}", text.out);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn what_was_left_out_is_a_warning_on_stderr_and_the_exit_is_zero() {
    let d = folder("ase-warn");
    let input = write(&d, "w.ase", &skewed());
    let out = d.join("w.png");
    let run = cli(&[&"export", &input, &"--out", &out]);
    assert_eq!(run.code, 0);
    assert_eq!(
        run.err,
        "pikslar-cli: warning: Pixels are 2:1, shown as square\n"
    );
    assert!(!run.out.contains("warning"), "{}", run.out);
    let info = cli(&[&"info", &input, &"--json"]);
    assert_eq!(info.code, 0);
    assert!(info.err.contains("warning: Pixels are 2:1"), "{}", info.err);
    assert!(!info.out.contains("warning"), "{}", info.out);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_damaged_aseprite_file_is_a_read_error_that_names_the_file() {
    let d = folder("ase-corrupt");
    let mut bytes = clean();
    bytes.truncate(70);
    let input = write(&d, "bad.ase", &bytes);
    for args in [vec!["export"], vec!["info"]] {
        let mut all: Vec<&dyn AsRef<std::ffi::OsStr>> = args.iter().map(|a| a as _).collect();
        all.push(&input);
        let run = cli(&all);
        assert_eq!(run.code, 2, "{}", run.err);
        assert!(run.err.contains("bad.ase"), "{}", run.err);
        assert!(run.err.starts_with("pikslar-cli: "), "{}", run.err);
    }
    assert_eq!(listing(&d), ["bad.ase"], "nothing written");
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn nothing_ever_names_an_aseprite_file_as_output() {
    let d = folder("ase-never-written");
    let input = write(&d, "a.ase", &clean());
    let run = cli(&[&"export", &input, &"--out", &input]);
    assert_eq!(run.code, 1, "{}", run.err);
    assert_eq!(fs::read(&input).expect("kept"), clean());
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn the_help_says_aseprite_files_are_read_and_never_written() {
    for args in [vec!["help"], vec!["help", "info"], vec!["help", "export"]] {
        let all: Vec<&dyn AsRef<std::ffi::OsStr>> = args.iter().map(|a| a as _).collect();
        let help = cli(&all).out;
        assert!(help.contains(".ase"), "{args:?}");
        assert!(help.contains("never writes"), "{args:?}");
    }
}
