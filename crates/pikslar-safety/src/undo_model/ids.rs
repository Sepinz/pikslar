//! Persisted ids through random edits, undo and redo, and saving: every state undo or redo
//! comes back to has the layer and frame ids it had, ids in one document are distinct, the
//! floors never go down, and a saved file opens with the same ids and floors.

use super::{bad, State};
use pikslar_core::{ColorMode, Document, Editor, FrameCmd, FrameInfo, LayerCmd};
use std::collections::HashSet;

fn frame_ids(doc: &Document) -> Vec<u32> {
    doc.frames.iter().map(FrameInfo::id).collect()
}

fn layer_ids(doc: &Document) -> Vec<u32> {
    doc.layers()
        .iter()
        .map(|l| l.persisted_id().get())
        .collect()
}

fn distinct(ids: &[u32]) -> bool {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    sorted.windows(2).all(|pair| pair[0] != pair[1]) && sorted.first() != Some(&0)
}

/// `now` is the state `expected` was recorded as, back again; `recorded` is every state the
/// run has recorded, none of which may have a floor above what the document has now.
pub(super) fn check_ids(
    expected: &Document,
    now: &Document,
    recorded: &[State],
) -> Result<(), String> {
    let (frames, layers) = (frame_ids(now), layer_ids(now));
    if frames != frame_ids(expected) {
        return bad(format!(
            "frame ids {frames:?}, not {:?}",
            frame_ids(expected)
        ));
    }
    if layers != layer_ids(expected) {
        return bad(format!(
            "layer ids {layers:?}, not {:?}",
            layer_ids(expected)
        ));
    }
    if !distinct(&frames) || !distinct(&layers) {
        return bad(format!("ids repeat: frames {frames:?}, layers {layers:?}"));
    }
    let highest = |ids: &[u32]| ids.iter().copied().max().unwrap_or(0);
    if now.frame_floor() < highest(&frames) || now.layer_floor() < highest(&layers) {
        return bad("a floor is below an id in use".to_owned());
    }
    let lowered = recorded
        .iter()
        .map(|state| &state.doc)
        .any(|d| d.frame_floor() > now.frame_floor() || d.layer_floor() > now.layer_floor());
    if lowered {
        return bad("a floor went down".to_owned());
    }
    never_given_twice(now, recorded)
}

/// No id is ever given twice: going through the states in the order they were recorded, an id
/// no earlier state had is above the floor of the state before it. (A layer can keep its id
/// while becoming a new layer object, as merging down does, so ids are not tied to keys.)
fn never_given_twice(now: &Document, recorded: &[State]) -> Result<(), String> {
    let mut seen: (HashSet<u32>, HashSet<u32>) = Default::default();
    let mut floors: Option<(u32, u32)> = None;
    for doc in recorded.iter().map(|state| &state.doc).chain([now]) {
        for (kind, ids, seen, floor) in [
            ("frame", frame_ids(doc), &mut seen.0, floors.map(|f| f.0)),
            ("layer", layer_ids(doc), &mut seen.1, floors.map(|f| f.1)),
        ] {
            for id in ids {
                let fresh = seen.insert(id);
                if let Some(floor) = floor.filter(|&floor| fresh && id <= floor) {
                    return bad(format!(
                        "{kind} id {id} was given again, under floor {floor}"
                    ));
                }
            }
        }
        floors = Some((doc.frame_floor(), doc.layer_floor()));
    }
    Ok(())
}

/// A saved copy opens with every id and both floors as they are.
pub(super) fn check_saved_ids(now: &Document, back: &Document) -> Result<(), String> {
    if frame_ids(back) != frame_ids(now) || layer_ids(back) != layer_ids(now) {
        return bad(format!(
            "saving changed ids: frames {:?} to {:?}, layers {:?} to {:?}",
            frame_ids(now),
            frame_ids(back),
            layer_ids(now),
            layer_ids(back)
        ));
    }
    if (back.frame_floor(), back.layer_floor()) != (now.frame_floor(), now.layer_floor()) {
        return bad(format!(
            "saving changed the floors: {:?} to {:?}",
            (now.frame_floor(), now.layer_floor()),
            (back.frame_floor(), back.layer_floor())
        ));
    }
    after_reopening(now, back)
}

/// A frame and a layer added after reopening take ids above every id the run gave.
fn after_reopening(now: &Document, back: &Document) -> Result<(), String> {
    let mut editor = Editor::new(back.clone());
    let at = editor.doc().layers().len();
    let added = editor
        .apply(FrameCmd::Add { at: 0, copy: false })
        .and_then(|_| {
            editor.apply(LayerCmd::Add {
                at,
                mode: ColorMode::Rgba,
            })
        });
    if added.is_err() {
        // At a limit: nothing new to check.
        return Ok(());
    }
    let frame = editor.doc().frames[0].id();
    let layer = editor.doc().layers()[at].persisted_id().get();
    match frame > now.frame_floor() && layer > now.layer_floor() {
        true => Ok(()),
        false => bad(format!(
            "after reopening, frame {frame} and layer {layer} were given again"
        )),
    }
}
