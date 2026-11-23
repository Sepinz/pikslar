use super::*;
use crate::test_app::{app, within};
use std::path::PathBuf;

/// The menu place of a plugin's first command, under the lines that say who it is.
const FIRST_COMMAND: usize = IDENTITY_ITEMS;

/// A scratch folder holding a plugin to install and a store to install it in.
struct Place(PathBuf);

impl Place {
    fn new(name: &str) -> Place {
        let dir = std::env::temp_dir().join(format!("pikslar-pui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("source")).expect("a folder");
        Place(dir)
    }

    /// A plugin with two commands, asking for `caps`.
    fn plugin(&self, caps: &str) -> PathBuf {
        let source = self.0.join("source");
        let manifest = format!(
            r#"{{"manifest": 1, "id": "com.example.frames", "name": "Frames", "version": "1.0.0",
"api": 1, "capabilities": [{caps}], "commands": [
{{"id": "add", "title": "Add a frame", "script": "add.pks"}},
{{"id": "count", "title": "Count frames", "script": "count.pks"}}]}}"#
        );
        std::fs::write(source.join("plugin.json"), manifest).expect("written");
        std::fs::write(source.join("add.pks"), "add_frame(0);").expect("written");
        std::fs::write(source.join("count.pks"), "print(len(frames()));").expect("written");
        source
    }

    fn store(&self) -> PluginStore {
        PluginStore::in_dir(&self.0.join("store"))
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn labels(entries: &Entries) -> Vec<String> {
    entries.iter().map(|(l, _)| l.to_string()).collect()
}

fn said(a: &App) -> String {
    a.message.as_ref().map(|m| m.0.clone()).unwrap_or_default()
}

fn run_now(a: &mut App) {
    a.run_started_with(|finished, _| {
        while !finished() {
            std::thread::yield_now();
        }
    });
}

fn installed(place: &Place, caps: &str) -> App {
    let mut a = app(1);
    a.plugins.store = place.store();
    a.review_plugin(&place.plugin(caps));
    a.approve_plugin();
    a.open_scripts_menu((0, 0));
    a
}

#[test]
fn the_review_shows_what_it_asks_for_and_nothing_installs_until_approved() {
    let place = Place::new("review");
    let mut a = app(1);
    a.plugins.store = place.store();
    a.review_plugin(&place.plugin(r#""read-document", "edit-document""#));
    assert!(a.panel_open());
    let lines = a
        .plugins
        .review
        .as_ref()
        .map(|p| p.review().lines())
        .unwrap_or_default();
    assert_eq!(lines[0], "Frames 1.0.0");
    assert!(lines.iter().any(|l| l.contains("Change the open document")));
    assert!(lines
        .iter()
        .any(|l| l == "It adds: Add a frame, Count frames."));
    a.plugin_review_key(Key::Escape);
    assert!(a.plugins.review.is_none() && a.plugins.store.list().is_empty());
    assert_eq!(said(&a), "Nothing was installed");
    a.review_plugin(&place.0.join("missing"));
    assert!(
        said(&a).starts_with("Error: Nothing was installed: "),
        "{}",
        said(&a)
    );
}

#[test]
fn commands_sit_under_the_plugins_own_name() {
    let place = Place::new("menu");
    let a = installed(&place, r#""edit-document""#);
    assert_eq!(
        said(&a),
        "Installed Frames 1.0.0. Its commands are in Scripts"
    );
    let top = labels(&a.scripts_entries());
    assert_eq!(
        top,
        ["Script panel...", "Install plugin...", "Plugin: Frames"]
    );
    let own = labels(&a.plugin_entries(0));
    let print = match &a.plugins.listed[0].state {
        Ok(p) => p.fingerprint(),
        Err(why) => panic!("disabled: {why}"),
    };
    assert_eq!(print.len(), 64 + 7, "eight groups of eight: {print}");
    assert_eq!(
        own,
        [
            "Frames 1.0.0 (com.example.frames)".to_string(),
            "Author: not stated".to_string(),
            format!("Fingerprint: {print}"),
            "Add a frame".to_string(),
            "Count frames".to_string(),
            "Uninstall Frames".to_string()
        ]
    );
}

#[test]
fn the_identity_lines_run_nothing_and_commands_keep_their_places() {
    let place = Place::new("identity");
    let mut a = installed(&place, r#""edit-document""#);
    let frames = a.editor.doc().frames.len();
    for n in 0..FIRST_COMMAND {
        a.plugin_menu_action(0, n);
        run_now(&mut a);
    }
    assert_eq!(a.editor.doc().frames.len(), frames, "no command ran");
    assert_eq!(a.plugins.listed.len(), 1, "nothing was uninstalled");
    a.plugin_menu_action(0, FIRST_COMMAND);
    run_now(&mut a);
    assert_eq!(a.editor.doc().frames.len(), frames + 1, "Add a frame ran");
    a.plugin_menu_action(0, FIRST_COMMAND + 2);
    assert!(
        a.plugins.store.list().is_empty(),
        "the last item uninstalls"
    );
}

#[test]
fn a_command_runs_with_exactly_what_was_approved() {
    let place = Place::new("caps");
    let mut a = app(1);
    a.plugins.store = place.store();
    a.review_plugin(&place.plugin(r#""edit-document""#));
    a.approve_plugin();
    a.open_scripts_menu((0, 0));
    a.plugin_menu_action(0, FIRST_COMMAND);
    run_now(&mut a);
    assert_eq!(a.editor.doc().frames.len(), 2, "editing was approved");
    let depth = a.editor.history().undo_depth();
    a.plugin_menu_action(0, FIRST_COMMAND + 1);
    run_now(&mut a);
    let status = a.scripts.status.clone().map(|s| s.text).unwrap_or_default();
    assert!(
        status.starts_with("Frames: Count frames changed nothing"),
        "{status}"
    );
    assert_eq!(
        a.editor.history().undo_depth(),
        depth,
        "reading was not approved"
    );
}

#[test]
fn a_changed_plugin_is_disabled_and_says_why() {
    let place = Place::new("tamper");
    let mut a = installed(&place, r#""edit-document""#);
    let script = place
        .0
        .join("store")
        .join("com.example.frames")
        .join("add.pks");
    std::fs::write(&script, "add_frame(0); add_frame(0);").expect("changed");
    a.plugin_menu_action(0, FIRST_COMMAND);
    assert!(
        said(&a).contains("cannot run: its files changed"),
        "{}",
        said(&a)
    );
    assert_eq!(a.scripts.state, crate::script_panel::RunState::Idle);
    assert_eq!(a.editor.doc().frames.len(), 1);
    let top = labels(&a.scripts_entries());
    assert_eq!(top[2], "Plugin: com.example.frames (disabled)");
    let own = labels(&a.plugin_entries(0));
    assert!(own[0].starts_with("Disabled: its files changed"), "{own:?}");
    assert_eq!(own[1], "Uninstall com.example.frames");
    a.plugin_menu_action(0, 1);
    assert_eq!(said(&a), "Uninstalled com.example.frames");
    assert!(a.plugins.listed.is_empty() && a.plugins.store.list().is_empty());
}

#[test]
fn with_settings_off_plugins_are_off() {
    let place = Place::new("off");
    let mut a = app(1);
    a.review_plugin(&place.plugin(""));
    assert!(a.plugins.review.is_none());
    assert!(said(&a).contains("plugins are off"), "{}", said(&a));
}

/// A plugin that asks for a text file to read and prints it.
fn picker(place: &Place) -> PathBuf {
    let source = place.0.join("pick");
    std::fs::create_dir_all(&source).expect("a folder");
    let manifest = r#"{"manifest": 1, "id": "com.example.pick", "name": "Picker", "version": "1.0.0",
"api": 1, "capabilities": ["pick-file-to-read"], "commands": [
{"id": "show", "title": "Show a file", "script": "show.pks"}]}"#;
    std::fs::write(source.join("plugin.json"), manifest).expect("written");
    let script = "let h = pick_file_to_read(\"Choose notes\", [\"txt\"]);\nprint(file_name(h) + \": \" + read_file(h));";
    std::fs::write(source.join("show.pks"), script).expect("written");
    source
}

#[test]
fn a_pick_left_unanswered_when_the_wait_ends_does_not_hang_the_run() {
    let place = Place::new("unanswered");
    let mut a = app(1);
    a.plugins.store = place.store();
    a.review_plugin(&picker(&place));
    a.approve_plugin();
    a.open_scripts_menu((0, 0));
    a.plugin_menu_action(0, FIRST_COMMAND);
    within(20, "a run waiting on a pick", || {
        a.run_started_with(|finished, _| {
            // The run asks; the window goes away before it answers.
            let start = std::time::Instant::now();
            while !finished() && start.elapsed() < std::time::Duration::from_millis(200) {
                std::thread::yield_now();
            }
        })
    });
    assert!(a.scripts.status.as_ref().is_some_and(|s| s.failed));
    assert!(a.scripts.output.is_empty());
}

#[test]
fn a_plugin_granted_reading_a_picked_file_gets_it_through_the_windows_dialog() {
    let place = Place::new("pick");
    let source = picker(&place);
    let notes = place.0.join("notes.txt");
    std::fs::write(&notes, "hello").expect("written");
    let mut a = app(1);
    a.plugins.store = place.store();
    a.review_plugin(&source);
    a.approve_plugin();
    a.open_scripts_menu((0, 0));
    a.plugin_menu_action(0, FIRST_COMMAND);
    let bridge = a.scripts.bridge.clone();
    let mut asked = Vec::new();
    a.run_started_with(|finished, _| {
        while !finished() {
            bridge.answer(|q| {
                asked.push(q.clone());
                Some(notes.clone())
            });
            std::thread::yield_now();
        }
    });
    assert_eq!(a.scripts.output, ["notes.txt: hello"]);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].title,
        "Plugin com.example.pick \"Picker\" asks: Choose notes"
    );
    assert!(!a.is_modified(), "it was granted no edits");
}

/// A plugin that asks where to save a report and writes it.
fn saver(place: &Place) -> PathBuf {
    let source = place.0.join("save");
    std::fs::create_dir_all(&source).expect("a folder");
    let manifest = r#"{"manifest": 1, "id": "com.example.save", "name": "Saver", "version": "1.0.0",
"api": 1, "capabilities": ["pick-file-to-write"], "commands": [
{"id": "save", "title": "Save a report", "script": "save.pks"}]}"#;
    std::fs::write(source.join("plugin.json"), manifest).expect("written");
    let script =
        "let h = pick_file_to_write(\"Save the report\", [\"txt\"]);\nwrite_file(h, \"report\");";
    std::fs::write(source.join("save.pks"), script).expect("written");
    source
}

#[test]
fn a_plugin_granted_writing_saves_through_the_save_dialog_and_the_systems_replace() {
    let place = Place::new("save");
    let source = saver(&place);
    let report = place.0.join("report.txt");
    std::fs::write(&report, "old").expect("written");
    let mut a = app(1);
    a.plugins.store = place.store();
    a.review_plugin(&source);
    a.approve_plugin();
    a.open_scripts_menu((0, 0));
    a.plugin_menu_action(0, FIRST_COMMAND);
    let bridge = a.scripts.bridge.clone();
    let mut asked = Vec::new();
    a.run_started_with(|finished, _| {
        while !finished() {
            bridge.answer(|q| {
                asked.push(q.clone());
                Some(report.clone())
            });
            std::thread::yield_now();
        }
    });
    assert!(a.scripts.status.as_ref().is_some_and(|s| !s.failed));
    assert_eq!(std::fs::read(&report).expect("read"), b"report");
    assert_eq!(asked.len(), 1);
    let q = &asked[0];
    assert_eq!(
        q.title,
        "Plugin com.example.save \"Saver\" asks: Save the report"
    );
    assert!(q.save && q.extension.as_deref() == Some("txt"));
    let left: Vec<_> = std::fs::read_dir(&place.0)
        .expect("listed")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("~pikslar"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(!a.is_modified(), "it was granted no edits");
}

#[test]
fn a_stray_folder_name_is_shown_without_hidden_characters() {
    let place = Place::new("strayname");
    let mut a = app(1);
    a.plugins.store = place.store();
    let stray = place.0.join("store").join("Not\u{202E}an\u{200B}id");
    std::fs::create_dir_all(&stray).expect("a folder");
    a.open_scripts_menu((0, 0));
    let top = labels(&a.scripts_entries());
    assert_eq!(top[2], "Plugin: Notanid (disabled)");
    let own = labels(&a.plugin_entries(0));
    assert_eq!(own[1], "Uninstall Notanid");
    assert!(own
        .iter()
        .all(|l| pikslar_plugin::hidden::first_unshown(l).is_none()));
}

/// A plugin with the longest description, made of long words, asking for everything.
fn greedy(place: &Place) -> PathBuf {
    let source = place.0.join("greedy");
    std::fs::create_dir_all(&source).expect("a folder");
    let words: Vec<String> = (0..10).map(|n| format!("{n}{}", "w".repeat(38))).collect();
    let description = words.join(" ");
    assert_eq!(description.chars().count(), 399);
    let manifest = format!(
        r#"{{"manifest": 1, "id": "com.example.greedy", "name": "Greedy", "version": "1.0.0",
"api": 1, "description": "{description}", "capabilities": ["read-document", "edit-document",
"pick-file-to-read", "pick-file-to-write"], "commands": [
{{"id": "go", "title": "Go", "script": "go.pks"}}]}}"#
    );
    std::fs::write(source.join("plugin.json"), manifest).expect("written");
    std::fs::write(source.join("go.pks"), "print(1);").expect("written");
    source
}

/// Each named rectangle of the last drawing: its name, top and bottom.
fn named(a: &App) -> Vec<(String, i32, i32)> {
    let layout = a.ui.layout().expect("names are kept");
    layout
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let (y, h) = (
                f.get(2)?.parse::<i32>().ok()?,
                f.get(4)?.parse::<i32>().ok()?,
            );
            Some((f.get(5)?.to_string(), y, y + h))
        })
        .collect()
}

#[test]
fn every_capability_shows_above_install_or_install_is_withheld() {
    let place = Place::new("greedy");
    let mut shown_with_install = 0;
    // 1920 x 1040 at scale 2 is a 1080p screen at 200%; the last two are too small.
    for (w, h, scale) in [
        (1200, 700, 1),
        (1920, 1040, 2),
        (900, 420, 2),
        (700, 360, 3),
    ] {
        let mut a = app(1);
        a.plugins.store = place.store();
        a.review_plugin(&greedy(&place));
        a.ui.record_names();
        a.resized(w, h, scale);
        crate::test_app::draw_whole_at(&mut a, w, h);
        let rows = named(&a);
        let screen_bottom = h;
        let install = rows.iter().find(|r| r.0 == "Install");
        let caps: Vec<_> = rows
            .iter()
            .filter(|r| r.0.starts_with("Review: - "))
            .collect();
        match install {
            Some(button) => {
                shown_with_install += 1;
                assert_eq!(caps.len(), 4, "at {w}x{h} scale {scale}: {rows:?}");
                for cap in &caps {
                    assert!(
                        cap.2 <= button.1,
                        "{cap:?} runs under Install at scale {scale}"
                    );
                }
                assert!(button.2 <= screen_bottom);
                let description = rows.iter().find(|r| r.0.contains("Its own description"));
                let lines =
                    description.map_or(0, |d| (d.2 - d.1) / (crate::script_draw::LINE * scale));
                assert!(lines <= 3, "{lines} lines of description at scale {scale}");
            }
            None => assert!(
                rows.iter()
                    .any(|r| r.0.contains("too small to show everything")),
                "neither Install nor the reason at {w}x{h} scale {scale}: {rows:?}"
            ),
        }
    }
    assert!(
        shown_with_install >= 2,
        "a large enough window offers Install"
    );
    assert!(
        shown_with_install < 4,
        "a small window at scale 3 withholds it"
    );
}

fn status_of(a: &App) -> String {
    a.scripts.status.clone().map(|s| s.text).unwrap_or_default()
}

#[test]
fn run_in_the_panel_loads_the_plugin_again_each_time() {
    let place = Place::new("rerun");
    let mut a = installed(&place, r#""edit-document""#);
    a.plugin_menu_action(0, FIRST_COMMAND);
    run_now(&mut a);
    assert_eq!(a.editor.doc().frames.len(), 2);
    let script = place
        .0
        .join("store")
        .join("com.example.frames")
        .join("add.pks");
    std::fs::write(&script, "add_frame(0); add_frame(0);").expect("changed");
    // Run script again, from the panel: the plugin's files changed since it was approved.
    a.start_script(None);
    run_now(&mut a);
    assert_eq!(
        a.editor.doc().frames.len(),
        2,
        "the changed copy did not run"
    );
    assert!(
        status_of(&a).contains("its files changed"),
        "{}",
        status_of(&a)
    );
    a.uninstall_plugin("com.example.frames");
    assert!(
        a.scripts.loaded.is_none(),
        "an uninstalled plugin cannot be run again"
    );
}

#[test]
fn approving_a_plugin_again_forgets_its_loaded_command() {
    let place = Place::new("reapprove");
    let mut a = installed(&place, r#""edit-document""#);
    a.plugin_menu_action(0, FIRST_COMMAND);
    run_now(&mut a);
    assert!(a.scripts.loaded.is_some());
    a.review_plugin(&place.0.join("source"));
    a.approve_plugin();
    assert!(a.scripts.loaded.is_none());
}

#[test]
fn a_command_is_run_by_its_id_even_when_the_plugin_changed_meanwhile() {
    let place = Place::new("byid");
    let mut a = installed(&place, r#""edit-document""#);
    // Installed again behind the open menu, with the commands the other way round.
    let source = place.0.join("source");
    let manifest = std::fs::read_to_string(source.join("plugin.json")).expect("manifest");
    let swapped = manifest
        .replace("1.0.0", "1.1.0")
        .replace(
            r#"{"id": "add", "title": "Add a frame", "script": "add.pks"},"#,
            "",
        )
        .replace(
            r#"{"id": "count", "title": "Count frames", "script": "count.pks"}]"#,
            r#"{"id": "count", "title": "Count frames", "script": "count.pks"},
{"id": "add", "title": "Add a frame", "script": "add.pks"}]"#,
        );
    std::fs::write(source.join("plugin.json"), swapped).expect("written");
    let store = place.store();
    store
        .approve(store.review(&source).expect("review"))
        .expect("approve");
    a.plugin_menu_action(0, FIRST_COMMAND);
    run_now(&mut a);
    assert_eq!(
        a.editor.doc().frames.len(),
        2,
        "Add a frame ran, as the menu said"
    );
    // Gone in the next version: said, and nothing runs.
    let gone = std::fs::read_to_string(source.join("plugin.json"))
        .expect("manifest")
        .replace("1.1.0", "1.2.0")
        .replace(
            r#",
{"id": "add", "title": "Add a frame", "script": "add.pks"}]"#,
            "]",
        );
    std::fs::write(source.join("plugin.json"), gone).expect("written");
    std::fs::remove_file(source.join("add.pks")).expect("removed");
    store
        .approve(store.review(&source).expect("review"))
        .expect("approve");
    a.plugin_menu_action(0, FIRST_COMMAND);
    assert_eq!(a.scripts.state, crate::script_panel::RunState::Idle);
    assert!(said(&a).contains("no longer has"), "{}", said(&a));
}

/// The worst-case review at `w` x `h`, scale `scale`: the lines of the description shown, the
/// capability lines shown, and whether Install is offered.
fn review_at(place: &Place, w: i32, h: i32, scale: i32) -> (i32, usize, bool, Vec<String>) {
    let mut a = app(1);
    a.plugins.store = place.store();
    a.review_plugin(&greedy(place));
    a.ui.record_names();
    a.resized(w, h, scale);
    crate::test_app::draw_whole_at(&mut a, w, h);
    let rows = named(&a);
    let lines = rows
        .iter()
        .find(|r| r.0.contains("Its own description"))
        .map_or(0, |d| (d.2 - d.1) / (crate::script_draw::LINE * scale));
    let caps = rows
        .iter()
        .filter(|r| r.0.starts_with("Review: - "))
        .count();
    let install = rows.iter().any(|r| r.0 == "Install");
    let parts = rows.iter().map(|r| r.0.clone()).collect();
    (lines, caps, install, parts)
}

#[test]
fn the_worst_case_review_fits_a_1080p_screen_at_200_percent_with_install() {
    let place = Place::new("greedy-1080");
    let (lines, caps, install, rows) = review_at(&place, 1920, 1040, 2);
    assert!(install, "Install is offered: {rows:?}");
    assert_eq!(caps, 4, "every capability is shown");
    assert!((1..=3).contains(&lines), "{lines} lines of description");
}

#[test]
fn the_description_shrinks_before_a_capability_is_dropped() {
    crate::test_app::large_text();
    let place = Place::new("greedy-shrink");
    let (roomy, caps_roomy, _, _) = review_at(&place, 1920, 1200, 2);
    let (tight, caps_tight, install, rows) = review_at(&place, 1920, 1040, 2);
    assert_eq!(roomy, 3, "with room the description keeps three lines");
    assert!(tight < roomy, "short of space it gives way: {tight}");
    assert_eq!((caps_roomy, caps_tight), (4, 4));
    assert!(install, "{rows:?}");
    assert_eq!(tight, 1, "it gave way down to one line");
}

fn review_tap(a: &mut App, key: Key, shift: bool) {
    let mods = pikslar_platform::Mods {
        shift,
        ..Default::default()
    };
    a.key_down(key, mods, false, pikslar_platform::Window::detached());
    crate::test_app::draw_whole(a);
}

#[test]
fn the_review_starts_on_do_not_install_and_enter_never_installs() {
    let (install, deny) = (
        crate::script_draw::ID_SCRIPTS + 10,
        crate::script_draw::ID_SCRIPTS + 11,
    );
    let place = Place::new("review-keys");
    let mut a = app(1);
    a.plugins.store = place.store();
    a.review_plugin(&place.plugin(r#""read-document""#));
    crate::test_app::draw_whole(&mut a);
    review_tap(&mut a, Key::Enter, false);
    assert!(a.plugins.review.is_some(), "Enter with nothing focused");
    review_tap(&mut a, Key::Tab, false);
    assert_eq!(a.ui.focus.focused(), Some(deny), "the first stop declines");
    review_tap(&mut a, Key::Tab, false);
    assert_eq!(a.ui.focus.focused(), Some(install));
    review_tap(&mut a, Key::Enter, false);
    assert!(
        a.plugins.review.is_some(),
        "Enter on Install does not install"
    );
    assert!(a.plugins.store.list().is_empty());
    review_tap(&mut a, Key::Space, false);
    assert!(a.plugins.review.is_none());
    assert_eq!(
        a.plugins.store.list().len(),
        1,
        "Space on a visible Install installs"
    );
}

#[test]
fn shift_tab_from_nowhere_reaches_install_and_enter_declines_on_do_not_install() {
    let place = Place::new("review-deny");
    let mut a = app(1);
    a.plugins.store = place.store();
    a.review_plugin(&place.plugin(r#""read-document""#));
    crate::test_app::draw_whole(&mut a);
    review_tap(&mut a, Key::Tab, true);
    assert_eq!(
        a.ui.focus.focused(),
        Some(crate::script_draw::ID_SCRIPTS + 10)
    );
    review_tap(&mut a, Key::Tab, true);
    assert_eq!(
        a.ui.focus.focused(),
        Some(crate::script_draw::ID_SCRIPTS + 11)
    );
    review_tap(&mut a, Key::Enter, false);
    assert!(
        a.plugins.review.is_none(),
        "Enter on Do not install declines"
    );
    assert!(a.plugins.store.list().is_empty(), "and installed nothing");
}

#[test]
fn opening_the_review_clears_the_keyboard_focus_like_every_other_dialog() {
    let place = Place::new("review-focus");
    let mut a = app(1);
    a.plugins.store = place.store();
    a.focus_on(7);
    assert!(a.ui.focus.focused().is_some());
    a.review_plugin(&place.plugin(r#""read-document""#));
    assert!(a.plugins.review.is_some());
    assert_eq!(a.ui.focus.focused(), None);
}
