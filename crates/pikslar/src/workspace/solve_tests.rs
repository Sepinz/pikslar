use super::*;
use crate::workspace::model::{Preset, Tabs, PANEL_COUNT};
use crate::workspace::presets::default_layout;

/// The named screens of the layout suite at 1x, and the high-DPI ones.
const SCREENS: [(i32, i32, i32); 6] = [
    (1366, 705, 1),
    (1920, 1017, 1),
    (2560, 1377, 1),
    (640, 600, 1),
    (1920, 1003, 2),
    (3840, 2050, 3),
];

fn win(w: i32, h: i32, scale: i32) -> Window {
    Window { w, h, scale }
}

fn share(r: &Solved, w: i32, h: i32) -> f64 {
    f64::from(r.canvas.w * r.canvas.h) / f64::from(w * h)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: i32) -> i32 {
        (self.next() % n.max(1) as u64) as i32
    }

    fn chance(&mut self, in_ten: i32) -> bool {
        self.below(10) < in_ten
    }
}

/// Any arrangement the model can hold, sizes in and out of range included.
fn random_workspace(rng: &mut Rng) -> Workspace {
    let mut panels = Panel::ALL;
    for i in (1..PANEL_COUNT).rev() {
        panels.swap(i, rng.below(i as i32 + 1) as usize);
    }
    let placed = &panels[..rng.below(PANEL_COUNT as i32 + 1) as usize];
    let mut groups: [Vec<Vec<Panel>>; 3] = Default::default();
    for &p in placed {
        let side = rng.below(3) as usize;
        let slots = &mut groups[side];
        if slots.is_empty() || (slots.len() < MOST_SLOTS && rng.chance(5)) {
            slots.push(Vec::new());
        }
        let at = rng.below(slots.len() as i32) as usize;
        slots[at].push(p);
    }
    let mut ws = Workspace::default().with_compact(rng.chance(3));
    for side in Side::ALL {
        let slots: Vec<Slot> = groups[side.index()]
            .iter()
            .map(|tabs| Slot {
                size: if rng.chance(4) { 0 } else { rng.below(700) },
                open: rng.chance(8),
                active: rng.below(4) as u8,
                tabs: Tabs::of(tabs),
            })
            .collect();
        let size = rng.below(1200);
        ws = ws.with_area(side, Area::new(size, rng.chance(8), &slots));
    }
    ws
}

/// A window the program allows, with the automatic interface size.
fn random_window(rng: &mut Rng) -> Window {
    let (w, h) = (640 + rng.below(3400), 600 + rng.below(1800));
    let scale = (1..=3)
        .rev()
        .find(|s| w / s >= 640 && h / s >= 480)
        .unwrap_or(1);
    win(w, h, scale)
}

fn area_of(r: Rect) -> i64 {
    i64::from(r.w.max(0)) * i64::from(r.h.max(0))
}

fn overlaps(a: Rect, b: Rect) -> bool {
    !a.intersect(b).is_empty()
}

/// The regions lie inside the window, do not overlap and cover it; slots do the same inside
/// their area.
fn assert_tiles(r: &Solved, w: i32, h: i32, what: &str) {
    let window = Rect::new(0, 0, w, h);
    let regions = r.regions();
    for (i, a) in regions.iter().enumerate() {
        assert!(a.w >= 0 && a.h >= 0, "{what}: negative region {i} {a:?}");
        assert_eq!(
            a.intersect(window),
            *a,
            "{what}: region {i} {a:?} leaves the window"
        );
        for b in &regions[i + 1..] {
            assert!(!overlaps(*a, *b), "{what}: {a:?} overlaps {b:?}");
        }
    }
    let total: i64 = regions.iter().map(|&a| area_of(a)).sum();
    assert_eq!(total, i64::from(w) * i64::from(h), "{what}: gaps");
    for side in Side::ALL {
        let ar = r.area(side);
        if ar.slots().is_empty() {
            continue;
        }
        let sum: i64 = ar.slots().iter().map(|s| area_of(s.rect)).sum();
        assert_eq!(sum, area_of(ar.rect), "{what}: {side:?} slots leave gaps");
        for (i, a) in ar.slots().iter().enumerate() {
            assert_eq!(
                a.rect.intersect(ar.rect),
                a.rect,
                "{what}: slot outside {side:?}"
            );
            for b in &ar.slots()[i + 1..] {
                assert!(!overlaps(a.rect, b.rect), "{what}: {side:?} slots overlap");
            }
        }
    }
}

fn along(r: Rect, side: Side) -> i32 {
    if side == Side::Bottom {
        r.w
    } else {
        r.h
    }
}

/// Every shown slot keeps its least size, and the bottom dock its least height.
fn assert_least_kept(r: &Solved, what: &str) {
    let ws = r.effective;
    let s = r.menu.h / MENU;
    for side in Side::ALL {
        let ar = r.area(side);
        let area = ws.area(side);
        let axis = if side == Side::Bottom {
            Axis::Across
        } else {
            Axis::Down
        };
        let compact = ws.color_compact && holds_color(area);
        for (sr, slot) in ar.slots().iter().zip(area.slots()) {
            let lo = least(slot, compact, axis) * s;
            assert!(
                along(sr.rect, side) >= lo,
                "{what}: {side:?} {slot:?} under {lo}"
            );
        }
        if side == Side::Bottom && !ar.slots().is_empty() {
            let lo = area
                .slots()
                .iter()
                .map(|sl| least(sl, ws.color_compact, Axis::Down))
                .fold(TIMELINE_LINE, i32::max);
            assert!(ar.rect.h >= lo * s, "{what}: bottom under its least");
        }
    }
    assert!(
        r.canvas.w >= MIN_CANVAS && r.canvas.h >= MIN_CANVAS,
        "{what}: {:?}",
        r.canvas
    );
    assert!(
        r.tools.h >= TWO_COLUMN_NEED * s,
        "{what}: tool strip too short"
    );
}

/// What the solver folded, it folded in the documented order and only when it had to.
fn assert_folds_documented(input: &Workspace, r: &Solved, what: &str) {
    let out = r.effective;
    for side in Side::ALL {
        let (before, after) = (input.area(side), out.area(side));
        let mut folded_ranks = Vec::new();
        for (b, a) in before.slots().iter().zip(after.slots()) {
            if b.open && !a.open {
                let rank = b.active_panel().and_then(Panel::fold_rank);
                assert!(
                    rank.is_some(),
                    "{what}: {side:?} folded {b:?}, which never folds"
                );
                folded_ranks.extend(rank);
            }
        }
        let highest = folded_ranks.iter().copied().max();
        for a in after.slots().iter().filter(|a| a.open) {
            if let (Some(rank), Some(h)) = (a.active_panel().and_then(Panel::fold_rank), highest) {
                assert!(rank >= h, "{what}: {side:?} folded rank {h} before {rank}");
            }
        }
        if out.color_compact && !input.color_compact && holds_color(after) {
            assert!(
                next_fold(after).is_none(),
                "{what}: Color compacted too early"
            );
        }
    }
    assert_rails_documented(input, r, what);
}

fn least_width(a: &Area, side: Side) -> i32 {
    match (a.is_empty(), a.open) {
        (true, _) => 0,
        (false, false) => RAIL,
        (false, true) => side.range().0,
    }
}

fn assert_rails_documented(input: &Workspace, r: &Solved, what: &str) {
    let out = r.effective;
    let s = r.menu.h / MENU;
    let middle = r.canvas.h;
    let room = r.canvas.w + r.area(Side::Left).rect.w + r.area(Side::Right).rect.w;
    for side in [Side::Left, Side::Right] {
        let (b, a) = (input.area(side), out.area(side));
        if !(b.open && !b.is_empty() && !a.open) {
            continue;
        }
        let (folded, _) = fold_all(*b);
        let irreducible: i32 = folded
            .slots()
            .iter()
            .map(|sl| least(sl, true, Axis::Down))
            .sum();
        let other = if side == Side::Left {
            least_width(out.area(Side::Right), Side::Right)
                .min(asked_width(out.area(Side::Right), Side::Right))
        } else {
            asked_width(out.area(Side::Left), Side::Left)
        };
        let too_wide = room - (side.range().0 + other) * s < MIN_CANVAS;
        assert!(
            irreducible * s > middle || too_wide,
            "{what}: {side:?} became a rail without need"
        );
    }
}

/// The area with every foldable slot folded.
fn fold_all(a: Area) -> (Area, bool) {
    let mut a = a;
    while let Some(i) = next_fold(&a) {
        a = a.with_slot(
            i,
            Slot {
                open: false,
                ..a.slots()[i]
            },
        );
    }
    (a, true)
}

#[test]
fn every_preset_tiles_every_named_screen() {
    for preset in Preset::BUILT_IN {
        for (w, h, s) in SCREENS {
            let ws = default_layout(preset);
            let r = solve(&ws, win(w, h, s));
            let what = format!("{preset:?} {w}x{h}@{s}");
            assert_tiles(&r, w, h, &what);
            assert_least_kept(&r, &what);
            assert_folds_documented(&ws, &r, &what);
        }
    }
}

#[test]
fn the_canvas_share_stays_above_the_limits() {
    let draw = default_layout(Preset::Draw);
    for (w, h, limit) in [(1366, 705, 0.62), (1920, 1017, 0.72), (2560, 1377, 0.78)] {
        let got = share(&solve(&draw, win(w, h, 1)), w, h);
        assert!(got >= limit, "Draw {w}x{h}: {got:.3} under {limit}");
    }
    let animate = solve(&default_layout(Preset::Animate), win(1366, 705, 1));
    let got = share(&animate, 1366, 705);
    assert!((0.52..0.56).contains(&got), "Animate: {got:.3}");
    assert_eq!(animate.tool_columns, 1);
}

#[test]
fn draw_at_1366_by_705_is_laid_out_as_drawn() {
    let r = solve(&default_layout(Preset::Draw), win(1366, 705, 1));
    assert_eq!(r.menu, Rect::new(0, 0, 1366, 24));
    assert_eq!(r.context, Rect::new(0, 24, 1366, 28));
    assert_eq!(r.tools, Rect::new(0, 52, 36, 605));
    assert_eq!(r.area(Side::Right).rect, Rect::new(1118, 52, 248, 605));
    assert_eq!(r.area(Side::Bottom).rect, Rect::new(0, 657, 1366, 28));
    assert_eq!(r.status, Rect::new(0, 685, 1366, 20));
    assert_eq!(r.area(Side::Left).rect.w, 0, "empty and closed");
    let heights: Vec<i32> = r
        .area(Side::Right)
        .slots()
        .iter()
        .map(|s| s.rect.h)
        .collect();
    assert_eq!(
        heights,
        [150, 112, 605 - 262],
        "the last unsized slot takes the rest"
    );
    assert_eq!(
        r.effective,
        default_layout(Preset::Draw),
        "nothing needed changing"
    );
}

#[test]
fn a_short_window_folds_slots_in_order_then_compacts_color() {
    let ws = default_layout(Preset::Draw).with_area(
        Side::Bottom,
        Area::new(28, true, &[Slot::new(0, &[Panel::Timeline])]),
    );
    // 2x on 1280 by 1000: the middle is 1000 - 2 * 100 = 800 pixels, 400 units.
    let r = solve(&ws, win(1280, 1000, 2));
    let right = r.effective.area(Side::Right);
    assert!(
        right.slots().iter().all(|s| s.open),
        "150 + 112 + 110 fits 400"
    );
    // Layers' slot with Lint active: Lint folds first.
    let lint_first = ws.with_area(
        Side::Right,
        ws.area(Side::Right).with_slot(
            2,
            Slot {
                active: 2,
                ..ws.area(Side::Right).slots()[2]
            },
        ),
    );
    // 1x on 640 by 360: the bottom dock leaves too little and becomes its rail, so the middle
    // is 360 - 72 - 24 = 264 units, short of the least 150 + 64 + 60 = 274 by one fold.
    let r = solve(&lint_first, win(640, 360, 1));
    assert_tiles(&r, 640, 360, "short");
    assert!(r.area(Side::Bottom).rail);
    let right = r.effective.area(Side::Right);
    let open: Vec<bool> = right.slots().iter().map(|s| s.open).collect();
    assert_eq!(open, [true, true, false], "only the Lint slot folds");
    assert!(
        !r.effective.color_compact,
        "Color stays full while a slot can fold"
    );
}

#[test]
fn color_compacts_only_after_every_foldable_slot() {
    let right = Area::new(
        248,
        true,
        &[
            Slot::new(0, &[Panel::Color]),
            Slot::new(0, &[Panel::Palette]),
            Slot::new(0, &[Panel::Layers]),
        ],
    );
    let ws = default_layout(Preset::Draw).with_area(Side::Right, right);
    // Middle: 600 - 72 - 28 = 500 at 1x. 150 + 64 + 110 = 324 fits.
    let r = solve(&ws, win(1000, 600, 1));
    assert!(!r.effective.color_compact);
    // Make the bottom tall so the middle is short: 2x on 1280 by 960, bottom 120u.
    let tall = ws.with_area(
        Side::Bottom,
        Area::new(400, true, &[Slot::new(0, &[Panel::Timeline])]),
    );
    let r = solve(&tall, win(1280, 960, 2));
    let what = "tall bottom";
    assert_tiles(&r, 1280, 960, what);
    assert_least_kept(&r, what);
    assert_folds_documented(&tall, &r, what);
}

#[test]
fn the_bottom_dock_is_held_to_half_the_window_and_the_middle_its_least() {
    let ws = default_layout(Preset::Animate).with_area(
        Side::Bottom,
        Area::new(1024, true, &[Slot::new(0, &[Panel::Timeline])]),
    );
    let r = solve(&ws, win(1920, 1017, 1));
    assert!(r.area(Side::Bottom).rect.h <= 1017 / 2);
    let r = solve(&ws, win(640, 600, 1));
    assert!(r.canvas.h >= MIN_CANVAS);
    assert!(r.tools.h >= TWO_COLUMN_NEED);
}

#[test]
fn a_crowded_width_narrows_then_folds_the_left_area_first() {
    let left = Area::new(320, true, &[Slot::new(0, &[Panel::Lint])]);
    let right = Area::new(400, true, &[Slot::new(0, &[Panel::Palette, Panel::Color])]);
    let ws = default_layout(Preset::Draw)
        .with_area(Side::Left, left)
        .with_area(Side::Right, right);
    let wide = solve(&ws, win(1920, 1017, 1));
    assert_eq!(wide.area(Side::Left).rect.w, 320);
    let narrow = solve(&ws, win(800, 600, 1));
    assert!(narrow.canvas.w >= MIN_CANVAS);
    // 800 - 36 = 764; 764 - 280 = 484 left for both: left to 200 and right to 284 fits.
    assert_eq!(narrow.area(Side::Left).rect.w, 200);
    assert_eq!(narrow.area(Side::Right).rect.w, 284);
    let narrower = solve(&ws, win(640, 600, 1));
    // 640 - 36 - 280 = 324: both at 200 do not fit, so the left folds to its rail.
    assert!(narrower.area(Side::Left).rail);
    assert!(!narrower.area(Side::Right).rail);
    assert_eq!(narrower.area(Side::Right).rect.w, 300);
}

#[test]
fn a_short_middle_puts_the_tools_in_two_columns() {
    let ws = default_layout(Preset::Draw);
    assert_eq!(solve(&ws, win(1366, 705, 1)).tool_columns, 1);
    let r = solve(&ws, win(1280, 1000, 2));
    // 1000 - 2 * 100 = 800 pixels: under 462u at 2x.
    assert_eq!(r.tool_columns, 2);
    assert_eq!(r.tools.w, Metrics::LIGHTBOX.tools_two * 2);
}

#[test]
fn tiny_or_odd_windows_still_tile() {
    let mut rng = Rng(0x5eed_1e55);
    for (w, h, s) in [
        (0, 0, 1),
        (1, 1, 1),
        (10, 700, 2),
        (700, 10, 3),
        (639, 599, 0),
        (5000, 50, 1),
    ] {
        for _ in 0..50 {
            let ws = random_workspace(&mut rng);
            let r = solve(&ws, win(w, h, s));
            assert_tiles(&r, w, h, &format!("{w}x{h}@{s}"));
            assert_eq!(
                solve(&r.effective, win(w, h, s)),
                r,
                "idempotent at {w}x{h}"
            );
        }
    }
}

#[test]
fn random_arrangements_keep_every_property() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for n in 0..4000 {
        let ws = random_workspace(&mut rng);
        let window = if n % 4 == 0 {
            let (w, h, s) = SCREENS[(n / 4) % SCREENS.len()];
            win(w, h, s)
        } else {
            random_window(&mut rng)
        };
        let what = format!("case {n} {window:?}");
        let r = solve(&ws, window);
        assert_tiles(&r, window.w, window.h, &what);
        assert_least_kept(&r, &what);
        assert_folds_documented(&ws, &r, &what);
        assert_eq!(solve(&ws, window), r, "{what}: deterministic");
        assert_eq!(solve(&r.effective, window), r, "{what}: idempotent");
    }
}

#[test]
fn rails_keep_their_place_and_hide_the_slots() {
    let left = Area::new(240, false, &[Slot::new(0, &[Panel::Lint])]);
    let ws = default_layout(Preset::Draw).with_area(Side::Left, left);
    let r = solve(&ws, win(1366, 705, 1));
    let l = r.area(Side::Left);
    assert!(l.rail && l.slots().is_empty());
    assert_eq!(l.rect, Rect::new(36, 52, RAIL, 605));
    assert_eq!(r.canvas.x, 36 + RAIL);
}
