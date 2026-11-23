//! The window event handler: input events in, frames and idle wake-ups out.

use super::{App, Gesture};
use pikslar_platform::{Event, Handler, Key, Window};
use pikslar_render::{Rect, Surface};
use std::time::{Duration, Instant};

impl Handler for App {
    fn event(&mut self, event: Event, window: Window) {
        // Windows queues a key's character straight after the key, ahead of pointer input, so
        // anything else between them means the character belonged to no key: one from the emoji
        // panel, say, or an input method. Posted keys can put the release of a key that types
        // ahead of its character, so that release keeps the character spent.
        let own_release = matches!(event, Event::KeyUp { key }
            if self.key_text_spent == Some(key) && types_text(key));
        if !own_release && !matches!(event, Event::KeyDown { .. } | Event::Text(_)) {
            self.key_text_spent = None;
        }
        match event {
            Event::Move { x, y } => {
                self.footprint.pointer_away = false;
                self.rest_moved(x, y, Instant::now());
                if self.gesture != Gesture::None {
                    self.ui.mouse = (x, y);
                    self.canvas_move(x, y);
                } else {
                    let hinted = self.slice_hint();
                    // Overlapping tags can change hands with no change of widget under the list.
                    let moved = self.ui.track(x, y) || self.timeline.tag_row.contains(x, y);
                    self.ui_dirty |= moved || hinted != self.slice_hint();
                }
            }
            Event::Down { x, y, button, mods } => self.pointer_down(x, y, button, mods),
            Event::Up { x, y, button } => self.pointer_up(x, y, button),
            Event::Wheel { x, y, ticks } => {
                self.rest_interrupted();
                if self.menu.is_some() {
                    // A menu taller than the window pages with the wheel.
                    self.ui.menu_scroll(ticks);
                    // The wheel has the view now, so a row the keyboard chose must not pull it back.
                    self.menu_cursor = None;
                    self.ui.menu_highlight = None;
                    self.ui_dirty = true;
                } else if self.panel_open() {
                    // What is under a dialog stays as it was.
                } else if self.layout.canvas.contains(x, y) {
                    self.zoom(ticks, x, y);
                } else {
                    self.panel_wheel(ticks, x, y);
                }
            }
            Event::KeyDown { key, mods, repeat } => self.key_down(key, mods, repeat, window),
            Event::KeyUp { key: Key::Space } => {
                self.space_held = false;
                self.key_cursor_release();
            }
            Event::KeyUp { .. } => {}
            Event::Text(c) => self.text_typed(c),
            Event::Resized { w, h, scale } => self.resized(w, h, scale),
            Event::Leave => {
                self.footprint.pointer_away = true;
                self.rest_interrupted();
            }
            Event::CaptureLost => self.capture_lost(),
            Event::Dropped(paths) => self.files_dropped(&paths, window),
            Event::LookChanged => self.follow_look(),
            Event::CloseRequested => {
                if self.confirm_discard(window) {
                    self.finish_rewind_closing();
                    self.recovery.set_keep(false);
                    window.quit();
                }
            }
        }
    }

    fn frame(&mut self, surface: &mut Surface<'_>, window: Window, dirty: &mut Vec<Rect>) {
        // The question is asked over a painted window, not a blank one.
        if self.presented && std::mem::take(&mut self.pending_offer) {
            self.offer_recovery(window);
        }
        self.report_save_failure(window);
        self.collect_opened(window);
        self.collect_save(window);
        // Asked for on the last frame, which already says it is running.
        self.run_started_script(window);
        self.advance_playback();
        self.ui_dirty |= self.timeline.edge_scrub;
        self.sync_onion();
        self.expire_message();
        self.recheck_save_blocked();
        self.tick_recovery();
        self.keep_standing();
        self.tick_settings();
        self.tick_pending();
        self.settle_rewind(false);
        self.refresh_lint();
        self.sync_selection();
        self.sync_scale_outline();
        self.tick_tooltip(Instant::now());
        self.sync_matrix_layout();
        if std::mem::take(&mut self.ui_dirty) {
            self.draw_interface(surface, window, dirty);
            if self.ui_dirty {
                self.draw_interface(surface, window, dirty);
                self.ui_dirty = false;
            }
        }
        self.draw_canvas(surface, dirty);
        // Whatever floats over the canvas is drawn again over what the canvas just drew.
        if (self.panel_open() || self.rest.shown) && !dirty.is_empty() {
            self.draw_interface(surface, window, dirty);
        }
        // What the drawing just changed (a splitter's key, a slot menu) is written now, not
        // when some later event happens to bring another frame.
        self.tick_settings();
        self.update_cursor(window);
        self.update_title(window);
        self.update_title_bar(window);
        self.presented = true;
        self.save_failure_drawn();
    }

    fn wake_after(&self) -> Option<Duration> {
        let now = Instant::now();
        let play = self
            .playing
            .then(|| self.next_frame_at.saturating_duration_since(now));
        let message = self
            .message
            .as_ref()
            .filter(|_| !self.message_is_error)
            .map(|(_, until)| until.saturating_duration_since(now));
        let offer = self.pending_offer.then_some(Duration::ZERO);
        let script = (self.scripts.state == crate::script_panel::RunState::Starting)
            .then_some(Duration::ZERO);
        let scrub = self.timeline.edge_wait();
        [
            play,
            message,
            offer,
            script,
            scrub,
            self.ants_wait(),
            self.tooltip_wait(),
            self.recovery_wait(),
            self.pending_wait(),
            self.report_wait(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn session_ending(&mut self, _window: Window) -> bool {
        self.end_session()
    }

    fn session_ended(&mut self, _window: Window, ending: bool) {
        self.session_over(ending);
    }
}

/// Whether Windows sends a character for the key, on some layout.
fn types_text(key: Key) -> bool {
    match key {
        Key::Char(_)
        | Key::Space
        | Key::BracketLeft
        | Key::BracketRight
        | Key::Comma
        | Key::Period
        | Key::Plus
        | Key::Minus
        | Key::Tab => true,
        Key::F(_)
        | Key::Menu
        | Key::AltTap
        | Key::Escape
        | Key::Enter
        | Key::Left
        | Key::Right
        | Key::Up
        | Key::Down
        | Key::Home
        | Key::End
        | Key::Delete
        | Key::Backspace => false,
    }
}
