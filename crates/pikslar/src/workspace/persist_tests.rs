use super::*;
use crate::workspace::presets::default_panels;

/// Splits written lines back into `(name, value)` pairs, as the settings parser does.
fn pairs(lines: &[String]) -> Vec<(&str, &str)> {
    lines
        .iter()
        .filter_map(|l| l.split_once('='))
        .map(|(n, v)| (n.trim(), v.trim()))
        .collect()
}

fn read_text(text: &str) -> Workspaces {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    read(&pairs(&lines))
}

fn round_trip(w: &Workspaces) -> Workspaces {
    read(&pairs(&lines(w)))
}

fn tabs(w: &Workspace, side: Side) -> Vec<Vec<Panel>> {
    w.area(side)
        .slots()
        .iter()
        .map(|s| s.tabs.as_slice().to_vec())
        .collect()
}

/// Every panel at most once; every default panel of `preset` placed or hidden; hidden panels
/// unplaced and hideable; at most three slots, none empty; sizes in range.
fn assert_valid(w: &Workspace, preset: Preset, what: &str) {
    let mut seen = PanelSet::EMPTY;
    for side in Side::ALL {
        let a = w.area(side);
        let (lo, hi) = side.range();
        assert!(
            (lo..=hi).contains(&a.size),
            "{what}: {side:?} size {}",
            a.size
        );
        assert!(a.slots().len() <= MOST_SLOTS, "{what}: slots");
        for sl in a.slots() {
            assert!(!sl.tabs.is_empty(), "{what}: empty slot");
            assert!((0..=MOST_SLOT_SIZE).contains(&sl.size), "{what}: slot size");
            assert!(usize::from(sl.active) < sl.tabs.len(), "{what}: active tab");
            for &p in sl.tabs.as_slice() {
                assert!(!seen.contains(p), "{what}: {p:?} twice");
                seen = seen.with(p);
            }
        }
    }
    for p in default_panels(preset).iter() {
        assert!(
            seen.contains(p) || w.hidden.contains(p),
            "{what}: {p:?} lost"
        );
    }
    for p in w.hidden.iter() {
        assert!(p.can_hide() && !seen.contains(p), "{what}: hidden {p:?}");
    }
}

#[test]
fn the_defaults_write_nothing_and_read_back() {
    let d = Workspaces::default();
    assert!(lines(&d).is_empty());
    assert_eq!(read(&[]), d);
}

#[test]
fn every_default_layout_is_left_as_it_is_by_repair() {
    for p in Preset::BUILT_IN {
        assert_eq!(repair(default_layout(p), p), default_layout(p), "{p:?}");
    }
}

#[test]
fn changed_presets_and_the_custom_workspace_round_trip() {
    let d = Workspaces::default();
    let animate = default_layout(Preset::Animate)
        .with_area(
            Side::Right,
            default_layout(Preset::Animate)
                .area(Side::Right)
                .with_size(300),
        )
        .with_compact(false);
    let draw = default_layout(Preset::Draw)
        .with_area(
            Side::Right,
            Area::new(
                248,
                true,
                &[
                    Slot::new(150, &[Panel::Color]),
                    Slot::new(112, &[Panel::Palette]),
                    Slot {
                        active: 1,
                        open: false,
                        ..Slot::new(0, &[Panel::Layers, Panel::Lint])
                    },
                ],
            ),
        )
        .with_hidden(PanelSet::EMPTY.with(Panel::Slices));
    let w = d
        .with_layout(Preset::Animate, animate)
        .with_layout(Preset::Draw, draw)
        .with_names(250)
        .save_custom("Shading");
    let written = lines(&w);
    assert!(written.iter().any(|l| l == "workspace = custom"));
    assert!(written.iter().any(|l| l == "dock.draw.hidden = slices"));
    assert!(
        !written.iter().any(|l| l.starts_with("dock.tiles.")),
        "unchanged presets"
    );
    assert_eq!(round_trip(&w), w);
    let bytes: usize = written
        .iter()
        .filter(|l| l.starts_with("dock.custom") || l.starts_with("workspace.custom"))
        .map(|l| l.len() + 2)
        .sum();
    assert!(bytes < 400, "a full workspace is small: {bytes}");
}

#[test]
fn the_documented_example_reads() {
    let w = read_text(
        "workspace = draw\n\
         dock.draw.right = 248 open | 150 open 0 color | 112 open 0 palette | 0 open 0 layers slices lint\n\
         dock.draw.bottom = 28 open | 0 open 0 timeline\n\
         dock.draw.left = 0 closed\n\
         timeline.names = 176\n",
    );
    let draw = w.layout(Preset::Draw).expect("built in");
    assert_eq!(
        tabs(&draw, Side::Right),
        tabs(&default_layout(Preset::Draw), Side::Right)
    );
    assert_eq!(
        draw.area(Side::Left).size,
        Side::Left.range().0,
        "0 is clamped"
    );
    assert!(w.custom.is_none());
}

#[test]
fn a_panel_listed_twice_keeps_its_first_place() {
    let w = read_text(
        "dock.draw.right = 248 open | 150 open 0 color | 0 open 1 palette color layers\n\
         dock.draw.left = 240 open | 0 open 0 palette lint\n",
    );
    let draw = w.layout(Preset::Draw).expect("built in");
    // Palette was active in the second slot and stays active there after Color left it.
    let right = draw.area(Side::Right);
    assert_eq!(
        tabs(&draw, Side::Right)[..2],
        [vec![Panel::Color], vec![Panel::Palette, Panel::Layers]]
    );
    assert_eq!(right.slots()[1].active_panel(), Some(Panel::Palette));
    assert_eq!(tabs(&draw, Side::Left), [vec![Panel::Lint]]);
    assert_valid(&draw, Preset::Draw, "twice");
}

#[test]
fn a_slot_left_empty_by_a_repeat_is_removed() {
    let w = read_text("dock.draw.right = 248 open | 150 open 0 color | 112 open 0 color\n");
    let draw = w.layout(Preset::Draw).expect("built in");
    assert_eq!(
        draw.area(Side::Right).slots()[0].tabs.as_slice(),
        [Panel::Color]
    );
    assert_ne!(
        draw.area(Side::Right).slots()[1].tabs.as_slice(),
        [Panel::Color]
    );
    assert_valid(&draw, Preset::Draw, "empty");
}

#[test]
fn a_missing_panel_returns_to_its_default_area() {
    // Palette and Layers' slot are gone; Timeline's line lost its only slot.
    let w = read_text(
        "dock.draw.right = 248 open | 150 open 0 color\n\
         dock.draw.bottom = 28 open\n",
    );
    let draw = w.layout(Preset::Draw).expect("built in");
    assert_eq!(
        tabs(&draw, Side::Right),
        [
            vec![Panel::Color],
            vec![Panel::Palette],
            vec![Panel::Layers, Panel::Slices, Panel::Lint]
        ]
    );
    assert_eq!(tabs(&draw, Side::Bottom), [vec![Panel::Timeline]]);
    assert_valid(&draw, Preset::Draw, "missing");
}

#[test]
fn a_missing_panel_joins_the_slot_at_its_default_position() {
    let w = read_text(
        "dock.draw.right = 248 open | 0 open 0 lint | 0 open 0 slices | 0 open 0 color
",
    );
    let draw = w.layout(Preset::Draw).expect("built in");
    assert_eq!(
        tabs(&draw, Side::Right),
        [
            vec![Panel::Lint],
            vec![Panel::Slices, Panel::Palette],
            vec![Panel::Color, Panel::Layers]
        ]
    );
    assert_valid(&draw, Preset::Draw, "full");
}

#[test]
fn a_panel_hidden_by_hand_stays_hidden_but_color_and_timeline_never_hide() {
    let w = read_text(
        "dock.draw.right = 248 open | 150 open 0 color | 0 open 0 layers\n\
         dock.draw.hidden = palette slices color timeline\n\
         dock.draw.bottom = 28 open\n",
    );
    let draw = w.layout(Preset::Draw).expect("built in");
    assert!(draw.find(Panel::Palette).is_none());
    assert!(draw.find(Panel::Slices).is_none());
    assert!(draw.find(Panel::Lint).is_some(), "not hidden, so put back");
    assert!(draw.find(Panel::Timeline).is_some());
    assert_eq!(
        draw.hidden,
        PanelSet::EMPTY.with(Panel::Palette).with(Panel::Slices)
    );
    assert_valid(&draw, Preset::Draw, "hidden");
}

#[test]
fn a_hidden_panel_that_is_placed_is_not_hidden() {
    let w = read_text("dock.draw.hidden = palette lint\n");
    let draw = w.layout(Preset::Draw).expect("built in");
    assert_eq!(draw.hidden, PanelSet::EMPTY);
    assert_eq!(draw, default_layout(Preset::Draw));
}

#[test]
fn unknown_panel_names_and_lines_are_dropped() {
    let w = read_text(
        "dock.draw.right = 248 open | 150 open 0 color brushes | 112 open 0 palette | 0 open 0 layers slices lint\n\
         dock.nowhere.right = 248 open\n\
         dock.draw.middle = 10 open\n\
         dock.draw.hidden = everything\n\
         workspace = sculpt\n",
    );
    assert_eq!(w, Workspaces::default());
}

#[test]
fn sizes_are_clamped() {
    let w = read_text(
        "dock.draw.right = 9000 open | -5 open 0 color | 99999 open 0 palette | 0 open 0 layers\n\
         dock.draw.bottom = 1 open | 0 open 0 timeline\n\
         timeline.names = 5\n",
    );
    let draw = w.layout(Preset::Draw).expect("built in");
    assert_eq!(draw.area(Side::Right).size, 400);
    assert_eq!(draw.area(Side::Right).slots()[0].size, 0);
    assert_eq!(draw.area(Side::Right).slots()[1].size, MOST_SLOT_SIZE);
    assert_eq!(draw.area(Side::Bottom).size, 28);
    assert_eq!(w.timeline_names, 120);
}

#[test]
fn an_active_tab_past_the_end_is_the_last_tab() {
    let w = read_text("dock.draw.left = 240 open | 0 open 9 history scripts\n");
    let draw = w.layout(Preset::Draw).expect("built in");
    assert_eq!(draw.area(Side::Left).slots()[0].active, 1);
}

#[test]
fn more_than_three_slots_merge_into_the_last() {
    let w = read_text(
        "dock.draw.left = 240 open | 0 open 0 history | 0 open 0 scripts | 0 open 0 tiles | 0 open 0 preview | 0 open 0 tiles\n",
    );
    let draw = w.layout(Preset::Draw).expect("built in");
    assert_eq!(
        tabs(&draw, Side::Left),
        [
            vec![Panel::History],
            vec![Panel::Scripts],
            vec![Panel::Tiles, Panel::Preview]
        ]
    );
}

#[test]
fn a_line_that_cannot_be_read_falls_back_to_the_preset_default() {
    let bad = [
        "248 open | 150 shut 0 color",
        "wide open",
        "248",
        "248 open extra | 0 open 0 color",
        "248 open | x open 0 color",
        "248 open | 0 open -1 color",
        "",
    ];
    for value in bad {
        let text =
            format!("dock.tiles.left = 240 open | 0 open 0 lint\ndock.tiles.right = {value}\n");
        let w = read_text(&text);
        let tiles = w.layout(Preset::Tiles).expect("built in");
        let d = default_layout(Preset::Tiles);
        assert_eq!(tiles.area(Side::Right), d.area(Side::Right), "{value:?}");
        assert_eq!(
            tabs(&tiles, Side::Left),
            [vec![Panel::Lint]],
            "the good line stays"
        );
    }
    let w = read_text("dock.animate.color = maybe\n");
    assert!(w.layout(Preset::Animate).expect("built in").color_compact);
}

#[test]
fn a_custom_workspace_never_saved_is_not_chosen() {
    let w = read_text("workspace = custom\n");
    assert_eq!(w.current, Preset::Draw);
    assert!(w.custom.is_none());
    let w = read_text("workspace = custom\ndock.custom.left = 240 open | 0 open 0 scripts\n");
    assert_eq!(w.current, Preset::Custom);
    let c = w.custom.as_ref().expect("custom");
    assert_eq!(c.name, "Custom");
    assert_valid(&c.layout, Preset::Custom, "custom");
    let w = read_text("workspace.custom.name = \u{7}  Night\u{1b} work  \n");
    assert_eq!(w.custom.expect("custom").name, "Night work");
}

#[test]
fn a_later_line_wins() {
    let w = read_text("workspace = tiles\nworkspace = palette\nworkspace = nonsense\n");
    assert_eq!(w.current, Preset::Palette);
}

#[test]
fn damaged_lines_always_give_a_valid_arrangement() {
    let words = [
        "0", "28", "248", "-3", "9999", "open", "closed", "|", "|", "color", "palette", "layers",
        "timeline", "history", "tiles", "preview", "lint", "slices", "scripts", "brush", "1", "2",
        "x",
    ];
    let fields = ["right", "bottom", "left", "color", "hidden"];
    let mut seed: u64 = 0x5eed_1234_abcd_0001;
    let mut next = |n: usize| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(seed >> 33).unwrap_or(0) % n
    };
    for round in 0..3000 {
        let preset = [
            Preset::Draw,
            Preset::Animate,
            Preset::Tiles,
            Preset::Palette,
            Preset::Custom,
        ][next(5)];
        let mut text = String::new();
        for _ in 0..1 + next(4) {
            let field = fields[next(fields.len())];
            let value: Vec<&str> = (0..next(16)).map(|_| words[next(words.len())]).collect();
            text.push_str(&format!(
                "dock.{}.{field} = {}\n",
                preset.word(),
                value.join(" ")
            ));
        }
        let w = read_text(&text);
        let layout = match preset {
            Preset::Custom => w.custom.as_ref().expect("custom").layout,
            p => w.layout(p).expect("built in"),
        };
        assert_valid(&layout, preset, &format!("round {round}: {text}"));
        assert_eq!(repair(layout, preset), layout, "repair is settled: {text}");
        assert_eq!(round_trip(&w), w, "round trip: {text}");
    }
}

#[test]
fn a_folded_timeline_alone_is_read_open() {
    let w = read_text("dock.draw.bottom = 28 open | 0 closed 0 timeline\n");
    assert!(w.active().area(Side::Bottom).slots()[0].open);
}

#[test]
fn a_panel_read_where_it_may_not_be_goes_back_to_its_default_area() {
    let w = read_text("dock.draw.right = 248 open | 0 open 0 timeline\n");
    let draw = w.active();
    assert_eq!(draw.find(Panel::Timeline).map(|f| f.0), Some(Side::Bottom));
    assert_valid(&draw, Preset::Draw, "timeline on the right");
    let w = read_text(
        "dock.tiles.bottom = 28 open | 0 open 0 timeline layers tiles\nworkspace = tiles\n",
    );
    let tiles = w.active();
    assert_eq!(tabs(&tiles, Side::Bottom), vec![vec![Panel::Timeline]]);
    assert_eq!(tiles.find(Panel::Layers).map(|f| f.0), Some(Side::Right));
    assert_eq!(tiles.find(Panel::Tiles).map(|f| f.0), Some(Side::Right));
    assert_valid(&tiles, Preset::Tiles, "layers and tiles below");
}
