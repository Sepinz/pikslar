//! What the store tests share: a fresh folder, an editor with history, and a save, a reopen
//! and an undo into stored steps as the program does them.
#![allow(dead_code)]

use pikslar_core::color::rgba;
use pikslar_core::{
    ColorMode, Document, DocumentId, DrawCmd, Editor, FrameCmd, LayerCmd, SelectionCmd, StepCmd,
    Tool,
};
use pikslar_rewind::{attach, persist, plan, Attach, Budgets, Chain, Persist, Store, Written};
use std::path::PathBuf;

/// An empty folder of its own for one test.
pub fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("rewind")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("made");
    dir
}

pub fn dot(e: &mut Editor, layer: usize, frame: usize, x: i32, color: u32) {
    (e.layer, e.frame, e.color) = (layer, frame, color);
    let begin = DrawCmd::Begin {
        tool: Tool::Pencil,
        x,
        y: x / 2,
    };
    e.apply(begin).expect("begin");
    e.apply(DrawCmd::Commit).expect("commit");
}

/// An editor with strokes, a frame and a layer added, and a selection: every kind of step.
pub fn busy(id: u128, strokes: usize) -> Editor {
    let mut e = Editor::new(Document::new(48, 48, ColorMode::Rgba).expect("valid"));
    e.set_document_id(DocumentId::new(id));
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    for n in 0..strokes {
        dot(
            &mut e,
            n % 2,
            n / 2 % 2,
            (n * 5 % 47) as i32,
            rgba(n as u8 * 13, 9, 200, 255),
        );
        if n == 3 {
            e.apply(SelectionCmd::All).expect("select");
        }
    }
    e
}

/// Every state undo passes from where the editor is, oldest first, the one it is in last.
pub fn states_down(e: &mut Editor) -> Vec<Document> {
    let mut states = vec![e.doc().clone()];
    while e.history().undo_depth() > 0 {
        e.apply(StepCmd::Undo).expect("undo");
        states.push(e.doc().clone());
    }
    for _ in 1..states.len() {
        e.apply(StepCmd::Redo).expect("redo");
    }
    states.reverse();
    states
}

/// The file the program saves and its print.
pub fn saved(e: &Editor) -> (Vec<u8>, u128) {
    let bytes = pikslar_format::piks::save(e.doc());
    let print = pikslar_rewind::hash128(&bytes);
    (bytes, print)
}

/// A save with Rewind on, as the program makes it. Returns what was written.
pub fn save(
    store: &Store,
    e: &mut Editor,
    chain: Option<&Chain>,
    budgets: Budgets,
    hook: pikslar_rewind::write::Hook<'_>,
) -> Result<Written, pikslar_rewind::RewindError> {
    let id = e.doc().document_id.get().expect("an id");
    let plan = plan(e, chain, budgets.keep).expect("plans");
    let (_, print) = saved(e);
    let job = Persist {
        id,
        expect: chain.map(|c| c.head),
        copy_from: None,
        link: plan.link,
        records: plan.records,
        print,
        budgets,
    };
    let written = persist(store, job, hook)?;
    e.clear_rewind_rewrites();
    Ok(written)
}

pub fn no_hook() -> impl FnMut(pikslar_rewind::Step) -> std::io::Result<()> {
    |_| Ok(())
}

/// Saves, keeping the chain as the program does.
pub fn save_ok(store: &Store, e: &mut Editor, chain: &mut Option<Chain>, budgets: Budgets) {
    let written = save(store, e, chain.as_ref(), budgets, &mut no_hook()).expect("written");
    match chain {
        Some(c) => c.take(&written),
        None => *chain = Some(Chain::written(&written)),
    }
}

/// Opens the saved file in a new session and attaches its history.
pub fn reopen(store: &Store, bytes: &[u8]) -> (Editor, Attach) {
    let doc = pikslar_format::piks::load(bytes).expect("opens");
    let id = doc.document_id.get().expect("an id");
    let mut e = Editor::new(doc);
    let found = attach(store, id, pikslar_rewind::hash128(bytes));
    if let Attach::Attached(chain) = &found {
        e.rebase_rewind(chain.head.tip);
    }
    (e, found)
}

/// Undo as the program does it: from memory, then from the store while steps are kept.
/// False once nothing is left, or a stored step does not read.
pub fn undo(store: &Store, e: &mut Editor, chain: &mut Chain) -> bool {
    loop {
        if e.history().undo_depth() > 0 {
            e.apply(StepCmd::Undo).expect("undo");
            return true;
        }
        let below = e.rewind_marks().below;
        if !chain.has_step(below) {
            return false;
        }
        let Ok(step) = chain.read_older(store, below, e.rewind_tile_room()) else {
            return false;
        };
        if e.push_older(step).is_err() {
            return false;
        }
    }
}
