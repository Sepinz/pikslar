//! Keyboard focus: which widget the keys act on, and the order Tab walks them in.
//!
//! The interface is immediate mode, so the order is rebuilt as a dialog draws. A dialog
//! brackets its widgets with [`Focus::collect`]; every widget drawn in between joins the order.
//! The main window brackets its panels the same way, so Tab walks every control there too;
//! with nothing collected, Tab walks the sliders.

/// A change of value asked of the focused slider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nudge {
    /// This many steps, negative for down.
    Step(i32),
    Start,
    End,
}

/// A key for a focused widget that reads keys itself (a splitter, a tab, a segmented control,
/// a stepper), beyond the click Space and Enter give every widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Enter,
    /// Shift+F10 or the menu key.
    Menu,
}

/// A [`WidgetKey`] and whether Shift was held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Press {
    pub key: WidgetKey,
    pub shift: bool,
}

impl Press {
    pub const fn plain(key: WidgetKey) -> Press {
        Press { key, shift: false }
    }
}

/// Focus state. A key sets a request; the widget it is for takes it on the next draw.
#[derive(Debug, Default)]
pub struct Focus {
    order: Vec<u32>,
    sliders: Vec<u32>,
    focused: Option<u32>,
    collecting: bool,
    activate: bool,
    nudge: Option<Nudge>,
    /// Widgets drawn this pass that read keys themselves.
    keyed: Vec<u32>,
    press: Option<Press>,
}

impl Focus {
    /// Starts a draw pass: the order is built again.
    pub fn begin(&mut self) {
        self.order.clear();
        self.sliders.clear();
        self.keyed.clear();
        self.collecting = false;
    }

    /// Whether widgets drawn from now on join the order.
    pub fn collect(&mut self, on: bool) {
        self.collecting = on;
    }

    /// A widget drawn this pass.
    pub fn register(&mut self, id: u32) {
        if self.collecting {
            self.order.push(id);
        }
    }

    /// A slider drawn this pass, reachable even outside a dialog.
    pub fn register_slider(&mut self, id: u32) {
        self.sliders.push(id);
    }

    /// Whether `id` is a slider, which takes Left and Right for its value.
    pub fn is_slider(&self, id: u32) -> bool {
        self.sliders.contains(&id)
    }

    pub fn focused(&self) -> Option<u32> {
        self.focused
    }

    pub fn is(&self, id: u32) -> bool {
        self.focused == Some(id)
    }

    pub fn set(&mut self, id: u32) {
        self.focused = Some(id);
    }

    /// Ends a draw pass: a request nobody took is dropped, so it cannot fire later.
    pub fn end(&mut self) {
        self.activate = false;
        self.nudge = None;
        self.press = None;
        // A pass that drew no stops at all, as a partial redraw may, says nothing about focus.
        let drew = self.drew_any();
        if drew && self.focused.is_some_and(|f| !self.is_drawn(f)) {
            self.focused = None;
        }
    }

    /// Whether `id` was drawn as a focus stop this pass.
    fn is_drawn(&self, id: u32) -> bool {
        self.order.contains(&id) || self.sliders.contains(&id) || self.keyed.contains(&id)
    }

    /// Whether this pass drew any focus stop.
    fn drew_any(&self) -> bool {
        !self.order.is_empty() || !self.sliders.is_empty() || !self.keyed.is_empty()
    }

    /// Drops focus, and any request not yet taken.
    pub fn clear(&mut self) {
        self.focused = None;
        self.activate = false;
        self.nudge = None;
        self.press = None;
    }

    /// The widgets Tab walks now: a dialog's, or else the sliders.
    pub fn order(&self) -> &[u32] {
        if self.order.is_empty() {
            &self.sliders
        } else {
            &self.order
        }
    }

    /// Moves focus to the next widget, or the previous one, wrapping. From nowhere it goes to
    /// the first, or the last going back. Returns the widget now focused.
    pub fn step(&mut self, back: bool) -> Option<u32> {
        let order = self.order();
        let last = order.len().checked_sub(1)?;
        let at = self
            .focused
            .and_then(|f| order.iter().position(|&id| id == f));
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => last,
            (Some(0), true) => last,
            (Some(i), true) => i - 1,
            (Some(i), false) if i == last => 0,
            (Some(i), false) => i + 1,
        };
        self.focused = Some(order[next]);
        self.focused
    }

    /// Like [`Focus::step`], but only among the widgets `keep` accepts, and only when the focused
    /// one is among them: the arrows of a row of tabs or a list.
    pub fn step_in(&mut self, back: bool, keep: impl Fn(u32) -> bool) -> Option<u32> {
        let group: Vec<u32> = self
            .order()
            .iter()
            .copied()
            .filter(|&id| keep(id))
            .collect();
        let at = group.iter().position(|&id| self.is(id))?;
        let next = if back {
            (at + group.len() - 1) % group.len()
        } else {
            (at + 1) % group.len()
        };
        self.focused = Some(group[next]);
        self.focused
    }

    /// Asks the focused widget to act as if clicked. False when nothing is focused.
    pub fn request_activate(&mut self) -> bool {
        let drew = self.drew_any();
        self.activate = self.focused.is_some_and(|f| !drew || self.is_drawn(f));
        self.activate
    }

    /// Whether `id` was asked to act. The request is spent.
    pub fn take_activate(&mut self, id: u32) -> bool {
        let yes = self.activate && self.is(id);
        if yes {
            self.activate = false;
        }
        yes
    }

    pub fn request_nudge(&mut self, nudge: Nudge) {
        self.nudge = Some(nudge);
    }

    /// A widget drawn this pass that reads [`WidgetKey`]s itself.
    pub fn register_keys(&mut self, id: u32) {
        self.keyed.push(id);
    }

    /// Whether `id` reads keys itself, so the arrows, Home, End, Enter and the menu key go to
    /// it through [`Focus::request_press`] rather than moving focus.
    pub fn takes_keys(&self, id: u32) -> bool {
        self.keyed.contains(&id)
    }

    /// Hands `press` to the focused widget on the next draw.
    pub fn request_press(&mut self, press: Press) {
        self.press = Some(press);
    }

    /// The key asked of `id`, if it is the focused one. The request is spent.
    pub fn take_press(&mut self, id: u32) -> Option<Press> {
        if self.is(id) {
            self.press.take()
        } else {
            None
        }
    }

    /// The change asked of slider `id`, if it is the focused one. The request is spent.
    pub fn take_nudge(&mut self, id: u32) -> Option<Nudge> {
        if self.is(id) {
            self.nudge.take()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(ids: &[u32]) -> Focus {
        let mut f = Focus::default();
        f.begin();
        f.collect(true);
        ids.iter().for_each(|&id| f.register(id));
        f.collect(false);
        f
    }

    #[test]
    fn tab_walks_the_order_and_wraps_both_ways() {
        let mut f = drawn(&[5, 6, 7]);
        assert_eq!(f.step(false), Some(5), "from nowhere to the first");
        assert_eq!(f.step(false), Some(6));
        assert_eq!(f.step(false), Some(7));
        assert_eq!(f.step(false), Some(5), "wraps");
        assert_eq!(f.step(true), Some(7), "and back");
        f.clear();
        assert_eq!(f.step(true), Some(7), "backwards from nowhere is the last");
    }

    #[test]
    fn arrows_stay_inside_their_group_and_wrap() {
        let mut f = drawn(&[1, 2, 10, 11, 12]);
        f.set(2);
        assert_eq!(f.step_in(false, |id| id < 10), Some(1), "wraps in the tabs");
        f.set(10);
        assert_eq!(f.step_in(false, |id| id >= 10), Some(11));
        assert_eq!(f.step_in(true, |id| id >= 10), Some(10));
        assert_eq!(f.step_in(true, |id| id >= 10), Some(12));
        f.set(1);
        assert_eq!(f.step_in(false, |id| id >= 10), None, "not in the group");
        assert_eq!(f.focused(), Some(1));
    }

    #[test]
    fn focus_on_a_widget_that_was_not_drawn_this_pass_is_dropped_at_its_end() {
        let mut f = drawn(&[1, 2]);
        f.set(2);
        f.begin();
        f.collect(true);
        f.register(1);
        f.collect(false);
        f.end();
        assert_eq!(f.focused(), None);
        assert!(!f.request_activate(), "Enter falls through to the dialog");
        // A slider that is still drawn keeps it.
        f.begin();
        f.register_slider(9);
        f.set(9);
        f.end();
        assert_eq!(f.focused(), Some(9));
    }

    #[test]
    fn a_focus_that_is_no_longer_drawn_starts_again() {
        let mut f = drawn(&[1, 2]);
        f.set(9);
        assert_eq!(f.step(false), Some(1));
    }

    #[test]
    fn nothing_joins_the_order_unless_collecting() {
        let mut f = Focus::default();
        f.begin();
        f.register(4);
        assert_eq!(f.step(false), None);
        assert!(f.order().is_empty());
    }

    #[test]
    fn outside_a_dialog_tab_walks_the_sliders() {
        let mut f = Focus::default();
        f.begin();
        f.register_slider(30);
        f.register_slider(31);
        assert_eq!(f.step(false), Some(30));
        assert_eq!(f.step(false), Some(31));
    }

    #[test]
    fn activation_and_nudges_go_to_the_focused_widget_once() {
        let mut f = drawn(&[1, 2]);
        assert!(!f.request_activate(), "nothing is focused");
        f.set(2);
        assert!(f.request_activate());
        assert!(!f.take_activate(1));
        assert!(f.take_activate(2));
        assert!(!f.take_activate(2), "spent");
        f.request_nudge(Nudge::Step(-1));
        assert_eq!(f.take_nudge(1), None);
        assert_eq!(f.take_nudge(2), Some(Nudge::Step(-1)));
        assert_eq!(f.take_nudge(2), None);
    }

    #[test]
    fn a_key_press_goes_to_the_focused_keyed_widget_once_and_lapses_at_the_end() {
        let mut f = Focus::default();
        f.begin();
        f.register_keys(4);
        f.set(4);
        assert!(f.takes_keys(4));
        assert!(!f.takes_keys(5));
        f.request_press(Press::plain(WidgetKey::Home));
        assert_eq!(f.take_press(5), None, "not the focused one");
        assert_eq!(f.take_press(4), Some(Press::plain(WidgetKey::Home)));
        assert_eq!(f.take_press(4), None, "spent");
        f.request_press(Press::plain(WidgetKey::End));
        f.end();
        assert_eq!(
            f.focused(),
            Some(4),
            "a keyed widget drawn this pass keeps focus"
        );
        assert_eq!(f.take_press(4), None, "an untaken key lapses");
    }
}
