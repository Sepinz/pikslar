//! The Canvas size dialog: a typed width and height, where the old canvas goes in the new one,
//! and a crop to the selection. Opened by clicking the size in the top bar. Like the shortcuts
//! dialog it covers everything while open, and the keyboard belongs to it. It also shows how the
//! document combines its layers, and changes that as one step.

use crate::app::App;
use pikslar_core::command::{anchor_shift, crop_to, CanvasCmd};
use pikslar_core::document::MAX_DIMENSION;
use pikslar_core::Blending;
use pikslar_platform::{Key, Mods};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, default_button, draw_text, MENU_LEVEL};

pub(crate) const ID_CANVAS: u32 = 50_000;
const WIDTH: i32 = 300;
const HEIGHT: i32 = 284;
const ROW: i32 = 28;
/// The most digits a side can have: 8192 is four.
const DIGITS: usize = 4;
/// What each anchor button says, left to right and top to bottom.
const ANCHORS: [&str; 9] = [
    "Keep at top left",
    "Keep at top",
    "Keep at top right",
    "Keep at left",
    "Keep in the middle",
    "Keep at right",
    "Keep at bottom left",
    "Keep at bottom",
    "Keep at bottom right",
];

/// What is being typed and where the old canvas goes, 0 to 2 across and down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanvasDialog {
    pub width: String,
    pub height: String,
    /// True while the height takes the digits typed.
    pub on_height: bool,
    pub anchor: (u8, u8),
}

impl CanvasDialog {
    /// The size typed, if each side is a number from 1 to the largest canvas.
    pub(crate) fn size(&self) -> Option<(usize, usize)> {
        let side = |s: &str| s.parse().ok().filter(|&n| (1..=MAX_DIMENSION).contains(&n));
        Some((side(&self.width)?, side(&self.height)?))
    }

    fn field(&mut self) -> &mut String {
        match self.on_height {
            true => &mut self.height,
            false => &mut self.width,
        }
    }

    /// Adds a digit to the side being typed. Anything else, or a fifth digit, is ignored.
    pub(crate) fn typed(&mut self, c: char) {
        let field = self.field();
        if c.is_ascii_digit() && field.len() < DIGITS {
            field.push(c);
        }
    }
}

/// The widgets by row, for the arrow keys. Crop to selection is there only with a selection.
fn canvas_grid(app: &App) -> Vec<&'static [u32]> {
    const FIELDS: &[u32] = &[ID_CANVAS, ID_CANVAS + 1];
    const TOP: &[u32] = &[ID_CANVAS + 2, ID_CANVAS + 3, ID_CANVAS + 4];
    const MIDDLE: &[u32] = &[ID_CANVAS + 5, ID_CANVAS + 6, ID_CANVAS + 7];
    const BOTTOM: &[u32] = &[ID_CANVAS + 8, ID_CANVAS + 9, ID_CANVAS + 10];
    const BLEND: &[u32] = &[ID_CANVAS + 14];
    const ACTIONS: &[u32] = &[ID_CANVAS + 11, ID_CANVAS + 12, ID_CANVAS + 13];
    const NO_CROP: &[u32] = &[ID_CANVAS + 11, ID_CANVAS + 13];
    let last = if app.editor.selection().is_some() {
        ACTIONS
    } else {
        NO_CROP
    };
    vec![FIELDS, TOP, MIDDLE, BOTTOM, BLEND, last]
}

impl App {
    pub(crate) fn open_canvas_dialog(&mut self) {
        self.close_menu();
        self.finish_naming();
        let doc = self.editor.doc();
        self.canvas_dialog = Some(CanvasDialog {
            width: doc.width.to_string(),
            height: doc.height.to_string(),
            on_height: false,
            anchor: (1, 1),
        });
        self.ui.focus.set(ID_CANVAS);
        self.ui_dirty = true;
    }

    pub(crate) fn close_canvas_dialog(&mut self) {
        self.ui.focus.clear();
        self.canvas_dialog = None;
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    /// Resizes the canvas as chosen and closes the dialog, or says why it cannot.
    pub(crate) fn apply_canvas_dialog(&mut self) {
        let Some(dialog) = self.canvas_dialog.clone() else {
            return;
        };
        let Some(size) = dialog.size() else {
            return self.notify(format!("Width and height go from 1 to {MAX_DIMENSION}"));
        };
        let doc = self.editor.doc();
        let (dx, dy) = anchor_shift((doc.width, doc.height), size, dialog.anchor);
        let command = CanvasCmd::Resize {
            width: size.0,
            height: size.1,
            dx,
            dy,
        };
        if self.run(command).is_some() {
            self.close_canvas_dialog();
        }
    }

    /// Switches the document between Pikslar's blending and Aseprite-compatible blending, as one
    /// step. The dialog stays open.
    pub(crate) fn toggle_blending(&mut self) {
        let blending = match self.editor.doc().blending {
            Blending::Pikslar => Blending::Aseprite,
            Blending::Aseprite => Blending::Pikslar,
        };
        if self.run(CanvasCmd::SetBlending { blending }).is_some() {
            self.full_canvas = true;
            self.notify(format!("Blending: {}", blending.name()));
        }
    }

    /// Crops the canvas to the selection's bounds and closes the dialog.
    pub(crate) fn crop_to_selection(&mut self) {
        let doc = self.editor.doc();
        let (cw, ch) = (doc.width as i32, doc.height as i32);
        let bounds = self
            .editor
            .selection()
            .map(|m| m.within(cw, ch).trimmed().bounds());
        let Some((x, y, w, h)) = bounds.filter(|&(_, _, w, h)| w > 0 && h > 0) else {
            return self.notify("Select something to crop to".to_string());
        };
        if self.run(crop_to(x, y, w as usize, h as usize)).is_some() {
            self.close_canvas_dialog();
        }
    }

    pub(crate) fn canvas_dialog_typed(&mut self, c: char) {
        if let Some(d) = self.canvas_dialog.as_mut() {
            d.typed(c);
            self.ui_dirty = true;
        }
    }

    /// Escape closes, Tab and the arrows move focus, Space acts on what has it, Enter acts on a
    /// button and otherwise resizes, Backspace takes a digit away and Delete all of them.
    pub(crate) fn canvas_dialog_key(&mut self, key: Key, mods: Mods) {
        if self.canvas_dialog.is_none() {
            return;
        }
        self.ui_dirty = true;
        let on_button = self.ui.focus.focused().is_some_and(|id| id > ID_CANVAS + 1);
        match key {
            Key::Escape => self.close_canvas_dialog(),
            Key::Enter if !on_button => self.apply_canvas_dialog(),
            Key::Backspace => {
                if let Some(d) = self.canvas_dialog.as_mut() {
                    d.field().pop();
                }
            }
            // The arrows move focus here, so there is no caret: Delete empties the side to type
            // a new number, as selecting it all and typing would.
            Key::Delete => {
                if let Some(d) = self.canvas_dialog.as_mut() {
                    d.field().clear();
                }
            }
            _ => {
                if self.focus_key(key, mods) || self.focus_grid_key(key, &canvas_grid(self)) {
                    self.follow_focus_to_field();
                }
            }
        }
    }

    /// The side being typed is the one that has focus.
    fn follow_focus_to_field(&mut self) {
        let focused = self.ui.focus.focused();
        if let Some(d) = self.canvas_dialog.as_mut() {
            match focused {
                Some(id) if id == ID_CANVAS => d.on_height = false,
                Some(id) if id == ID_CANVAS + 1 => d.on_height = true,
                _ => {}
            }
        }
    }

    pub(crate) fn canvas_dialog_rect(&self) -> Rect {
        let s = self.ui.scale;
        let (w, h) = (WIDTH * s, HEIGHT * s);
        let area = self.layout.canvas;
        let screen = (self.layout.bottom.w, self.layout.bottom.bottom());
        let x = (area.x + (area.w - w) / 2).clamp(0, (screen.0 - w).max(0));
        let y = (area.y + (area.h - h) / 2).clamp(0, (screen.1 - h).max(0));
        Rect::new(x, y, w, h)
    }

    pub(crate) fn draw_canvas_dialog(&mut self, sf: &mut Surface<'_>, dirty: &mut Vec<Rect>) {
        let Some(d) = self.canvas_dialog.clone() else {
            return;
        };
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let panel = self.canvas_dialog_rect();
        let back = self.ui.enter_level(MENU_LEVEL);
        pikslar_ui::chrome::popup(sf, panel, s);
        self.ui.focus.collect(true);
        let left = panel.x + 16 * s;
        draw_text(sf, left, panel.y + 14 * s, "Canvas size", theme::text(), ts);
        self.size_fields(sf, &d, left, panel.y + 36 * s);
        self.anchor_grid(sf, &d, left, panel.y + 36 * s + (ROW + 8) * s);
        self.blending_row(sf, left, panel.bottom() - 102 * s);
        let note = "Pixels outside are kept";
        draw_text(sf, left, panel.bottom() - 66 * s, note, theme::muted(), ts);
        self.canvas_actions(sf, left, panel.bottom() - 40 * s);
        self.ui.focus.collect(false);
        self.ui.enter_level(back);
        dirty.push(panel.inset(-s));
    }

    /// The two sides, each a button that takes the digits typed once clicked.
    fn size_fields(&mut self, sf: &mut Surface<'_>, d: &CanvasDialog, x: i32, y: i32) {
        let s = self.ui.scale;
        let sides = [("Width", &d.width, false), ("Height", &d.height, true)];
        for (n, (name, value, height)) in sides.into_iter().enumerate() {
            let r = Rect::new(x + n as i32 * 136 * s, y, 128 * s, ROW * s);
            let on = d.on_height == height;
            let caret = if on { "_" } else { "" };
            let label = format!("{name} {value}{caret}");
            if button(&mut self.ui, sf, ID_CANVAS + n as u32, r, &label, on) {
                if let Some(d) = self.canvas_dialog.as_mut() {
                    d.on_height = height;
                }
                self.ui_dirty = true;
            }
        }
    }

    /// Nine buttons, where the old canvas goes in the new one.
    fn anchor_grid(&mut self, sf: &mut Surface<'_>, d: &CanvasDialog, x: i32, y: i32) {
        let s = self.ui.scale;
        draw_text(
            sf,
            x,
            y + 6 * s,
            "Keep at",
            theme::muted(),
            self.ui.text_scale(),
        );
        for n in 0..9u8 {
            let at = (n % 3, n / 3);
            let r = Rect::new(
                x + (72 + i32::from(at.0) * 26) * s,
                y + i32::from(at.1) * 26 * s,
                22 * s,
                22 * s,
            );
            let id = ID_CANVAS + 2 + u32::from(n);
            let clicked = button(&mut self.ui, sf, id, r, "", d.anchor == at);
            self.ui.name(id, ANCHORS[usize::from(n)]);
            if clicked {
                if let Some(d) = self.canvas_dialog.as_mut() {
                    d.anchor = at;
                }
                self.ui_dirty = true;
            }
        }
    }

    /// How the layers combine, a button that switches it.
    fn blending_row(&mut self, sf: &mut Surface<'_>, x: i32, y: i32) {
        let s = self.ui.scale;
        let blending = self.editor.doc().blending;
        let r = Rect::new(x, y, 268 * s, ROW * s);
        self.note_hint_named(
            r,
            "Blending",
            "How layers combine; Aseprite-compatible draws imported Aseprite art as Aseprite does",
        );
        let label = format!("Blending: {}", blending.name());
        let on = blending == Blending::Aseprite;
        if button(&mut self.ui, sf, ID_CANVAS + 14, r, &label, on) {
            self.defer(|a| a.toggle_blending());
        }
    }

    fn canvas_actions(&mut self, sf: &mut Surface<'_>, x: i32, y: i32) {
        let s = self.ui.scale;
        let apply = Rect::new(x, y, 72 * s, ROW * s);
        let crop = Rect::new(apply.right() + 4 * s, y, 112 * s, ROW * s);
        let cancel = Rect::new(crop.right() + 4 * s, y, 72 * s, ROW * s);
        if default_button(&mut self.ui, sf, ID_CANVAS + 11, apply, "Resize") {
            self.defer(|a| a.apply_canvas_dialog());
        }
        if self.editor.selection().is_some()
            && button(
                &mut self.ui,
                sf,
                ID_CANVAS + 12,
                crop,
                "Crop to selection",
                false,
            )
        {
            self.defer(|a| a.crop_to_selection());
        }
        if button(&mut self.ui, sf, ID_CANVAS + 13, cancel, "Cancel", false) {
            self.close_canvas_dialog();
        }
    }
}

#[cfg(test)]
#[path = "canvas_dialog_tests.rs"]
mod tests;
