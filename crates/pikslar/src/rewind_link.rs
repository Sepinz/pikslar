//! Rewind in the program (`docs/rewind.md`): undo history kept after a project is closed, in a
//! store of its own beside crash recovery. Off unless turned on. The project file is always
//! saved first; history is written afterwards on a thread of its own, and nothing that goes
//! wrong with it fails, delays or changes a save. Problems are said on the status line.

use crate::app::App;
use crate::pending::{QUIT_WAIT, STILL_HISTORY, UI_WAIT};
use pikslar_core::{DocumentId, OlderStep, RewindRefusal, StepCmd};
use pikslar_platform::Window;
use pikslar_rewind::{attach, Attach, Budgets, Chain, RewindError, Store, Written};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

/// How many steps that change nothing in the file one undo passes over at most.
const MOST_UNCHANGED: usize = 64;

/// A write under way.
struct Job {
    handle: JoinHandle<Result<Written, RewindError>>,
    id: u128,
    /// The lowest position it writes: written again next time if it fails.
    from: u64,
    /// When it started, for the diagnostics.
    started: std::time::Instant,
    /// The document it wrote for was replaced before it ended: its result is let go.
    left: bool,
}

/// A save whose steps wait for the write under way to end.
struct Deferred {
    /// The print of the file it saved.
    print: u128,
    /// The editor's revision when it saved: the steps are taken only if it is still that.
    revision: u64,
}

/// What a save about to give the document a new id changed, to put back if it is not saved.
pub(crate) struct RewindBefore {
    id: DocumentId,
    chain: Option<Chain>,
    foreign: bool,
    copy_from: Option<Chain>,
}

/// Rewind's part of a session.
#[derive(Default)]
pub(crate) struct RewindLink {
    pub(crate) on: bool,
    /// The store's folder: `--rewind-dir`, or the usual one.
    dir: Option<PathBuf>,
    /// The stored history the document undoes into, when one is attached.
    chain: Option<Chain>,
    /// History is kept under the document's id that is not this file's: it is left as it is,
    /// and the next save gives the file an id of its own.
    foreign: bool,
    /// Save As: the history the new file's starts from.
    copy_from: Option<Chain>,
    job: Option<Job>,
    /// A save made while a write was under way, written when that ends.
    deferred: Option<Deferred>,
    /// A stored step did not read this session: none below it is tried again.
    stuck: bool,
    /// The print of the file as it was opened, when the open took it.
    pub(crate) opened: Option<u128>,
    /// What waits for the write under way to end, in the order asked for.
    after: Vec<After>,
    /// A wait for a write was left to the frame loop, which takes this down when it ends.
    waiting: bool,
}

/// Something that needs no write under way, asked for while one was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum After {
    Attach(u128),
    Toggle,
    Forget,
    ForgetAll,
}

impl RewindLink {
    pub(crate) fn store(&self) -> Option<Store> {
        self.dir.clone().map(Store::at)
    }

    /// A new document: nothing of the last one's history carries over. A write under way
    /// for it goes on.
    pub(crate) fn waiting(&self) -> bool {
        self.waiting
    }

    pub(crate) fn document_replaced(&mut self) {
        (self.chain, self.foreign, self.copy_from, self.stuck) = (None, false, None, false);
        self.deferred = None;
    }
}

/// How a Rewind write went, in the diagnostics.
fn note_persist(started: std::time::Instant, result: &Result<Written, RewindError>) {
    use crate::diag::{note, Event};
    let failed = Event::new("rewind-persist", "fail").took(started);
    match result {
        Ok(_) => note(Event::new("rewind-persist", "ok").took(started)),
        Err(RewindError::Io(e)) => note(failed.phase("io").io(e)),
        Err(RewindError::Damaged(_)) => note(failed.phase("damaged")),
        Err(RewindError::Changed) => note(failed.phase("changed")),
        Err(RewindError::InUse) => note(failed.phase("in-use")),
    }
}

impl App {
    /// Where the store is, and whether Rewind is on, from the settings. A project opened at
    /// launch is attached here, once the settings are known.
    pub(crate) fn use_rewind(&mut self, dir: Option<PathBuf>) {
        self.rewind.dir = dir.or_else(Store::default_dir);
        self.rewind.on = self.settings_saved.rewind && self.rewind.dir.is_some();
        if self.rewind.on && self.path.is_some() {
            let print = self.opened_print();
            self.attach_rewind(print);
        }
    }

    fn doc_id(&self) -> Option<u128> {
        self.editor.doc().document_id.get()
    }

    /// Looks for history for the document just opened, by its id and the print of the file.
    /// Only the head is read; steps are read when undo reaches them.
    pub(crate) fn attach_rewind(&mut self, print: Option<u128>) {
        if print.is_some() {
            self.rewind.opened = print;
        }
        let (Some(store), Some(id), Some(print)) = (self.rewind.store(), self.doc_id(), print)
        else {
            return;
        };
        if !self.rewind.on || self.path.is_none() {
            return;
        }
        if !self.rewind_free(After::Attach(print)) {
            return;
        }
        match attach(&store, id, print) {
            Attach::Attached(chain) => {
                // Positions already moved up if Rewind was on earlier in this session.
                let below = self.editor.rewind_marks().below;
                let Some(by) = chain.head.tip.checked_sub(below) else {
                    self.rewind.foreign = true;
                    return;
                };
                self.editor.rebase_rewind(by);
                let kept = chain.head.tip - chain.head.base;
                self.rewind.chain = Some(chain);
                if kept > 0 {
                    self.notify(format!(
                        "Rewind kept {kept} earlier steps: undo reaches them"
                    ));
                }
            }
            Attach::NoHistory => {}
            Attach::Changed => {
                self.rewind.foreign = true;
                self.notify(
                    "This file changed since Rewind kept its history, so that history is not \
                     used. It is kept as it was."
                        .to_string(),
                );
            }
            Attach::Damaged(why) => {
                self.rewind.foreign = true;
                self.notify(format!("Rewind could not read this file's history ({why})"));
            }
        }
    }

    /// Before a save: the file the history will end at takes an id of its own when it has
    /// none, when it is saved under another name, or when the history under its id is not
    /// this file's. No step, and no change to undo. Returns what to put back if the file is
    /// not written after all ([`App::rewind_not_saved`]).
    pub(crate) fn prepare_rewind_save(&mut self, target: &Path) -> Option<RewindBefore> {
        if !self.rewind.on {
            return None;
        }
        let elsewhere = self
            .path
            .as_deref()
            .is_none_or(|p| !crate::files::same_file(p, target));
        if self.doc_id().is_some() && !elsewhere && !self.rewind.foreign {
            return None;
        }
        self.settle_rewind(false);
        let before = RewindBefore {
            id: self.editor.doc().document_id,
            chain: self.rewind.chain.clone(),
            foreign: self.rewind.foreign,
            copy_from: self.rewind.copy_from.clone(),
        };
        let chain = self.rewind.chain.take();
        self.rewind.copy_from = chain.or(self.rewind.copy_from.take()).filter(|_| elsewhere);
        self.rewind.foreign = false;
        let id = pikslar_rewind::new_document_id();
        self.editor.set_document_id(DocumentId::new(id));
        Some(before)
    }

    /// The save [`App::prepare_rewind_save`] made ready did not write the file: failed,
    /// cancelled, or went to a copy instead. The id and history are as they were.
    pub(crate) fn rewind_not_saved(&mut self, before: Option<RewindBefore>) {
        if let Some(before) = before {
            self.editor.set_document_id(before.id);
            self.rewind.chain = before.chain;
            self.rewind.foreign = before.foreign;
            self.rewind.copy_from = before.copy_from;
        }
    }

    /// After a save that reached the disk: takes the steps to keep and writes them on a thread
    /// of its own, which also encodes them. `print` is the hash of the file as saved. While an earlier
    /// write is still under way the save never waits for it: this one is made when that ends,
    /// if the document is still as saved then.
    pub(crate) fn rewind_saved(&mut self, print: u128) {
        if !self.rewind.on || self.rewind.store().is_none() || self.doc_id().is_none() {
            return;
        }
        self.settle_rewind(false);
        match self.rewind.job {
            Some(_) => {
                let revision = self.editor.revision();
                self.rewind.deferred = Some(Deferred { print, revision });
            }
            None => self.start_rewind_write(print),
        }
    }

    /// Takes the steps the saved file ends at and starts the thread that encodes and writes
    /// them.
    fn start_rewind_write(&mut self, print: u128) {
        let (Some(store), Some(id)) = (self.rewind.store(), self.doc_id()) else {
            return;
        };
        let copy = self.rewind.copy_from.take();
        let known = copy.as_ref().or(self.rewind.chain.as_ref());
        let gathered = match pikslar_rewind::gather(&mut self.editor, known) {
            Ok(gathered) => gathered,
            Err(_) => return self.notify("Rewind kept nothing this time: finish first".into()),
        };
        let from = gathered.lowest();
        let expect = self.rewind.chain.as_ref().map(|c| c.head);
        let copy_from = copy.map(|c| c.head);
        self.editor.clear_rewind_rewrites();
        let write = move || {
            let budgets = Budgets::default();
            let plan = gathered.encode(budgets.keep);
            let job = pikslar_rewind::Persist {
                id,
                expect,
                copy_from,
                link: plan.link,
                records: plan.records,
                print,
                budgets,
            };
            pikslar_rewind::persist(&store, job, &mut |_| Ok(()))
        };
        match std::thread::Builder::new()
            .name("rewind".into())
            .spawn(write)
        {
            Ok(handle) => {
                let started = std::time::Instant::now();
                self.rewind.job = Some(Job {
                    handle,
                    id,
                    from,
                    started,
                    left: false,
                });
            }
            Err(e) => self.rewind_not_kept(from, &RewindError::Io(e)),
        }
    }

    /// Takes the result of a write that has ended, or waits for one under way if `wait`.
    /// Then makes a write that was waiting for it.
    pub(crate) fn settle_rewind(&mut self, wait: bool) {
        let ended = self
            .rewind
            .job
            .as_ref()
            .is_some_and(|j| j.handle.is_finished());
        if ended || wait {
            self.end_rewind_job();
        }
        if self.rewind.job.is_none() && self.rewind.deferred.is_none() {
            self.run_rewind_after();
        }
    }

    fn end_rewind_job(&mut self) {
        if let Some(job) = self.rewind.job.take() {
            let stopped = RewindError::Damaged("the Rewind write stopped unexpectedly");
            let result = job.handle.join().unwrap_or(Err(stopped));
            note_persist(job.started, &result);
            if !job.left {
                self.rewind_ended((job.id, job.from), result);
            }
        }
        if let Some(deferred) = self.rewind.deferred.take() {
            // Changed since that save: its steps are written by the next one.
            if deferred.revision == self.editor.revision() {
                self.start_rewind_write(deferred.print);
            }
        }
    }

    /// With no write under way: does what waited for it, in order.
    fn run_rewind_after(&mut self) {
        self.rewind.waiting = false;
        for after in std::mem::take(&mut self.rewind.after) {
            match after {
                // History is attached only while this session has none of its own.
                After::Attach(print) if self.editor.history().undo_depth() == 0 => {
                    self.attach_rewind(Some(print))
                }
                After::Attach(_) => self.rewind.foreign = self.has_kept_history(),
                After::Toggle => self.toggle_rewind(),
                After::Forget => self.forget_rewind(),
                After::ForgetAll => self.forget_all_rewind(),
            }
        }
    }

    /// Waits up to `within` until no write is under way or waiting. True once none is.
    pub(crate) fn finish_rewind_within(&mut self, within: Duration) -> bool {
        let end = std::time::Instant::now() + within;
        loop {
            self.settle_rewind(false);
            if self.rewind.job.is_none() && self.rewind.deferred.is_none() {
                return true;
            }
            if std::time::Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Waits a moment for a write under way. Past that, `then` waits its turn, which the
    /// frame loop gives it, and the status line says so: false then.
    fn rewind_free(&mut self, then: After) -> bool {
        if self.rewind.after.is_empty() && self.finish_rewind_within(UI_WAIT) {
            return true;
        }
        self.rewind.after.push(then);
        self.rewind.waiting = true;
        self.say_pending(STILL_HISTORY);
        false
    }

    /// Before the document is replaced, what it saved is written, for a moment. Past that the
    /// write goes on, its result is let go, and what waited for it goes with the document.
    pub(crate) fn let_rewind_go(&mut self) {
        if self.finish_rewind_within(UI_WAIT) {
            return;
        }
        crate::diag::note(crate::diag::Event::new("rewind-persist", "left"));
        if let Some(job) = &mut self.rewind.job {
            job.left = true;
        }
        self.rewind.deferred = None;
        self.rewind.after.clear();
        self.rewind.waiting = true;
    }

    /// The session is closing: waits for a write under way, bounded, and notes one left.
    pub(crate) fn finish_rewind_closing(&mut self) {
        if !self.finish_rewind_within(QUIT_WAIT) {
            crate::diag::note(crate::diag::Event::new("rewind-persist", "abandoned"));
        }
    }

    /// Whether a write is under way or waiting.
    #[cfg(test)]
    pub(crate) fn rewind_writing(&self) -> bool {
        self.rewind.job.is_some() || self.rewind.deferred.is_some()
    }

    /// A write that does not end until `gate` says so, or 10 s pass: a stuck disk. It is for
    /// no document's history, so its end changes nothing.
    #[cfg(test)]
    pub(crate) fn hold_rewind(&mut self, gate: std::sync::mpsc::Receiver<()>) {
        let handle = std::thread::spawn(move || {
            let _ = gate.recv_timeout(Duration::from_secs(10));
            Err(RewindError::InUse)
        });
        self.rewind.job = Some(Job {
            handle,
            id: 0,
            from: u64::MAX,
            started: std::time::Instant::now(),
            left: false,
        });
    }

    /// Waits until no write is under way or waiting, for tests.
    #[cfg(test)]
    pub(crate) fn finish_rewind(&mut self) {
        while self.rewind.job.is_some() || self.rewind.deferred.is_some() {
            self.settle_rewind(true);
        }
    }

    fn rewind_ended(&mut self, (id, from): (u128, u64), result: Result<Written, RewindError>) {
        if Some(id) != self.doc_id() {
            // Save As took the history the write was for: what it wrote is the copy's start.
            if let (Ok(written), Some(copy)) = (&result, &mut self.rewind.copy_from) {
                if copy.head.id == written.head.id {
                    copy.take(written);
                }
            }
            return;
        }
        match result {
            Ok(written) => self.rewind_written(&written),
            Err(RewindError::Changed) => {
                (self.rewind.chain, self.rewind.foreign) = (None, true);
                self.notify(format!(
                    "Rewind kept nothing this time: {}. The file is saved.",
                    RewindError::Changed
                ));
            }
            Err(e) => self.rewind_not_kept(from, &e),
        }
    }

    fn rewind_written(&mut self, written: &Written) {
        match &mut self.rewind.chain {
            Some(chain) if chain.head.id == written.head.id => chain.take(written),
            _ => self.rewind.chain = Some(Chain::written(written)),
        }
    }

    fn rewind_not_kept(&mut self, from: u64, why: &RewindError) {
        if from != u64::MAX {
            self.editor.rewind_not_written_from(from);
        }
        self.notify(format!(
            "Rewind kept nothing this time ({why}). The file is saved."
        ));
    }

    /// Before undo with nothing left in memory: brings in the next stored step, passing over
    /// steps that change nothing in the file, such as a selection.
    pub(crate) fn rewind_before_undo(&mut self) {
        if !self.rewind.on || self.rewind.stuck || self.editor.history().undo_depth() > 0 {
            return;
        }
        let Some(store) = self.rewind.store() else {
            return;
        };
        self.settle_rewind(false);
        for _ in 0..MOST_UNCHANGED {
            let below = self.editor.rewind_marks().below;
            let room = self.editor.rewind_tile_room();
            let Some(chain) = self.rewind.chain.as_mut().filter(|c| c.has_step(below)) else {
                return;
            };
            if self.editor.history().is_over_budget() {
                return self.rewind_full();
            }
            let step = match chain.read_older(&store, below, room) {
                Ok(step) => step,
                // A write under way may have rewritten the log: tried again at the next undo.
                Err(_) if self.rewind.job.is_some() => return,
                Err(e) => return self.rewind_stuck(&e.to_string()),
            };
            let unchanged = matches!(step, OlderStep::Unchanged);
            match self.editor.push_older(step) {
                Ok(()) => {}
                // Floating pixels or a stroke: the step is read again at the next undo.
                Err(RewindRefusal::Busy | RewindRefusal::NotAtBottom) => return,
                Err(RewindRefusal::DoesNotFit) => {
                    return self.rewind_stuck("a kept step does not fit this file")
                }
                Err(RewindRefusal::Full) => return self.rewind_full(),
            }
            if !unchanged {
                return;
            }
            self.run(StepCmd::Undo);
        }
    }

    /// Undo has reached the oldest step memory can hold; Rewind is not stuck, and after an
    /// edit or on reopening it goes further.
    fn rewind_full(&mut self) {
        self.notify(
            "Undo stops here for now: undo history in memory is full. Reopen the file to undo \
             further"
                .to_string(),
        );
    }

    fn rewind_stuck(&mut self, why: &str) {
        self.rewind.stuck = true;
        self.notify(format!(
            "Rewind could not read older steps ({why}): undo stops here"
        ));
    }

    /// Turns Rewind on or off from the View menu, saying what it does.
    pub(crate) fn toggle_rewind(&mut self) {
        if !self.rewind_free(After::Toggle) {
            return;
        }
        if self.rewind.on {
            self.rewind.on = false;
            self.rewind.document_replaced();
            self.notify(
                "Rewind is off. History already kept stays until you forget it.".to_string(),
            );
            return;
        }
        if self.rewind.dir.is_none() {
            return self.notify("Rewind needs a folder of your own to keep history in".into());
        }
        self.rewind.on = true;
        self.notify(
            "Rewind is on: from each save, undo history is kept for that file in your own \
             app data folder, never in the file, up to 256 MB a file and 2 GB in all. Forget \
             it from the View menu."
                .to_string(),
        );
        // History from before this session is attached only while this one has none.
        let history = self.editor.history();
        if history.undo_depth() + history.redo_depth() == 0 {
            if self.path.is_some() {
                let print = self.opened_print();
                self.attach_rewind(print);
            }
        } else if self.has_kept_history() {
            self.rewind.foreign = true;
        }
    }

    /// The print of the document as it was opened: taken by the open when Rewind was on then,
    /// or else from the document as this version saves it, which is the file's own bytes when
    /// nothing changed since and this version wrote it. Never read again from the disk, where
    /// another program may have changed the file since it was opened.
    fn opened_print(&self) -> Option<u128> {
        let saved = || pikslar_rewind::hash128(&pikslar_format::piks::save(self.editor.doc()));
        Some(self.rewind.opened.unwrap_or_else(saved))
    }

    fn has_kept_history(&self) -> bool {
        let id = self.doc_id();
        let store = self.rewind.store();
        id.zip(store)
            .is_some_and(|(id, store)| store.has_history(id))
    }

    /// Forgets the open file's history, leaving the file and everything else as it is.
    pub(crate) fn forget_rewind(&mut self) {
        if !self.rewind_free(After::Forget) {
            return;
        }
        let (Some(store), Some(id)) = (self.rewind.store(), self.doc_id()) else {
            return self.notify("This file has no Rewind history".to_string());
        };
        match store.forget(id) {
            Ok(()) => {
                self.rewind.document_replaced();
                self.notify("Forgot this file's Rewind history".to_string());
            }
            Err(e) => self.notify(format!("Rewind history was kept: {e}")),
        }
    }

    /// Forgets every file's history, once the person said Yes: it cannot be undone.
    pub(crate) fn ask_forget_all_rewind(&mut self, window: Window) {
        if (self.ask_forget_all)(window) {
            self.forget_all_rewind();
        } else {
            self.notify("Kept all Rewind history".to_string());
        }
    }

    /// Forgets every file's history.
    pub(crate) fn forget_all_rewind(&mut self) {
        if !self.rewind_free(After::ForgetAll) {
            return;
        }
        let Some(store) = self.rewind.store() else {
            return;
        };
        match store.forget_all() {
            Ok(0) => {
                self.rewind.document_replaced();
                self.notify("Forgot all Rewind history".to_string());
            }
            Ok(kept) => {
                self.rewind.document_replaced();
                self.notify(format!(
                    "Forgot Rewind history, except {kept} another Pikslar is using"
                ));
            }
            Err(e) => self.notify(format!("Some Rewind history was kept: {e}")),
        }
    }

    /// The head of the attached history, for tests.
    #[cfg(test)]
    pub(crate) fn rewind_head(&self) -> Option<pikslar_rewind::Head> {
        self.rewind.chain.as_ref().map(|c| c.head)
    }
}

#[cfg(test)]
#[path = "rewind_link_tests.rs"]
mod tests;
