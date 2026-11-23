//! For measurement runs only (`spikes/soak.ps1 -Workload fixed`): what the document and its
//! undo history hold, written to a file at most once a second so a soak can set it beside the
//! process's own memory. Asked for with `--memory-dump <file>`; never written otherwise.

use crate::app::App;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How often the file is brought up to date, at most.
const EVERY: Duration = Duration::from_secs(1);

/// The file and when it was last written.
pub(crate) struct MemoryDump {
    path: PathBuf,
    last: Option<Instant>,
    failed: bool,
}

impl MemoryDump {
    pub(crate) fn new(path: PathBuf) -> Self {
        MemoryDump {
            path,
            last: None,
            failed: false,
        }
    }
}

/// One line of `key=value` pairs: the counts a soak records each minute.
pub(crate) fn memory_line(app: &App) -> String {
    let doc = app.editor.doc();
    let history = app.editor.history();
    let cels: usize = doc.layers().iter().map(|l| l.cels().len()).sum();
    format!(
        "frames={} layers={} cels={} document_bytes={} undo_steps={} redo_steps={} \
         history_bytes={} pushes={}\n",
        doc.frames.len(),
        doc.layers().len(),
        cels,
        doc.held_bytes(),
        history.undo_depth(),
        history.redo_depth(),
        history.memory_used(),
        history.pushes(),
    )
}

impl App {
    /// Keeps the document's and history's sizes in `path` from now on, at most once a second.
    pub fn dump_memory_to(&mut self, path: PathBuf) {
        self.memory_dump = Some(MemoryDump::new(path));
    }

    /// Bounds undo history to `kib` KiB for this document: measurement runs only.
    pub fn limit_undo_for_measurement(&mut self, kib: usize) {
        self.editor.set_undo_budget(kib.saturating_mul(1024));
    }

    /// Called after each frame is drawn. A file that cannot be written is left alone, once said.
    pub(crate) fn write_memory(&mut self) {
        let due = match &self.memory_dump {
            Some(d) => !d.failed && d.last.is_none_or(|t| t.elapsed() >= EVERY),
            None => false,
        };
        if !due {
            return;
        }
        let line = memory_line(self);
        let Some(dump) = &mut self.memory_dump else {
            return;
        };
        dump.last = Some(Instant::now());
        let mut temp = dump.path.clone().into_os_string();
        temp.push(".part");
        let written =
            std::fs::write(&temp, &line).and_then(|()| std::fs::rename(&temp, &dump.path));
        if let Err(e) = written {
            dump.failed = true;
            let message = format!(
                "Could not write the memory dump to {}: {e}",
                dump.path.display()
            );
            self.notify(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_app::*;

    #[test]
    fn the_line_counts_document_and_history() {
        let mut a = app(1);
        let before = memory_line(&a);
        assert!(before.contains("undo_steps=0 redo_steps=0"), "{before}");
        let (x, y) = screen(&a, 4, 4);
        a.pointer_down(x, y, pikslar_platform::Button::Left, PLAIN);
        a.pointer_up(x, y, pikslar_platform::Button::Left);
        let after = memory_line(&a);
        assert!(after.contains("undo_steps=1 "), "{after}");
        assert!(after.ends_with('\n') && after.split_whitespace().count() == 8);
    }

    #[test]
    fn a_measurement_budget_bounds_history_and_the_default_is_untouched() {
        let mut a = app(1);
        assert_eq!(
            pikslar_core::history::DEFAULT_BUDGET,
            256 << 20,
            "the program's own budget"
        );
        a.limit_undo_for_measurement(1);
        for i in 0..200 {
            let (x, y) = screen(&a, i % 30, (i / 30) % 30);
            a.pointer_down(x, y, pikslar_platform::Button::Left, PLAIN);
            a.pointer_up(x, y, pikslar_platform::Button::Left);
        }
        let steps = a.editor.history().undo_depth();
        assert!(
            steps < 200,
            "a 1 KiB budget keeps fewer than every step: {steps}"
        );
    }

    #[test]
    fn the_dump_is_written_and_then_waits_a_second() {
        let dir = std::env::temp_dir().join(format!("pikslar-mem-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("memory.txt");
        let mut a = app(1);
        a.dump_memory_to(path.clone());
        a.write_memory();
        let first = std::fs::read_to_string(&path).expect("written");
        assert!(first.starts_with("frames="), "{first}");
        std::fs::remove_file(&path).expect("removed");
        a.write_memory();
        assert!(!path.exists(), "not again within the second");
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }
}
