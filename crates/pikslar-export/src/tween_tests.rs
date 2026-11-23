//! A tweened hold exports exactly as a copy placed by hand: each frame its own drawing, at the
//! offset the plan's table gives, in no hold.

use crate::frame_png;
use pikslar_core::{Cel, CelCmd, ColorMode, Document, Ease, Editor, Grid};

fn drawing() -> Grid {
    let cells: Vec<u32> = (0..6 * 5)
        .map(|i| match i % 4 {
            0 => 0,
            _ => 0xff00_0000 | (i as u32 * 0x01_0305),
        })
        .collect();
    Grid::from_cells(6, 5, &cells)
}

/// Five frames showing the drawing at `places`, each a cel of its own.
fn by_hand(places: &[(i32, i32)]) -> Document {
    let mut doc = Document::new(16, 12, ColorMode::Rgba).expect("valid");
    for (frame, &(x, y)) in places.iter().enumerate() {
        if frame > 0 {
            doc.add_frame(frame, false).expect("room");
        }
        let cel = Cel::at(drawing().into_image(), x, y).expect("in range");
        doc.put_cel(0, frame, cel).expect("frame");
    }
    doc
}

#[test]
fn a_tweened_hold_exports_as_a_copy_offset_by_hand() {
    // A hold of five frames, the last moved alone to (8, -2), tweened with ease out.
    let mut doc = by_hand(&[(0, 0); 5]);
    doc.set_holds(0, &[1; 5]).expect("hold");
    let last = doc.layers()[0].cel(4).moved_to(8, -2).expect("in range");
    doc.put_cel(0, 4, last).expect("frame");
    let mut e = Editor::new(doc);
    let layer = e.layer_id(0).expect("layer");
    let command = CelCmd::Tween {
        layer,
        from: 0,
        to: 4,
        ease: Ease::Out,
    };
    assert!(e.apply(command).expect("tween").changed);
    // Ease out over 0, 1/4, 1/2, 3/4, 1: x 0, 4, 6, 8, 8 and y 0, -1, -2, -2, -2.
    let places = [(0, 0), (4, -1), (6, -2), (8, -2), (8, -2)];
    let tweened = e.doc();
    assert!(tweened.layers()[0]
        .cels()
        .iter()
        .all(|cel| cel.same_image(tweened.layers()[0].cel(0))));
    let hand = by_hand(&places);
    for frame in 0..5 {
        assert_eq!(
            frame_png(tweened, frame).expect("export"),
            frame_png(&hand, frame).expect("export"),
            "frame {frame}"
        );
    }
}
