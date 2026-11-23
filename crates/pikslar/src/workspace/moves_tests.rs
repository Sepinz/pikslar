use super::*;
use crate::workspace::model::Preset;
use crate::workspace::presets::default_layout;

fn draw() -> Workspace {
    default_layout(Preset::Draw)
}

fn tabs(ws: &Workspace, side: Side) -> Vec<Vec<Panel>> {
    ws.area(side)
        .slots()
        .iter()
        .map(|s| s.tabs.as_slice().to_vec())
        .collect()
}

#[test]
fn the_palette_moves_to_the_closed_left_dock_and_opens_it() {
    let ws = move_panel(draw(), Panel::Palette, Target::NewSlot(Side::Left)).expect("moved");
    assert!(ws.area(Side::Left).open);
    assert_eq!(tabs(&ws, Side::Left), vec![vec![Panel::Palette]]);
    assert_eq!(
        tabs(&ws, Side::Right),
        vec![
            vec![Panel::Color],
            vec![Panel::Layers, Panel::Slices, Panel::Lint]
        ],
        "its slot went with it"
    );
    let back = move_panel(ws, Panel::Palette, Target::Beside(Side::Right, 0)).expect("moved");
    let first = back.area(Side::Right).slots()[0];
    assert_eq!(first.tabs.as_slice(), [Panel::Color, Panel::Palette]);
    assert_eq!(first.active_panel(), Some(Panel::Palette), "shown");
    assert!(back.area(Side::Left).is_empty());
}

#[test]
fn the_rules_keep_the_timeline_below_and_layers_and_tiles_out_of_it() {
    let ws = draw();
    assert_eq!(
        move_panel(ws, Panel::Timeline, Target::NewSlot(Side::Right)),
        None
    );
    assert_eq!(
        move_panel(ws, Panel::Layers, Target::NewSlot(Side::Bottom)),
        None
    );
    assert_eq!(
        move_panel(ws, Panel::Tiles, Target::Beside(Side::Bottom, 0)),
        None
    );
    assert!(move_panel(ws, Panel::Palette, Target::Beside(Side::Bottom, 0)).is_some());
}

#[test]
fn a_move_that_changes_nothing_or_finds_no_room_is_refused() {
    let ws = draw();
    assert_eq!(
        move_panel(ws, Panel::Color, Target::Beside(Side::Right, 0)),
        None
    );
    assert_eq!(
        move_panel(ws, Panel::Color, Target::NewSlot(Side::Right)),
        None
    );
    assert_eq!(
        move_panel(ws, Panel::Timeline, Target::NewSlot(Side::Bottom)),
        None,
        "alone in its slot"
    );
    let ws = move_panel(ws, Panel::Lint, Target::NewSlot(Side::Left)).expect("moved");
    assert_eq!(
        move_panel(ws, Panel::Slices, Target::NewSlot(Side::Right)),
        None,
        "three slots already"
    );
}

#[test]
fn a_tab_taken_from_a_new_slot_in_its_own_area_and_a_later_target_moves_up() {
    let ws = draw();
    let ws = move_panel(ws, Panel::Color, Target::Beside(Side::Right, 2)).expect("moved");
    assert_eq!(
        tabs(&ws, Side::Right),
        vec![
            vec![Panel::Palette],
            vec![Panel::Layers, Panel::Slices, Panel::Lint, Panel::Color]
        ]
    );
    let ws = move_panel(ws, Panel::Lint, Target::NewSlot(Side::Right)).expect("moved");
    assert_eq!(tabs(&ws, Side::Right)[2], vec![Panel::Lint]);
    let second = ws.area(Side::Right).slots()[1];
    assert_eq!(second.active_panel(), Some(Panel::Color), "kept on Color");
}

#[test]
fn hiding_keeps_color_and_the_timeline_and_moving_shows_a_hidden_panel_again() {
    let ws = draw();
    assert_eq!(hide_panel(ws, Panel::Color), None);
    assert_eq!(hide_panel(ws, Panel::Timeline), None);
    let ws = hide_panel(ws, Panel::Slices).expect("hidden");
    assert!(ws.hidden.contains(Panel::Slices));
    assert_eq!(ws.find(Panel::Slices), None);
    let layers = ws.area(Side::Right).slots()[2];
    assert_eq!(layers.tabs.as_slice(), [Panel::Layers, Panel::Lint]);
    assert_eq!(layers.active_panel(), Some(Panel::Layers));
    let ws = move_panel(ws, Panel::Slices, Target::Beside(Side::Right, 2)).expect("placed");
    assert!(!ws.hidden.contains(Panel::Slices));
}

#[test]
fn hiding_the_shown_tab_shows_the_one_before() {
    let ws = draw();
    let ws = crate::dock::with_slot(ws, Side::Right, 2, |sl| Slot { active: 2, ..sl });
    let ws = hide_panel(ws, Panel::Lint).expect("hidden");
    let slot = ws.area(Side::Right).slots()[2];
    assert_eq!(slot.active_panel(), Some(Panel::Slices));
}

#[test]
fn a_folded_slot_left_with_the_timeline_alone_opens_again() {
    let ws = default_layout(Preset::Animate);
    let ws = crate::dock::with_slot(ws, Side::Bottom, 0, |sl| Slot { open: false, ..sl });
    let hidden = hide_panel(ws, Panel::History).expect("hidden");
    assert!(hidden.area(Side::Bottom).slots()[0].open, "hidden");
    let moved = move_panel(ws, Panel::History, Target::NewSlot(Side::Left)).expect("moved");
    assert!(moved.area(Side::Bottom).slots()[0].open, "moved");
}
