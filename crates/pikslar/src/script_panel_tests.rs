use super::*;
use crate::test_app::{app, draw, within};
use std::time::Duration;

fn runnable(name: &str, source: &str) -> Runnable {
    Runnable {
        name: name.to_string(),
        source: source.to_string(),
        caps: Capabilities::ALL,
        plugin: None,
    }
}

/// What a run must leave alone when it keeps nothing.
fn state(a: &App) -> (u64, usize, usize, bool) {
    (
        a.editor.revision(),
        a.editor.history().undo_depth(),
        a.editor.doc().frames.len(),
        a.is_modified(),
    )
}

fn just_wait(finished: &dyn Fn() -> bool, _: &AtomicBool) {
    while !finished() {
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn status(a: &App) -> String {
    a.scripts
        .status
        .as_ref()
        .map(|s| s.text.clone())
        .unwrap_or_default()
}

#[test]
fn a_run_starts_on_the_next_frame_and_is_one_named_undo_step() {
    let mut a = app(1);
    a.start_script(Some(runnable(
        "frames.pks",
        "add_frame(0); add_frame(0); print(\"two\");",
    )));
    assert!(a.scripts.open && a.panel_open());
    assert_eq!(a.scripts.state, RunState::Starting);
    assert!(status(&a).contains("running"), "{}", status(&a));
    a.run_started_with(just_wait);
    assert_eq!(a.scripts.state, RunState::Idle);
    assert_eq!(a.editor.doc().frames.len(), 3);
    assert_eq!(a.scripts.output, ["two"]);
    assert!(status(&a).contains("frames.pks ran"), "{}", status(&a));
    a.undo();
    assert_eq!(
        a.editor.doc().frames.len(),
        1,
        "one step undoes the whole run"
    );
    let said = a.message.as_ref().map(|m| m.0.clone()).unwrap_or_default();
    assert_eq!(said, "Undid script frames.pks");
}

#[test]
fn a_cancelled_run_changes_nothing() {
    let mut a = app(2);
    a.editor.color = 0xff00_00ff;
    let before = state(&a);
    a.start_script(Some(runnable("long.pks", "add_frame(0); while true { }")));
    a.run_started_with(|finished, cancel| {
        std::thread::sleep(Duration::from_millis(20));
        cancel.store(true, Ordering::Relaxed);
        just_wait(finished, cancel);
    });
    assert_eq!(state(&a), before);
    assert!(a.scripts.status.as_ref().is_some_and(|s| s.failed));
    assert_eq!(status(&a), "long.pks was stopped. Nothing changed");
}

#[test]
fn a_failing_run_says_where_and_changes_nothing() {
    let mut a = app(1);
    let before = state(&a);
    a.start_script(Some(runnable(
        "bad.pks",
        "add_frame(0);\nprint(\"before\");\n  remove_layer(424242);",
    )));
    a.run_started_with(just_wait);
    assert_eq!(state(&a), before);
    let said = status(&a);
    assert!(
        said.starts_with("bad.pks changed nothing. Error at line 3, column 3"),
        "{said}"
    );
    assert_eq!(
        a.scripts.output,
        ["before"],
        "what it printed is still shown"
    );
    a.clear_script_output();
    assert!(a.scripts.output.is_empty() && a.scripts.status.is_none());
}

#[test]
fn long_output_and_long_errors_are_cut_short_and_draw_quickly() {
    let mut a = app(1);
    a.start_script(Some(runnable("print.pks", "print(repeat(\"a\", 65000));")));
    a.run_started_with(just_wait);
    let ident = "x".repeat(200_000);
    a.start_script(Some(runnable("bad.pks", &format!("let y = {ident};"))));
    a.run_started_with(just_wait);
    assert!(a.scripts.output.iter().all(|l| l.chars().count() <= 1100));
    assert!(status(&a).chars().count() <= 1100, "{}", status(&a).len());
    assert!(status(&a).starts_with("bad.pks changed nothing"));
    let start = std::time::Instant::now();
    draw(&mut a);
    let took = start.elapsed();
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn a_wait_that_ends_early_cancels_the_run_instead_of_hanging() {
    let mut a = app(1);
    a.script_limits.fuel = pikslar_bind::ScriptLimits::FUEL_RANGE.1;
    let before = state(&a);
    a.start_script(Some(runnable("spin.pks", "add_frame(0); while true { }")));
    // As when the window is closed or the app quits: the wait returns while the run goes on.
    within(20, "a run left by its wait", || {
        a.run_started_with(|_, _| {})
    });
    assert_eq!(state(&a), before);
    assert_eq!(status(&a), "spin.pks was stopped. Nothing changed");
}

#[test]
fn a_launch_script_is_stopped_after_its_time() {
    let mut a = app(1);
    a.script_limits.fuel = pikslar_bind::ScriptLimits::FUEL_RANGE.1;
    let dir = std::env::temp_dir().join(format!("pikslar-launch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let path = dir.join("spin.pks");
    std::fs::write(&path, "while true { }").expect("written");
    let before = state(&a);
    within(20, "a launch script", || {
        a.run_script_file_within(&path, Duration::from_millis(100))
    });
    assert_eq!(state(&a), before);
    let said = a.message.as_ref().map(|m| m.0.clone()).unwrap_or_default();
    assert!(said.contains("was stopped after 0.1 s"), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stop_is_esc_or_a_press_on_the_stop_button() {
    let stop = Rect::new(10, 10, 40, 20);
    let esc = Event::KeyDown {
        key: Key::Escape,
        mods: Default::default(),
        repeat: false,
    };
    let press = |x, button| Event::Down {
        x,
        y: 15,
        button,
        mods: Default::default(),
    };
    assert!(stops(&esc, stop));
    assert!(stops(&press(20, Button::Left), stop));
    assert!(!stops(&press(80, Button::Left), stop));
    assert!(!stops(&press(20, Button::Right), stop));
}

#[test]
fn the_panel_loads_a_file_and_runs_nothing_until_asked() {
    let mut a = app(1);
    let dir = std::env::temp_dir().join(format!("pikslar-panel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let path = dir.join("tidy.pks");
    std::fs::write(&path, "add_frame(0);").expect("written");
    a.open_script_panel();
    a.load_script_file(&path);
    assert_eq!(
        a.scripts.loaded.as_ref().map(|r| r.name.as_str()),
        Some("tidy.pks")
    );
    assert_eq!(a.editor.doc().frames.len(), 1);
    a.load_script_file(&dir.join("gone.pks"));
    assert!(status(&a).contains("could not be read"));
    assert!(
        a.scripts.loaded.is_some(),
        "the last good script stays loaded"
    );
    draw(&mut a);
    a.script_panel_key(Key::Escape);
    assert!(!a.scripts.open);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hidden_characters_in_what_a_script_prints_are_not_shown() {
    let mut a = app(1);
    let source = "print(\"pay\u{202E}txt.exe\u{200B} no\u{00A0}break\");";
    a.start_script(Some(runnable("hide.pks", source)));
    a.run_started_with(just_wait);
    assert_eq!(a.scripts.output, ["paytxt.exe no break"]);
    a.start_script(Some(runnable("bad.pks", "let x = y\u{2066}z;")));
    a.run_started_with(just_wait);
    assert!(status(&a).starts_with("bad.pks changed nothing"));
    assert_eq!(pikslar_plugin::hidden::first_unshown(&status(&a)), None);
    assert_eq!(
        crate::script_draw::fitted("a\u{202E}b\u{3000}c", 1000, 1),
        "ab c"
    );
}

fn said(a: &App) -> String {
    a.message.as_ref().map(|m| m.0.clone()).unwrap_or_default()
}

/// A frame added in the interface: one step of its own.
fn interface_step(a: &mut App) {
    a.run(pikslar_core::FrameCmd::Add { at: 0, copy: false });
}

#[test]
fn a_run_that_records_no_step_says_so_and_undo_does_not_name_it() {
    for (name, source) in [
        ("reads.pks", "print(width());"),
        ("hides.pks", "set_visible(layers()[0], false);"),
    ] {
        let mut a = app(1);
        interface_step(&mut a);
        let frames = a.editor.doc().frames.len();
        a.start_script(Some(runnable(name, source)));
        a.run_started_with(just_wait);
        assert!(status(&a).contains("nothing to undo"), "{}", status(&a));
        a.undo();
        assert_eq!(a.editor.doc().frames.len(), frames - 1, "the frame went");
        assert!(!said(&a).contains("Undid script"), "{name}: {}", said(&a));
    }
}

#[test]
fn a_new_document_forgets_the_last_scripts_step() {
    let mut a = app(1);
    a.start_script(Some(runnable("frames.pks", "add_frame(0);")));
    a.run_started_with(just_wait);
    assert!(status(&a).contains("one step to undo"), "{}", status(&a));
    a.replace_document(crate::app::starter_document(32), None);
    interface_step(&mut a);
    a.undo();
    assert!(!said(&a).contains("Undid script"), "{}", said(&a));
}

#[test]
fn failures_are_told_by_kind() {
    use pikslar_api::ApiError;
    use pikslar_core::LimitError;
    let at = pikslar_script::Pos { line: 3, column: 7 };
    let refused = |at, waiting| Failure::Refused {
        error: ApiError::NoSuchLayer(9),
        at,
        waiting,
    };
    let busy = Failure::Refused {
        error: ApiError::Refused(LimitError::Busy),
        at: None,
        waiting: false,
    };
    let text = |f: &Failure| failure_text("x.pks", f);
    let at_call = text(&refused(Some(at), false));
    assert!(at_call.starts_with("x.pks changed nothing. Error at line 3, column 7: "));
    assert!(!at_call.contains("waiting"), "{at_call}");
    let flushed = text(&refused(Some(at), true));
    assert!(
        flushed.contains("line 3, column 7: set_pixel (waiting pixels)"),
        "{flushed}"
    );
    let at_end = text(&refused(None, true));
    assert!(!at_end.contains("Error at at"), "{at_end}");
    assert!(at_end.contains("pixels"), "{at_end}");
    let grown = text(&Failure::Refused {
        error: ApiError::Refused(LimitError::NoMemory),
        at: None,
        waiting: false,
    });
    assert!(!grown.contains("pixels it left"), "{grown}");
    assert!(grown.starts_with("x.pks changed nothing: "), "{grown}");
    let thread = text(&Failure::Thread("no threads".into()));
    assert!(!thread.contains("Error at"), "{thread}");
    assert!(thread.contains("could not start: no threads"), "{thread}");
    let busy = text(&busy);
    assert!(!busy.contains("end of the script"), "{busy}");
    assert!(
        busy.contains("did not run") && busy.contains("floating"),
        "{busy}"
    );
}

#[test]
fn a_second_start_before_the_first_runs_changes_nothing() {
    let mut a = app(1);
    a.start_script(Some(runnable("first.pks", "add_frame(0);")));
    a.start_script(Some(runnable("second.pks", "print(1);")));
    let loaded = a.scripts.loaded.as_ref().map(|r| r.name.clone());
    assert_eq!(loaded.as_deref(), Some("first.pks"));
    a.run_started_with(just_wait);
    assert_eq!(a.editor.doc().frames.len(), 2);
}

#[test]
fn a_file_that_could_not_be_taken_back_is_not_called_unwritten() {
    let lost = failure_text("Report", &Failure::Files(FileError::NotPutBack));
    assert!(
        lost.starts_with("Report did not change the document, but"),
        "{lost}"
    );
    assert!(
        lost.contains("kept beside it") && !lost.contains("not written"),
        "{lost}"
    );
    let full = FileError::Io("could not write the file: no space".into());
    let said = failure_text("Report", &Failure::Files(full));
    assert!(said.contains("was not written"), "{said}");
}

#[test]
fn own_folders_are_the_settings_and_plugins_folders() {
    let mut a = app(1);
    let dir = std::env::temp_dir().join(format!("pikslar-own-{}", std::process::id()));
    a.use_settings(crate::settings::SettingsFile::in_dir(&dir));
    let own = a.own_folders();
    assert!(own.contains(&dir), "{own:?}");
    assert!(
        own.contains(&dir.join(pikslar_plugin::PLUGINS_FOLDER)),
        "{own:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
