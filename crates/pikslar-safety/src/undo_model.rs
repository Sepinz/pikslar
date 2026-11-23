//! Random edits checked against a remembered timeline of every state.
//!
//! The model knows nothing about how history works. It records what the document was after each
//! step, then walks backwards and forwards through undo and redo and compares.

use crate::deep_use::check_shape;
use crate::go;
use crate::mutate::Rng;
use crate::same::{first_difference, with_holds_numbered_afresh};
use pikslar_core::command::CanvasCmd;
use pikslar_core::{rgba, Clip, ColorMode, Damage, Document, Editor, Mask, SelectOp, Tool};
use pikslar_core::{BlendMode, Blending, Command, LimitError, TagCmd};
use pikslar_core::{DrawCmd, FloatingCmd, SelectionCmd};
use pikslar_core::{FrameCmd, LayerCmd, PaletteCmd, StepCmd};
use pikslar_format::piks;
use pikslar_render::Compositor;
use std::sync::Arc;

pub mod autotile;
mod autotile_footprint;
mod autotile_rules;
mod brushes;
mod counts;
mod drags;
mod floating;
mod groups;
mod ids;
mod links;
mod locks;
mod palette;
mod palette_tiles;
pub mod rewind;
mod slices;
mod tiles;
mod tiles_terrain;
mod timeline;
mod tween;

pub use counts::Counts;

const SIDE: usize = 32;
/// One step in this many also takes a tilemap action (M10).
const TILE_ODDS: usize = 7;
/// Scrubs to random places in history at the end of each run, before the last back to the end.
const SCRUBS: usize = 4;

/// The settings of one random run.
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    pub mode: ColorMode,
    pub steps: usize,
    /// History budget in bytes. `None` keeps the program's own.
    pub budget: Option<usize>,
}

#[derive(Clone)]
struct State {
    doc: Document,
    selection: Option<Arc<Mask>>,
}

impl State {
    fn of(editor: &Editor) -> State {
        State {
            doc: editor.doc().clone(),
            selection: editor.selection().cloned(),
        }
    }
}

struct Model {
    counts: Counts,
    editor: Editor,
    /// What each step left behind, oldest first.
    timeline: Vec<State>,
    /// False once a budget may have dropped old steps.
    every_step_kept: bool,
    /// Where in the timeline the document should be.
    at: usize,
    rng: Rng,
    compositor: Compositor,
}

fn bad(message: String) -> Result<(), String> {
    Err(message)
}

impl Model {
    fn new(plan: Plan, seed: u64) -> Result<Model, String> {
        let doc = tween::starting_document(SIDE, plan.mode, seed)?;
        let editor = match plan.budget {
            Some(budget) => Editor::with_undo_budget(doc, budget),
            None => Editor::new(doc),
        };
        Ok(Model {
            counts: Counts::default(),
            timeline: vec![State::of(&editor)],
            every_step_kept: plan.budget.is_none(),
            at: 0,
            rng: Rng::new(seed),
            compositor: Compositor::new(SIDE, SIDE),
            editor,
        })
    }

    fn point(&mut self) -> (i32, i32) {
        (
            self.rng.between(-2, SIDE as i32 + 1),
            self.rng.between(-2, SIDE as i32 + 1),
        )
    }

    fn compare(&self, step: &str) -> Result<(), String> {
        check_shape(self.editor.doc())?;
        let expected = &self.timeline[self.at];
        if let Some(what) = first_difference(&expected.doc, self.editor.doc()) {
            return bad(format!("after {step}: {what}"));
        }
        if let Err(what) = ids::check_ids(&expected.doc, self.editor.doc(), &self.timeline) {
            return bad(format!("after {step}: {what}"));
        }
        if expected.doc.colors.colors() != self.editor.doc().colors.colors() {
            return bad(format!("after {step}: colors left in the table"));
        }
        if expected.selection.as_deref() != self.editor.selection().map(|m| &**m) {
            return bad(format!("after {step}: the selection differs"));
        }
        self.check_place_and_stack(step)?;
        groups::check_groups(self.editor.doc())?;
        slices::check_slices(self.editor.doc())?;
        timeline::check_holds(self.editor.doc())
    }

    /// The undo stack must hold exactly the steps the model has counted.
    fn check_place_and_stack(&self, step: &str) -> Result<(), String> {
        let (editor, doc) = (&self.editor, self.editor.doc());
        if editor.layer >= doc.layers().len() || editor.frame >= doc.frames.len() {
            return bad(format!("after {step}: the place is outside the document"));
        }
        let history = editor.history();
        let (undo, redo) = (history.undo_depth(), history.redo_depth());
        if redo != self.timeline.len() - 1 - self.at {
            return bad(format!(
                "after {step}: {redo} redo steps, expected otherwise"
            ));
        }
        if undo > self.at || (self.every_step_kept && undo != self.at) {
            return bad(format!("after {step}: {undo} undo steps at {}", self.at));
        }
        Ok(())
    }

    /// The picture built up from damage must equal one drawn from nothing.
    fn compare_picture(&mut self, step: &str) -> Result<(), String> {
        let damage = self.editor.take_damage();
        let size = (self.editor.doc().width, self.editor.doc().height);
        // As the app does, a canvas of another size gets a compositor of its own size, which
        // the damage must then fill whole.
        if (self.compositor.width(), self.compositor.height()) != size {
            self.compositor = self.compositor.resized(size.0, size.1);
        }
        self.compositor
            .update(&self.editor, damage, &mut Vec::new());
        let mut fresh = Compositor::new(size.0, size.1);
        fresh.update(&self.editor, Damage::All, &mut Vec::new());
        if self.compositor.pixels() != fresh.pixels() {
            return bad(format!("after {step}: the redrawn picture is wrong"));
        }
        Ok(())
    }

    /// Moves to another layer and frame, as clicking in the timeline does.
    fn wander(&mut self) {
        self.editor.layer = self.rng.below(self.editor.doc().layers().len());
        self.editor.frame = self.rng.below(self.editor.doc().frames.len());
        self.editor.color = rgba(
            self.rng.next_u64() as u8,
            self.rng.next_u64() as u8,
            self.rng.next_u64() as u8,
            255,
        );
        self.editor.take_damage();
        self.compositor
            .update(&self.editor, Damage::All, &mut Vec::new());
    }

    /// Runs a change and records it if it became a step.
    fn edit(&mut self, name: &str, change: impl FnOnce(&mut Self)) -> Result<(), String> {
        let before = self.editor.history().pushes();
        let revision = self.editor.revision();
        change(self);
        go(&mut self.editor, StepCmd::EndDrag);
        if self.editor.history().pushes() != before {
            self.record_step(name, revision)?;
        } else {
            if self.editor.revision() != revision {
                return bad(format!(
                    "{name}: the document says it changed, with no step"
                ));
            }
            self.forget_redo_dropped_for_room();
            self.compare(name)?;
        }
        self.compare_picture(name)
    }

    /// A drag that came to nothing gives back what could be redone, unless history was over
    /// its budget while the drag went on: then that is the first thing dropped.
    fn forget_redo_dropped_for_room(&mut self) {
        if !self.every_step_kept && !self.editor.history().can_redo() {
            self.timeline.truncate(self.at + 1);
        }
    }

    fn record_step(&mut self, name: &str, revision: u64) -> Result<(), String> {
        let state = State::of(&self.editor);
        let previous = &self.timeline[self.at];
        // Two layers alike but for their ids, such as blank maps of one tileset, can swap.
        let ids = |d: &Document| d.layers().iter().map(|l| l.id()).collect::<Vec<_>>();
        let changed = first_difference(&previous.doc, &state.doc).is_some()
            || ids(&previous.doc) != ids(&state.doc);
        if !changed && previous.selection == state.selection {
            return bad(format!("{name}: a step that changed nothing"));
        }
        if changed && self.editor.revision() == revision {
            return bad(format!("{name}: changed the document without saying so"));
        }
        timeline::check_rate_change(&previous.doc, &state.doc)?;
        self.timeline.truncate(self.at + 1);
        self.timeline.push(state);
        self.at += 1;
        self.counts.steps += 1;
        self.check_place_and_stack(name)
    }

    /// A command turned down for memory must leave everything exactly as it found it.
    fn refused(&mut self) -> Result<(), String> {
        // A locked layer refuses first, which `toggle_lock` checks on its own, and so does a
        // drawing shared with one.
        let (layer, frame) = (self.editor.layer, self.editor.frame);
        let shared = self.editor.shared_refusal(layer, frame);
        if self.editor.paint_refusal(layer).or(shared).is_some() {
            return Ok(());
        }
        let before = (self.editor.layer, self.editor.frame, self.editor.revision());
        let (x, y) = self.point();
        let at = self.editor.frame;
        let layer = self.editor.layer_id(self.editor.layer).expect("layer");
        let mode = self.editor.doc().layers()[self.editor.layer].mode;
        let command: Command = match self.rng.below(6) {
            0 => FrameCmd::Add { at, copy: true }.into(),
            1 => LayerCmd::Add { at: 0, mode }.into(),
            2 => LayerCmd::Remove { layer }.into(),
            3 => SelectionCmd::Invert.into(),
            4 => DrawCmd::Begin {
                tool: Tool::Pencil,
                x,
                y,
            }
            .into(),
            _ => FloatingCmd::Paste {
                clip: std::sync::Arc::new(Clip::from_image(2, 2, vec![rgba(1, 2, 3, 255); 4])),
                x,
                y,
            }
            .into(),
        };
        self.editor.set_memory_check(|_| false);
        let result = self.editor.apply(command);
        self.editor.set_memory_check(|_| true);
        if result != Err(LimitError::NoMemory) {
            return bad(format!("a command without memory gave {result:?}"));
        }
        if before != (self.editor.layer, self.editor.frame, self.editor.revision()) {
            return bad("a refused command moved the place or revision".to_owned());
        }
        self.counts.refused += 1;
        self.compare("a refused command")
    }

    /// Showing or hiding a layer changes the file, but is never undone: every state keeps it.
    fn toggle_visibility(&mut self) -> Result<(), String> {
        let index = self.editor.layer;
        let (id, visible) = {
            let layer = &self.editor.doc().layers()[index];
            (layer.id(), !layer.visible)
        };
        let layer = self.editor.layer_id(index).expect("layer");
        let (pushes, revision) = (self.editor.history().pushes(), self.editor.revision());
        go(&mut self.editor, LayerCmd::SetVisible { layer, visible });
        if self.editor.history().pushes() != pushes {
            return bad("showing or hiding a layer became a step".to_owned());
        }
        if self.editor.revision() == revision {
            return bad("showing or hiding a layer did not mark the file changed".to_owned());
        }
        for layer in self.timeline.iter_mut().flat_map(|s| s.doc.layers_mut()) {
            if layer.id() == id {
                layer.visible = visible;
            }
        }
        self.compare("showing or hiding a layer")?;
        self.compare_picture("showing or hiding a layer")
    }

    fn change_selection(&mut self) {
        let (x, y) = self.point();
        let shape = Box::new(Mask::rect(x, y, 6, 6));
        let command = match self.rng.below(6) {
            0 => SelectionCmd::All,
            1 => SelectionCmd::None,
            2 => SelectionCmd::Invert,
            3 => SelectionCmd::Select {
                shape,
                op: SelectOp::Add,
            },
            4 => SelectionCmd::Select {
                shape,
                op: SelectOp::Subtract,
            },
            _ => SelectionCmd::Select {
                shape,
                op: SelectOp::Replace,
            },
        };
        go(&mut self.editor, command);
    }

    fn more_settings(&mut self) {
        let index = self.editor.layer;
        let layer = self.editor.layer_id(index).expect("layer");
        let current = &self.editor.doc().layers()[index];
        let (blend, mode) = (current.blend, current.mode);
        let last = self.editor.doc().layers().len() - 1;
        let tags = self.editor.doc().tags.len();
        let tag = self.rng.below(tags.max(1));
        let swatches = self.editor.doc().colors.swatches().to_vec();
        let command: Command = match self.rng.below(8) {
            0 => TagCmd::Add {
                name: "x".into(),
                from: 0,
                to: 0,
            }
            .into(),
            1 => {
                // Any other mode, so the step always changes something.
                let n = usize::from(blend.number()) + 1 + self.rng.below(8);
                let blend = BlendMode::ALL[n % BlendMode::ALL.len()];
                LayerCmd::SetBlend { layer, blend }.into()
            }
            2 => {
                let to = self.rng.below(last + 1);
                LayerCmd::Move { layer, to }.into()
            }
            3 => {
                let mode = match mode {
                    ColorMode::Rgba => ColorMode::Palette,
                    ColorMode::Palette => ColorMode::Rgba,
                };
                LayerCmd::SetMode { layer, mode }.into()
            }
            4 => TagCmd::Remove { tag }.into(),
            5 if self.rng.below(2) == 0 => TagCmd::CycleDirection { tag }.into(),
            5 => {
                let repeat = self.rng.below(4) as u16;
                TagCmd::SetRepeat { tag, repeat }.into()
            }
            6 => TagCmd::SetName {
                tag,
                name: "named".into(),
            }
            .into(),
            _ => match swatches.first() {
                Some(&id) => PaletteCmd::RemoveSwatch { id }.into(),
                None => DrawCmd::DeleteSelected.into(),
            },
        };
        go(&mut self.editor, command);
    }

    fn stroke(&mut self, tool: Tool) {
        let (a, b) = (self.point(), self.point());
        if self
            .editor
            .apply(DrawCmd::Begin {
                tool,
                x: a.0,
                y: a.1,
            })
            .is_ok()
        {
            go(&mut self.editor, DrawCmd::To { x: b.0, y: b.1 });
            go(&mut self.editor, DrawCmd::Commit);
        }
    }

    fn structure(&mut self) {
        let (layer, frame) = (self.editor.layer, self.editor.frame);
        let (a, b) = (
            self.rng.below(self.editor.doc().frames.len()),
            self.rng.below(self.editor.doc().frames.len()),
        );
        let (from, to) = (a.min(b), a.max(b));
        let canvas = self.another_canvas();
        let name = format!("L{}", self.rng.below(4));
        let e = &mut self.editor;
        let _ = match self.rng.below(16) {
            0 => e.apply(FrameCmd::Add {
                at: frame,
                copy: true,
            }),
            1 => e.apply(FrameCmd::Add {
                at: frame,
                copy: false,
            }),
            2 => e.apply(LayerCmd::Add {
                at: layer,
                mode: ColorMode::Rgba,
            }),
            3 => e.apply(FrameCmd::Remove {
                from: frame,
                to: frame,
            }),
            4 => e.apply(LayerCmd::Remove {
                layer: e.layer_id(layer).expect("layer"),
            }),
            5 => e.apply(FrameCmd::HoldPrevious {
                layer: e.layer_id(layer).expect("layer"),
                frame,
            }),
            6 => e.apply(FrameCmd::HoldAcross {
                layer: e.layer_id(layer).expect("layer"),
                source: from,
                from,
                to,
            }),
            7 => e.apply(FrameCmd::Release {
                layer: e.layer_id(layer).expect("layer"),
                from: frame,
                to: frame,
            }),
            8 => e.apply(FrameCmd::Move { from, to }),
            9 => e.apply(FrameCmd::Remove { from, to }),
            10 => e.apply(LayerCmd::PlaceReference {
                below: layer,
                image: std::sync::Arc::new(e.doc().layers()[layer].cel(frame).image().clone()),
                name,
            }),
            11 | 12 => e.apply(canvas),
            13 => e.apply(LayerCmd::Duplicate {
                layer: e.layer_id(layer).expect("layer"),
            }),
            14 => e.apply(LayerCmd::MergeDown {
                layer: e.layer_id(layer).expect("layer"),
            }),
            15 => e.apply(LayerCmd::Rename {
                layer: e.layer_id(layer).expect("layer"),
                name,
            }),
            _ => e.apply(pikslar_core::TagCmd::Add {
                name: "t".into(),
                from,
                to,
            }),
        };
    }

    /// A resize or a crop, around the size the model starts with.
    fn another_canvas(&mut self) -> CanvasCmd {
        let (width, height) = (1 + self.rng.below(2 * SIDE), 1 + self.rng.below(2 * SIDE));
        let (dx, dy) = (self.rng.between(-8, 8), self.rng.between(-8, 8));
        if self.rng.chance(2) {
            return pikslar_core::command::crop_to(-dx, -dy, width, height);
        }
        CanvasCmd::Resize {
            width,
            height,
            dx,
            dy,
        }
    }

    fn settings(&mut self) {
        let (layer, frame) = (self.editor.layer, self.editor.frame);
        let color = self.editor.color;
        match self.rng.below(5) {
            0 => self
                .editor
                .apply(LayerCmd::SetOpacity {
                    layer: self.editor.layer_id(layer).expect("layer"),
                    opacity: self.rng.next_u64() as u8,
                })
                .map(|_| ())
                .unwrap_or(()),
            1 => self
                .editor
                .apply(FrameCmd::Retime {
                    from: frame,
                    durations: vec![10 + self.rng.below(900) as u32],
                })
                .map(|_| ())
                .unwrap_or(()),
            2 => {
                let _ = self.editor.apply(PaletteCmd::AddSwatch { color });
            }
            3 => {
                // The other blending, so the step always changes how the layers combine.
                let blending = match self.editor.doc().blending {
                    Blending::Pikslar => Blending::Aseprite,
                    Blending::Aseprite => Blending::Pikslar,
                };
                if self
                    .editor
                    .apply(CanvasCmd::SetBlending { blending })
                    .is_ok()
                {
                    self.counts.blendings += 1;
                }
            }
            _ => {
                let id = 1 + self.rng.below(self.editor.doc().colors.len());
                go(&mut self.editor, StepCmd::EndDrag);
                let _ = self.editor.apply(PaletteCmd::Recolor {
                    id: id as u32,
                    color,
                });
            }
        }
    }

    fn undo(&mut self) -> Result<(), String> {
        let did = self.editor.apply(StepCmd::Undo).expect("undo").changed;
        match (did, self.at) {
            (true, 0) => return bad("undo did something at the very start".to_owned()),
            (true, _) => {
                self.at -= 1;
                self.counts.undos += 1;
            }
            (false, 0) => {}
            // Older steps may have been dropped to stay within the budget.
            (false, _) if self.editor.history().undo_depth() == 0 => {
                self.timeline.drain(..self.at);
                self.every_step_kept = false;
                self.at = 0;
                self.counts.dropped += 1;
            }
            (false, _) => return bad("undo refused with steps left".to_owned()),
        }
        self.compare("undo")?;
        self.compare_picture("undo")
    }

    fn redo(&mut self) -> Result<(), String> {
        let did = self.editor.apply(StepCmd::Redo).expect("redo").changed;
        let last = self.timeline.len() - 1;
        match (did, self.at == last) {
            (true, true) => return bad("redo did something at the end".to_owned()),
            (true, false) => {
                self.at += 1;
                self.counts.redos += 1;
            }
            (false, false) if self.editor.history().can_redo() => {
                return bad("redo refused with steps left".to_owned());
            }
            (false, false) => return bad("the redo steps vanished".to_owned()),
            (false, true) => {}
        }
        self.compare("redo")?;
        self.compare_picture("redo")
    }

    fn compare_with(&self, expected: &Document, step: &str) -> Result<(), String> {
        match first_difference(expected, self.editor.doc()) {
            Some(what) => bad(format!("after {step}: {what}")),
            None => Ok(()),
        }
    }

    /// One random edit, then now and then a tilemap action as well. The tilemap actions come on
    /// top so that every other kind of edit keeps its share of a run.
    fn one_step(&mut self) -> Result<(), String> {
        self.one_edit()?;
        match self.rng.chance(TILE_ODDS) {
            true => self.tile_step(),
            false => Ok(()),
        }
    }

    fn one_edit(&mut self) -> Result<(), String> {
        self.wander();
        match self.rng.below(41) {
            37..=40 => self.palette_step(),
            27..=36 => self.brush_step(),
            25 | 26 => self.link_step(),
            24 => self.edit("tween", Self::tween),
            22 | 23 => self.slice_step(),
            21 => self.toggle_fold(),
            20 => self.edit("groups", Self::grouping),
            19 => self.toggle_lock(),
            18 => self.edit("timeline drag", Self::timeline_drag),
            16 => self.edit("drag", Self::drag),
            17 => self.edit("turn", Self::turn),
            15 => self.toggle_visibility(),
            12 => self.edit("selection", Self::change_selection),
            13 => self.edit("layer and tag settings", Self::more_settings),
            14 => self.refused(),
            0 => self.edit("pencil", |m| m.stroke(Tool::Pencil)),
            1 => self.edit("eraser", |m| m.stroke(Tool::Eraser)),
            2 => self.edit("line", |m| m.stroke(Tool::Line)),
            3 => self.edit("rectangle", |m| m.stroke(Tool::Rect)),
            4 => self.edit("fill", |m| m.stroke(Tool::Fill)),
            5 => self.edit("structure", Self::structure),
            6 => self.edit("settings", Self::settings),
            7 => self.floating(),
            8 | 9 => self.undo(),
            _ => self.redo(),
        }
    }

    /// Undoes to the start and redoes to the end, comparing every state on the way.
    fn walk_whole_timeline(&mut self) -> Result<(), String> {
        while self.at > 0 && self.editor.history().can_undo() {
            self.undo()?;
        }
        // Whatever could not be undone must be what the budget dropped, and nothing else.
        if self.at > 0 {
            self.counts.dropped += 1;
        }
        while self.editor.history().can_redo() {
            self.redo()?;
        }
        Ok(())
    }

    /// Scrubs to a few places in history and back to the end, as dragging along the history
    /// list does, comparing each place with the state recorded there and the picture.
    fn scrub_timeline(&mut self) -> Result<(), String> {
        for _ in 0..SCRUBS {
            let history = self.editor.history();
            let kept = history.undo_depth() + history.redo_depth();
            let depth = self.rng.below(kept + 1);
            self.scrub(depth)?;
        }
        let history = self.editor.history();
        self.scrub(history.undo_depth() + history.redo_depth())
    }

    fn scrub(&mut self, depth: usize) -> Result<(), String> {
        let now = self.editor.history().undo_depth();
        let did = self
            .editor
            .apply(StepCmd::ScrubTo { depth })
            .expect("scrub")
            .changed;
        if did != (depth != now) || self.editor.history().undo_depth() != depth {
            return bad(format!("a scrub from {now} to {depth} went elsewhere"));
        }
        // What the budget dropped lies before every step history keeps.
        self.at = self.at - now + depth;
        self.counts.scrubs += u64::from(did);
        self.counts.scrubbed_steps += now.abs_diff(depth) as u64;
        self.compare("scrub")?;
        self.compare_picture("scrub")
    }

    fn check_save(&self) -> Result<(), String> {
        let saved = piks::save(self.editor.doc());
        let back = piks::load(&saved).map_err(|e| format!("own save will not open: {e}"))?;
        ids::check_saved_ids(self.editor.doc(), &back)?;
        let back = with_holds_numbered_afresh(&back).map_err(|e| format!("own save {e}"))?;
        let now = with_holds_numbered_afresh(self.editor.doc())?;
        match first_difference(&back, &now) {
            Some(what) => bad(format!("save changed: {what}")),
            None => Ok(()),
        }
    }
}

/// One random run. Any failure names what went wrong, and the seed reproduces it.
pub fn run(plan: Plan, seed: u64) -> Result<Counts, String> {
    run_saved(plan, seed).map(|(counts, _)| counts)
}

/// A run that also gives the project file its last document makes, so that two runs of the
/// same input can be compared byte for byte.
pub fn run_saved(plan: Plan, seed: u64) -> Result<(Counts, Vec<u8>), String> {
    let mut model = Model::new(plan, seed)?;
    for step in 0..plan.steps {
        model
            .one_step()
            .map_err(|e| format!("seed {seed} step {step}: {e}"))?;
    }
    model
        .check_save()
        .map_err(|e| format!("seed {seed} save: {e}"))?;
    model
        .walk_whole_timeline()
        .map_err(|e| format!("seed {seed} final walk: {e}"))?;
    model
        .scrub_timeline()
        .map_err(|e| format!("seed {seed} scrub: {e}"))?;
    Ok((model.counts, piks::save(model.editor.doc())))
}
