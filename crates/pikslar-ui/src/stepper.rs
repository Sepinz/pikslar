//! A stepper: "- n +", a whole number with a button either side, such as the brush size in
//! the context bar (docs/ui-redesign.md 3.4).
//!
//! The number is the focus stop. Keys on it: Right and Up add one, Left and Down take one away
//! (ten with Shift), Home and End go to the limits. The buttons are reachable with Tab too, and
//! are switched off at the limit they lead to.

use crate::chrome::{self, Look};
use crate::focus::WidgetKey;
use crate::font::{cap_height, draw_text, text_width};
use crate::icons::ActionIcon;
use crate::role::{Role, State};
use crate::widgets::Ui;
use pikslar_render::{Rect, Surface};

/// A stepper's number and its limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub value: i32,
    pub least: i32,
    pub most: i32,
}

impl Range {
    fn clamp(self, v: i32) -> i32 {
        v.clamp(self.least, self.most.max(self.least))
    }

    /// The number a key asks for.
    fn key(self, key: WidgetKey, shift: bool) -> Option<i32> {
        let step = if shift { 10 } else { 1 };
        let v = match key {
            WidgetKey::Right | WidgetKey::Up => self.value.saturating_add(step),
            WidgetKey::Left | WidgetKey::Down => self.value.saturating_sub(step),
            WidgetKey::Home => self.least,
            WidgetKey::End => self.most,
            WidgetKey::Enter | WidgetKey::Menu => return None,
        };
        Some(self.clamp(v))
    }
}

/// `v` written in decimal into `buf`, with no allocation.
fn digits(v: i32, buf: &mut [u8; 12]) -> &str {
    let mut n = v.unsigned_abs();
    let mut at = buf.len();
    loop {
        at -= 1;
        buf[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if v < 0 {
        at -= 1;
        buf[at] = b'-';
    }
    std::str::from_utf8(&buf[at..]).unwrap_or("")
}

/// Draws the stepper called `name` in `r` over `parent`: the number at `id`, the buttons at
/// `id + 1` (less) and `id + 2` (more), each as wide as `r` is tall. Returns the new number
/// when it changes.
pub fn stepper(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    (id, r): (u32, Rect),
    (name, parent): (&str, u32),
    range: Range,
) -> Option<i32> {
    let s = ui.scale.max(1);
    let less = Rect::new(r.x, r.y, r.h, r.h);
    let more = Rect::new(r.right() - r.h, r.y, r.h, r.h);
    let field = Rect::new(
        less.right() + s,
        r.y,
        (more.x - less.right() - 2 * s).max(0),
        r.h,
    );
    let off = |at_limit: bool| State {
        disabled: at_limit,
        ..State::NONE
    };
    let down = chrome::art_button(
        ui,
        sf,
        (id + 1, less),
        ActionIcon::Minus.art(),
        parent,
        off(range.value <= range.least),
    );
    let state = ui.interact(id, field);
    ui.focus.register_keys(id);
    ui.describe(Role::SpinButton, State::value(range.value));
    ui.name(id, name);
    let look = Look::of(state, false, false);
    chrome::well(sf, field, s, parent, look.hover);
    let mut buf = [0u8; 12];
    let text = digits(range.value, &mut buf);
    let ts = ui.text_scale();
    let tx = field.x + (field.w - text_width(text, ts)) / 2;
    let ty = field.y + (field.h - cap_height() * ts) / 2;
    draw_text(sf, tx, ty, text, look.ink(), ts);
    ui.ring(sf, id, field);
    let up = chrome::art_button(
        ui,
        sf,
        (id + 2, more),
        ActionIcon::Plus.art(),
        parent,
        off(range.value >= range.most),
    );
    if ui.naming() {
        ui.name(id + 1, &format!("Less {name}"));
        ui.name(id + 2, &format!("More {name}"));
    }
    let keyed = ui
        .focus
        .take_press(id)
        .and_then(|p| range.key(p.key, p.shift));
    let clicked = match (down, up) {
        (true, _) => Some(range.clamp(range.value - 1)),
        (_, true) => Some(range.clamp(range.value + 1)),
        _ => None,
    };
    clicked.or(keyed).filter(|&v| v != range.value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::Press;
    use crate::theme::{self, Theme};

    const W: i32 = 100;
    const H: i32 = 30;
    const R: Rect = Rect::new(5, 5, 80, 20);

    fn pass(ui: &mut Ui, px: &mut [u32], value: i32) -> Option<i32> {
        let mut sf = Surface { px, w: W, h: H };
        ui.begin();
        ui.focus.collect(true);
        let range = Range {
            value,
            least: 1,
            most: 64,
        };
        let out = stepper(ui, &mut sf, (40, R), ("Size", 7), range);
        ui.focus.collect(false);
        ui.end();
        out
    }

    fn click(ui: &mut Ui, px: &mut [u32], at: (i32, i32), value: i32) -> Option<i32> {
        ui.mouse = at;
        ui.pressed = true;
        ui.released = true;
        pass(ui, px, value)
    }

    #[test]
    fn the_buttons_step_by_one_and_stop_at_the_limits() {
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        assert_eq!(click(&mut ui, &mut px, (10, 15), 5), Some(4));
        assert_eq!(click(&mut ui, &mut px, (80, 15), 5), Some(6));
        assert_eq!(
            click(&mut ui, &mut px, (10, 15), 1),
            None,
            "switched off at the least"
        );
        assert_eq!(click(&mut ui, &mut px, (80, 15), 64), None);
        assert_eq!(ui.kind_of(42).map(|k| k.state.disabled), Some(true));
    }

    #[test]
    fn keys_on_the_number_step_jump_by_ten_and_reach_the_limits() {
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        pass(&mut ui, &mut px, 5);
        ui.focus.set(40);
        let mut key = |ui: &mut Ui, key, shift, value| {
            ui.focus.request_press(Press { key, shift });
            pass(ui, &mut px, value)
        };
        assert_eq!(key(&mut ui, WidgetKey::Right, false, 5), Some(6));
        assert_eq!(key(&mut ui, WidgetKey::Down, false, 5), Some(4));
        assert_eq!(key(&mut ui, WidgetKey::Up, true, 5), Some(15));
        assert_eq!(key(&mut ui, WidgetKey::Left, true, 5), Some(1), "clamped");
        assert_eq!(key(&mut ui, WidgetKey::End, false, 5), Some(64));
        assert_eq!(
            key(&mut ui, WidgetKey::Home, false, 1),
            None,
            "already there"
        );
        assert!(ui.focus.takes_keys(40));
    }

    #[test]
    fn digits_are_written_without_allocating() {
        let mut buf = [0u8; 12];
        assert_eq!(digits(0, &mut buf), "0");
        assert_eq!(digits(64, &mut buf), "64");
        assert_eq!(digits(-7, &mut buf), "-7");
        assert_eq!(digits(i32::MIN, &mut buf), "-2147483648");
    }

    #[test]
    fn the_number_sits_in_a_well_and_takes_the_focus_ring() {
        theme::set(Theme::LIGHTBOX);
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        ui.focus.set(40);
        pass(&mut ui, &mut px, 8);
        let at = |x: i32, y: i32| px[(y * W + x) as usize];
        assert_eq!(at(28, 7), theme::well());
        assert_eq!(at(26, 5), 7, "notched");
        assert_eq!(at(30, 3), theme::focus(), "ring above the field");
        theme::set(Theme::NORMAL);
    }

    #[test]
    fn it_lists_a_value_and_two_named_buttons() {
        let mut px = vec![0; (W * H) as usize];
        let mut ui = Ui::new();
        ui.record_names();
        pass(&mut ui, &mut px, 8);
        let text = ui.layout().unwrap_or_default();
        assert!(text.contains("\tSize\tspinbutton\tvalue=8\n"), "{text}");
        assert!(text.contains("\tLess Size\tbutton\t\n"), "{text}");
        assert!(text.contains("\tMore Size\tbutton\t\n"), "{text}");
    }
}
