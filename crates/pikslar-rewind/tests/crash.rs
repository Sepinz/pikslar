//! A write stopped at every step, as a crash stops it, leaves the project at its old history
//! or its new one, both readable to the end, and the next write goes through.

mod common;

use common::*;
use pikslar_rewind::{Attach, Budgets, Head, Step, Store};

/// Every step a write passes, in order.
fn steps_of(budgets: Budgets, name: &str, first_save: bool) -> Vec<Step> {
    let (store, mut e, chain, _) = ready(name, first_save);
    let mut steps = Vec::new();
    save(&store, &mut e, chain.as_ref(), budgets, &mut |s| {
        steps.push(s);
        Ok(())
    })
    .expect("written");
    steps
}

/// A store with one save done when `first_save` is false, and an editor with a step more.
type Ready = (
    Store,
    pikslar_core::Editor,
    Option<pikslar_rewind::Chain>,
    Option<Vec<u8>>,
);

fn ready(name: &str, first_save: bool) -> Ready {
    let store = Store::at(fresh(name));
    let mut e = busy(7, 8);
    let (mut chain, mut old) = (None, None);
    if !first_save {
        save_ok(&store, &mut e, &mut chain, BUDGETS);
        old = Some(saved(&e).0);
        for n in 0..4 {
            dot(&mut e, 1, 1, n * 9, 0xff10_2030 + n as u32);
        }
    }
    (store, e, chain, old)
}

const BUDGETS: Budgets = Budgets {
    project: 256 << 20,
    keep: 192 << 20,
    store: 2 << 30,
};

/// Undoes as far as the store allows from the saved file; how many steps that was.
fn depth_readable(store: &Store, bytes: &[u8]) -> Option<u64> {
    let (mut e, found) = reopen(store, bytes);
    let Attach::Attached(mut chain) = found else {
        return None;
    };
    let mut n = 0;
    while undo(store, &mut e, &mut chain) {
        n += 1;
    }
    assert_eq!(n, chain.head.tip - chain.head.base, "every kept step reads");
    Some(n)
}

fn crash_everywhere(budgets: Budgets, first_save: bool) {
    let name = format!("crash-{}-{first_save}", budgets.project);
    let steps = steps_of(budgets, &name, first_save);
    assert!(steps.len() > 8, "{steps:?}");
    for (n, at) in steps.iter().enumerate() {
        let (store, mut e, chain, old_bytes) = ready(&name, first_save);
        let old: Option<Head> = store.read_head(7).expect("reads");
        let mut count = 0;
        // Sticky, as a crash is: nothing after the stop runs.
        let result = save(&store, &mut e, chain.as_ref(), budgets, &mut |_| {
            count += 1;
            match count > n {
                true => Err(std::io::Error::other("stopped")),
                false => Ok(()),
            }
        });
        let now = store.read_head(7).expect("never torn");
        let (new_bytes, new_print) = saved(&e);
        match now {
            Some(head) if head.print == new_print => {
                let depth = depth_readable(&store, &new_bytes).expect("attaches");
                assert!(depth > 0, "stop at {at:?}");
            }
            other => {
                assert_eq!(other.map(|h| h.tip), old.map(|h| h.tip), "stop at {at:?}");
                assert!(
                    result.is_err(),
                    "stop at {at:?} reported done with the old head"
                );
                if let Some(bytes) = &old_bytes {
                    depth_readable(&store, bytes).expect("the old history attaches");
                }
            }
        }
        // The program carries on: the next write tidies up and goes through.
        let mut chain = chain;
        if let Ok(w) = &result {
            if let Some(c) = chain.as_mut() {
                c.take(w);
            }
        }
        let expect = store.read_head(7).expect("reads");
        let fresh_chain = expect.map(pikslar_rewind::Chain::attached);
        let usable = match (&chain, &fresh_chain) {
            (Some(c), Some(f)) if c.head.same_chain(&f.head) => chain,
            _ => fresh_chain,
        };
        save(&store, &mut e, usable.as_ref(), budgets, &mut no_hook()).expect("next write");
        let (bytes, _) = saved(&e);
        depth_readable(&store, &bytes).expect("attaches after");
    }
}

#[test]
fn a_first_save_stopped_anywhere_leaves_no_history_or_all_of_it() {
    crash_everywhere(BUDGETS, true);
}

#[test]
fn a_later_save_stopped_anywhere_leaves_the_old_history_or_the_new() {
    crash_everywhere(BUDGETS, false);
}

#[test]
fn a_compaction_or_eviction_stopped_anywhere_leaves_a_sound_history() {
    let small = Budgets {
        project: 600,
        keep: 500,
        store: 2 << 30,
    };
    let steps = steps_of(small, "crash-steps-small", false);
    assert!(steps.contains(&Step::Compacted), "{steps:?}");
    crash_everywhere(small, false);
}
