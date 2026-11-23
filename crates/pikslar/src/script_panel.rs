//! The Scripts panel: open a `.pks` file, run it, stop it, read what it printed and why it
//! failed. A run that succeeds is one undo step named after the script; one that fails or is
//! stopped changes nothing at all. Plugin commands run here too, with only what the plugin
//! was granted.
//!
//! A run is started on the frame after Run is pressed, so the panel already says it is
//! running. The window then waits for it while still answering: a press on Stop, or Esc,
//! cancels it (`pikslar_platform::wait_answering`). Other input meanwhile is dropped.

use crate::app::App;
use crate::script_files::{BridgePicker, PickBridge, SystemDisk};
use pikslar_api::ApiError;
use pikslar_bind::{run_script_with, Capabilities, Failure, FileAccess, Outcome};
use pikslar_core::LimitError;
use pikslar_platform::{
    open_file_dialog, open_file_dialog_titled, save_file_dialog_titled, wait_answering, Button,
    Event, Key, Window,
};
use pikslar_plugin::files::FileError;
use pikslar_plugin::hidden::shown;
use pikslar_plugin::CapabilitySet;
use pikslar_render::Rect;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Lines of printed output the panel keeps; older ones scroll away.
pub(crate) const MOST_OUTPUT_LINES: usize = 200;
/// The most characters of one output line, or of the status, that are kept.
pub(crate) const MOST_LINE_CHARS: usize = 1024;
const SCRIPT_FILTER: &str = "Pikslar scripts\0*.pks\0";

/// What can be run: a script file, or a plugin's command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Runnable {
    /// The file's name, or "Plugin: Command".
    pub name: String,
    pub source: String,
    pub caps: Capabilities,
    /// For a plugin's command: which one. Every run loads the plugin again by this, so its
    /// source and what it was granted are always what the store holds now.
    pub plugin: Option<PluginCommand>,
}

/// A plugin's command, named by ids that stay the same when the plugin is reinstalled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginCommand {
    pub id: String,
    pub command: String,
    /// The plugin's name and what it was granted when it was last loaded.
    pub name: String,
    pub granted: CapabilitySet,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RunState {
    #[default]
    Idle,
    /// Run was pressed; the run starts on the next frame.
    Starting,
}

/// How the last run ended, as the panel says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Status {
    pub text: String,
    pub failed: bool,
}

/// Where the history stood right after a run recorded its step: the revision, the count of
/// steps ever pushed, and the script's name. An undo names the script only while both match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScriptStep {
    pub revision: u64,
    pub pushes: u64,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ScriptPanel {
    pub open: bool,
    pub loaded: Option<Runnable>,
    pub state: RunState,
    pub output: Vec<String>,
    pub status: Option<Status>,
    /// Where Stop was drawn, for a press while the run goes on.
    pub stop_at: Rect,
    /// The step a script's run recorded, so its undo can say so.
    pub last_step: Option<ScriptStep>,
    /// Where a run asks for an open dialog, which the window shows.
    pub bridge: Arc<PickBridge>,
}

impl App {
    pub(crate) fn open_script_panel(&mut self) {
        self.close_menu();
        self.finish_naming();
        self.scripts.open = true;
        self.ui_dirty = true;
    }

    pub(crate) fn close_script_panel(&mut self) {
        if self.scripts.state == RunState::Idle {
            self.scripts.open = false;
            self.full_canvas = true;
            self.ui_dirty = true;
        }
    }

    /// Asks for a `.pks` file and loads it, ready to run with every capability.
    pub(crate) fn choose_script(&mut self, window: Window) {
        if let Some(path) = open_file_dialog(window, SCRIPT_FILTER) {
            self.load_script_file(&path);
        }
    }

    pub(crate) fn load_script_file(&mut self, path: &Path) {
        let name = crate::scripting::file_name(path);
        match crate::scripting::read_source(path) {
            Ok(source) => {
                self.scripts.loaded = Some(Runnable {
                    name: name.clone(),
                    source,
                    caps: Capabilities::ALL,
                    plugin: None,
                });
                self.set_status(format!("{name} is ready to run"), false);
            }
            Err(why) => self.set_status(format!("{name} could not be read: {why}"), true),
        }
    }

    /// Runs what is loaded on the next frame, with the panel open to show it.
    pub(crate) fn start_script(&mut self, runnable: Option<Runnable>) {
        // A run already asked for keeps what it was asked to run.
        if self.scripts.state != RunState::Idle {
            return;
        }
        if let Some(r) = runnable {
            self.scripts.loaded = Some(r);
        }
        if self.scripts.loaded.is_none() {
            return;
        }
        self.scripts.open = true;
        self.scripts.state = RunState::Starting;
        let name = self.scripts.loaded.as_ref().map(|r| r.name.clone());
        self.set_status(format!("{} is running", name.unwrap_or_default()), false);
    }

    pub(crate) fn clear_script_output(&mut self) {
        self.scripts.output.clear();
        self.scripts.status = None;
        self.ui_dirty = true;
    }

    fn set_status(&mut self, text: String, failed: bool) {
        let text = clipped(&text, MOST_LINE_CHARS);
        self.scripts.status = Some(Status { text, failed });
        self.ui_dirty = true;
    }

    /// From the frame: a run that was asked for runs now, while the window keeps answering.
    pub(crate) fn run_started_script(&mut self, window: Window) {
        let stop = self.scripts.stop_at;
        let bridge = self.scripts.bridge.clone();
        self.run_started_with(|finished, cancel| {
            wait_answering(
                window,
                || {
                    bridge.answer(|q| match q.save {
                        true => save_file_dialog_titled(
                            window,
                            &q.filter,
                            &q.title,
                            q.extension.as_deref(),
                        ),
                        false => open_file_dialog_titled(window, &q.filter, &q.title),
                    });
                    !finished()
                },
                |e| {
                    if stops(&e, stop) {
                        cancel.store(true, Ordering::Relaxed);
                    }
                },
            );
        });
    }

    /// Runs the started script, with `wait` waiting for it and setting the flag to cancel.
    pub(crate) fn run_started_with(
        &mut self,
        mut wait: impl FnMut(&dyn Fn() -> bool, &AtomicBool),
    ) {
        if self.scripts.state != RunState::Starting {
            return;
        }
        self.scripts.state = RunState::Idle;
        let Some(mut run) = self.scripts.loaded.clone() else {
            return;
        };
        if let Some(command) = &run.plugin {
            match self.plugin_runnable(&command.id, &command.command) {
                Ok(fresh) => run = fresh,
                Err(why) => return self.set_status(why, true),
            }
        }
        let files = run.plugin.as_ref().map(|p| {
            let picker = BridgePicker::new(self.scripts.bridge.clone(), &p.name, &p.id);
            FileAccess::new(&p.name, p.granted, Box::new(picker))
                .with_disk(Box::new(SystemDisk))
                .with_protected(self.own_folders())
        });
        let bridge = self.scripts.bridge.clone();
        bridge.open();
        let pushes = self.editor.history().pushes();
        let out = self.run_script_waiting(&run.source, run.caps, files, |finished, cancel| {
            wait(finished, cancel);
            // The wait ended with the run still going: the window is closing or the app
            // quits. The run is cancelled, and a dialog it waits for will never be shown.
            if !finished() {
                cancel.store(true, Ordering::Relaxed);
                bridge.close();
            }
        });
        self.script_finished(&run.name, out, pushes);
    }

    /// `pushes` is the count of history steps before the run, so a run that recorded none
    /// does not claim one.
    fn script_finished(&mut self, name: &str, out: Outcome, pushes: u64) {
        let printed = out.printed.lines().map(|l| clipped(l, MOST_LINE_CHARS));
        self.scripts.output.extend(printed);
        let extra = self.scripts.output.len().saturating_sub(MOST_OUTPUT_LINES);
        self.scripts.output.drain(..extra);
        match out.result {
            Ok(_) if self.editor.history().pushes() > pushes => {
                self.scripts.last_step = Some(ScriptStep {
                    revision: self.editor.revision(),
                    pushes: self.editor.history().pushes(),
                    name: name.to_string(),
                });
                self.set_status(format!("{name} ran: one step to undo"), false);
            }
            Ok(_) => self.set_status(format!("{name} ran: nothing to undo"), false),
            Err(failure) => {
                let mut text = failure_text(name, &failure);
                if out.files_written {
                    text.push_str(". The file it saved was written all the same");
                }
                self.set_status(text, true)
            }
        }
    }

    /// The folders no plugin's file may be written in or under: the settings folder in use,
    /// the plugins folder, and the usual per-user folder (which holds crash recovery).
    pub(crate) fn own_folders(&self) -> Vec<PathBuf> {
        let settings = self.settings_file.dir().map(Path::to_path_buf);
        let plugins = self.plugins.store.folder().map(Path::to_path_buf);
        let usual = crate::settings::SettingsFile::default_dir();
        [settings, plugins, usual].into_iter().flatten().collect()
    }

    /// Undo says when it took back a script's step.
    pub(crate) fn script_step_undone(&mut self, revision_before: u64) {
        let Some(step) = self.scripts.last_step.take() else {
            return;
        };
        // Undo pushes nothing, so the count is the same as before it.
        let now = (revision_before, self.editor.history().pushes());
        if now == (step.revision, step.pushes) {
            self.notify(format!("Undid script {}", step.name));
        } else {
            self.scripts.last_step = Some(step);
        }
    }

    /// Esc closes the panel; a key goes nowhere else while it is open.
    pub(crate) fn script_panel_key(&mut self, key: Key) {
        if key == Key::Escape {
            self.close_script_panel();
        }
    }
}

/// Whether `e` asks a running script to stop: Esc, or a press on Stop.
fn stops(e: &Event, stop: Rect) -> bool {
    match *e {
        Event::KeyDown {
            key: Key::Escape, ..
        } => true,
        Event::Down {
            x,
            y,
            button: Button::Left,
            ..
        } => stop.contains(x, y),
        _ => false,
    }
}

/// `text`, cut to `most` characters with "..." after it when it was longer, and with what
/// would hide or reorder it taken out ([`pikslar_plugin::hidden`]). What a script
/// printed or failed with can be megabytes; no more than this is ever kept to be drawn.
pub(crate) fn clipped(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((at, _)) => format!("{}...", shown(&text[..at])),
        None => shown(text),
    }
}

/// Why a run kept nothing, in the panel's words: a place only when there is one.
pub(crate) fn failure_text(name: &str, failure: &Failure) -> String {
    if failure.is_cancel() {
        return format!("{name} was stopped. Nothing changed");
    }
    match failure {
        Failure::Refused {
            error: error @ ApiError::Refused(LimitError::Busy),
            ..
        } => format!("{name} did not run. {error}"),
        Failure::Script(e) => format!("{name} changed nothing. Error at {e}"),
        // The failure's own text names the line and column, and says when the refused
        // write was pixels `set_pixel` had kept waiting.
        Failure::Refused { at: Some(_), .. } => {
            format!("{name} changed nothing. Error at {failure}")
        }
        Failure::Refused {
            error,
            at: None,
            waiting: true,
        } => {
            format!("{name} changed nothing. The pixels it left to write were refused: {error}")
        }
        Failure::Refused {
            error,
            at: None,
            waiting: false,
        } => format!("{name} changed nothing: {error}"),
        Failure::Thread(why) => format!("{name} could not start: {why}"),
        Failure::Files(why @ FileError::NotPutBack) => {
            format!("{name} did not change the document, but {why}")
        }
        Failure::Files(why) => {
            format!("{name} changed nothing. The file it was saving was not written: {why}")
        }
    }
}

/// For [`App::run_script_waiting`]: runs `source` on a thread while `wait` waits for it.
pub(crate) fn run_beside(
    editor: &mut pikslar_core::Editor,
    (source, caps): (&str, Capabilities),
    limits: pikslar_bind::ScriptLimits,
    files: Option<FileAccess>,
    mut wait: impl FnMut(&dyn Fn() -> bool, &AtomicBool),
) -> Outcome {
    let cancel = AtomicBool::new(false);
    std::thread::scope(|s| {
        let thread = std::thread::Builder::new().name("pikslar script host".into());
        let cancel = &cancel;
        match thread.spawn_scoped(s, move || {
            run_script_with(editor, source, caps, limits, cancel, files)
        }) {
            Ok(run) => {
                wait(&|| run.is_finished(), cancel);
                // Whatever made the wait end, the join must not wait for a run nobody can
                // stop any more.
                cancel.store(true, Ordering::Relaxed);
                // A run told to stop can still be in a slow write or a dialog's answer: the
                // wait goes on, so the window keeps answering, rather than a bare join.
                while !run.is_finished() {
                    wait(&|| run.is_finished(), cancel);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                run.join()
                    .unwrap_or_else(|_| thread_failed("the script's thread stopped"))
            }
            Err(e) => thread_failed(&e.to_string()),
        }
    })
}

fn thread_failed(why: &str) -> Outcome {
    Outcome {
        result: Err(Failure::Thread(why.to_string())),
        printed: String::new(),
        fuel_used: 0,
        peak_memory: 0,
        files_written: false,
    }
}

#[cfg(test)]
#[path = "script_panel_tests.rs"]
mod tests;
