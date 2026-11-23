//! Links across the document: the table, the members, the rules and the tidy.

use super::*;
use crate::cel_drag::{CelDrag, DragKind, Landing};
use crate::document::ColorMode;
use crate::grid::Grid;
use crate::layer::Cel;

/// Two layers of four frames, each cel with a drawing of its own.
fn doc() -> Document {
    let mut d = Document::new(4, 2, ColorMode::Rgba).expect("size");
    d.add_layer(1, ColorMode::Rgba).expect("room");
    for at in 1..4 {
        d.add_frame(at, false).expect("room");
    }
    for layer in 0..2 {
        for frame in 0..4 {
            let value = 1 + layer as u32 * 10 + frame as u32;
            d.set_cel(layer, frame, Grid::from_cells(4, 2, &[value; 8]))
                .expect("fits");
        }
    }
    d
}

/// Links the cel at `to` to the cel at `from`, as an Alt drag of one cel does.
fn link_cel(d: &mut Document, from: (usize, usize), to: (usize, usize)) -> Result<(), LimitError> {
    d.drag_cels(&CelDrag {
        layers: (from.0, from.0),
        frames: (from.1, from.1),
        to_layer: to.0,
        to_frame: to.1,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    })
}

fn shown(d: &Document, layer: usize, frame: usize) -> u32 {
    d.layers[layer].cel(frame).get(0, 0)
}

#[test]
fn a_link_with_one_cel_on_each_of_two_layers_is_kept() {
    let mut d = doc();
    link_cel(&mut d, (0, 1), (1, 2)).expect("cels");
    let id = d.layers[0].link(1);
    assert_ne!(id, 0);
    assert_eq!(d.layers[1].link(2), id, "one id on both layers");
    assert!(d.layers[1].cel(2).same_image(d.layers[0].cel(1)));
    assert_eq!(
        shown(&d, 1, 2),
        2,
        "the landing cel shows the source's drawing"
    );
    d.tidy_holds();
    assert_eq!(d.layers[1].link(2), id, "the tidy keeps it");
    assert_eq!(d.check_links(), Ok(()));
    assert!(d.links_across(0, 1) && d.links_across(1, 2));
    assert!(!d.links_across(0, 0));
}

#[test]
fn taking_either_cel_out_ends_the_link() {
    let mut d = doc();
    link_cel(&mut d, (0, 1), (1, 2)).expect("cels");
    d.release_hold(1, 2).expect("cel");
    assert_eq!((d.layers[0].link(1), d.layers[1].link(2)), (0, 0));
    let mut d = doc();
    link_cel(&mut d, (0, 1), (1, 2)).expect("cels");
    d.remove_frame(1).expect("frame");
    assert_eq!(
        d.layers[1].link(1),
        0,
        "the cel left on the other layer is no link"
    );
    assert_eq!(d.check_links(), Ok(()));
}

#[test]
fn a_cel_landing_in_a_link_leaves_the_one_it_was_in() {
    let mut d = doc();
    d.hold_previous(1, 1).expect("frames");
    link_cel(&mut d, (0, 3), (1, 1)).expect("cels");
    assert_eq!(d.layers[1].link(0), 0, "the hold left with one frame ends");
    assert_eq!(d.check_links(), Ok(()));
}

#[test]
fn the_table_lists_every_member_in_order() {
    let mut d = doc();
    d.hold_previous(0, 3).expect("frames");
    link_cel(&mut d, (1, 0), (0, 0)).expect("cels");
    let table = d.link_table();
    let across = d.layers[1].link(0);
    let held = d.layers[0].link(3);
    let member = |id, layer, frame| Member { id, layer, frame };
    assert_eq!(
        table.members(across),
        [member(across, 0, 0), member(across, 1, 0)]
    );
    assert_eq!(
        table.members(held),
        [member(held, 0, 2), member(held, 0, 3)]
    );
    assert!(table.reaches_across(across) && !table.reaches_across(held));
    assert_eq!(table.links().count(), 2);
    assert!(table.all().windows(2).all(|pair| pair[0] < pair[1]));
    assert!(table.members(0).is_empty() && table.members(999).is_empty());
    assert_eq!(d.link_members(1, 0), [(0, 0), (1, 0)]);
    assert_eq!(d.link_members(1, 1), [(1, 1)], "a cel alone");
    assert!(d.link_members(1, 9).is_empty());
}

#[test]
fn a_new_id_is_clear_of_every_layer() {
    let mut d = doc();
    link_cel(&mut d, (0, 0), (1, 0)).expect("cels");
    d.hold_previous(1, 3).expect("frames");
    assert_ne!(d.layers[1].link(3), d.layers[1].link(0));
}

/// A document with a link broken by hand, and the fault it should show.
fn broken(rule: usize) -> (Document, LinkFault) {
    let mut d = doc();
    let shared = d.layers[0].cel(0).clone();
    match rule {
        1 => d.layers[0].set_link(0, 9),
        2 => {
            d.layers[0].set_link(0, 9);
            d.layers[1].set_link(1, 9);
        }
        3..=5 => {
            d.layers[1].put_cel(0, shared).expect("frame");
            d.layers[0].set_link(0, 9);
            d.layers[1].set_link(0, 9);
            match rule {
                3 => d.layers[1].mode = ColorMode::Palette,
                4 => d.layers[1].kind = LayerKind::Reference,
                _ => d.layers[1].kind = LayerKind::Group,
            }
        }
        _ => {}
    }
    let fault = match rule {
        1 => LinkFault::Lone(9),
        2 => LinkFault::Unshared(9),
        3 => LinkFault::MixedModes(9),
        4 => LinkFault::Reference(9),
        _ => LinkFault::Group(9),
    };
    (d, fault)
}

#[test]
fn the_check_names_each_rule_broken_by_hand() {
    for rule in 1..=5 {
        let (d, fault) = broken(rule);
        assert_eq!(d.check_links(), Err(fault), "L{rule}");
        assert!(fault.to_string().starts_with(&format!("L{rule}:")));
    }
    assert_eq!(doc().check_links(), Ok(()));
}

#[test]
fn a_reference_held_on_its_own_layer_keeps_the_rules() {
    let mut d = doc();
    d.layers[1].kind = LayerKind::Reference;
    d.hold_across(1, 0, 0, 3).expect("frames");
    assert_eq!(d.check_links(), Ok(()));
}

#[test]
fn only_this_file_ends_links_or_sets_ids() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    visit(&root, &mut |path, text| {
        let name = path.to_string_lossy().replace('\\', "/");
        let allowed = [
            "/links.rs",
            "/layer.rs",
            "/layer/",
            "/cel_drag.rs",
            "_tests.rs",
        ]
        .iter()
        .any(|ok| name.contains(ok))
            || name.ends_with("/tests.rs");
        for call in [
            ".set_link(",
            ".hold_frame(",
            "drop_lone_holds",
            ".shift_holds(",
        ] {
            let from_parts = call == ".shift_holds(" && name.ends_with("/document.rs");
            if text.contains(call) && !allowed && !from_parts {
                offenders.push(format!("{name}: {call}"));
            }
        }
    });
    assert!(offenders.is_empty(), "{offenders:?}");
}

fn visit(dir: &std::path::Path, check: &mut dyn FnMut(&std::path::Path, &str)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, check);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            check(&path, &text);
        }
    }
}

/// Two cels on each layer sharing one drawing, as a file naming one image record gives them.
fn sharing(d: &mut Document, cels: &[(usize, usize)]) {
    let (layer, frame) = cels[0];
    let drawing = d.layers[layer].cel(frame).clone();
    for &(layer, frame) in &cels[1..] {
        d.put_cel(layer, frame, drawing.clone()).expect("frame");
    }
}

#[test]
fn set_links_ranks_links_across_the_document() {
    let mut d = doc();
    sharing(&mut d, &[(0, 0), (0, 1)]);
    sharing(&mut d, &[(0, 3), (1, 1)]);
    let broken = d
        .set_links(&[vec![7, 7, 0, 9], vec![0, 9, 3, 0]])
        .expect("shape");
    assert_eq!(broken, 0);
    assert_eq!(d.layers[0].links().collect::<Vec<_>>(), [1, 1, 0, 2]);
    assert_eq!(
        d.layers[1].links().collect::<Vec<_>>(),
        [0, 2, 0, 0],
        "an id on one cel is no link"
    );
    assert_eq!(shown(&d, 1, 2), 13, "a lone id keeps its own drawing");
    assert_eq!(d.check_links(), Ok(()));
}

#[test]
fn set_links_never_changes_a_drawing() {
    let mut d = doc();
    let before = d.layers.clone();
    let broken = d
        .set_links(&[vec![5, 5, 0, 0], vec![0, 0, 0, 0]])
        .expect("shape");
    assert_eq!(broken, 1, "two cels showing different drawings");
    assert!(d.layers.iter().all(|l| l.links().all(|id| id == 0)));
    for (layer, was) in d.layers.iter().zip(before.iter()) {
        assert_eq!(layer.cels(), was.cels(), "every drawing kept");
    }
    assert_eq!(d.check_links(), Ok(()));
}

#[test]
fn set_links_keeps_each_layers_sound_part_of_a_broken_link() {
    let mut d = doc();
    d.layers[1].mode = ColorMode::Palette;
    sharing(&mut d, &[(0, 0), (0, 1)]);
    sharing(&mut d, &[(1, 2), (1, 3)]);
    let broken = d
        .set_links(&[vec![5, 5, 0, 6], vec![5, 0, 5, 5]])
        .expect("shape");
    assert_eq!(broken, 1, "the link joining two modes");
    assert_eq!(d.layers[0].links().collect::<Vec<_>>(), [1, 1, 0, 0]);
    assert_eq!(
        d.layers[1].links().collect::<Vec<_>>(),
        [0, 0, 2, 2],
        "its own hold stays; the cel showing another drawing leaves"
    );
    assert_eq!((shown(&d, 0, 3), shown(&d, 1, 0)), (4, 11), "drawings kept");
    assert_eq!(d.check_links(), Ok(()));
    assert_eq!(d.set_links(&[vec![0; 4]]), Err(LimitError::NoSuchItem));
}

#[test]
fn set_links_takes_a_group_out_of_a_link() {
    let mut d = doc();
    d.layers[1].kind = LayerKind::Group;
    sharing(&mut d, &[(0, 0), (0, 1)]);
    let broken = d
        .set_links(&[vec![5, 5, 0, 0], vec![5, 5, 0, 0]])
        .expect("shape");
    assert_eq!(broken, 1);
    assert_eq!(d.layers[0].links().collect::<Vec<_>>(), [1, 1, 0, 0]);
    assert!(d.layers[1].links().all(|id| id == 0));
}

#[test]
fn a_cel_given_a_drawing_of_its_own_leaves_its_link() {
    let mut d = doc();
    link_cel(&mut d, (0, 1), (1, 2)).expect("cels");
    d.hold_previous(0, 3).expect("frames");
    let moved = d.layers[1].cel(2).moved_to(3, -1).expect("in range");
    d.put_cel(1, 2, moved).expect("frame");
    assert_ne!(
        d.layers[1].link(2),
        0,
        "the same drawing elsewhere keeps it"
    );
    d.set_cel(1, 2, Grid::from_cells(4, 2, &[9; 8]))
        .expect("fits");
    assert_eq!((d.layers[0].link(1), d.layers[1].link(2)), (0, 0));
    assert_eq!(shown(&d, 0, 1), 2, "the other cel keeps its drawing");
    let own = Cel::new(Grid::from_cells(4, 2, &[8; 8]));
    d.put_cel(0, 3, own).expect("frame");
    assert_eq!((d.layers[0].link(2), d.layers[0].link(3)), (0, 0));
    assert_eq!(d.check_links(), Ok(()));
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "links")]
fn a_command_breaking_a_rule_of_the_links_is_caught_in_a_debug_build() {
    let mut e = crate::editor::Editor::new(doc());
    e.transact(|d| link_cel(d, (0, 1), (1, 2))).expect("linked");
    let own = Cel::new(Grid::from_cells(4, 2, &[9; 8]));
    let _ = e.transact(|d| d.layers[1].put_cel(2, own));
}

#[test]
fn members_sort_by_link_then_layer_then_frame_through_their_keys() {
    let values = [0u32, 1, 7, u32::MAX];
    let small = [0u16, 1, 300, u16::MAX];
    let mut members = Vec::new();
    for &id in values.iter().rev() {
        for &layer in &small {
            for &frame in small.iter().rev() {
                members.push(Member { id, layer, frame });
            }
        }
    }
    members.push(members[5]);
    let mut want = members.clone();
    want.sort_unstable();
    sort_members(&mut members);
    assert_eq!(members, want);
    assert!(want.iter().all(|m| Member::from_key(m.key()) == *m));
}
