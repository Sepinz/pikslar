//! Runs of frames moved, duplicated and tagged at random: every frame lands where the map says,
//! tags always cover frames that exist, and each change is one step that undoes exactly.

use crate::command::{FrameCmd, StepCmd, TagCmd};
use crate::document::{ColorMode, Document};
use crate::editor::Editor;
use crate::frame_block::after_block_move;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// Up to 8 frames timed 1, 2, 3 ... so each frame can be followed, and up to 3 tags.
fn editor(rng: &mut Rng) -> Editor {
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
    let frames = 1 + rng.below(8);
    for at in 1..frames {
        doc.add_frame(at, false).expect("room");
    }
    for (i, f) in doc.frames.iter_mut().enumerate() {
        f.duration_ms = i as u32 + 1;
    }
    for _ in 0..rng.below(4) {
        let (a, b) = (rng.below(frames), rng.below(frames));
        doc.add_tag("t", a.min(b), a.max(b)).expect("tag");
    }
    Editor::new(doc)
}

fn times(doc: &Document) -> Vec<u32> {
    doc.frames.iter().map(|f| f.duration_ms).collect()
}

fn tags_cover_frames(doc: &Document, what: &str) {
    for tag in &doc.tags {
        assert!(
            tag.from <= tag.to && tag.to < doc.frames.len(),
            "{what}: {tag:?} of {}",
            doc.frames.len()
        );
    }
}

/// A random run command, which may not fit.
fn command(rng: &mut Rng, doc: &Document) -> FrameCmd {
    let len = doc.frames.len();
    let from = rng.below(len);
    let to = from + rng.below(len - from);
    match rng.below(2) {
        0 => FrameCmd::Duplicate { from, to },
        _ => FrameCmd::MoveRun {
            from,
            to,
            at: rng.below(len + 1),
        },
    }
}

#[test]
fn every_run_change_lands_where_the_map_says_and_undoes() {
    let mut made = 0;
    for seed in 1..=2_000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut e = editor(&mut rng);
        for step in 0..4 {
            let c = command(&mut rng, &e.doc);
            let what = format!("seed {seed} step {step}: {c:?}");
            let (before, depth) = (e.doc.clone(), e.history().undo_depth());
            let Ok(outcome) = e.apply(c.clone()) else {
                assert_eq!(e.doc, before, "{what}: refused, unchanged");
                continue;
            };
            made += 1;
            tags_cover_frames(&e.doc, &what);
            let old = times(&before);
            match c {
                FrameCmd::MoveRun { from, to, at } => {
                    for (f, t) in old.iter().enumerate() {
                        let went = after_block_move(f, (from, to), at);
                        assert_eq!(e.doc.frames[went].duration_ms, *t, "{what}");
                    }
                }
                FrameCmd::Duplicate { from, to } => {
                    let mut want = old.clone();
                    want.splice(to + 1..to + 1, old[from..=to].iter().copied());
                    assert_eq!(times(&e.doc), want, "{what}");
                }
                _ => unreachable!(),
            }
            if e.doc != before {
                assert_eq!(e.history().undo_depth(), depth + 1, "{what}");
                let after = e.doc.clone();
                e.apply(StepCmd::Undo).expect("undo");
                assert_eq!(e.doc, before, "{what}: undo");
                e.apply(StepCmd::Redo).expect("redo");
                assert_eq!(e.doc, after, "{what}: redo");
            } else {
                assert!(!outcome.changed, "{what}");
            }
        }
    }
    assert!(made > 5_000, "{made}");
}

#[test]
fn duplicating_keeps_the_current_frame_on_its_drawing() {
    let mut rng = Rng(3);
    let mut e = editor(&mut rng);
    while e.doc.frames.len() < 6 {
        let at = e.doc.frames.len();
        e.doc.add_frame(at, false).expect("room");
    }
    for (current, want) in [(0, 0), (1, 3), (2, 4), (3, 5), (5, 7)] {
        let mut e = Editor::new(e.doc.clone());
        e.frame = current;
        e.apply(FrameCmd::Duplicate { from: 1, to: 2 })
            .expect("fits");
        assert_eq!(e.frame, want, "from frame {current}");
    }
}

#[test]
fn a_tag_range_is_one_step() {
    let mut rng = Rng(7);
    let mut e = editor(&mut rng);
    e.doc.add_frame(1, false).expect("room");
    e.doc.add_tag("walk", 0, 0).expect("tag");
    let last = e.doc.tags.len() - 1;
    e.apply(TagCmd::SetRange {
        tag: last,
        from: 0,
        to: 1,
    })
    .expect("valid");
    assert_eq!((e.doc.tags[last].from, e.doc.tags[last].to), (0, 1));
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!((e.doc.tags[last].from, e.doc.tags[last].to), (0, 0));
}
