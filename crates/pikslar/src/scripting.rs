//! Running a script on the open document, for the script panel to come and for
//! `--run-script` at launch. Nothing about scripts exists until the first one runs: the
//! interpreter's thread is started for each run and ends with it.

use crate::app::App;
use pikslar_bind::{Capabilities, Outcome};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A script file larger than this is not read: the interpreter takes no more source.
const MOST_SOURCE: u64 = 256 << 10;

impl App {
    /// Runs `source` on the document as one undo step, with the host functions `caps` grants
    /// and the limits from the settings. Nothing is kept unless the whole script succeeds.
    /// The tests run scripts this way; the app always runs them beside the window.
    #[cfg(test)]
    pub(crate) fn run_script(
        &mut self,
        source: &str,
        caps: Capabilities,
        cancel: &AtomicBool,
    ) -> Outcome {
        self.finish_gesture();
        self.playing = false;
        let layer = self.editor.layer;
        let limits = self.script_limits;
        let out = pikslar_bind::run_script(&mut self.editor, source, caps, limits, cancel);
        if out.result.is_ok() {
            self.editor.damage_all();
            self.after_history(layer);
        }
        out
    }

    /// [`App::run_script`] on a thread of its own, while `wait` waits for it to finish and may
    /// set the flag it is given to cancel it.
    pub(crate) fn run_script_waiting(
        &mut self,
        source: &str,
        caps: Capabilities,
        files: Option<pikslar_bind::FileAccess>,
        wait: impl FnMut(&dyn Fn() -> bool, &AtomicBool),
    ) -> Outcome {
        self.finish_gesture();
        self.playing = false;
        let layer = self.editor.layer;
        let limits = self.script_limits;
        let editor = &mut self.editor;
        let out = crate::script_panel::run_beside(editor, (source, caps), limits, files, wait);
        if out.result.is_ok() {
            self.editor.damage_all();
            self.after_history(layer);
        }
        out
    }

    /// Runs a script file with every capability, as the artist's own, and says how it went.
    /// At launch there is no window to press Stop in, so a run still going after
    /// [`LAUNCH_SCRIPT_TIME`] is cancelled, as Stop would.
    pub(crate) fn run_script_file(&mut self, path: &Path) {
        self.run_script_file_within(path, LAUNCH_SCRIPT_TIME);
    }

    pub(crate) fn run_script_file_within(&mut self, path: &Path, most: Duration) {
        let name = file_name(path);
        let message = match read_source(path) {
            Ok(source) => {
                let out = self.run_script_waiting(&source, Capabilities::ALL, None, |done, c| {
                    wait_at_most(most, done, c)
                });
                match out.result {
                    Ok(_) => format!("Script {name} ran"),
                    Err(f) if f.is_cancel() => format!(
                        "Script {name} was stopped after {} s. Nothing changed",
                        most.as_secs_f64()
                    ),
                    Err(failure) => format!("Script {name} changed nothing: {failure}"),
                }
            }
            Err(why) => format!("Script {name} could not be read: {why}"),
        };
        use crate::script_panel::{clipped, MOST_LINE_CHARS};
        self.notify(clipped(&message, MOST_LINE_CHARS));
    }
}

/// How long a `--run-script` run may take before it is cancelled.
pub(crate) const LAUNCH_SCRIPT_TIME: Duration = Duration::from_secs(60);

/// Waits until `done()`, cancelling the run once `most` has passed.
fn wait_at_most(most: Duration, done: &dyn Fn() -> bool, cancel: &AtomicBool) {
    let end = Instant::now() + most;
    while !done() {
        if Instant::now() >= end {
            cancel.store(true, Ordering::Relaxed);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

pub(crate) fn read_source(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(MOST_SOURCE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MOST_SOURCE {
        return Err("it is larger than 256 KiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "it is not UTF-8 text".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_app::app;
    use pikslar_core::StepCmd;

    #[test]
    fn a_script_is_one_undo_step_in_the_app() {
        let mut a = app(2);
        let before = a.editor.revision();
        let out = a.run_script(
            "add_frame(0); rename_layer(layers()[0], \"Lines\");",
            Capabilities::ALL,
            &AtomicBool::new(false),
        );
        assert!(out.result.is_ok(), "{:?}", out.result);
        assert_eq!(a.editor.doc().frames.len(), 3);
        assert!(a.is_modified() && a.editor.revision() > before);
        a.editor.apply(StepCmd::Undo).expect("undoes");
        assert_eq!(a.editor.doc().frames.len(), 2);
    }

    #[test]
    fn a_failed_script_changes_nothing_and_says_why() {
        let mut a = app(2);
        let before = (a.editor.revision(), a.editor.history().undo_depth());
        let dir = std::env::temp_dir().join(format!("pikslar-script-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder");
        let path = dir.join("bad.pks");
        std::fs::write(&path, "add_frame(0);\nremove_layer(424242);").expect("written");
        a.run_script_file(&path);
        assert_eq!(
            (a.editor.revision(), a.editor.history().undo_depth()),
            before
        );
        let said = a.message.as_ref().map(|m| m.0.clone()).unwrap_or_default();
        assert!(
            said.contains("changed nothing") && said.contains("line 2"),
            "{said}"
        );
        a.run_script_file(&dir.join("missing.pks"));
        let said = a.message.as_ref().map(|m| m.0.clone()).unwrap_or_default();
        assert!(said.contains("could not be read"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn limits_come_from_the_settings() {
        let mut a = app(1);
        a.script_limits.fuel = 10_000;
        let out = a.run_script("while true { }", Capabilities::ALL, &AtomicBool::new(false));
        assert!(
            (9_000..=10_000).contains(&out.fuel_used),
            "{}",
            out.fuel_used
        );
        assert!(out.result.is_err_and(|f| f.is_stop()));
    }
}
