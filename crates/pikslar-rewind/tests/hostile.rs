//! A store crafted to do harm, as the security review built it: each is refused cleanly, quickly
//! and without large allocations.

mod common;
use common::*;
use pikslar_rewind::frame::{log_start, Head, LOG_START};
use pikslar_rewind::{attach, Attach, Budgets, Store};
use std::fs;

fn plant_head(store: &Store, head: &Head) {
    fs::create_dir_all(store.project_dir(head.id)).expect("made");
    fs::write(store.project_dir(head.id).join("head"), head.encode()).expect("written");
}

#[test]
fn a_head_with_positions_past_any_history_is_refused() {
    let dir = fresh("hostile_tip");
    let store = Store::at(&dir);
    let e = busy(9, 3);
    let (_, print) = saved(&e);
    let log = store.log_path(9, 1);
    let huge = Head {
        id: 9,
        gen: 1,
        base: u64::MAX - 1,
        tip: u64::MAX,
        tip_end: 100,
        print,
        used: 1,
    };
    plant_head(&store, &huge);
    fs::write(&log, [&log_start(1)[..], &[0u8; 84]].concat()).expect("log");
    assert!(matches!(attach(&store, 9, print), Attach::Damaged(_)));
    let past_the_log = Head {
        base: 0,
        tip: 1,
        tip_end: 4096,
        ..huge
    };
    plant_head(&store, &past_the_log);
    assert!(matches!(attach(&store, 9, print), Attach::Damaged(_)));
    let sound = Head {
        tip_end: 100,
        ..past_the_log
    };
    plant_head(&store, &sound);
    assert!(matches!(attach(&store, 9, print), Attach::Attached(_)));
    let empty = Head {
        tip: 0,
        tip_end: LOG_START,
        ..past_the_log
    };
    plant_head(&store, &empty);
    assert!(matches!(attach(&store, 9, print), Attach::Attached(_)));
}

#[test]
fn only_folders_named_exactly_as_the_store_names_them_are_projects() {
    let dir = fresh("hostile_names");
    let store = Store::at(&dir);
    let odd = [
        format!("+{:031x}", 5u128),
        format!("{:032X}", 0xabcu128 << 100),
        format!("{:031x}", 6u128),
    ];
    for name in &odd {
        fs::create_dir_all(dir.join(name)).expect("made");
        fs::write(dir.join(name).join("log-0000000000000001"), [1u8; 64]).expect("written");
    }
    fs::create_dir_all(store.project_dir(7)).expect("made");
    let found: Vec<u128> = store.projects().iter().map(|p| p.id).collect();
    assert_eq!(found, [7]);
    assert_eq!(
        store.total_bytes(),
        0,
        "what is not a project is not counted"
    );
    store.forget_all().expect("forgot");
    assert!(odd.iter().all(|name| dir.join(name).exists()), "left alone");
}

#[test]
fn files_that_are_not_projects_never_make_eviction_remove_more() {
    let dir = fresh("hostile_stray");
    let store = Store::at(&dir);
    for id in [1u128, 2, 3] {
        fs::create_dir_all(store.project_dir(id)).expect("made");
        let log = store.project_dir(id).join("log-0000000000000001");
        fs::write(log, vec![0u8; 1000]).expect("written");
    }
    fs::write(dir.join("big.bin"), vec![0u8; 5000]).expect("written");
    fs::create_dir_all(store.project_dir(2).join("deep")).expect("made");
    fs::write(store.project_dir(2).join("deep").join("x"), [0u8; 9000]).expect("written");
    assert_eq!(store.total_bytes(), 3000);
    assert_eq!(pikslar_rewind::write::evict(&store, 2500, 1), 1);
    assert_eq!(store.projects().len(), 2);
}

/// Frames that share drawings, released and held again, with structural steps among them.
fn holds() -> pikslar_core::Editor {
    use pikslar_core::{ColorMode, Document, Editor, FrameCmd, LayerCmd};
    let mut e = Editor::new(Document::new(40, 40, ColorMode::Rgba).expect("valid"));
    e.set_document_id(pikslar_core::DocumentId::new(3));
    for at in 1..4 {
        e.apply(FrameCmd::Add { at, copy: false }).expect("room");
    }
    let layer = e.layer_id(0).expect("layer");
    dot(&mut e, 0, 0, 3, 0xff00_00ff);
    let hold = FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 1,
        to: 3,
    };
    e.apply(hold).expect("held");
    dot(&mut e, 0, 1, 6, 0xff00_ff00);
    let release = FrameCmd::Release {
        layer,
        from: 2,
        to: 2,
    };
    e.apply(release).expect("released");
    dot(&mut e, 0, 2, 9, 0xffff_0000);
    dot(&mut e, 0, 0, 12, 0xff10_1010);
    let add = LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    };
    e.apply(add).expect("room");
    dot(&mut e, 0, 3, 15, 0xff20_2020);
    e.apply(FrameCmd::HoldPrevious { layer, frame: 3 })
        .expect("held");
    dot(&mut e, 0, 3, 18, 0xff30_3030);
    e
}

#[test]
fn taking_the_steps_to_keep_leaves_what_history_is_charged() {
    let mut e = holds();
    let used = e.history().memory_used();
    let gathered = pikslar_rewind::gather(&mut e, None).expect("taken");
    assert_eq!(e.history().memory_used(), used, "while the steps are held");
    let plan = gathered.encode(Budgets::default().keep);
    assert_eq!(plan.records.len(), e.history().undo_depth());
    assert_eq!(e.history().memory_used(), used);
}

/// Save As from a history whose log runs on past its tip: what the new id gets.
fn copied(name: &str, project: u64) -> Result<u64, pikslar_rewind::RewindError> {
    let store = Store::at(fresh(name));
    let mut e = busy(40, 4);
    let mut chain = None;
    let budgets = Budgets::default();
    save_ok(&store, &mut e, &mut chain, budgets);
    let from = chain.expect("chain").head;
    let log = store.log_path(40, from.gen);
    let mut bytes = fs::read(&log).expect("log");
    bytes.resize(bytes.len() + 50_000, 7);
    fs::write(&log, &bytes).expect("grown");
    e.set_document_id(pikslar_core::DocumentId::new(41));
    let job = pikslar_rewind::Persist {
        id: 41,
        expect: None,
        copy_from: Some(from),
        link: pikslar_rewind::write::Link::After {
            pos: from.tip,
            end: from.tip_end,
        },
        records: Vec::new(),
        print: saved(&e).1,
        budgets: Budgets { project, ..budgets },
    };
    let written = pikslar_rewind::persist(&store, job, &mut no_hook())?;
    assert_eq!(written.head.tip_end, from.tip_end);
    Ok(fs::metadata(store.log_path(41, from.gen))
        .expect("copied")
        .len())
}

#[test]
fn save_as_copies_only_the_kept_history_and_only_within_the_budget() {
    let tip_end = copied("hostile_copy", 256 << 20).expect("copied");
    assert!(tip_end < 50_000, "nothing past the tip: {tip_end}");
    assert!(
        copied("hostile_copy_big", tip_end - 1).is_err(),
        "past the budget"
    );
}

#[test]
fn a_last_use_in_the_future_counts_as_now() {
    let dir = fresh("hostile_used");
    let store = Store::at(&dir);
    let forged = Head {
        id: 12,
        gen: 1,
        base: 0,
        tip: 0,
        tip_end: LOG_START,
        print: 1,
        used: u64::MAX,
    };
    plant_head(&store, &forged);
    assert!(store.measure(12).used <= pikslar_rewind::now_secs());
}

/// A directory junction at `link` to `target`, as anyone can make without rights.
#[cfg(windows)]
fn junction(link: &std::path::Path, target: &std::path::Path) {
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("cmd runs");
    assert!(made.success(), "junction made");
}

#[cfg(windows)]
#[test]
fn links_in_place_of_the_store_or_a_project_are_never_followed() {
    // Short, as `mklink` cannot make a link at a path past 260 characters.
    let dir = std::env::temp_dir().join(format!("pikslar-links-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let elsewhere = dir.join("elsewhere");
    fs::create_dir_all(elsewhere.join(format!("{:032x}", 5u128))).expect("made");
    fs::write(elsewhere.join("keep.txt"), b"not the store's").expect("written");
    // The store's own folder is a link.
    let linked = Store::at(dir.join("store"));
    junction(linked.root(), &elsewhere);
    assert!(linked.projects().is_empty(), "nothing is listed through it");
    assert!(linked.lock(9).is_err(), "nothing is written through it");
    assert!(!elsewhere.join(format!("{:032x}", 9u128)).exists());
    // A project's folder is a link.
    let store = Store::at(dir.join("real"));
    fs::create_dir_all(store.root()).expect("made");
    junction(&store.project_dir(6), &elsewhere);
    assert!(store.projects().is_empty());
    assert!(store.lock(6).is_err());
    assert!(store.forget(6).is_err() || elsewhere.join("keep.txt").exists());
    assert!(
        elsewhere.join("keep.txt").exists(),
        "what it points at is left"
    );
}
