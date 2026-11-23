//! The four built-in presets' default arrangements (section 3.6 of docs/ui-redesign.md).
//!
//! | Preset | Right dock | Bottom dock | Left dock |
//! |---|---|---|---|
//! | Draw | Color (full); Palette; Layers, Slices, Pixel Lint | Timeline (single line) | closed |
//! | Animate | Palette, Color (compact); Preview | Timeline, 6 rows; History tab | closed |
//! | Tiles | Tiles (tall); Palette, Color; Layers | Timeline (single line) | closed |
//! | Palette | Palette (tall, 320u wide), Color (full) | Timeline (single line) | Pixel Lint (rail) |

use super::model::{Area, Panel, PanelSet, Preset, Side, Slot, Workspace};

/// The right dock's default width (units).
pub(crate) const RIGHT_WIDTH: i32 = 248;
/// The left dock's width when first opened (units).
pub(crate) const LEFT_WIDTH: i32 = 240;
/// The timeline's single line (units).
pub(crate) const TIMELINE_LINE: i32 = 28;
/// One timeline row (units).
pub(crate) const TIMELINE_ROW: i32 = 16;
/// Animate's open timeline (six rows, with the frame-number and tag rows), 168 units: the height
/// that gives the canvas the "about 53 %" of section 3.8 at 1366 by 705 with a 232u right dock.
pub(crate) const ANIMATE_BOTTOM: i32 = 168;
/// Animate's narrower right dock, which keeps its canvas share above 52 % (decision 6).
pub(crate) const ANIMATE_RIGHT: i32 = 232;
/// Palette's wide right dock.
pub(crate) const PALETTE_RIGHT: i32 = 320;

/// The default arrangement of `preset`. The custom workspace has none of its own and falls
/// back to Draw's.
pub(crate) fn default_layout(preset: Preset) -> Workspace {
    match preset {
        Preset::Draw | Preset::Custom => draw(),
        Preset::Animate => animate(),
        Preset::Tiles => tiles(),
        Preset::Palette => palette(),
    }
}

fn timeline_line() -> Area {
    Area::new(TIMELINE_LINE, true, &[Slot::new(0, &[Panel::Timeline])])
}

fn closed_left() -> Area {
    Area::new(LEFT_WIDTH, false, &[])
}

fn workspace(right: Area, bottom: Area, left: Area, compact: bool) -> Workspace {
    Workspace::default()
        .with_area(Side::Right, right)
        .with_area(Side::Bottom, bottom)
        .with_area(Side::Left, left)
        .with_compact(compact)
}

fn draw() -> Workspace {
    let right = Area::new(
        RIGHT_WIDTH,
        true,
        &[
            Slot::new(150, &[Panel::Color]),
            Slot::new(112, &[Panel::Palette]),
            Slot::new(0, &[Panel::Layers, Panel::Slices, Panel::Lint]),
        ],
    );
    workspace(right, timeline_line(), closed_left(), false)
}

fn animate() -> Workspace {
    let right = Area::new(
        ANIMATE_RIGHT,
        true,
        &[
            Slot::new(200, &[Panel::Palette, Panel::Color]),
            Slot::new(0, &[Panel::Preview]),
        ],
    );
    let bottom = Area::new(
        ANIMATE_BOTTOM,
        true,
        &[Slot::new(0, &[Panel::Timeline, Panel::History])],
    );
    workspace(right, bottom, closed_left(), true)
}

fn tiles() -> Workspace {
    let right = Area::new(
        RIGHT_WIDTH,
        true,
        &[
            Slot::new(0, &[Panel::Tiles]),
            Slot::new(150, &[Panel::Palette, Panel::Color]),
            Slot::new(110, &[Panel::Layers]),
        ],
    );
    workspace(right, timeline_line(), closed_left(), false)
}

fn palette() -> Workspace {
    let right = Area::new(
        PALETTE_RIGHT,
        true,
        &[
            Slot::new(0, &[Panel::Palette]),
            Slot::new(150, &[Panel::Color]),
        ],
    );
    // Pixel Lint waits on its rail, one click away: open, it held one button beside a canvas
    // with a quarter of the window (final review, defect 5).
    let left = Area::new(LEFT_WIDTH, false, &[Slot::new(0, &[Panel::Lint])]);
    workspace(right, timeline_line(), left, false)
}

/// The panels a preset places by default; the repair rules put any of these back when a saved
/// arrangement lost one it did not hide.
pub(crate) fn default_panels(preset: Preset) -> PanelSet {
    default_layout(preset).placed()
}

#[cfg(test)]
#[path = "presets_tests.rs"]
mod tests;
