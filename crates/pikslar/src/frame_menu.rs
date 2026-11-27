//! The menu a right click on a frame of the strip opens, and the popup menus' shared handling.

use crate::app::App;
use crate::brush_thumbs::{library_page, ROOM};
use pikslar_platform::{Key, Window};
use pikslar_render::{Rect, Surface};
use pikslar_ui::{
    menu_grouped, menu_key, menu_rect_with_room, MenuItem, MenuKey, MenuNav, MenuResult, MENU_LEVEL,
};
use std::borrow::Cow;

const ID_MENU: u32 = 600_000;

/// Which menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuKind {
    Frame,
    Onion,
    View,
    File,
    Edit,
    Window,
    /// The titles of the menu bar that did not fit.
    More,
    Blend,
    /// How many times the tag at this place plays.
    Tag(usize),
    /// Whether time is in milliseconds or by frame rate.
    Rate,
    /// A slice, and the document pixel it was clicked at.
    Slice {
        index: usize,
        at: (i32, i32),
    },
    /// How the selected frames ease from one place to another.
    Tween,
    /// The brush button's menu, and the menus it opens.
    Brush,
    /// The palette menu, and the menus it opens.
    Palette,
    PaletteSort,
    PaletteRamp,
    PaletteReplace,
    PaletteImport,
    /// A page of the groups, and one group's menu.
    PaletteGroups(usize),
    PaletteGroup(usize),
    BrushHandle,
    BrushRecent,
    /// A page of the library, to choose a brush from.
    BrushLibrary(usize),
    /// A page of the library, to remove a brush from.
    BrushRemove(usize),
    /// A page of the library, to rename a brush from.
    BrushRename(usize),
    /// The tile menu, the tileset menu and the menus they open, and the New tilemap layer menu.
    Tile,
    Tileset,
    TileSize,
    TileUnused,
    NewTilemap,
    /// The terrain menu, the kinds of a new terrain set, and a slot's menu.
    Terrain,
    TerrainNew,
    TerrainSlot,
    /// The Scripts menu, and one plugin's menu.
    Scripts,
    Plugin(usize),
    /// The context bar's settings that did not fit.
    BarMore,
    /// A dock slot's menu, for the panel of the tab it was opened on.
    Slot {
        side: crate::workspace::model::Side,
        slot: usize,
        panel: crate::workspace::model::Panel,
    },
}

/// A menu's items as they are offered: some names are made from the document or the files.
pub(crate) type Entries = Vec<(Cow<'static, str>, bool)>;

/// Room a menu leaves before its labels: the library menus show a picture of each brush there.
fn picture_room(kind: MenuKind) -> i32 {
    if kind == MenuKind::View {
        return crate::view_menu::VIEW_ROOM;
    }
    library_page(kind).map_or(0, |_| ROOM)
}

pub(crate) fn as_items(entries: &Entries) -> Vec<MenuItem<'_>> {
    entries
        .iter()
        .map(|(label, enabled)| MenuItem {
            label,
            enabled: *enabled,
        })
        .collect()
}

/// The rows that start a group in the top menus, by the words of the row that starts each:
/// a one-unit rule parts them (R1, A17). Words that are not in the menu start no group.
fn group_words(kind: MenuKind) -> &'static [&'static [&'static str]] {
    use crate::view_menu as v;
    match kind {
        MenuKind::File => &[&["Open a file..."], &["Save"], &["Export..."]],
        MenuKind::Edit => &[
            &["Cut"],
            &["Select all"],
            &["Flip across"],
            &["Canvas size..."],
        ],
        MenuKind::View => &[
            &[v::KEYS_ITEM],
            &[v::REWIND_ON, v::REWIND_OFF],
            &["Hide slices", "Show slices"],
            &[crate::large_text::LABEL, crate::large_text::LABEL_ON],
            &[v::DIAG_ON, v::DIAG_OFF, v::DIAG_RUN_OFF],
        ],
        _ => &[],
    }
}

impl App {
    /// Where the groups of the open menu start, as row numbers.
    pub(crate) fn menu_groups(&self, kind: MenuKind, entries: &Entries) -> Vec<usize> {
        if kind == MenuKind::Window {
            // The workspaces, then the panels, then what changes the workspace.
            let presets = self.preset_count();
            return vec![presets, presets + self.panel_count()];
        }
        let words = |n: usize| pikslar_ui::menu::split_hint(&entries[n].0).0;
        group_words(kind)
            .iter()
            .filter_map(|starts| (1..entries.len()).find(|&n| starts.contains(&words(n))))
            .collect()
    }
}

/// An open menu, and where it was opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OpenMenu {
    pub at: (i32, i32),
    pub kind: MenuKind,
}

/// What the frame menu offers, in order.
const LABELS: [&str; 5] = [
    "New frame",
    "Duplicate frame",
    "Delete frame",
    "Hold drawing",
    "Tween position",
];

impl App {
    /// Opens the frame menu if `(x, y)` is on a frame of the strip, after making that frame
    /// current unless it is one of the selected frames.
    pub(crate) fn open_frame_menu(&mut self, x: i32, y: i32) -> bool {
        let Some(frame) = self.frame_at(x, y) else {
            return false;
        };
        let chosen = self
            .selected_frames()
            .is_some_and(|(a, b)| (a..=b).contains(&frame));
        // Playback stops, so the command acts on the frame that was clicked.
        self.playing = false;
        if !chosen {
            if !self.settle() {
                return false;
            }
            self.timeline.press(frame, frame, false);
            self.set_frame(frame);
        }
        self.menu = Some(OpenMenu {
            at: (x, y),
            kind: MenuKind::Frame,
        });
        self.ui_dirty = true;
        true
    }

    /// A key pressed while a menu is open: Up, Down, Home and End move the highlight, a letter
    /// jumps to a row, Enter chooses it and Esc closes the menu.
    pub(crate) fn menu_key(&mut self, key: Key, window: Window) {
        let Some(open) = self.menu else {
            return;
        };
        let nav = match key {
            Key::Escape => return self.close_menu(),
            Key::Up => MenuKey::Up,
            Key::Down => MenuKey::Down,
            Key::Home => MenuKey::Home,
            Key::End => MenuKey::End,
            Key::Enter => MenuKey::Enter,
            Key::Char(c) => MenuKey::Letter(char::from(c)),
            _ => return,
        };
        let entries = self.menu_entries(open.kind);
        let current = self.menu_cursor.filter(|(m, _)| *m == open).map(|(_, n)| n);
        match menu_key(&as_items(&entries), current, nav) {
            MenuNav::Highlight(n) => self.menu_cursor = Some((open, n)),
            MenuNav::Chosen(n) => {
                self.close_menu();
                self.menu_chosen(open, n, window);
            }
            MenuNav::None => {}
        }
        self.ui_dirty = true;
    }

    pub(crate) fn close_menu(&mut self) {
        if self.menu.take().is_some() {
            self.menu_cursor = None;
            self.ctx.menu_closed();
            // It may have been over the canvas.
            self.full_canvas = true;
            self.ui_dirty = true;
        }
    }

    fn screen(&self) -> Rect {
        Rect::new(0, 0, self.layout.bottom.w, self.layout.bottom.bottom())
    }

    pub(crate) fn menu_entries(&self, kind: MenuKind) -> Entries {
        let fixed = match kind {
            MenuKind::Onion => self.onion_items(),
            MenuKind::View => self.view_items(),
            MenuKind::Blend => self.blend_items(),
            MenuKind::Tag(tag) => self.tag_items(tag),
            MenuKind::Rate => self.rate_items(),
            MenuKind::Tween => self.tween_items(),
            MenuKind::Slice { index, at } => self.slice_items(index, at),
            MenuKind::File => return self.file_entries(),
            MenuKind::Edit => return self.edit_entries(),
            MenuKind::Window => return self.window_entries(),
            MenuKind::More => return self.more_entries(),
            MenuKind::BarMore => return self.bar_more_entries(),
            MenuKind::Scripts => return self.scripts_entries(),
            MenuKind::Plugin(n) => return self.plugin_entries(n),
            MenuKind::Slot { side, slot, panel } => {
                return self.dock_slot_entries(side, slot, panel)
            }
            MenuKind::Palette
            | MenuKind::PaletteSort
            | MenuKind::PaletteRamp
            | MenuKind::PaletteReplace
            | MenuKind::PaletteImport
            | MenuKind::PaletteGroups(_)
            | MenuKind::PaletteGroup(_) => return self.palette_entries(kind),
            MenuKind::Brush
            | MenuKind::BrushHandle
            | MenuKind::BrushRecent
            | MenuKind::BrushLibrary(_)
            | MenuKind::BrushRemove(_)
            | MenuKind::BrushRename(_) => return self.brush_entries(kind),
            MenuKind::Tile
            | MenuKind::Tileset
            | MenuKind::TileSize
            | MenuKind::TileUnused
            | MenuKind::NewTilemap
            | MenuKind::Terrain
            | MenuKind::TerrainNew
            | MenuKind::TerrainSlot => return self.tile_entries(kind),
            MenuKind::Frame => {
                let several = self.editor.doc().frames.len() > 1;
                LABELS
                    .map(|label| MenuItem {
                        label,
                        enabled: several || label != "Delete frame",
                    })
                    .to_vec()
            }
        };
        fixed
            .into_iter()
            .map(|item| (Cow::Borrowed(item.label), item.enabled))
            .collect()
    }

    pub(crate) fn menu_area(&self, open: OpenMenu) -> Rect {
        menu_rect_with_room(
            self.ui.scale,
            open.at,
            &as_items(&self.menu_entries(open.kind)),
            self.screen(),
            picture_room(open.kind),
        )
    }

    /// Before anything is drawn: an open menu takes the pointer from everything under it.
    pub(crate) fn cover_menu(&mut self) {
        if self.menu.is_some()
            || self.keys_dialog.is_some()
            || self.canvas_dialog.is_some()
            || self.scripts.open
            || self.plugins.review.is_some()
        {
            let whole = self.screen();
            self.ui.cover(whole, MENU_LEVEL);
        }
        // A brush's name box floats over the panels; a press on it only keeps the name.
        if let Some(naming) = self.brush_naming_rect() {
            self.ui.cover(naming.inset(-self.ui.scale), MENU_LEVEL);
        }
        // The export panel can reach over the side panels in a small window, and a click on it
        // must not also reach what is under it.
        if let Some(choice) = self.export_dialog {
            let panel = self.export_panel_rect(choice);
            self.ui.cover(panel.inset(-self.ui.scale), MENU_LEVEL);
        }
    }

    /// Drawn after everything else, so it is on top.
    pub(crate) fn draw_menu(
        &mut self,
        sf: &mut Surface<'_>,
        window: Window,
        dirty: &mut Vec<Rect>,
    ) {
        let Some(open) = self.menu else {
            return;
        };
        let rect = self.menu_area(open);
        let entries = self.menu_entries(open.kind);
        let room = picture_room(open.kind);
        self.ui.menu_highlight = self.menu_cursor.filter(|(m, _)| *m == open).map(|(_, n)| n);
        self.ctx.menu_starts_at_top(&mut self.ui, open);
        let groups = self.menu_groups(open.kind, &entries);
        let items = as_items(&entries);
        let chosen = menu_grouped(&mut self.ui, sf, (ID_MENU, rect), &items, room, &groups);
        if open.kind == MenuKind::View {
            self.draw_view_marks(sf, rect);
        }
        if let Some(page) = library_page(open.kind) {
            self.draw_library_thumbs(sf, rect, page);
        }
        match chosen {
            MenuResult::Open => {}
            MenuResult::Dismissed => self.defer(|a| a.close_menu()),
            MenuResult::Chosen(n) => self.defer(move |a| {
                a.close_menu();
                a.menu_chosen(open, n, window);
            }),
        }
        dirty.push(rect.inset(-self.ui.scale));
    }

    /// Does what row `n` of the menu `open` says.
    fn menu_chosen(&mut self, open: OpenMenu, n: usize, window: Window) {
        // A row chosen from the menu bar hands the keyboard back to the canvas.
        if crate::menu_bar::bar_index(open.kind).is_some() {
            self.ui.focus.clear();
        }
        match open.kind {
            MenuKind::Frame => self.frame_menu_action(n, open.at),
            MenuKind::Tween => self.tween_menu_action(n),
            MenuKind::Onion => self.onion_menu_action(n),
            MenuKind::View => self.view_menu_action(n, window),
            MenuKind::File => self.file_menu_action(n, window),
            MenuKind::Edit => self.edit_menu_action(n, window),
            MenuKind::Window => self.window_menu_action(n),
            MenuKind::More => self.more_menu_action(n),
            MenuKind::Scripts => self.scripts_menu_action(n, open.at, window),
            MenuKind::Plugin(p) => self.plugin_menu_action(p, n),
            MenuKind::BarMore => self.bar_more_action(n),
            MenuKind::Slot { side, slot, panel } => self.slot_menu_action((side, slot), panel, n),
            MenuKind::Blend => self.blend_menu_action(n),
            MenuKind::Tag(tag) => self.tag_menu_action(tag, n),
            MenuKind::Rate => self.rate_menu_action(n),
            MenuKind::Slice { index, at } => self.slice_menu_action(index, at, n),
            kind @ (MenuKind::Palette
            | MenuKind::PaletteSort
            | MenuKind::PaletteRamp
            | MenuKind::PaletteReplace
            | MenuKind::PaletteImport
            | MenuKind::PaletteGroups(_)
            | MenuKind::PaletteGroup(_)) => self.palette_menu_action(kind, n, open.at, window),
            kind @ (MenuKind::Brush
            | MenuKind::BrushHandle
            | MenuKind::BrushRecent
            | MenuKind::BrushLibrary(_)
            | MenuKind::BrushRemove(_)
            | MenuKind::BrushRename(_)) => self.brush_menu_action(kind, n, open.at),
            kind @ (MenuKind::Tile
            | MenuKind::Tileset
            | MenuKind::TileSize
            | MenuKind::TileUnused
            | MenuKind::NewTilemap
            | MenuKind::Terrain
            | MenuKind::TerrainNew
            | MenuKind::TerrainSlot) => self.tile_menu_action(kind, n, open.at),
        }
    }

    fn frame_menu_action(&mut self, n: usize, at: (i32, i32)) {
        match n {
            0 => self.new_frame(false),
            1 => self.new_frame(true),
            2 => self.remove_frames(),
            3 => self.toggle_hold(),
            4 => self.open_tween_menu(at),
            _ => {}
        }
        self.ui_dirty = true;
    }
}
