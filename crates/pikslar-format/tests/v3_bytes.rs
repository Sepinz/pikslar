//! Pins the exact bytes of a saved version 3 project.
//!
//! The values were recorded from the version 3 writer when it first became the one used. The
//! version 2 values these replaced were recorded before cels were bounded, and the corpus keeps
//! version 2 files. A change to what is written must be deliberate, and must change these.

use pikslar_core::{rgba, ColorMode, Command, Document, DrawCmd, Editor, Grid, Tool};
use pikslar_format::piks::save;

const RED: u32 = rgba(200, 10, 10, 255);
const GREEN: u32 = rgba(10, 200, 10, 128);

fn fingerprint(bytes: &[u8]) -> (usize, u64) {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    });
    (bytes.len(), hash)
}

fn stroke(editor: &mut Editor, tool: Tool, path: &[(i32, i32)]) {
    editor
        .apply(Command::Draw(DrawCmd::Stroke {
            tool,
            path: path.to_vec(),
        }))
        .expect("stroke");
}

#[test]
fn a_blank_project_keeps_its_bytes() {
    let doc = Document::new(40, 24, ColorMode::Palette).expect("valid");
    assert_eq!(fingerprint(&save(&doc)), (98, 2898850891009519782));
}

#[test]
fn a_canvas_that_is_not_whole_tiles_keeps_its_bytes() {
    let mut doc = Document::new(37, 23, ColorMode::Rgba).expect("valid");
    let cells: Vec<u32> = (0..37 * 23)
        .map(|i| if i % 7 == 0 { RED } else { 0 })
        .collect();
    doc.set_cel(0, 0, Grid::from_cells(37, 23, &cells))
        .expect("fits");
    assert_eq!(fingerprint(&save(&doc)), (362, 1439711009826914933));
}

#[test]
fn art_in_one_corner_keeps_its_bytes() {
    let mut editor = Editor::new(Document::new(256, 256, ColorMode::Rgba).expect("valid"));
    editor.color = GREEN;
    stroke(&mut editor, Tool::Pencil, &[(240, 240), (250, 252)]);
    assert_eq!(fingerprint(&save(editor.doc())), (142, 2662248293748831973));
}

#[test]
fn something_drawn_and_erased_keeps_its_bytes() {
    let mut editor = Editor::new(Document::new(64, 64, ColorMode::Palette).expect("valid"));
    editor.color = RED;
    stroke(&mut editor, Tool::Pencil, &[(2, 2), (60, 60)]);
    stroke(&mut editor, Tool::Eraser, &[(60, 60), (40, 40)]);
    assert_eq!(
        fingerprint(&save(editor.doc())),
        (240, 15122713563410190020)
    );
}

#[test]
fn holds_on_two_layers_keep_their_bytes() {
    let mut doc = Document::new(32, 32, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Palette).expect("room");
    (1..4).for_each(|at| doc.add_frame(at, false).expect("room"));
    doc.set_cel(0, 0, Grid::from_cells(32, 32, &[RED; 32 * 32]))
        .expect("fits");
    doc.set_cel(0, 2, Grid::from_cells(32, 32, &[GREEN; 32 * 32]))
        .expect("fits");
    doc.set_cel(1, 1, Grid::from_cells(32, 32, &[1; 32 * 32]))
        .expect("fits");
    doc.hold_previous(0, 1).expect("held");
    doc.hold_previous(0, 3).expect("held");
    doc.hold_previous(1, 2).expect("held");
    doc.hold_across(1, 2, 2, 3).expect("held");
    assert_eq!(fingerprint(&save(&doc)), (314, 13559637036338981962));
}

#[test]
fn a_hold_released_in_the_middle_keeps_its_bytes() {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    (1..6).for_each(|at| doc.add_frame(at, false).expect("room"));
    doc.set_cel(0, 0, Grid::from_cells(16, 16, &[RED; 16 * 16]))
        .expect("fits");
    doc.hold_across(0, 0, 0, 2).expect("held");
    doc.hold_across(0, 3, 3, 5).expect("held");
    doc.release_hold(0, 1).expect("released");
    doc.remove_frame(4).expect("removed");
    // Was (184, 2857220861557093940) before frame ids were saved. Removing frame 4 leaves the
    // ids 1, 2, 3, 4, 6, so a 100-byte `Nids` section now follows the others: tag and length,
    // version, layer floor 1, frame floor 6, the one layer id with its check, and the five
    // frame ids, each with its print. (It was 60 bytes, with one fingerprint for all the frames
    // and no layer checks, before the per-frame prints and layer checks.)
    let bytes = save(&doc);
    assert!(bytes.windows(4).any(|w| w == b"Nids"));
    assert_eq!(fingerprint(&bytes), (284, 13703895240886122305));
}
