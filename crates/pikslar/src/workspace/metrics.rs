//! The measures the solver works with: the fixed bars, the tool strip and each panel's least
//! size. [`Metrics::LIGHTBOX`] is the redesign's (section 3 of docs/ui-redesign.md); the app
//! uses its own while panels still draw as they did before the redesign (see `layout.rs`), so
//! one solver serves both until the later phases change the bars.

use super::model::{Panel, COLOR_COMPACT, TAB_STRIP};
use super::presets::TIMELINE_LINE;

const TOOL_COUNT: i32 = 14;
const TOOL_PITCH: i32 = 30;
const TOOL_TOP: i32 = 6;
/// The colour wells at the strip's foot.
const WELLS: i32 = 36;

/// Fixed bars, tool strip and panel sizes, all in units.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Metrics {
    pub menu: i32,
    pub context: i32,
    pub status: i32,
    /// The tool strip's width in one column, and in two.
    pub tools_one: i32,
    pub tools_two: i32,
    /// The height the tool strip needs in one column, and in two.
    pub one_column_need: i32,
    pub two_column_need: i32,
    /// The least height the bottom dock leaves the middle when it grows.
    pub middle_least: i32,
    /// The bottom dock never squeezes the side docks below their least with nothing folded.
    pub keep_docks: bool,
    /// A folded slot or an empty one.
    pub strip: i32,
    /// The Color panel's compact form, tab strip included.
    pub color_compact: i32,
    /// The least height of the bottom dock.
    pub timeline_line: i32,
    /// A slot's least length down a side dock, tab strip included, Color in its full form.
    pub least: fn(Panel) -> i32,
    /// A slot's least width side by side in the bottom dock.
    pub least_across: fn(Panel) -> i32,
    /// What a folded slot showing this panel keeps, tab strip included.
    pub folded: fn(Panel) -> i32,
    /// What a folded slot takes when drawn, tab strip included: no more than `folded`, and the
    /// room between the two goes to the open slots.
    pub shown: fn(Panel) -> i32,
}

fn strip_only(_: Panel) -> i32 {
    TAB_STRIP
}

impl Metrics {
    /// The redesign's measures.
    pub(crate) const LIGHTBOX: Metrics = Metrics {
        menu: 24,
        context: 28,
        status: 20,
        tools_one: 36,
        tools_two: 64,
        one_column_need: TOOL_TOP + TOOL_COUNT * TOOL_PITCH + WELLS,
        two_column_need: TOOL_TOP + (TOOL_COUNT + 1) / 2 * TOOL_PITCH + WELLS,
        middle_least: TOOL_TOP + (TOOL_COUNT + 1) / 2 * TOOL_PITCH + WELLS,
        keep_docks: true,
        strip: TAB_STRIP,
        color_compact: COLOR_COMPACT,
        timeline_line: TIMELINE_LINE,
        least: Panel::least,
        least_across: Panel::least_across,
        folded: strip_only,
        shown: strip_only,
    };
}
