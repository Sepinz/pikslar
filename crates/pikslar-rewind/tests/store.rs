//! Saving, reopening and undoing into stored steps; budgets, compaction, eviction, locking
//! and forgetting.

mod common;

use common::*;
use pikslar_core::StepCmd;
use pikslar_rewind::{Attach, Budgets, Chain, Head, RewindError, Store};

const BIG: Budgets = Budgets {
    project: 256 << 20,
    keep: 192 << 20,
    store: 2 << 30,
};

#[test]
fn a_reopened_file_undoes_through_every_stored_state_and_redoes_back() {
    let store = Store::at(fresh("round"));
    let mut e = busy(1, 12);
    let states = states_down(&mut e);
    let mut chain = None;
    save_ok(&store, &mut e, &mut chain, BIG);
    let (bytes, _) = saved(&e);
    let (mut back, found) = reopen(&store, &bytes);
    let Attach::Attached(mut chain) = found else {
        panic!("attached: {found:?}");
    };
    let mut seen = vec![back.doc().clone()];
    while undo(&store, &mut back, &mut chain) {
        seen.push(back.doc().clone());
    }
    seen.reverse();
    assert_eq!(seen.len(), states.len());
    for (n, (a, b)) in seen.iter().zip(&states).enumerate() {
        assert!(a == b, "state {n}");
    }
    for state in &states[1..] {
        back.apply(StepCmd::Redo).expect("redo");
        assert!(back.doc() == state);
    }
}

#[test]
fn later_saves_append_and_a_rewritten_past_starts_from_what_still_holds() {
    let store = Store::at(fresh("append"));
    let mut e = busy(2, 6);
    let mut chain = None;
    save_ok(&store, &mut e, &mut chain, BIG);
    let first = chain.clone().expect("chain").head;
    dot(&mut e, 0, 0, 40, 0xff00_00ff);
    save_ok(&store, &mut e, &mut chain, BIG);
    let second = chain.clone().expect("chain").head;
    assert_eq!(
        (second.gen, second.tip),
        (first.gen, first.tip + 1),
        "appended"
    );
    // Reopen, undo into stored steps, draw: the stored steps above are replaced.
    let (bytes, _) = saved(&e);
    let (mut back, found) = reopen(&store, &bytes);
    let Attach::Attached(mut c) = found else {
        panic!("attached");
    };
    for _ in 0..3 {
        assert!(undo(&store, &mut back, &mut c));
    }
    dot(&mut back, 1, 0, 7, 0xff00_ff00);
    let mut states = states_down(&mut back);
    let mut kept = Some(c);
    save_ok(&store, &mut back, &mut kept, BIG);
    let third = kept.expect("chain").head;
    assert_eq!((third.gen, third.tip), (first.gen, second.tip - 2));
    let (bytes, _) = saved(&back);
    let (mut again, found) = reopen(&store, &bytes);
    let Attach::Attached(mut c) = found else {
        panic!("attached");
    };
    let mut seen = vec![again.doc().clone()];
    while undo(&store, &mut again, &mut c) {
        seen.push(again.doc().clone());
    }
    seen.reverse();
    // Only the new step is in memory in `back`; the rest came from the store there too.
    let newest = states.pop().expect("one");
    assert!(seen.last() == Some(&newest));
    assert_eq!(seen.len(), third.tip as usize + 1);
}

#[test]
fn a_file_that_is_not_the_one_saved_attaches_nothing_and_its_history_stays() {
    let store = Store::at(fresh("changed"));
    let mut e = busy(3, 4);
    let mut chain = None;
    save_ok(&store, &mut e, &mut chain, BIG);
    let before = store.read_head(3).expect("reads");
    dot(&mut e, 0, 0, 30, 0xff12_3456);
    let (bytes, _) = saved(&e);
    let (_, found) = reopen(&store, &bytes);
    assert_eq!(found, Attach::Changed);
    assert_eq!(store.read_head(3).expect("reads"), before, "untouched");
    let none = busy(99, 1);
    let (bytes, _) = saved(&none);
    assert_eq!(reopen(&store, &bytes).1, Attach::NoHistory);
}

#[test]
fn a_session_that_did_not_write_the_history_is_refused() {
    let store = Store::at(fresh("refused"));
    let mut e = busy(4, 3);
    let mut chain = None;
    save_ok(&store, &mut e, &mut chain, BIG);
    let mut other = busy(4, 2);
    let refused = save(&store, &mut other, None, BIG, &mut no_hook());
    assert!(matches!(refused, Err(RewindError::Changed)));
    let lock = store.lock(4).expect("held");
    dot(&mut e, 0, 0, 1, 0xff00_0000);
    let held = save(&store, &mut e, chain.as_ref(), BIG, &mut no_hook());
    assert!(matches!(held, Err(RewindError::InUse)));
    assert!(matches!(store.forget(4), Err(RewindError::InUse)));
    drop(lock);
    save(&store, &mut e, chain.as_ref(), BIG, &mut no_hook()).expect("free again");
}

#[test]
fn past_its_budget_a_project_keeps_its_newest_steps() {
    let store = Store::at(fresh("budget"));
    let small = Budgets {
        project: 6000,
        keep: 3000,
        store: 2 << 30,
    };
    let mut e = busy(5, 2);
    let mut chain = None;
    for n in 0..40 {
        dot(&mut e, 0, 0, n * 7 % 47, 0xff00_0000 | (n as u32 * 77_777));
        save_ok(&store, &mut e, &mut chain, small);
        let log = std::fs::metadata(store.log_path(5, chain.as_ref().expect("c").head.gen))
            .expect("log")
            .len();
        assert!(log <= small.project + 2000, "save {n}: {log}");
    }
    let head = chain.expect("chain").head;
    assert!(head.gen > 1, "compacted");
    assert!(head.base > 0, "the oldest steps went");
    let (bytes, _) = saved(&e);
    let (mut back, found) = reopen(&store, &bytes);
    let Attach::Attached(mut c) = found else {
        panic!("attached");
    };
    let mut undone = 0;
    while undo(&store, &mut back, &mut c) {
        undone += 1;
    }
    assert_eq!(undone, head.tip - head.base, "every kept step reads");
}

/// A project folder whose head says it was last used at `used`.
fn planted(store: &Store, id: u128, used: u64, bytes: usize) {
    let dir = store.project_dir(id);
    std::fs::create_dir_all(&dir).expect("made");
    let head = Head {
        id,
        gen: 1,
        base: 0,
        tip: 0,
        tip_end: 16,
        print: 0,
        used,
    };
    std::fs::write(dir.join("head"), head.encode()).expect("written");
    std::fs::write(dir.join(pikslar_rewind::store::log_name(1)), vec![0; bytes]).expect("log");
}

#[test]
fn past_the_store_budget_the_least_recently_used_projects_go_first() {
    let store = Store::at(fresh("evict"));
    for (id, used) in [(10, 500), (11, 100), (12, 300), (13, 200)] {
        planted(&store, id, used, 10_000);
    }
    let evicted = pikslar_rewind::write::evict(&store, 25_000, 0);
    assert_eq!(evicted, 2);
    let mut left: Vec<u128> = store.projects().iter().map(|p| p.id).collect();
    left.sort();
    assert_eq!(left, [10, 12]);
    // The project being written stays, however old.
    planted(&store, 14, 1, 30_000);
    let evicted = pikslar_rewind::write::evict(&store, 35_000, 14);
    assert_eq!(evicted, 2);
    assert_eq!(store.projects().len(), 1);
    assert!(store.total_bytes() <= 35_000);
}

#[test]
fn forgetting_removes_one_project_or_all_and_nothing_else() {
    let root = fresh("forget");
    std::fs::write(root.join("note.txt"), b"not a project").expect("written");
    let store = Store::at(&root);
    for id in [20, 21, 22] {
        planted(&store, id, 5, 100);
    }
    store.forget(21).expect("forgot");
    assert!(!store.has_history(21));
    assert_eq!(store.projects().len(), 2);
    assert_eq!(store.forget_all().expect("forgot"), 0);
    assert!(store.projects().is_empty());
    assert!(root.join("note.txt").exists());
    store.forget(77).expect("nothing to forget is fine");
}

#[test]
fn save_as_starts_a_new_id_from_the_copied_history() {
    let store = Store::at(fresh("save-as"));
    let mut e = busy(30, 5);
    let mut chain = None;
    save_ok(&store, &mut e, &mut chain, BIG);
    let from: Chain = chain.expect("chain");
    dot(&mut e, 0, 0, 3, 0xff44_4444);
    e.set_document_id(pikslar_core::DocumentId::new(31));
    let plan = pikslar_rewind::plan(&mut e, Some(&from), BIG.keep).expect("plans");
    let (bytes, print) = saved(&e);
    let job = pikslar_rewind::Persist {
        id: 31,
        expect: None,
        copy_from: Some(from.head),
        link: plan.link,
        records: plan.records,
        print,
        budgets: BIG,
    };
    pikslar_rewind::persist(&store, job, &mut no_hook()).expect("written");
    let (mut back, found) = reopen(&store, &bytes);
    let Attach::Attached(mut c) = found else {
        panic!("attached");
    };
    let mut undone = 0;
    while undo(&store, &mut back, &mut c) {
        undone += 1;
    }
    assert_eq!(undone, from.head.tip + 1);
    assert_eq!(
        store.read_head(30).expect("reads"),
        Some(from.head),
        "the old one stays"
    );
}

#[test]
fn a_stored_stroke_on_a_copied_frame_shares_the_drawing_again_when_undone() {
    use pikslar_core::{ColorMode, Document, DocumentId, Editor, FrameCmd};
    let store = Store::at(fresh("shared"));
    let mut e = Editor::new(Document::new(48, 48, ColorMode::Rgba).expect("valid"));
    e.set_document_id(DocumentId::new(7));
    dot(&mut e, 0, 0, 4, pikslar_core::color::rgba(9, 9, 9, 255));
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    dot(&mut e, 0, 1, 20, pikslar_core::color::rgba(200, 9, 9, 255));
    let mut chain = None;
    save_ok(&store, &mut e, &mut chain, BIG);
    let (bytes, _) = saved(&e);
    let (mut back, found) = reopen(&store, &bytes);
    let Attach::Attached(mut chain) = found else {
        panic!("attached");
    };
    assert!(undo(&store, &mut back, &mut chain), "the stroke");
    let layer = &back.doc().layers()[0];
    let cels = (layer.cel(0).image(), layer.cel(1).image());
    assert!(std::ptr::eq(cels.0, cels.1), "one drawing again");
}
