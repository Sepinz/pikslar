//! Cels sharing a drawing across layers export exactly as copies placed by hand: each layer's
//! frame shows the drawing at its own place.

use crate::frame_png;
use pikslar_core::{Cel, CelCmd, CelDrag, ColorMode, Document, DragKind, Editor, Grid, Landing};

fn drawing() -> Grid {
    let cells: Vec<u32> = (0..7 * 4)
        .map(|i| match i % 3 {
            0 => 0,
            _ => 0xff00_0000 | (i as u32 * 0x02_0703),
        })
        .collect();
    Grid::from_cells(7, 4, &cells)
}

/// Two layers of two frames, each cel a copy of the drawing placed at `places[layer][frame]`,
/// or blank where `None`.
fn by_hand(places: [[Option<(i32, i32)>; 2]; 2]) -> Document {
    let mut doc = Document::new(16, 12, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("room");
    doc.add_frame(1, false).expect("room");
    for (layer, row) in places.iter().enumerate() {
        for (frame, place) in row.iter().enumerate() {
            if let Some((x, y)) = *place {
                let cel = Cel::at(drawing().into_image(), x, y).expect("in range");
                doc.put_cel(layer, frame, cel).expect("frame");
            }
        }
    }
    doc
}

#[test]
fn a_drawing_linked_across_layers_exports_as_copies_placed_by_hand() {
    let mut e = Editor::new(by_hand([[Some((1, 2)), None], [None, None]]));
    let link = CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 1,
        to_frame: 1,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    };
    e.apply(CelCmd::Drag(link)).expect("linked");
    let layer = e.layer_id(1).expect("layer");
    let apart = CelCmd::MoveOne {
        layer,
        frame: 1,
        dx: 6,
        dy: 5,
    };
    e.apply(apart).expect("moved");
    let linked = e.doc();
    assert!(linked.layers()[1]
        .cel(1)
        .same_image(linked.layers()[0].cel(0)));
    let hand = by_hand([[Some((1, 2)), None], [None, Some((7, 7))]]);
    for frame in 0..2 {
        assert_eq!(
            frame_png(linked, frame).expect("export"),
            frame_png(&hand, frame).expect("export"),
            "frame {frame}"
        );
    }
}
