//! Renaming a library brush: "Rename in the library..." lists the library, and choosing a
//! brush opens its name for typing in a small box where the menu was, beside its picture.
//! Enter keeps it, Escape or an empty name leaves it as it was, and a press anywhere else keeps
//! it, as with a layer's name. The name follows the rule every name does (`pikslar_core::names`)
//! and a name another brush has is numbered, as saving does. The library is app data, not the
//! document, so like saving and removing a brush this is no step of undo and leaves the file
//! unchanged; the brush in hand, a copy, keeps the name it had.

use crate::app::App;
use crate::brush_store::MAX_BRUSH_NAME;
use crate::brush_thumbs::{paint, SIDE};
use crate::brush_ui::PAGE;
use crate::frame_menu::MenuKind;
use pikslar_platform::{Key, Mods};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{cap_height, draw_text_edit, edit_for, Edit, TextEdit, MENU_PAD};

/// The box's width and height, in interface units.
const WIDTH: i32 = 240;
const HEIGHT: i32 = 24;

/// The library brush whose name is being typed, the name so far, and where the box is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrushNaming {
    pub index: usize,
    pub edit: TextEdit,
    pub at: (i32, i32),
}

impl App {
    /// A row of the rename menu: the last row of a full page goes on to the next page.
    pub(crate) fn rename_action(&mut self, page: usize, n: usize, at: (i32, i32)) {
        if n == PAGE {
            return self.open_brush_kind(at, MenuKind::BrushRename(page + 1));
        }
        self.start_brush_naming(page * PAGE + n, at);
    }

    pub(crate) fn start_brush_naming(&mut self, index: usize, at: (i32, i32)) {
        let Some(brush) = self.brush_ui.store.brushes().get(index) else {
            return;
        };
        let edit = TextEdit::new(&brush.name, MAX_BRUSH_NAME);
        self.finish_naming();
        self.finish_layer_naming();
        self.finish_slice_naming();
        self.finish_group_naming();
        self.finish_tileset_naming();
        self.brush_ui.naming = Some(BrushNaming { index, edit, at });
        self.ui_dirty = true;
    }

    /// Keeps the name typed and writes the library, unless it is empty or the same.
    pub(crate) fn finish_brush_naming(&mut self) {
        let Some(naming) = self.brush_ui.naming.take() else {
            return;
        };
        self.ui_dirty = true;
        self.full_canvas = true;
        match self.brush_ui.store.rename(naming.index, naming.edit.text()) {
            Ok(Some(name)) => self.notify(format!("Renamed the brush {name}")),
            Ok(None) => {}
            Err(why) => self.notify_error(why),
        }
    }

    /// While a brush is being named, the keyboard belongs to the name.
    pub(crate) fn brush_naming_key(&mut self, key: Key, mods: Mods) {
        let edit = match key {
            Key::Enter => return self.finish_brush_naming(),
            Key::Escape => {
                self.brush_ui.naming = None;
                self.ui_dirty = true;
                self.full_canvas = true;
                return;
            }
            // Characters come as text, from the keyboard layout.
            _ => match edit_for(key, mods) {
                Some(edit) => edit,
                None => return,
            },
        };
        self.edit_brush_name(edit);
    }

    pub(crate) fn edit_brush_name(&mut self, edit: Edit) {
        let Some(naming) = &mut self.brush_ui.naming else {
            return;
        };
        self.ui_dirty = true;
        let full = naming.edit.text().chars().count() >= MAX_BRUSH_NAME;
        if !naming.edit.apply(edit) && matches!(edit, Edit::Insert(_)) && full {
            self.notify(format!("Brush names stop at {MAX_BRUSH_NAME} letters"));
        }
    }

    /// Where the box is drawn, kept inside the window, while a brush is being named.
    pub(crate) fn brush_naming_rect(&self) -> Option<Rect> {
        let naming = self.brush_ui.naming.as_ref()?;
        let s = self.ui.scale;
        let screen = Rect::new(0, 0, self.layout.bottom.w, self.layout.bottom.bottom());
        let (w, h) = ((WIDTH * s).min(screen.w), (HEIGHT * s).min(screen.h));
        let x = naming.at.0.clamp(screen.x, screen.right() - w);
        let y = naming.at.1.clamp(screen.y, screen.bottom() - h);
        Some(Rect::new(x, y, w, h))
    }

    /// Draws the box on top of everything: the brush's picture, then the name being typed.
    pub(crate) fn draw_brush_naming(&mut self, sf: &mut Surface<'_>, dirty: &mut Vec<Rect>) {
        let Some(rect) = self.brush_naming_rect() else {
            return;
        };
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        pikslar_ui::chrome::popup(sf, rect, s);
        self.ui.name_rect(rect, "Brush name");
        self.brush_ui.thumbs.update(&self.brush_ui.store);
        let side = SIDE as i32 * s;
        let picture = (rect.x + MENU_PAD * s / 2, rect.y + (rect.h - side) / 2);
        let Some(naming) = &self.brush_ui.naming else {
            return;
        };
        if let Some(cells) = self.brush_ui.thumbs.of(naming.index) {
            paint(sf, cells, picture.0, picture.1, s);
        }
        let text_x = picture.0 + side + MENU_PAD * s / 2;
        let room = rect.right() - text_x - MENU_PAD * s / 2;
        let at = (text_x, rect.y + (rect.h - cap_height() * ts) / 2);
        draw_text_edit(
            sf,
            at,
            room,
            &naming.edit,
            (theme::text(), theme::accent()),
            ts,
        );
        dirty.push(rect.inset(-s));
    }
}
