//! A tag's repeat count as an animation's plays: what was asked wins, then the tag's own count.

use crate::{animation_order, ExportError, Frames};

fn order(doc: &Document, which: &Frames, asked: Option<u32>) -> (Vec<usize>, u32) {
    animation_order(doc, which, asked).expect("the frames are there")
}
use pikslar_core::{ColorMode, Direction, Document, MAX_TAG_REPEAT};

/// Six frames; tag 0 is frames 1 to 3 played `repeat` times in `direction`.
fn tagged(direction: Direction, repeat: u16) -> Document {
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
    for at in 1..6 {
        doc.add_frame(at, false).expect("frame");
    }
    doc.add_tag("t", 1, 3).expect("tag");
    doc.tags[0].direction = direction;
    doc.tags[0].repeat = repeat;
    doc
}

fn tag() -> Frames {
    Frames::Tag("t".into())
}

#[test]
fn a_tag_played_three_times_loops_three_times() {
    let doc = tagged(Direction::Forward, 3);
    assert_eq!(order(&doc, &tag(), None), (vec![1, 2, 3], 3));
    let reverse = tagged(Direction::Reverse, 2);
    assert_eq!(order(&reverse, &tag(), None), (vec![3, 2, 1], 2));
}

#[test]
fn a_loop_asked_for_wins_over_the_tags_repeat() {
    let doc = tagged(Direction::Forward, 3);
    assert_eq!(order(&doc, &tag(), Some(0)), (vec![1, 2, 3], 0));
    assert_eq!(order(&doc, &tag(), Some(7)), (vec![1, 2, 3], 7));
}

#[test]
fn frames_that_are_no_tag_play_forever_unless_asked() {
    let doc = tagged(Direction::Forward, 3);
    let range = Frames::Range(1, 3);
    assert_eq!(order(&doc, &range, None), (vec![1, 2, 3], 0));
    assert_eq!(order(&doc, &range, Some(2)), (vec![1, 2, 3], 2));
}

#[test]
fn a_ping_pong_played_a_set_number_of_times_is_written_out_in_full() {
    let once = tagged(Direction::PingPong, 1);
    assert_eq!(order(&once, &tag(), None), (vec![1, 2, 3], 1));
    let thrice = tagged(Direction::PingPong, 3);
    let there = vec![1, 2, 3, 2, 1, 2, 3];
    assert_eq!(order(&thrice, &tag(), None), (there, 1));
    let forever = tagged(Direction::PingPong, 0);
    let cycle = vec![1, 2, 3, 2];
    assert_eq!(order(&forever, &tag(), None), (cycle, 0));
}

#[test]
fn the_longest_ping_pong_written_out_stays_within_the_frames_an_animation_may_have() {
    let doc = tagged(Direction::PingPong, MAX_TAG_REPEAT);
    let Ok((order, 1)) = animation_order(&doc, &tag(), None) else {
        panic!("written out");
    };
    assert_eq!(order.len(), 1 + 2 * usize::from(MAX_TAG_REPEAT));
    let missing = Frames::Tag("none".into());
    let refused = animation_order(&doc, &missing, None);
    assert!(matches!(refused, Err(ExportError::NoSuchTag(name)) if name == "none"));
}

#[test]
fn a_sheet_names_a_tags_repeat_only_when_it_has_one() {
    let mut doc = tagged(Direction::Forward, 0);
    doc.add_tag("u", 0, 0).expect("tag");
    doc.tags[1].repeat = 4;
    let mut out = String::new();
    crate::sheet::push_tags(&mut out, &doc.tags);
    let expected = concat!(
        r#"{"name":"t","from":1,"to":3,"direction":"forward"},"#,
        r#"{"name":"u","from":0,"to":0,"direction":"forward","repeat":"4"}"#
    );
    assert_eq!(out, expected);
}
