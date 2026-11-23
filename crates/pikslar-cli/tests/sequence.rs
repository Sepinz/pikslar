//! PNG sequences: the app's pixels in every file, predictable names, a description written last,
//! and nothing replaced that should not be.

mod common;
use common::{cli, corpus, folder, listing, project};
use pikslar_core::Document;
use pikslar_format::piks;
use std::fs;
use std::path::Path;

fn load(path: &Path) -> Document {
    piks::load(&fs::read(path).expect("read")).expect("loads")
}

fn sequence(project: &Path, out: &Path, extra: &[&str]) -> common::Run {
    let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> =
        vec![&"export", &project, &"--format", &"sequence", &"-o", &out];
    args.extend(extra.iter().map(|a| a as &dyn AsRef<std::ffi::OsStr>));
    cli(&args)
}

/// Every file of a sequence of `doc`'s `frames`, as the app would write each frame.
fn assert_is_sequence_of(d: &Path, doc: &Document, frames: std::ops::RangeInclusive<usize>) {
    for (position, frame) in frames.clone().enumerate() {
        let name = format!("walk_{:04}.png", position + 1);
        let bytes = pikslar_export::frame_png(doc, frame).expect("app bytes");
        assert_eq!(fs::read(d.join(&name)).expect("member"), bytes, "{name}");
    }
    let json = fs::read_to_string(d.join("walk.json")).expect("description");
    assert_eq!(json.matches("\"filename\"").count(), frames.count());
}

#[test]
fn each_frame_is_its_own_file_with_the_apps_bytes() {
    for name in ["v3-animated.piks", "v3-offsets.piks", "v2-palette.piks"] {
        let d = folder(&format!("seq-{name}"));
        let doc = load(&corpus(name));
        let out = d.join("walk.png");
        let run = sequence(&corpus(name), &out, &[]);
        assert_eq!(run.code, 0, "{name}: {}", run.err);
        let n = doc.frames.len();
        assert_eq!(run.out.lines().count(), n + 1);
        assert!(
            run.out.trim_end().ends_with("walk.json"),
            "the description is last"
        );
        assert_is_sequence_of(&d, &doc, 0..=n - 1);
        let mut expected: Vec<String> = (1..=n).map(|i| format!("walk_{i:04}.png")).collect();
        expected.push("walk.json".into());
        expected.sort();
        assert_eq!(listing(&d), expected, "{name}");
        fs::remove_dir_all(d).expect("cleanup");
    }
}

#[test]
fn a_chosen_range_is_numbered_from_one_and_names_its_frames() {
    let d = folder("seq-range");
    let p = project(&d.join("p.piks"), 5, 6, 1);
    let run = sequence(&p, &d.join("walk.png"), &["--frames", "3-5"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_is_sequence_of(&d, &load(&p), 2..=4);
    let json = fs::read_to_string(d.join("walk.json")).expect("description");
    assert!(json.starts_with("{\"frames\":[{\"filename\":\"walk_0001.png\",\"frame\":2,"));
    assert!(json.contains("\"firstFrame\":2"));
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn the_same_export_twice_gives_the_same_bytes_and_needs_overwrite() {
    let d = folder("seq-again");
    let p = project(&d.join("p.piks"), 7, 4, 2);
    let out = d.join("walk.png");
    assert_eq!(sequence(&p, &out, &[]).code, 0);
    let before: Vec<Vec<u8>> = listing(&d)
        .iter()
        .map(|n| fs::read(d.join(n)).expect("r"))
        .collect();
    let refused = sequence(&p, &out, &[]);
    assert_eq!(refused.code, 4);
    assert!(refused.err.contains("--overwrite"), "{}", refused.err);
    assert_eq!(sequence(&p, &out, &["--overwrite"]).code, 0);
    let after: Vec<Vec<u8>> = listing(&d)
        .iter()
        .map(|n| fs::read(d.join(n)).expect("r"))
        .collect();
    assert_eq!(before, after);
    fs::remove_dir_all(d).expect("cleanup");
}

fn snapshot(d: &Path) -> Vec<(String, Vec<u8>)> {
    listing(d)
        .into_iter()
        .map(|n| {
            let bytes = fs::read(d.join(&n)).unwrap_or_default();
            (n, bytes)
        })
        .collect()
}

#[test]
fn files_left_from_a_longer_sequence_stop_a_shorter_one() {
    let d = folder("seq-stale");
    let p = project(&d.join("p.piks"), 4, 5, 3);
    let out = d.join("walk.png");
    assert_eq!(sequence(&p, &out, &[]).code, 0);
    let before = snapshot(&d);
    let run = sequence(&p, &out, &["--frames", "1-2", "--overwrite"]);
    assert_eq!(run.code, 4);
    assert!(
        run.err.contains("walk_0003.png") && run.err.contains("2 other"),
        "{}",
        run.err
    );
    assert_eq!(snapshot(&d), before, "nothing changed");
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_description_someone_else_wrote_stops_the_export_even_with_overwrite() {
    let d = folder("seq-foreign");
    let p = project(&d.join("p.piks"), 4, 3, 4);
    fs::write(d.join("walk.json"), "{\"mine\":true}").expect("theirs");
    let before = snapshot(&d);
    let run = sequence(&p, &d.join("walk.png"), &["--overwrite"]);
    assert_eq!(run.code, 4);
    assert!(run.err.contains("not written by Pikslar"), "{}", run.err);
    assert_eq!(snapshot(&d), before);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_folder_at_any_member_stops_everything() {
    let d = folder("seq-blocked");
    let p = project(&d.join("p.piks"), 4, 3, 5);
    let out = d.join("walk.png");
    let at = d.join("walk_0002.png");
    fs::create_dir(&at).expect("folder");
    let before = snapshot(&d);
    let run = sequence(&p, &out, &["--overwrite"]);
    assert_eq!(run.code, 4);
    assert!(run.err.contains("walk_0002.png"), "{}", run.err);
    assert_eq!(snapshot(&d), before);
    fs::remove_dir(&at).expect("unblock");
    assert_eq!(
        sequence(&p, &out, &[]).code,
        0,
        "with the way clear it works"
    );
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn files_at_the_old_temporary_names_are_kept_and_stop_nothing() {
    let d = folder("seq-old-temps");
    let p = project(&d.join("p.piks"), 4, 3, 5);
    let out = d.join("walk.png");
    let theirs = ["walk_0003.png.pikslar-tmp", "walk.json.pikslar-tmp"];
    for name in theirs {
        fs::write(d.join(name), b"not this export's").expect("someone's file");
    }
    let run = sequence(&p, &out, &["--overwrite"]);
    assert_eq!(run.code, 0, "{}", run.err);
    for name in theirs {
        assert_eq!(fs::read(d.join(name)).expect("kept"), b"not this export's");
    }
    let names: Vec<String> = snapshot(&d).into_iter().map(|(n, _)| n).collect();
    assert_eq!(
        names,
        [
            "p.piks",
            "walk.json",
            "walk.json.pikslar-tmp",
            "walk_0001.png",
            "walk_0002.png",
            "walk_0003.png",
            "walk_0003.png.pikslar-tmp",
        ]
    );
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_bad_name_or_a_missing_folder_writes_nothing() {
    let d = folder("seq-paths");
    let p = project(&d.join("p.piks"), 4, 2, 6);
    let long = format!("{}.png", "a".repeat(240));
    for (out, code) in [
        (d.join("missing").join("walk.png"), 4),
        (d.join("con.png"), 4),
        (d.join(long), 4),
        (d.join("walk.gif"), 1),
    ] {
        let run = sequence(&p, &out, &[]);
        assert_eq!(run.code, code, "{}: {}", out.display(), run.err);
        assert_eq!(listing(&d), ["p.piks"]);
    }
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn thousands_of_frames_export_as_thousands_of_files() {
    let d = folder("seq-many");
    let p = project(&d.join("p.piks"), 2, 4096, 7);
    let out = d.join("many.png");
    let run = sequence(&p, &out, &[]);
    assert_eq!(run.code, 0, "{}", run.err);
    let names = listing(&d);
    assert_eq!(names.len(), 4096 + 2);
    assert_eq!(names[0], "many.json");
    assert_eq!(names[1], "many_0001.png");
    assert_eq!(names[4096], "many_4096.png");
    let doc = load(&p);
    for frame in [0, 1, 2047, 4095] {
        let name = format!("many_{:04}.png", frame + 1);
        let bytes = pikslar_export::frame_png(&doc, frame).expect("app");
        assert_eq!(fs::read(d.join(name)).expect("member"), bytes);
    }
    let json = fs::read_to_string(d.join("many.json")).expect("description");
    assert_eq!(json.matches("\"filename\"").count(), 4096);
    fs::remove_dir_all(d).expect("cleanup");
}
