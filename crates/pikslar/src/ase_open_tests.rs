//! Opening Aseprite files: a new project that is not saved anywhere, whatever the file's name.

use crate::app::App;
use crate::files;
use crate::session::{with_required_extension, OPEN_FILTER};
use pikslar_core::rgba;
use std::path::PathBuf;

#[path = "../../pikslar-format/src/ase_build.rs"]
pub(crate) mod ase_build;
use ase_build::*;

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-ase-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("folder");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("written");
    path
}

fn clean() -> Vec<u8> {
    let pic = rgba_picture(2, 2, 10);
    Ase::new(4, 3, 32)
        .frame(100, vec![plain("ink"), cel_raw(0, 1, 1, 2, 2, &pic)])
        .bytes()
}

fn with_warning() -> Vec<u8> {
    let mut ase = Ase::new(4, 3, 32);
    ase.ratio = (2, 1);
    ase.frame(100, vec![plain("ink")]).bytes()
}

#[test]
fn an_aseprite_file_opens_as_a_document_with_nowhere_to_save() {
    for name in ["a.ase", "b.ASE", "c.aseprite", "d.AsePrite"] {
        let path = scratch(name, &clean());
        let opened = files::open(&path).expect(name);
        assert_eq!((opened.doc.width, opened.doc.height), (4, 3), "{name}");
        assert_eq!(opened.doc.layers()[0].name, "ink");
        assert_eq!(opened.doc.layers()[0].cel(0).get(1, 1), rgba(10, 0, 0, 255));
        assert!(opened.warnings.is_empty());
        assert_eq!(files::save_path_for(path.clone(), &opened.notes), None);
        assert_eq!(files::protected_path(&path, &opened.notes), None);
    }
}

#[test]
fn launching_with_an_aseprite_file_is_an_unsaved_untitled_project() {
    let path = scratch("hero.ase", &clean());
    let app = App::launch(Some(path), None, None);
    assert_eq!(app.path, None);
    assert_eq!(app.document_label(), "Untitled *");
    assert!(!app.first_use, "this is not an empty first session");
    assert_eq!(app.editor.doc().layers()[0].name, "ink");
    assert_eq!(app.message, None, "nothing was left out");
}

#[test]
fn what_was_left_out_is_told_once_in_one_line() {
    let path = scratch("ratio.aseprite", &with_warning());
    let app = App::launch(Some(path), None, None);
    let (text, _) = app.message.as_ref().expect("a notice");
    assert!(text.starts_with("Opened from Aseprite: "), "{text}");
    assert!(text.contains("2:1"), "{text}");
    assert!(!text.contains('\n'));
}

#[test]
fn several_warnings_are_counted() {
    let opened = files::open(&scratch("w.ase", &with_warning())).expect("opens");
    let many = vec![opened.warnings[0].clone(); 3];
    assert_eq!(
        files::aseprite_notice(&many).as_deref(),
        Some("Opened from Aseprite: 3 things were left out or changed")
    );
    assert_eq!(files::aseprite_notice(&[]), None);
}

#[test]
fn saving_never_picks_an_aseprite_name() {
    let ase = PathBuf::from("art/hero.ase");
    assert_eq!(
        with_required_extension(ase, "piks"),
        PathBuf::from("art/hero.piks")
    );
    let upper = PathBuf::from("art/hero.ASEPRITE");
    assert_eq!(
        with_required_extension(upper, "piks"),
        PathBuf::from("art/hero.piks")
    );
}

#[test]
fn the_open_dialog_offers_aseprite_files() {
    assert!(OPEN_FILTER.contains("*.piks;*.png;*.ase;*.aseprite\0"));
    assert!(OPEN_FILTER.contains("Aseprite files\0*.ase;*.aseprite\0"));
    assert!(OPEN_FILTER.ends_with('\0'));
}

#[test]
fn a_damaged_aseprite_file_says_so_and_leaves_a_fresh_document() {
    let mut bytes = clean();
    bytes.truncate(60);
    let path = scratch("bad.ase", &bytes);
    assert!(files::open(&path).is_err());
    let app = App::launch(Some(path), Some(8), None);
    assert_eq!(app.path, None);
    assert!(app.message.is_some());
}

#[test]
fn an_import_reads_as_unsaved_so_closing_it_asks_first() {
    let path = scratch("asks.ase", &clean());
    let app = App::launch(Some(path), None, None);
    assert!(app.is_modified(), "nothing holds it as a project yet");
    assert_eq!(app.document_label(), "Untitled *");
    let window = pikslar_platform::Window::detached();
    let mut opened = App::launch(None, Some(8), None);
    let path = scratch("asks-too.aseprite", &clean());
    assert_eq!(opened.open_now(window, path), crate::session::Opened::Now);
    assert!(opened.has_unsaved_work());
    assert!(
        !opened.confirm_discard(window),
        "the question is asked, and a window that cannot answer keeps the work"
    );
}

#[test]
fn a_project_opened_after_an_import_is_not_marked_changed() {
    let window = pikslar_platform::Window::detached();
    let mut app = App::launch(Some(scratch("first.ase", &clean())), None, None);
    let project = scratch("after.piks", &[]);
    files::save(&project, app.editor.doc()).expect("saved");
    app.saved_revision = app.editor.revision();
    assert_eq!(app.open_now(window, project), crate::session::Opened::Now);
    assert!(!app.is_modified());
}

#[test]
fn save_suggests_a_project_beside_the_imported_file() {
    for name in ["walk.aseprite", "walk.ase", "walk.ASE"] {
        let path = scratch(name, &clean());
        let app = App::launch(Some(path.clone()), None, None);
        let suggested = app.save_start().expect("a name to start from");
        assert_eq!(
            suggested.file_name().and_then(|n| n.to_str()),
            Some("walk.piks")
        );
        assert_eq!(
            suggested.parent(),
            std::path::absolute(&path).expect("whole").parent(),
            "{name}: in the same folder"
        );
    }
    let window = pikslar_platform::Window::detached();
    let mut app = App::launch(None, Some(8), None);
    assert_eq!(app.save_start(), None, "a new sprite has no name yet");
    app.open_now(window, scratch("run.ase", &clean()));
    let suggested = app.save_start().expect("suggested");
    assert_eq!(
        suggested.file_name().and_then(|n| n.to_str()),
        Some("run.piks")
    );
}

#[test]
fn a_typed_aseprite_name_becomes_a_project_name_without_a_double_extension() {
    let name = |p: &str| crate::session::project_name(PathBuf::from(p));
    assert_eq!(name("art/from-ase.ase"), PathBuf::from("art/from-ase.piks"));
    assert_eq!(
        name("art/from-ase.ase.piks"),
        PathBuf::from("art/from-ase.piks")
    );
    assert_eq!(
        name("art/walk.aseprite.piks"),
        PathBuf::from("art/walk.piks")
    );
    assert_eq!(
        name("art/walk.ASEPRITE.PIKS"),
        PathBuf::from("art/walk.piks")
    );
    assert_eq!(name("art/walk.v2.piks"), PathBuf::from("art/walk.v2.piks"));
    assert_eq!(name("art/walk.png"), PathBuf::from("art/walk.piks"));
    assert_eq!(name("art/walk"), PathBuf::from("art/walk.piks"));
}
