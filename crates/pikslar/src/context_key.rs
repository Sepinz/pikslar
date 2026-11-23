//! Shift F10 and the Menu key: a context menu at the active row, from the keyboard.
//!
//! On the frames it is the menu a right click on the frame opens. On the layers it is the New
//! tilemap layer menu (the one a right click on the New layer button opens), placed beside the
//! active row; it is not a menu of the row itself. Which row is the keyboard's: the layer after
//! the layer keys or a press in the layers panel, the frame otherwise.

use crate::app::App;
use crate::frame_menu::MenuKind;
use crate::panel_timeline::CELL;
use pikslar_platform::Key;
use pikslar_render::Rect;

/// Where the keyboard last was, and the rows the menus open at.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ContextKeys {
    /// Whether the layers, not the frames, have the keyboard.
    layers: bool,
    layers_area: Rect,
    layer_row: Rect,
    /// The menu drawn last, to tell a new one from the same one drawn again.
    menu_seen: Option<crate::frame_menu::OpenMenu>,
}

impl ContextKeys {
    /// The layers panel is being drawn again: the active row is whatever this pass draws.
    pub fn note_layers(&mut self, area: Rect) {
        self.layers_area = area;
        self.layer_row = Rect::default();
    }

    /// A layer key moved a layer, so the layers have the keyboard.
    pub fn layers_acted(&mut self) {
        self.layers = true;
    }

    pub fn note_active_row(&mut self, row: Rect) {
        self.layer_row = row;
    }
}

impl App {
    /// A press moves the keyboard to the layers or to the frames, when it is in either.
    pub(crate) fn note_context_press(&mut self, x: i32, y: i32) {
        if self.ctx.layers_area.contains(x, y) {
            self.ctx.layers = true;
        } else if self.layout.bottom.contains(x, y) {
            self.ctx.layers = false;
        }
    }

    /// The frame keys walk the frames. Up and Down take the keyboard to the layers only when
    /// they move a layer, see [`ContextKeys::layers_acted`].
    pub(crate) fn note_context_key(&mut self, key: Key) {
        match key {
            Key::Left | Key::Right | Key::Comma | Key::Period | Key::Home | Key::End => {
                self.ctx.layers = false;
            }
            _ => {}
        }
    }

    /// The active layer's row, or the New layer button when no row was drawn.
    pub(crate) fn layer_row_for_keys(&self) -> Rect {
        if self.ctx.layer_row.w > 0 {
            self.ctx.layer_row
        } else {
            self.tile_ui.new_layer
        }
    }

    /// Opens a context menu at the active row: on the layers the New tilemap layer menu beside
    /// the row, on the frames the frame menu. False when there is no row to open it at.
    pub(crate) fn context_key(&mut self) -> bool {
        if self.bar_context_key() {
            return true;
        }
        if self.ctx.layers {
            let row = self.layer_row_for_keys();
            if row.w > 0 {
                self.open_palette_kind((row.x + row.h, row.y), MenuKind::NewTilemap);
                return true;
            }
        }
        self.open_frame_menu_by_key()
    }

    /// The frame menu at the active frame, which is first scrolled into view. It never makes
    /// another frame current: the menu acts on the frame the keyboard is on.
    fn open_frame_menu_by_key(&mut self) -> bool {
        let strip = self.timeline.strip;
        let cell = CELL * self.ui.scale;
        if strip.w < cell {
            return false;
        }
        let (frame, count) = (self.editor.frame, self.editor.doc().frames.len());
        let visible = (strip.w / cell).max(1) as usize;
        self.timeline.show(frame, count, visible);
        let shown = frame.saturating_sub(self.timeline.first()) as i32;
        let at = (strip.x + shown * cell + 2, strip.y + 2);
        let chosen = self
            .selected_frames()
            .is_some_and(|(a, b)| (a..=b).contains(&frame));
        self.playing = false;
        if !chosen {
            if !self.settle() {
                return false;
            }
            self.timeline.press(frame, frame, false);
        }
        self.menu = Some(crate::frame_menu::OpenMenu {
            at,
            kind: MenuKind::Frame,
        });
        self.ui_dirty = true;
        true
    }
}

impl App {
    /// The View menu, as its shortcut opens it.
    pub(crate) fn open_view_menu_by_key(&mut self) {
        self.open_bar_menu(crate::menu_bar::VIEW, false);
    }

    /// The Scripts menu, as its shortcut opens it.
    pub(crate) fn open_scripts_menu_by_key(&mut self) {
        self.open_bar_menu(crate::menu_bar::SCRIPTS, false);
    }
}

impl ContextKeys {
    /// A menu that is not the one drawn last starts at its first row.
    pub fn menu_starts_at_top(
        &mut self,
        ui: &mut pikslar_ui::Ui,
        open: crate::frame_menu::OpenMenu,
    ) {
        if self.menu_seen != Some(open) {
            self.menu_seen = Some(open);
            ui.menu_first = 0;
        }
    }
}

impl ContextKeys {
    /// The menu is gone: the next one, even the same, starts at its first row.
    pub fn menu_closed(&mut self) {
        self.menu_seen = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_layers_pass_forgets_the_old_active_row() {
        let mut c = ContextKeys::default();
        c.note_active_row(Rect::new(1, 2, 30, 20));
        c.note_layers(Rect::new(0, 0, 100, 100));
        assert_eq!(c.layer_row, Rect::default(), "not drawn this pass, so none");
        assert_eq!(c.layers_area, Rect::new(0, 0, 100, 100));
    }
}
