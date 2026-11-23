use super::*;
use crate::panel_right::Section;
use crate::test_app::*;
use crate::workspace::model::Side;
use std::fs;
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("pikslar-ui-settings-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

fn saved_sprite(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let a = app(1);
    crate::files::save(&path, a.editor.doc()).expect("saved");
    path
}

#[test]
fn chosen_settings_survive_a_restart() {
    let dir = temp_dir("restart");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    a.fold_section(Section::Palette, true);
    a.interface_size = Some(2);
    a.comp.onion_style.strength = 0;
    a.tick_settings();
    let mut b = app(1);
    b.use_settings(SettingsFile::in_dir(&dir));
    let right = *b.workspaces.active().area(Side::Right);
    let open: Vec<bool> = right.slots().iter().map(|sl| sl.open).collect();
    assert_eq!(open, [true, false, true]);
    assert_eq!(b.interface_size, Some(2));
    assert_eq!(b.comp.onion_style.strength, 0);
    assert_eq!(b.message, None, "nothing to complain about");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn damaged_settings_never_stop_launch_or_touch_the_project() {
    let dir = temp_dir("damaged");
    let file = dir.join("settings.txt");
    fs::write(
        &file,
        b"\xff\xfe\x00garbage\r\nfold.layers = yes\r\ninterface_size = 77",
    )
    .expect("w");
    let mut a = app(1);
    let before = (a.editor.revision(), a.editor.history().undo_depth());
    a.use_settings(SettingsFile::in_dir(&dir));
    let right = *a.workspaces.active().area(Side::Right);
    assert!(
        !right.slots()[2].open,
        "the layers slot folded, as the old line said"
    );
    assert_eq!(a.interface_size, None);
    let after = (a.editor.revision(), a.editor.history().undo_depth());
    assert_eq!(after, before, "settings are outside undo");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nothing_is_written_until_a_setting_changes_and_then_once() {
    let dir = temp_dir("writes");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    a.tick_settings();
    assert!(
        !dir.join("settings.txt").exists(),
        "defaults are not written"
    );
    a.fold_section(Section::Layers, true);
    a.tick_settings();
    let written = fs::metadata(dir.join("settings.txt"))
        .expect("written")
        .modified();
    fs::remove_file(dir.join("settings.txt")).expect("removed");
    a.tick_settings();
    assert!(!dir.join("settings.txt").exists(), "no change, no write");
    assert!(written.is_ok());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_settings_folder_that_cannot_be_written_is_said_once() {
    let dir = temp_dir("blocked");
    // A folder where the new file is written first cannot be replaced by it.
    fs::create_dir_all(dir.join("settings.txt.new")).expect("dir");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    assert_eq!(a.message, None, "nothing to read yet");
    a.fold_section(Section::Color, true);
    a.tick_settings();
    assert!(a
        .message
        .as_ref()
        .is_some_and(|(m, _)| m.contains("could not be saved")));
    a.message = None;
    a.fold_section(Section::Color, false);
    a.tick_settings();
    assert_eq!(a.message, None, "said once");
    fs::remove_dir(dir.join("settings.txt.new")).expect("unblocked");
    a.fold_section(Section::Color, true);
    a.tick_settings();
    assert!(dir.join("settings.txt").exists(), "saved once it can be");
    fs::create_dir_all(dir.join("settings.txt.new")).expect("dir");
    a.fold_section(Section::Color, false);
    a.tick_settings();
    assert!(
        a.message.is_some(),
        "a new failure after a success is said again"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_changed_shortcut_alone_is_saved_and_read_back() {
    use crate::shortcuts::{Action, Binding};
    let dir = temp_dir("keys");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    let j = Binding {
        letter: b'F',
        ctrl: false,
        shift: false,
    };
    a.set_binding(Action::Fill, Some(j));
    a.tick_settings();
    let mut b = app(1);
    b.use_settings(SettingsFile::in_dir(&dir));
    assert_eq!(b.keymap.binding(Action::Fill), Some(j));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_settings_file_that_cannot_be_used_is_set_aside_not_written_over() {
    let dir = temp_dir("set-aside");
    let file = dir.join("settings.txt");
    let huge = vec![b'x'; 70_000];
    fs::write(&file, &huge).expect("w");
    let walk = saved_sprite(&dir, "walk.piks");
    let mut a = App::launch(Some(walk), None, None);
    a.use_settings(SettingsFile::in_dir(&dir));
    assert!(a
        .message
        .as_ref()
        .is_some_and(|(m, _)| m.contains("kept as settings.txt.bad")));
    a.tick_settings();
    assert_eq!(fs::read(dir.join("settings.txt.bad")).expect("kept"), huge);
    assert!(
        file.exists(),
        "the file opened at launch is remembered anew"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_settings_file_held_by_another_program_is_never_written_over() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = temp_dir("held");
    let file = dir.join("settings.txt");
    fs::write(&file, "fold.layers = yes\r\nkey.save = Ctrl W\r\n").expect("w");
    // No sharing at all, as a program holding the file open for itself would.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .expect("held");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    assert!(a
        .message
        .as_ref()
        .is_some_and(|(m, _)| m.contains("not kept this time")));
    a.fold_section(Section::Color, true);
    a.tick_settings();
    drop(held);
    let text = fs::read_to_string(&file).expect("still there");
    assert!(
        text.contains("key.save = Ctrl W"),
        "the person's keys survive"
    );
    assert!(!dir.join("settings.txt.bad").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn opened_and_saved_files_join_the_recent_files_newest_first() {
    let dir = temp_dir("recent");
    let walk = saved_sprite(&dir, "walk.piks");
    let run = saved_sprite(&dir, "run.piks");
    let mut a = App::launch(Some(walk.clone()), None, None);
    a.use_settings(SettingsFile::in_dir(&dir));
    assert_eq!(a.recent, vec![walk.clone()], "the file opened at launch");
    let window = pikslar_platform::Window::detached();
    assert_eq!(a.open_now(window, run.clone()), crate::session::Opened::Now);
    assert_eq!(a.recent, vec![run.clone(), walk.clone()]);
    a.tick_settings();
    let mut b = app(1);
    b.use_settings(SettingsFile::in_dir(&dir));
    assert_eq!(b.recent, vec![run.clone(), walk.clone()]);
    let labels: Vec<_> = b.open_items().into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels[0], "Open a file...  Ctrl O");
    assert!(labels[1].starts_with("run.piks  in "));
    b.open_menu_action(2, window);
    b.wait_for_open(window);
    assert_eq!(b.path.as_deref(), Some(walk.as_path()));
    assert_eq!(b.recent[0], walk);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_recent_file_that_is_gone_leaves_the_list() {
    let dir = temp_dir("gone");
    let walk = saved_sprite(&dir, "walk.piks");
    let mut a = app(1);
    let damaged = dir.join("damaged.piks");
    fs::write(&damaged, b"not a sprite").expect("w");
    a.recent = vec![dir.join("missing.piks"), damaged.clone(), walk.clone()];
    a.open_menu_action(1, pikslar_platform::Window::detached());
    a.wait_for_open(pikslar_platform::Window::detached());
    assert_eq!(a.recent, vec![damaged.clone(), walk.clone()]);
    a.open_menu_action(1, pikslar_platform::Window::detached());
    a.wait_for_open(pikslar_platform::Window::detached());
    assert_eq!(a.recent, vec![damaged, walk], "one that is there stays");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_open_menu_says_when_there_are_no_recent_files() {
    let a = app(1);
    let items = a.open_items();
    assert_eq!(items.len(), 3, "open, none yet, and placing a reference");
    assert_eq!(items[1], (Cow::Borrowed(NONE_YET), false));
}

#[test]
fn long_recent_names_are_shortened_and_any_script_is_kept() {
    let label = recent_label(Path::new("C:\\art\\漢字 walk.piks"));
    assert_eq!(label, "漢字 walk.piks  in art");
    let long = recent_label(Path::new(&format!("C:\\art\\{}.piks", "x".repeat(60))));
    assert_eq!(long.chars().count(), LONGEST_NAME);
    assert!(
        long.contains("...") && long.ends_with(".piks  in art"),
        "{long}"
    );
}

#[test]
fn two_long_names_that_differ_near_the_end_stay_apart_and_keep_their_extension() {
    let a = format!("{}1", "n".repeat(59));
    let b = format!("{}2", "n".repeat(59));
    let (la, lb) = (
        recent_label(Path::new(&format!("C:/art/{a}.piks"))),
        recent_label(Path::new(&format!("C:/art/{b}.piks"))),
    );
    assert_ne!(la, lb, "the end of the name tells them apart");
    for label in [&la, &lb] {
        assert!(label.chars().count() <= LONGEST_NAME, "{label}");
        assert!(
            label.contains("...") && (label.contains("1.piks") || label.contains("2.piks")),
            "{label}"
        );
        assert!(label.ends_with("  in art"), "the folder stays: {label}");
    }
}

#[test]
fn a_long_folder_is_shortened_in_the_middle_and_the_name_survives() {
    let path = format!("C:/{}/walk.piks", "f".repeat(50));
    let label = recent_label(Path::new(&path));
    assert!(label.starts_with("walk.piks  in "), "{label}");
    assert!(label.contains("..."), "{label}");
    assert!(label.chars().count() <= LONGEST_NAME);
}

#[test]
fn the_open_button_opens_its_menu() {
    let mut a = app(1);
    a.open_file_menu((10, 30));
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::File));
}

#[test]
fn a_dropped_file_opens_and_only_the_first_of_several() {
    let dir = temp_dir("drop");
    let walk = saved_sprite(&dir, "walk.piks");
    let run = saved_sprite(&dir, "run.piks");
    let mut a = app(1);
    let window = pikslar_platform::Window::detached();
    a.files_dropped(std::slice::from_ref(&walk), window);
    a.wait_for_open(pikslar_platform::Window::detached());
    assert_eq!(a.path.as_deref(), Some(walk.as_path()));
    assert_eq!(a.message, None, "one file, nothing to explain");
    a.files_dropped(&[run.clone(), walk.clone()], window);
    a.wait_for_open(pikslar_platform::Window::detached());
    assert_eq!(a.path.as_deref(), Some(run.as_path()));
    assert!(a
        .message
        .as_ref()
        .is_some_and(|(m, _)| m.contains("one file at a time")));
    a.message = None;
    a.files_dropped(&[dir.join("missing.piks"), walk.clone()], window);
    a.wait_for_open(pikslar_platform::Window::detached());
    assert_eq!(a.message, None, "nothing opened, so nothing claims it did");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_drop_under_an_open_dialog_leaves_the_document_alone() {
    let dir = temp_dir("drop-dialog");
    let walk = saved_sprite(&dir, "walk.piks");
    let mut a = app(1);
    a.open_new_dialog();
    a.files_dropped(&[walk], pikslar_platform::Window::detached());
    a.wait_for_open(pikslar_platform::Window::detached());
    assert_eq!(a.path, None);
    assert!(a.new_dialog.is_some());
    assert!(a
        .message
        .as_ref()
        .is_some_and(|(m, _)| m.contains("Close the dialog")));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_drop_closes_an_open_menu_and_opens_the_file() {
    let dir = temp_dir("drop-menu");
    let walk = saved_sprite(&dir, "walk.piks");
    let mut a = app(1);
    a.open_file_menu((10, 30));
    a.files_dropped(
        std::slice::from_ref(&walk),
        pikslar_platform::Window::detached(),
    );
    a.wait_for_open(pikslar_platform::Window::detached());
    assert_eq!(a.menu, None);
    assert_eq!(a.path.as_deref(), Some(walk.as_path()));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn settings_that_cannot_be_read_are_reported_as_an_error_that_stays() {
    let dir = temp_dir("unreadable");
    // A folder where the file should be cannot be read as one.
    fs::create_dir_all(dir.join("settings.txt")).expect("folder");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    let said = a.message.as_ref().map(|(text, _)| text.as_str());
    assert!(
        said.is_some_and(|t| t.starts_with("Error: Settings could not be read")),
        "{said:?}"
    );
    assert!(a.message_is_error);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn crash_recovery_being_off_is_reported_as_an_error_that_stays() {
    let mut a = app(1);
    a.recovery_off("no room".to_string());
    assert_eq!(
        a.message.as_ref().map(|(t, _)| t.as_str()),
        Some("Error: Crash recovery is off: no room")
    );
    assert!(a.message_is_error);
}

#[test]
fn workspace_lines_survive_saving_another_setting() {
    let dir = temp_dir("workspace");
    let file = dir.join("settings.txt");
    fs::write(
        &file,
        "workspace = palette\r\nworkspace.custom.name = Shading\r\ntimeline.names = 210\r\n",
    )
    .expect("w");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    a.fold_section(Section::Palette, true);
    a.tick_settings();
    let text = fs::read_to_string(&file).expect("r");
    assert!(text.contains("workspace = palette"), "{text}");
    assert!(text.contains("workspace.custom.name = Shading"), "{text}");
    assert!(text.contains("timeline.names = 210"), "{text}");
    assert!(
        text.contains("dock.palette.right = 320 open | 0 closed 0 palette"),
        "{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}
