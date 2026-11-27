//! Pikslar: a lightweight pixel art editor and animation studio.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod about;
mod app;
#[cfg(test)]
mod ase_open_tests;
mod blend_menu;
mod brush_button;
mod brush_naming;
#[cfg(test)]
mod brush_naming_tests;
mod brush_store;
mod brush_thumbs;
mod brush_ui;
#[cfg(test)]
mod brush_ui_tests;
#[cfg(test)]
mod c1_failure_tests;
mod canvas;
mod canvas_dialog;
mod cel_ui;
#[cfg(test)]
mod cel_ui_tests;
mod color_ui;
#[cfg(test)]
mod command_tests;
#[cfg(test)]
mod composition_tests;
mod context_actions;
mod context_bar;
mod context_key;
#[cfg(test)]
mod context_key_tests;
mod context_tools;
#[cfg(test)]
mod crash_tests;
mod diag;
mod diag_crash;
mod diag_redact;
mod diag_ui;
mod diag_writer;
mod dock;
mod dock_focus;
#[cfg(test)]
mod dock_focus_tests;
mod dock_keys;
#[cfg(test)]
mod drag_tests;
mod export_dialog;
#[cfg(test)]
mod export_keys_tests;
mod export_map;
mod export_panel;
mod export_run;
#[cfg(test)]
mod export_tests;
mod fault;
#[cfg(test)]
mod feel_tests;
mod file_menu;
mod files;
mod focus_keys;
mod focus_space;
mod footprint;
mod footprint_shape;
mod frame_menu;
#[cfg(test)]
mod frame_menu_tests;
mod frame_runs;
#[cfg(test)]
mod frame_runs_tests;
#[cfg(test)]
mod frame_step_tests;
#[cfg(test)]
mod groups_ui_tests;
mod hints;
#[cfg(test)]
mod hints_tests;
#[cfg(test)]
mod ids_open_tests;
mod key_cursor;
mod keys;
mod keys_dialog;
#[cfg(test)]
mod keys_tests;
#[cfg(test)]
mod label_fit_tests;
mod large_text;
mod layer_naming;
#[cfg(test)]
mod layer_naming_tests;
mod layout;
#[cfg(test)]
mod layout_preset_tests;
#[cfg(test)]
mod layout_tests;
mod loading;
#[cfg(test)]
mod lock_tests;
mod look;
#[cfg(test)]
mod m5_review_tests;
#[cfg(test)]
mod main_focus_tests;
mod matrix_drag;
mod memory_dump;
mod menu_bar;
#[cfg(test)]
mod menu_keys_tests;
#[cfg(test)]
mod message_tests;
mod modal;
#[cfg(test)]
mod modified_tests;
mod name_fields;
#[cfg(test)]
mod name_fields_tests;
#[cfg(test)]
mod new_dialog_tests;
mod onion_menu;
#[cfg(test)]
mod onion_menu_tests;
mod orphan_backup;
mod outside;
#[cfg(test)]
mod p9_polish_tests;
mod palette_groups;
mod palette_io;
mod palette_menu;
mod palette_naming;
mod panel_bottom;
mod panel_fit;
mod panel_history;
mod panel_layers;
mod panel_lint;
mod panel_matrix;
#[cfg(test)]
mod panel_matrix_tests;
mod panel_palette;
mod panel_preview;
mod panel_right;
mod panel_side;
mod panel_tiles;
mod panel_timeline;
mod panels;
mod pending;
mod plugin_review;
mod plugin_ui;
#[cfg(test)]
mod polish_tests;
#[cfg(test)]
mod r1_tests;
mod rate_menu;
mod recovery;
#[cfg(test)]
mod recovery_tests;
mod reference_ui;
#[cfg(test)]
mod reference_ui_tests;
#[cfg(test)]
mod replay_tests;
mod rewind_link;
mod save_keys;
mod save_run;
mod save_state;
mod script_draw;
mod script_files;
mod script_panel;
mod scripting;
mod select_ui;
#[cfg(test)]
mod selection_fit_tests;
mod session;
mod settings;
mod settings_script;
mod settings_ui;
mod shortcuts;
mod slice_marks;
mod slice_ui;
#[cfg(test)]
mod slider_keys_tests;
mod slot_menu;
#[cfg(test)]
mod soak_b_study_tests;
#[cfg(test)]
mod stall_tests;
mod standing;
mod system_publish;
mod tag_menu;
#[cfg(test)]
mod target_size_tests;
mod terrain_menu;
mod terrain_ui;
#[cfg(test)]
mod test_app;
#[cfg(test)]
mod text_review_tests;
#[cfg(test)]
mod text_room_tests;
#[cfg(test)]
mod text_rotate_tests;
mod text_ui;
#[cfg(test)]
mod text_ui_tests;
mod tile_canvas;
mod tile_menu;
mod tile_naming;
#[cfg(test)]
mod tile_sheet_tests;
mod timeline;
mod timeline_actions;
#[cfg(test)]
mod timeline_form_tests;
mod timeline_geom;
#[cfg(test)]
mod timeline_scroll_tests;
#[cfg(test)]
mod timeline_tests;
mod tooltip;
#[cfg(test)]
mod tooltip_tests;
mod true_preview;
#[cfg(test)]
mod true_preview_tests;
mod tween_menu;
#[cfg(test)]
mod tween_menu_tests;
#[cfg(test)]
mod ui_timing_tests;
mod view_menu;
mod window_actions;
mod window_menu;
mod workspace;

use app::App;
use pikslar_platform::{run, WindowOptions};
use recovery::Recovery;
use settings::SettingsFile;
use std::path::PathBuf;

struct Args {
    file: Option<PathBuf>,
    size: Option<usize>,
    zoom: Option<i32>,
    recovery_dir: Option<PathBuf>,
    no_recovery: bool,
    /// Where Rewind keeps history instead of the usual folder.
    rewind_dir: Option<PathBuf>,
    /// Where settings are kept instead of the usual folder.
    settings_dir: Option<PathBuf>,
    /// Keeps no settings at all: measurement and test runs must neither see nor change the
    /// user's own.
    no_settings: bool,
    /// Keeps no crash notes and no saving log this session (`docs/diagnostics.md`).
    no_diagnostics: bool,
    /// A test asks for where each control is, written here after each change.
    /// A script run on the document once it is open, as one undo step.
    script: Option<PathBuf>,
    layout_dump: Option<PathBuf>,
    /// Measurement runs only: the document and history sizes written here once a second.
    memory_dump: Option<PathBuf>,
    /// Measurement runs only: an undo budget in KiB for the document opened at launch.
    undo_budget_kib: Option<usize>,
    /// Draws as if Windows high contrast were on with these colors, or off, whatever it says.
    look: Option<look::ForcedLook>,
    options: WindowOptions,
}

fn parse_args(raw: impl Iterator<Item = String>) -> Args {
    let values: Vec<String> = raw.collect();
    let mut args = Args {
        file: None,
        size: None,
        zoom: None,
        recovery_dir: None,
        no_recovery: false,
        rewind_dir: None,
        settings_dir: None,
        no_settings: false,
        no_diagnostics: false,
        script: None,
        layout_dump: None,
        memory_dump: None,
        undo_budget_kib: None,
        look: None,
        options: WindowOptions::default(),
    };
    let mut i = 0;
    while i < values.len() {
        let number = |at: usize| values.get(at).and_then(|v| v.parse::<i32>().ok());
        match values[i].as_str() {
            "--window" => {
                if let (Some(x), Some(y), Some(w), Some(h)) =
                    (number(i + 1), number(i + 2), number(i + 3), number(i + 4))
                {
                    args.options.placement = Some((x, y, w, h));
                }
                i += 4;
            }
            "--size" => {
                args.size = number(i + 1).and_then(|n| usize::try_from(n).ok());
                i += 1;
            }
            "--zoom" => {
                args.zoom = number(i + 1).filter(|z| (1..=64).contains(z));
                i += 1;
            }
            "--scale" => {
                args.options.scale = number(i + 1).filter(|n| (1..=4).contains(n));
                i += 1;
            }
            "--dpi" => {
                args.options.dpi = number(i + 1).filter(|n| (48..=960).contains(n));
                i += 1;
            }
            "--recovery-dir" => {
                args.recovery_dir = values.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--rewind-dir" => {
                args.rewind_dir = values.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--layout-dump" => {
                args.layout_dump = values.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--memory-dump" => {
                args.memory_dump = values.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--undo-budget-kib" => {
                args.undo_budget_kib = number(i + 1).and_then(|n| usize::try_from(n).ok());
                i += 1;
            }
            "--run-script" => {
                args.script = values.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--settings-dir" => {
                args.settings_dir = values.get(i + 1).map(PathBuf::from);
                i += 1;
            }
            "--high-contrast" => {
                args.look = values.get(i + 1).and_then(|v| look::parse_look(v));
                i += 1;
            }
            "--text-scale" => {
                args.options.text_scale = number(i + 1).filter(|n| (100..=225).contains(n));
                i += 1;
            }
            "--no-settings" => args.no_settings = true,
            "--no-diagnostics" => args.no_diagnostics = true,
            "--no-recovery" => args.no_recovery = true,
            "--topmost" => args.options.topmost = true,
            path => args.file = Some(PathBuf::from(path)),
        }
        i += 1;
    }
    args
}

/// A recovery folder that cannot be used is worth telling the artist about.
fn start_recovery(off: bool, dir: Option<PathBuf>) -> (Recovery, Option<String>) {
    if off {
        return (Recovery::disabled(), None);
    }
    let Some(dir) = dir.or_else(Recovery::default_dir) else {
        return (Recovery::disabled(), Some("no folder for it".to_string()));
    };
    match Recovery::try_in_dir(&dir) {
        Ok(recovery) => (recovery, None),
        Err(e) => (Recovery::disabled(), Some(e.to_string())),
    }
}

/// Where diagnostics live: beside the settings, or nowhere.
fn diagnostics_dir(off: bool, settings_dir: &Option<PathBuf>) -> Option<PathBuf> {
    match (off, settings_dir) {
        (true, _) => None,
        (false, Some(dir)) => Some(dir.clone()),
        (false, None) => SettingsFile::default_dir(),
    }
}

fn main() {
    about::exit_if_asked();
    // A file name that is not valid Unicode must not stop the program from starting.
    let mut args = parse_args(
        std::env::args_os()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned()),
    );
    // Measurement runs only (`spikes/limits.ps1`): times from input to present.
    use pikslar_platform::frame_log;
    args.options.frame_log = frame_log::path_from(std::env::var_os(frame_log::VAR));
    // First, so a panic from here on leaves a note. Never on disk without settings, and never
    // on before the persisted choice is known.
    let diag_off = args.no_settings || args.no_diagnostics;
    diag_ui::launch_diagnostics(diagnostics_dir(diag_off, &args.settings_dir));
    diag_crash::install();
    let offer_restore = args.file.is_none();
    // Characters the interface font lacks come from the system's fonts, the first time each
    // is drawn; nothing is done until then.
    pikslar_ui::fallback::set_source(Some(pikslar_platform::rasterize_glyph));
    let (recovery, problem) = start_recovery(args.no_recovery, args.recovery_dir);
    let mut app = App::launch(args.file, args.size, args.zoom);
    app.forced_look = args.look;
    app.follow_look();
    let settings = match (args.no_settings, args.settings_dir) {
        (true, _) => SettingsFile::disabled(),
        (false, Some(dir)) => SettingsFile::in_dir(&dir),
        (false, None) => SettingsFile::default_dir()
            .map_or_else(SettingsFile::disabled, |dir| SettingsFile::in_dir(&dir)),
    };
    app.text_percent = args
        .options
        .text_scale
        .unwrap_or_else(pikslar_platform::text_percent);
    app.use_settings(settings);
    app.use_diagnostics(args.no_diagnostics && !args.no_settings);
    app.use_rewind(args.rewind_dir);
    app.use_recovery(recovery, offer_restore);
    if let Some(path) = args.layout_dump {
        app.dump_layout_to(path);
    }
    if let Some(path) = args.memory_dump {
        app.dump_memory_to(path);
    }
    if let Some(kib) = args.undo_budget_kib {
        app.limit_undo_for_measurement(kib);
    }
    if let Some(script) = args.script {
        app.run_script_file(&script);
    }
    if let Some(problem) = problem {
        app.recovery_off(problem);
    }
    if let Err(message) = run("Pikslar", args.options, app) {
        eprintln!("{message}");
        std::process::exit(1);
    }
    diag::note(diag::Event::new("exit", "clean"));
    diag::flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(items: &[&str]) -> Args {
        parse_args(items.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_measurement_options() {
        let a = parse(&[
            "--window",
            "1",
            "2",
            "3",
            "4",
            "--size",
            "128",
            "--topmost",
            "a.piks",
        ]);
        assert_eq!(a.options.placement, Some((1, 2, 3, 4)));
        assert_eq!(a.size, Some(128));
        assert!(a.options.topmost);
        assert_eq!(a.layout_dump, None);
        assert_eq!(a.options.frame_log, None, "no frame log unless asked for");
        assert!(!a.no_settings && a.settings_dir.is_none());
        let c = parse(&[
            "--settings-dir",
            "kept",
            "--no-settings",
            "--rewind-dir",
            "r",
        ]);
        assert_eq!(c.rewind_dir, Some(PathBuf::from("r")));
        assert_eq!(c.settings_dir, Some(PathBuf::from("kept")));
        assert!(c.no_settings);
        assert!(!c.no_diagnostics);
        assert!(parse(&["--no-diagnostics"]).no_diagnostics);
        let s = parse(&["--run-script", "tidy.pks", "a.piks"]);
        assert_eq!(s.script, Some(PathBuf::from("tidy.pks")));
        assert_eq!(s.file, Some(PathBuf::from("a.piks")));
        assert_eq!(a.script, None);
        let l = parse(&["--high-contrast", "white", "--text-scale", "150"]);
        assert_eq!(l.look, Some(Some(pikslar_ui::theme::SystemColors::WHITE)));
        assert_eq!(l.options.text_scale, Some(150));
        let off = parse(&["--high-contrast", "off", "--text-scale", "999"]);
        assert_eq!(off.look, Some(None));
        assert_eq!(off.options.text_scale, None, "out of range");
        let b = parse(&["--layout-dump", "at.txt"]);
        assert_eq!(b.layout_dump, Some(PathBuf::from("at.txt")));
        assert_eq!(b.file, None);
        let m = parse(&["--memory-dump", "m.txt", "--undo-budget-kib", "1024"]);
        assert_eq!(m.memory_dump, Some(PathBuf::from("m.txt")));
        assert_eq!(m.undo_budget_kib, Some(1024));
        assert_eq!((a.memory_dump.clone(), a.undo_budget_kib), (None, None));
        assert_eq!(parse(&["--undo-budget-kib", "-5"]).undo_budget_kib, None);
        assert_eq!(a.file, Some(PathBuf::from("a.piks")));
    }

    #[test]
    fn bad_numbers_are_ignored() {
        let a = parse(&["--size", "huge", "--window", "1"]);
        assert_eq!(a.size, None);
        assert_eq!(a.options.placement, None);
    }
}
