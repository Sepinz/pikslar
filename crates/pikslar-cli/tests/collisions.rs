//! Names that collide with something already there: the temporary files, the project through
//! another name, a description someone else wrote, and names Windows would change.

mod common;
use common::{cli, corpus, folder, listing};
use std::fs;
use std::path::Path;

fn export(project: &Path, out: &Path, extra: &[&str]) -> common::Run {
    let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> = vec![&"export", &project, &"-o", &out];
    args.extend(extra.iter().map(|a| a as &dyn AsRef<std::ffi::OsStr>));
    cli(&args)
}

/// Each export run makes its temporary files new, under names with its own tag, so a file at
/// the name earlier versions used, whatever it is, is never written, removed or in the way.
#[test]
fn a_project_with_the_old_temporary_name_exports_and_is_kept() {
    let d = folder("temp-project");
    let project = d.join("p.png.pikslar-tmp");
    fs::copy(corpus("v3-animated.piks"), &project).expect("copy");
    let before = fs::read(&project).expect("read");
    let run = export(&project, &d.join("p.png"), &["--overwrite"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(fs::read(&project).expect("read"), before);
    assert_eq!(listing(&d), ["p.png", "p.png.pikslar-tmp"]);
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_project_linked_at_the_old_temporary_name_keeps_its_bytes() {
    let d = folder("temp-link");
    let project = d.join("art.piks");
    fs::copy(corpus("v3-animated.piks"), &project).expect("copy");
    let before = fs::read(&project).expect("read");
    fs::hard_link(&project, d.join("out.png.pikslar-tmp")).expect("link");
    let run = export(&project, &d.join("out.png"), &["--overwrite"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(fs::read(&project).expect("read"), before);
    assert_eq!(
        fs::read(d.join("out.png.pikslar-tmp")).expect("read"),
        before
    );
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_file_at_either_old_temporary_name_of_a_sheet_is_kept() {
    for temp in ["s.png.pikslar-tmp", "s.json.pikslar-tmp"] {
        let d = folder("temp-sheet");
        fs::write(d.join(temp), "precious").expect("write");
        let run = export(
            &corpus("v3-animated.piks"),
            &d.join("s.png"),
            &["--format", "sheet", "--overwrite"],
        );
        assert_eq!(run.code, 0, "{temp}: {}", run.err);
        assert_eq!(fs::read_to_string(d.join(temp)).expect("read"), "precious");
        let mut want = vec!["s.json", "s.png", temp];
        want.sort();
        assert_eq!(listing(&d), want);
        fs::remove_dir_all(d).expect("cleanup");
    }
}

#[test]
fn a_description_someone_else_wrote_is_kept_even_with_overwrite() {
    let d = folder("foreign-json");
    let json = d.join("hero.json");
    fs::write(&json, "{\"mine\":true}").expect("write");
    let project = corpus("v3-animated.piks");
    let out = d.join("hero.png");
    let run = export(&project, &out, &["--format", "sheet", "--overwrite"]);
    assert_eq!(run.code, 4, "{}", run.err);
    assert!(run.err.contains("not written by Pikslar"), "{}", run.err);
    assert_eq!(fs::read_to_string(&json).expect("read"), "{\"mine\":true}");
    assert_eq!(
        listing(&d),
        ["hero.json"],
        "the image is not written either"
    );
    fs::remove_file(&json).expect("remove");
    let first = export(&project, &out, &["--format", "sheet"]);
    assert_eq!(first.code, 0, "{}", first.err);
    let again = export(&project, &out, &["--format", "sheet", "--overwrite"]);
    assert_eq!(
        again.code, 0,
        "an earlier export's description is replaced: {}",
        again.err
    );
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn names_windows_would_refuse_or_change_write_nothing() {
    let d = folder("odd-names");
    fs::create_dir(d.join("sub")).expect("folder");
    let long = format!("{}.png", "n".repeat(231));
    let cases = [
        (d.join("ab:c.png"), "does not allow"),
        (d.join("z.png:s.png"), "does not allow"),
        (d.join("sub.").join("t.png"), "folder name \"sub.\""),
        (d.join("sub ").join("t.png"), "folder name \"sub \""),
        (d.join(&long), "too long"),
    ];
    for (target, says) in &cases {
        let run = export(&corpus("v3-animated.piks"), target, &[]);
        assert_eq!(run.code, 4, "{}: {}", target.display(), run.err);
        assert!(run.err.contains(says), "{}: {}", target.display(), run.err);
    }
    assert_eq!(listing(&d), ["sub"]);
    assert!(listing(&d.join("sub")).is_empty());
    let fits = d.join(format!("{}.png", "n".repeat(230)));
    assert_eq!(export(&corpus("v3-animated.piks"), &fits, &[]).code, 0);
    fs::remove_dir_all(d).expect("cleanup");
}

/// Needs the right to make links: on Windows, administrator or Developer Mode. Without it the
/// test cannot set up its case and says so rather than failing.
#[test]
fn a_link_at_a_target_is_refused_and_what_it_points_to_is_kept() {
    let d = folder("link");
    let kept = d.join("kept.png");
    fs::write(&kept, b"someone's file").expect("write");
    let link = d.join("walk.png");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&kept, &link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&kept, &link);
    if made.is_err() {
        eprintln!("skipped: links cannot be made here");
        fs::remove_dir_all(d).expect("cleanup");
        return;
    }
    let run = export(&corpus("v3-animated.piks"), &link, &["--overwrite"]);
    assert_eq!(run.code, 4, "{}", run.err);
    assert!(run.err.contains("is a link"), "{}", run.err);
    assert_eq!(fs::read(&kept).expect("kept"), b"someone's file");
    assert_eq!(listing(&d), ["kept.png", "walk.png"]);
    fs::remove_dir_all(d).expect("cleanup");
}
