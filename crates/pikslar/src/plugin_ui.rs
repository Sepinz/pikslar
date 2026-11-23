//! Plugins in the interface: the Scripts menu, installing after a review, running a command
//! with what was approved, and uninstalling. The store lives beside the settings
//! (`<settings>/plugins`); with settings off, plugins are off.
//!
//! A plugin's commands are offered only in a menu of its own, reached by an item that starts
//! with "Plugin:", so no command can pass for one of Pikslar's own. Each run loads the plugin
//! again, so files changed since the install stop it from running, with the reason said.

use crate::app::App;
use crate::frame_menu::{Entries, MenuKind, OpenMenu};
use crate::script_panel::{clipped, PluginCommand, RunState, Runnable};
use pikslar_platform::{open_file_dialog, Key, Window};
use pikslar_plugin::{Listed, PendingInstall, PluginStore};
use std::borrow::Cow;
use std::path::Path;

const PLUGIN_FILTER: &str = "Plugin manifest (plugin.json)\0plugin.json\0";
/// The most characters of a stray folder's name, or of why a plugin is disabled, in a menu.
const MOST_ID_CHARS: usize = 64;
const MOST_REASON_CHARS: usize = 160;

/// A folder name from the store as a menu may show it. A plugin id is plain ASCII, but a
/// folder anything else made can hold any text, so it is cut short and its hidden characters
/// are taken out.
fn shown_id(id: &str) -> String {
    clipped(id, MOST_ID_CHARS)
}
/// The Scripts menu's own items, before the plugins.
const OWN_ITEMS: usize = 2;
/// The lines at the top of an installed plugin's menu that say who it is: name, version and
/// id; the author, marked unverified; the content fingerprint.
const IDENTITY_ITEMS: usize = 3;

#[derive(Debug, Default)]
pub(crate) struct PluginUi {
    pub store: PluginStore,
    /// The plugins as last listed: when the Scripts menu opened, or after a change.
    pub listed: Vec<Listed>,
    /// A plugin reviewed and waiting for Install or Do not install.
    pub review: Option<PendingInstall>,
}

impl App {
    pub(crate) fn open_scripts_menu(&mut self, at: (i32, i32)) {
        self.plugins.listed = self.plugins.store.list();
        self.menu = Some(OpenMenu {
            at,
            kind: MenuKind::Scripts,
        });
        self.ui_dirty = true;
    }

    pub(crate) fn scripts_entries(&self) -> Entries {
        let mut items: Entries = vec![
            (Cow::Borrowed("Script panel..."), true),
            (Cow::Borrowed("Install plugin..."), true),
        ];
        items.extend(self.plugins.listed.iter().map(|l| {
            let label = match &l.state {
                Ok(p) => format!("Plugin: {}", p.manifest().name),
                Err(_) => format!("Plugin: {} (disabled)", shown_id(&l.id)),
            };
            (Cow::Owned(label), true)
        }));
        items
    }

    /// The menu of one plugin: who it is, its commands, and Uninstall; or why it is disabled.
    pub(crate) fn plugin_entries(&self, n: usize) -> Entries {
        let Some(listed) = self.plugins.listed.get(n) else {
            return Vec::new();
        };
        let mut items: Entries = Vec::new();
        match &listed.state {
            Ok(p) => {
                let m = p.manifest();
                items.push((format!("{} {} ({})", m.name, m.version, m.id).into(), false));
                let author = pikslar_plugin::author_line(m.author.as_deref());
                items.push((author.into(), false));
                let print = format!("Fingerprint: {}", p.fingerprint());
                items.push((print.into(), false));
                items.extend(
                    m.commands
                        .iter()
                        .map(|c| (Cow::Owned(c.title.clone()), true)),
                );
                items.push((format!("Uninstall {}", m.name).into(), true));
            }
            Err(why) => {
                let why = clipped(&why.to_string(), MOST_REASON_CHARS);
                items.push((format!("Disabled: {why}").into(), false));
                items.push((format!("Uninstall {}", shown_id(&listed.id)).into(), true));
            }
        }
        items
    }

    pub(crate) fn scripts_menu_action(&mut self, n: usize, at: (i32, i32), window: Window) {
        match n {
            0 => self.open_script_panel(),
            1 => self.install_plugin(window),
            _ => {
                self.menu = Some(OpenMenu {
                    at,
                    kind: MenuKind::Plugin(n - OWN_ITEMS),
                });
            }
        }
        self.ui_dirty = true;
    }

    pub(crate) fn plugin_menu_action(&mut self, plugin: usize, n: usize) {
        let Some(listed) = self.plugins.listed.get(plugin) else {
            return;
        };
        let id = listed.id.clone();
        // The command as the menu showed it, named by its id, not by its place.
        let commands: Vec<String> = listed.state.as_ref().map_or(Vec::new(), |p| {
            p.manifest().commands.iter().map(|c| c.id.clone()).collect()
        });
        // The lines naming the plugin (or saying why it is disabled) come first.
        let header = match listed.state {
            Ok(_) => IDENTITY_ITEMS,
            Err(_) => 1,
        };
        match n.checked_sub(header) {
            None => {}
            Some(c) if c < commands.len() => self.run_plugin_command(&id, &commands[c]),
            Some(_) => self.uninstall_plugin(&id),
        }
        self.ui_dirty = true;
    }

    /// Asks for a plugin's `plugin.json`, then reviews the folder it is in.
    fn install_plugin(&mut self, window: Window) {
        if let Some(manifest) = open_file_dialog(window, PLUGIN_FILTER) {
            if let Some(folder) = manifest.parent() {
                self.review_plugin(folder);
            }
        }
    }

    /// Shows what installing the folder would mean. Nothing is written yet.
    pub(crate) fn review_plugin(&mut self, folder: &Path) {
        self.ui.focus.clear();
        match self.plugins.store.review(folder) {
            Ok(pending) => self.plugins.review = Some(pending),
            Err(e) => self.notify_error(format!("Nothing was installed: {e}")),
        }
        self.ui_dirty = true;
    }

    pub(crate) fn approve_plugin(&mut self) {
        self.ui.focus.clear();
        let Some(pending) = self.plugins.review.take() else {
            return;
        };
        let review = pending.review();
        let (named, id) = (
            format!("{} {}", review.name, review.version),
            review.id.clone(),
        );
        match self.plugins.store.approve(pending) {
            Ok(()) => self.notify(format!("Installed {named}. Its commands are in Scripts")),
            Err(e) => self.notify_error(format!("{named} was not installed: {e}")),
        }
        self.forget_loaded_plugin(&id);
        self.plugins.listed = self.plugins.store.list();
        self.close_plugin_review();
    }

    pub(crate) fn deny_plugin(&mut self) {
        self.ui.focus.clear();
        if let Some(pending) = self.plugins.review.take() {
            pending.deny();
            self.notify("Nothing was installed".to_string());
        }
        self.close_plugin_review();
    }

    fn close_plugin_review(&mut self) {
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    /// Tab moves between Do not install (first) and Install, and Space acts on the one that has
    /// focus. Enter acts only on Do not install: approving a plugin is never the default. Returns whether it was used.
    pub(crate) fn plugin_review_focus_key(
        &mut self,
        key: Key,
        mods: pikslar_platform::Mods,
    ) -> bool {
        // Enter acts only on Do not install: it never installs.
        let on_decline = self.ui.focus.focused() == Some(crate::script_draw::ID_SCRIPTS + 11);
        match key {
            Key::Tab | Key::Space => self.focus_key(key, mods),
            Key::Enter if on_decline => self.focus_key(key, mods),
            _ => false,
        }
    }

    pub(crate) fn plugin_review_key(&mut self, key: Key) {
        if key == Key::Escape {
            self.deny_plugin();
        }
    }

    /// Runs the command `command` of plugin `id` with exactly what the user approved.
    pub(crate) fn run_plugin_command(&mut self, id: &str, command: &str) {
        match self.plugin_runnable(id, command) {
            Ok(run) => self.start_script(Some(run)),
            Err(why) => {
                self.plugins.listed = self.plugins.store.list();
                self.notify(why);
            }
        }
    }

    /// Loads the plugin again, so files changed since it was listed or last run stop it, and
    /// finds its command by id. The reason, in the user's words, when it cannot run.
    pub(crate) fn plugin_runnable(&self, id: &str, command: &str) -> Result<Runnable, String> {
        let shown = shown_id(id);
        let plugin = self
            .plugins
            .store
            .load(id)
            .map_err(|why| format!("{shown} cannot run: {why}"))?;
        let m = plugin.manifest();
        let found = m.commands.iter().find(|c| c.id == command);
        let (Some(c), Some(source)) = (found, plugin.script_for(command)) else {
            return Err(format!("{} no longer has that command", m.name));
        };
        Ok(Runnable {
            name: format!("{}: {}", m.name, c.title),
            source: source.to_string(),
            caps: plugin.granted().into(),
            plugin: Some(PluginCommand {
                id: id.to_string(),
                command: command.to_string(),
                name: m.name.clone(),
                granted: plugin.granted(),
            }),
        })
    }

    pub(crate) fn uninstall_plugin(&mut self, id: &str) {
        match self.plugins.store.uninstall(id) {
            Ok(()) => self.notify(format!("Uninstalled {}", shown_id(id))),
            Err(e) => self.notify_error(format!("{} was not uninstalled: {e}", shown_id(id))),
        }
        self.forget_loaded_plugin(id);
        self.plugins.listed = self.plugins.store.list();
    }

    /// A command of plugin `id` that is loaded in the Scripts panel is dropped: what it was
    /// loaded from was replaced or removed.
    fn forget_loaded_plugin(&mut self, id: &str) {
        let loaded = self.scripts.loaded.as_ref().and_then(|r| r.plugin.as_ref());
        if loaded.is_some_and(|p| p.id == id) && self.scripts.state == RunState::Idle {
            self.scripts.loaded = None;
        }
    }
}

#[cfg(test)]
#[path = "plugin_ui_tests.rs"]
mod tests;
