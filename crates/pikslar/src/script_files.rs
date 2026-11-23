//! The open and save dialogs a plugin's run asks for. The run is on a thread of its own, but dialogs
//! belong to the window's thread: the run puts its question in the bridge and waits, and the
//! window, which is waiting for the run anyway, shows the dialog and gives back the answer.
//! The dialog's title always starts with `Plugin id "Name" asks:`, so a plugin cannot pass its
//! request off as Pikslar's own or another plugin's: the ASCII id comes first, the name's own
//! double quotes are shown as single ones so it cannot end its quotes early, and the plugin's
//! words come after it.

use pikslar_plugin::files::{Access, Disk, FilePicker, FileRequest};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

/// A dialog asked for: its title and filter, as the platform's dialogs take them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Question {
    pub title: String,
    pub filter: String,
    /// The save dialog, which confirms overwriting, rather than the open dialog.
    pub save: bool,
    /// For a save: the extension a typed name gets, the first kind the plugin asked for.
    pub extension: Option<String>,
}

#[derive(Debug, Default)]
enum Slot {
    #[default]
    Empty,
    Asked(Question),
    Answered(Option<PathBuf>),
    /// Nobody answers any more: the window stopped waiting for the run. Every ask is
    /// cancelled at once, as if the user had cancelled the dialog.
    Closed,
}

/// Where a run's question waits for the window's answer.
#[derive(Debug, Default)]
pub(crate) struct PickBridge {
    slot: Mutex<Slot>,
    answered: Condvar,
}

impl PickBridge {
    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// On the window's thread: if a run asked for a dialog, `show` shows it and its answer
    /// goes back to the run.
    pub(crate) fn answer(&self, show: impl FnOnce(&Question) -> Option<PathBuf>) {
        let question = match &*self.slot() {
            Slot::Asked(q) => q.clone(),
            _ => return,
        };
        let chosen = show(&question);
        let mut slot = self.slot();
        if let Slot::Asked(_) = &*slot {
            *slot = Slot::Answered(chosen);
        }
        drop(slot);
        self.answered.notify_all();
    }

    /// Before a run: questions will be answered.
    pub(crate) fn open(&self) {
        *self.slot() = Slot::Empty;
    }

    /// The window stopped waiting: a question waiting now, or asked later, gets `None`.
    pub(crate) fn close(&self) {
        *self.slot() = Slot::Closed;
        self.answered.notify_all();
    }

    /// On the run's thread: asks, and waits for the answer. `None` at once when the bridge is
    /// closed, and as soon as it is closed while waiting.
    fn ask(&self, question: Question) -> Option<PathBuf> {
        let mut slot = self.slot();
        if let Slot::Closed = &*slot {
            return None;
        }
        *slot = Slot::Asked(question);
        loop {
            match &*slot {
                Slot::Answered(_) => break,
                Slot::Closed => return None,
                _ => {}
            }
            slot = self
                .answered
                .wait(slot)
                .unwrap_or_else(PoisonError::into_inner);
        }
        match std::mem::take(&mut *slot) {
            Slot::Answered(chosen) => chosen,
            _ => None,
        }
    }
}

/// What a run's [`pikslar_bind::FileAccess`] asks through, for one plugin.
pub(crate) struct BridgePicker {
    bridge: Arc<PickBridge>,
    /// `Plugin id "Name"`: what every dialog title starts with.
    asker: String,
}

impl BridgePicker {
    pub(crate) fn new(bridge: Arc<PickBridge>, name: &str, id: &str) -> BridgePicker {
        BridgePicker {
            bridge,
            asker: asker(name, id),
        }
    }
}

/// `Plugin id "Name"`: the id first, as it is ASCII and checked, then the name in quotes,
/// with any double quote of its own shown as a single one.
pub(crate) fn asker(name: &str, id: &str) -> String {
    format!("Plugin {id} \"{}\"", name.replace('"', "'"))
}

impl FilePicker for BridgePicker {
    /// The name given here is the plugin's own; the title says which plugin by the name and
    /// the id this picker was made for.
    fn pick(&mut self, _plugin_name: &str, request: &FileRequest) -> Option<PathBuf> {
        self.bridge.ask(question(&self.asker, request))
    }
}

/// A fixed `Plugin id "Name" asks:` first, then the plugin's words; only the kinds of file
/// it asked for. A plugin named "Pikslar" still reads as a plugin.
pub(crate) fn question(asker: &str, request: &FileRequest) -> Question {
    let patterns = match request.extensions.is_empty() {
        true => "*.*".to_string(),
        false => request
            .extensions
            .iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(";"),
    };
    let save = request.access == Access::Write;
    Question {
        title: format!("{asker} asks: {}", request.title),
        filter: format!("{patterns}\0{patterns}\0"),
        save,
        extension: request.extensions.first().filter(|_| save).cloned(),
    }
}

/// Writes a plugin's files the system's way: `ReplaceFileW` keeps the original's
/// permissions and streams, and a new file never replaces one that appeared meanwhile.
pub(crate) struct SystemDisk;

impl Disk for SystemDisk {
    fn replace(&mut self, target: &Path, fresh: &Path, backup: &Path) -> std::io::Result<()> {
        pikslar_platform::replace_file(target, fresh, backup)
    }

    fn place_new(&mut self, fresh: &Path, target: &Path) -> std::io::Result<()> {
        pikslar_platform::move_new(fresh, target)
    }

    fn put_back(&mut self, backup: &Path, target: &Path) -> std::io::Result<()> {
        pikslar_platform::move_over(backup, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_plugin::files::Access;

    #[test]
    fn the_dialog_names_the_plugin_and_offers_only_what_it_asked_for() {
        let r = FileRequest::new(Access::Read, "Choose a palette", &["gpl", "hex"]).expect("ok");
        let q = question(&asker("Palettes", "com.example.palettes"), &r);
        let said = "Plugin com.example.palettes \"Palettes\" asks: Choose a palette";
        assert_eq!(q.title, said);
        assert_eq!(q.filter, "*.gpl;*.hex\0*.gpl;*.hex\0");
        assert!(!q.save && q.extension.is_none());
        let any = FileRequest::new(Access::Read, "Any", &[]).expect("ok");
        assert_eq!(question("P", &any).filter, "*.*\0*.*\0");
    }

    #[test]
    fn a_save_asks_for_the_save_dialog_with_the_plugins_first_kind_only() {
        let r = FileRequest::new(Access::Write, "Save the report", &["txt", "csv"]).expect("ok");
        let q = question(&asker("Report", "com.example.report"), &r);
        let said = "Plugin com.example.report \"Report\" asks: Save the report";
        assert_eq!(q.title, said);
        assert_eq!(q.filter, "*.txt;*.csv\0*.txt;*.csv\0");
        assert!(q.save);
        assert_eq!(q.extension.as_deref(), Some("txt"));
        let any = FileRequest::new(Access::Write, "Save", &[]).expect("ok");
        assert_eq!(question("P", &any).extension, None);
    }

    #[test]
    fn the_id_comes_first_and_a_name_cannot_pose_as_another_id() {
        let name = "Palettes\" (com.example.palettes) asks: ";
        let r = FileRequest::new(Access::Read, "Choose", &[]).expect("ok");
        let q = question(&asker(name, "com.evil.x"), &r);
        let said = "Plugin com.evil.x \"Palettes' (com.example.palettes) asks: \" asks: Choose";
        assert_eq!(q.title, said);
        assert_eq!(
            q.title.matches('"').count(),
            2,
            "the name stays inside its quotes"
        );
    }

    #[test]
    fn a_question_from_the_run_is_answered_on_the_window_thread() {
        let bridge = Arc::new(PickBridge::default());
        bridge.answer(|_| panic!("nothing was asked"));
        let mut picker = BridgePicker::new(bridge.clone(), "P", "com.example.p");
        let asking = std::thread::spawn(move || {
            let r = FileRequest::new(Access::Read, "Choose", &[]).expect("ok");
            picker.pick("P", &r)
        });
        let mut shown = None;
        while shown.is_none() {
            bridge.answer(|q| {
                shown = Some(q.title.clone());
                Some(PathBuf::from("chosen.txt"))
            });
            std::thread::yield_now();
        }
        assert_eq!(
            asking.join().ok().flatten(),
            Some(PathBuf::from("chosen.txt"))
        );
        let said = "Plugin com.example.p \"P\" asks: Choose";
        assert_eq!(shown.as_deref(), Some(said));
    }

    struct Chosen(Vec<PathBuf>);

    impl FilePicker for Chosen {
        fn pick(&mut self, _: &str, _: &FileRequest) -> Option<PathBuf> {
            self.0.pop()
        }
    }

    #[test]
    fn the_system_disk_replaces_and_makes_files_and_keeps_no_backup_after_success() {
        use pikslar_plugin::files::FileGrants;
        use pikslar_plugin::{Capability, CapabilitySet};
        let dir = std::env::temp_dir().join(format!("pikslar-sysdisk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("own")).expect("folders");
        std::fs::write(dir.join("old.txt"), "old").expect("written");
        let caps = CapabilitySet::NONE.with(Capability::PickFileToWrite);
        let mut grants = FileGrants::for_run("P", caps)
            .with_disk(Box::new(SystemDisk))
            .with_protected(vec![dir.join("own")]);
        let targets = [
            dir.join("own").join("x.txt"),
            dir.join("new.txt"),
            dir.join("old.txt"),
        ];
        let mut picker = Chosen(targets.to_vec());
        let r = FileRequest::new(Access::Write, "Save", &["txt"]).expect("ok");
        let mut pick = || {
            grants
                .pick(&mut picker, &r)
                .expect("picked")
                .expect("chosen")
        };
        let (old, new, own) = (pick(), pick(), pick());
        for h in [old, new, own] {
            grants.claim_write(h, 3).expect("claimed");
        }
        grants
            .commit(&[(old, b"new"), (new, b"new")], &|| true)
            .expect("written");
        assert!(
            grants.commit(&[(own, b"new")], &|| true).is_err(),
            "Pikslar's own folder"
        );
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .expect("listed")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["new.txt", "old.txt", "own"]);
        assert_eq!(std::fs::read(dir.join("old.txt")).expect("read"), b"new");
        assert!(std::fs::read_dir(dir.join("own"))
            .expect("listed")
            .next()
            .is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_closed_bridge_answers_none_at_once_and_reopens_for_the_next_run() {
        let bridge = Arc::new(PickBridge::default());
        let mut picker = BridgePicker::new(bridge.clone(), "P", "com.example.p");
        let waiting = std::thread::spawn(move || {
            let r = FileRequest::new(Access::Read, "Choose", &[]).expect("ok");
            picker.pick("P", &r)
        });
        while !matches!(&*bridge.slot(), Slot::Asked(_)) {
            std::thread::yield_now();
        }
        bridge.close();
        assert_eq!(waiting.join().ok().flatten(), None);
        let r = FileRequest::new(Access::Read, "Again", &[]).expect("ok");
        assert_eq!(
            BridgePicker::new(bridge.clone(), "P", "com.example.p").pick("P", &r),
            None
        );
        bridge.answer(|_| panic!("a closed bridge shows no dialog"));
        bridge.open();
        assert!(matches!(&*bridge.slot(), Slot::Empty));
    }
}
