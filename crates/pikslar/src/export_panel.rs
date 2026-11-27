//! Drawing the export panel. Each row shows only what the chosen kind of file has, and every
//! button is as wide as its words.

use crate::app::App;
use crate::export_dialog::{
    kind_hint, kind_label, place, ExportChoice, FrameChoice, MOST_COLUMNS, MOST_PADDING, MOST_SCALE,
};
use crate::export_map::{MAP_HINT, MAP_LABEL};
use pikslar_export::Kind;
use pikslar_platform::Window;
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, cap_height, draw_text, icon_button_if, text_width, Icon, MENU_LEVEL};
use std::borrow::Cow;

pub(crate) const ID_EXPORT: u32 = 500;
/// Rows, buttons and gaps, in interface units.
const ROW: i32 = 36;
const HIGH: i32 = 28;
const GAP: i32 = 4;
const MARGIN: i32 = 16;
/// The words that name each row, which sit in a column at the panel's left.
const ROW_NAMES: [&str; 8] = [
    "Format", "Frames", "Range", "Layers", "Color", "Playback", "Data", "Layout",
];

/// Where the next control in a row goes.
struct Flow {
    x: i32,
    y: i32,
}

impl App {
    pub fn draw_export_dialog(
        &mut self,
        sf: &mut Surface<'_>,
        window: Window,
        dirty: &mut Vec<Rect>,
    ) {
        let Some(choice) = self.export_dialog else {
            return;
        };
        let s = self.ui.scale;
        let panel = self.export_panel_rect(choice);
        self.ui.focus.collect(true);
        let back = self.ui.enter_level(MENU_LEVEL);
        pikslar_ui::chrome::popup(sf, panel, s);
        let left = panel.x + MARGIN * s;
        let ts = self.ui.text_scale();
        draw_text(sf, left, panel.y + 14 * s, "Export", theme::text(), ts);
        let mut next = choice;
        let mut y = panel.y + 38 * s;
        self.row_name(sf, (left, y), "Format");
        let wide = self.names_width();
        next = self.kind_row(sf, next, Flow { x: left + wide, y });
        let by_kind = next.kind != choice.kind;
        y += ROW * s;
        let right = panel.right() - MARGIN * s;
        if choice.map {
            self.map_rows(sf, (left, y), right);
        } else {
            next = self.detail_rows(sf, next, (left, y), right);
        }
        if next != choice {
            if let Some(said) = ExportChoice::changed_by_itself(choice, next, by_kind) {
                self.defer(move |a| a.notify(said));
            }
            // A shorter panel leaves the canvas under the old one to paint again.
            self.full_canvas |= next.kind != choice.kind
                || next.map != choice.map
                || has_range(next) != has_range(choice);
            self.export_dialog = Some(next);
            self.ui_dirty = true;
        }
        self.export_actions(
            sf,
            window,
            Flow {
                x: left,
                y: panel.bottom() - 40 * s,
            },
        );
        self.ui.focus.collect(false);
        self.ui.enter_level(back);
        dirty.push(panel.inset(-s));
    }

    /// The frames, the picture and what the kind of file has, under the kinds.
    fn detail_rows(
        &mut self,
        sf: &mut Surface<'_>,
        choice: ExportChoice,
        (left, mut y): (i32, i32),
        right: i32,
    ) -> ExportChoice {
        let s = self.ui.scale;
        let x = left + self.names_width();
        self.row_name(sf, (left, y), "Frames");
        let mut next = self.frames_row(sf, choice, Flow { x, y }, right);
        y += ROW * s;
        if has_range(choice) {
            self.row_name(sf, (left, y), "Range");
            next = self.range_row(sf, next, Flow { x, y });
            y += ROW * s;
        }
        self.row_name(sf, (left, y), "Layers");
        next = self.picture_row(sf, next, Flow { x, y });
        y += ROW * s;
        match next.kind {
            Kind::Sheet => self.sheet_rows(sf, next, (left, y)),
            Kind::Gif | Kind::Apng => {
                self.row_name(sf, (left, y), "Playback");
                self.loop_row(sf, next, Flow { x, y })
            }
            Kind::Png | Kind::Sequence => {
                self.row_name(sf, (left, y), "Color");
                self.indexed_row(sf, next, Flow { x, y })
            }
        }
    }

    /// Where the panel for `choice` goes: centred on the canvas, but kept on screen where the
    /// canvas is narrower than the panel, as in the smallest window.
    pub(crate) fn export_panel_rect(&self, choice: ExportChoice) -> Rect {
        let s = self.ui.scale;
        let rows = match choice.map {
            true => 4,
            false => 5 + 2 * i32::from(choice.kind == Kind::Sheet) + i32::from(has_range(choice)),
        };
        let (w, h) = (self.panel_width(), (38 + rows * ROW + 12) * s);
        let area = self.layout.canvas;
        let screen = (self.layout.bottom.w, self.layout.bottom.bottom());
        let x = (area.x + (area.w - w) / 2).clamp(0, (screen.0 - w).max(0));
        let y = (area.y + (area.h - h) / 2).clamp(0, (screen.1 - h).max(0));
        Rect::new(x, y, w, h)
    }

    /// As wide as the widest row: the kinds, or layers and scale.
    fn panel_width(&self) -> i32 {
        let s = self.ui.scale;
        let kinds: i32 = Kind::ALL
            .iter()
            .map(|&k| self.fit(kind_label(k)) + GAP * s)
            .sum();
        let kinds = kinds + i32::from(self.has_tilemaps()) * (self.fit(MAP_LABEL) + GAP * s);
        let picture = self.fit("Visible layers") + self.fit("This layer") + self.stepper_width();
        kinds.max(picture + 2 * GAP * s) + self.names_width() + 2 * MARGIN * s
    }

    /// The room for the row names: the widest, and a gap.
    fn names_width(&self) -> i32 {
        let ts = self.ui.text_scale();
        let widest = ROW_NAMES
            .iter()
            .map(|n| text_width(n, ts))
            .max()
            .unwrap_or(0);
        widest + 12 * self.ui.scale
    }

    /// A row's name, vertically centred on the row's buttons.
    fn row_name(&mut self, sf: &mut Surface<'_>, (x, y): (i32, i32), name: &str) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let y = y + (HIGH * s - cap_height() * ts) / 2;
        draw_text(sf, x, y, name, theme::muted(), ts);
    }

    /// A button's width for `label`.
    fn fit(&self, label: &str) -> i32 {
        text_width(label, self.ui.text_scale()) + 24 * self.ui.scale
    }

    fn stepper_width(&self) -> i32 {
        let s = self.ui.scale;
        text_width("Padding 64", self.ui.text_scale()) + (8 + 2 * HIGH + GAP) * s
    }

    /// The next button in the row, as wide as `label`; true when clicked.
    fn place(
        &mut self,
        sf: &mut Surface<'_>,
        at: &mut Flow,
        id: u32,
        label: &str,
        on: bool,
    ) -> bool {
        self.place_as(sf, at, (id, label), (on, false))
    }

    /// As `place`; a `default` button (the one Enter takes) gets the default outline that every
    /// dialog uses, since Accent is for what is chosen.
    fn place_as(
        &mut self,
        sf: &mut Surface<'_>,
        at: &mut Flow,
        (id, label): (u32, &str),
        (on, default): (bool, bool),
    ) -> bool {
        let s = self.ui.scale;
        let r = Rect::new(at.x, at.y, self.fit(label), HIGH * s);
        at.x = r.right() + GAP * s;
        let clicked = button(&mut self.ui, sf, ID_EXPORT + id, r, label, on);
        if default {
            pikslar_ui::chrome::default_edge(sf, r, s);
        }
        clicked
    }

    fn kind_row(&mut self, sf: &mut Surface<'_>, c: ExportChoice, mut at: Flow) -> ExportChoice {
        let s = self.ui.scale;
        let mut next = c;
        for (i, kind) in Kind::ALL.into_iter().enumerate() {
            let label = kind_label(kind);
            self.note_hint(
                Rect::new(at.x, at.y, self.fit(label), HIGH * s),
                kind_hint(kind),
            );
            if self.place(sf, &mut at, i as u32, label, c.kind == kind && !c.map) {
                next = c.with_kind(kind);
            }
        }
        if self.has_tilemaps() {
            let hint = Rect::new(at.x, at.y, self.fit(MAP_LABEL), HIGH * s);
            self.note_hint(hint, MAP_HINT);
            if self.place(sf, &mut at, Kind::ALL.len() as u32, MAP_LABEL, c.map) {
                next = c.with_map();
            }
        }
        next
    }

    /// What a TMJ holds, and the layers it leaves out, by name.
    fn map_rows(&mut self, sf: &mut Surface<'_>, (left, y): (i32, i32), right: i32) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let text_y = |row: i32| y + row * ROW * s + (HIGH * s - cap_height() * ts) / 2;
        let what = "This frame's maps, and an atlas per tileset";
        draw_text(
            sf,
            left,
            text_y(0),
            &fit_label(what, right - left, ts),
            theme::muted(),
            ts,
        );
        let left_out = self.map_left_out();
        let said = match left_out.is_empty() {
            true => "Nothing is left out".to_string(),
            false => format!("Left out: {}", left_out.join(", ")),
        };
        draw_text(
            sf,
            left,
            text_y(1),
            &fit_label(&said, right - left, ts),
            theme::text(),
            ts,
        );
    }

    fn frames_row(
        &mut self,
        sf: &mut Surface<'_>,
        c: ExportChoice,
        mut at: Flow,
        right: i32,
    ) -> ExportChoice {
        let mut next = c;
        let frame_on = c.kind == Kind::Png || c.frames == FrameChoice::This;
        if self.place(sf, &mut at, 10, "This frame", frame_on) {
            next = c.this_frame();
        }
        let all_on = c.kind != Kind::Png && c.frames == FrameChoice::All;
        if self.place(sf, &mut at, 11, "All frames", all_on) {
            // A still image is one picture: every frame is the Frames kind, a picture each.
            next = match c.kind {
                Kind::Png => c.all_frames_of_still(),
                _ => ExportChoice {
                    frames: FrameChoice::All,
                    ..c
                },
            };
        }
        if c.kind == Kind::Png {
            return next;
        }
        let doc = self.editor.doc();
        let first = c.first_range(self.editor.frame, &doc.tags, doc.frames.len());
        let hint = Rect::new(at.x, at.y, self.fit("Range"), HIGH * self.ui.scale);
        self.note_hint(hint, "Frames from one to another, both included");
        if self.place(sf, &mut at, 13, "Range", has_range(c)) {
            next.frames = first;
        }
        let count = self.editor.doc().tags.len();
        if count == 0 {
            return next;
        }
        let tags = &self.editor.doc().tags;
        let shown = match c.frames {
            FrameChoice::Tag(id) => place(tags, id).unwrap_or(0),
            _ => 0,
        };
        let (this, after) = (tags[shown].id, tags[(shown + 1) % count].id);
        let s = self.ui.scale;
        let room = right - at.x - 24 * s;
        let name = format!("Tag {}", self.editor.doc().tags[shown].name);
        let label = fit_label(&name, room, self.ui.text_scale());
        self.note_hint(
            Rect::new(at.x, at.y, self.fit(&label), HIGH * s),
            "A tag's frames. Click again for the next tag",
        );
        let on = matches!(c.frames, FrameChoice::Tag(_));
        if self.place(sf, &mut at, 12, &label, on) {
            next.frames = FrameChoice::Tag(if on { after } else { this });
        }
        next
    }

    /// The first and last frame of a range, counted from 1 as the timeline shows them.
    fn range_row(&mut self, sf: &mut Surface<'_>, c: ExportChoice, mut at: Flow) -> ExportChoice {
        let FrameChoice::Range(from, to) = c.frames else {
            return c;
        };
        let most = self.editor.doc().frames.len().max(1);
        let first = self.stepper(sf, &mut at, 14, ("From", from + 1), (1, most));
        let last = self.stepper(sf, &mut at, 16, ("To", to + 1), (1, most));
        match (first - 1, last - 1) {
            (f, _) if f != from => c.range_from(f),
            (_, t) if t != to => c.range_to(t),
            _ => c,
        }
    }

    fn picture_row(&mut self, sf: &mut Surface<'_>, c: ExportChoice, mut at: Flow) -> ExportChoice {
        let s = self.ui.scale;
        let mut next = c;
        if self.place(sf, &mut at, 20, "Visible layers", !c.this_layer) {
            next.this_layer = false;
        }
        let hint = Rect::new(at.x, at.y, self.fit("This layer"), HIGH * s);
        self.note_hint(hint, "Only the layer being edited, shown or hidden");
        if self.place(sf, &mut at, 21, "This layer", c.this_layer) {
            next.this_layer = true;
        }
        at.x += GAP * s;
        next.scale = self.stepper(sf, &mut at, 22, ("Scale", c.scale), (1, MOST_SCALE));
        next
    }

    /// A value with a smaller and a larger button. Returns the value after a click.
    fn stepper(
        &mut self,
        sf: &mut Surface<'_>,
        at: &mut Flow,
        id: u32,
        (label, value): (&str, usize),
        (least, most): (usize, usize),
    ) -> usize {
        let s = self.ui.scale;
        self.stepper_label(sf, at, (label, value));
        let x = at.x + self.stepper_width() - 2 * HIGH * s - GAP * s;
        let minus = Rect::new(x, at.y, HIGH * s, HIGH * s);
        let plus = Rect::new(x + (HIGH + GAP) * s, at.y, HIGH * s, HIGH * s);
        let (less, more) = stepper_hints(label);
        self.note_hint_named(minus, &format!("Less {label}"), less);
        self.note_hint_named(plus, &format!("More {label}"), more);
        at.x = plus.right() + 2 * GAP * s;
        // The minus and plus icons of every other stepper, off at the limit each leads to.
        let mut next = value;
        if icon_button_if(
            value > least,
            &mut self.ui,
            sf,
            ID_EXPORT + id,
            minus,
            Icon::Minus,
        ) {
            next = value.saturating_sub(1).max(least);
            let pair = (ID_EXPORT + id, ID_EXPORT + id + 1);
            self.hand_focus_at_limit(pair, next == least && least < most);
        }
        if icon_button_if(
            value < most,
            &mut self.ui,
            sf,
            ID_EXPORT + id + 1,
            plus,
            Icon::Plus,
        ) {
            next = (value + 1).min(most);
            let pair = (ID_EXPORT + id + 1, ID_EXPORT + id);
            self.hand_focus_at_limit(pair, next == most && least < most);
        }
        next
    }

    /// A stepper's words and value, at the start of its place.
    fn stepper_label(&self, sf: &mut Surface<'_>, at: &Flow, (label, value): (&str, usize)) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let text = match (label, value) {
            ("By tag", 0) => label.to_string(),
            _ => format!("{label} {value}"),
        };
        let y = at.y + (HIGH * s - cap_height() * ts) / 2;
        draw_text(sf, at.x, y, &text, theme::muted(), ts);
    }

    /// A stepper button its own press takes to its limit turns off, so the focus it has moves
    /// to the stepper's other button and the keys go on working the stepper. Tab still stops
    /// on a button that is off, as on every other control that is off.
    fn hand_focus_at_limit(&mut self, (pressed, other): (u32, u32), at_limit: bool) {
        if at_limit && self.ui.focus.is(pressed) {
            self.ui.focus.set(other);
        }
    }

    fn indexed_row(&mut self, sf: &mut Surface<'_>, c: ExportChoice, mut at: Flow) -> ExportChoice {
        let s = self.ui.scale;
        let rgba = Rect::new(at.x, at.y, self.fit("RGBA"), HIGH * s);
        let palette = Rect::new(
            at.x + self.fit("RGBA") + GAP * s,
            at.y,
            self.fit("Palette"),
            HIGH * s,
        );
        self.note_hint(rgba, "RGBA: every color and alpha as drawn");
        self.note_hint(
            palette,
            "Palette: an indexed PNG of at most 256 colors, in palette order when all are swatches",
        );
        let mut next = c;
        if self.place(sf, &mut at, 36, "RGBA", !c.indexed) {
            next.indexed = false;
        }
        if self.place(sf, &mut at, 30, "Palette", c.indexed) {
            next.indexed = true;
        }
        next
    }

    fn loop_row(&mut self, sf: &mut Surface<'_>, c: ExportChoice, mut at: Flow) -> ExportChoice {
        let mut next = c;
        if self.place(sf, &mut at, 40, "Loop", !c.once) {
            next.once = false;
        }
        if self.place(sf, &mut at, 41, "Play once", c.once) {
            next.once = true;
        }
        next
    }

    /// A sheet's colour, the data file's shape, and how the frames are laid out.
    fn sheet_rows(
        &mut self,
        sf: &mut Surface<'_>,
        c: ExportChoice,
        (left, y): (i32, i32),
    ) -> ExportChoice {
        let s = self.ui.scale;
        let x = left + self.names_width();
        self.row_name(sf, (left, y), "Color");
        let mut next = self.indexed_row(sf, c, Flow { x, y });
        let y = y + ROW * s;
        self.row_name(sf, (left, y), "Data");
        let mut row = Flow { x, y };
        if self.place(sf, &mut row, 37, "JSON array", !c.hash) {
            next.hash = false;
        }
        let hint = Rect::new(row.x, y, self.fit("JSON hash"), HIGH * s);
        self.note_hint(
            hint,
            "The .json keyed by frame name, as Pixi reads it. Phaser reads both",
        );
        if self.place(sf, &mut row, 31, "JSON hash", c.hash) {
            next.hash = true;
        }
        let y = y + ROW * s;
        self.row_name(sf, (left, y), "Layout");
        let mut row = Flow { x, y };
        let shown = c.columns.unwrap_or(0);
        let label = if shown == 0 { "By tag" } else { "Columns" };
        let hint = Rect::new(x, row.y, self.stepper_width(), HIGH * s);
        self.note_hint(hint, "Frames a row. Below 1, a row for each tag");
        let columns = self.stepper(sf, &mut row, 32, (label, shown), (0, MOST_COLUMNS));
        next.columns = (columns > 0).then_some(columns);
        let padding = ("Padding", c.padding);
        next.padding = self.stepper(sf, &mut row, 34, padding, (0, MOST_PADDING));
        next
    }

    fn export_actions(&mut self, sf: &mut Surface<'_>, window: Window, mut at: Flow) {
        if self.place_as(sf, &mut at, (50, "Export..."), (false, true)) {
            self.defer(move |a| a.export_chosen(window));
        } else if self.place(sf, &mut at, 51, "Cancel", false) {
            self.close_export_dialog();
        }
    }
}

/// Whether the panel shows a range's ends: a still image has no range.
fn has_range(c: ExportChoice) -> bool {
    c.kind != Kind::Png && matches!(c.frames, FrameChoice::Range(..))
}

/// `label`, shortened with a trailing ".." if it is wider than `width`.
fn fit_label(label: &str, width: i32, ts: i32) -> String {
    if text_width(label, ts) <= width {
        return label.to_string();
    }
    let mut short: String = label.to_string();
    while !short.is_empty() && text_width(&format!("{short}.."), ts) > width {
        short.pop();
    }
    format!("{short}..")
}

#[cfg(test)]
#[path = "export_panel_tests.rs"]
mod tests;

/// What a stepper's minus and plus do, said when the pointer or the keyboard is on them.
fn stepper_hints(label: &str) -> (Cow<'static, str>, Cow<'static, str>) {
    match label {
        "Scale" => ("Smaller scale".into(), "Larger scale".into()),
        "From" => ("Start a frame earlier".into(), "Start a frame later".into()),
        "To" => ("End a frame earlier".into(), "End a frame later".into()),
        _ => (
            format!("{label}: one less").into(),
            format!("{label}: one more").into(),
        ),
    }
}
