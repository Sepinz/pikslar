//! The budgets of one run: the cells it may give back and the pixels it may work on.

use super::*;
use pikslar_api::run_ops;
use pikslar_core::{ColorMode, Document};

/// A blank canvas, its one layer and its one frame.
fn canvas(w: usize, h: usize) -> (Editor, i64, i64) {
    let mut e = Editor::new(Document::new(w, h, ColorMode::Rgba).expect("size"));
    let Ok(replies) = run_ops(&mut e, API_VERSION, &[Op::Document]) else {
        panic!("the document is read");
    };
    let Some(Reply::Document(d)) = replies.first() else {
        panic!("a document");
    };
    let (layer, frame) = (i64::from(d.layers[0]), d.frames[0] as i64);
    (e, layer, frame)
}

fn plan(ops: Vec<Op>) -> Vec<Planned> {
    ops.into_iter()
        .map(|op| Planned {
            op,
            referring: None,
        })
        .collect()
}

fn read(layer: i64, frame: i64, width: i64, height: i64) -> Op {
    Op::Pixels {
        layer,
        frame,
        x: 0,
        y: 0,
        width,
        height,
    }
}

fn fill(layer: i64, frame: i64, width: i64, height: i64) -> Op {
    Op::FillRect {
        layer,
        frame,
        x: 0,
        y: 0,
        width,
        height,
        color: 0xff00_00ff,
    }
}

#[test]
fn a_run_may_give_back_at_most_its_budget_of_cells() {
    let (mut e, layer, frame) = canvas(1024, 1024);
    let one = plan(vec![read(layer, frame, 1024, 1024)]);
    assert!(run(&mut e, 1, &one).is_ok(), "one whole read fits");
    let two = plan(vec![
        fill(layer, frame, 4, 4),
        read(layer, frame, 1024, 1024),
        read(layer, frame, 1024, 1024),
    ]);
    let before = e.revision();
    let Err(refused) = run(&mut e, 1, &two) else {
        panic!("two whole reads are too much");
    };
    assert_eq!((refused.code, refused.at), ("too_large", Some(2)));
    assert!(refused.message.contains("1048576"), "{}", refused.message);
    assert_eq!(e.revision(), before, "nothing of the run is kept");
}

#[test]
fn a_run_may_work_on_at_most_its_budget_of_pixels() {
    let budget = Budget {
        cells: 1 << 20,
        work: 1000,
    };
    let (mut e, layer, frame) = canvas(20, 20);
    let fits = plan(vec![fill(layer, frame, 20, 20), fill(layer, frame, 20, 20)]);
    assert!(
        run_within(&mut e, 1, &fits, budget).is_ok(),
        "800 pixels fit"
    );
    let over = plan(vec![
        fill(layer, frame, 20, 20),
        Op::Fill {
            layer,
            frame,
            x: 0,
            y: 0,
            color: 0xff00_ff00,
        },
        fill(layer, frame, 20, 20),
    ]);
    let Err(refused) = run_within(&mut e, 1, &over, budget) else {
        panic!("a fill works on the whole canvas");
    };
    assert_eq!((refused.code, refused.at), ("too_large", Some(2)));
    let lines = plan(vec![Op::DrawLine {
        layer,
        frame,
        from: (0, 0),
        to: (5000, 0),
        color: 0xff00_00ff,
    }]);
    let refused = run_within(&mut e, 1, &lines, budget).err().map(|r| r.code);
    assert_eq!(refused, Some("too_large"), "a line costs its length");
}
