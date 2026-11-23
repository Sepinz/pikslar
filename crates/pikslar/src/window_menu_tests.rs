//! The Window menu: panels shown and hidden, workspaces, reset, save and canvas only.

use super::*;
use crate::test_app::*;
use crate::workspace::model::{Panel, Preset};

/// The row whose words, before any key, are `word`.
fn row(a: &App, word: &str) -> usize {
    a.window_entries()
        .iter()
        .position(|(l, _)| l.split('\t').next() == Some(word))
        .unwrap_or_else(|| panic!("no row {word}: {:?}", a.window_entries()))
}

fn words(a: &App) -> Vec<String> {
    a.window_entries()
        .iter()
        .map(|(l, _)| l.to_string())
        .collect()
}

#[test]
fn every_panel_is_listed_with_its_state_in_words() {
    let a = app(1);
    let rows = a.window_entries();
    let all = words(&a);
    assert!(all.contains(&"Hide Palette".to_string()), "{all:?}");
    assert!(
        all.contains(&"Show Tiles".to_string()),
        "not placed in Draw"
    );
    assert!(all.contains(&"Color, always shown".to_string()));
    assert!(all.contains(&"Timeline, always shown".to_string()));
    let always: Vec<_> = rows
        .iter()
        .filter(|(l, _)| l.contains("always shown"))
        .collect();
    assert_eq!(always.len(), 2);
    assert!(always.iter().all(|(_, enabled)| !enabled));
}

#[test]
fn the_workspaces_say_which_is_in_use_and_show_their_keys() {
    let mut a = app(1);
    assert!(words(&a).contains(&"Draw workspace, in use\tCtrl 1".to_string()));
    assert!(words(&a).contains(&"Animate workspace\tCtrl 2".to_string()));
    let animate = row(&a, "Animate workspace");
    a.window_menu_action(animate);
    assert_eq!(a.workspaces.current, Preset::Animate);
    assert!(words(&a).contains(&"Animate workspace, in use\tCtrl 2".to_string()));
}

#[test]
fn a_hidden_panel_comes_back_by_the_menu_beside_its_old_neighbours() {
    let mut a = app(1);
    let home = a.workspaces.active();
    let hide = row(&a, "Hide Palette");
    a.window_menu_action(hide);
    assert!(!a.workspaces.active().placed().contains(Panel::Palette));
    assert!(a.workspaces.active().hidden.contains(Panel::Palette));
    let show = row(&a, "Show Palette");
    a.window_menu_action(show);
    let ws = a.workspaces.active();
    assert!(ws.placed().contains(Panel::Palette));
    assert!(!ws.hidden.contains(Panel::Palette));
    assert_eq!(ws.placed(), home.placed());
}

#[test]
fn a_panel_that_never_hides_cannot_be_chosen() {
    let mut a = app(1);
    let before = a.workspaces.active();
    let at = row(&a, "Color, always shown");
    a.window_menu_action(at);
    assert_eq!(a.workspaces.active(), before);
}

#[test]
fn reset_restores_the_preset_and_save_makes_the_custom_one_in_use() {
    let mut a = app(1);
    let home = a.workspaces.active();
    let hide = row(&a, "Hide Palette");
    a.window_menu_action(hide);
    let reset = row(&a, "Reset the Draw workspace");
    a.window_menu_action(reset);
    assert_eq!(a.workspaces.active(), home);
    let save = row(&a, "Save as custom workspace");
    a.window_menu_action(save);
    assert_eq!(a.workspaces.current, Preset::Custom);
    let custom = row(&a, "Custom workspace, in use");
    assert!(a.window_entries()[custom].1);
    let reset_custom = row(&a, "Reset the Custom workspace");
    assert!(
        !a.window_entries()[reset_custom].1,
        "a custom one has no default"
    );
}

#[test]
fn canvas_only_changes_its_words_and_its_menu_row_toggles_it() {
    let mut a = app(1);
    let at = row(&a, "Canvas only");
    a.window_menu_action(at);
    assert!(a.window_ui.canvas_only);
    let back = row(&a, "Show the docks");
    a.window_menu_action(back);
    assert!(!a.window_ui.canvas_only);
}

#[test]
fn a_panel_shown_goes_somewhere_allowed_even_when_its_old_dock_is_crowded() {
    let mut a = app(1);
    for word in [
        "Hide Layers",
        "Hide Palette",
        "Hide Slices",
        "Hide Pixel Lint",
    ] {
        let at = row(&a, word);
        a.window_menu_action(at);
    }
    let show = row(&a, "Show Layers");
    a.window_menu_action(show);
    assert!(a.workspaces.active().placed().contains(Panel::Layers));
}
