//! Files that must not be replaced, projects that cannot be read, places that cannot be written.

mod common;
use common::{cli, corpus, folder, listing};
use std::fs;
use std::path::Path;

fn sheet(project: &Path, out: &Path, overwrite: bool) -> common::Run {
    if overwrite {
        cli(&[
            &"export",
            &project,
            &"--format",
            &"sheet",
            &"-o",
            &out,
            &"--overwrite",
        ])
    } else {
        cli(&[&"export", &project, &"--format", &"sheet", &"-o", &out])
    }
}

#[test]
fn nothing_is_replaced_without_overwrite_not_even_half_a_sheet() {
    let d = folder("keep");
    let p = corpus("v3-animated.piks");
    let (png, json) = (d.join("walk.png"), d.join("walk.json"));
    let earlier = "{\"meta\":{\"app\":\"Pikslar\"}}";
    fs::write(&json, earlier).expect("an earlier export's description");
    let run = sheet(&p, &png, false);
    assert_eq!(run.code, 4, "{}", run.err);
    assert!(run.err.contains("walk.json already exists") && run.err.contains("--overwrite"));
    assert_eq!(
        listing(&d),
        ["walk.json"],
        "the image was not written either"
    );
    assert_eq!(fs::read_to_string(&json).expect("json"), earlier);
    let run = sheet(&p, &png, true);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_ne!(fs::read_to_string(&json).expect("json"), earlier);
    assert_eq!(listing(&d), ["walk.json", "walk.png"]);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn projects_that_cannot_be_read_exit_2_and_write_nothing() {
    let d = folder("bad");
    let good = fs::read(corpus("v3-animated.piks")).expect("read");
    let mut flipped = good.clone();
    let middle = flipped.len() / 2;
    flipped[middle] ^= 0xff;
    let cases: [(&str, &[u8]); 5] = [
        ("empty.piks", b""),
        ("text.piks", b"not a project"),
        ("half.piks", &good[..good.len() / 2]),
        ("flipped.piks", &flipped),
        ("header.piks", &good[..8]),
    ];
    let out = d.join("out");
    fs::create_dir(&out).expect("output folder");
    for (name, bytes) in cases {
        let project = d.join(name);
        fs::write(&project, bytes).expect("write");
        for format in ["png", "sheet", "sequence"] {
            let a = out.join("a.png");
            let run = cli(&[&"export", &project, &"--format", &format, &"-o", &a]);
            assert_eq!(run.code, 2, "{name} {format}: {}", run.err);
            assert!(run.err.contains(name), "{name}: {}", run.err);
        }
        assert_eq!(cli(&[&"info", &project]).code, 2, "{name}");
    }
    for missing in [d.join("missing.piks"), d.clone()] {
        assert_eq!(
            cli(&[&"export", &missing, &"-o", &out.join("a.png")]).code,
            2
        );
    }
    assert!(listing(&out).is_empty());
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn places_that_cannot_be_written_exit_4_and_change_nothing() {
    let d = folder("places");
    let project = d.join("art.png");
    fs::copy(corpus("v3-animated.piks"), &project).expect("a project with an image's name");
    let before = fs::read(&project).expect("read");
    fs::create_dir(d.join("folder.png")).expect("a folder with an image's name");
    let locked = d.join("locked.png");
    fs::write(&locked, "keep").expect("write");
    let mut perms = fs::metadata(&locked).expect("meta").permissions();
    perms.set_readonly(true);
    fs::set_permissions(&locked, perms.clone()).expect("read-only");
    let targets = [
        (d.join("missing").join("a.png"), "does not exist"),
        (d.join("folder.png"), "is a folder"),
        (project.clone(), "is the project being exported"),
        (locked.clone(), "locked.png is read-only"),
        (d.join("CON.png"), "keeps for a device"),
        (d.join("nul.png"), "keeps for a device"),
        (d.join("a.png."), "ends in a dot or a space"),
        (d.join("a.png "), "ends in a dot or a space"),
    ];
    for (target, says) in &targets {
        let run = cli(&[&"export", &project, &"-o", target, &"--overwrite"]);
        assert_eq!(run.code, 4, "{}: {}", target.display(), run.err);
        assert!(run.err.contains(says), "{}: {}", target.display(), run.err);
        assert!(run.out.is_empty());
    }
    assert_eq!(
        fs::read(&project).expect("read"),
        before,
        "the project is untouched"
    );
    assert_eq!(fs::read_to_string(&locked).expect("read"), "keep");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&locked, perms).expect("writable");
    assert_eq!(listing(&d), ["art.png", "folder.png", "locked.png"]);
    fs::remove_dir_all(d).expect("cleanup");
}
