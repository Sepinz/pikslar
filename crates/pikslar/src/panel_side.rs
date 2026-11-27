//! The top bar, the tool strip and the new-sprite panel.

use crate::app::{App, NewDialog, ToolChoice};
use crate::shortcuts::Action;
use pikslar_core::FloatingCmd;
use pikslar_core::{ColorMode, Tool};
use pikslar_platform::Window;
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, cap_height, default_button, draw_text, icon_button, text_width, Icon};
use std::borrow::Cow;

type ToolEntry = (ToolChoice, Icon, &'static str, Action, &'static str);

const PAINT_TOOLS: [ToolEntry; 7] = [
    (
        ToolChoice::Paint(Tool::Pencil),
        Icon::Pencil,
        "Pencil",
        Action::Pencil,
        "",
    ),
    (
        ToolChoice::Paint(Tool::Eraser),
        Icon::Eraser,
        "Eraser",
        Action::Eraser,
        "",
    ),
    (
        ToolChoice::Paint(Tool::Fill),
        Icon::Fill,
        "Fill",
        Action::Fill,
        "",
    ),
    (
        ToolChoice::Paint(Tool::Picker),
        Icon::Picker,
        "Color picker",
        Action::Picker,
        "",
    ),
    (
        ToolChoice::Paint(Tool::Line),
        Icon::Line,
        "Line",
        Action::Line,
        "",
    ),
    (
        ToolChoice::Paint(Tool::Rect),
        Icon::Rect,
        "Rectangle",
        Action::Rect,
        "",
    ),
    (
        ToolChoice::Text,
        Icon::Text,
        "Text",
        Action::Text,
        "Enter keeps, Esc cancels",
    ),
];
const SELECT_TOOLS: [ToolEntry; 4] = [
    (
        ToolChoice::Marquee,
        Icon::Marquee,
        "Select box",
        Action::Marquee,
        "Shift adds, Alt removes",
    ),
    (
        ToolChoice::Lasso,
        Icon::Lasso,
        "Lasso",
        Action::Lasso,
        "Shift adds, Alt removes",
    ),
    (
        ToolChoice::Wand,
        Icon::Wand,
        "Magic wand",
        Action::Wand,
        "Ctrl picks all matching",
    ),
    (
        ToolChoice::Move,
        Icon::Move,
        "Move and scale",
        Action::Move,
        "Alt moves only this cel",
    ),
];
/// A new sprite is taught two things, and only for its first few edits.
const FIRST_USE_EDITS: usize = 20;
const FIRST_HINT: &str = "Hover anything to see what it does";
const NEXT_HINT: &str = "N adds a frame, Enter plays";
const TOOL_SIZE: i32 = 32;
const NEW_SIZES: [usize; 6] = [16, 32, 64, 128, 256, 512];
/// The New dialog's widgets by row, which the arrow keys walk.
pub(crate) const NEW_GRID: [&[u32]; 3] = [
    &[
        ID_DIALOG,
        ID_DIALOG + 1,
        ID_DIALOG + 2,
        ID_DIALOG + 3,
        ID_DIALOG + 4,
        ID_DIALOG + 5,
    ],
    &[ID_DIALOG + 10, ID_DIALOG + 11],
    &[ID_DIALOG + 20, ID_DIALOG + 21],
];

pub(crate) const ID_FILE: u32 = 100;
pub(crate) const ID_TOOL: u32 = 120;
pub(crate) const ID_BRUSH: u32 = 140;
pub(crate) const ID_DIALOG: u32 = 160;

const SELECTION_LABELS: [&str; 3] = ["Flip H", "Flip V", "Rotate"];

/// What the tool strip can show at a given height, in interface units: the tools, which shrink
/// as the strip gets shorter. The brush's controls are in the context bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StripFit {
    tool: i32,
}

impl StripFit {
    const TOOLS: i32 = 11;
    /// The top margin, the gaps under each tool and the two separators.
    const CHROME: i32 = 6 + 2 * Self::TOOLS + 16;

    fn for_height(height: i32) -> StripFit {
        StripFit {
            tool: ((height - Self::CHROME) / Self::TOOLS).clamp(16, TOOL_SIZE),
        }
    }
}

impl App {
    pub fn draw_top(&mut self, sf: &mut Surface<'_>) {
        self.sync_text();
        let s = self.ui.scale;
        let bar = self.layout.top;
        let height = 24 * s;
        let y = bar.y + 4 * s;
        let x = self.draw_menu_bar(sf, bar.x + 8 * s, y, height);
        let ts = self.ui.text_scale();
        let text_y = bar.y + (bar.h - cap_height() * ts) / 2;
        let info = self.document_info();
        let info_x = bar.right() - text_width(&info, ts) - 10 * s;
        if info_x >= x {
            self.size_button(
                sf,
                Rect::new(info_x, y, bar.right() - info_x, height),
                &info,
            );
        }
        let label = self.document_label();
        let label_x = x + 12 * s;
        let after_label = label_x + text_width(&label, ts) + 18 * s;
        let limit = info_x - 12 * s;
        let actions = if self.has_selection() {
            self.selection_actions_width()
        } else {
            0
        };
        // The window title also names the file, so the name gives way to the actions.
        if after_label + actions <= limit {
            draw_text(sf, label_x, text_y, &label, theme::text(), ts);
            self.draw_selection_actions(sf, after_label, y, limit);
        } else {
            self.draw_selection_actions(sf, label_x, y, limit);
        }
        self.draw_first_use_hint(sf, after_label, text_y, limit);
        sf.fill(Rect::new(bar.x, bar.bottom() - s, bar.w, s), theme::rule());
    }

    /// What a new sprite is taught. Once learned it is gone for good, whatever is undone.
    pub(crate) fn first_use_hint(&mut self) -> Option<&'static str> {
        let edits = self.editor.history().undo_depth();
        if self.editor.doc().frames.len() > 1 || edits >= FIRST_USE_EDITS {
            self.first_use = false;
        }
        self.first_use
            .then_some(if edits == 0 { FIRST_HINT } else { NEXT_HINT })
    }

    fn draw_first_use_hint(&mut self, sf: &mut Surface<'_>, x: i32, y: i32, limit: i32) {
        let Some(hint) = self.first_use_hint().filter(|_| !self.has_selection()) else {
            return;
        };
        let ts = self.ui.text_scale();
        if x + text_width(hint, ts) <= limit {
            draw_text(sf, x, y, hint, theme::muted(), ts);
        }
    }

    /// Remembers what to say in the status line while the pointer is over `rect`.
    pub fn note_hint(&mut self, rect: Rect, hint: impl Into<Cow<'static, str>>) {
        let hint = hint.into();
        self.ui.name_rect(rect, &hint);
        self.say_hint_over(rect, hint);
    }

    /// Like `note_hint`, for a control whose stable name (the one `--layout-dump` lists) is
    /// shorter than the hint it shows.
    pub fn note_hint_named(&mut self, rect: Rect, name: &str, hint: impl Into<Cow<'static, str>>) {
        self.ui.name_rect(rect, name);
        self.say_hint_over(rect, hint.into());
    }

    /// The status line's hint while the pointer is over `rect`, or while the keyboard focus is
    /// on the control drawn there and the pointer is over nothing with a hint of its own: the
    /// pointer is not the only way to reach a control. Under an open menu or dialog, controls
    /// say nothing.
    pub(crate) fn say_hint_over(&mut self, rect: Rect, hint: Cow<'static, str>) {
        let under = self.menu.is_some() || self.ui.pointer_covered();
        if under {
            return;
        }
        if rect.contains(self.ui.mouse.0, self.ui.mouse.1) {
            self.hint = Some(hint);
            self.hint_from_focus = false;
        } else if self.hint.is_none() && self.focus_rect == Some(rect) {
            self.hint = Some(hint);
            self.hint_from_focus = true;
        }
    }

    fn selection_actions_width(&self) -> i32 {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        SELECTION_LABELS
            .iter()
            .map(|label| text_width(label, ts) + 18 * s)
            .sum()
    }

    /// Transform buttons, shown only when there is something to transform.
    fn draw_selection_actions(&mut self, sf: &mut Surface<'_>, left: i32, y: i32, limit: i32) {
        if !self.has_selection() {
            return;
        }
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        let mut x = left;
        for (i, label) in SELECTION_LABELS.iter().enumerate() {
            let w = text_width(label, ts) + 14 * s;
            if x + w > limit {
                return;
            }
            let rect = Rect::new(x, y, w, 24 * s);
            let hint = self.keymap.selection_hint(i);
            self.note_hint(rect, hint);
            if button(
                &mut self.ui,
                sf,
                ID_FILE + 10 + i as u32,
                rect,
                label,
                false,
            ) {
                self.defer(move |a| match i {
                    0 => a.select_change(FloatingCmd::FlipHorizontal),
                    1 => a.select_change(FloatingCmd::FlipVertical),
                    _ => a.select_change(FloatingCmd::Rotate { clockwise: true }),
                });
            }
            x += w + 4 * s;
        }
    }

    /// The document's size and zoom, which opens the Canvas size dialog when clicked.
    fn size_button(&mut self, sf: &mut Surface<'_>, r: Rect, info: &str) {
        let id = crate::canvas_dialog::ID_CANVAS + 20;
        self.note_hint(r, "Canvas size and crop");
        let state = self.ui.interact(id, r);
        self.ui.name(id, "Canvas size");
        let ts = self.ui.text_scale();
        let color = if state.hover {
            theme::text()
        } else {
            theme::muted()
        };
        draw_text(
            sf,
            r.x,
            r.y + (r.h - cap_height() * ts) / 2,
            info,
            color,
            ts,
        );
        if state.clicked {
            self.defer(|a| a.open_canvas_dialog());
        }
    }

    fn document_info(&self) -> String {
        let percent = self.view.num * 100 / self.view.den;
        let (w, h) = (self.editor.doc().width, self.editor.doc().height);
        match self.shown_bounds() {
            Some((_, _, sw, sh)) => format!("{sw}x{sh} of {w}x{h}  {percent}%"),
            None => format!("{w}x{h}  {percent}%"),
        }
    }

    pub fn draw_side(&mut self, sf: &mut Surface<'_>) {
        self.draw_context_bar(sf);
        let s = self.ui.scale;
        let strip = self.layout.side;
        let fit = StripFit::for_height(strip.h / s.max(1));
        // Short strips use smaller tools, which stay centred in the strip's width.
        // Tools are never narrower than 24 units, though they may be shorter, so that the column
        // keeps a 24 unit pitch.
        let wide = fit.tool.max(24);
        let x = strip.x + (6 + (TOOL_SIZE - wide) / 2) * s;
        let (size, width) = (fit.tool * s, wide * s);
        let y = self.draw_tool_group(sf, &PAINT_TOOLS, (x, strip.y + 6 * s), 0, (width, size));
        let separator = Rect::new(x + 4 * s, y + 3 * s, width - 8 * s, s);
        sf.fill(separator, theme::rule());
        let first = PAINT_TOOLS.len() as u32;
        self.draw_tool_group(sf, &SELECT_TOOLS, (x, y + 8 * s), first, (width, size));
        sf.fill(
            Rect::new(strip.right() - s, strip.y, s, strip.h),
            theme::rule(),
        );
    }

    fn draw_tool_group(
        &mut self,
        sf: &mut Surface<'_>,
        tools: &[ToolEntry],
        (x, top): (i32, i32),
        first_id: u32,
        (width, size): (i32, i32),
    ) -> i32 {
        let s = self.ui.scale;
        let mut y = top;
        for (i, (tool, icon, text, action, tail)) in tools.iter().enumerate() {
            let rect = Rect::new(x, y, width, size);
            let hint = self.keymap.hint(text, *action, tail);
            self.note_hint(rect, hint);
            let id = ID_TOOL + first_id + i as u32;
            if icon_button(&mut self.ui, sf, id, rect, *icon, self.tool == *tool) {
                let tool = *tool;
                self.defer(move |a| a.set_tool(tool));
            }
            y += size + 2 * s;
        }
        y
    }

    pub fn draw_new_dialog(&mut self, sf: &mut Surface<'_>, window: Window, dirty: &mut Vec<Rect>) {
        let Some(mut dialog) = self.new_dialog else {
            return;
        };
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        // A row's name above its choices: as tall as the face's capitals and a gap of 6 units.
        let named = cap_height() * ts + 6 * s;
        let (w, h) = (360 * s, 188 * s + 2 * named);
        let area = self.layout.canvas;
        let panel = Rect::new(area.x + (area.w - w) / 2, area.y + (area.h - h) / 2, w, h);
        pikslar_ui::chrome::popup(sf, panel, s);
        self.ui.focus.collect(true);
        let left = panel.x + 16 * s;
        draw_text(sf, left, panel.y + 14 * s, "New sprite", theme::text(), ts);
        let muted = theme::muted();
        let sizes_y = panel.y + 40 * s;
        draw_text(sf, left, sizes_y, "Size (px, square)", muted, ts);
        self.draw_size_choices(sf, &mut dialog, left, sizes_y + named);
        let modes_y = panel.y + 84 * s + named;
        draw_text(sf, left, modes_y, "Color mode", muted, ts);
        self.draw_mode_choices(sf, &mut dialog, left, modes_y + named);
        self.new_dialog = Some(dialog);
        let row = panel.y + 144 * s + 2 * named;
        let (create, cancel) = (
            Rect::new(left, row, 100 * s, 28 * s),
            Rect::new(left + 108 * s, row, 100 * s, 28 * s),
        );
        if default_button(&mut self.ui, sf, ID_DIALOG + 20, create, "Create") {
            self.defer(move |a| {
                if a.create_new(window, dialog) {
                    a.close_new_dialog();
                }
            });
        } else if button(&mut self.ui, sf, ID_DIALOG + 21, cancel, "Cancel", false) {
            self.close_new_dialog();
        }
        self.ui.focus.collect(false);
        dirty.push(panel.inset(-s));
    }

    fn draw_size_choices(&mut self, sf: &mut Surface<'_>, dialog: &mut NewDialog, x: i32, y: i32) {
        let s = self.ui.scale;
        for (i, &size) in NEW_SIZES.iter().enumerate() {
            let rect = Rect::new(x + i as i32 * 54 * s, y, 50 * s, 32 * s);
            let label = size.to_string();
            self.note_hint_named(rect, &label, format!("{size} by {size} pixels"));
            if button(
                &mut self.ui,
                sf,
                ID_DIALOG + i as u32,
                rect,
                &label,
                dialog.size == size,
            ) {
                dialog.size = size;
                self.ui_dirty = true;
            }
        }
    }

    fn draw_mode_choices(&mut self, sf: &mut Surface<'_>, dialog: &mut NewDialog, x: i32, y: i32) {
        let s = self.ui.scale;
        let modes = [
            (
                ColorMode::Palette,
                "Palette",
                "Palette: each pixel is a swatch, so changing a swatch recolors the picture",
            ),
            (
                ColorMode::Rgba,
                "RGBA",
                "RGBA: each pixel keeps its own color and alpha",
            ),
        ];
        for (i, (mode, label, hint)) in modes.iter().enumerate() {
            let rect = Rect::new(x + i as i32 * 124 * s, y, 120 * s, 32 * s);
            self.note_hint_named(rect, label, *hint);
            if button(
                &mut self.ui,
                sf,
                ID_DIALOG + 10 + i as u32,
                rect,
                label,
                dialog.mode == *mode,
            ) {
                dialog.mode = *mode;
                self.ui_dirty = true;
            }
        }
        sf.fill(Rect::new(x, y + 40 * s, 328 * s, s), theme::raised());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_keeps_full_size_tools_on_a_normal_window() {
        assert_eq!(StripFit::for_height(582).tool, 32);
    }

    #[test]
    fn nothing_runs_off_the_strip_at_any_useful_height() {
        for height in 242..1200 {
            let fit = StripFit::for_height(height);
            let needed = 6 + 10 * (fit.tool + 2) + 16;
            assert!(needed <= height, "{height}: {fit:?}");
        }
    }

    #[test]
    fn tools_keep_a_24_unit_pitch_down_to_a_short_strip() {
        for height in 310..1200 {
            assert!(StripFit::for_height(height).tool >= 22, "{height}");
        }
    }
}
