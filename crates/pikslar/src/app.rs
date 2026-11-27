//! Application state and the per-frame flow.

use crate::color_ui::{to_hsva, Hsva};
use crate::layout::Layout;
use crate::panel_lint::LintState;
use crate::recovery::Recovery;
use pikslar_core::{
    Clip, ColorMode, Command, Document, Edge, Editor, FloatingCmd, Outcome, StepCmd, Tool,
};
use pikslar_platform::{Button, Key, Mods, Window};
use pikslar_render::{Compositor, DragPreview, Rect, Surface, View};
use pikslar_ui::theme;
use pikslar_ui::Ui;
use std::path::PathBuf;
use std::time::Instant;

const DEFAULT_SIZE: usize = 64;

mod handler;
mod messages;
mod starter;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use starter::starter_document;
use starter::{add_starter_palette, first_swatch_color};
pub use types::{Gesture, NewDialog, ToolChoice};

type Deferred = Box<dyn FnOnce(&mut App)>;

pub struct App {
    pub editor: Editor,
    pub comp: Compositor,
    pub view: View,
    pub ui: Ui,
    pub layout: Layout,
    pub tool: ToolChoice,
    pub rhythm: bool,
    pub gesture: Gesture,
    /// The button that started the current gesture.
    gesture_button: Button,
    pub lasso: Vec<(i32, i32)>,
    pub drag_point: (i32, i32),
    pub clip: Option<Clip>,
    pub clip_stamp: u32,
    pub lint: LintState,
    pub recovery: Recovery,
    /// What the recovery copy on disk shows: document revision and floating stamp.
    pub recovered: (u64, u64, u64),
    pub recovery_due: Option<Instant>,
    pub pending_offer: bool,
    /// What panels asked for while drawing, done once the whole interface has been drawn.
    deferred: Vec<Deferred>,
    in_draw: bool,
    presented: bool,
    pub outline: Vec<Edge>,
    pub drawn_selection: u64,
    pub drawn_shape: u64,
    pub outline_origin: (i32, i32),
    pub preview: Option<DragPreview>,
    pub off_canvas: crate::cel_ui::OffCanvas,
    /// A size a large selection is being dragged to, applied on release.
    pub scale_target: Option<(i32, i32, i32, i32)>,
    pub scale_outline: Vec<Edge>,
    pub ant_phase: i32,
    pub next_ant_at: Instant,
    pub path: Option<PathBuf>,
    /// A file that holds data this version could not keep. Save never replaces it.
    pub(crate) protected_path: Option<PathBuf>,
    /// The Aseprite file this unsaved project was read from. It is never written: Save
    /// only uses it to suggest a `.piks` name beside it.
    pub(crate) imported_from: Option<PathBuf>,
    /// A file being read on another thread, to open once it is.
    pub(crate) loading: Option<crate::loading::Loading>,
    /// A read that was stopped and has not ended yet. The next read waits for it.
    pub(crate) stopping: Option<std::thread::JoinHandle<()>>,
    /// A file to open on this thread, none other starting, once the stopped read ends.
    pub(crate) open_after_stop: Option<(PathBuf, crate::loading::Then)>,
    /// The work as it was made safe for Windows ending the session, if it was.
    pub(crate) session_kept: Option<(u64, u64, u64)>,
    pub saved_revision: u64,
    pub ui_dirty: bool,
    pub full_canvas: bool,
    pub space_held: bool,
    /// The last key was this shortcut, so the character Windows sends after it is not typed.
    pub key_text_spent: Option<Key>,
    /// The key held down went down as a shortcut, so its repeats are shortcuts too.
    pub shortcut_held: bool,
    pub playing: bool,
    pub next_frame_at: Instant,
    pub timeline: crate::timeline::Timeline,
    pub hsva: Hsva,
    pub synced_color: u32,
    pub selected_swatch: Option<u32>,
    pub footprint: crate::footprint::Footprint,
    pub panel_scroll: crate::panel_fit::PanelScroll,
    /// The workspaces: the arrangement of the docks in use, and each preset's. Kept in the
    /// settings, never with the work.
    pub(crate) workspaces: crate::workspace::model::Workspaces,
    /// Tiles is the shown tab while a tilemap layer is active, unless another was chosen.
    pub(crate) tiles_shown: bool,
    /// The dock slots as their strips were last drawn, for a click on last frame's tabs.
    pub(crate) drawn_strips: crate::dock::DrawnStrips,
    /// A sprite made from nothing, whose author may be new here too.
    pub first_use: bool,
    pub live_palette: bool,
    /// The selected swatch's color when last looked at, to tell a pick from a recolor.
    pub swatch_seen: u32,
    pub new_dialog: Option<NewDialog>,
    /// The export panel, while it is open.
    pub export_dialog: Option<crate::export_dialog::ExportChoice>,
    pub(crate) keys_dialog: Option<crate::keys_dialog::KeysDialog>,
    pub(crate) canvas_dialog: Option<crate::canvas_dialog::CanvasDialog>,
    /// The layer whose name is being typed.
    pub(crate) layer_naming: Option<crate::layer_naming::LayerNaming>,
    /// The text being typed with the text tool, if a session is open.
    pub(crate) text: Option<crate::text_ui::TextSession>,
    /// How text is drawn, kept from one session to the next.
    pub(crate) text_style: crate::text_ui::TextStyle,
    /// The slice whose name is being typed.
    pub(crate) slice_naming: Option<crate::slice_ui::SliceNaming>,
    /// The palette section's view state: the swatch pair, the marked group, a name being typed.
    pub(crate) palette_ui: crate::panel_palette::PaletteUi,
    pub(crate) tile_ui: crate::panel_tiles::TileUi,
    /// The context bar's own settings and where it drew its menus' buttons.
    pub(crate) bar: crate::context_bar::ContextBar,
    /// Whether slices are drawn over the canvas. View state: never undone or saved with work.
    pub(crate) show_slices: bool,
    /// What the slice marks looked like when the canvas was last drawn.
    pub(crate) slices_drawn: u64,
    /// The layer row clicked last, and when, to tell a double click.
    pub(crate) last_row_click: Option<(usize, Instant)>,
    /// What the last export chose, for the next one to start from.
    pub export_choice: crate::export_dialog::ExportChoice,
    /// Where a test asked for the layout, and the layout last written there.
    pub(crate) layout_dump: Option<crate::session::LayoutDump>,
    pub(crate) memory_dump: Option<crate::memory_dump::MemoryDump>,
    /// A popup menu, drawn above everything and holding the pointer and keyboard.
    pub(crate) menu: Option<crate::frame_menu::OpenMenu>,
    /// Which row Shift F10 opens its menu beside.
    pub(crate) ctx: crate::context_key::ContextKeys,
    /// Where the onion skin button was last drawn, for its menu.
    pub(crate) onion_button: Rect,
    /// The frame at true size in a corner of the canvas.
    pub(crate) true_preview: crate::true_preview::TruePreview,
    /// Where the pointer rests, for its tooltip.
    pub(crate) rest: crate::tooltip::Rest,
    /// The cel matrix under the frame strip.
    pub(crate) matrix: crate::panel_matrix::Matrix,
    /// The keys held when the pointer last went down.
    pub(crate) press_mods: Mods,
    pub message: Option<(String, Instant)>,
    /// Whether `message` is an error, which stays until the next input.
    pub(crate) message_is_error: bool,
    /// News that came while an error was showing, said once the error is taken down.
    pub(crate) held_news: Option<String>,
    /// The work kept aside for Windows ending the session while the window is busy.
    pub(crate) standing: crate::standing::Kept,
    /// The row of the open menu the keyboard is on, with the menu it belongs to.
    pub(crate) menu_cursor: Option<(crate::frame_menu::OpenMenu, usize)>,
    /// What the pointer is resting on, or else what the keyboard focus is on, said in the
    /// status line.
    pub hint: Option<std::borrow::Cow<'static, str>>,
    /// The hint is the focused control's, not the pointer's, so no tooltip shows it at the
    /// pointer.
    pub(crate) hint_from_focus: bool,
    /// Where the focused control was drawn last pass, whose hint the status line says while the
    /// pointer rests on nothing that has one.
    pub(crate) focus_rect: Option<Rect>,
    pub changed: Vec<Rect>,
    pub xmap: Vec<i32>,
    pub(crate) size: (i32, i32),
    /// The interface size chosen in the View menu, or `None` to follow the display.
    pub(crate) interface_size: Option<i32>,
    /// *Large text*: interface words in the doubled Mono face (see `large_text.rs`).
    pub(crate) large_text: bool,
    /// The user has chosen Large text on or off; until then Windows' text size decides.
    pub(crate) large_text_chosen: bool,
    /// Windows' text size in percent (or the one forced), for the automatic Large text.
    pub(crate) text_percent: i32,
    /// Files opened or saved, newest first, offered in the Open menu.
    pub(crate) recent: Vec<PathBuf>,
    /// Which letter does what; the user can change it.
    pub(crate) keymap: crate::shortcuts::Keymap,
    pub(crate) settings_file: crate::settings::SettingsFile,
    /// What was last written, so a write happens only on a change.
    pub(crate) settings_saved: crate::settings::Settings,
    /// A failed write is said once, not on every change.
    pub(crate) settings_failed: bool,
    /// Local crash and saving diagnostics are kept (`docs/diagnostics.md`).
    pub(crate) diagnostics: bool,
    /// `--no-diagnostics`: none are kept this run, and the setting is left as it is.
    pub(crate) diagnostics_run_off: bool,
    /// What saving is doing, for the status bar.
    pub(crate) save: crate::save_state::SaveStatus,
    /// The save running on its thread, and the one queued after it.
    pub(crate) save_job: Option<crate::save_run::SaveJob>,
    pub(crate) save_next: Option<crate::save_run::FollowUp>,
    /// Counts documents replaced, so a save that ends after one is not taken as the new one's.
    pub(crate) documents: u64,
    /// How the document's own file looked when last read or written here (`outside.rs`).
    pub(crate) seen: Option<crate::outside::Seen>,
    /// Asks what to do about a file another program changed; a test answers it instead.
    pub(crate) ask_changed: fn(pikslar_platform::Window, &str) -> pikslar_platform::ChangedChoice,
    /// Offers a backup an interrupted save left (`orphan_backup.rs`); a test answers it instead.
    pub(crate) ask_backup: fn(pikslar_platform::Window, &str) -> Option<bool>,
    /// Asks, when a save would replace a backup nobody decided about, whether it may; a test
    /// answers it instead.
    pub(crate) ask_replace_backup: fn(pikslar_platform::Window, &str) -> Option<bool>,
    /// Asks before every file's kept undo history is forgotten; a test answers it instead.
    pub(crate) ask_forget_all: fn(pikslar_platform::Window) -> bool,
    /// Backups a save may replace: decided about, or left by this session's own save.
    pub(crate) backups_known: Vec<std::path::PathBuf>,
    /// How long the saving thread waits before it starts, for tests of a slow disk.
    pub(crate) save_delay: Option<std::time::Duration>,
    /// How long a save waits for the byte compare, and extra read time for slow-disk tests.
    pub(crate) compare: (std::time::Duration, Option<std::time::Duration>),
    /// What each script run is held to, from the settings.
    pub(crate) script_limits: pikslar_bind::ScriptLimits,
    /// The Scripts panel: what is loaded, what it printed, how its last run ended.
    pub(crate) scripts: crate::script_panel::ScriptPanel,
    /// Installed plugins, and one waiting for its review to be answered.
    pub(crate) plugins: crate::plugin_ui::PluginUi,
    /// Custom brushes: the library file (read on first use) and the menu state.
    pub(crate) brush_ui: crate::brush_ui::BrushUi,
    /// Undo history kept between sessions, when turned on.
    pub(crate) rewind: crate::rewind_link::RewindLink,
    pub(crate) needs_fit: bool,
    pub(crate) start_zoom: Option<i32>,
    /// The look a test fixed instead of following Windows high contrast.
    pub(crate) forced_look: Option<crate::look::ForcedLook>,
    pub(crate) key_cursor: crate::key_cursor::KeyCursor,
    /// The menu bar's titles and the canvas-only state.
    pub(crate) window_ui: crate::menu_bar::WindowUi,
    title: String,
    /// Whether Windows was last asked for a dark title bar.
    pub(crate) dark_title: Option<bool>,
}

impl App {
    pub fn launch(file: Option<PathBuf>, size: Option<usize>, zoom: Option<i32>) -> App {
        let requested = size.unwrap_or(DEFAULT_SIZE);
        let opened = file.as_deref().map(crate::files::open);
        let (mut protected, mut was_opened) = (None, false);
        let imported = file.as_deref().filter(|p| crate::files::is_aseprite(p));
        let imported = imported.map(|p| std::path::absolute(p).unwrap_or(p.to_path_buf()));
        let mut recent = Vec::new();
        let (doc, path, message) = match (opened, file) {
            (Some(Ok(opened)), path) => {
                // Kept whole, so a later run from another folder still finds it.
                recent.extend(
                    path.as_deref()
                        .map(|p| std::path::absolute(p).unwrap_or(p.to_path_buf())),
                );
                protected = path
                    .as_deref()
                    .and_then(|p| crate::files::protected_path(p, &opened.notes));
                let target = path.and_then(|p| crate::files::save_path_for(p, &opened.notes));
                let message = opened.notice();
                was_opened = true;
                (opened.doc, target, message)
            }
            (Some(Err(e)), _) => (starter_document(requested), None, Some(e.to_string())),
            _ => (starter_document(requested), None, None),
        };
        let mut app = App::with_document(doc, path);
        app.recent = recent;
        app.protected_path = protected;
        app.first_use &= !was_opened;
        if let Some(source) = imported.filter(|_| was_opened) {
            app.mark_imported(source);
        }
        app.start_zoom = zoom;
        if let Some(text) = message {
            app.notify(text);
        }
        app
    }

    fn with_document(mut doc: Document, path: Option<PathBuf>) -> App {
        let first_use = path.is_none();
        add_starter_palette(&mut doc);
        let comp = Compositor::new(doc.width, doc.height);
        let mut editor = Editor::new(doc);
        editor.mark_saved();
        let color = first_swatch_color(&editor);
        let mut app = App {
            comp,
            view: View {
                num: 1,
                den: 1,
                x: 0,
                y: 0,
            },
            ui: Ui::new(),
            layout: Layout::default(),
            tool: ToolChoice::Paint(Tool::Pencil),
            rhythm: false,
            gesture: Gesture::None,
            gesture_button: Button::Left,
            lasso: Vec::new(),
            drag_point: (0, 0),
            clip: None,
            clip_stamp: 0,
            lint: LintState::default(),
            recovery: Recovery::disabled(),
            recovered: (editor.revision(), 0, 0),
            recovery_due: None,
            pending_offer: false,
            deferred: Vec::new(),
            in_draw: false,
            presented: false,
            outline: Vec::new(),
            drawn_selection: editor.selection_version(),
            drawn_shape: editor.selection_shape_version(),
            outline_origin: (0, 0),
            preview: None,
            off_canvas: Default::default(),
            scale_target: None,
            scale_outline: Vec::new(),
            ant_phase: 0,
            next_ant_at: Instant::now(),
            path,
            protected_path: None,
            imported_from: None,
            loading: None,
            stopping: None,
            open_after_stop: None,
            session_kept: None,
            saved_revision: editor.revision(),
            ui_dirty: true,
            full_canvas: true,
            space_held: false,
            key_text_spent: None,
            shortcut_held: false,
            playing: false,
            timeline: Default::default(),
            next_frame_at: Instant::now(),
            hsva: to_hsva(color),
            synced_color: color,
            selected_swatch: None,
            footprint: Default::default(),
            panel_scroll: Default::default(),
            workspaces: Default::default(),
            tiles_shown: true,
            drawn_strips: Default::default(),
            interface_size: None,
            large_text: false,
            large_text_chosen: false,
            text_percent: 100,
            recent: Vec::new(),
            keymap: Default::default(),
            settings_file: Default::default(),
            settings_saved: Default::default(),
            settings_failed: false,
            diagnostics: true,
            diagnostics_run_off: false,
            save: Default::default(),
            save_job: None,
            save_next: None,
            documents: 0,
            seen: None,
            ask_changed: pikslar_platform::ask_changed_on_disk,
            ask_backup: pikslar_platform::ask_open_backup,
            ask_replace_backup: pikslar_platform::ask_replace_backup,
            ask_forget_all: pikslar_platform::ask_forget_all_history,
            backups_known: Vec::new(),
            save_delay: None,
            compare: (crate::outside::COMPARE_WAIT, None),
            script_limits: pikslar_bind::ScriptLimits::DEFAULT,
            scripts: Default::default(),
            plugins: Default::default(),
            brush_ui: Default::default(),
            rewind: Default::default(),
            first_use,
            live_palette: false,
            swatch_seen: 0,
            new_dialog: None,
            export_dialog: None,
            keys_dialog: None,
            canvas_dialog: None,
            layer_naming: None,
            text: None,
            text_style: Default::default(),
            slice_naming: None,
            palette_ui: Default::default(),
            tile_ui: Default::default(),
            bar: Default::default(),
            show_slices: true,
            slices_drawn: 0,
            last_row_click: None,
            export_choice: Default::default(),
            layout_dump: None,
            memory_dump: None,
            menu: None,
            ctx: crate::context_key::ContextKeys::default(),
            onion_button: Rect::new(0, 0, 0, 0),
            true_preview: Default::default(),
            rest: Default::default(),
            matrix: Default::default(),
            press_mods: Mods::default(),
            message: None,
            message_is_error: false,
            held_news: None,
            standing: crate::standing::register(),
            menu_cursor: None,
            hint: None,
            hint_from_focus: false,
            focus_rect: None,
            changed: Vec::with_capacity(64),
            xmap: Vec::new(),
            size: (0, 0),
            needs_fit: true,
            start_zoom: None,
            forced_look: None,
            key_cursor: Default::default(),
            window_ui: Default::default(),
            title: String::new(),
            dark_title: None,
            editor,
        };
        app.editor.color = color;
        app
    }

    /// Swaps in a different document, keeping the window and interface state.
    pub(crate) fn replace_document(&mut self, mut doc: Document, path: Option<PathBuf>) {
        // What the document being replaced saved is kept before it goes, if it can be soon.
        self.let_rewind_go();
        add_starter_palette(&mut doc);
        self.panel_scroll = Default::default();
        // The onion skin style is a setting, not part of the document, so it stays.
        let onion = self.comp.onion_style;
        self.comp = Compositor::new(doc.width, doc.height);
        self.comp.onion_style = onion;
        // Brushes outlive the document they were captured from.
        let brushes = self.editor.brushes().clone();
        self.editor = Editor::new(doc);
        // A script's step was in the history that just went.
        self.scripts.last_step = None;
        self.rewind.document_replaced();
        self.rewind.opened = None;
        self.editor.use_brushes(brushes);
        self.editor.color = first_swatch_color(&self.editor);
        self.path = path;
        self.imported_from = None;
        self.first_use = false;
        self.saved_revision = self.editor.revision();
        self.recovered = (self.editor.revision(), 0, 0);
        self.editor.mark_saved();
        self.save_forget();
        self.documents += 1;
        self.seen = None;
        self.save_next = None;
        self.recovery_due = None;
        self.recovery.clear();
        self.selected_swatch = None;
        self.playing = false;
        self.timeline = Default::default();
        // A name being typed belongs to the document it was started in.
        self.slice_naming = None;
        self.layer_naming = None;
        self.palette_ui = Default::default();
        self.tile_ui = Default::default();
        // Cels picked in the old document are not the new one's, whatever the counts say.
        self.matrix.selection = None;
        self.matrix.press = None;
        self.matrix.reset_view();
        self.gesture = Gesture::None;
        self.lasso.clear();
        // A new editor restarts the count, which may match the old one by chance.
        self.drawn_selection = u64::MAX;
        self.drawn_shape = u64::MAX;
        self.preview = None;
        self.scale_target = None;
        self.recovery.note_origin(self.path.as_deref());
        self.reset_lint();
        self.fit_view();
    }

    /// An imported file is not a project yet: it reads as unsaved until saved as one, so
    /// closing it asks first, as any unsaved work does.
    pub(crate) fn mark_imported(&mut self, source: PathBuf) {
        self.imported_from = Some(source);
        self.saved_revision = u64::MAX;
        self.editor.forget_saved();
    }

    /// Runs a command and shows any refusal. Returns what it did, if it was allowed.
    pub(crate) fn run(&mut self, command: impl Into<Command>) -> Option<Outcome> {
        debug_assert!(!self.in_draw, "a panel changed the document while drawing");
        self.ui_dirty = true;
        match self.editor.apply(command) {
            Ok(outcome) => {
                // A change of frames clips the selection now, so what is shown is what is used.
                self.timeline.fit_to(self.editor.doc().frames.len());
                Some(outcome)
            }
            Err(e) => {
                self.notify_error(e.to_string());
                None
            }
        }
    }

    /// Applies a command from pointer movement, which only changes the canvas.
    pub(crate) fn run_on_canvas(&mut self, command: impl Into<Command>) {
        debug_assert!(!self.in_draw, "a panel changed the document while drawing");
        if let Err(e) = self.editor.apply(command) {
            self.notify(e.to_string());
        }
    }

    /// True while what is shown differs from what was saved, floating pixels that moved or
    /// changed included, though they are not placed yet. Saving places them first.
    ///
    /// Unchanged is told two ways, and either is enough: the revision has not moved since the
    /// save, or undo and redo (which move it) have brought the history back to the place the
    /// file was saved at. A document never saved, or recovered, has neither: `saved_revision`
    /// is then `u64::MAX` and the history keeps no saved place.
    pub fn is_modified(&self) -> bool {
        let unchanged = self.editor.revision() == self.saved_revision || self.editor.is_at_saved();
        !unchanged || self.editor.floating_stamp() != 0
    }

    fn file_name(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || "Untitled".to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    pub fn document_label(&self) -> String {
        format!(
            "{}{}",
            self.file_name(),
            if self.is_modified() { " *" } else { "" }
        )
    }

    /// Ends any drag and places floating pixels, before the context changes.
    /// Applies floating pixels. False if they could not be, and so must stay where they are.
    pub fn settle(&mut self) -> bool {
        self.finish_gesture();
        self.run(FloatingCmd::Anchor);
        !self.editor.is_floating()
    }

    pub fn set_tool(&mut self, tool: ToolChoice) {
        if tool != ToolChoice::Text {
            self.commit_text();
        }
        if matches!(tool, ToolChoice::Paint(_)) {
            self.settle();
        } else {
            self.finish_gesture();
        }
        self.tool = tool;
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    pub fn toggle_onion(&mut self) {
        self.comp.onion = !self.comp.onion;
        self.editor.damage_all();
        self.ui_dirty = true;
    }

    pub fn undo(&mut self) {
        self.finish_gesture();
        self.rewind_before_undo();
        self.playing = false;
        let revision = self.editor.revision();
        let layer = self.editor.layer;
        self.run(StepCmd::Undo);
        self.script_step_undone(revision);
        self.after_history(layer);
    }

    pub fn redo(&mut self) {
        self.finish_gesture();
        self.playing = false;
        let layer = self.editor.layer;
        self.run(StepCmd::Redo);
        self.after_history(layer);
    }

    /// Undo goes to what it changed, which cannot be seen if that layer is hidden.
    pub(crate) fn after_history(&mut self, layer_before: usize) {
        self.timeline.fit_to(self.editor.doc().frames.len());
        let layer = self.editor.layer;
        let shown = self.editor.doc().shown_layers();
        if layer != layer_before && !shown.get(layer).copied().unwrap_or(true) {
            self.notify("That change is on a hidden layer".to_string());
        }
        self.ui_dirty = true;
    }

    pub fn open_new_dialog(&mut self) {
        self.ui.focus.clear();
        self.new_dialog = Some(NewDialog {
            size: DEFAULT_SIZE,
            mode: ColorMode::Palette,
        });
        self.ui_dirty = true;
    }

    /// A panel over the canvas has the pointer and the keyboard.
    /// A dialog, panel or menu has the keyboard (`modal.rs`).
    pub fn panel_open(&self) -> bool {
        self.modal().is_some()
    }

    pub fn close_new_dialog(&mut self) {
        self.ui.focus.clear();
        self.new_dialog = None;
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    pub fn adjust_brush(&mut self, delta: i32) {
        self.editor.brush_size = (self.editor.brush_size + delta).clamp(1, 64);
        self.ui_dirty = true;
    }

    pub(crate) fn pointer_down(&mut self, x: i32, y: i32, button: Button, mods: Mods) {
        self.dismiss_error();
        self.rest_interrupted();
        // One gesture at a time: a second button must not disturb a stroke in progress.
        if self.gesture != Gesture::None || self.ui.down {
            return;
        }
        if self.menu.is_some() && button != Button::Left {
            self.close_menu();
            return;
        }
        self.note_context_press(x, y);
        self.ui.focus.clear();
        self.finish_naming_unless_on_tag(x, y);
        self.finish_layer_naming_unless_on_row(x, y);
        self.text_press_elsewhere(x, y);
        self.finish_slice_naming();
        self.finish_group_naming();
        self.finish_tileset_naming();
        self.finish_brush_naming();
        let right = button == Button::Right && !self.panel_open();
        let menus = |a: &mut Self| {
            a.open_frame_menu(x, y)
                || a.open_onion_menu(x, y)
                || a.open_tag_menu(x, y)
                || a.open_rate_menu(x, y)
                || a.open_slice_menu(x, y, mods)
                || a.open_palette_menu(x, y)
                || a.open_tile_menus(x, y)
                || a.open_tab_menu(x, y)
        };
        if right && menus(self) {
            return;
        }
        self.ui.shift = mods.shift;
        self.press_mods = mods;
        self.ui.track(x, y);
        let on_canvas = self.on_canvas(x, y) && !self.panel_open();
        if on_canvas && self.ui.widget_at(x, y) == 0 {
            self.gesture_button = button;
            self.canvas_down(x, y, button, mods);
        } else if button == Button::Left {
            self.ui.down = true;
            self.ui.pressed = true;
            self.ui_dirty = true;
        }
    }

    pub(crate) fn pointer_up(&mut self, x: i32, y: i32, button: Button) {
        self.ui.mouse = (x, y);
        if self.gesture != Gesture::None {
            if button == self.gesture_button {
                self.finish_gesture();
            }
        } else if button == Button::Left && self.ui.down {
            self.ui.down = false;
            self.ui.released = true;
            self.ui_dirty = true;
            self.end_dragged_value();
        }
    }

    /// A slider let go is finished, even if the next change is to the same value.
    fn end_dragged_value(&mut self) {
        if self.timeline.naming.is_none() {
            self.run(StepCmd::EndDrag);
        }
    }

    /// A normal release also reports lost capture, so only a held pointer is cancelled.
    fn capture_lost(&mut self) {
        self.finish_gesture();
        self.space_held = false;
        if self.ui.down {
            self.ui.down = false;
            self.ui.active = 0;
            self.ui_dirty = true;
            self.end_dragged_value();
        }
    }

    fn expire_message(&mut self) {
        self.expire_message_at(Instant::now());
    }

    pub(crate) fn expire_message_at(&mut self, now: Instant) {
        if !self.message_is_error
            && self
                .message
                .as_ref()
                .is_some_and(|(_, until)| now >= *until)
        {
            self.clear_message();
        }
    }

    fn update_title(&mut self, window: Window) {
        let title = format!("{} - Pikslar", self.document_label());
        if title != self.title {
            window.set_title(&title);
            self.title = title;
        }
    }

    #[cfg(test)]
    pub(crate) fn draw_for_test(&mut self, surface: &mut Surface<'_>) {
        self.ui.begin();
        self.in_draw = true;
        self.cover_menu();
        self.draw_area(surface, crate::workspace::model::Side::Bottom);
        self.draw_menu(surface, Window::detached(), &mut Vec::new());
        self.draw_brush_naming(surface, &mut Vec::new());
        self.ui.end();
        self.in_draw = false;
        self.do_deferred();
    }

    pub(crate) fn draw_interface(
        &mut self,
        surface: &mut Surface<'_>,
        window: Window,
        dirty: &mut Vec<Rect>,
    ) {
        // The docks show what the active layer wants (Tiles on a tilemap layer).
        self.sync_matrix_layout();
        let focused_was = self.focus_before_draw();
        self.focus_rect = focused_was;
        self.ui.begin();
        self.name_canvas();
        self.in_draw = true;
        self.hint = None;
        self.hint_from_focus = false;
        self.cover_menu();
        for rect in self.layout.chrome() {
            surface.fill(rect, theme::panel());
        }
        // The menu bar is a Header band, like the tab strips and the status bar.
        surface.fill(self.layout.top, theme::header());
        // With no dialog or menu over them, Tab walks the panels' controls in the order drawn.
        let panels_keys = !self.panel_open() && self.menu.is_none();
        self.ui.focus.collect(panels_keys);
        self.draw_top(surface);
        self.draw_side(surface);
        self.draw_docks(surface);
        self.ui.focus.collect(false);
        if panels_keys {
            self.ui.focus_ring_inside(surface);
        }
        self.draw_new_dialog(surface, window, dirty);
        self.draw_export_dialog(surface, window, dirty);
        self.draw_keys_dialog(surface, dirty);
        self.draw_canvas_dialog(surface, dirty);
        self.draw_script_panel(surface, window, dirty);
        self.draw_plugin_review(surface, dirty);
        self.draw_menu(surface, window, dirty);
        self.draw_brush_naming(surface, dirty);
        self.draw_tooltip(surface, dirty);
        self.ui.end();
        self.refocus_after_draw(focused_was);
        self.in_draw = false;
        self.do_deferred();
        dirty.extend(self.layout.chrome());
        self.write_layout();
        self.write_memory();
    }

    /// Holds a panel's action until drawing ends, so what panels read cannot change under them.
    /// This allocates once per click or dragged frame, never during ordinary drawing.
    pub(crate) fn defer(&mut self, action: impl FnOnce(&mut App) + 'static) {
        self.deferred.push(Box::new(action));
    }

    fn do_deferred(&mut self) {
        let mut actions = std::mem::take(&mut self.deferred);
        while !actions.is_empty() {
            for action in actions.drain(..) {
                action(self);
            }
            // Anything those actions queued runs now too, in the order it was asked for.
            std::mem::swap(&mut actions, &mut self.deferred);
        }
        self.deferred = actions;
    }
}
