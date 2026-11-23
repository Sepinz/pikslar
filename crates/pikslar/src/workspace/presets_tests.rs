use super::*;
use crate::workspace::model::{Tabs, MOST_SLOTS};

fn tabs(w: &Workspace, side: Side) -> Vec<Vec<Panel>> {
    w.area(side)
        .slots()
        .iter()
        .map(|s| s.tabs.as_slice().to_vec())
        .collect()
}

#[test]
fn draw_matches_the_preset_table() {
    use Panel::*;
    let w = default_layout(Preset::Draw);
    assert_eq!(
        tabs(&w, Side::Right),
        [vec![Color], vec![Palette], vec![Layers, Slices, Lint]]
    );
    assert_eq!(tabs(&w, Side::Bottom), [vec![Timeline]]);
    assert_eq!(w.area(Side::Bottom).size, TIMELINE_LINE);
    assert!(w.area(Side::Left).is_empty() && !w.area(Side::Left).open);
    assert_eq!(w.area(Side::Right).size, RIGHT_WIDTH);
    assert!(!w.color_compact);
}

#[test]
fn animate_tiles_and_palette_match_the_preset_table() {
    use Panel::*;
    let a = default_layout(Preset::Animate);
    assert_eq!(tabs(&a, Side::Right), [vec![Palette, Color], vec![Preview]]);
    assert_eq!(tabs(&a, Side::Bottom), [vec![Timeline, History]]);
    assert!(a.color_compact);
    assert_eq!(a.area(Side::Right).size, 232);
    assert!(a.area(Side::Bottom).size > TIMELINE_LINE + 6 * TIMELINE_ROW);
    let t = default_layout(Preset::Tiles);
    assert_eq!(
        tabs(&t, Side::Right),
        [vec![Tiles], vec![Palette, Color], vec![Layers]]
    );
    assert_eq!(
        t.area(Side::Right).slots()[0].size,
        0,
        "Tiles takes the rest"
    );
    let p = default_layout(Preset::Palette);
    assert_eq!(tabs(&p, Side::Right), [vec![Palette], vec![Color]]);
    assert_eq!(p.area(Side::Right).size, 320);
    assert_eq!(tabs(&p, Side::Left), [vec![Lint]]);
    assert!(!p.area(Side::Left).open, "Pixel Lint waits on its rail");
}

#[test]
fn every_preset_is_a_valid_arrangement() {
    for preset in Preset::BUILT_IN {
        let w = default_layout(preset);
        let mut seen = Vec::new();
        for side in Side::ALL {
            let area = w.area(side);
            let (lo, hi) = side.range();
            assert!((lo..=hi).contains(&area.size), "{preset:?} {side:?}");
            assert!(area.slots().len() <= MOST_SLOTS);
            for slot in area.slots() {
                assert!(!slot.tabs.is_empty());
                assert!(slot.active_panel().is_some());
                seen.extend_from_slice(slot.tabs.as_slice());
            }
        }
        let unique = seen.iter().fold(Tabs::default(), |t, &p| t.with(p));
        assert_eq!(unique.len(), seen.len(), "{preset:?}: no panel twice");
        for p in [Panel::Color, Panel::Timeline] {
            assert!(w.placed().contains(p), "{preset:?} shows {p:?}");
        }
        assert!(w.hidden.is_empty());
        assert_eq!(default_panels(preset), w.placed());
    }
    assert_eq!(default_layout(Preset::Custom), default_layout(Preset::Draw));
}
