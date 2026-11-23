//! A damaged Rewind store: cut short, bits flipped, lengths made huge, ids that do not match.
//! Damage stays in that project's history: nothing panics, opening is never refused, no state
//! read back differs from the one written, memory stays small, and the project file is never
//! touched.

use pikslar_core::{rgba, ColorMode, Document, DocumentId, DrawCmd, Editor, LayerCmd, StepCmd};
use pikslar_format::piks;
use pikslar_rewind::frame::{self, RecordHead};
use pikslar_rewind::{attach, persist, plan, Attach, Budgets, Chain, Persist, Store};
use pikslar_safety::mutate::Rng;
use pikslar_safety::same::{first_difference, with_holds_numbered_afresh};
use pikslar_safety::tracking::measure;
use std::fs;
use std::path::{Path, PathBuf};

const ID: u128 = 0x5eed;

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("rewind-malformed")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("made");
    dir
}

/// A store holding a sound history and the project file it ends at, kept outside the store.
/// Also every state, oldest first.
fn sound(name: &str) -> (Store, PathBuf, Vec<Document>) {
    let root = fresh(name);
    let store = Store::at(root.join("store"));
    let mut e = Editor::new(Document::new(40, 40, ColorMode::Rgba).expect("valid"));
    e.set_document_id(DocumentId::new(ID));
    let mut states = vec![e.doc().clone()];
    for n in 0..12 {
        if n == 5 {
            let add = LayerCmd::Add {
                at: 1,
                mode: ColorMode::Rgba,
            };
            e.apply(add).expect("room");
        } else {
            (e.layer, e.color) = (0, rgba(n * 20, 30, 40, 255));
            let begin = DrawCmd::Begin {
                tool: pikslar_core::Tool::Pencil,
                x: i32::from(n) * 3,
                y: 7,
            };
            e.apply(begin).expect("begin");
            e.apply(DrawCmd::Commit).expect("commit");
        }
        states.push(e.doc().clone());
    }
    let steps = plan(&mut e, None, Budgets::default().keep).expect("plans");
    let bytes = piks::save(e.doc());
    let job = Persist {
        id: ID,
        expect: None,
        copy_from: None,
        link: steps.link,
        records: steps.records,
        print: pikslar_rewind::hash128(&bytes),
        budgets: Budgets::default(),
    };
    persist(&store, job, &mut |_| Ok(())).expect("written");
    let file = root.join("drawing.piks");
    fs::write(&file, &bytes).expect("saved");
    (store, file, states)
}

/// Opens the file and undoes as far as the store allows, each state checked against the one
/// written. Returns how many steps read; the file must open whatever the store holds.
fn undo_all(store: &Store, file: &Path, states: &[Document]) -> usize {
    let bytes = fs::read(file).expect("the project file is there");
    let doc = piks::load(&bytes).expect("the project file opens");
    let mut e = Editor::new(doc);
    let Attach::Attached(mut chain) = attach(store, ID, pikslar_rewind::hash128(&bytes)) else {
        return 0;
    };
    e.rebase_rewind(chain.head.tip);
    let mut undone = 0;
    while let Some(step) = older(store, &mut e, &mut chain) {
        if e.push_older(step).is_err() {
            break;
        }
        e.apply(StepCmd::Undo).expect("undo");
        undone += 1;
        let expected = &states[states.len() - 1 - undone];
        let (a, b) = (
            with_holds_numbered_afresh(expected).expect("sound"),
            with_holds_numbered_afresh(e.doc()).expect("sound"),
        );
        assert_eq!(
            first_difference(&a, &b),
            None,
            "a damaged step read back as another state"
        );
    }
    undone
}

fn older(store: &Store, e: &mut Editor, chain: &mut Chain) -> Option<pikslar_core::OlderStep> {
    let below = e.rewind_marks().below;
    chain
        .has_step(below)
        .then(|| chain.read_older(store, below, e.rewind_tile_room()).ok())?
}

fn log_of(store: &Store) -> PathBuf {
    let head = store.read_head(ID).expect("reads").expect("kept");
    store.log_path(ID, head.gen)
}

#[test]
fn a_sound_store_reads_every_step() {
    let (store, file, states) = sound("sound");
    assert_eq!(undo_all(&store, &file, &states), states.len() - 1);
}

#[test]
fn a_log_cut_anywhere_reads_the_steps_before_the_cut_and_nothing_wrong() {
    let (store, file, states) = sound("cut");
    let project = fs::read(&file).expect("saved");
    let log = log_of(&store);
    let whole = fs::read(&log).expect("log");
    let mut seen = Vec::new();
    for len in (0..whole.len()).step_by(7) {
        fs::write(&log, &whole[..len]).expect("cut");
        seen.push(undo_all(&store, &file, &states));
    }
    assert!(
        seen.iter().all(|&n| n < states.len() - 1),
        "a cut log read whole"
    );
    assert_eq!(
        fs::read(&file).expect("saved"),
        project,
        "the project file was touched"
    );
}

#[test]
fn a_flipped_bit_anywhere_in_the_log_is_found() {
    let (store, file, states) = sound("flip");
    let log = log_of(&store);
    let whole = fs::read(&log).expect("log");
    let mut rng = Rng::new(9);
    for _ in 0..600 {
        let mut damaged = whole.clone();
        let at = rng.below(damaged.len());
        damaged[at] ^= 1 << rng.below(8);
        fs::write(&log, &damaged).expect("flipped");
        let read = undo_all(&store, &file, &states);
        assert!(
            read < states.len() - 1,
            "byte {at} flipped and every step read"
        );
    }
}

#[test]
fn a_damaged_head_attaches_nothing_and_blocks_no_open() {
    let (store, file, states) = sound("head");
    let path = store.project_dir(ID).join("head");
    let whole = fs::read(&path).expect("head");
    let bytes = fs::read(&file).expect("saved");
    for at in 0..whole.len() {
        let mut damaged = whole.clone();
        damaged[at] ^= 0x40;
        fs::write(&path, &damaged).expect("flipped");
        let found = attach(&store, ID, pikslar_rewind::hash128(&bytes));
        assert!(matches!(found, Attach::Damaged(_)), "byte {at}: {found:?}");
        assert_eq!(undo_all(&store, &file, &states), 0);
    }
    for len in [0, 1, 50, 95, 97, 4096] {
        let mut cut = whole.clone();
        cut.resize(len, 7);
        fs::write(&path, &cut).expect("cut");
        let found = attach(&store, ID, pikslar_rewind::hash128(&bytes));
        assert!(matches!(found, Attach::Damaged(_)), "length {len}");
    }
    // A later save is refused rather than writing over what it cannot read.
    let job = Persist {
        id: ID,
        expect: None,
        copy_from: None,
        link: pikslar_rewind::write::Link::Fresh { base: 0 },
        records: Vec::new(),
        print: 1,
        budgets: Budgets::default(),
    };
    let refused = persist(&store, job, &mut |_| Ok(()));
    assert!(matches!(refused, Err(pikslar_rewind::RewindError::Changed)));
}

/// Replaces the tip record with one whose check holds but whose contents lie.
fn plant_tip(store: &Store, kind: u8, flags: u16, decoded: u32, stored: &[u8]) {
    let mut head = store.read_head(ID).expect("reads").expect("kept");
    let log = store.log_path(ID, head.gen);
    let mut bytes = fs::read(&log).expect("log");
    let start = bytes.len() as u64;
    let record = RecordHead {
        kind,
        flags,
        pos: head.tip + 1,
        prev_end: head.tip_end,
        decoded,
    };
    bytes.extend_from_slice(&frame::encode_record(start, record, stored));
    fs::write(&log, &bytes).expect("log");
    head.tip += 1;
    head.tip_end = bytes.len() as u64;
    fs::write(store.project_dir(ID).join("head"), head.encode()).expect("head");
}

#[test]
fn huge_lengths_with_a_sound_check_cost_little_memory() {
    let mut huge_count = vec![0, 0, 0, 0];
    huge_count.extend_from_slice(&u32::MAX.to_le_bytes());
    huge_count.extend_from_slice(&[1; 64]);
    let cases: [(u8, u16, u32, Vec<u8>); 4] = [
        (1, 0, huge_count.len() as u32, huge_count),
        (1, frame::DEFLATED, frame::MOST_DECODED, vec![0x55; 64]),
        (2, 0, 64, vec![0xff; 64]),
        (9, 0, 1, vec![1]),
    ];
    for (n, (kind, flags, decoded, stored)) in cases.into_iter().enumerate() {
        let (store, file, _) = sound(&format!("huge-{n}"));
        plant_tip(&store, kind, flags, decoded, &stored);
        let head = store.read_head(ID).expect("reads").expect("kept");
        let (read, usage) = measure(|| store.read_step(&head, head.tip_end, head.tip, ROOM));
        assert!(read.is_err(), "case {n} read");
        assert!(usage.peak < 32 << 20, "case {n} held {} bytes", usage.peak);
        let doc = piks::load(&fs::read(&file).expect("saved")).expect("opens");
        assert!(doc.document_id.get().is_some());
    }
    // A trailer claiming a record that starts before the log, or is larger than any kept.
    let (store, _, _) = sound("huge-trailer");
    let log = log_of(&store);
    let mut bytes = fs::read(&log).expect("log");
    let len = bytes.len();
    bytes[len - 16..len - 8].copy_from_slice(&0u64.to_le_bytes());
    fs::write(&log, &bytes).expect("log");
    let head = store.read_head(ID).expect("reads").expect("kept");
    let (read, usage) = measure(|| store.read_step(&head, head.tip_end, head.tip, ROOM));
    assert!(read.is_err());
    assert!(usage.peak < 32 << 20);
}

#[test]
fn a_history_under_the_wrong_id_or_position_is_refused() {
    let (store, file, states) = sound("ids");
    let bytes = fs::read(&file).expect("saved");
    let other = 0xbad;
    let (from, to) = (store.project_dir(ID), store.project_dir(other));
    fs::create_dir_all(&to).expect("made");
    for entry in fs::read_dir(&from).expect("listed").flatten() {
        fs::copy(entry.path(), to.join(entry.file_name())).expect("copied");
    }
    let found = attach(&store, other, pikslar_rewind::hash128(&bytes));
    assert!(matches!(found, Attach::Damaged(_)), "{found:?}");
    // The head names one position, the record at its end another.
    let mut head = store.read_head(ID).expect("reads").expect("kept");
    head.tip += 1;
    fs::write(from.join("head"), head.encode()).expect("head");
    assert_eq!(undo_all(&store, &file, &states), 0);
    assert!(store
        .read_step(&head, head.tip_end, head.tip, ROOM)
        .is_err());
}

/// Tiles a 40 by 40 document can name, as the program passes it.
const ROOM: usize = 196;

#[test]
fn a_small_record_that_inflates_huge_is_refused_before_it_is_inflated() {
    // Twelve million tiles that hold nothing: 60 MB that deflates to very little.
    let count: usize = 12_000_000;
    let mut payload = vec![0u8; 8 + count * 5];
    payload[4..8].copy_from_slice(&(count as u32).to_le_bytes());
    let packed = pikslar_format::deflate::deflate(&payload);
    let deflated = (frame::DEFLATED, payload.len() as u32, packed);
    // Within the inflate ratio, but more tiles than the document can have.
    let mut many = vec![0u8; 8 + 4000 * 5];
    many[4..8].copy_from_slice(&4000u32.to_le_bytes());
    // Twice the same tile.
    let mut twice = vec![0u8; 8 + 2 * 5];
    twice[4..8].copy_from_slice(&2u32.to_le_bytes());
    let cases = [
        deflated,
        (0, many.len() as u32, many),
        (0, twice.len() as u32, twice),
    ];
    for (n, (flags, decoded, stored)) in cases.into_iter().enumerate() {
        let (store, _, _) = sound(&format!("bomb-{n}"));
        plant_tip(&store, 1, flags, decoded, &stored);
        let head = store.read_head(ID).expect("reads").expect("kept");
        let (read, usage) = measure(|| store.read_step(&head, head.tip_end, head.tip, ROOM));
        assert!(read.is_err(), "case {n} read");
        assert!(usage.peak < 4 << 20, "case {n} held {} bytes", usage.peak);
    }
}

#[test]
fn a_whole_document_claiming_more_than_its_record_can_grow_to_is_refused_unread() {
    let (store, _, _) = sound("bomb-whole");
    plant_tip(&store, 2, frame::DEFLATED, 100 << 20, &[0x55; 64]);
    let head = store.read_head(ID).expect("reads").expect("kept");
    let (read, usage) = measure(|| store.read_step(&head, head.tip_end, head.tip, ROOM));
    assert!(read.is_err());
    assert!(usage.peak < 1 << 20, "held {} bytes", usage.peak);
}
