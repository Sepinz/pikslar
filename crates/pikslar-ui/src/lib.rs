//! An immediate-mode interface drawn with plain rectangles and a bitmap font.

pub mod chrome;
pub mod fallback;
pub mod focus;
pub mod fold;
pub mod font;
pub mod icons;
pub mod menu;
pub mod rail;
pub mod role;
pub mod scroll;
pub mod segmented;
pub mod splitter;
pub mod stepper;
pub mod tabs;
pub mod text_edit;
pub mod text_field;
pub mod text_fit;
pub mod theme;
pub mod tooltip;
pub mod widgets;

pub use focus::{Focus, Nudge, Press, WidgetKey};
pub use fold::fold_header;
pub use font::{cap_height, cells, draw_text, em, fit_h, text_width, ADVANCE, GLYPH_H, GLYPH_W};
pub use icons::{draw_icon, Icon, ICON_SIZE};
pub use menu::{
    menu, menu_key, menu_rect, menu_rect_with_room, menu_row, menu_with_room, MenuItem, MenuKey,
    MenuNav, MenuResult, MENU_LEVEL, MENU_PAD,
};
pub use scroll::scroll_bar;
pub use text_edit::{edit_for, multi_line, one_line, Edit, TextEdit};
pub use text_field::{draw_text_edit, first_shown};
pub use text_fit::{fit_to_width, StackText};
pub use tooltip::{draw_tooltip, tooltip_rect, wrap, TOOLTIP_WIDTH};
pub use widgets::{
    button, default_button, icon_button, icon_button_if, slider, slider_stepped, Interaction,
    Slide, Ui,
};
