//! Slices with extreme values, brought in from an Aseprite file, saved as a project and loaded
//! again: the same slices, and a document that reports no damage and may be saved over.

use crate::ase_build::*;
use crate::ase_tests::open;
use crate::piks;
use pikslar_core::{MAX_SLICE_NAME, SLICE_REACH};

fn extreme(name: &str, bounds: (i32, i32, u32, u32)) -> (String, SliceKey) {
    let key = SliceKey {
        frame: 0,
        bounds,
        center: Some((0, 0, bounds.2, bounds.3)),
        pivot: Some((SLICE_REACH, -SLICE_REACH)),
    };
    (name.to_owned(), key)
}

#[test]
fn slices_with_extreme_values_survive_a_save_and_load() {
    let reach = SLICE_REACH;
    let span = (2 * reach) as u32;
    let slices = [
        extreme("whole", (-reach, -reach, span, span)),
        extreme("corner", (reach - 1, reach - 1, 1, 1)),
        extreme("one", (-reach, reach - 1, span, 1)),
        extreme(&"\u{1F600}".repeat(MAX_SLICE_NAME), (0, 0, 3, 3)),
        extreme("caf\u{e9} \u{4e2d}\u{6587} \u{202e}x", (1, 1, 2, 2)),
        ("".to_owned(), extreme("", (4, 4, 2, 2)).1),
    ];
    let chunks: Vec<Vec<u8>> = slices
        .iter()
        .map(|(n, k)| slice(n, std::slice::from_ref(k)))
        .collect();
    let mut all = vec![plain("L")];
    all.extend(chunks);
    let bytes = Ase::new(16, 16, 32).frame(100, all).bytes();
    let (doc, _) = open(&bytes);
    assert_eq!(doc.slices.len(), slices.len());
    let (back, notes) = piks::load_noted(&piks::save(&doc)).expect("a project that loads");
    assert_eq!(back.slices, doc.slices);
    assert!(notes.damaged.is_empty(), "{:?}", notes.damaged);
    assert!(notes.can_save_over());
}
