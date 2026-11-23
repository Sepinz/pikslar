//! Tests for holds, tags, spans, onion ghosts and timing.

use super::*;
use crate::color::rgba;
use crate::document::ColorMode;
use crate::editor::Editor;
use crate::editor::Tool;
use crate::FrameCmd;

const INK: u32 = rgba(10, 200, 30, 255);

fn editor(frames: usize) -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("valid"));
    for at in 1..frames {
        e.transact(|d| d.add_frame(at, false)).expect("room");
    }
    e.color = INK;
    e
}

fn dot(e: &mut Editor, frame: usize, x: i32) {
    e.frame = frame;
    e.begin(Tool::Pencil, x, 3).expect("begin");
    e.commit();
}

fn ink(e: &Editor, frame: usize, x: usize) -> bool {
    e.doc.layers[0].cel(frame).get(x as i32, 3) != 0
}

#[test]
fn an_edit_to_any_held_frame_reaches_them_all() {
    let mut e = editor(4);
    dot(&mut e, 0, 1);
    e.doc.hold_across(0, 0, 0, 2).expect("frames");
    dot(&mut e, 1, 5);
    assert!(ink(&e, 0, 5) && ink(&e, 1, 5) && ink(&e, 2, 5));
    assert!(!ink(&e, 3, 5), "outside the hold");
}

#[test]
fn a_moved_selection_lands_in_every_held_frame() {
    let mut e = editor(3);
    dot(&mut e, 0, 2);
    e.doc.hold_across(0, 0, 0, 2).expect("frames");
    e.frame = 1;
    e.select_all();
    assert!(e.lift(false));
    e.move_floating(4, 0);
    e.anchor();
    assert!(ink(&e, 0, 6) && ink(&e, 1, 6) && ink(&e, 2, 6));
    assert!(!ink(&e, 2, 2), "and is gone from where it was");
}

#[test]
fn undo_and_redo_stay_in_step_across_a_hold() {
    let mut e = editor(3);
    e.doc.hold_across(0, 0, 0, 2).expect("frames");
    dot(&mut e, 2, 6);
    assert!(ink(&e, 0, 6));
    e.undo();
    assert!(!ink(&e, 0, 6) && !ink(&e, 2, 6));
    e.redo();
    assert!(ink(&e, 0, 6) && ink(&e, 1, 6));
}

#[test]
fn releasing_a_hold_gives_a_frame_its_own_drawing() {
    let mut e = editor(3);
    dot(&mut e, 0, 1);
    e.doc.hold_across(0, 0, 0, 2).expect("frames");
    e.doc.release_hold(0, 1).expect("frames");
    dot(&mut e, 1, 8);
    assert!(ink(&e, 1, 8) && !ink(&e, 0, 8) && !ink(&e, 2, 8));
    assert!(ink(&e, 1, 1), "it keeps what it had");
    dot(&mut e, 0, 9);
    assert!(ink(&e, 2, 9) && !ink(&e, 1, 9));
}

#[test]
fn the_last_two_of_a_hold_end_it_together() {
    let mut e = editor(2);
    e.doc.hold_previous(0, 1).expect("frames");
    e.doc.release_hold(0, 0).expect("frames");
    assert!(!e.doc.is_held(0, 0) && !e.doc.is_held(0, 1));
}

#[test]
fn holding_the_previous_frame_shows_its_drawing_and_joins_its_hold() {
    let mut e = editor(3);
    dot(&mut e, 0, 2);
    e.doc.hold_previous(0, 1).expect("frames");
    e.doc.hold_previous(0, 2).expect("frames");
    assert!(ink(&e, 2, 2));
    assert!(e.doc.held_into_next(0, 0) && e.doc.held_into_next(0, 1));
    assert_eq!(e.doc.hold_previous(0, 0), Err(LimitError::NoSuchItem));
}

#[test]
fn ids_from_anywhere_become_small_and_lone_holds_end() {
    let mut links = [u32::MAX, 7, 0, u32::MAX, 5];
    tidy_links(&mut links);
    assert_eq!(links, [1, 0, 0, 1, 0], "never more than one id per frame");
    let mut e = editor(2);
    e.doc.layers[0]
        .set_holds(&[u32::MAX, 0], 0)
        .expect("one per frame");
    e.doc.hold_previous(0, 1).expect("frames");
    assert!(e.doc.held_into_next(0, 0));
}

#[test]
fn a_new_hold_is_numbered_clear_of_every_layer() {
    let mut e = editor(4);
    e.transact(|d| d.add_layer(1, ColorMode::Rgba))
        .expect("room");
    e.doc.hold_previous(0, 1).expect("frames");
    e.doc.hold_previous(1, 1).expect("frames");
    assert_ne!(e.doc.layers[0].link(0), 0);
    assert_ne!(e.doc.layers[0].link(0), e.doc.layers[1].link(0));
}

#[test]
fn holds_on_two_layers_stay_separate() {
    let mut e = editor(2);
    e.transact(|d| d.add_layer(1, ColorMode::Rgba))
        .expect("room");
    e.doc.set_holds(0, &[1, 1]).expect("one per frame");
    e.doc.set_holds(1, &[1, 1]).expect("one per frame");
    assert_ne!(e.doc.layers[0].link(0), e.doc.layers[1].link(0));
    dot(&mut e, 0, 4);
    assert!(ink(&e, 1, 4), "the hold on this layer changed");
    assert!(e.doc.layers[1].cel(0).is_blank(), "the other layer did not");
}

#[test]
fn a_colour_mode_change_keeps_a_hold_one_drawing() {
    let mut e = editor(3);
    dot(&mut e, 0, 2);
    e.doc.hold_across(0, 0, 0, 2).expect("frames");
    e.transact(|d| d.set_layer_mode(0, ColorMode::Palette))
        .expect("converts");
    let cels = e.doc.layers[0].cels();
    assert!(cels[0].same_image(&cels[2]));
}

#[test]
fn removing_a_run_of_frames_keeps_tags_and_holds_true() {
    let mut e = editor(6);
    e.doc.add_tag("a", 1, 4).expect("valid");
    e.doc.hold_across(0, 0, 0, 2).expect("frames");
    e.doc.remove_frames(1, 2).expect("room");
    assert_eq!(e.doc.frames.len(), 4);
    assert!(!e.doc.is_held(0, 0), "one frame left of the hold");
    assert_eq!((e.doc.tags[0].from, e.doc.tags[0].to), (1, 2));
    assert_eq!(e.doc.remove_frames(0, 3), Err(LimitError::LastFrame));
    assert_eq!(e.doc.remove_frames(3, 9), Err(LimitError::NoSuchItem));
    assert_eq!(e.doc.remove_frames(2, 1), Err(LimitError::NoSuchItem));
}

#[test]
fn a_new_frame_keeps_held_layers_held_and_leaves_others_blank() {
    let mut e = editor(2);
    e.transact(|d| d.add_layer(1, ColorMode::Rgba))
        .expect("room");
    dot(&mut e, 0, 4);
    e.doc.hold_across(0, 0, 0, 1).expect("frames");
    e.layer = 1;
    dot(&mut e, 1, 7);
    e.transact(|d| d.add_frame(2, false)).expect("room");
    assert!(e.doc.is_held(0, 2) && ink(&e, 2, 4));
    assert!(e.doc.layers[1].cel(2).get(7, 3) == 0);
}

#[test]
fn linked_state_survives_undoing_structural_changes() {
    let mut e = editor(3);
    e.transact(|d| d.hold_across(0, 0, 0, 2)).expect("frames");
    assert!(e.doc.is_held(0, 1));
    e.undo();
    assert!(!e.doc.is_held(0, 1));
    e.redo();
    assert!(e.doc.held_into_next(0, 1));
}

#[test]
fn moving_a_frame_takes_its_art_timing_and_holds() {
    let mut e = editor(4);
    dot(&mut e, 0, 1);
    e.doc.frames[0].duration_ms = 300;
    e.doc.hold_previous(0, 2).expect("frames");
    e.doc.move_frame(0, 3).expect("frames");
    assert!(ink(&e, 3, 1) && !ink(&e, 0, 1));
    assert_eq!(e.doc.frames[3].duration_ms, 300);
    assert!(e.doc.held_into_next(0, 0));
    assert_eq!(e.doc.move_frame(0, 9), Err(LimitError::NoSuchItem));
}

#[test]
fn a_one_frame_tag_goes_where_its_frame_goes() {
    let mut e = editor(6);
    e.doc.add_tag("blink", 2, 2).expect("valid");
    e.doc.move_frame(2, 4).expect("frames");
    assert_eq!((e.doc.tags[0].from, e.doc.tags[0].to), (4, 4));
}

#[test]
fn an_empty_cel_is_known_without_looking_at_it() {
    let mut e = editor(1);
    assert!(e.doc.layers[0].cel(0).is_blank());
    dot(&mut e, 0, 2);
    assert!(!e.doc.layers[0].cel(0).is_blank());
    e.undo();
    assert!(e.doc.layers[0].cel(0).is_blank());
}

#[test]
fn tags_follow_frames_being_added_removed_and_moved() {
    let mut e = editor(6);
    e.doc.add_tag("walk", 1, 3).expect("valid");
    e.doc.add_frame(0, false).expect("room");
    assert_eq!((e.doc.tags[0].from, e.doc.tags[0].to), (2, 4));
    e.doc.add_frame(3, false).expect("room");
    assert_eq!((e.doc.tags[0].from, e.doc.tags[0].to), (2, 5));
    e.doc.add_frame(6, false).expect("room");
    assert_eq!(
        (e.doc.tags[0].from, e.doc.tags[0].to),
        (2, 6),
        "added at its end"
    );
    e.doc.remove_frame(0).expect("room");
    e.doc.remove_frame(2).expect("room");
    assert_eq!((e.doc.tags[0].from, e.doc.tags[0].to), (1, 4));
    e.doc.move_frame(1, 6).expect("room");
    assert_eq!(
        (e.doc.tags[0].from, e.doc.tags[0].to),
        (1, 3),
        "its frame left"
    );
    e.doc.move_frame(6, 2).expect("room");
    assert_eq!(
        (e.doc.tags[0].from, e.doc.tags[0].to),
        (1, 4),
        "placed inside"
    );
    e.doc.move_frame(1, 4).expect("room");
    assert_eq!(
        (e.doc.tags[0].from, e.doc.tags[0].to),
        (1, 4),
        "moved within"
    );
    e.doc.move_frame(0, 6).expect("room");
    assert_eq!((e.doc.tags[0].from, e.doc.tags[0].to), (0, 3), "shifted");
}

#[test]
fn a_tag_goes_when_its_last_frame_goes() {
    let mut e = editor(3);
    e.doc.add_tag("hit", 1, 1).expect("valid");
    e.doc.remove_frame(1).expect("room");
    assert!(e.doc.tags.is_empty());
}

#[test]
fn bad_tags_are_refused_and_long_names_cut() {
    let mut e = editor(3);
    assert_eq!(e.doc.add_tag("x", 2, 1), Err(LimitError::NoSuchItem));
    assert_eq!(e.doc.add_tag("x", 0, 3), Err(LimitError::NoSuchItem));
    let at = e.doc.add_tag(&"a".repeat(99), 0, 2).expect("valid");
    assert_eq!(e.doc.tags[at].name.len(), MAX_TAG_NAME);
}

#[test]
fn spans_play_in_their_direction() {
    let play = |direction, start: usize, n: usize| {
        let span = Span {
            from: 2,
            to: 4,
            direction,
        };
        let (mut at, mut back, mut seen) = (start, false, Vec::new());
        for _ in 0..n {
            at = span.step(at, &mut back);
            seen.push(at);
        }
        seen
    };
    assert_eq!(play(Direction::Forward, 2, 4), [3, 4, 2, 3]);
    assert_eq!(play(Direction::Reverse, 4, 4), [3, 2, 4, 3]);
    assert_eq!(play(Direction::PingPong, 2, 6), [3, 4, 3, 2, 3, 4]);
    assert_eq!(play(Direction::Forward, 9, 2), [2, 3], "from outside");
    assert_eq!(play(Direction::Reverse, 0, 1), [4], "from outside");
}

#[test]
fn a_single_frame_ping_pong_stays_put() {
    let span = Span {
        from: 3,
        to: 3,
        direction: Direction::PingPong,
    };
    assert_eq!(span.step(3, &mut false), 3);
}

#[test]
fn ghosts_never_name_a_frame_that_does_not_exist() {
    let stale = Some(Span {
        from: 0,
        to: 9,
        direction: Direction::Forward,
    });
    let ghosts = onion_ghosts(1, 3, 3, 3, stale);
    assert!(ghosts.iter().all(|g| g.frame < 3));
    assert!(onion_ghosts(5, 3, 1, 1, None).is_empty());
}

#[test]
fn ghosts_stop_at_the_ends_without_a_loop() {
    let frames = |g: Vec<Ghost>| g.iter().map(|g| g.frame).collect::<Vec<_>>();
    assert_eq!(frames(onion_ghosts(0, 5, 2, 1, None)), [1]);
    assert_eq!(frames(onion_ghosts(4, 5, 2, 1, None)), [2, 3]);
    assert_eq!(
        frames(onion_ghosts(2, 5, 1, 1, None)),
        [3, 1],
        "nearest last"
    );
}

#[test]
fn ghosts_wrap_within_a_loop_and_never_repeat() {
    let span = Some(Span {
        from: 1,
        to: 3,
        direction: Direction::Forward,
    });
    let g = onion_ghosts(1, 9, 1, 1, span);
    assert_eq!(g.iter().map(|g| g.frame).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(
        g[1].distance, -1,
        "the frame before a loop starts is its end"
    );
    let many = onion_ghosts(1, 9, 3, 3, span);
    assert_eq!(many.len(), 2);
    assert!(onion_ghosts(0, 1, 2, 2, None).is_empty());
}

#[test]
fn ghosts_outside_the_loop_do_not_wrap() {
    let span = Some(Span {
        from: 1,
        to: 3,
        direction: Direction::Forward,
    });
    let g = onion_ghosts(6, 9, 1, 1, span);
    assert_eq!(g.iter().map(|g| g.frame).collect::<Vec<_>>(), [7, 5]);
}

#[test]
fn timing_changes_merge_into_one_undo_step_until_something_else_happens() {
    let mut e = editor(3);
    let start = e.history.undo_depth();
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![120; 2],
    })
    .expect("timing");
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![130; 2],
    })
    .expect("timing");
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![140; 2],
    })
    .expect("timing");
    assert_eq!(e.history.undo_depth(), start + 1);
    assert_eq!(e.doc.frames[1].duration_ms, 140);
    assert_eq!(e.doc.frames[2].duration_ms, 100);
    e.undo();
    assert_eq!(e.doc.frames[0].duration_ms, 100);
    dot(&mut e, 0, 1);
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![150; 2],
    })
    .expect("timing");
    e.apply(FrameCmd::Retime {
        from: 2,
        durations: vec![150; 1],
    })
    .expect("timing");
    assert_eq!(e.history.undo_depth(), start + 3);
}

#[test]
fn setting_the_same_timing_is_not_an_edit() {
    let mut e = editor(2);
    let (depth, revision) = (e.history.undo_depth(), e.revision);
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![100; 2],
    })
    .expect("timing");
    assert_eq!((e.history.undo_depth(), e.revision), (depth, revision));
}

#[test]
fn timing_stays_in_a_sensible_range() {
    let mut e = editor(1);
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![0; 1],
    })
    .expect("timing");
    assert_eq!(e.doc.frames[0].duration_ms, MIN_DURATION_MS);
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![u32::MAX; 1],
    })
    .expect("timing");
    assert_eq!(e.doc.frames[0].duration_ms, MAX_DURATION_MS);
}

/// The frames shown playing `span` `repeat` times from `start`, until it stops or `limit`.
fn played(span: Span, repeat: u16, start: usize, limit: usize) -> Vec<usize> {
    let (mut head, mut frame, mut shown) = (Playhead::default(), start, vec![start]);
    while shown.len() < limit {
        match span.step_counted(frame, &mut head, repeat) {
            Some(next) => frame = next,
            None => break,
        }
        shown.push(frame);
    }
    shown
}

fn span(from: usize, to: usize, direction: Direction) -> Span {
    Span {
        from,
        to,
        direction,
    }
}

#[test]
fn a_tag_played_twice_shows_its_frames_twice() {
    let forward = span(2, 4, Direction::Forward);
    assert_eq!(played(forward, 2, 2, 99), [2, 3, 4, 2, 3, 4]);
}

#[test]
fn a_tag_that_repeats_forever_never_stops() {
    let forward = span(0, 1, Direction::Forward);
    assert_eq!(played(forward, 0, 0, 7), [0, 1, 0, 1, 0, 1, 0]);
}

#[test]
fn a_reverse_tag_played_once_stops_on_its_first_frame() {
    let reverse = span(1, 3, Direction::Reverse);
    assert_eq!(played(reverse, 1, 3, 99), [3, 2, 1]);
}

#[test]
fn a_ping_pong_counts_each_way_as_a_pass() {
    let there_and_back = span(0, 2, Direction::PingPong);
    assert_eq!(played(there_and_back, 1, 0, 99), [0, 1, 2]);
    assert_eq!(played(there_and_back, 2, 0, 99), [0, 1, 2, 1, 0]);
    assert_eq!(played(there_and_back, 3, 0, 99), [0, 1, 2, 1, 0, 1, 2]);
}

#[test]
fn a_one_frame_tag_played_three_times_shows_it_three_times() {
    for direction in [Direction::Forward, Direction::Reverse, Direction::PingPong] {
        assert_eq!(played(span(5, 5, direction), 3, 5, 99), [5, 5, 5]);
    }
}

#[test]
fn playing_from_the_last_frame_of_a_tag_starts_a_pass_rather_than_ending_one() {
    let forward = span(0, 2, Direction::Forward);
    assert_eq!(played(forward, 1, 2, 99), [2, 0, 1, 2]);
    assert_eq!(
        played(forward, 1, 9, 99),
        [9, 0, 1, 2],
        "from outside, it goes in"
    );
}

#[test]
fn setting_a_repeat_is_one_step_that_undo_takes_back() {
    let mut e = editor(3);
    e.apply(crate::TagCmd::Add {
        name: "walk".into(),
        from: 0,
        to: 2,
    })
    .expect("tag");
    e.apply(crate::StepCmd::EndDrag).expect("named");
    e.apply(crate::TagCmd::SetRepeat { tag: 0, repeat: 4 })
        .expect("set");
    assert_eq!(e.doc.tags[0].repeat, 4);
    e.apply(crate::StepCmd::Undo).expect("undo");
    assert_eq!(e.doc.tags[0].repeat, 0);
    assert_eq!(e.doc.tags.len(), 1, "the tag itself is a step of its own");
    let past = MAX_TAG_REPEAT + 1;
    e.apply(crate::TagCmd::SetRepeat {
        tag: 0,
        repeat: past,
    })
    .expect("kept in range");
    assert_eq!(e.doc.tags[0].repeat, MAX_TAG_REPEAT);
    let missing = crate::TagCmd::SetRepeat { tag: 1, repeat: 1 };
    assert_eq!(e.apply(missing), Err(LimitError::NoSuchItem));
}

#[test]
fn tags_that_differ_only_in_repeat_are_not_equal() {
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
    doc.add_tag("a", 0, 0).expect("tag");
    let mut other = doc.tags[0].clone();
    other.repeat = 2;
    assert_ne!(doc.tags[0], other);
}
