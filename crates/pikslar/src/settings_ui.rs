//! Settings in use: read at launch, written when one changes, and the Open menu's recent
//! files. Settings are view state, outside undo, and never part of a project.

use crate::app::{App, Gesture};
use crate::frame_menu::{MenuKind, OpenMenu};
use crate::loading::Then;
use crate::settings::{Settings, SettingsFile};
use crate::shortcuts::Action;
use pikslar_platform::Window;
use std::borrow::Cow;
use std::path::Path;

const NONE_YET: &str = "No recent files yet";
pub(crate) const PLACE_REFERENCE: &str = "Place a reference picture...";
/// The longest file name shown, in characters, before it is shortened.
const LONGEST_NAME: usize = 40;

/// The longest a folder is shown in a recent file's label.
const LONGEST_FOLDER: usize = 14;
const ELLIPSIS: &str = "...";

/// `text` cut to `max` characters by dropping its middle, so both ends show. Always marks the
/// cut with an ellipsis.
pub(crate) fn shorten_middle(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(ELLIPSIS.len());
    let (head, tail) = (keep - keep / 2, keep / 2);
    let mut out: String = chars[..head].iter().collect();
    out.push_str(ELLIPSIS);
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// A file name cut to `max` characters in the middle of its stem, so the extension and the
/// end of the name, where files tend to differ, both stay.
fn shorten_name(name: &str, max: usize) -> String {
    let dot = name.rfind('.').filter(|&at| at > 0);
    let (stem, ext) = dot.map_or((name, ""), |at| name.split_at(at));
    let room = max.saturating_sub(ext.chars().count());
    if name.chars().count() <= max || room <= ELLIPSIS.len() {
        return shorten_middle(name, max);
    }
    format!("{}{ext}", shorten_middle(stem, room))
}

/// How a recent file is named in the menu: its name, then the folder it is in.
pub(crate) fn recent_label(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy());
    let folder = path
        .parent()
        .and_then(Path::file_name)
        .map(|f| shorten_middle(&f.to_string_lossy(), LONGEST_FOLDER));
    match folder {
        Some(folder) => {
            let tail = format!("  in {folder}");
            let room = LONGEST_NAME.saturating_sub(tail.chars().count());
            format!("{}{tail}", shorten_name(&name, room))
        }
        None => shorten_name(&name, LONGEST_NAME),
    }
}

impl App {
    /// Reads the settings and uses them. Files opened at launch go first in the recent files.
    pub(crate) fn use_settings(&mut self, file: SettingsFile) {
        let (loaded, problem) = file.load();
        // A file that is there but could not be used is never written over: it is set aside
        // first, or, if even that fails, nothing is saved this time.
        let (file, problem) = match problem {
            Some(why) if file.set_aside() => (
                file,
                Some(format!("{why}. It was kept as settings.txt.bad")),
            ),
            Some(why) => (
                SettingsFile::disabled(),
                Some(format!("{why}. Changes are not kept this time")),
            ),
            None => (file, None),
        };
        self.interface_size = loaded.interface_size;
        self.apply_large_text_setting(loaded.large_text);
        Window::detached().set_interface_size(loaded.interface_size);
        self.workspaces = loaded.workspaces.clone();
        self.follow_workspace();
        self.comp.onion_style = loaded.onion;
        self.keymap = loaded.keys.clone();
        self.script_limits = loaded.script;
        self.diagnostics = !loaded.no_diagnostics;
        let launched = std::mem::replace(&mut self.recent, loaded.recent.clone());
        for path in launched.iter().rev() {
            crate::settings::remember(&mut self.recent, path);
        }
        // The brush library lives beside the settings and follows them; it is read on first use.
        self.brush_ui = crate::brush_ui::BrushUi::new(crate::brush_store::BrushStore::at(
            file.sibling(crate::brush_store::FILE_NAME),
        ));
        // Plugins live beside the settings too, and are off when settings are.
        self.plugins.store = pikslar_plugin::PluginStore::for_settings_dir(file.dir());
        self.settings_saved = loaded;
        self.settings_file = file;
        if let Some(problem) = problem {
            self.notify_error(format!(
                "Settings could not be read, so the defaults are used: {problem}"
            ));
        }
        self.ui_dirty = true;
    }

    pub(crate) fn settings_changed(&self) -> bool {
        let saved = &self.settings_saved;
        saved.interface_size != self.interface_size
            || saved.large_text != self.large_text_chosen.then_some(self.large_text)
            || saved.workspaces != self.workspaces
            || saved.onion != self.comp.onion_style
            || saved.recent != self.recent
            || saved.keys != self.keymap
            || saved.script != self.script_limits
            || saved.rewind != self.rewind.on
            || saved.no_diagnostics == self.diagnostics
    }

    pub(crate) fn current_settings(&self) -> Settings {
        Settings {
            interface_size: self.interface_size,
            large_text: self.large_text_chosen.then_some(self.large_text),
            onion: self.comp.onion_style,
            recent: self.recent.clone(),
            keys: self.keymap.clone(),
            script: self.script_limits,
            rewind: self.rewind.on,
            no_diagnostics: !self.diagnostics,
            workspaces: self.workspaces.clone(),
        }
    }

    /// Writes the settings when one has changed. A failure is said once and never retried
    /// for the same settings, so a read-only folder costs nothing per frame. While a pointer
    /// button is held (a splitter being dragged, say) nothing is written: the frame that
    /// handles the release writes where the drag ended.
    pub(crate) fn tick_settings(&mut self) {
        let held = self.ui.down || self.gesture != Gesture::None;
        if held || !self.settings_file.is_enabled() || !self.settings_changed() {
            return;
        }
        let now = self.current_settings();
        match self.settings_file.save(&now) {
            Ok(()) => self.settings_failed = false,
            Err(e) => {
                crate::diag::note(crate::diag::Event::new("settings-write", "fail").io(&e));
                if !std::mem::replace(&mut self.settings_failed, true) {
                    self.notify_error(format!("Settings could not be saved: {e}"));
                }
            }
        }
        self.settings_saved = now;
    }

    pub(crate) fn open_file_menu(&mut self, at: (i32, i32)) {
        self.menu = Some(OpenMenu {
            at,
            kind: MenuKind::File,
        });
        self.ui_dirty = true;
    }

    pub(crate) fn open_items(&self) -> Vec<(Cow<'static, str>, bool)> {
        let open = self.keymap.hint("Open a file...", Action::Open, "");
        let mut items = vec![(Cow::Owned(open), true)];
        if self.recent.is_empty() {
            items.push((Cow::Borrowed(NONE_YET), false));
        }
        items.extend(
            self.recent
                .iter()
                .map(|p| (Cow::Owned(recent_label(p)), true)),
        );
        items.push((Cow::Borrowed(PLACE_REFERENCE), true));
        items
    }

    /// Opens the first file dropped on the window: Pikslar holds one document at a time. An
    /// open dialog keeps its document, so a drop then only says why nothing happened.
    pub(crate) fn files_dropped(&mut self, paths: &[std::path::PathBuf], window: Window) {
        let Some(first) = paths.first() else {
            return;
        };
        self.close_menu();
        if self.panel_open() {
            self.notify("Close the dialog first to open a dropped file".to_string());
            return;
        }
        let then = match paths.len() {
            1 => Then::Nothing,
            _ => Then::SayOneAtATime,
        };
        self.begin_open(window, first.clone(), then);
    }

    /// True for the open menu's last item, which places a reference picture.
    pub(crate) fn is_place_reference_item(&self, n: usize) -> bool {
        n + 1 == self.open_items().len()
    }

    pub(crate) fn open_menu_action(&mut self, n: usize, window: Window) {
        if n == 0 {
            self.open(window);
            return;
        }
        if self.is_place_reference_item(n) {
            self.place_reference(window);
            return;
        }
        let Some(path) = self.recent.get(n - 1).cloned() else {
            return;
        };
        // A file that is gone leaves the list. One that is there but would not open stays, as
        // a share that is offline or a file another program holds may open later.
        self.begin_open(window, path, Then::ForgetIfGone);
    }
}

#[cfg(test)]
#[path = "settings_ui_tests.rs"]
mod tests;
