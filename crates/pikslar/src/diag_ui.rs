//! The program's side of diagnostics (`docs/diagnostics.md`): the setting, the View menu's
//! switch and delete, and the crash note mentioned at launch.

use crate::app::App;
use crate::diag::{self, Event, CRASH_DIR};
use std::path::{Path, PathBuf};

/// Whether the settings in `dir` keep diagnostics: on unless they say otherwise.
pub(crate) fn persisted_on(dir: &Path) -> bool {
    !crate::settings::SettingsFile::in_dir(dir)
        .load()
        .0
        .no_diagnostics
}

/// Starts diagnostics in `root` as its settings say, before anything else at launch, so a
/// person who turned them off gets no crash note even from the first moments.
pub(crate) fn launch_diagnostics(root: Option<PathBuf>) {
    let on = root.as_deref().is_some_and(persisted_on);
    diag::start(root, on);
}

impl App {
    /// After the settings are read: diagnostics follow the setting, the launch is noted, and a
    /// crash note the last session left is mentioned once. With `run_off`
    /// (`--no-diagnostics`) none are kept and the setting is never written by this run.
    pub(crate) fn use_diagnostics(&mut self, run_off: bool) {
        self.diagnostics_run_off = run_off;
        diag::set_on(self.diagnostics && !run_off);
        diag::note(Event::new("start", "ok"));
        if let Some(root) = diag::root().filter(|_| diag::is_on()) {
            self.mention_crash(&root.join(CRASH_DIR));
        }
    }

    /// The View menu's switch. The choice is kept in the settings.
    pub(crate) fn toggle_diagnostics(&mut self) {
        if self.diagnostics_run_off {
            return self.notify(
                "Crash and saving notes are off for this run (--no-diagnostics). The setting is                  unchanged"
                    .to_string(),
            );
        }
        self.diagnostics = !self.diagnostics;
        if !self.diagnostics {
            diag::note(Event::new("diagnostics", "off"));
        }
        diag::set_on(self.diagnostics);
        let said = match (self.diagnostics, diag::root(), diag::problem()) {
            (false, _, _) => "Crash and saving notes are no longer kept".to_string(),
            (true, None, _) => "Crash and saving notes will be kept from the next start, \
                                as this session keeps no settings"
                .to_string(),
            (true, Some(_), Some(why)) => {
                format!("Crash and saving notes cannot be written this session: {why}")
            }
            (true, Some(_), None) => "Crash and saving notes are kept".to_string(),
        };
        self.notify(said);
    }

    /// The View menu's delete: the log and every crash note go.
    pub(crate) fn delete_diagnostics(&mut self) {
        if diag::root().is_none() {
            self.notify("This session keeps no crash or saving notes".to_string());
            return;
        }
        match diag::delete() {
            Ok(()) => self.notify("Crash and saving notes deleted".to_string()),
            Err(e) => {
                self.notify_error(format!("Crash and saving notes could not be deleted: {e}"))
            }
        }
    }
}
