//! What the timeline is pointing at, and the frame operations it offers.

use crate::app::{App, Gesture};
use crate::shortcuts::Action;
use pikslar_core::{
    tick_ms, ticks, Command, Direction, FloatingCmd, FrameCmd, Playhead, Span, StepCmd, TagCmd,
    MAX_TAG_NAME,
};
use pikslar_platform::{Key, Mods};
use pikslar_render::Rect;
use pikslar_ui::{edit_for, Edit, TextEdit};
use std::time::{Duration, Instant};

const MAX_ONION_DEPTH: usize = 3;
const NAME_WHEN_EMPTY: &str = "tag";
/// How often the frame changes while a scrub is held past the end of the strip.
const EDGE_STEP: Duration = Duration::from_millis(45);
pub(crate) const DURATION_STEP_MS: u32 = 10;
pub(crate) const ID_TAG: u32 = 210_000;
pub(crate) const TAG_IDS: std::ops::Range<u32> = ID_TAG..ID_TAG + pikslar_core::MAX_TAGS as u32;

/// Where a drag on the timing began, and the timings it started from.
pub struct TimingDrag {
    pub x: i32,
    pub from: usize,
    pub durations: Vec<u32>,
}

#[derive(Default)]
pub struct Timeline {
    /// The first frame shown in the strip.
    first: usize,
    followed: Option<usize>,
    /// Selected frames, as the frame the selection began on and the one it reached.
    range: Option<(usize, usize)>,
    selecting: bool,
    /// The tag being looped.
    pub tag: Option<usize>,
    /// The tag whose name is being typed.
    pub naming: Option<usize>,
    /// The tag being named was made for this, so abandoning the name removes it.
    naming_new: bool,
    /// The name being typed, with its caret.
    pub name_edit: TextEdit,
    /// Which way a ping-pong is playing, and the passes of a tag played so far.
    head: Playhead,
    /// The frame whose timing is being edited while playback shows the others.
    play_anchor: usize,
    /// The part of a nanosecond, in units of `1 / units_per_second`, owed by the waits so far.
    play_carry: u64,
    /// A drag on the timing in progress.
    pub timing_drag: Option<TimingDrag>,
    tags_seen: usize,
    /// A scrub is being held past the end of the strip, so frames keep going by.
    pub edge_scrub: bool,
    pub next_edge_step: Option<Instant>,
    /// A clock for tests to hold still or advance by hand, in place of the wall clock.
    #[cfg(test)]
    pub clock: Option<Instant>,
    pub strip: Rect,
    pub tag_row: Rect,
    pub duration_area: Rect,
    /// The part of the timing control that is dragged.
    pub readout: Rect,
    /// Selected frames being dragged along the strip.
    pub run_drag: Option<crate::frame_runs::RunDrag>,
    /// A tag's end being dragged.
    pub tag_edge: Option<crate::frame_runs::TagEdge>,
}

impl Timeline {
    /// The first frame to show, scrolling only when the current frame has moved out of view.
    pub fn scrolled(&mut self, frame: usize, count: usize, visible: usize) -> usize {
        if self.followed != Some(frame) {
            self.followed = Some(frame);
            if frame < self.first {
                self.first = frame;
            } else if frame >= self.first + visible {
                self.first = frame + 1 - visible;
            }
        }
        self.first = self.first.min(count.saturating_sub(visible));
        self.first
    }

    /// Scrolls the strip, if it must, so that `frame` is among the `visible` frames shown.
    pub fn show(&mut self, frame: usize, count: usize, visible: usize) {
        self.followed = None;
        self.scrolled(frame, count, visible);
    }

    fn scroll(&mut self, ticks: i32) {
        self.first = self.first.saturating_add_signed(-(ticks as isize));
    }

    /// The first frame the strip showed when it was last drawn.
    pub fn first(&self) -> usize {
        self.first
    }

    /// The selected frames, first to last. One frame is not a selection.
    pub fn selected(&self) -> Option<(usize, usize)> {
        let (a, b) = self.range?;
        (a != b).then_some((a.min(b), a.max(b)))
    }

    /// The selected frames that a strip of `frames` frames still has.
    pub fn selected_within(&self, frames: usize) -> Option<(usize, usize)> {
        let (a, b) = self.selected()?;
        let last = frames.checked_sub(1)?;
        let (a, b) = (a.min(last), b.min(last));
        (a != b).then_some((a, b))
    }

    pub fn press(&mut self, current: usize, at: usize, shift: bool) {
        self.selecting = shift;
        self.range = shift.then(|| (self.range.map_or(current, |r| r.0), at));
    }

    /// Selects the frames `from` to `to`.
    pub fn select(&mut self, from: usize, to: usize) {
        self.selecting = false;
        self.range = Some((from, to));
    }

    pub fn unselect(&mut self) {
        self.selecting = false;
        self.range = None;
    }

    pub fn drag_to(&mut self, at: usize) {
        if let Some((_, head)) = self.range.as_mut().filter(|_| self.selecting) {
            *head = at;
        }
    }

    /// How long until a held scrub past the strip's end moves one frame on.
    pub fn edge_wait(&self) -> Option<Duration> {
        self.edge_scrub.then(|| {
            self.next_edge_step.map_or(Duration::ZERO, |at| {
                at.saturating_duration_since(Instant::now())
            })
        })
    }

    /// The time scrubs are paced by: the wall clock, or a test's own.
    fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(at) = self.clock {
            return at;
        }
        Instant::now()
    }

    /// True if a held scrub past the strip's end may move on a frame now.
    pub fn edge_step_due(&mut self) -> bool {
        let now = self.now();
        let due = self.next_edge_step.is_none_or(|at| now >= at);
        if due {
            self.next_edge_step = Some(now + EDGE_STEP);
        }
        due
    }

    /// Keeps the selection to the `frames` there are, so the frames shown selected are the
    /// frames a frame action acts on. A change that removed selected frames clips it; one that
    /// left none of them drops it.
    pub fn fit_to(&mut self, frames: usize) {
        let Some((a, b)) = self.range else {
            return;
        };
        self.range = match frames.checked_sub(1) {
            Some(last) if a.min(b) <= last => Some((a.min(last), b.min(last))),
            _ => None,
        };
    }

    /// True for a selection none of whose frames are among the `frames` there are.
    pub fn selection_gone(&self, frames: usize) -> bool {
        self.selected().is_some_and(|(first, _)| first >= frames)
    }

    /// Forgets the selected frames and the loop. Returns true if there was anything to forget.
    fn clear(&mut self) -> bool {
        let had = self.range.is_some() || self.tag.is_some();
        self.range = None;
        self.tag = None;
        had
    }
}

impl App {
    /// The selected frames, clipped to the frames there are now: a command may have removed
    /// some since they were selected.
    pub(crate) fn selected_frames(&self) -> Option<(usize, usize)> {
        self.timeline
            .selected_within(self.editor.doc().frames.len())
    }

    fn active_tag(&self) -> Option<usize> {
        self.timeline
            .tag
            .filter(|&i| i < self.editor.doc().tags.len())
    }

    /// The frames being worked on as a loop, if a tag or several frames are chosen.
    pub(crate) fn looping_span(&self) -> Option<Span> {
        let frames = self.editor.doc().frames.len();
        if let Some(tag) = self.active_tag().map(|i| &self.editor.doc().tags[i]) {
            return Some(Span {
                from: tag.from.min(frames - 1),
                to: tag.to.min(frames - 1),
                direction: tag.direction,
            });
        }
        let (from, to) = self.selected_frames()?;
        Some(Span {
            from: from.min(frames - 1),
            to: to.min(frames - 1),
            direction: Direction::Forward,
        })
    }

    pub fn set_frame(&mut self, frame: usize) {
        if self.settle() {
            self.show_frame(frame);
        }
    }

    fn show_frame(&mut self, frame: usize) {
        let to = frame.min(self.editor.doc().frames.len() - 1);
        let from = std::mem::replace(&mut self.editor.frame, to);
        // The onion skin draws neighbouring frames too, which all move along with this one.
        // Showing the frame already shown redraws it whole, as it always has.
        if self.comp.onion || from == self.editor.frame {
            self.editor.damage_all();
        } else {
            self.editor.damage_frame_change(from);
        }
        self.ui_dirty = true;
    }

    pub fn step_frame(&mut self, forward: bool) {
        self.playing = false;
        let at = self.editor.frame;
        let count = self.editor.doc().frames.len();
        let span = self
            .looping_span()
            .filter(|s| s.contains(at))
            .unwrap_or(Span::whole(count));
        let length = span.to - span.from + 1;
        let delta = if forward { 1 } else { length - 1 };
        self.set_frame(span.from + (at - span.from + delta) % length);
    }

    /// Goes to the first or last frame, staying inside the loop as stepping does.
    pub fn end_frame(&mut self, last: bool) {
        self.playing = false;
        let at = self.editor.frame;
        let span = self
            .looping_span()
            .filter(|s| s.contains(at))
            .unwrap_or(Span::whole(self.editor.doc().frames.len()));
        let target = if last { span.to } else { span.from };
        if target != at {
            self.set_frame(target);
        }
    }

    pub fn toggle_play(&mut self) {
        if !self.playing && !self.settle() {
            return;
        }
        self.playing = !self.playing;
        self.timeline.play_carry = 0;
        self.next_frame_at = Instant::now() + self.next_wait();
        self.timeline.play_anchor = self.editor.frame;
        self.timeline.head = Playhead::default();
        self.ui_dirty = true;
    }

    /// Timing is kept from the schedule, not from when the app woke, so errors do not add up.
    pub(crate) fn advance_playback(&mut self) {
        let now = Instant::now();
        if !self.playing || now < self.next_frame_at {
            return;
        }
        let span = self
            .looping_span()
            .unwrap_or(Span::whole(self.editor.doc().frames.len()));
        let repeat = self
            .active_tag()
            .map_or(0, |i| self.editor.doc().tags[i].repeat);
        let Some(next) = span.step_counted(self.editor.frame, &mut self.timeline.head, repeat)
        else {
            // The tag has played as many times as it is set to.
            self.playing = false;
            self.ui_dirty = true;
            return;
        };
        // Looking around changes nothing in the document, so it can outlast a frame.
        if matches!(self.gesture, Gesture::Pan { .. }) {
            self.show_frame(next);
        } else {
            self.set_frame(next);
        }
        let wait = self.next_wait();
        self.next_frame_at += wait;
        if self.next_frame_at < now {
            self.next_frame_at = now + wait;
        }
    }

    /// How long the shown frame waits. The nanoseconds a frame's exact time leaves over are
    /// carried to the next wait, so frames of a tick that is no whole number of nanoseconds
    /// still add up to exact seconds.
    pub(crate) fn next_wait(&mut self) -> Duration {
        let doc = self.editor.doc();
        let per_second = u64::from(doc.units_per_second());
        let owed = u64::from(doc.frame_units(self.editor.frame)) * 1_000_000_000
            + self.timeline.play_carry;
        self.timeline.play_carry = owed % per_second;
        // Never under a millisecond, as before.
        Duration::from_nanos((owed / per_second).max(1_000_000))
    }

    /// Keeps the onion skin in step with the loop and with playback.
    pub(crate) fn sync_onion(&mut self) {
        // Undo can take tags away or bring them back. A remembered position would then mean
        // a different tag.
        let tags = self.editor.doc().tags.len();
        if tags != std::mem::replace(&mut self.timeline.tags_seen, tags) {
            self.timeline.tag = None;
        }
        self.timeline.fit_to(self.editor.doc().frames.len());
        let looping = self.looping_span();
        if self.comp.onion_loop != looping || self.comp.playing != self.playing {
            self.comp.onion_loop = looping;
            self.comp.playing = self.playing;
            if self.comp.onion {
                self.editor.damage_all();
            }
        }
    }

    pub fn cycle_onion_depth(&mut self) {
        self.comp.onion_depth = self.comp.onion_depth % MAX_ONION_DEPTH + 1;
        self.comp.onion = true;
        self.editor.damage_all();
        self.notify(format!("Onion skin: {} each way", self.comp.onion_depth));
    }

    /// Runs a frame command, forgetting the selected frames. True if it ran.
    pub(crate) fn frame_op(&mut self, command: FrameCmd) -> bool {
        let tags = self.editor.doc().tags.len();
        self.finish_gesture();
        self.playing = false;
        if self.run(command).is_none() {
            return false;
        }
        self.timeline.range = None;
        if self.editor.doc().tags.len() != tags {
            self.timeline.tag = None;
        }
        true
    }

    /// Adds a frame after this one; a copy duplicates the selected frames if there are any.
    pub fn new_frame(&mut self, copy: bool) {
        if let Some((from, to)) = self.selected_frames().filter(|_| copy) {
            self.duplicate_run(from, to);
            return;
        }
        let at = self.editor.frame + 1;
        self.frame_op(FrameCmd::Add { at, copy });
    }

    /// Removes the selected frames, or else the current one. A selection with none of its
    /// frames left is refused rather than taken for the current frame.
    pub fn remove_frames(&mut self) {
        if self.timeline.selection_gone(self.editor.doc().frames.len()) {
            self.timeline.unselect();
            self.notify("The selected frames are gone. Nothing was removed".to_string());
            return;
        }
        let at = self.editor.frame;
        let (from, to) = self.selected_frames().unwrap_or((at, at));
        self.frame_op(FrameCmd::Remove { from, to });
    }

    /// Moves this frame, or the selected frames, one place along.
    pub fn move_frame(&mut self, forward: bool) {
        if let Some((from, to)) = self.selected_frames() {
            let len = self.editor.doc().frames.len();
            let at = if forward {
                (to + 1 < len).then_some(from + 1)
            } else {
                from.checked_sub(1)
            };
            match at {
                Some(at) => self.move_run(from, to, at),
                None => self.notify("Those frames are already at the end".to_string()),
            }
            return;
        }
        let at = self.editor.frame;
        let Some(to) = (if forward {
            Some(at + 1).filter(|&t| t < self.editor.doc().frames.len())
        } else {
            at.checked_sub(1)
        }) else {
            self.notify("That frame is already at the end".to_string());
            return;
        };
        self.frame_op(FrameCmd::Move { from: at, to });
    }

    /// Joins this frame to the drawing before it, or gives it back a drawing of its own.
    /// With frames selected, does the same for all of them as one step.
    pub fn toggle_hold(&mut self) {
        let (index, at) = (self.editor.layer, self.editor.frame);
        let Some(layer) = self.editor.layer_id(index) else {
            return;
        };
        if let Some((from, to)) = self.selected_frames() {
            self.toggle_hold_range(index, from, to);
        } else if self.editor.doc().is_held(index, at) {
            self.frame_op(FrameCmd::Release {
                layer,
                from: at,
                to: at,
            });
        } else if at == 0 {
            self.notify("The first frame has nothing to hold".to_string());
        } else {
            self.frame_op(FrameCmd::HoldPrevious { layer, frame: at });
        }
    }

    fn toggle_hold_range(&mut self, index: usize, from: usize, to: usize) {
        let Some(layer) = self.editor.layer_id(index) else {
            return;
        };
        if (from..=to).all(|f| self.editor.doc().is_held(index, f)) {
            self.frame_op(FrameCmd::Release { layer, from, to });
        } else {
            self.frame_op(FrameCmd::HoldAcross {
                layer,
                source: from,
                from,
                to,
            });
        }
    }

    /// Shows this frame's drawing across the selected frames, or across the whole layer.
    pub fn hold_across(&mut self) {
        let at = self.editor.frame;
        let last = self.editor.doc().frames.len() - 1;
        let (from, to) = self.selected_frames().unwrap_or((0, last));
        if let Some(layer) = self.editor.layer_id(self.editor.layer) {
            self.frame_op(FrameCmd::HoldAcross {
                layer,
                source: at,
                from,
                to,
            });
        }
    }

    pub fn toggle_loop(&mut self, tag: usize) {
        self.timeline.tag = (self.timeline.tag != Some(tag)).then_some(tag);
        self.timeline.range = None;
        self.ui_dirty = true;
    }

    /// Tags the selected frames and starts naming the tag, or renames the one being looped.
    pub fn tag_key(&mut self) {
        let at = self.editor.frame;
        let renaming = self
            .active_tag()
            .filter(|_| self.selected_frames().is_none());
        if let Some(tag) = renaming {
            self.finish_gesture();
            self.playing = false;
            self.ui_dirty = true;
            self.run(FloatingCmd::Anchor);
            self.run(StepCmd::EndDrag);
            self.timeline.range = None;
            self.start_naming(tag, false);
            return;
        }
        let (from, to) = self.selected_frames().unwrap_or((at, at));
        self.finish_gesture();
        let added = self.run(TagCmd::Add {
            name: String::new(),
            from,
            to,
        });
        if let Some(tag) = added.and_then(|o| o.created) {
            let tag = tag as usize;
            self.timeline.range = None;
            self.timeline.tag = Some(tag);
            self.timeline.tags_seen = self.editor.doc().tags.len();
            self.start_naming(tag, true);
            self.playing = false;
        }
    }

    pub fn cycle_direction(&mut self) {
        let Some(tag) = self.active_tag() else {
            self.notify("Click a tag to choose its direction".to_string());
            return;
        };
        // The point of changing direction is to watch the difference, so playback carries on.
        self.finish_gesture();
        self.run(TagCmd::CycleDirection { tag });
    }

    fn name_of(&self, tag: usize) -> Option<String> {
        self.editor.doc().tags.get(tag).map(|t| t.name.clone())
    }

    /// A tag must never be left nameless.
    fn give_name_if_empty(&mut self, tag: usize) {
        if self.name_of(tag).is_some_and(|name| name.is_empty()) {
            self.run(TagCmd::SetName {
                tag,
                name: NAME_WHEN_EMPTY.to_string(),
            });
        }
    }

    /// Ends naming, keeping what was typed.
    pub(crate) fn finish_naming(&mut self) {
        let Some(tag) = self.timeline.naming.take() else {
            return;
        };
        self.give_name_if_empty(tag);
        self.run(StepCmd::EndDrag);
    }

    /// A press anywhere but on a tag ends naming. A tag's own click does it after its release.
    pub(crate) fn finish_naming_unless_on_tag(&mut self, x: i32, y: i32) {
        let id = self.ui.widget_at(x, y);
        if !(TAG_IDS.contains(&id)) {
            self.finish_naming();
        }
    }

    fn remove_named_tag(&mut self) {
        let named = self.timeline.naming.take();
        if let Some(tag) = named.filter(|&i| i < self.editor.doc().tags.len()) {
            let command: Command = if self.timeline.naming_new {
                StepCmd::AbortJoin.into()
            } else {
                TagCmd::Remove { tag }.into()
            };
            self.run(command);
            self.timeline.tag = None;
        }
    }

    pub(crate) fn cancel_naming(&mut self) {
        if self.timeline.naming.take().is_some() {
            self.run(StepCmd::AbortJoin);
            self.timeline.tag = self.active_tag();
        }
    }

    /// Starts typing a tag's name, with the caret after what it is called now.
    pub(crate) fn start_naming(&mut self, tag: usize, new: bool) {
        let name = self.name_of(tag).unwrap_or_default();
        let mut field = TextEdit::new(&name, MAX_TAG_NAME);
        field.apply(Edit::End { extend: false });
        self.timeline.name_edit = field;
        self.timeline.naming = Some(tag);
        self.timeline.naming_new = new;
    }

    /// A character typed: into the name being typed, if there is one.
    pub(crate) fn text_typed(&mut self, c: char) {
        self.sync_text();
        if self.key_text_spent.take().is_some() {
            return;
        }
        if !self.edit_naming(Edit::Insert(c)) && self.canvas_dialog.is_some() {
            self.canvas_dialog_typed(c);
        }
    }

    /// Edits the name being typed; every change joins the one step naming makes.
    pub(crate) fn edit_name(&mut self, edit: Edit) {
        let Some(tag) = self.timeline.naming else {
            return;
        };
        self.ui_dirty = true;
        let field = &mut self.timeline.name_edit;
        if field.apply(edit) {
            let name = field.text().to_string();
            self.run(TagCmd::SetName { tag, name });
        } else if matches!(edit, Edit::Insert(_)) && field.text().chars().count() >= MAX_TAG_NAME {
            self.notify(format!("Tag names stop at {MAX_TAG_NAME} letters"));
        }
    }

    /// While a tag is being named, the keyboard belongs to the name.
    pub(crate) fn naming_key(&mut self, key: Key, mods: Mods) {
        let edit = match key {
            Key::Enter => return self.finish_naming(),
            Key::Escape => return self.cancel_naming(),
            Key::Delete if mods.shift => return self.remove_named_tag(),
            // Characters come as text, from the keyboard layout.
            _ => match edit_for(key, mods) {
                Some(edit) => edit,
                None => return,
            },
        };
        self.edit_name(edit);
    }

    /// Letters that work on frames. Returns true if one was used.
    pub(crate) fn timeline_action(&mut self, action: Action) -> bool {
        match action {
            Action::NewFrame => self.new_frame(false),
            Action::DuplicateFrame => self.new_frame(true),
            Action::Hold => self.toggle_hold(),
            Action::HoldAcross => self.hold_across(),
            Action::Tag => self.tag_key(),
            Action::Direction => self.cycle_direction(),
            Action::OnionDepth => self.cycle_onion_depth(),
            Action::TruePreview => self.toggle_true_preview(),
            _ => return false,
        }
        true
    }

    /// Clears the selected frames and loop. Returns true if there were any.
    pub(crate) fn clear_timeline(&mut self) -> bool {
        self.ui_dirty = true;
        self.timeline.clear()
    }

    pub(crate) fn timeline_wheel(&mut self, notches: i32, x: i32, y: i32) {
        if self.timeline.naming.is_some() {
            return;
        }
        if self.timeline.strip.contains(x, y) {
            self.timeline.scroll(notches);
            self.ui_dirty = true;
        } else if self.timeline.duration_area.contains(x, y) {
            self.nudge_timing(i64::from(notches));
        }
    }

    /// The frames a timing change applies to. Playback must not make that a moving target.
    pub(crate) fn timing_target(&self) -> (usize, usize) {
        let last = self.editor.doc().frames.len() - 1;
        if let Some((from, to)) = self.selected_frames() {
            return (from.min(last), to.min(last));
        }
        let at = if self.playing {
            self.timeline.play_anchor.min(last)
        } else {
            self.editor.frame
        };
        (at, at)
    }

    pub(crate) fn target_durations(&self) -> (usize, Vec<u32>) {
        let (from, to) = self.timing_target();
        let frames = &self.editor.doc().frames[from..=to];
        (from, frames.iter().map(|f| f.duration_ms).collect())
    }

    /// Lengthens or shortens the target frames by `steps`, keeping the differences between them.
    pub(crate) fn nudge_timing(&mut self, steps: i64) {
        let (from, durations) = self.target_durations();
        self.finish_gesture();
        self.retime_by(from, &durations, steps);
    }

    /// `ms` moved by `steps`: a step is 10 ms, or one tick at a frame rate, and a frame keeps
    /// at least one tick.
    fn stepped_ms(&self, ms: u32, steps: i64) -> u32 {
        match self.editor.doc().frame_rate {
            Some(fps) => {
                let moved = (i64::from(ticks(ms, fps)) + steps).clamp(1, i64::from(u32::MAX));
                tick_ms(moved as u32, fps)
            }
            None => (i64::from(ms) + steps * i64::from(DURATION_STEP_MS)).max(0) as u32,
        }
    }

    pub(crate) fn retime_by(&mut self, from: usize, durations: &[u32], steps: i64) {
        let moved: Vec<u32> = durations
            .iter()
            .map(|&ms| self.stepped_ms(ms, steps))
            .collect();
        self.run(FrameCmd::Retime {
            from,
            durations: moved,
        });
    }
}
