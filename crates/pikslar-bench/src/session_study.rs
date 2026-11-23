//! A long session of frame changes, then every step undone and redone, for
//! `pikslar-bench history-study`. Outside the gate's baseline.

use crate::history_study::{kept_beyond_document, long_document};
use pikslar_core::{rgba, CelCmd, Command, Document, DrawCmd, Editor, FrameCmd, LayerCmd};
use pikslar_core::{StepCmd, Tool, Turn};
use pikslar_format::piks;
use std::time::Instant;

/// Steps through the same pseudo-random numbers on every run and build.
struct Numbers(u64);

impl Numbers {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

/// One of the frame changes an animator makes most.
fn frame_change(e: &Editor, turn: usize, n: &mut Numbers) -> Command {
    let frames = e.doc().frames.len();
    let (a, b) = (n.below(frames), n.below(frames));
    let layer = e.layer_id(n.below(e.doc().layers().len())).expect("layer");
    match turn % 6 {
        0 => FrameCmd::Add { at: a, copy: true }.into(),
        1 => FrameCmd::Add { at: a, copy: false }.into(),
        2 => FrameCmd::Move { from: a, to: b }.into(),
        3 => FrameCmd::Remove { from: a, to: a }.into(),
        4 => FrameCmd::Retime {
            from: a,
            durations: vec![40 + 10 * n.below(20) as u32],
        }
        .into(),
        _ => FrameCmd::HoldPrevious {
            layer,
            frame: a.max(1),
        }
        .into(),
    }
}

fn millis(f: impl FnOnce()) -> f64 {
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64() * 1e3
}

/// Makes `changes` frame changes in a row, then scrubs to the start and back to the end.
fn session(layers: usize, frames: usize, changes: usize) {
    let saved = piks::save(&long_document(layers, frames));
    let fresh = || Editor::new(piks::load(&saved).expect("own save"));
    let base = kept_beyond_document(fresh());
    let mut e = fresh();
    let mut n = Numbers(20261112);
    let apply = millis(|| {
        for turn in 0..changes {
            let command = frame_change(&e, turn, &mut n);
            let _ = e.apply(command);
            e.apply(StepCmd::EndDrag).expect("end");
        }
    });
    let depth = e.history().undo_depth();
    let said = e.history().memory_used();
    let back = millis(|| {
        e.apply(StepCmd::ScrubTo { depth: 0 }).expect("scrub");
    });
    let forth = millis(|| {
        e.apply(StepCmd::ScrubTo { depth }).expect("scrub");
    });
    let kept = kept_beyond_document(e).saturating_sub(base);
    println!(
        "session_{layers}x{frames}_{changes}_frame_changes  steps {depth:>4}  said {:>8.1} MB  kept {:>8.1} MB  apply {:>6.1} ms  undo all {:>6.1} ms  redo all {:>6.1} ms",
        said as f64 / 1e6,
        kept as f64 / 1e6,
        apply,
        back,
        forth
    );
}

/// Any of the commands an animator makes most, frame changes among them.
fn any_change(e: &mut Editor, turn: usize, n: &mut Numbers) {
    let layer = e.layer_id(e.layer).expect("layer");
    let frame = e.frame;
    let (x, y) = (n.below(256) as i32, n.below(256) as i32);
    let command: Command = match n.below(8) {
        0..=2 => DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: (0..12).map(|i| (x + i, y + i / 3)).collect(),
        }
        .into(),
        3 => CelCmd::Move {
            layer,
            frame,
            dx: x - 128,
            dy: y - 128,
        }
        .into(),
        4 => CelCmd::Turn {
            layer,
            frame,
            turn: Turn::FlipHorizontal,
        }
        .into(),
        5 => LayerCmd::SetOpacity {
            layer,
            opacity: x as u8,
        }
        .into(),
        _ => frame_change(e, turn, n),
    };
    e.color = rgba(x as u8, y as u8, 90, 255);
    let _ = e.apply(command);
    e.apply(StepCmd::EndDrag).expect("end");
    e.layer = n.below(e.doc().layers().len());
    e.frame = n.below(e.doc().frames.len());
}

/// What history says it keeps after a mixed session, against what it keeps: with every step
/// done, and again with every step undone, when history keeps what redo puts back. The editor's
/// own working memory, such as the overlay's tiles, is measured on the same session with every
/// step undone and dropped, and taken off.
fn charges(steps: usize, seed: u64) {
    let saved = piks::save(&long_document(12, 2000));
    let fresh = || Editor::new(piks::load(&saved).expect("own save"));
    let base = kept_beyond_document(fresh());
    let base_said = fresh().history().memory_used();
    let session = || {
        let mut e = fresh();
        let mut n = Numbers(seed);
        for turn in 0..steps {
            any_change(&mut e, turn, &mut n);
        }
        e
    };
    let work = working_memory(session(), base, base_said);
    let ratio = |e: Editor| {
        let said = e.history().memory_used().saturating_sub(base_said) as f64;
        let kept = kept_beyond_document(e).saturating_sub(base + work) as f64;
        (said / 1e6, said / kept)
    };
    let (done, done_ratio) = ratio(session());
    let mut e = session();
    e.apply(StepCmd::ScrubTo { depth: 0 }).expect("scrub");
    let (undone, undone_ratio) = ratio(e);
    println!(
        "charges_12x2000_{steps}_mixed_steps_seed_{seed}  done said {done:>5.2} MB  said/kept {done_ratio:.3}  undone said {undone:>5.2} MB  said/kept {undone_ratio:.3}  work {:.2} MB",
        work as f64 / 1e6
    );
}

/// What the editor keeps for its own work after a session: the same session with every step
/// undone, then dropped by a new step. History's lists keep their room once emptied, and that
/// room is history's, not work.
fn working_memory(mut e: Editor, base: usize, base_said: usize) -> usize {
    e.apply(StepCmd::ScrubTo { depth: 0 }).expect("scrub");
    let layer = e.layer_id(0).expect("layer");
    let moved = CelCmd::Move {
        layer,
        frame: 0,
        dx: 1,
        dy: 0,
    };
    e.apply(moved).expect("move");
    // A cel move is a dragged value, which sets redo aside until it ends.
    e.apply(StepCmd::EndDrag).expect("end");
    assert!(!e.history().can_redo(), "every step is gone");
    let lists = e.history().memory_used().saturating_sub(base_said);
    kept_beyond_document(e).saturating_sub(base + lists)
}

/// Steps between the copies a long session is compared with.
const CHECK_EVERY: usize = 100;

/// A long mixed session, then undone to the start and redone to the end one step at a time.
/// Every hundredth state is compared with a copy taken when it was first reached: a copy of
/// every state would itself hold gigabytes of old cel lists.
fn long_session(steps: usize) {
    let mut e = Editor::new(long_document(12, 2000));
    let mut n = Numbers(11);
    let mut copies: Vec<(usize, Document)> = vec![(0, e.doc().clone())];
    let mut recorded = 0;
    let start = Instant::now();
    for turn in 0..steps {
        let pushes = e.history().pushes();
        any_change(&mut e, turn, &mut n);
        if e.history().pushes() != pushes {
            recorded += 1;
            if recorded % CHECK_EVERY == 0 {
                copies.push((recorded, e.doc().clone()));
            }
        }
    }
    let made = start.elapsed().as_secs_f64();
    let kept = e.history().undo_depth();
    let said = e.history().memory_used();
    let first = recorded - kept;
    let mut compared = 0;
    let mut check = |e: &Editor, at: usize| {
        if let Some((_, copy)) = copies.iter().find(|(depth, _)| *depth == at) {
            assert!(e.doc() == copy, "state {at} differs");
            compared += 1;
        }
    };
    let back = millis(|| {
        for at in (first..recorded).rev() {
            e.apply(StepCmd::Undo).expect("undo");
            check(&e, at);
        }
    });
    let forth = millis(|| {
        for at in first + 1..=recorded {
            e.apply(StepCmd::Redo).expect("redo");
            check(&e, at);
        }
    });
    println!(
        "long_session_12x2000  steps {recorded:>5}  kept {kept:>5}  compared {compared:>3}, all equal  made {made:>5.1} s  said {:>7.1} MB  undo all {back:>7.1} ms  redo all {forth:>7.1} ms",
        said as f64 / 1e6
    );
}

/// Frame changes while something outside history holds a copy of the document, as the
/// recovery writer does while it saves.
fn with_a_copy_held() {
    let saved = piks::save(&long_document(12, 2000));
    let changes: [(&str, Command); 3] = [
        (
            "add",
            FrameCmd::Add {
                at: 1000,
                copy: false,
            }
            .into(),
        ),
        (
            "remove",
            FrameCmd::Remove {
                from: 1000,
                to: 1000,
            }
            .into(),
        ),
        (
            "move_first_to_last",
            FrameCmd::Move { from: 0, to: 1999 }.into(),
        ),
    ];
    for (name, command) in changes {
        let mut times = Vec::new();
        for _ in 0..9 {
            let mut e = Editor::new(piks::load(&saved).expect("own save"));
            let copy = e.doc().clone();
            times.push(
                millis(|| {
                    e.apply(command.clone()).expect("applies");
                }) * 1e3,
            );
            drop(copy);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "copy_held_12x2000_frame_{name:<20} apply {:>7.1} us",
            times[4]
        );
    }
}

pub fn run() {
    println!("--- sessions");
    session(12, 2000, 500);
    session(4, 4000, 500);
    with_a_copy_held();
    for seed in [7, 11, 23, 41] {
        charges(1000, seed);
    }
    long_session(10_000);
}
