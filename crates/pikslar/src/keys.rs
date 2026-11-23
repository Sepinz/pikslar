//! Keyboard shortcuts.

use crate::app::{App, Gesture, ToolChoice};
use crate::modal::Modal;
use crate::shortcuts::{Action, Binding};
use pikslar_core::{Command, FloatingCmd, SelectionCmd, Tool, Turn};
use pikslar_platform::{Key, Mods, Window};

const BIG_NUDGE: i32 = 10;

/// A held drag belongs to the hand making it. Only keys that cannot end it still work.
fn allowed_while_held(key: Key) -> bool {
    matches!(
        key,
        Key::Escape | Key::Space | Key::BracketLeft | Key::BracketRight
    )
}

impl App {
    fn set_tool_for(&mut self, action: Action) -> bool {
        let tool = match action {
            Action::Pencil => ToolChoice::Paint(Tool::Pencil),
            Action::Eraser => ToolChoice::Paint(Tool::Eraser),
            Action::Fill => ToolChoice::Paint(Tool::Fill),
            Action::Picker => ToolChoice::Paint(Tool::Picker),
            Action::Line => ToolChoice::Paint(Tool::Line),
            Action::Rect => ToolChoice::Paint(Tool::Rect),
            Action::Marquee => ToolChoice::Marquee,
            Action::Lasso => ToolChoice::Lasso,
            Action::Wand => ToolChoice::Wand,
            Action::Move => ToolChoice::Move,
            Action::Text => ToolChoice::Text,
            _ => return false,
        };
        self.set_tool(tool);
        true
    }

    fn file_action(&mut self, action: Action, window: Window) -> bool {
        match action {
            Action::Undo => self.undo(),
            Action::Redo | Action::RedoOther => self.redo(),
            Action::Save | Action::SaveAs => self.request_save(window, action == Action::SaveAs),
            Action::Open => self.open(window),
            Action::PlaceReference => self.place_reference(window),
            Action::Group => self.layer_action(6),
            Action::New => self.open_new_dialog(),
            Action::Export => self.open_export_dialog(false),
            Action::ExportSheet => self.open_export_dialog(true),
            Action::SelectAll => self.select_change(SelectionCmd::All),
            Action::SelectNone => self.select_change(SelectionCmd::None),
            Action::Invert => self.select_change(SelectionCmd::Invert),
            Action::Copy => {
                self.copy(window);
            }
            Action::Cut => self.cut(window),
            Action::Paste => self.paste(window),
            _ => return false,
        }
        true
    }

    pub fn select_change(&mut self, command: impl Into<Command>) {
        self.finish_gesture();
        self.run(command);
    }

    /// Actions on the current selection, or on the whole cel when there is none.
    fn transform_action(&mut self, action: Action) -> bool {
        let (change, whole) = match action {
            Action::FlipHorizontal => (FloatingCmd::FlipHorizontal, Turn::FlipHorizontal),
            Action::FlipVertical => (FloatingCmd::FlipVertical, Turn::FlipVertical),
            Action::RotateClockwise => (FloatingCmd::Rotate { clockwise: true }, Turn::Clockwise),
            Action::RotateCounter => (
                FloatingCmd::Rotate { clockwise: false },
                Turn::CounterClockwise,
            ),
            _ => return false,
        };
        if self.turn_tile_key(whole) || self.turn_brush_key(whole) {
            return true;
        }
        if self.has_selection() {
            self.select_change(change);
        } else {
            self.turn_cel(whole);
        }
        true
    }

    /// Does what `action` does, as its shortcut would.
    pub(crate) fn run_action(&mut self, action: Action, window: Window) {
        // A shortcut that opens a dialog ends the text session first, as choosing another tool
        // does, or the session would keep every key from the dialog.
        if matches!(
            action,
            Action::New
                | Action::Export
                | Action::ExportSheet
                | Action::CanvasSize
                | Action::ViewMenu
                | Action::ScriptsMenu
                | Action::KeysDialog
        ) {
            self.commit_text();
        }
        let done = self.set_tool_for(action)
            || self.file_action(action, window)
            || self.transform_action(action)
            || self.timeline_action(action)
            || self.workspace_action(action)
            || self.context_bar_action(action)
            || self.timeline_form_action(action)
            || self.window_action(action);
        if !done {
            match action {
                Action::Onion => self.toggle_onion(),
                Action::FitView => self.fit_view(),
                Action::CaptureBrush => self.capture_brush_key(),
                Action::CanvasSize => self.open_canvas_dialog(),
                Action::KeyCursor => self.toggle_key_cursor(),
                Action::ViewMenu => self.open_view_menu_by_key(),
                Action::ScriptsMenu => self.open_scripts_menu_by_key(),
                Action::KeysDialog => self.open_keys_dialog(),
                Action::TileMode => self.toggle_tile_mode(),
                Action::TextFont | Action::TextScale | Action::TextSpacing | Action::TextAlign => {
                    self.text_style_action(action, false);
                }
                _ => {}
            }
        }
    }

    /// A held key does its action once.
    fn letter_key(&mut self, letter: u8, mods: Mods, repeat: bool, window: Window) {
        let pressed = Binding {
            letter,
            ctrl: mods.ctrl,
            shift: mods.shift,
        };
        if let Some(action) = self.keymap.action_for(pressed).filter(|_| !repeat) {
            self.run_action(action, window);
        }
    }

    /// Arrows move picked-up pixels. Otherwise they walk the timeline and the layers,
    /// and Shift carries the current frame or layer along.
    fn direction_key(&mut self, key: Key, mods: Mods, repeat: bool) {
        // With the Move tool, arrows move the selection, or the whole cel when there is none.
        let nudging = self.editor.is_floating() || self.tool == ToolChoice::Move;
        let shift = mods.shift;
        let reorder = shift && !mods.ctrl;
        // Holding the key must not carry a frame or layer to the end, one undo step at a time.
        if reorder && repeat && !nudging {
            return;
        }
        match key {
            Key::Left if nudging => self.arrow((-1, 0), mods, repeat),
            Key::Right if nudging => self.arrow((1, 0), mods, repeat),
            Key::Up if nudging => self.arrow((0, -1), mods, repeat),
            Key::Down if nudging => self.arrow((0, 1), mods, repeat),
            Key::Left | Key::Comma if reorder => self.move_frame(false),
            Key::Right | Key::Period if reorder => self.move_frame(true),
            Key::Left | Key::Comma => self.step_frame(false),
            Key::Right | Key::Period => self.step_frame(true),
            Key::Up | Key::Down if reorder => {
                self.ctx.layers_acted();
                self.layer_action(if key == Key::Up { 2 } else { 3 });
            }
            Key::Up => self.step_layer(1),
            Key::Down => self.step_layer(-1),
            Key::Home => self.end_frame(false),
            Key::End => self.end_frame(true),
            _ => {}
        }
    }

    fn delete_key(&mut self, shift: bool, repeat: bool) {
        if shift {
            if !repeat {
                self.remove_frames();
            }
        } else if self.has_selection() {
            self.delete_selected();
        } else if !repeat {
            self.notify("Nothing is selected. Shift Delete removes the frame".to_string());
        }
    }

    /// Makes the row `delta` above the active one active, skipping what folded groups hold.
    fn step_layer(&mut self, delta: i32) {
        let listed = self.editor.doc().listed_layers();
        let row = self.listed_row(&listed, self.editor.layer) as i32;
        let last = listed.len() as i32 - 1;
        let layer = listed[(row - delta).clamp(0, last) as usize];
        if layer != self.editor.layer && self.settle() {
            self.editor.layer = layer;
            self.ctx.layers_acted();
        }
    }

    fn escape(&mut self) {
        if self.gesture != crate::app::Gesture::None {
            self.cancel_gesture();
        } else if self.loading.is_some() {
            self.stop_opening();
        } else if self.playing {
            self.playing = false;
        } else if self.editor.is_floating() {
            self.run(FloatingCmd::Revert);
        } else if self.has_selection() {
            self.deselect();
        } else {
            self.clear_timeline();
        }
        self.ui_dirty = true;
    }

    pub(crate) fn arrow(&mut self, (dx, dy): (i32, i32), mods: Mods, repeat: bool) {
        if matches!(self.gesture, Gesture::Scale { .. }) {
            self.finish_gesture();
        }
        let distance = if mods.shift { BIG_NUDGE } else { 1 };
        let (dx, dy) = (dx * distance, dy * distance);
        if self.editor.is_floating() || self.has_selection() {
            self.nudge(dx, dy, repeat);
        } else {
            self.nudge_cel((dx, dy), mods.alt != self.bar.only_cel, repeat);
        }
    }

    /// A dialog, panel or menu has the keyboard (`modal.rs`).
    fn modal_key(&mut self, modal: Modal, key: Key, mods: Mods, repeat: bool, window: Window) {
        match modal {
            Modal::KeysDialog => {
                if !repeat {
                    self.keys_dialog_key(key, mods);
                }
            }
            Modal::CanvasDialog => self.canvas_dialog_key(key, mods),
            Modal::PluginReview => {
                if !self.plugin_review_focus_key(key, mods) {
                    self.plugin_review_key(key);
                }
            }
            Modal::Scripts => self.script_panel_key(key),
            Modal::Menu | Modal::Export | Modal::New => self.dialog_key(key, mods, window),
        }
    }

    /// While the New dialog is open, the keyboard belongs to it.
    fn dialog_key(&mut self, key: Key, mods: Mods, window: Window) {
        if self.menu.is_some() {
            if !self.bar_menu_key(key, mods) {
                self.menu_key(key, window);
            }
            return;
        }
        if let Some(choice) = self.export_dialog {
            self.export_key(choice, key, mods, window);
            return;
        }
        let Some(dialog) = self.new_dialog else {
            return;
        };
        if self.focus_key(key, mods) || self.focus_grid_key(key, &crate::panel_side::NEW_GRID) {
            self.ui_dirty = true;
            return;
        }
        match key {
            Key::Enter if self.create_new(window, dialog) => self.close_new_dialog(),
            Key::Escape => self.close_new_dialog(),
            _ => {}
        }
        self.ui_dirty = true;
    }

    pub(crate) fn pointer_busy(&self) -> bool {
        self.ui.active != 0 || matches!(self.gesture, Gesture::Stroke | Gesture::Pan { .. })
    }

    fn zoom_at_center(&mut self, steps: i32) {
        let c = self.layout.canvas;
        self.zoom(steps, c.x + c.w / 2, c.y + c.h / 2);
    }

    /// Sends the key to the name being typed, if one is. Returns whether one was.
    fn naming_route(&mut self, key: Key, mods: Mods, window: Window) -> bool {
        if self.naming_clipboard_key(key, mods, window) {
            return true;
        }
        if self.text.is_some() {
            return self.text_key(key, mods);
        }
        if self.timeline.naming.is_some() {
            self.naming_key(key, mods);
        } else if self.layer_naming.is_some() {
            self.layer_naming_key(key, mods);
        } else if self.slice_naming.is_some() {
            self.slice_naming_key(key, mods);
        } else if self.palette_ui.naming.is_some() {
            self.group_naming_key(key, mods);
        } else if self.tile_ui.naming.is_some() {
            self.tileset_naming_key(key, mods);
        } else if self.brush_ui.naming.is_some() {
            self.brush_naming_key(key, mods);
        } else {
            return false;
        }
        true
    }

    pub fn key_down(&mut self, key: Key, mods: Mods, repeat: bool, window: Window) {
        if !repeat {
            self.dismiss_error();
        }
        self.sync_text();
        self.rest_interrupted();
        // A held T that started naming repeats as T, not as text in the new name. While a
        // field takes text, no key is a shortcut, so its character is typed.
        if !repeat {
            self.shortcut_held = self.timeline.naming.is_none()
                && self.text.is_none()
                && self.layer_naming.is_none()
                && self.slice_naming.is_none()
                && self.palette_ui.naming.is_none()
                && self.tile_ui.naming.is_none()
                && self.brush_ui.naming.is_none()
                && self.canvas_dialog.is_none();
        }
        // With Ctrl the key types no character (AltGr's never reach here), so none is spent,
        // and a character typed next is kept.
        self.key_text_spent = (self.shortcut_held && !mods.ctrl).then_some(key);
        // Before any route that could keep it: Save says why it cannot save (save_keys.rs).
        if self.save_key_held_back(key, mods, repeat) {
            return;
        }
        if self.naming_route(key, mods, window) {
            return;
        }
        if let Some(modal) = self.modal() {
            self.modal_key(modal, key, mods, repeat, window);
            return;
        }
        if self.key_cursor_key(key, mods, repeat) {
            return;
        }
        if self.pointer_busy() && !allowed_while_held(key) {
            return;
        }
        if self.dock_key(key, mods) || self.slider_key(key, mods) {
            return;
        }
        self.note_context_key(key);
        match key {
            Key::Char(c) if mods.alt && !mods.ctrl => {
                self.bar_access(c);
            }
            Key::Char(c) => self.letter_key(c, mods, repeat, window),
            Key::Space => self.space_held = true,
            Key::Enter if repeat => {}
            Key::Enter if self.editor.is_floating() => self.select_change(FloatingCmd::Anchor),
            Key::Enter if self.tool == ToolChoice::Text => self.start_text_by_key(),
            Key::Enter => self.toggle_play(),
            Key::Menu => {
                self.context_key();
            }
            Key::F(10) if mods.shift => {
                self.context_key();
            }
            Key::F(n) if !repeat => self.function_key(n, mods, window),
            Key::AltTap => self.menu_bar_toggle(),
            Key::Escape => self.escape(),
            Key::Delete | Key::Backspace => self.delete_key(mods.shift, repeat),
            Key::BracketLeft => self.adjust_brush(-1),
            Key::BracketRight => self.adjust_brush(1),
            Key::Plus => self.zoom_at_center(1),
            Key::Minus => self.zoom_at_center(-1),
            _ => self.direction_key(key, mods, repeat),
        }
    }
}
