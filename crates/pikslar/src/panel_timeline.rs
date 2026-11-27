//! The frame strip, the tags above it, and frame timing.

use crate::app::App;
use crate::panel_bottom::fit_to_width;
use crate::timeline::{TimingDrag, ID_TAG};
use pikslar_core::{ticks, Direction, StepCmd, Tag};
use pikslar_render::{Rect, Surface};
use pikslar_ui::chrome;
use pikslar_ui::icons::ActionIcon;
use pikslar_ui::role::State;
use pikslar_ui::theme;
use pikslar_ui::{cap_height, draw_text, draw_text_edit, em, text_width, Interaction};

const ID_DURATION: u32 = 320;
pub(crate) const ID_READOUT: u32 = ID_DURATION + 2;
const ID_STRIP: u32 = 200_000;
pub(crate) const CELL: i32 = 34;
/// Screen units of drag for one step of duration.
const DRAG_PER_STEP: i32 = 3;

fn arrow(tag: &Tag) -> &'static str {
    match tag.direction {
        Direction::Forward => "",
        Direction::Reverse => " <",
        Direction::PingPong => " <>",
    }
}

/// A tag's name, which way it plays and how many times; a tag being named keeps room for the
/// caret.
pub(crate) fn tag_label(tag: &Tag, naming: bool) -> String {
    let caret = if naming { " " } else { "" };
    let times = match tag.repeat {
        0 => String::new(),
        n => format!(" ×{n}"),
    };
    format!("{}{caret}{}{times}", tag.name, arrow(tag))
}

/// The top of a frame's number: centred in the room above the cel mark at the cell's foot.
pub(crate) fn number_y(r: Rect, s: i32, ts: i32) -> i32 {
    let room = r.h - MARK_ROOM * s;
    r.y + ((room - cap_height() * ts) / 2).max(s)
}

/// The room the cel mark keeps at the foot of a frame cell, in units.
const MARK_ROOM: i32 = 6;

impl App {
    fn scrub(&mut self, state: Interaction, under: usize, edge: i32) {
        if state.started {
            self.playing = false;
            if !self.start_run_drag(under) {
                let shift = self.ui.shift;
                self.timeline.press(self.editor.frame, under, shift);
            }
        }
        if self.carry_run(state, under) {
            self.timeline.edge_scrub = false;
            return;
        }
        self.timeline.edge_scrub = state.held && edge != 0;
        if !state.held {
            return;
        }
        let target = if edge == 0 {
            under
        } else if self.timeline.edge_step_due() {
            let last = self.editor.doc().frames.len() as i64 - 1;
            (self.editor.frame as i64 + edge as i64).clamp(0, last) as usize
        } else {
            self.editor.frame
        };
        self.timeline.drag_to(target);
        if target != self.editor.frame {
            self.defer(move |a| a.set_frame(target));
        }
    }

    /// The frame drawn at a point of the strip, if any.
    pub(crate) fn frame_at(&self, x: i32, y: i32) -> Option<usize> {
        let strip = self.timeline.strip;
        if !strip.contains(x, y) {
            return None;
        }
        let frame = self.timeline.first() + ((x - strip.x) / (CELL * self.ui.scale)) as usize;
        (frame < self.editor.doc().frames.len()).then_some(frame)
    }

    pub(crate) fn draw_frame_strip(&mut self, sf: &mut Surface<'_>, area: Rect) {
        let s = self.ui.scale;
        let cell = CELL * s;
        let visible = (area.w / cell).max(1) as usize;
        let count = self.editor.doc().frames.len();
        let first = self.timeline.scrolled(self.editor.frame, count, visible);
        let shown = visible.min(count - first);
        self.timeline.strip = area;
        self.note_hint(
            area,
            "Drag to scrub. Shift selects frames, drag them to move. K holds a drawing",
        );
        let cells = Rect::new(area.x, area.y, shown as i32 * cell, area.h);
        let state = self.ui.interact(ID_STRIP, cells);
        let offset = ((self.ui.mouse.0 - area.x).max(0) / cell) as usize;
        let under = first + offset.min(shown - 1);
        let edge = (self.ui.mouse.0 >= area.right()) as i32 - (self.ui.mouse.0 < area.x) as i32;
        self.scrub(state, under, edge);
        for frame in first..first + shown {
            let x = area.x + (frame - first) as i32 * cell;
            let hover = state.hover && frame == under;
            let r = Rect::new(x, area.y, cell - 2 * s, area.h);
            if self.ui.naming() {
                self.ui.name_rect(r, &format!("Frame {}", frame + 1));
            }
            self.draw_cell(sf, r, frame, hover);
        }
        self.draw_hold_lines(sf, area, first, shown);
        self.draw_run_landing(sf, area, first);
    }

    fn draw_cell(&self, sf: &mut Surface<'_>, r: Rect, frame: usize, hover: bool) {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        let current = frame == self.editor.frame;
        let chosen = self
            .timeline
            .selected()
            .is_some_and(|(a, b)| (a..=b).contains(&frame));
        let fill = match (current, chosen, hover) {
            (true, _, true) => theme::accent_hover(),
            (true, _, false) => theme::accent(),
            (false, true, _) => theme::hover(),
            (false, false, true) => theme::raised(),
            (false, false, false) => theme::well(),
        };
        if current || chosen {
            sf.fill(r, fill);
        } else {
            // A frame nobody has picked is a well: a unit of Edge round its face.
            chrome::well(sf, r, s, theme::panel(), hover);
            if hover {
                sf.fill(r.inset(s), fill);
            }
        }
        let ink = if current {
            theme::on_accent()
        } else {
            theme::muted()
        };
        let label = (frame + 1).to_string();
        // Large text's doubled digits fit two to a cell; a longer number steps down a whole
        // whole multiple of the interface scale at a time, so it stays pixel exact and never
        // meets its neighbour's.
        let room = r.w - 2 * s;
        let ts = (1..=ts / s)
            .rev()
            .map(|k| k * s)
            .find(|&t| text_width(&label, t) <= room)
            .unwrap_or(s);
        let x = r.x + (r.w - text_width(&label, ts)) / 2;
        draw_text(sf, x, number_y(r, s, ts), &label, ink, ts);
        let layer = &self.editor.doc().layers()[self.editor.layer];
        if !layer.cel(frame).is_blank() {
            sf.fill(
                Rect::new(r.x + r.w / 2 - 5 * s, r.y + r.h - 6 * s, 10 * s, 2 * s),
                if current { ink } else { theme::cel() },
            );
        }
    }

    /// A line through the marks of neighbouring frames that show one drawing.
    fn draw_hold_lines(&self, sf: &mut Surface<'_>, area: Rect, first: usize, shown: usize) {
        let s = self.ui.scale;
        let cell = CELL * s;
        let y = area.y + area.h - 6 * s;
        for frame in first..first + shown.saturating_sub(1) {
            if self.editor.doc().held_into_next(self.editor.layer, frame) {
                let x = area.x + (frame - first) as i32 * cell + (cell - 2 * s) / 2;
                sf.fill(Rect::new(x, y, cell, 2 * s), theme::text());
            }
        }
    }

    pub(crate) fn draw_tag_row(&mut self, sf: &mut Surface<'_>, area: Rect) {
        let cell = CELL * self.ui.scale;
        let visible = (area.w / cell).max(1) as usize;
        let first =
            self.timeline
                .scrolled(self.editor.frame, self.editor.doc().frames.len(), visible);
        self.timeline.tag_row = area;
        let mut shown: Vec<(usize, Rect)> = (0..self.editor.doc().tags.len())
            .filter_map(|index| self.tag_rect(area, index, first, visible, cell))
            .collect();
        // The narrowest tag is on top and takes the click, so overlapped tags can all be
        // reached. A tag being named is wider than its frames and goes above everything.
        let naming = self.timeline.naming;
        sort_by_width(&mut shown, naming);
        let mut covered = false;
        let mut states: Vec<Interaction> = shown
            .iter()
            .map(|&(index, r)| {
                let mut state = self.ui.interact(ID_TAG + index as u32, r);
                state.hover &= !covered;
                covered |= state.hover;
                state
            })
            .collect();
        let under = self.frame_in_row(area, first, visible);
        for (&(index, r), state) in shown.iter().zip(&states) {
            if state.started && self.start_tag_edge(r, index, under, first + visible) {
                break;
            }
        }
        if self.carry_tag_edge(under) {
            states.iter_mut().for_each(|state| state.clicked = false);
        }
        for (&(index, r), &state) in shown.iter().zip(&states).rev() {
            self.draw_tag(sf, r, index, state);
        }
        self.draw_tag_edge(sf, area, first);
    }

    /// The frame under the pointer along the tag row, kept to the frames in view.
    fn frame_in_row(&self, area: Rect, first: usize, visible: usize) -> usize {
        let cell = CELL * self.ui.scale;
        let last = self.editor.doc().frames.len().min(first + visible) - 1;
        let offset = ((self.ui.mouse.0 - area.x).max(0) / cell) as usize;
        (first + offset).min(last)
    }

    /// Where a tag is drawn, if any of it is in view.
    fn tag_rect(
        &self,
        area: Rect,
        index: usize,
        first: usize,
        visible: usize,
        cell: i32,
    ) -> Option<(usize, Rect)> {
        let tag = &self.editor.doc().tags[index];
        if tag.to < first || tag.from >= first + visible {
            return None;
        }
        let s = self.ui.scale;
        let left = tag.from.max(first) - first;
        let right = tag.to.min(first + visible - 1) - first;
        let mut r = Rect::new(
            area.x + left as i32 * cell,
            area.y,
            (right - left + 1) as i32 * cell - 2 * s,
            area.h,
        );
        if self.timeline.naming == Some(index) {
            let wanted = text_width(&tag_label(tag, true), self.ui.text_scale()) + 8 * s;
            r.w = r.w.max(wanted).min(area.right() - r.x);
        }
        Some((index, r))
    }

    fn draw_tag(&mut self, sf: &mut Surface<'_>, r: Rect, index: usize, state: Interaction) {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        self.note_hint(
            r,
            "Click to loop, drag an end to give it other frames. T renames, Shift T turns, a \
             right click sets how many times it plays",
        );
        if self.ui.naming() {
            self.ui.name_rect(r, &format!("Tag {}", index + 1));
        }
        let active = self.timeline.tag == Some(index);
        let fill = match (active, state.hover) {
            (true, true) => theme::accent_hover(),
            (true, false) => theme::accent(),
            (false, true) => theme::hover(),
            (false, false) => theme::raised(),
        };
        sf.fill(r, fill);
        let ink = if active {
            theme::on_accent()
        } else {
            theme::text()
        };
        let at = (r.x + 4 * s, r.y + (r.h - cap_height() * ts) / 2);
        let tag = &self.editor.doc().tags[index];
        if self.timeline.naming == Some(index) {
            let arrow = arrow(tag);
            let room = (r.w - 8 * s - text_width(arrow, ts) - em() * ts).max(em() * ts);
            let edit = &self.timeline.name_edit;
            let used = draw_text_edit(sf, at, room, edit, (ink, theme::pressed()), ts);
            draw_text(sf, at.0 + used + em() * ts, at.1, arrow, ink, ts);
        } else {
            let label = tag_label(tag, false);
            draw_text(
                sf,
                at.0,
                at.1,
                &fit_to_width(&label, r.w - 8 * s, ts),
                ink,
                ts,
            );
        }
        if state.clicked {
            self.defer(move |a| {
                a.finish_naming();
                a.toggle_loop(index);
            });
        }
    }

    pub(crate) fn draw_duration(&mut self, sf: &mut Surface<'_>, area: Rect) {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        self.timeline.duration_area = area;
        let size = area.h;
        let readout = Rect::new(area.x, area.y, area.w - size * 2 - 4 * s, area.h);
        self.timeline.readout = readout;
        let state = self.drag_duration(readout);
        if state.hover {
            sf.fill(
                readout,
                if state.held {
                    theme::pressed()
                } else {
                    theme::raised()
                },
            );
        }
        let label = duration_label(&self.target_durations().1, self.editor.doc().frame_rate);
        let y = area.y + (area.h - cap_height() * ts) / 2;
        let inner = readout.w - 8 * s;
        draw_text(
            sf,
            area.x + 4 * s,
            y,
            &fit_to_width(&label, inner, ts),
            theme::text(),
            ts,
        );
        let minus = Rect::new(area.right() - size * 2 - 2 * s, area.y, size, size);
        let plus = Rect::new(area.right() - size, area.y, size, size);
        self.note_hint(minus, "Show this frame for less time");
        self.note_hint(plus, "Show this frame for longer");
        let parent = theme::panel();
        let less = ActionIcon::Minus.art();
        let more = ActionIcon::Plus.art();
        if self.ui.naming() {
            self.ui.name(ID_DURATION, "Shorter");
            self.ui.name(ID_DURATION + 1, "Longer");
        }
        if chrome::art_button(
            &mut self.ui,
            sf,
            (ID_DURATION, minus),
            less,
            parent,
            State::NONE,
        ) {
            self.defer(|a| {
                a.run(StepCmd::EndDrag);
                a.nudge_timing(-1);
                a.run(StepCmd::EndDrag);
            });
        }
        if chrome::art_button(
            &mut self.ui,
            sf,
            (ID_DURATION + 1, plus),
            more,
            parent,
            State::NONE,
        ) {
            self.defer(|a| {
                a.run(StepCmd::EndDrag);
                a.nudge_timing(1);
                a.run(StepCmd::EndDrag);
            });
        }
    }

    fn drag_duration(&mut self, readout: Rect) -> Interaction {
        let state = self.ui.interact(ID_READOUT, readout);
        self.note_hint(
            readout,
            "Drag or scroll to change timing, right click for frame rate",
        );
        if state.started {
            let (from, durations) = self.target_durations();
            let x = self.ui.mouse.0;
            self.defer(|a| {
                a.run(StepCmd::EndDrag);
            });
            self.timeline.timing_drag = Some(TimingDrag { x, from, durations });
        }
        if !state.held {
            return state;
        }
        let mouse = self.ui.mouse.0;
        self.defer(move |a| a.drag_timing_to(mouse));
        state
    }

    fn drag_timing_to(&mut self, mouse: i32) {
        if let Some(drag) = self.timeline.timing_drag.take() {
            let steps = (mouse - drag.x) / (DRAG_PER_STEP * self.ui.scale);
            self.retime_by(drag.from, &drag.durations, i64::from(steps));
            self.timeline.timing_drag = Some(drag);
        }
    }
}

/// Narrowest first, the tag being named before all. An insertion sort, because at most
/// [`MAX_TAGS`](pikslar_core::MAX_TAGS) tags are in view and a library sort costs 8 KB of binary.
fn sort_by_width(shown: &mut [(usize, Rect)], naming: Option<usize>) {
    let key = |&(index, r): &(usize, Rect)| (Some(index) != naming, r.w, index);
    for placed in 1..shown.len() {
        let mut at = placed;
        while at > 0 && key(&shown[at]) < key(&shown[at - 1]) {
            shown.swap(at, at - 1);
            at -= 1;
        }
    }
}

/// One timing, how many frames share it, and a mark if they do not all agree. At a frame rate
/// the timing is in ticks, "2f", as it is kept.
pub(crate) fn duration_label(durations: &[u32], fps: Option<u8>) -> String {
    let (units, suffix): (Vec<u32>, &str) = match fps {
        Some(fps) => (durations.iter().map(|&ms| ticks(ms, fps)).collect(), "f"),
        None => (durations.to_vec(), "ms"),
    };
    let first = units.first().copied().unwrap_or(0);
    let mixed = if units.iter().any(|&u| u != first) {
        "~"
    } else {
        ""
    };
    match units.len() {
        0 | 1 => format!("{first}{suffix}"),
        n => format!("{mixed}{first}{suffix} x{n}"),
    }
}
