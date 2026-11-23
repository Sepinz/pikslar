//! The layer's own tests: cels, holds and writes on one layer.

use super::*;
use crate::grid::Grid;

#[test]
fn moving_within_a_list_matches_removing_and_inserting() {
    for len in 1..7 {
        for from in 0..len {
            for to in 0..len {
                let mut moved: Vec<usize> = (0..len).collect();
                move_within(&mut moved, from, to);
                let mut expected: Vec<usize> = (0..len).collect();
                let item = expected.remove(from);
                expected.insert(to, item);
                assert_eq!(moved, expected, "{from} to {to} of {len}");
            }
        }
    }
}

fn dot(value: u32) -> Cel {
    Cel::new(Grid::from_cells(2, 1, &[value, 0]))
}

fn layer(values: &[u32]) -> Layer {
    let mut layer = Layer::new("test".into(), ColorMode::Rgba);
    values.iter().for_each(|&v| layer.push_cel(dot(v)));
    layer
}

fn pictures(layer: &Layer) -> Vec<u32> {
    layer.cels().iter().map(|c| c.get(0, 0)).collect()
}

#[test]
fn a_cel_reads_as_the_image_it_was_given() {
    let cel = Cel::new(Grid::from_cells(37, 23, &vec![5; 37 * 23]));
    assert_eq!(cel.get(36, 22), 5);
    assert_eq!(cel.get(37, 0), 0, "outside reads as transparent");
    assert_eq!(cel.to_cells(37, 23), vec![5; 37 * 23]);
    assert!(!cel.is_blank());
    assert!(cel.has_tile(0, 0));
    assert!(!cel.has_tile(3, 0), "the table ends with the drawing");
}

#[test]
fn a_blank_cel_has_no_tiles() {
    let cel = Cel::new(Grid::new(32, 32));
    assert!(cel.is_blank());
    assert!(!cel.has_tile(3, 0));
    let mut raw = [9; crate::tile::TILE_AREA];
    cel.decode(3, 0, &mut raw);
    assert!(raw.iter().all(|&c| c == 0));
}

#[test]
fn a_clone_shares_the_image_until_either_is_edited() {
    let original = dot(4);
    let mut copy = original.clone();
    assert!(copy.same_image(&original));
    assert_eq!(copy.image_id(), original.image_id());
    let mut changes = [(TileKey { x: 0, y: 0 }, None)];
    copy.image_mut().swap_cells(&mut changes);
    assert!(changes[0].1.is_some());
    assert!(!copy.same_image(&original));
    assert_eq!(original.get(0, 0), 4, "the other cel keeps its picture");
    assert_eq!(copy.get(0, 0), 0);
}

#[test]
fn an_own_image_is_equal_but_not_shared() {
    let original = dot(4);
    let own = original.with_own_image();
    assert_eq!(own, original);
    assert!(!own.same_image(&original));
}

#[test]
fn frames_keep_one_cel_and_one_hold_each() {
    let mut l = layer(&[1, 2]);
    assert_eq!(l.frame_count(), 2);
    assert_eq!(l.links().collect::<Vec<_>>(), [0, 0]);
    l.insert_frame(1, dot(9), 7);
    assert_eq!(pictures(&l), [1, 9, 2]);
    assert_eq!(l.links().collect::<Vec<_>>(), [0, 7, 0]);
    l.move_frame(1, 2);
    assert_eq!(pictures(&l), [1, 2, 9]);
    assert_eq!(l.link(2), 7);
    l.remove_frames(0..=1);
    assert_eq!(pictures(&l), [9]);
    assert_eq!(l.links().count(), 1);
}

#[test]
fn asking_about_a_missing_frame_finds_no_hold() {
    assert_eq!(layer(&[1]).link(9), 0);
}

#[test]
fn putting_a_cel_on_a_missing_frame_is_refused() {
    let mut l = layer(&[1]);
    assert_eq!(l.put_cel(1, dot(2)), Err(LimitError::NoSuchItem));
    l.put_cel(0, dot(2)).expect("frame exists");
    assert_eq!(pictures(&l), [2]);
}

#[test]
fn holds_from_any_ids_become_small_and_really_shared() {
    let mut l = layer(&[1, 2, 3]);
    l.set_holds(&[u32::MAX, 5, u32::MAX], 0)
        .expect("one per frame");
    assert_eq!(l.links().collect::<Vec<_>>(), [1, 0, 1]);
    assert!(l.cel(0).same_image(l.cel(2)));
    assert_eq!(pictures(&l), [1, 2, 1]);
}

#[test]
fn holds_of_the_wrong_length_change_nothing() {
    let mut l = layer(&[1, 2]);
    assert_eq!(l.set_holds(&[1], 0), Err(LimitError::NoSuchItem));
    assert_eq!(l.links().collect::<Vec<_>>(), [0, 0]);
}

#[test]
fn an_edit_reaches_only_its_own_hold() {
    let mut l = layer(&[1, 2, 3, 4]);
    l.set_holds(&[1, 1, 2, 2], 0).expect("one per frame");
    l.swap_tiles(1, &mut [(TileKey { x: 0, y: 0 }, None)], &mut None);
    assert_eq!(pictures(&l), [0, 0, 3, 3]);
}

#[test]
fn writing_nothing_keeps_a_shared_image_shared() {
    let mut l = layer(&[1, 2]);
    let shared = l.cel(0).clone();
    l.put_cel(1, shared).expect("frame exists");
    l.swap_tiles(0, &mut [], &mut None);
    assert!(l.cel(0).same_image(l.cel(1)));
}

#[test]
fn holding_a_frame_shows_the_source_cel() {
    let mut l = layer(&[1, 2]);
    l.hold_frame(1, 0, 3);
    l.set_link(0, 3);
    assert!(l.cel(1).same_image(l.cel(0)));
}

#[test]
fn a_copy_shares_the_list_of_cels_until_it_changes() {
    let mut l = layer(&[1, 2]);
    let kept = l.clone();
    assert!(Arc::ptr_eq(&kept.cels, &l.cels));
    l.set_link(0, 3);
    assert!(!Arc::ptr_eq(&kept.cels, &l.cels));
    assert_eq!(kept.link(0), 0, "the copy keeps what it had");
}

#[test]
fn a_cel_stays_sixteen_bytes() {
    assert_eq!(size_of::<Cel>(), 16);
}

/// A cel placed away from the origin, which only tests can make while offsets stay zero.
fn placed(value: u32, x: i16, y: i16) -> Cel {
    Cel { x, y, ..dot(value) }
}

#[test]
fn copying_a_whole_cel_takes_its_position() {
    let mut l = layer(&[1, 2]);
    l.cels_mut()[0] = placed(1, 3, 4);
    l.hold_frame(1, 0, 8);
    assert_eq!(l.cel(1).position(), (3, 4));
    assert_eq!(l.cel(1).link(), 8);
    assert!(l.cel(1).same_image(l.cel(0)));
}

#[test]
fn copying_a_drawing_keeps_each_cels_own_position() {
    let mut l = layer(&[1, 2]);
    l.cels_mut()[0] = placed(1, 3, 4);
    l.cels_mut()[1] = placed(2, -5, 6);
    l.set_holds(&[1, 1], 0).expect("one per frame");
    assert!(l.cel(1).same_image(l.cel(0)), "held frames share");
    assert_eq!(l.cel(0).position(), (3, 4));
    assert_eq!(l.cel(1).position(), (-5, 6));
    l.swap_tiles(0, &mut [(TileKey { x: 0, y: 0 }, None)], &mut None);
    assert!(
        l.cel(1).same_image(l.cel(0)),
        "and go on sharing after an edit"
    );
    assert_eq!(l.cel(1).position(), (-5, 6), "an edit moves nothing");
}

#[test]
fn replacing_drawings_keeps_positions_and_holds() {
    let mut l = layer(&[1, 2]);
    l.cels_mut()[1] = placed(2, 7, 8);
    l.set_link(1, 4);
    l.replace_images(vec![Image::blank(), Image::blank()]);
    assert_eq!(l.cel(1).position(), (7, 8));
    assert_eq!(l.cel(1).link(), 4);
    assert!(l.cel(1).is_blank());
}

#[test]
fn putting_a_cel_keeps_the_frames_hold() {
    let mut l = layer(&[1, 2]);
    l.set_link(1, 4);
    l.put_cel(1, dot(9)).expect("frame exists");
    assert_eq!(l.link(1), 4);
}

#[test]
fn a_cel_joining_a_layer_brings_no_hold_with_it() {
    let held = Cel { link: 9, ..dot(1) };
    assert_eq!(layer(&[]).with_cels(vec![held.clone()]).link(0), 0);
    let mut l = layer(&[]);
    l.push_cel(held);
    assert_eq!(l.link(0), 0);
}

#[test]
fn an_edit_of_a_hold_changes_the_table_in_place() {
    let mut l = layer(&[1, 2, 3]);
    l.set_holds(&[1, 1, 1], 0).expect("one per frame");
    let table = l.cel(0).image_id();
    l.swap_tiles(1, &mut [(TileKey { x: 0, y: 0 }, None)], &mut None);
    assert_eq!(l.cel(0).image_id(), table, "no copy was made");
    assert!(l.cel(2).same_image(l.cel(1)));
    assert_eq!(pictures(&l), [0, 0, 0]);
}

#[test]
fn a_write_goes_back_to_the_drawing_it_split_only_if_that_shows_the_same() {
    let mut l = layer(&[1, 2]);
    let other = Arc::new(l.cel(1).image.as_ref().clone());
    let mut split = Some(Arc::downgrade(&other));
    l.swap_tiles(0, &mut [(TileKey { x: 0, y: 0 }, None)], &mut split);
    assert!(
        !Arc::ptr_eq(&l.cel(0).image, &other),
        "a different drawing is not shown"
    );
    assert_eq!(pictures(&l), [0, 2]);
}

#[test]
fn an_edit_of_a_hold_leaves_an_earlier_copy_alone() {
    let mut l = layer(&[1, 2]);
    l.set_holds(&[1, 1], 0).expect("one per frame");
    let kept = l.clone();
    l.swap_tiles(1, &mut [(TileKey { x: 0, y: 0 }, None)], &mut None);
    assert_eq!(pictures(&kept), [1, 1], "a snapshot keeps its picture");
    assert_eq!(pictures(&l), [0, 0]);
}
