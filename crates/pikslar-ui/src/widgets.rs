//! Interaction state and the widgets built on it.

use crate::chrome::Look;
use crate::focus::Nudge;
use crate::font::{cap_height, draw_text, text_width};
use crate::icons::{draw_icon, Icon, ICON_SIZE};
pub use crate::role::{Kind, Role, State};
use crate::theme;
use pikslar_render::{Rect, Surface};

/// Pointer state, plus which widget owns the pointer.
#[derive(Debug, Default)]
pub struct Ui {
    pub mouse: (i32, i32),
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
    /// Whether Shift was held when the pointer went down.
    pub shift: bool,
    /// The widget holding the pointer, or 0.
    pub active: u32,
    /// The row of the open menu the keyboard is on, drawn with a marker bar.
    pub menu_highlight: Option<usize>,
    /// The first row of the open menu in view, when it has more rows than fit.
    pub menu_first: usize,
    /// Size of one interface unit in screen pixels.
    pub scale: i32,
    widgets: Vec<(Rect, u32)>,
    /// The layer each widget of `widgets` was drawn on.
    levels: Vec<u8>,
    /// The role and state of each widget of `widgets`.
    kinds: Vec<Kind>,
    hover: u32,
    /// Where the held widget was caught, for widgets that are dragged by a part of them.
    pub grab: i32,
    /// The layer widgets are drawn on now: 0 for panels, higher for what floats above them.
    level: u8,
    /// Rectangles that belong to a layer above the panels this pass, such as an open menu.
    covers: Vec<(Rect, u8)>,
    /// Which widget the keys act on.
    pub focus: crate::focus::Focus,
    /// What each widget is called, kept only while a test asks for the layout.
    names: Option<Names>,
    /// The focused widget drew its own ring this pass.
    ringed: bool,
}

/// Names for widgets: the words on a button first, then the hint given for its rectangle, then
/// the name of its icon.
#[derive(Debug, Default)]
struct Names {
    by_id: Vec<(u32, String)>,
    by_rect: Vec<(Rect, String)>,
    icons: Vec<(u32, String)>,
}

/// The last name given to `id` in `list`.
fn named(list: &[(u32, String)], id: u32) -> Option<&str> {
    list.iter()
        .rev()
        .find(|(i, _)| *i == id)
        .map(|(_, n)| n.as_str())
}

impl Names {
    fn of(&self, id: u32, r: Rect) -> &str {
        let by_rect = || self.by_rect.iter().rev().find(|(at, _)| *at == r);
        named(&self.by_id, id)
            .or_else(|| by_rect().map(|(_, n)| n.as_str()))
            .or_else(|| named(&self.icons, id))
            .unwrap_or("")
    }

    fn clear(&mut self) {
        self.by_id.clear();
        self.by_rect.clear();
        self.icons.clear();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Interaction {
    pub hover: bool,
    pub clicked: bool,
    pub held: bool,
    /// The press began on this widget during this pass.
    pub started: bool,
}

impl Ui {
    pub fn new() -> Self {
        Ui {
            scale: 1,
            ..Default::default()
        }
    }

    /// Pixels per font pixel: a unit for Pikslar Text, two for the doubled Pikslar Mono.
    pub fn text_scale(&self) -> i32 {
        crate::font::text_scale(self.scale)
    }

    /// The scale icons are drawn at: two pixels a unit, whatever the face of the words.
    pub fn icon_scale(&self) -> i32 {
        self.scale * 2
    }

    /// Scrolls the open menu by wheel `ticks`, positive up, three rows a tick. The draw keeps it
    /// in range.
    pub fn menu_scroll(&mut self, ticks: i32) {
        let by = (ticks as isize).saturating_mul(-3);
        self.menu_first = self.menu_first.saturating_add_signed(by);
    }

    /// Clears the widget list before a redraw.
    pub fn begin(&mut self) {
        self.widgets.clear();
        self.focus.begin();
        self.ringed = false;
        self.levels.clear();
        self.kinds.clear();
        self.covers.clear();
        self.level = 0;
        if let Some(names) = &mut self.names {
            names.clear();
        }
    }

    /// From now on keeps what each widget is called, for [`Ui::layout`]. For tests: each name
    /// costs an allocation.
    pub fn record_names(&mut self) {
        self.names.get_or_insert_with(Names::default);
    }

    /// Whether names are kept, so a caller can skip building one.
    pub fn naming(&self) -> bool {
        self.names.is_some()
    }

    /// Calls the widget `id` `name`, if names are kept.
    pub fn name(&mut self, id: u32, name: &str) {
        if let Some(names) = &mut self.names {
            names.by_id.push((id, name.to_string()));
        }
    }

    /// Calls the icon button `id` after its icon, if names are kept and it has no better name.
    pub(crate) fn name_icon(&mut self, id: u32, name: &str) {
        if let Some(names) = &mut self.names {
            names.icons.push((id, name.to_string()));
        }
    }

    /// Calls the widget with exactly `rect` `name`, if names are kept and it has no other. A
    /// rectangle no widget has, such as one frame of the strip, is listed on its own.
    pub fn name_rect(&mut self, rect: Rect, name: &str) {
        if let Some(names) = &mut self.names {
            names.by_rect.push((rect, name.to_string()));
        }
    }

    /// Every widget of the last pass, one a line in drawn order: id, x, y, width, height, name,
    /// role and state (see [`Role::word`] and [`State::words`]), separated by tabs. Then named
    /// rectangles that are no widget's, with id 0 and no role, and last the focused widget
    /// again, with id 0 and its name after `focus: `. None unless names are kept. Scripts read
    /// the name from column 6, so columns are only ever added after it.
    pub fn layout(&self) -> Option<String> {
        let names = self.names.as_ref()?;
        let line = |id: u32, r: Rect, name: &str, kind: Kind| {
            let (role, state) = (kind.role.word(), kind.state.words());
            format!(
                "{id}\t{}\t{}\t{}\t{}\t{name}\t{role}\t{state}\n",
                r.x, r.y, r.w, r.h
            )
        };
        let mut out = String::new();
        for (&(r, id), &kind) in self.widgets.iter().zip(&self.kinds) {
            out.push_str(&line(id, r, names.of(id, r), kind));
        }
        for (r, name) in &names.by_rect {
            if !self.widgets.iter().any(|(at, _)| at == r) {
                out.push_str(&line(0, *r, name, Kind::default()));
            }
        }
        // Where keyboard focus is, for scripts that walk with Tab.
        let focused = self.focus.focused();
        if let Some((&(r, id), &kind)) = self
            .widgets
            .iter()
            .zip(&self.kinds)
            .rev()
            .find(|((_, id), _)| Some(*id) == focused)
        {
            out.push_str(&line(0, r, &format!("focus: {}", names.of(id, r)), kind));
        }
        Some(out)
    }

    /// Says what the widget drawn last is and what state it shows.
    pub fn describe(&mut self, role: Role, state: State) {
        if let Some(kind) = self.kinds.last_mut() {
            *kind = Kind { role, state };
        }
    }

    /// The role and state of widget `id` in the last pass.
    pub fn kind_of(&self, id: u32) -> Option<Kind> {
        self.widgets
            .iter()
            .zip(&self.kinds)
            .rev()
            .find(|((_, w), _)| *w == id)
            .map(|(_, &k)| k)
    }

    /// A widget that holds others, such as a tab strip or a rail: listed in the layout, and in
    /// front of the canvas for the pointer, but no focus stop and no answer to the pointer of
    /// its own. Draw it before what it holds, which then wins the pointer.
    pub fn container(&mut self, id: u32, rect: Rect, role: Role, state: State) {
        self.widgets.push((rect, id));
        self.levels.push(self.level);
        self.kinds.push(Kind { role, state });
    }

    /// Whether a widget drawn so far this pass, on this layer, lies under the point.
    pub fn claimed(&self, x: i32, y: i32) -> bool {
        self.widgets
            .iter()
            .zip(&self.levels)
            .any(|((r, _), &l)| l == self.level && r.contains(x, y))
    }

    /// Consumes the one-shot press and release after a redraw.
    pub fn end(&mut self) {
        self.focus.end();
        if self.released || !self.down {
            self.active = 0;
        }
        self.pressed = false;
        self.released = false;
    }

    /// Says, before anything under it is drawn, that `rect` belongs to layer `level` this pass.
    /// Widgets on lower layers then ignore the pointer inside it.
    pub fn cover(&mut self, rect: Rect, level: u8) {
        self.covers.push((rect, level));
    }

    /// Draws what follows on layer `level`, and returns the layer to go back to.
    pub fn enter_level(&mut self, level: u8) -> u8 {
        std::mem::replace(&mut self.level, level)
    }

    /// Whether the pointer is under something drawn above the controls now being drawn.
    pub fn pointer_covered(&self) -> bool {
        self.covered(self.level, self.mouse.0, self.mouse.1)
    }

    /// Whether a layer above `level` covers the point.
    fn covered(&self, level: u8, x: i32, y: i32) -> bool {
        self.covers
            .iter()
            .any(|&(r, l)| l > level && r.contains(x, y))
    }

    pub fn interact(&mut self, id: u32, rect: Rect) -> Interaction {
        self.widgets.push((rect, id));
        self.focus.register(id);
        self.levels.push(self.level);
        self.kinds.push(Kind::default());
        let (x, y) = self.mouse;
        let inside = rect.contains(x, y) && !self.covered(self.level, x, y);
        let free = self.active == 0 || self.active == id;
        let mut out = Interaction {
            hover: inside && free,
            ..Default::default()
        };
        if out.hover && self.pressed && self.active == 0 {
            self.active = id;
            out.started = true;
        }
        if self.active == id {
            out.held = self.down || self.released;
            out.clicked = self.released && inside;
        }
        // Space or Enter on the focused widget is a click.
        if self.focus.take_activate(id) {
            out.clicked = true;
        }
        out
    }

    /// Draws the focus ring round `r` when `id` is the focused widget, as [`Ui::ring`] does.
    pub fn focus_ring(&mut self, sf: &mut Surface<'_>, id: u32, r: Rect) {
        if !self.focus.is(id) {
            return;
        }
        self.ringed = true;
        crate::chrome::ring(sf, r, self.scale.max(1));
    }

    /// The redesign's focus ring round `r` when `id` is the focused widget: two units of the
    /// Focus colour, one unit outside (see [`crate::chrome::ring`]).
    pub fn ring(&mut self, sf: &mut Surface<'_>, id: u32, r: Rect) {
        if self.focus.is(id) {
            self.ringed = true;
            crate::chrome::ring(sf, r, self.scale.max(1));
        }
    }

    /// Rings the focused widget when it drew none of its own, as a widget that paints itself
    /// does: two units of the focus color just inside it, where nothing else paints over it.
    pub fn focus_ring_inside(&mut self, sf: &mut Surface<'_>) {
        let Some(id) = self.focus.focused().filter(|_| !self.ringed) else {
            return;
        };
        let Some(&(r, _)) = self.widgets.iter().rev().find(|(_, w)| *w == id) else {
            return;
        };
        let t = (2 * self.scale.max(1)).min(r.w / 2).min(r.h / 2).max(1);
        let c = theme::focus();
        sf.fill(Rect::new(r.x, r.y, r.w, t), c);
        sf.fill(Rect::new(r.x, r.bottom() - t, r.w, t), c);
        sf.fill(Rect::new(r.x, r.y, t, r.h), c);
        sf.fill(Rect::new(r.right() - t, r.y, t, r.h), c);
        self.ringed = true;
    }

    /// The widget under a point in the last drawn pass, or 0.
    pub fn widget_at(&self, x: i32, y: i32) -> u32 {
        self.widgets
            .iter()
            .zip(&self.levels)
            .rev()
            .find(|((r, _), &level)| r.contains(x, y) && !self.covered(level, x, y))
            .map(|(w, _)| w)
            .map_or(0, |&(_, id)| id)
    }

    /// Every widget of the last drawn pass.
    pub fn widgets(&self) -> &[(Rect, u32)] {
        &self.widgets
    }

    /// Records a pointer move and reports whether the interface must redraw.
    pub fn track(&mut self, x: i32, y: i32) -> bool {
        self.mouse = (x, y);
        let hovered = self.widget_at(x, y);
        let changed = hovered != self.hover;
        self.hover = hovered;
        changed || self.active != 0
    }
}

/// A button face: edge, lip and notched corners, changing under the pointer.
fn fill_face(ui: &Ui, sf: &mut Surface<'_>, r: Rect, state: Interaction, selected: bool) {
    let look = Look::of(state, selected, false);
    crate::chrome::face_here(sf, r, ui.scale.max(1), look);
}

/// A text button. Returns true when clicked.
pub fn button(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    r: Rect,
    label: &str,
    selected: bool,
) -> bool {
    let state = ui.interact(id, r);
    ui.describe(Role::Button, State::chosen(selected));
    ui.name(id, label);
    let look = Look::of(state, selected, false);
    fill_face(ui, sf, r, state, selected);
    let scale = ui.text_scale();
    let x = r.x + (r.w - text_width(label, scale)) / 2;
    let y = r.y + (r.h - cap_height() * scale) / 2 + look.drop() * ui.scale;
    draw_text(sf, x, y, label, look.ink(), scale);
    ui.focus_ring(sf, id, r);
    state.clicked
}

/// The button Enter takes in a dialog: a plain [`button`] whose outline is Text, so amber stays
/// for what is chosen (final review, defect 8). Returns true when clicked.
pub fn default_button(ui: &mut Ui, sf: &mut Surface<'_>, id: u32, r: Rect, label: &str) -> bool {
    let clicked = button(ui, sf, id, r, label, false);
    crate::chrome::default_edge(sf, r, ui.scale.max(1));
    clicked
}

/// An icon button that can be switched off, which it shows. Off, it ignores the pointer.
pub fn icon_button_if(
    usable: bool,
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    r: Rect,
    icon: Icon,
) -> bool {
    if usable {
        return icon_button(ui, sf, id, r, icon, false);
    }
    ui.interact(id, r);
    ui.describe(
        Role::Button,
        State {
            disabled: true,
            ..State::NONE
        },
    );
    if ui.names.is_some() {
        ui.name_icon(id, &format!("{icon:?} (off)"));
    }
    let look = Look::of(Interaction::default(), false, true);
    crate::chrome::face_here(sf, r, ui.scale.max(1), look);
    let scale = ui.icon_scale();
    draw_icon(
        sf,
        r.x + (r.w - ICON_SIZE * scale) / 2,
        r.y + (r.h - ICON_SIZE * scale) / 2,
        icon,
        look.ink(),
        scale,
    );
    false
}

/// A square icon button. Returns true when clicked.
pub fn icon_button(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    r: Rect,
    icon: Icon,
    selected: bool,
) -> bool {
    let state = ui.interact(id, r);
    ui.describe(Role::Button, State::chosen(selected));
    if ui.names.is_some() {
        ui.name_icon(id, &format!("{icon:?}"));
    }
    let look = Look::of(state, selected, false);
    fill_face(ui, sf, r, state, selected);
    let scale = ui.icon_scale();
    let (x, y) = (
        r.x + (r.w - ICON_SIZE * scale) / 2,
        r.y + (r.h - ICON_SIZE * scale) / 2 + look.drop() * ui.scale,
    );
    draw_icon(sf, x, y, icon, look.ink(), scale);
    ui.focus_ring(sf, id, r);
    state.clicked
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    pub value: f32,
    /// True for the first change of a drag.
    pub started: bool,
}

/// A slider's handle at `x` over the track `r`, in three bands so it shows on a bar of any
/// shade: the ink in the middle, a ring round it that the bar's own colour cannot match (the
/// surface in high contrast, where the ink is the text colour; the rule otherwise), and the text
/// colour again outside the ring.
fn draw_handle(sf: &mut Surface<'_>, (x, r): (i32, Rect), s: i32, live: bool) {
    let core = if live { theme::accent() } else { theme::text() };
    let ring = if crate::icons::high_contrast() {
        theme::panel()
    } else {
        theme::rule()
    };
    let knob = s * 3;
    // Kept on the track at both ends, so it never overhangs onto what lies beside it.
    let x = x
        .min(r.right() - (knob - knob / 2) - 2 * s)
        .max(r.x + knob / 2 + 2 * s);
    let outer = Rect::new(x - knob / 2 - 2 * s, r.y, knob + 4 * s, r.h);
    sf.fill(outer, theme::text());
    sf.fill(
        Rect::new(outer.x + s, outer.y, outer.w - 2 * s, outer.h),
        ring,
    );
    sf.fill(Rect::new(x - knob / 2, r.y, knob, r.h), core);
}

/// A slider whose track the caller paints. `value` runs from 0 to 1. A key moves it by 1/255.
pub fn slider(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    r: Rect,
    value: f32,
    track: impl FnOnce(&mut Surface<'_>, Rect),
) -> Option<Slide> {
    slider_stepped(ui, sf, id, r, value, 1.0 / 255.0, track)
}

/// [`slider`] whose keys move it by `unit` (Shift, ten of them), for values with their own grain.
pub fn slider_stepped(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    r: Rect,
    value: f32,
    unit: f32,
    track: impl FnOnce(&mut Surface<'_>, Rect),
) -> Option<Slide> {
    // The track keeps its look, but the part that answers the pointer is never under 24 units
    // tall, for a hand that does not aim well.
    let want = 24 * ui.scale.max(1);
    let hit = if r.h < want {
        Rect::new(r.x, r.y - (want - r.h) / 2, r.w, want)
    } else {
        r
    };
    let state = ui.interact(id, hit);
    ui.focus.register_slider(id);
    track(sf, r);
    let usable = (r.w - 1).max(1);
    let dragged = (state.held && ui.down).then(|| Slide {
        value: ((ui.mouse.0 - r.x) as f32 / usable as f32).clamp(0.0, 1.0),
        started: state.started,
    });
    let keyed = ui.focus.take_nudge(id).map(|n| Slide {
        value: match n {
            Nudge::Step(k) => (value + k as f32 * unit).clamp(0.0, 1.0),
            Nudge::Start => 0.0,
            Nudge::End => 1.0,
        },
        started: true,
    });
    let result = dragged.or(keyed);
    let shown = result.map_or(value, |s| s.value);
    ui.describe(Role::Slider, State::value((shown * 100.0).round() as i32));
    let x = r.x + (shown * usable as f32).round() as i32;
    draw_handle(sf, (x, r), ui.scale, state.hover || state.held);
    ui.focus_ring(sf, id, r);
    result
}

#[cfg(test)]
#[path = "slider_keys_tests.rs"]
mod slider_keys_tests;

#[cfg(test)]
#[path = "layout_focus_tests.rs"]
mod layout_focus_tests;

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod tests;
