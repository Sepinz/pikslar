//! A focused slider answers the keys: steps, and the two ends.

use super::*;
use crate::focus::Nudge;

fn drawn(ui: &mut Ui, value: f32, unit: f32) -> Option<Slide> {
    let mut px = vec![0u32; 120 * 20];
    let mut sf = Surface {
        px: &mut px,
        w: 120,
        h: 20,
    };
    ui.begin();
    let out = slider_stepped(
        ui,
        &mut sf,
        7,
        Rect::new(0, 0, 101, 10),
        value,
        unit,
        |_, _| {},
    );
    ui.end();
    out
}

fn focused() -> Ui {
    let mut ui = Ui::new();
    drawn(&mut ui, 0.5, 1.0 / 255.0);
    assert_eq!(
        ui.focus.step(false),
        Some(7),
        "Tab reaches a slider outside a dialog"
    );
    ui
}

#[test]
fn a_step_moves_one_unit_and_ten_with_shift() {
    let mut ui = focused();
    ui.focus.request_nudge(Nudge::Step(-1));
    let got = drawn(&mut ui, 0.5, 1.0 / 255.0).expect("changed");
    assert!((got.value - (0.5 - 1.0 / 255.0)).abs() < 1e-6);
    ui.focus.request_nudge(Nudge::Step(10));
    let got = drawn(&mut ui, 0.5, 1.0 / 255.0).expect("changed");
    assert!((got.value - (0.5 + 10.0 / 255.0)).abs() < 1e-6);
}

#[test]
fn home_and_end_go_to_the_ends_and_nothing_goes_past() {
    let mut ui = focused();
    ui.focus.request_nudge(Nudge::Start);
    assert_eq!(drawn(&mut ui, 0.5, 0.01).map(|s| s.value), Some(0.0));
    ui.focus.request_nudge(Nudge::End);
    assert_eq!(drawn(&mut ui, 0.5, 0.01).map(|s| s.value), Some(1.0));
    ui.focus.request_nudge(Nudge::Step(-10));
    assert_eq!(drawn(&mut ui, 0.02, 0.01).map(|s| s.value), Some(0.0));
    ui.focus.request_nudge(Nudge::Step(10));
    assert_eq!(drawn(&mut ui, 0.98, 0.01).map(|s| s.value), Some(1.0));
}

#[test]
fn an_unfocused_slider_ignores_a_nudge_and_a_focused_one_is_outlined() {
    let mut ui = Ui::new();
    drawn(&mut ui, 0.5, 0.01);
    ui.focus.request_nudge(Nudge::Step(1));
    assert_eq!(drawn(&mut ui, 0.5, 0.01), None);
    ui.focus.step(false);
    let mut px = vec![0u32; 120 * 20];
    let mut sf = Surface {
        px: &mut px,
        w: 120,
        h: 20,
    };
    ui.begin();
    slider_stepped(
        &mut ui,
        &mut sf,
        7,
        Rect::new(5, 5, 101, 10),
        0.5,
        0.01,
        |_, _| {},
    );
    assert_eq!(
        px[8 * 120 + 3],
        crate::theme::focus(),
        "the ring, two units wide, left of the track"
    );
}
