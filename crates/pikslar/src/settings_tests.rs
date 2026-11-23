use super::*;

/// Whether each slot of Draw's right dock is open.
fn draw_open(s: &Settings) -> Vec<bool> {
    let ws = s.workspaces.layout(Preset::Draw).unwrap_or_default();
    let area = *ws.area(crate::workspace::model::Side::Right);
    area.slots().iter().map(|sl| sl.open).collect()
}

fn chosen() -> Settings {
    let mut s = Settings {
        interface_size: Some(2),
        large_text: Some(true),
        onion: OnionStyle {
            past: false,
            future: true,
            strength: 4,
            colors: OnionColors::Own,
        },
        recent: Vec::new(),
        keys: Keymap::default(),
        script: pikslar_bind::ScriptLimits {
            call_depth: 40,
            ..pikslar_bind::ScriptLimits::DEFAULT
        },
        rewind: true,
        no_diagnostics: true,
        workspaces: crate::workspace::model::Workspaces::default()
            .with_names(200)
            .save_custom("Shading"),
    };
    s.keys.bind(
        crate::shortcuts::Action::Fill,
        crate::shortcuts::parse_binding("B").flatten(),
    );
    s.keys.bind(crate::shortcuts::Action::Save, None);
    remember(&mut s.recent, Path::new("C:\\art\\walk.piks"));
    remember(&mut s.recent, Path::new("D:\\Æøå 漢\\run cycle.piks"));
    s
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-settings-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn settings_read_back_as_written() {
    let s = chosen();
    assert_eq!(Settings::parse(&s.to_text()), s);
    assert_eq!(
        Settings::parse(&Settings::default().to_text()),
        Settings::default()
    );
}

#[test]
fn the_workspace_lines_are_kept_and_a_default_file_gains_none() {
    let text = chosen().to_text();
    assert!(text.contains("workspace = custom"), "{text}");
    assert!(text.contains("workspace.custom.name = Shading"), "{text}");
    assert!(!Settings::default().to_text().contains("workspace"));
    assert!(!Settings::default().to_text().contains("dock."));
    let damaged = Settings::parse(
        "workspace = tiles
dock.tiles.right = nonsense
",
    );
    assert_eq!(
        damaged.workspaces.active(),
        crate::workspace::presets::default_layout(crate::workspace::model::Preset::Tiles)
    );
}

#[test]
fn an_empty_or_foreign_file_gives_the_defaults() {
    for text in [
        "",
        "\u{0}\u{1}garbage",
        "{\"json\": true}",
        "interface_size",
    ] {
        assert_eq!(Settings::parse(text), Settings::default(), "{text:?}");
    }
}

#[test]
fn a_byte_order_mark_does_not_hide_the_first_setting() {
    let s = Settings::parse("\u{feff}fold.layers = yes\r\n");
    assert_eq!(draw_open(&s), [true, true, false]);
}

#[test]
fn a_damaged_line_keeps_its_default_and_the_rest_are_read() {
    let text = "interface_size = 9\r\nfold.color = maybe\r\nfold.layers = yes\r\n\
                onion.strength = 99\r\nonion.colors = plaid\r\nonion.future = no\r\n\
                recent = \r\nrecent = a.piks\r\nrecent = a.piks\r\nnonsense = 1\r\n";
    let s = Settings::parse(text);
    assert_eq!(s.interface_size, None);
    assert_eq!(draw_open(&s), [true, true, false]);
    let default_onion = OnionStyle::default();
    assert_eq!(s.onion.strength, default_onion.strength);
    assert_eq!(s.onion.colors, default_onion.colors);
    assert!(!s.onion.future && s.onion.past);
    assert_eq!(
        s.recent,
        vec![PathBuf::from("a.piks")],
        "no blanks, no repeats"
    );
}

#[test]
fn the_onion_skin_never_comes_back_hiding_both_sides() {
    let s = Settings::parse("onion.past = no\nonion.future = no\nonion.strength = 0\n");
    assert!(s.onion.past && s.onion.future);
    assert_eq!(s.onion.strength, 0, "the rest is kept");
}

#[test]
fn recent_files_are_newest_first_without_repeats_and_limited() {
    let mut s = Settings::default();
    for n in 0..12 {
        remember(&mut s.recent, Path::new(&format!("{n}.piks")));
    }
    remember(&mut s.recent, Path::new("5.piks"));
    assert_eq!(s.recent.len(), MOST_RECENT);
    assert_eq!(s.recent[0], PathBuf::from("5.piks"));
    assert_eq!(s.recent[1], PathBuf::from("11.piks"));
    assert_eq!(
        s.recent
            .iter()
            .filter(|p| **p == Path::new("5.piks"))
            .count(),
        1
    );
    let many: String = (0..20).map(|n| format!("recent = {n}.piks\n")).collect();
    assert_eq!(Settings::parse(&many).recent.len(), MOST_RECENT);
}

#[test]
fn the_file_survives_a_restart_and_a_missing_one_is_no_problem() {
    let dir = temp_dir("restart");
    let file = SettingsFile::in_dir(&dir);
    assert_eq!(file.load(), (Settings::default(), None), "never written");
    file.save(&chosen()).expect("saved");
    assert_eq!(file.load(), (chosen(), None));
    assert!(
        !dir.join("settings.txt.new").exists(),
        "nothing left beside it"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_huge_or_unreadable_file_is_reported_and_ignored() {
    let dir = temp_dir("huge");
    fs::create_dir_all(&dir).expect("dir");
    fs::write(dir.join(FILE_NAME), vec![b'x'; 70_000]).expect("written");
    let (s, problem) = SettingsFile::in_dir(&dir).load();
    assert_eq!(s, Settings::default());
    assert!(problem.is_some_and(|p| p.contains("too large")));
    // A folder where the file should be cannot be read as one.
    let odd = temp_dir("folder");
    fs::create_dir_all(odd.join(FILE_NAME)).expect("dir");
    let (s, problem) = SettingsFile::in_dir(&odd).load();
    assert_eq!(s, Settings::default());
    assert!(problem.is_some());
    assert!(SettingsFile::in_dir(&odd).save(&chosen()).is_err());
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&odd);
}

#[test]
fn a_disabled_file_reads_defaults_and_writes_nothing() {
    let file = SettingsFile::disabled();
    assert!(!file.is_enabled());
    assert_eq!(file.load(), (Settings::default(), None));
    assert!(file.save(&chosen()).is_ok());
}

#[test]
fn folds_of_older_settings_fold_draws_slots_unless_the_docks_were_saved() {
    let s = Settings::parse("fold.palette = yes\r\nfold.color = no\r\n");
    assert_eq!(draw_open(&s), [true, false, true]);
    let written = s.to_text();
    assert!(!written.contains("fold."), "{written}");
    let again = Settings::parse(&format!("{written}fold.color = yes\r\n"));
    assert_eq!(draw_open(&again), [true, false, true], "the dock lines win");
}

#[test]
fn old_fold_lines_only_fold_the_four_old_sections() {
    let s = Settings::parse("fold.timeline = yes\r\nfold.slices = yes\r\n");
    let ws = s.workspaces.layout(Preset::Draw).unwrap_or_default();
    assert_eq!(ws, crate::workspace::presets::default_layout(Preset::Draw));
}

#[test]
fn diagnostics_are_on_unless_turned_off() {
    assert!(!Settings::default().no_diagnostics, "on by default");
    assert!(Settings::default().to_text().contains("diagnostics = yes"));
    assert!(Settings::parse("diagnostics = no\r\n").no_diagnostics);
    assert!(!Settings::parse("diagnostics = maybe\r\n").no_diagnostics);
    assert!(chosen().to_text().contains("diagnostics = no"));
}
