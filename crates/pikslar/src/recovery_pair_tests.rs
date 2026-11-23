//! The copy and its sidecar are two files written one after the other. Whichever was written
//! last, a pair from two different writes is noticed and the origin is not used.

use super::*;
use pikslar_core::ColorMode;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-pair-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn doc(w: usize) -> Document {
    Document::new(w, 8, ColorMode::Palette).expect("valid")
}

fn the(dir: &Path, extension: &str) -> PathBuf {
    fs::read_dir(dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == extension))
        .expect("the file")
}

/// A session that wrote `doc` for `origin`, protecting `protected`, and then crashed.
fn crashed(dir: &Path, doc: &Document, origin: &Path, protected: &Path) {
    let mut first = Recovery::in_dir(dir);
    let origin = Origin {
        path: Some(origin),
        protected: Some(protected),
    };
    first.write_now(doc, None, origin).expect("write");
    first.simulate_crash();
}

#[test]
fn a_copy_written_after_its_sidecar_is_not_given_the_older_origin() {
    let dir = temp_dir("copy-later");
    let (hero, kept) = (
        PathBuf::from("C:/art/hero.piks"),
        PathBuf::from("C:/art/old.ase"),
    );
    crashed(&dir, &doc(8), &hero, &kept);
    // The process stopped after the next copy and before its sidecar.
    let later = piks::try_save_recovery(&doc(16), None).expect("encodes");
    fs::write(the(&dir, "piks"), later).expect("the later copy");
    let orphan = Recovery::in_dir(&dir).find_orphan().expect("found");
    assert_eq!(orphan.origin, None, "the origin belongs to another copy");
    assert_eq!(orphan.stamp, None);
    assert_eq!(orphan.protected, Some(kept), "still never saved over");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_sidecar_written_before_its_copy_is_not_used_for_the_older_copy() {
    let dir = temp_dir("sidecar-first");
    let hero = PathBuf::from("C:/art/hero.piks");
    crashed(&dir, &doc(8), &hero, &hero);
    let older = fs::read(the(&dir, "piks")).expect("the copy");
    let other = Path::new("C:/art/villain.piks");
    // The next write's sidecar went down for another file, and its copy never followed.
    let text = paired_text(&origin_text(Origin::file(other), None), b"never written");
    fs::write(the(&dir, "origin"), text).expect("the next sidecar");
    assert_eq!(fs::read(the(&dir, "piks")).expect("still"), older);
    let orphan = Recovery::in_dir(&dir).find_orphan().expect("found");
    assert_eq!(orphan.origin, None, "villain.piks is not this work's");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_matching_pair_keeps_its_origin_through_a_restore_and_a_second_crash() {
    let dir = temp_dir("matching");
    let hero = PathBuf::from("C:/art/hero.piks");
    crashed(&dir, &doc(8), &hero, &hero);
    let mut second = Recovery::in_dir(&dir);
    let orphan = second.find_orphan().expect("found");
    assert_eq!(orphan.origin.as_deref(), Some(hero.as_path()));
    second.adopt(&orphan).expect("adopted");
    second.simulate_crash();
    let again = Recovery::in_dir(&dir).find_orphan().expect("found again");
    assert_eq!(again.origin, Some(hero), "the adopted copy is paired too");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_sidecar_from_before_the_print_is_still_read() {
    let dir = temp_dir("older");
    let hero = PathBuf::from("C:/art/hero.piks");
    crashed(&dir, &doc(8), &hero, &hero);
    fs::write(the(&dir, "origin"), "C:/art/hero.piks\n\n").expect("an older sidecar");
    let orphan = Recovery::in_dir(&dir).find_orphan().expect("found");
    assert_eq!(orphan.origin, Some(hero));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_copy_with_no_origin_is_paired_as_well() {
    let text = paired_text(&origin_text(Origin::NONE, None), b"copy");
    let side = parse_sidecar(&text);
    assert_eq!(side.origin, None);
    assert_eq!(side.print, Some(pikslar_rewind::hash128(b"copy")));
}
