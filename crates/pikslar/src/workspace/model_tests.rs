use super::*;

#[test]
fn every_panel_has_a_word_that_reads_back() {
    for p in Panel::ALL {
        assert_eq!(Panel::from_word(p.word()), Some(p));
    }
    assert_eq!(Panel::from_word("Color"), None, "words are lower case");
    assert_eq!(Panel::from_word("brushes"), None);
    for p in Preset::BUILT_IN.into_iter().chain([Preset::Custom]) {
        assert_eq!(Preset::from_word(p.word()), Some(p));
    }
}

#[test]
fn least_sizes_follow_the_panel_table() {
    assert_eq!(Panel::Color.least(), 150);
    assert_eq!(COLOR_COMPACT, 56);
    assert_eq!(Panel::Palette.least(), 64);
    assert_eq!(Panel::Layers.least(), 110);
    assert_eq!(Panel::Timeline.least(), 28);
    assert_eq!(Panel::History.least(), 28);
    assert_eq!(Panel::Tiles.least(), 120);
    assert_eq!(Panel::Preview.least(), 80);
    assert_eq!(Panel::Lint.least(), 60);
    assert_eq!(Panel::Slices.least(), 60);
    assert_eq!(Panel::Scripts.least(), 80);
}

#[test]
fn only_color_and_timeline_cannot_hide() {
    let fixed: Vec<Panel> = Panel::ALL.into_iter().filter(|p| !p.can_hide()).collect();
    assert_eq!(fixed, [Panel::Color, Panel::Timeline]);
}

#[test]
fn the_fold_order_is_the_documented_one() {
    let mut foldable: Vec<Panel> = Panel::ALL
        .into_iter()
        .filter(|p| p.fold_rank().is_some())
        .collect();
    foldable.sort_by_key(|p| p.fold_rank());
    use Panel::*;
    assert_eq!(
        foldable,
        [Lint, Slices, Preview, Palette, History, Tiles, Scripts]
    );
    for p in [Color, Layers, Timeline] {
        assert_eq!(p.fold_rank(), None, "{p:?} never folds away");
    }
}

#[test]
fn panel_sets_hold_each_panel_once() {
    let s = PanelSet::EMPTY
        .with(Panel::Lint)
        .with(Panel::Lint)
        .with(Panel::Color);
    assert_eq!(s.iter().collect::<Vec<_>>(), [Panel::Color, Panel::Lint]);
    assert!(s.without(Panel::Lint).contains(Panel::Color));
    assert!(!s.without(Panel::Lint).contains(Panel::Lint));
    assert!(PanelSet::EMPTY.is_empty());
}

#[test]
fn tabs_keep_order_and_drop_repeats() {
    let t = Tabs::of(&[Panel::Layers, Panel::Slices, Panel::Layers, Panel::Lint]);
    assert_eq!(t.as_slice(), [Panel::Layers, Panel::Slices, Panel::Lint]);
    assert_eq!(
        t.without(Panel::Slices).as_slice(),
        [Panel::Layers, Panel::Lint]
    );
    assert!(t
        .without(Panel::Layers)
        .without(Panel::Slices)
        .without(Panel::Lint)
        .is_empty());
    let all = Panel::ALL.into_iter().fold(Tabs::default(), Tabs::with);
    assert_eq!(all.len(), PANEL_COUNT);
}

#[test]
fn the_active_panel_falls_back_to_the_last_tab() {
    let mut s = Slot::new(0, &[Panel::Layers, Panel::Slices]);
    assert_eq!(s.active_panel(), Some(Panel::Layers));
    s.active = 9;
    assert_eq!(s.active_panel(), Some(Panel::Slices));
    assert_eq!(Slot::new(0, &[]).active_panel(), None);
}

#[test]
fn an_area_holds_at_most_three_slots() {
    let slots = [Slot::new(0, &[Panel::Lint]); 5];
    assert_eq!(Area::new(248, true, &slots).slots().len(), MOST_SLOTS);
    let a = Area::new(248, true, &slots[..2]);
    let b = a.with_slot(1, Slot::new(10, &[Panel::Color]));
    assert_eq!(b.slots()[1].size, 10);
    assert_eq!(
        a.with_slot(7, Slot::default()),
        a,
        "out of range changes nothing"
    );
}

#[test]
fn a_workspace_finds_its_panels() {
    let w = Workspaces::default().active();
    assert_eq!(w.find(Panel::Slices), Some((Side::Right, 2, 1)));
    assert_eq!(w.find(Panel::Timeline), Some((Side::Bottom, 0, 0)));
    assert_eq!(w.find(Panel::Scripts), None);
    assert!(w.placed().contains(Panel::Color));
}

#[test]
fn presets_keep_their_own_changes_and_reset_to_their_default() {
    let all = Workspaces::default();
    assert_eq!(all.current, Preset::Draw);
    let wide = all
        .active()
        .with_area(Side::Right, Area::new(400, true, &[]));
    let changed = all.with_layout(Preset::Draw, wide);
    assert_eq!(changed.active(), wide);
    let animate = changed.choose(Preset::Animate);
    assert_eq!(animate.current, Preset::Animate);
    assert_eq!(animate.active(), presets::default_layout(Preset::Animate));
    assert_eq!(
        animate.choose(Preset::Draw).active(),
        wide,
        "Draw kept its change"
    );
    assert_eq!(changed.reset(Preset::Draw), all);
}

#[test]
fn the_custom_workspace_exists_only_once_saved() {
    let all = Workspaces::default();
    assert_eq!(all.choose(Preset::Custom), all, "nothing to choose yet");
    assert_eq!(all.layout(Preset::Custom), None);
    let saved = all.choose(Preset::Animate).save_custom("  Shading\u{7}  ");
    assert_eq!(saved.current, Preset::Custom);
    let custom = saved.custom.clone().expect("saved");
    assert_eq!(custom.name, "Shading");
    assert_eq!(custom.layout, presets::default_layout(Preset::Animate));
    assert_eq!(
        saved.reset(Preset::Custom),
        saved,
        "a custom workspace has no default"
    );
    let again = saved.save_custom("Lines");
    assert_eq!(
        again.custom.map(|c| c.name),
        Some("Lines".to_string()),
        "still one"
    );
}

#[test]
fn names_are_cleaned_and_widths_clamped() {
    assert_eq!(clean_name(""), "Custom");
    assert_eq!(clean_name(" \r\n "), "Custom");
    assert_eq!(clean_name(&"x".repeat(80)).chars().count(), MOST_NAME_CHARS);
    assert_eq!(clean_name("Æøå 漢"), "Æøå 漢");
    let w = Workspaces::default();
    assert_eq!(w.with_names(5).timeline_names, NAMES_RANGE.0);
    assert_eq!(w.with_names(9999).timeline_names, NAMES_RANGE.1);
    assert_eq!(w.timeline_names, NAMES_DEFAULT);
}

use crate::workspace::presets;
