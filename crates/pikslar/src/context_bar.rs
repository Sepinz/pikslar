//! The context bar (docs/ui-redesign.md 3.4): one row under the top bar showing the active
//! tool and its settings. What each tool shows is in `context_tools.rs`; when the bar is too
//! narrow its settings give way from the right into a More menu, never the brush size.

use crate::app::App;
use crate::context_tools::settings;
use pikslar_core::SelectOp;
use pikslar_render::{Rect, Surface};
use pikslar_ui::chrome::{self, Look};
use pikslar_ui::role::{Role, State};
use pikslar_ui::{cap_height, draw_icon, draw_text, text_width, ICON_SIZE};
use pikslar_ui::{fit_to_width, theme};

/// The bar's group (a named rectangle, no widget), the size stepper's three ids, and the More
/// button. The other controls keep the brush controls' old ids or sit beside the text bar's.
pub(crate) const ID_STEPPER: u32 = 146;
pub(crate) const ID_MORE: u32 = 149;
/// The selection mode switch and the wand's switch: a container and its segments.
pub(crate) const ID_MODE: u32 = 150;
pub(crate) const ID_WAND: u32 = 155;
pub(crate) const ID_ONLY_CEL: u32 = 159;
/// The Tiles and Pixels switch: the container is just below `ID_TILES`, its segments are
/// `ID_TILES` and `ID_TILES + 1`, as the buttons it replaces were.
pub(crate) const ID_TILE_SWITCH: u32 = crate::panel_tiles::ID_TILES - 1;

/// Controls are 20 units tall in the 28-unit bar: room for the focus ring, which reaches three
/// units outside a control, without leaving the bar.
pub(crate) const CONTROL: i32 = 20;

/// The height in units of the bar's controls in the face in use: [`CONTROL`], and two more in
/// *Large text*, whose doubled letters would otherwise reach the button's lower edge.
pub(crate) fn control() -> i32 {
    match pikslar_ui::font::face() {
        pikslar_ui::font::Face::Text => CONTROL,
        pikslar_ui::font::Face::Mono => CONTROL + 2,
    }
}
/// Room at the bar's ends, and between controls.
const MARGIN: i32 = 8;
pub(crate) const GAP: i32 = 4;

/// What the bar remembers: settings that have no other home, and where it last drew.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContextBar {
    /// How a new selection combines with the old one, before Shift and Alt.
    pub select: SelectOp,
    /// The wand picks every matching pixel, not the connected area (Ctrl turns it round).
    pub all_matching: bool,
    /// The Move tool moves the active cel alone (Alt turns it round).
    pub only_cel: bool,
    /// The settings from this place in the tool's list went into the More menu.
    pub hidden_from: usize,
    /// Where the Brush and More buttons were drawn, for their menus.
    pub brush: Rect,
    pub more: Rect,
}

impl Default for ContextBar {
    fn default() -> Self {
        ContextBar {
            select: SelectOp::Replace,
            all_matching: false,
            only_cel: false,
            hidden_from: usize::MAX,
            brush: Rect::default(),
            more: Rect::default(),
        }
    }
}

/// How many settings are shown, and whether they use their short forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub shown: usize,
    pub narrow: bool,
    pub more: bool,
}

/// Fits settings with `(wide, short)` widths into `room`, `gap` apart: all of them, wide if
/// they fit, else short; else as many short ones as fit beside a More button `more_w` wide.
/// The first is always shown, since it is the brush size where there is one.
pub(crate) fn plan(room: i32, widths: &[(i32, i32)], (gap, more_w): (i32, i32)) -> Plan {
    let total = |pick: fn(&(i32, i32)) -> i32| {
        widths.iter().map(pick).sum::<i32>() + gap * widths.len().saturating_sub(1) as i32
    };
    if total(|w| w.0) <= room {
        return Plan {
            shown: widths.len(),
            narrow: false,
            more: false,
        };
    }
    if total(|w| w.1) <= room {
        return Plan {
            shown: widths.len(),
            narrow: true,
            more: false,
        };
    }
    let mut used = more_w;
    let mut shown = 0;
    for w in widths {
        used += gap + w.1;
        if used > room && shown > 0 {
            break;
        }
        shown += 1;
    }
    Plan {
        shown,
        narrow: true,
        more: shown < widths.len(),
    }
}

impl App {
    /// The bar: the tool's badge and settings, and on a tilemap layer the Tiles and Pixels
    /// switch at the right. Drawn before the tool strip, so Tab walks the menu, the bar, the
    /// tools.
    pub(crate) fn draw_context_bar(&mut self, sf: &mut Surface<'_>) {
        let bar = self.layout.context;
        if bar.h == 0 {
            return;
        }
        let s = self.ui.scale;
        sf.fill(Rect::new(bar.x, bar.bottom() - s, bar.w, s), theme::rule());
        let y = bar.y + (bar.h - (control() + 1) * s) / 2;
        let mut limit = bar.right() - MARGIN * s;
        if self.active_tileset().is_some() {
            limit = self.draw_tile_switch(sf, y, limit) - GAP * s;
        }
        let name = self.tool_name();
        self.ui.name_rect(bar, "Tool settings");
        let left = self.draw_badge(sf, bar, name, limit);
        if self.text.is_some() {
            self.draw_text_bar(sf, left, y, limit);
            return;
        }
        self.draw_settings(sf, (left, y), limit);
    }

    /// The tool's icon in the accent colour and, if it leaves room for the size, its name.
    /// Returns where the settings start.
    fn draw_badge(&mut self, sf: &mut Surface<'_>, bar: Rect, name: &str, limit: i32) -> i32 {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let icon = crate::context_tools::badge(self.tool);
        let is = self.ui.icon_scale();
        let side = ICON_SIZE * is;
        let x = bar.x + MARGIN * s;
        let y = bar.y + (bar.h - side) / 2;
        draw_icon(sf, x, y, icon, theme::accent(), is);
        let icon_end = x + side + 8 * s;
        let named = icon_end + text_width(name, ts) + 12 * s;
        let first = settings(self.tool)
            .first()
            .map_or(0, |&f| self.setting_width(f).0);
        let room = limit - named;
        let words = room >= first || settings(self.tool).is_empty();
        self.ui.name_rect(
            Rect::new(x, y, if words { named - x } else { side }, side),
            &format!("Active tool: {name}"),
        );
        if !words {
            return icon_end;
        }
        let ty = bar.y + (bar.h - cap_height() * ts) / 2;
        draw_text(sf, icon_end - 2 * s, ty, name, theme::text(), ts);
        named
    }

    fn draw_settings(&mut self, sf: &mut Surface<'_>, (left, y): (i32, i32), limit: i32) {
        let s = self.ui.scale;
        let list = settings(self.tool);
        let widths: Vec<(i32, i32)> = list.iter().map(|&f| self.setting_width(f)).collect();
        let more_w = 28 * s;
        let fit = plan(limit - left, &widths, (GAP * s, more_w));
        let mut x = left;
        for (&setting, &(wide, short)) in list.iter().zip(&widths).take(fit.shown) {
            let w = if fit.narrow { short } else { wide };
            self.draw_setting(sf, setting, Rect::new(x, y, w, control() * s), fit.narrow);
            x += w + GAP * s;
        }
        self.bar.hidden_from = if fit.more { fit.shown } else { usize::MAX };
        if fit.more {
            let rect = Rect::new(x, y, more_w, control() * s);
            self.bar.more = rect;
            self.draw_more_button(sf, rect);
        }
    }

    /// A button with words on a raised face, chosen when `on`. Names it `name`.
    pub(crate) fn bar_button(
        &mut self,
        sf: &mut Surface<'_>,
        (id, r): (u32, Rect),
        (label, name): (&str, &str),
        on: Option<bool>,
    ) -> bool {
        let s = self.ui.scale;
        let state = self.ui.interact(id, r);
        match on {
            Some(on) => self.ui.describe(Role::Toggle, State::chosen(on)),
            None => self.ui.describe(Role::Button, State::NONE),
        }
        self.ui.name(id, name);
        let look = Look::of(state, on == Some(true), false);
        chrome::face(sf, r, s, theme::panel(), look);
        let ts = self.ui.text_scale();
        let label = fit_to_width(label, r.w - 4 * s, ts);
        let x = r.x + (r.w - text_width(&label, ts)) / 2;
        let y = r.y + (r.h - cap_height() * ts) / 2 + look.drop() * s;
        draw_text(sf, x, y, &label, look.ink(), ts);
        self.ui.ring(sf, id, r);
        state.clicked
    }

    fn draw_more_button(&mut self, sf: &mut Surface<'_>, r: Rect) {
        self.note_hint(r, "More settings of this tool");
        let open = self
            .menu
            .is_some_and(|m| m.kind == crate::frame_menu::MenuKind::BarMore);
        if self.bar_button(sf, (ID_MORE, r), ("...", "More"), None) && !open {
            let at = (r.x, r.bottom());
            self.defer(move |a| a.open_bar_more(at));
        }
    }

    /// The Tiles and Pixels switch, right-aligned to `right`. Returns its left edge.
    fn draw_tile_switch(&mut self, sf: &mut Surface<'_>, y: i32, right: i32) -> i32 {
        use crate::panel_tiles::TileMode;
        use pikslar_ui::segmented::{segmented, Segment, Show};
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let seg = text_width("Pixels", ts) + 12 * s;
        let r = Rect::new(right - (2 * seg + s), y, 2 * seg + s, control() * s);
        let segments = [
            Segment {
                name: "Tiles",
                show: Show::Words("Tiles"),
            },
            Segment {
                name: "Pixels",
                show: Show::Words("Pixels"),
            },
        ];
        let hint = |a: &App, text| a.keymap.hint(text, crate::shortcuts::Action::TileMode, "");
        let (tiles, pixels) = (hint(self, TILES_HINT), hint(self, PIXELS_HINT));
        let half = Rect::new(r.x, r.y, r.w / 2, r.h);
        self.note_hint(half, tiles);
        self.note_hint(Rect::new(half.right(), r.y, r.w - half.w, r.h), pixels);
        let chosen = usize::from(self.tile_ui.mode == TileMode::Pixels);
        let id = (ID_TILE_SWITCH, r);
        if let Some(n) = segmented(&mut self.ui, sf, id, theme::panel(), &segments, chosen) {
            let mode = [TileMode::Tiles, TileMode::Pixels][n];
            self.defer(move |a| a.set_tile_mode(mode));
        }
        r.x
    }
}

const TILES_HINT: &str = "Tiles mode: the tools place, clear and pick tiles";
const PIXELS_HINT: &str = "Pixels mode: draw on the tiles placed here";

#[cfg(test)]
#[path = "context_bar_tests.rs"]
mod tests;
