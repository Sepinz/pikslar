//! Custom brushes in the interface: the brush menu, Ctrl+B, turning with the flip keys, and the
//! library. Brushes are view state, so nothing here is an undo step or marks the file changed.

use crate::app::{App, ToolChoice};
use crate::brush_button::Thumb;
use crate::brush_store::{from_library, to_library, BrushStore, Saved};
use crate::frame_menu::{Entries, MenuKind, OpenMenu};
use crate::shortcuts::Action;
use pikslar_core::custom_brush::{Brush, BrushChoice, BrushPaint, Handle, Preset};
use pikslar_core::{Tool, Turn};
use std::borrow::Cow;
use std::sync::Arc;

/// Rows of brushes a menu page lists, before "More...".
pub(crate) const PAGE: usize = 12;
const SHAPES: [(&str, BrushChoice); 5] = [
    ("Round brush", BrushChoice::Round),
    ("Square", BrushChoice::Preset(Preset::Square)),
    ("Diamond", BrushChoice::Preset(Preset::Diamond)),
    (
        "Horizontal line",
        BrushChoice::Preset(Preset::HorizontalLine),
    ),
    ("Vertical line", BrushChoice::Preset(Preset::VerticalLine)),
];
const HANDLES: [(Handle, &str); 5] = [
    (Handle::Centre, "Handle in the centre"),
    (Handle::Corner(0), "Handle at the top left"),
    (Handle::Corner(1), "Handle at the top right"),
    (Handle::Corner(2), "Handle at the bottom left"),
    (Handle::Corner(3), "Handle at the bottom right"),
];

/// Where each row of the brush menu is.
mod row {
    pub const CAPTURE: usize = 5;
    pub const OWN: usize = 6;
    pub const SHAPE: usize = 7;
    pub const HANDLE: usize = 8;
    pub const RECENT: usize = 9;
    pub const LIBRARY: usize = 10;
    pub const SAVE: usize = 11;
    pub const REMOVE: usize = 12;
    pub const RENAME: usize = 13;
    pub const LARGER: usize = 14;
    pub const SMALLER: usize = 15;
}

/// What the app keeps for brushes besides the editor's own state.
#[derive(Default)]
pub(crate) struct BrushUi {
    pub store: BrushStore,
    /// The library row that has been chosen once for removal.
    armed: Option<usize>,
    /// The pictures of the library, for its menus.
    pub thumbs: crate::brush_thumbs::Thumbs,
    /// The library brush whose name is being typed.
    pub naming: Option<crate::brush_naming::BrushNaming>,
    /// The picture on the tool strip button.
    pub thumb: Thumb,
}

impl BrushUi {
    pub fn new(store: BrushStore) -> Self {
        BrushUi {
            store,
            ..BrushUi::default()
        }
    }
}

/// A menu row.
fn row_of(label: impl Into<Cow<'static, str>>, enabled: bool) -> (Cow<'static, str>, bool) {
    (label.into(), enabled)
}

/// A row that names its state, as the interface-size rows do.
fn marked(label: &'static str, on: bool) -> (Cow<'static, str>, bool) {
    if on {
        (Cow::Owned(format!("{label}, in use")), false)
    } else {
        (Cow::Borrowed(label), true)
    }
}

fn describe(brush: &Brush) -> String {
    let (w, h) = brush.size();
    format!("{}  {w}x{h}", brush.name())
}

impl App {
    /// Whether the tool in hand paints with the brush.
    pub(crate) fn holds_brush_tool(&self) -> bool {
        matches!(
            self.tool,
            ToolChoice::Paint(Tool::Pencil | Tool::Eraser | Tool::Line | Tool::Rect)
        )
    }

    /// Chooses the brush, and a tool that paints with it.
    fn choose_brush(&mut self, choice: BrushChoice) {
        self.editor.set_brush(choice);
        if !self.holds_brush_tool() {
            self.set_tool(ToolChoice::Paint(Tool::Pencil));
        }
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    /// Ctrl+B: a brush from the floating pixels or the selection.
    pub(crate) fn capture_brush_key(&mut self) {
        self.finish_gesture();
        match self.editor.capture_brush(BrushPaint::OwnColors) {
            Ok(()) => {
                if let Some(brush) = self.editor.brushes().active_custom() {
                    let said = format!("Brush captured: {}", describe(brush));
                    self.notify(said);
                }
                if !self.holds_brush_tool() {
                    self.set_tool(ToolChoice::Paint(Tool::Pencil));
                }
                self.full_canvas = true;
                self.ui_dirty = true;
            }
            Err(e) => self.notify_error(e.to_string()),
        }
    }

    /// Turns the brush instead of the selection when a custom brush is being painted with and
    /// nothing is floating. Returns whether it did.
    pub(crate) fn turn_brush_key(&mut self, turn: Turn) -> bool {
        let wanted = self.editor.brushes().active_custom().is_some()
            && !self.editor.is_floating()
            && self.holds_brush_tool();
        if wanted {
            self.editor.turn_brush(turn);
            self.full_canvas = true;
            self.ui_dirty = true;
        }
        wanted
    }

    /// Opens the brush menu at `at`. The library is read now, the first time.
    pub(crate) fn open_brush_menu(&mut self, at: (i32, i32)) {
        self.read_library();
        self.brush_ui.armed = None;
        self.open_brush_kind(at, MenuKind::Brush);
    }

    pub(crate) fn open_brush_kind(&mut self, at: (i32, i32), kind: MenuKind) {
        self.menu = Some(OpenMenu { at, kind });
        self.ui_dirty = true;
    }

    /// Reads the library if it has not been, and says what went wrong with it.
    fn read_library(&mut self) {
        if let Some(said) = self.brush_ui.store.ensure_read() {
            self.notify(said);
        }
    }

    pub(crate) fn brush_entries(&self, kind: MenuKind) -> Entries {
        match kind {
            MenuKind::BrushHandle => self.handle_entries(),
            MenuKind::BrushRecent => self
                .editor
                .brushes()
                .recent()
                .iter()
                .map(|b| row_of(describe(b), true))
                .collect(),
            MenuKind::BrushLibrary(page) => self.library_entries(page, false),
            MenuKind::BrushRemove(page) => self.library_entries(page, true),
            MenuKind::BrushRename(page) => self.library_entries(page, false),
            _ => self.main_entries(),
        }
    }

    fn main_entries(&self) -> Entries {
        let state = self.editor.brushes();
        let custom = state.active_custom();
        let mut rows: Entries = SHAPES
            .iter()
            .map(|(label, choice)| marked(label, state.active() == choice))
            .collect();
        let own = custom.is_some_and(|b| b.paint() == BrushPaint::OwnColors);
        let shape = custom.is_some_and(|b| b.paint() == BrushPaint::Shape);
        let has_custom = custom.is_some();
        let library = self.brush_ui.store.brushes().len();
        let can_capture = self.editor.is_floating() || self.has_selection();
        let capture = self
            .keymap
            .hint("Capture from selection", Action::CaptureBrush, "");
        rows.push(row_of(capture, can_capture));
        rows.push(row_marked("Paint its own colors", own, has_custom));
        rows.push(row_marked(
            "Paint as a shape in the color",
            shape,
            has_custom,
        ));
        rows.push(row_of("Handle...", has_custom));
        rows.push(row_of("Recent brushes...", !state.recent().is_empty()));
        rows.push(row_of(format!("Library ({library})..."), library > 0));
        rows.push(row_of("Save to the library", has_custom));
        rows.push(row_of("Remove from the library...", library > 0));
        rows.push(row_of("Rename in the library...", library > 0));
        let size = self.editor.brush_size;
        // The same as the [ and ] keys and the stepper, for a window too short to have one.
        rows.push(row_of(format!("Larger, {size} to {}", size + 1), size < 64));
        rows.push(row_of(format!("Smaller, {size} to {}", size - 1), size > 1));
        rows
    }

    fn handle_entries(&self) -> Entries {
        let current = self.editor.brushes().active_custom().map(|b| b.handle());
        HANDLES
            .iter()
            .map(|&(handle, label)| marked(label, current == Some(handle)))
            .collect()
    }

    /// One page of the library as rows, with "More..." when it goes on. When removing, a row
    /// chosen once asks again.
    fn library_entries(&self, page: usize, removing: bool) -> Entries {
        let all = self.brush_ui.store.brushes();
        let start = page * PAGE;
        let mut rows: Entries = all
            .iter()
            .enumerate()
            .skip(start)
            .take(PAGE)
            .map(|(i, b)| {
                let label = if removing && self.brush_ui.armed == Some(i) {
                    format!("Remove {}? Choose again", b.name)
                } else {
                    format!("{}  {}x{}", b.name, b.width, b.height)
                };
                row_of(label, true)
            })
            .collect();
        if all.len() > start + PAGE {
            rows.push(row_of("More...", true));
        }
        rows
    }

    pub(crate) fn brush_menu_action(&mut self, kind: MenuKind, n: usize, at: (i32, i32)) {
        match kind {
            MenuKind::BrushHandle => {
                if let Some(&(handle, _)) = HANDLES.get(n) {
                    self.editor.set_brush_handle(handle);
                    self.full_canvas = true;
                }
            }
            MenuKind::BrushRecent => {
                let picked = self.editor.brushes().recent().get(n).cloned();
                if let Some(brush) = picked {
                    self.choose_brush(BrushChoice::Custom(brush));
                }
            }
            MenuKind::BrushLibrary(page) => self.library_action(page, n, at),
            MenuKind::BrushRemove(page) => self.remove_action(page, n, at),
            MenuKind::BrushRename(page) => self.rename_action(page, n, at),
            _ => self.main_action(n, at),
        }
        self.ui_dirty = true;
    }

    fn main_action(&mut self, n: usize, at: (i32, i32)) {
        match n {
            0..=4 => self.choose_brush(SHAPES[n].1.clone()),
            row::CAPTURE => self.capture_brush_key(),
            row::OWN | row::SHAPE => {
                let paint = if n == row::OWN {
                    BrushPaint::OwnColors
                } else {
                    BrushPaint::Shape
                };
                self.editor.set_brush_paint(paint);
                self.full_canvas = true;
            }
            row::HANDLE => self.open_brush_kind(at, MenuKind::BrushHandle),
            row::RECENT => self.open_brush_kind(at, MenuKind::BrushRecent),
            row::LIBRARY => self.open_brush_kind(at, MenuKind::BrushLibrary(0)),
            row::SAVE => self.save_to_library(),
            row::REMOVE => self.open_brush_kind(at, MenuKind::BrushRemove(0)),
            row::RENAME => self.open_brush_kind(at, MenuKind::BrushRename(0)),
            row::LARGER => self.adjust_brush(1),
            row::SMALLER => self.adjust_brush(-1),
            _ => {}
        }
    }

    fn library_action(&mut self, page: usize, n: usize, at: (i32, i32)) {
        let index = page * PAGE + n;
        if n == PAGE {
            return self.open_brush_kind(at, MenuKind::BrushLibrary(page + 1));
        }
        let brush = self
            .brush_ui
            .store
            .brushes()
            .get(index)
            .and_then(from_library);
        match brush {
            Some(brush) => self.choose_brush(BrushChoice::Custom(Arc::new(brush))),
            None => self.notify_error("That brush could not be used".to_string()),
        }
    }

    /// The first choice marks the row; choosing it again removes the brush.
    fn remove_action(&mut self, page: usize, n: usize, at: (i32, i32)) {
        if n == PAGE {
            self.brush_ui.armed = None;
            return self.open_brush_kind(at, MenuKind::BrushRemove(page + 1));
        }
        let index = page * PAGE + n;
        if self.brush_ui.armed != Some(index) {
            self.brush_ui.armed = Some(index);
            return self.open_brush_kind(at, MenuKind::BrushRemove(page));
        }
        self.brush_ui.armed = None;
        match self.brush_ui.store.remove(index) {
            Ok(name) => self.notify(format!("Removed {name} from the library")),
            Err(why) => self.notify_error(why),
        }
    }

    /// Puts the brush in hand in the library.
    pub(crate) fn save_to_library(&mut self) {
        let Some(brush) = self.editor.brushes().active_custom().cloned() else {
            let key = self.keymap.chord(Action::CaptureBrush);
            return self.notify(format!("Capture a brush first: select pixels, then {key}"));
        };
        self.read_library();
        match self.brush_ui.store.add(to_library(&brush)) {
            Ok(Saved::Added(name)) => self.notify(format!("Saved {name} to the library")),
            Ok(Saved::AlreadyThere) => {
                self.notify("That brush is already in the library".to_string())
            }
            Err(why) => self.notify_error(why),
        }
    }
}

/// A paint-mode row: disabled without a custom brush, and marked when it is the one in use.
fn row_marked(label: &'static str, on: bool, has_custom: bool) -> (Cow<'static, str>, bool) {
    let (label, enabled) = marked(label, on);
    (label, enabled && has_custom)
}
