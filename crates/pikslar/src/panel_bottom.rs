//! The bottom panel: frames, playback and the Rewind history scrubber.

use crate::app::App;
use crate::timeline_geom::Geom;
use pikslar_core::StepCmd;
use pikslar_render::{Rect, Surface};
pub(crate) use pikslar_ui::fit_to_width;
use pikslar_ui::theme;
use pikslar_ui::StackText;
use pikslar_ui::{
    cap_height, draw_text, icon_button, icon_button_if, slider_stepped, text_width, Icon, Slide,
};
use std::borrow::Cow;

pub(crate) const ID_TRANSPORT: u32 = 300;
const ID_REWIND: u32 = 330;
const TRANSPORT_HINTS: [&str; 7] = [
    "Play  Enter",
    "Previous frame  ,",
    "Next frame  .",
    "New frame  N",
    "Duplicate frame  D",
    "Remove frame  Shift Delete",
    "Onion skin  O, depth  Shift O. Right click for more",
];
const REMOVE_SELECTED_HINT: &str = "Remove the selected frames  Shift Delete";
const REMOVE_ACTION: usize = 5;
/// The widest edit counter the Rewind row keeps room for, so the bar never moves as it counts.
const COUNTER_ROOM: &str = "Edit 9999 of 9999";
/// The Rewind bar's least and largest length, in units.
const BAR_LEAST: i32 = 48;
const BAR_MOST: i32 = 360;

/// Rewind's count of edits: how many of them are applied now.
fn edit_counter(undo: usize, total: usize) -> StackText<40> {
    if total == 0 {
        StackText::new(format_args!("No edits"))
    } else {
        StackText::new(format_args!("Edit {undo} of {total}"))
    }
}

/// Where the parts of the Rewind row sit: the bar, the counter, and the room the status
/// message has. The bar takes at most half of what the label and counter leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RewindRow {
    pub track: Rect,
    pub counter: Rect,
    pub status: Rect,
}

impl RewindRow {
    pub(crate) fn of(area: Rect, s: i32, ts: i32) -> RewindRow {
        let (gap, right) = (8 * s, area.right().max(area.x));
        // Each part starts no further right than the area ends and is cut at its end, so a
        // narrow dock clips the row rather than drawing past it.
        let from = |x: i32| x.min(right);
        let left = from(area.x + text_width("Rewind", ts) + gap);
        let counter_w = text_width(COUNTER_ROOM, ts);
        let room = (right - left - 2 * gap - counter_w).max(0);
        let bar = (room / 2)
            .clamp(BAR_LEAST * s, BAR_MOST * s)
            .min(right - left);
        let track = Rect::new(left, area.y + 2 * s, bar, (area.h - 4 * s).max(0));
        let counter_x = from(track.right() + gap);
        let counter = Rect::new(counter_x, area.y, counter_w.min(right - counter_x), area.h);
        let status_x = from(counter_x + counter_w + gap);
        let status = Rect::new(status_x, area.y, right - status_x, area.h);
        RewindRow {
            track,
            counter,
            status,
        }
    }
}

impl App {
    /// The Timeline panel in `panel`: the tags row, the single line, the cel rows when open and
    /// the status row (docs/ui-redesign.md 3.5).
    pub(crate) fn draw_timeline(&mut self, sf: &mut Surface<'_>, panel: Rect) {
        let s = self.ui.scale;
        sf.fill(Rect::new(panel.x, panel.y, panel.w, s), theme::rule());
        let g = Geom::of(panel, s, self.workspaces.timeline_names);
        self.draw_transport(sf, &g);
        self.draw_frame_strip(sf, g.strip);
        self.draw_tag_row(sf, g.tags);
        let ts = self.ui.text_scale();
        let label_y = g.tags_label.y + (g.tags_label.h - cap_height() * ts) / 2;
        draw_text(sf, g.tags_label.x, label_y, "Tags", theme::muted(), ts);
        self.draw_fold(sf, g.chevron);
        self.draw_matrix(sf, &g);
        self.draw_duration(sf, g.timing);
        // The status bar is a Header band under a rule, across the whole panel.
        let band = Rect::new(panel.x, g.status.y - s, panel.w, g.status.h + 2 * s);
        sf.fill(band, theme::header());
        sf.fill(Rect::new(band.x, band.y, band.w, s), theme::rule());
        self.draw_status_row(sf, g.status);
    }

    /// The seven buttons of the line: play, previous and next left of the strip, then new,
    /// duplicate, remove and onion skin right of it.
    fn draw_transport(&mut self, sf: &mut Surface<'_>, g: &Geom) {
        let play_icon = if self.playing {
            Icon::Pause
        } else {
            Icon::Play
        };
        let icons = [
            play_icon,
            Icon::Prev,
            Icon::Next,
            Icon::Plus,
            Icon::Copy,
            Icon::Minus,
            Icon::Onion,
        ];
        for (i, icon) in icons.iter().enumerate() {
            let r = g.transport[i];
            let selected = (i == 0 && self.playing) || (i == 6 && self.comp.onion);
            if i == 6 {
                self.onion_button = r;
            }
            self.note_hint(r, self.transport_hint(i));
            let id = ID_TRANSPORT + i as u32;
            let clicked = if i == REMOVE_ACTION {
                let usable = self.editor.doc().frames.len() > 1;
                icon_button_if(usable, &mut self.ui, sf, id, r, *icon)
            } else {
                icon_button(&mut self.ui, sf, id, r, *icon, selected)
            };
            if clicked {
                self.defer(move |a| a.transport_action(i));
            }
        }
    }

    /// The foot of the body: messages and hints, and Rewind's bar unless the History panel
    /// is placed in the arrangement, where it has the bar.
    fn draw_status_row(&mut self, sf: &mut Surface<'_>, area: Rect) {
        if self.history_placed() {
            self.draw_idle_status(sf, area);
        } else {
            self.draw_rewind(sf, area);
        }
    }

    /// The status bar when Rewind lives in the History panel: the frame on the left and the
    /// edit count on the right always, a message in their place while there is one.
    fn draw_idle_status(&mut self, sf: &mut Surface<'_>, area: Rect) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let text_y = area.y + (area.h - cap_height() * ts) / 2;
        let history = self.editor.history();
        let (undo, total) = (
            history.undo_depth(),
            history.undo_depth() + history.redo_depth(),
        );
        let counter = edit_counter(undo, total);
        let counter_w = text_width(counter.as_str(), ts);
        draw_text(
            sf,
            area.right() - counter_w,
            text_y,
            counter.as_str(),
            theme::muted(),
            ts,
        );
        let track = Rect::new(area.x, area.y, area.w - counter_w - 12 * s, area.h);
        let said = self.status_text().is_some();
        self.draw_status(sf, track, text_y);
        if said {
            return;
        }
        let frame = self.editor.frame + 1;
        let frames = self.editor.doc().frames.len();
        let idle = StackText::<32>::new(format_args!("Frame {frame} of {frames}"));
        draw_text(sf, area.x, text_y, idle.as_str(), theme::muted(), ts);
    }

    fn transport_hint(&self, action: usize) -> Cow<'static, str> {
        if action == REMOVE_ACTION && self.selected_frames().is_some() {
            Cow::Borrowed(REMOVE_SELECTED_HINT)
        } else if let Some(text) = self.keymap.transport_hint(action) {
            Cow::Owned(text)
        } else {
            Cow::Borrowed(TRANSPORT_HINTS[action])
        }
    }

    fn transport_action(&mut self, action: usize) {
        match action {
            0 => self.toggle_play(),
            1 => self.step_frame(false),
            2 => self.step_frame(true),
            3 => self.new_frame(false),
            4 => self.new_frame(true),
            5 => self.remove_frames(),
            _ => self.toggle_onion(),
        }
        self.ui_dirty = true;
    }

    /// Rewind: drag through every edit made in this session. Left to right: the label, the
    /// bar, the edit counter, then the status message in the room that is left, so the two
    /// never share pixels (final review, defect 1).
    pub(crate) fn draw_rewind(&mut self, sf: &mut Surface<'_>, area: Rect) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let text_y = area.y + (area.h - cap_height() * ts) / 2;
        let name = fit_to_width("Rewind", area.w, ts);
        draw_text(sf, area.x, text_y, &name, theme::muted(), ts);
        let history = self.editor.history();
        let (undo, total) = (
            history.undo_depth(),
            history.undo_depth() + history.redo_depth(),
        );
        let row = RewindRow::of(area, s, ts);
        // On the label: a hint on the bar would cover the handle being reached for.
        let label = Rect::new(area.x, area.y, row.track.x - area.x, area.h);
        self.note_hint(
            label,
            "Rewind: drag the bar through every edit you have made",
        );
        let counter = edit_counter(undo, total);
        let counter = fit_to_width(counter.as_str(), row.counter.w, ts);
        draw_text(sf, row.counter.x, text_y, &counter, theme::muted(), ts);
        self.rewind_bar(sf, row.track, (undo, total));
        self.draw_status(sf, row.status, text_y);
    }

    /// Rewind's bar in `track`: a well with the edits made so far filled, and its handle.
    fn rewind_bar(&mut self, sf: &mut Surface<'_>, track: Rect, (undo, total): (usize, usize)) {
        let s = self.ui.scale;
        let value = if total == 0 {
            1.0
        } else {
            undo as f32 / total as f32
        };
        let filled = (value * (track.w - 2 * s) as f32) as i32;
        let paint = |sf: &mut Surface<'_>, r: Rect| {
            pikslar_ui::chrome::well(sf, r, s, theme::header(), false);
            let inner = r.inset(s);
            sf.fill(
                Rect::new(inner.x, inner.y, filled, inner.h),
                theme::raised(),
            );
        };
        let unit = 1.0 / total.max(1) as f32;
        let slid = slider_stepped(&mut self.ui, sf, ID_REWIND, track, value, unit, paint);
        if let Some(Slide { value, .. }) = slid {
            let depth = (value * total as f32).round() as usize;
            self.defer(move |a| {
                if a.settle() {
                    a.run(StepCmd::ScrubTo { depth });
                }
            });
        }
    }

    /// What saving is doing, at the right of `room`, then messages and hints right-aligned in
    /// what is left of it and cut to fit.
    pub(crate) fn draw_status(&mut self, sf: &mut Surface<'_>, room: Rect, text_y: i32) {
        let room = self.draw_save_label(sf, room, text_y);
        let Some(text) = self.status_text().map(str::to_owned) else {
            return;
        };
        let error = self.message_is_error && self.message.is_some();
        self.draw_status_words(sf, room, text_y, (&text, "Status"), error);
    }

    /// The save state at the right of `room`, in at most half of it. Returns the room left.
    fn draw_save_label(&mut self, sf: &mut Surface<'_>, room: Rect, text_y: i32) -> Rect {
        let Some(label) = self.save_label() else {
            return room;
        };
        let half = Rect::new(room.x + room.w / 2, room.y, room.w - room.w / 2, room.h);
        let width = self.draw_status_words(sf, half, text_y, (&label.text, "Save"), label.warning);
        let gap = 12 * self.ui.scale;
        Rect::new(room.x, room.y, (room.w - width - gap).max(0), room.h)
    }

    /// `text` right-aligned in `room`, in danger ink when `error`, named `kind` in the layout
    /// dump. Returns the width drawn.
    fn draw_status_words(
        &mut self,
        sf: &mut Surface<'_>,
        room: Rect,
        text_y: i32,
        (text, kind): (&str, &str),
        error: bool,
    ) -> i32 {
        let ts = self.ui.text_scale();
        // In high contrast Danger is the ink, so an error carries a mark of its own.
        let lead = if error && pikslar_ui::icons::high_contrast() {
            text_width("! ", ts)
        } else {
            0
        };
        let shown = fit_to_width(text, (room.w - lead).max(0), ts);
        let width = text_width(&shown, ts) + lead;
        let x = room.right() - width;
        // Named for scripts, which read what the status line says from the layout dump.
        if self.ui.naming() {
            let plate = Rect::new(x, room.y, width, room.h);
            self.ui.name_rect(plate, &format!("{kind} {text}"));
        }
        let ink = if error {
            theme::danger()
        } else {
            theme::text()
        };
        if lead > 0 {
            draw_text(sf, x, text_y, "!", ink, ts);
        }
        draw_text(sf, x + lead, text_y, &shown, ink, ts);
        width
    }

    pub(crate) fn status_text(&self) -> Option<&str> {
        // A message that arrives while a file is read is news, an error or a failed backup, and
        // goes ahead of the label. Starting a read clears what was said before it.
        if let Some((text, _)) = &self.message {
            return Some(text);
        }
        if let Some(loading) = &self.loading {
            return Some(&loading.label);
        }
        if self.timeline.naming.is_some() {
            return Some("Type a name. Enter keeps it, Esc cancels, Shift Delete removes the tag");
        }
        if self.layer_naming.is_some() {
            return Some("Type the layer's name. Enter keeps it, Esc cancels");
        }
        if self.slice_naming.is_some() {
            return Some("Type the slice's name. Enter keeps it, Esc cancels");
        }
        if self.palette_ui.naming.is_some() {
            return Some("Type the group's name. Enter keeps it, Esc cancels");
        }
        if self.tile_ui.naming.is_some() {
            return Some("Type the tileset's name. Enter keeps it, Esc cancels");
        }
        if self.brush_ui.naming.is_some() {
            return Some("Type the brush's name. Enter keeps it, Esc cancels");
        }
        if self.hint.is_some() {
            return self.hint.as_deref();
        }
        // The text tool's button says Keep, so its status line does too.
        let floating = if self.text.is_some() {
            "Enter keeps the text, Esc cancels"
        } else {
            "Enter applies, Esc cancels"
        };
        self.editor
            .is_floating()
            .then_some(floating)
            .or_else(|| self.slice_hint())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_text_is_cut_to_fit_with_an_ellipsis_and_never_splits_a_character() {
        let (text, scale) = ("Saved C:/art/sprite.piks", 2);
        assert_eq!(fit_to_width(text, 100_000, scale), text);
        let width = text_width("Saved...", scale);
        let cut = fit_to_width(text, width, scale);
        assert_eq!(cut, "Saved...");
        assert!(text_width(&cut, scale) <= width);
        assert_eq!(
            fit_to_width("abcdef", text_width("a...", scale), scale),
            "a..."
        );
        assert_eq!(
            fit_to_width("中bcdef", text_width("中...", scale), scale),
            "中..."
        );
        let narrow = text_width("..", scale);
        let dots = fit_to_width("Ã©â‚¬x", narrow, scale);
        assert!(dots.chars().all(|c| c == '.') && text_width(&dots, scale) <= narrow);
    }

    #[test]
    fn fitting_a_very_long_message_takes_linear_time() {
        let long = "é".repeat(64 << 10);
        let start = std::time::Instant::now();
        let cut = fit_to_width(&long, 400, 2);
        let took = start.elapsed();
        assert!(!cut.is_empty() && text_width(&cut, 2) <= 400);
        assert!(cut.ends_with("..."), "the cut is marked");
        assert!(took < std::time::Duration::from_millis(500), "{took:?}");
    }
}
