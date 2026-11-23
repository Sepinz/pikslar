//! Exporting from the app (decision D1): every kind of file shows floating pixels where they
//! are, and nothing about the project changes: not the pixels, the selection, the floating
//! pixels, undo and redo, or whether it is saved. The files are those the shared plan makes,
//! the same the command line writes for the same choices.

use crate::app::App;
use crate::export_dialog::{ExportChoice, FrameChoice};
use crate::test_app::{app, dot, key, PLAIN};
use pikslar_core::{FloatingCmd, LayerCmd, Mask, SelectOp, SelectionCmd, TagCmd};
use pikslar_export::{plan, write_outputs, Kind, Layers, Picture};
use pikslar_format::png;
use pikslar_platform::{Key, Mods};
use std::fs;
use std::path::PathBuf;

const SIDE: usize = 32;

/// A pixel at (1, 1), lifted and moved right by `dx`, with a step undone behind it so redo
/// has something to lose.
fn floating(dx: i32) -> App {
    let mut a = app(2);
    dot(&mut a, 20, 20);
    a.undo();
    dot(&mut a, 1, 1);
    let shape = Box::new(Mask::rect(1, 1, 1, 1));
    a.editor
        .apply(SelectionCmd::Select {
            shape,
            op: SelectOp::Replace,
        })
        .expect("select");
    a.editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    a.editor
        .apply(FloatingCmd::Move {
            dx,
            dy: 0,
            join: true,
        })
        .expect("move");
    assert!(a.editor.is_floating());
    a
}

fn folder(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("pikslar-app-export-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Everything export must leave as it was.
#[derive(Debug, PartialEq)]
struct State {
    revision: u64,
    undo: usize,
    redo: usize,
    floating: Option<(i32, i32)>,
    selection: Option<(i32, i32, i32, i32)>,
    modified: bool,
    doc: pikslar_core::Document,
}

fn state(a: &App) -> State {
    State {
        revision: a.editor.revision(),
        undo: a.editor.history().undo_depth(),
        redo: a.editor.history().redo_depth(),
        floating: a.editor.floating_view().map(|v| (v.x, v.y)),
        selection: a.editor.selection().map(|s| s.bounds()),
        modified: a.is_modified(),
        doc: a.editor.doc().clone(),
    }
}

fn choice(kind: Kind) -> ExportChoice {
    ExportChoice::default().with_kind(kind)
}

fn export(a: &mut App, kind: Kind, path: PathBuf) -> Option<String> {
    let mut never = |_: &str| panic!("nothing to ask");
    a.export_as(&choice(kind), path, &mut never)
        .expect("exports")
}

#[test]
fn every_kind_of_file_exports_floating_pixels_and_changes_nothing() {
    for kind in Kind::ALL {
        let mut a = floating(5);
        a.saved_revision = a.editor.revision();
        let before = state(&a);
        let dir = folder("unchanged");
        let path = dir.join(format!("out.{}", kind.extension()));
        let done = export(&mut a, kind, path.clone()).expect("written");
        assert!(done.starts_with("Exported "), "{done}");
        assert_eq!(state(&a), before, "{kind:?}");
        assert!(a.editor.is_floating());
        if matches!(kind, Kind::Png | Kind::Sheet | Kind::Apng) {
            let image = png::decode(&fs::read(&path).expect("read")).expect("png");
            let color = a.editor.color;
            assert_eq!(
                image.px[image.width + 6],
                color,
                "{kind:?}: moved to (6, 1)"
            );
            assert_eq!(image.px[image.width + 1], 0, "{kind:?}: lifted from (1, 1)");
        }
        fs::remove_dir_all(dir).expect("cleanup");
    }
}

#[test]
fn floating_pixels_moved_off_the_canvas_are_clipped() {
    let mut a = floating(SIDE as i32);
    let dir = folder("clipped");
    let path = dir.join("out.png");
    export(&mut a, Kind::Png, path.clone());
    let image = png::decode(&fs::read(&path).expect("read")).expect("png");
    assert_eq!((image.width, image.height), (SIDE, SIDE));
    assert!(image.px.iter().all(|&p| p == 0));
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn a_failed_export_changes_nothing_and_writes_nothing() {
    let mut a = floating(5);
    let before = state(&a);
    let dir = folder("failed");
    let missing = dir.join("no such folder").join("out.png");
    let mut never = |_: &str| panic!("nothing to ask");
    let refused = a.export_as(&choice(Kind::Png), missing, &mut never);
    assert!(refused.expect_err("refused").contains("does not exist"));
    let too_large = ExportChoice {
        scale: 1000,
        ..choice(Kind::Sheet)
    };
    let refused = a.export_as(&too_large, dir.join("s.png"), &mut never);
    assert!(
        !refused.expect_err("refused").contains("--"),
        "no command line words"
    );
    assert_eq!(state(&a), before);
    assert!(fs::read_dir(&dir).expect("list").next().is_none());
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn the_app_writes_the_bytes_the_shared_plan_makes_from_the_document() {
    let mut a = app(3);
    dot(&mut a, 3, 3);
    a.editor
        .apply(TagCmd::Add {
            name: "walk".into(),
            from: 1,
            to: 2,
        })
        .expect("tag");
    let picked = ExportChoice {
        frames: FrameChoice::Tag(a.editor.doc().tags[0].id),
        columns: Some(2),
        padding: 3,
        hash: true,
        scale: 2,
        ..choice(Kind::Sheet)
    };
    let dir = folder("same-bytes");
    let mut never = |_: &str| panic!("nothing to ask");
    a.export_as(&picked, dir.join("app.png"), &mut never)
        .expect("exports");
    let doc = a.editor.doc().clone();
    let picture = Picture::new(&doc, &Layers::Visible, 2).expect("drawn");
    let job = picked.job(0, &doc.tags).expect("tag");
    let mut outputs = plan(&job, &picture, dir.join("lib.png"))
        .expect("planned")
        .outputs;
    write_outputs(&mut outputs, &mut |_, _| Ok(())).expect("written");
    assert_eq!(
        fs::read(dir.join("app.png")).ok(),
        fs::read(dir.join("lib.png")).ok()
    );
    let text = |n: &str| {
        fs::read_to_string(dir.join(n))
            .expect("json")
            .replace("lib", "app")
    };
    assert_eq!(text("app.json"), text("lib.json"));
    assert!(text("app.json").contains("\"app 1\":{\"frame\""));
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn files_the_save_dialog_did_not_ask_about_are_asked_about_first() {
    let mut a = app(2);
    let dir = folder("ask");
    let path = dir.join("walk.png");
    let member = dir.join("walk_0001.png");
    fs::write(&member, "earlier").expect("write");
    let mut asked = Vec::new();
    let mut no = |name: &str| {
        asked.push(name.to_string());
        false
    };
    let kept = a.export_as(&choice(Kind::Sequence), path.clone(), &mut no);
    assert_eq!(kept, Ok(None));
    assert_eq!(asked, ["walk_0001.png"]);
    assert_eq!(fs::read_to_string(&member).expect("kept"), "earlier");
    let mut yes = |_: &str| true;
    let done = a.export_as(&choice(Kind::Sequence), path, &mut yes);
    assert!(done.expect("exports").is_some());
    assert!(png::decode(&fs::read(&member).expect("read")).is_ok());
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn a_description_someone_else_wrote_is_kept_and_said_so() {
    let mut a = app(2);
    let dir = folder("foreign");
    fs::write(dir.join("walk.json"), "{\"mine\":1}").expect("write");
    let done = export(&mut a, Kind::Sheet, dir.join("walk.png")).expect("written");
    assert!(done.contains("keeping the .json already there"), "{done}");
    let kept = fs::read_to_string(dir.join("walk.json")).expect("read");
    assert_eq!(kept, "{\"mine\":1}");
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn ctrl_e_opens_the_panel_with_the_last_choice_and_escape_closes_it() {
    let ctrl = Mods {
        ctrl: true,
        shift: false,
        alt: false,
    };
    let mut a = app(2);
    key(&mut a, Key::Char(b'E'), ctrl);
    assert_eq!(a.export_dialog.map(|c| c.kind), Some(Kind::Png));
    key(&mut a, Key::Escape, PLAIN);
    assert!(a.export_dialog.is_none());
    a.export_choice = choice(Kind::Gif);
    key(&mut a, Key::Char(b'E'), ctrl);
    assert_eq!(a.export_dialog.map(|c| c.kind), Some(Kind::Gif));
    key(&mut a, Key::Escape, PLAIN);
    let shift = Mods {
        shift: true,
        ..ctrl
    };
    key(&mut a, Key::Char(b'E'), shift);
    assert_eq!(a.export_dialog.map(|c| c.kind), Some(Kind::Sheet));
}

#[test]
fn a_digit_chooses_the_kind_and_a_new_kind_repaints_the_canvas_under_the_panel() {
    let mut a = app(2);
    a.open_export_dialog(true);
    for (digit, kind, repaint) in [(b'4', Kind::Gif, true), (b'4', Kind::Gif, false)] {
        a.full_canvas = false;
        key(&mut a, Key::Char(digit), PLAIN);
        assert_eq!(a.export_dialog.map(|c| c.kind), Some(kind));
        assert_eq!(a.full_canvas, repaint, "{digit}");
    }
}

#[test]
fn this_layer_and_indexed_reach_the_file() {
    let mut a = app(1);
    dot(&mut a, 2, 2);
    let layer = a.editor.layer_id(0).expect("layer");
    a.editor
        .apply(LayerCmd::SetVisible {
            layer,
            visible: false,
        })
        .expect("hidden");
    let dir = folder("this-layer");
    let picked = ExportChoice {
        this_layer: true,
        indexed: true,
        ..choice(Kind::Png)
    };
    let mut never = |_: &str| panic!("nothing to ask");
    a.export_as(&picked, dir.join("app.png"), &mut never)
        .expect("exports");
    let doc = a.editor.doc().clone();
    let picture = Picture::new(&doc, &Layers::Only(vec![0]), 1).expect("drawn");
    let picture = picture.with_indexed(true);
    let mut outputs = plan(
        &picked.job(0, &doc.tags).expect("job"),
        &picture,
        dir.join("lib.png"),
    )
    .expect("planned")
    .outputs;
    write_outputs(&mut outputs, &mut |_, _| Ok(())).expect("written");
    let app = fs::read(dir.join("app.png")).expect("read");
    assert_eq!(Some(&app), fs::read(dir.join("lib.png")).ok().as_ref());
    assert_eq!(app[25], 3, "an indexed PNG");
    let pixels = png::decode(&app).expect("decodes");
    assert_ne!(pixels.px[2 * pixels.width + 2], 0, "the hidden layer drawn");
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn files_left_from_a_longer_sequence_and_the_project_itself_are_refused() {
    let mut a = app(2);
    let dir = folder("stale");
    fs::write(dir.join("walk_0003.png"), "earlier").expect("write");
    let mut never = |_: &str| panic!("nothing to ask");
    let refused = a.export_as(&choice(Kind::Sequence), dir.join("walk.png"), &mut never);
    assert!(refused.expect_err("refused").contains("walk_0003.png"));
    let project = dir.join("art.png");
    fs::write(&project, "a project").expect("write");
    a.path = Some(project.clone());
    let refused = a.export_as(&choice(Kind::Png), project.clone(), &mut never);
    assert!(
        refused.is_err(),
        "the project is never replaced by an export"
    );
    assert_eq!(fs::read_to_string(&project).expect("kept"), "a project");
    let names: Vec<_> = fs::read_dir(&dir).expect("list").collect();
    assert_eq!(names.len(), 2, "nothing written");
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn a_refusal_is_found_before_anything_is_asked() {
    let mut a = app(2);
    let dir = folder("refused-first");
    fs::write(dir.join("walk_0001.png"), "earlier").expect("write");
    fs::write(dir.join("walk_0002.png"), "a project").expect("write");
    a.path = Some(dir.join("walk_0002.png"));
    let mut never = |_: &str| panic!("asked before the refusal");
    let refused = a.export_as(&choice(Kind::Sequence), dir.join("walk.png"), &mut never);
    assert!(refused.is_err());
    let bad = a.export_as(&choice(Kind::Png), dir.join("a?.png"), &mut never);
    assert!(bad.expect_err("refused").contains("Windows does not allow"));
    assert_eq!(
        fs::read_dir(&dir).expect("list").count(),
        2,
        "nothing written"
    );
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn keeping_a_description_still_waits_for_another_export_of_the_same_set() {
    let mut a = app(2);
    let dir = folder("same-lock");
    fs::write(dir.join("walk.json"), "{\"mine\":1}").expect("write");
    let other = pikslar_format::set_lock::SetLock::take(&dir.join("walk.json")).expect("held");
    let mut never = |_: &str| panic!("nothing to ask");
    let refused = a.export_as(&choice(Kind::Sequence), dir.join("walk.png"), &mut never);
    assert!(refused.is_err(), "another export of this set is running");
    assert!(!dir.join("walk_0001.png").exists());
    drop(other);
    assert!(export(&mut a, Kind::Sequence, dir.join("walk.png")).is_some());
    fs::remove_dir_all(dir).expect("cleanup");
}

/// Three frames a tag apart, each tag its own frames: idle 0, walk 1, run 2.
fn three_tags() -> App {
    let mut a = app(3);
    for (i, name) in ["idle", "walk", "run"].into_iter().enumerate() {
        a.editor
            .apply(TagCmd::Add {
                name: name.into(),
                from: i,
                to: i,
            })
            .expect("tag");
    }
    a
}

fn chosen_tag(a: &App) -> Option<String> {
    match a.export_dialog?.frames {
        FrameChoice::Tag(id) => crate::export_dialog::place(&a.editor.doc().tags, id)
            .map(|i| a.editor.doc().tags[i].name.clone()),
        _ => None,
    }
}

#[test]
fn the_chosen_tag_survives_others_being_removed_and_the_list_changing() {
    let mut a = three_tags();
    let walk = a.editor.doc().tags[1].id;
    a.export_choice = ExportChoice {
        frames: FrameChoice::Tag(walk),
        ..choice(Kind::Gif)
    };
    a.run(TagCmd::Remove { tag: 0 });
    a.open_export_dialog(false);
    assert_eq!(
        chosen_tag(&a).as_deref(),
        Some("walk"),
        "not run, now second"
    );
    a.close_export_dialog();
    a.undo();
    a.open_export_dialog(false);
    assert_eq!(
        chosen_tag(&a).as_deref(),
        Some("walk"),
        "idle back in front of it"
    );
    a.close_export_dialog();
    let dir = folder("tag-identity");
    let gif = dir.join("walk.gif");
    let mut never = |_: &str| panic!("nothing to ask");
    let chosen = a.export_choice;
    a.export_as(&chosen, gif.clone(), &mut never)
        .expect("exports")
        .expect("written");
    let doc = a.editor.doc().clone();
    let job = chosen.job(0, &doc.tags).expect("walk is there");
    assert_eq!(job.frames, pikslar_export::Frames::TagAt(1));
    fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn a_chosen_tag_that_was_removed_is_never_replaced_by_another() {
    let mut a = three_tags();
    let walk = a.editor.doc().tags[1].id;
    let picked = ExportChoice {
        frames: FrameChoice::Tag(walk),
        ..choice(Kind::Gif)
    };
    a.run(TagCmd::Remove { tag: 1 });
    let dir = folder("tag-gone");
    let mut never = |_: &str| panic!("nothing to ask");
    let refused = a
        .export_as(&picked, dir.join("walk.gif"), &mut never)
        .expect_err("refused");
    assert!(refused.contains("no longer in the project"), "{refused}");
    assert!(!dir.join("walk.gif").exists());
    a.export_choice = picked;
    a.open_export_dialog(false);
    assert_eq!(a.export_dialog.map(|c| c.frames), Some(FrameChoice::All));
    fs::remove_dir_all(dir).expect("cleanup");
}
