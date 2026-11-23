//! Rewind against the model: random edits with saves among them, then the file reopened in a
//! new session and undone through the stored steps, every state compared with the one the model
//! recorded, pixel for pixel. A second session goes back into the stored steps, draws, saves
//! and is reopened again, so appending, replacing a past and cutting to a budget are all met.

use super::{Counts, Model, Plan};
use crate::same::{first_difference, with_holds_numbered_afresh};
use pikslar_core::{rgba, Damage, Document, DocumentId, DrawCmd, Editor, StepCmd, Tool};
use pikslar_format::piks;
use pikslar_render::Compositor;
use pikslar_rewind::{attach, persist, plan, Attach, Budgets, Chain, Persist, Store};
use std::path::Path;

/// One save in this many steps.
const SAVE_ODDS: usize = 9;

/// One random run kept in a store at `dir`, with `budgets` for it. Any failure names the seed.
pub fn run_rewind(plan: Plan, seed: u64, dir: &Path, budgets: Budgets) -> Result<Counts, String> {
    let _ = std::fs::remove_dir_all(dir);
    let store = Store::at(dir);
    let id = u128::from(seed) + 1;
    let mut model = Model::new(plan, seed)?;
    model.editor.set_document_id(DocumentId::new(id));
    let mut chain = None;
    for step in 0..plan.steps {
        model
            .one_step()
            .map_err(|e| format!("seed {seed} step {step}: {e}"))?;
        if model.rng.chance(SAVE_ODDS) {
            save(&store, &mut model.editor, &mut chain, budgets)?;
        }
    }
    let first = settle(&store, &mut model.editor, &mut chain, budgets)?;
    let offset = model.at as i64 - model.editor.rewind_marks().depth as i64;
    let states: Vec<Document> = model.timeline[..=model.at]
        .iter()
        .map(|s| s.doc.clone())
        .collect();
    let (states, kept) = check_reopened(&store, &first, &states, offset, seed)?;
    second_session(&store, &first, states, kept, &mut model, budgets)
        .map_err(|e| format!("seed {seed} second session: {e}"))?;
    Ok(model.counts)
}

/// The last save of a session: the file and the chain it ends.
fn settle(
    store: &Store,
    editor: &mut Editor,
    chain: &mut Option<Chain>,
    budgets: Budgets,
) -> Result<Vec<u8>, String> {
    save(store, editor, chain, budgets)?;
    Ok(piks::save(editor.doc()))
}

/// A save with Rewind on, as the program makes it.
fn save(
    store: &Store,
    editor: &mut Editor,
    chain: &mut Option<Chain>,
    budgets: Budgets,
) -> Result<(), String> {
    let id = editor.doc().document_id.get().ok_or("no id")?;
    let steps = plan(editor, chain.as_ref(), budgets.keep).map_err(|e| format!("plan: {e:?}"))?;
    let print = pikslar_rewind::hash128(&piks::save(editor.doc()));
    let job = Persist {
        id,
        expect: chain.as_ref().map(|c| c.head),
        copy_from: None,
        link: steps.link,
        records: steps.records,
        print,
        budgets,
    };
    let written = persist(store, job, &mut |_| Ok(())).map_err(|e| format!("persist: {e}"))?;
    editor.clear_rewind_rewrites();
    match chain {
        Some(c) => c.take(&written),
        None => *chain = Some(Chain::written(&written)),
    }
    Ok(())
}

/// Opens the file in a new session with its history attached.
fn reopen(store: &Store, bytes: &[u8]) -> Result<(Editor, Chain), String> {
    let doc = piks::load(bytes).map_err(|e| format!("own save will not open: {e}"))?;
    let id = doc.document_id.get().ok_or("the id was not kept")?;
    let mut editor = Editor::new(doc);
    match attach(store, id, pikslar_rewind::hash128(bytes)) {
        Attach::Attached(chain) => {
            editor.rebase_rewind(chain.head.tip);
            Ok((editor, chain))
        }
        other => Err(format!("did not attach: {other:?}")),
    }
}

/// Undoes one step, from the store when memory has none. False when none is kept.
fn undo(store: &Store, editor: &mut Editor, chain: &mut Chain) -> Result<bool, String> {
    while editor.history().undo_depth() == 0 {
        let below = editor.rewind_marks().below;
        if !chain.has_step(below) {
            return Ok(false);
        }
        let step = chain
            .read_older(store, below, editor.rewind_tile_room())
            .map_err(|e| format!("step {below} does not read: {e}"))?;
        editor
            .push_older(step)
            .map_err(|e| format!("step {below} refused: {e:?}"))?;
    }
    editor.apply(StepCmd::Undo).map_err(|e| format!("{e:?}"))?;
    Ok(true)
}

/// The two documents are the same file and draw the same picture.
fn same(expected: &Document, found: &Document, what: &str) -> Result<(), String> {
    let (a, b) = (
        with_holds_numbered_afresh(expected)?,
        with_holds_numbered_afresh(found)?,
    );
    if let Some(difference) = first_difference(&a, &b) {
        return Err(format!("{what}: {difference}"));
    }
    let picture = |doc: &Document| {
        let mut compositor = Compositor::new(doc.width, doc.height);
        compositor.update(&Editor::new(doc.clone()), Damage::All, &mut Vec::new());
        compositor.pixels().to_vec()
    };
    match picture(expected) == picture(found) {
        true => Ok(()),
        false => Err(format!("{what}: the picture differs")),
    }
}

/// Reopens the file and undoes through every stored step, each state compared with the one
/// recorded at its position (`states[pos + offset]`, where `offset` may be below zero), then redoes back. Returns the states that
/// session can reach, oldest first, and how many of them came from the store.
fn check_reopened(
    store: &Store,
    bytes: &[u8],
    states: &[Document],
    offset: i64,
    seed: u64,
) -> Result<(Vec<Document>, usize), String> {
    let (mut editor, mut chain) = reopen(store, bytes).map_err(|e| format!("seed {seed}: {e}"))?;
    let head = chain.head;
    let mut undone = 0;
    while undo(store, &mut editor, &mut chain).map_err(|e| format!("seed {seed}: {e}"))? {
        undone += 1;
        let pos = head.tip - undone;
        let expected = &states[(pos as i64 + offset) as usize];
        same(
            expected,
            editor.doc(),
            &format!("seed {seed} undone to {pos}"),
        )?;
    }
    if undone != head.tip - head.base {
        return Err(format!(
            "seed {seed}: {undone} of {} undone",
            head.tip - head.base
        ));
    }
    for _ in 0..undone {
        editor.apply(StepCmd::Redo).map_err(|e| format!("{e:?}"))?;
    }
    same(states.last().ok_or("no state")?, editor.doc(), "redone")?;
    let from = (head.base as i64 + offset) as usize;
    Ok((states[from..].to_vec(), undone as usize))
}

/// Goes back into the stored steps, draws, saves, and checks the file reopened once more.
fn second_session(
    store: &Store,
    bytes: &[u8],
    mut states: Vec<Document>,
    kept: usize,
    model: &mut Model,
    budgets: Budgets,
) -> Result<(), String> {
    let (mut editor, chain) = reopen(store, bytes)?;
    let mut chain = Some(chain);
    let back = model.rng.below(kept + 1);
    for _ in 0..back {
        let c = chain.as_mut().ok_or("no chain")?;
        if !undo(store, &mut editor, c)? {
            return Err("a step that read before does not now".to_owned());
        }
        states.pop();
    }
    model.counts.undos += back as u64;
    for _ in 0..model.rng.below(4) {
        let before = editor.history().pushes();
        stroke(&mut editor, model);
        if editor.history().pushes() != before {
            states.push(editor.doc().clone());
            model.counts.steps += 1;
        }
    }
    let file = settle(store, &mut editor, &mut chain, budgets)?;
    let head = chain.ok_or("no chain")?.head;
    // States are kept from the chain's base; `states` starts at the old base.
    let offset = states.len() as i64 - 1 - head.tip as i64;
    check_reopened(store, &file, &states, offset, 0).map(|_| ())
}

fn stroke(editor: &mut Editor, model: &mut Model) {
    let (layers, frames) = (editor.doc().layers().len(), editor.doc().frames.len());
    editor.layer = model.rng.below(layers);
    editor.frame = model.rng.below(frames);
    editor.color = rgba(model.rng.next_u64() as u8, 40, 90, 255);
    let (x, y) = model.point();
    let _ = editor.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x,
        y,
    });
    let _ = editor.apply(DrawCmd::Commit);
}
