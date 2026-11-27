//! The text tool. A click opens self.defer(|a| a.refresh_stale_text()); session: an empty paste floats at the click, and each change
//! to the text swaps the pasted pixels for the text drawn again, so the whole of it, with any
//! moves, is one undo step when Enter anchors it. Esc takes it all back.

use crate::app::{App, Gesture};
use crate::context_bar::control;
use crate::shortcuts::{Action, Binding};
use pikslar_core::glyphs::{
    caret_position, rasterize_aligned, text_area, Align, Font, MAX_CHARS, MAX_SCALE, MAX_SPACING,
    MAX_TEXT_AREA,
};
use pikslar_core::{Clip, FloatingCmd};
use pikslar_platform::{Key, Mods};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, draw_text, draw_text_edit, edit_for, text_width, Edit, TextEdit};
use pikslar_ui::{cap_height, em, multi_line};
use std::sync::Arc;

/// The bar's field and its four buttons take five ids in a row.
pub(crate) const ID_TEXT: u32 = 185;
/// How many edits Ctrl+Z can take back within a session.
const UNDO_DEPTH: usize = 256;
const BIG_STEP: i32 = 8;
/// Said when an edit or a larger size would make the drawing too large to make.
const TOO_LARGE: &str = "The text would be too large to draw: use fewer lines or a smaller size";

/// How text is drawn, kept from one session to the next.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextStyle {
    pub font: Font,
    pub scale: i32,
    pub spacing: i32,
    pub align: Align,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            font: Font::Pikslar5x7,
            scale: 1,
            spacing: 1,
            align: Align::Left,
        }
    }
}

/// One of the three things a key or button changes about how text is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextSetting {
    Font,
    Scale,
    Spacing,
    Align,
}

pub(crate) struct TextSession {
    pub edit: TextEdit,
    pub font: Font,
    pub scale: i32,
    pub spacing: i32,
    pub align: Align,
    /// The characters the font lacks, drawn as question marks.
    pub missing: Vec<char>,
    /// What the text was before each change, for Ctrl+Z.
    undo: Vec<TextEdit>,
    /// The color the preview was drawn in.
    color: u32,
}

impl TextSession {
    /// Whether the drawing of the text as it stands is small enough to make.
    fn fits(&self) -> bool {
        self.fits_text(self.edit.text())
    }

    /// Whether the drawing of `text` at the session's size is small enough to make.
    fn fits_text(&self, text: &str) -> bool {
        text_area(self.font, text, self.scale, self.spacing) <= MAX_TEXT_AREA
    }

    fn new(style: TextStyle, color: u32) -> Self {
        TextSession {
            edit: TextEdit::new("", MAX_CHARS),
            font: style.font,
            scale: style.scale,
            spacing: style.spacing,
            align: style.align,
            missing: Vec::new(),
            undo: Vec::new(),
            color,
        }
    }
}

fn arrow(key: Key) -> Option<(i32, i32)> {
    match key {
        Key::Left => Some((-1, 0)),
        Key::Right => Some((1, 0)),
        Key::Up => Some((0, -1)),
        Key::Down => Some((0, 1)),
        _ => None,
    }
}

fn setting_of(action: Action) -> Option<TextSetting> {
    match action {
        Action::TextFont => Some(TextSetting::Font),
        Action::TextScale => Some(TextSetting::Scale),
        Action::TextSpacing => Some(TextSetting::Spacing),
        Action::TextAlign => Some(TextSetting::Align),
        _ => None,
    }
}

/// `value` one step on within `low..=high`, wrapping round.
fn stepped(value: i32, (low, high): (i32, i32), back: bool) -> i32 {
    let span = high - low + 1;
    low + (value - low + if back { span - 1 } else { 1 }) % span
}

impl App {
    /// Ends the session if something else has taken the pasted pixels, which anchors them.
    pub(crate) fn sync_text(&mut self) {
        if self.text.is_some() && !self.editor.is_pasting() {
            self.text = None;
            self.full_canvas = true;
            self.ui_dirty = true;
        }
    }

    /// A press on the canvas with the text tool: inside the text it drags it, elsewhere it
    /// keeps the text and starts new text there.
    pub(crate) fn text_down(&mut self, x: i32, y: i32) {
        self.sync_text();
        let (x, y) = self.view.to_doc(x, y);
        let inside = self.text.is_some()
            && self.editor.selection().is_some_and(|s| {
                let (bx, by, w, h) = s.bounds();
                w > 0 && (bx..bx + w).contains(&x) && (by..by + h).contains(&y)
            });
        if inside {
            self.gesture = Gesture::Grab {
                x,
                y,
                copy: false,
                auto: false,
            };
            return;
        }
        self.commit_text();
        self.start_text(x, y);
    }

    /// Opens a session with its top-left at a position. False if the layer refuses it.
    pub(crate) fn start_text(&mut self, x: i32, y: i32) -> bool {
        let clip = Arc::new(Clip::from_image(0, 0, Vec::new()));
        if self.run(FloatingCmd::Paste { clip, x, y }).is_none() {
            return false;
        }
        // A playing animation would move the frame and anchor the empty paste at the next tick.
        self.playing = false;
        self.text = Some(TextSession::new(self.text_style, self.editor.color));
        self.full_canvas = true;
        true
    }

    /// Where a session started from the keyboard begins: the selection's corner, or the middle
    /// of what is on screen.
    pub(crate) fn start_text_by_key(&mut self) {
        let (x, y) = match self.editor.selection().map(|s| s.bounds()) {
            Some((x, y, ..)) => (x, y),
            None => self.view_center(),
        };
        self.start_text(x, y);
    }

    /// Keeps the text, or leaves nothing behind if there is none.
    pub(crate) fn commit_text(&mut self) {
        let Some(session) = self.text.take() else {
            return;
        };
        if session.edit.text().trim().is_empty() {
            self.run(FloatingCmd::Revert);
        } else if !session.fits() {
            // Nothing was drawn for it, so keeping it would anchor an empty paste.
            self.run(FloatingCmd::Revert);
            self.notify_error(format!("{TOO_LARGE}, so it was not kept"));
        } else {
            self.run(FloatingCmd::Anchor);
        }
        self.full_canvas = true;
    }

    pub(crate) fn cancel_text(&mut self) {
        if self.text.take().is_some() {
            self.run(FloatingCmd::Revert);
            self.full_canvas = true;
        }
    }

    /// Draws the text again, in the current color, in place of what was floating. False, and
    /// said, if the layer would not take it: what was drawn then stays, with its color.
    pub(crate) fn refresh_text(&mut self) -> bool {
        let color = self.editor.color;
        let Some(t) = &self.text else {
            return false;
        };
        let size = (t.scale, t.spacing);
        let (clip, missing) = rasterize_aligned(t.font, size, t.align, t.edit.text(), color);
        let clip = Arc::new(clip);
        self.full_canvas = true;
        if self.run(FloatingCmd::ReplacePasted { clip }).is_none() {
            return false;
        }
        if let Some(t) = &mut self.text {
            (t.missing, t.color) = (missing, color);
        }
        true
    }

    /// The redraw for a color changed since the preview was drawn. If it fails it is said once,
    /// and not tried again until the color changes once more.
    fn refresh_stale_text(&mut self) {
        if !self.refresh_text() {
            let color = self.editor.color;
            if let Some(t) = &mut self.text {
                t.color = color;
            }
        }
    }

    /// Makes an edit to the text. A change draws it again.
    pub(crate) fn edit_text(&mut self, edit: Edit) {
        let Some(t) = &mut self.text else {
            return;
        };
        let before = t.edit.clone();
        let full = before.text().chars().count() >= MAX_CHARS;
        let changed = t.edit.apply(edit);
        let too_large = changed && !t.fits();
        if too_large {
            t.edit = before;
        } else if changed {
            t.undo.push(before);
        }
        self.full_canvas = true;
        self.ui_dirty = true;
        if changed && !too_large {
            self.redrawn_or_undone(None);
        } else if too_large {
            self.notify_error(TOO_LARGE.to_string());
        } else if full && matches!(edit, Edit::Insert(_) | Edit::Newline) {
            self.notify(format!("Text stops at {MAX_CHARS} letters"));
        }
    }

    /// Draws the text just changed. If the layer refuses, the change is taken back, both the
    /// text and its undo entry, so the canvas and the field agree. `undone_from` is the text an
    /// undo has just replaced: that undo's entry is taken off only once it has worked.
    fn redrawn_or_undone(&mut self, undone_from: Option<TextEdit>) {
        let ok = self.refresh_text();
        let Some(t) = &mut self.text else {
            return;
        };
        match (ok, undone_from) {
            (true, Some(_)) => {
                t.undo.pop();
            }
            (true, None) => {
                if t.undo.len() > UNDO_DEPTH {
                    t.undo.remove(0);
                }
            }
            (false, Some(now)) => t.edit = now,
            (false, None) => {
                if let Some(before) = t.undo.pop() {
                    t.edit = before;
                }
            }
        }
    }

    /// Ctrl+Z inside a session: the text as it was before the last change.
    fn text_undo(&mut self) {
        let Some(t) = &mut self.text else {
            return;
        };
        let Some(before) = t.undo.last().cloned() else {
            return;
        };
        let now = std::mem::replace(&mut t.edit, before);
        if t.fits() {
            self.redrawn_or_undone(Some(now));
        } else {
            // Too large at a size set since: the text stays as it is.
            t.edit = now;
            self.notify_error(TOO_LARGE.to_string());
        }
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    fn move_text(&mut self, dx: i32, dy: i32) {
        self.run(FloatingCmd::Move {
            dx,
            dy,
            join: false,
        });
        self.full_canvas = true;
    }

    /// Steps font, scale or spacing on, or back, for this session if there is one and the next.
    /// A change that would make the open session's drawing too large is refused and said.
    pub(crate) fn cycle_text(&mut self, setting: TextSetting, back: bool) {
        let was = self.text_style;
        let style = &mut self.text_style;
        match setting {
            TextSetting::Font => style.font = style.font.step(back),
            TextSetting::Scale => style.scale = stepped(style.scale, (1, MAX_SCALE), back),
            TextSetting::Spacing => style.spacing = stepped(style.spacing, (0, MAX_SPACING), back),
            TextSetting::Align => style.align = style.align.step(back),
        }
        let style = *style;
        self.ui_dirty = true;
        let Some(t) = &mut self.text else {
            return;
        };
        let kept = (t.font, t.scale, t.spacing, t.align);
        (t.font, t.scale, t.spacing, t.align) =
            (style.font, style.scale, style.spacing, style.align);
        let fits = t.fits();
        if fits && self.refresh_text() {
            return;
        }
        if let Some(t) = &mut self.text {
            (t.font, t.scale, t.spacing, t.align) = kept;
        }
        self.text_style = was;
        if !fits {
            self.notify_error(TOO_LARGE.to_string());
        }
    }

    /// Types text that was pasted, one line of it and no more than the field has room for, which
    /// is said once rather than for every character left over.
    pub(crate) fn insert_pasted_text(&mut self, text: &str) {
        let line = if self.text.is_some() {
            multi_line(text)
        } else {
            pikslar_ui::one_line(text)
        };
        let room = self.naming_field().map_or(0, TextEdit::room);
        if self.text.is_some() {
            self.paste_into_session(&line, room);
            return;
        }
        line.chars().take(room).for_each(|c| {
            let edit = if c == char::from(10) {
                Edit::Newline
            } else {
                Edit::Insert(c)
            };
            self.edit_naming(edit);
        });
        if line.chars().nth(room).is_some() {
            self.notify("The pasted text was cut to fit".to_string());
        }
    }

    /// A paste into a session is one change: one undo entry and one redraw. If the whole would
    /// make the drawing too large, the longest prefix that fits is kept and that is said.
    fn paste_into_session(&mut self, line: &str, room: usize) {
        let Some(t) = &self.text else {
            return;
        };
        let before = t.edit.clone();
        let mut working = before.clone();
        let mut kept = 0;
        for c in line.chars().take(room) {
            let edit = if c == char::from(10) {
                Edit::Newline
            } else {
                Edit::Insert(c)
            };
            let prev = working.clone();
            working.apply(edit);
            if !t.fits_text(working.text()) {
                working = prev;
                break;
            }
            kept += 1;
        }
        let cut_by_size = kept < line.chars().take(room).count();
        if let Some(t) = &mut self.text {
            if working.text() != before.text() {
                t.edit = working;
                t.undo.push(before);
            }
        }
        self.full_canvas = true;
        self.ui_dirty = true;
        if kept > 0 {
            self.redrawn_or_undone(None);
        }
        if cut_by_size {
            let said = if kept > 0 {
                "The pasted text was cut: the drawing would be too large"
            } else {
                TOO_LARGE
            };
            self.notify_error(said.to_string());
        } else if line.chars().nth(room).is_some() {
            self.notify("The pasted text was cut to fit".to_string());
        }
    }

    /// A key while a session is open. Returns whether the session took it; a Ctrl chord it has
    /// no use for goes on to the shortcuts.
    pub(crate) fn text_key(&mut self, key: Key, mods: Mods) -> bool {
        if self.text.is_none() {
            return false;
        }
        if self.text_focus_key(key, mods) {
            return true;
        }
        // Plain arrows move the caret, so Alt moves the text.
        if let Some((dx, dy)) = arrow(key).filter(|_| mods.alt) {
            let step = if mods.shift { BIG_STEP } else { 1 };
            self.move_text(dx * step, dy * step);
            return true;
        }
        match key {
            Key::Enter if mods.shift => self.edit_text(Edit::Newline),
            Key::Enter => self.commit_text(),
            Key::Escape => self.cancel_text(),
            Key::Char(letter) if mods.ctrl && self.text_history_key(letter, mods) => {}
            Key::Char(letter) if mods.ctrl && self.text_setting_key(letter, mods) => {}
            _ => match edit_for(key, mods) {
                Some(edit) => self.edit_text(edit),
                None => return !mods.ctrl,
            },
        }
        true
    }

    /// Tab moves focus off the field to the bar's buttons; Enter and Space press the one with
    /// focus (they would otherwise keep the text or type a space).
    fn text_focus_key(&mut self, key: Key, mods: Mods) -> bool {
        let on_button = self
            .ui
            .focus
            .focused()
            .is_some_and(|id| (ID_TEXT + 1..=ID_TEXT + 6).contains(&id));
        let used = match key {
            Key::Tab if !mods.ctrl => {
                self.ui.focus.step(mods.shift);
                true
            }
            Key::Enter | Key::Space if on_button && !mods.shift => {
                // The character Windows sends next is not typed into the text.
                self.key_text_spent = Some(key);
                self.ui.focus.request_activate()
            }
            _ => false,
        };
        self.ui_dirty |= used;
        used
    }

    /// The keys bound to Undo and Redo, whatever they are set to: Undo steps back through the
    /// session's own edits, and Redo, which a session has none of, is kept from reaching the
    /// document, whose history would throw the session away.
    fn text_history_key(&mut self, letter: u8, mods: Mods) -> bool {
        let pressed = Binding {
            letter,
            ctrl: mods.ctrl,
            shift: mods.shift,
        };
        match self.keymap.action_for(pressed) {
            Some(Action::Undo) => self.text_undo(),
            Some(Action::Redo | Action::RedoOther) => {}
            _ => return false,
        }
        true
    }

    /// Ctrl+F, Ctrl+T and Ctrl+H, as bound: font, size and letter gap. Shift goes back.
    fn text_setting_key(&mut self, letter: u8, mods: Mods) -> bool {
        let pressed = Binding {
            letter,
            ctrl: mods.ctrl,
            shift: false,
        };
        let setting = self.keymap.action_for(pressed).and_then(setting_of);
        setting
            .inspect(|&s| self.cycle_text(s, mods.shift))
            .is_some()
    }

    /// A press outside the canvas keeps the text, unless it is on the text's own controls.
    pub(crate) fn text_press_elsewhere(&mut self, x: i32, y: i32) {
        self.sync_text();
        let own = (ID_TEXT..ID_TEXT + 7).contains(&self.ui.widget_at(x, y));
        if self.text.is_some() && !own && !self.on_canvas(x, y) {
            self.commit_text();
        }
    }

    /// The one-pixel caret on the canvas: it marks where the next character goes, and does not
    /// blink, so an open session costs nothing while idle.
    pub(crate) fn draw_text_caret(&self, surface: &mut Surface<'_>, area: Rect) {
        let (Some(t), Some(selection)) = (&self.text, self.editor.selection()) else {
            return;
        };
        let (x, y, ..) = selection.bounds();
        let size = (t.scale, t.spacing);
        let (dx, dy) = caret_position(t.font, size, t.align, t.edit.text(), t.edit.caret());
        let caret = Rect::new(x + dx, y + dy, 1, t.font.line_rows() * t.scale);
        surface.fill(self.view.to_screen(caret).intersect(area), theme::accent());
    }

    /// The field and its controls, in the context bar: the field, its four settings, the
    /// letters that cannot be drawn, and Keep and Cancel after the settings.
    pub(crate) fn draw_text_bar(&mut self, sf: &mut Surface<'_>, left: i32, y: i32, limit: i32) {
        let Some(t) = &self.text else {
            return;
        };
        let (edit, missing) = (t.edit.clone(), t.missing.clone());
        let labels = [
            t.font.name().to_string(),
            format!("Size {}", t.scale),
            format!("Gap {}", t.spacing),
            t.align.name().to_string(),
        ];
        let stale = t.color != self.editor.color;
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let widths = labels.each_ref().map(|l| text_width(l, ts) + 14 * s);
        let buttons: i32 = widths.iter().map(|w| w + 4 * s).sum();
        let ends = [("Keep", ID_TEXT + 5), ("Cancel", ID_TEXT + 6)];
        let end_w = ends.map(|(l, _)| text_width(l, ts) + 14 * s);
        let end_room = end_w.iter().map(|w| w + 4 * s).sum::<i32>() + 4 * s;
        let room = (limit - left - buttons - end_room - 4 * s).max(0);
        let field = Rect::new(left, y, room.min(48 * em() * ts), control() * s);
        if field.w >= 8 * em() * ts {
            self.draw_text_field(sf, field, &edit);
            self.draw_text_buttons(sf, field.right() + 4 * s, y, &labels, widths);
            // Keep and Cancel follow the settings, beside the text they act on; the letters
            // that cannot be drawn come after them.
            let ends_x = field.right() + 4 * s + buttons + 4 * s;
            self.draw_text_ends(sf, (ends_x, y), ends, end_w);
            self.draw_missing(sf, ends_x + end_room + 4 * s, limit, &missing);
        }
        if stale {
            self.defer(|a| a.refresh_stale_text());
        }
    }

    /// Keep and Cancel, which Enter and Esc also do.
    fn draw_text_ends(
        &mut self,
        sf: &mut Surface<'_>,
        (left, y): (i32, i32),
        ends: [(&str, u32); 2],
        widths: [i32; 2],
    ) {
        let s = self.ui.scale;
        let hints = ["Keep the text  Enter", "Cancel the text  Esc"];
        let names = ["Keep text", "Cancel text"];
        let mut x = left;
        for i in 0..2 {
            let rect = Rect::new(x, y, widths[i], control() * s);
            self.note_hint(rect, hints[i]);
            let id = (ends[i].1, rect);
            if self.bar_button(sf, id, (ends[i].0, names[i]), None) {
                self.defer(move |a| {
                    if i == 0 {
                        a.commit_text()
                    } else {
                        a.cancel_text()
                    }
                });
            }
            x += widths[i] + 4 * s;
        }
    }

    fn draw_text_field(&mut self, sf: &mut Surface<'_>, field: Rect, edit: &TextEdit) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        self.note_hint(
            field,
            "Type the text  Enter keeps it, Shift Enter breaks the line, Esc cancels, Alt+arrows move it",
        );
        self.ui.interact(ID_TEXT, field);
        pikslar_ui::chrome::well(sf, field, s, theme::panel(), false);
        let at = (field.x + 6 * s, field.y + (field.h - cap_height() * ts) / 2);
        let shown = edit.mapped(|c| {
            if c == char::from(10) {
                char::from(0xb6)
            } else {
                c
            }
        });
        draw_text_edit(
            sf,
            at,
            field.w - 12 * s,
            &shown,
            (theme::text(), theme::accent()),
            ts,
        );
    }

    fn draw_text_buttons(
        &mut self,
        sf: &mut Surface<'_>,
        left: i32,
        y: i32,
        labels: &[String; 4],
        widths: [i32; 4],
    ) {
        const SETTINGS: [TextSetting; 4] = [
            TextSetting::Font,
            TextSetting::Scale,
            TextSetting::Spacing,
            TextSetting::Align,
        ];
        let s = self.ui.scale;
        let mut x = left;
        for i in 0..4 {
            let rect = Rect::new(x, y, widths[i], control() * s);
            let hint = self.keymap.text_setting_hint(i);
            self.note_hint(rect, hint);
            if button(
                &mut self.ui,
                sf,
                ID_TEXT + 1 + i as u32,
                rect,
                &labels[i],
                false,
            ) {
                let (setting, back) = (SETTINGS[i], self.ui.shift);
                self.defer(move |a| a.cycle_text(setting, back));
            }
            x += widths[i] + 4 * s;
        }
    }

    fn draw_missing(&self, sf: &mut Surface<'_>, x: i32, limit: i32, missing: &[char]) {
        if missing.is_empty() {
            return;
        }
        let ts = self.ui.text_scale();
        let list: String = missing.iter().collect();
        let note = format!("Drawn as ?: {list}");
        let y = self.layout.context.y + (self.layout.context.h - cap_height() * ts) / 2;
        if x + text_width(&note, ts) <= limit {
            draw_text(sf, x, y, &note, theme::muted(), ts);
        }
    }

    /// Ctrl+F, Ctrl+T and Ctrl+H with no session open: they set the style for the next one.
    pub(crate) fn text_style_action(&mut self, action: Action, back: bool) -> bool {
        let Some(setting) = setting_of(action) else {
            return false;
        };
        self.cycle_text(setting, back);
        true
    }
}
