//! The timeline as an artist uses it: through its keys, its strip and its playback.

use crate::app::App;
use crate::shortcuts::Action;
use pikslar_core::{Direction, Span};
use pikslar_platform::{Button, Key, Mods};
use pikslar_render::Surface;
use std::time::{Duration, Instant};

const WINDOW: (i32, i32) = (1200, 700);
const SHIFT: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: false,
};

fn app(frames: usize) -> App {
    let mut a = App::launch(None, Some(32), None);
    a.resized(WINDOW.0, WINDOW.1, 1);
    for _ in 1..frames {
        a.new_frame(false);
    }
    a.set_frame(0);
    draw(&mut a);
    a
}

fn draw(a: &mut App) {
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let mut surface = Surface {
        px: &mut px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    a.draw_for_test(&mut surface);
}

fn cell_x(a: &App, frame: usize) -> i32 {
    a.timeline.strip.x + frame as i32 * 34 + 10
}

fn press_strip(a: &mut App, frame: usize, mods: Mods) {
    let (x, y) = (cell_x(a, frame), a.timeline.strip.y + 5);
    a.pointer_down(x, y, Button::Left, mods);
    draw(a);
}

fn release(a: &mut App) {
    let (x, y) = a.ui.mouse;
    a.pointer_up(x, y, Button::Left);
    draw(a);
}

fn click_strip(a: &mut App, frame: usize, mods: Mods) {
    press_strip(a, frame, mods);
    release(a);
}

fn select(a: &mut App, from: usize, to: usize) {
    click_strip(a, from, Mods::default());
    click_strip(a, to, SHIFT);
}

fn type_name(a: &mut App, name: &str) {
    for c in name.chars() {
        a.text_typed(c);
    }
}

#[test]
fn pressing_on_the_strip_goes_to_that_frame_and_dragging_scrubs() {
    let mut a = app(5);
    press_strip(&mut a, 2, Mods::default());
    assert_eq!(a.editor.frame, 2);
    a.ui.track(cell_x(&a, 4), a.timeline.strip.y + 5);
    draw(&mut a);
    assert_eq!(a.editor.frame, 4);
    release(&mut a);
    assert_eq!(a.timeline.selected(), None);
}

#[test]
fn shift_selects_frames_and_a_plain_click_forgets_them() {
    let mut a = app(5);
    select(&mut a, 1, 3);
    assert_eq!(a.timeline.selected(), Some((1, 3)));
    click_strip(&mut a, 0, Mods::default());
    assert_eq!(a.timeline.selected(), None);
}

#[test]
fn stepping_stays_inside_the_selected_frames() {
    let mut a = app(5);
    select(&mut a, 1, 2);
    a.set_frame(2);
    a.step_frame(true);
    assert_eq!(a.editor.frame, 1);
    a.step_frame(false);
    assert_eq!(a.editor.frame, 2);
    assert!(a.clear_timeline());
    a.step_frame(true);
    assert_eq!(a.editor.frame, 3);
}

#[test]
fn new_and_duplicate_frames_arrive_after_the_current_one() {
    let mut a = app(2);
    a.timeline_action(Action::NewFrame);
    assert_eq!((a.editor.frame, a.editor.doc().frames.len()), (1, 3));
    a.editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    assert_eq!(a.editor.doc().frames.len(), 2);
    a.timeline_action(Action::DuplicateFrame);
    assert_eq!(a.editor.doc().frames.len(), 3);
}

#[test]
fn removing_several_frames_is_one_undo() {
    let mut a = app(5);
    select(&mut a, 1, 3);
    a.remove_frames();
    assert_eq!(a.editor.doc().frames.len(), 2);
    assert_eq!(a.editor.frame, 1);
    a.editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    assert_eq!(a.editor.doc().frames.len(), 5);
}

#[test]
fn every_frame_cannot_be_removed() {
    let mut a = app(3);
    select(&mut a, 0, 2);
    a.remove_frames();
    assert_eq!(a.editor.doc().frames.len(), 3, "nothing half done");
}

#[test]
fn a_frame_moves_with_the_cursor_and_stops_at_the_ends() {
    let mut a = app(3);
    a.editor
        .apply(pikslar_core::FrameCmd::Retime {
            from: 0,
            durations: vec![300],
        })
        .expect("timing");
    a.move_frame(true);
    assert_eq!(a.editor.frame, 1);
    assert_eq!(a.editor.doc().frames[1].duration_ms, 300);
    a.move_frame(true);
    a.move_frame(true);
    assert_eq!(a.editor.frame, 2, "stops at the end");
    a.set_frame(0);
    a.move_frame(false);
    assert_eq!(a.editor.frame, 0, "stops at the start");
}

#[test]
fn k_holds_the_previous_drawing_and_undoes_it() {
    let mut a = app(3);
    a.set_frame(1);
    a.toggle_hold();
    assert!(a.editor.doc().held_into_next(0, 0));
    a.toggle_hold();
    assert!(!a.editor.doc().is_held(0, 1));
    a.editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    assert!(a.editor.doc().is_held(0, 1));
}

#[test]
fn the_first_frame_has_nothing_to_hold() {
    let mut a = app(2);
    a.toggle_hold();
    assert!(!a.editor.doc().is_held(0, 0));
    assert!(a.message.is_some());
}

#[test]
fn shift_k_holds_across_the_layer_or_the_selected_frames() {
    let mut a = app(4);
    a.hold_across();
    assert!((0..4).all(|f| a.editor.doc().is_held(0, f)));
    a.editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    select(&mut a, 1, 2);
    a.hold_across();
    assert!(a.editor.doc().held_into_next(0, 1));
    assert!(!a.editor.doc().is_held(0, 3));
}

#[test]
fn a_tag_is_made_from_the_selection_and_named_by_typing() {
    let mut a = app(5);
    select(&mut a, 1, 3);
    a.tag_key();
    assert_eq!(a.timeline.naming, Some(0));
    type_name(&mut a, "run-2");
    a.naming_key(Key::Enter, Mods::default());
    let tag = &a.editor.doc().tags[0];
    assert_eq!((tag.name.as_str(), tag.from, tag.to), ("run-2", 1, 3));
    assert_eq!(a.timeline.tag, Some(0), "it starts out looping");
    assert_eq!(a.timeline.naming, None);
}

#[test]
fn an_unnamed_tag_still_gets_a_name() {
    let mut a = app(2);
    a.tag_key();
    a.finish_naming();
    assert_eq!(a.editor.doc().tags[0].name, "tag");
}

#[test]
fn backspace_edits_and_escape_abandons_a_new_tag() {
    let mut a = app(2);
    a.tag_key();
    type_name(&mut a, "ab");
    a.naming_key(Key::Backspace, Mods::default());
    assert_eq!(a.editor.doc().tags[0].name, "a");
    a.naming_key(Key::Escape, Mods::default());
    assert!(a.editor.doc().tags.is_empty());
    assert_eq!(a.timeline.naming, None);
    assert_eq!(a.timeline.tag, None);
}

#[test]
fn t_on_a_looping_tag_renames_it_and_delete_removes_it() {
    let mut a = app(3);
    a.tag_key();
    type_name(&mut a, "ab");
    a.finish_naming();
    a.tag_key();
    assert_eq!(a.timeline.naming, Some(0));
    a.naming_key(Key::Backspace, Mods::default());
    a.finish_naming();
    assert_eq!(a.editor.doc().tags[0].name, "a");
    a.tag_key();
    a.naming_key(Key::Delete, Mods::default());
    assert_eq!(a.editor.doc().tags.len(), 1, "Delete edits the name");
    a.naming_key(Key::Delete, SHIFT);
    assert!(a.editor.doc().tags.is_empty());
    assert_eq!(a.timeline.tag, None);
}

#[test]
fn clicking_elsewhere_keeps_the_name_typed_so_far() {
    let mut a = app(3);
    a.tag_key();
    type_name(&mut a, "x");
    let c = a.layout.canvas;
    a.pointer_down(c.x + 5, c.y + 5, Button::Left, Mods::default());
    a.pointer_up(c.x + 5, c.y + 5, Button::Left);
    assert_eq!(a.timeline.naming, None);
    assert_eq!(a.editor.doc().tags[0].name, "x");
}

#[test]
fn the_wheel_does_not_change_timing_while_naming() {
    let mut a = app(2);
    a.tag_key();
    let area = a.timeline.duration_area;
    a.timeline_wheel(2, area.x + 5, area.y + 5);
    assert_eq!(a.editor.doc().frames[0].duration_ms, 100);
    a.naming_key(Key::Escape, Mods::default());
    assert!(a.editor.doc().tags.is_empty());
}

#[test]
fn a_cancelled_new_tag_leaves_nothing_to_undo_or_redo() {
    let mut a = app(2);
    let depth = a.editor.history().undo_depth();
    a.tag_key();
    a.naming_key(Key::Escape, Mods::default());
    assert!(a.editor.doc().tags.is_empty());
    assert_eq!(a.editor.history().undo_depth(), depth);
    assert_eq!(a.editor.history().redo_depth(), 0);
}

fn start_stroke(a: &mut App) {
    let c = a.layout.canvas;
    a.set_tool(crate::app::ToolChoice::Paint(pikslar_core::Tool::Pencil));
    let (x, y) = (c.x + c.w / 2, c.y + c.h / 2);
    a.pointer_down(x, y, Button::Left, Mods::default());
    assert!(
        a.gesture != crate::app::Gesture::None,
        "a stroke is under way"
    );
}

#[test]
fn frame_keys_during_a_stroke_keep_the_stroke() {
    let mut a = app(2);
    let depth = a.editor.history().undo_depth();
    start_stroke(&mut a);
    a.timeline_action(Action::NewFrame);
    assert_eq!(a.gesture, crate::app::Gesture::None);
    assert_eq!(
        a.editor.history().undo_depth(),
        depth + 2,
        "the stroke and the frame"
    );
}

#[test]
fn a_timing_change_during_a_stroke_keeps_the_stroke() {
    let mut a = app(2);
    let depth = a.editor.history().undo_depth();
    start_stroke(&mut a);
    a.nudge_timing(100);
    assert_eq!(a.editor.history().undo_depth(), depth + 2);
}

#[test]
fn undo_bringing_a_tag_back_does_not_loop_the_wrong_one() {
    let mut a = app(4);
    a.tag_key();
    a.finish_naming();
    a.set_frame(2);
    a.tag_key();
    a.finish_naming();
    a.sync_onion();
    a.run(pikslar_core::TagCmd::Remove { tag: 0 });
    a.sync_onion();
    a.timeline.tag = Some(0);
    a.sync_onion();
    assert_eq!(a.timeline.tag, Some(0), "picked after the change");
    a.editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    a.sync_onion();
    assert_eq!(a.timeline.tag, None);
}

#[test]
fn undoing_a_tag_away_stops_looping_it() {
    let mut a = app(3);
    a.tag_key();
    a.finish_naming();
    a.sync_onion();
    a.editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    a.sync_onion();
    assert_eq!(a.timeline.tag, None);
}

#[test]
fn removing_frames_under_a_looping_tag_keeps_painting() {
    let mut a = app(6);
    a.toggle_onion();
    select(&mut a, 3, 5);
    a.tag_key();
    a.finish_naming();
    a.set_frame(3);
    a.sync_onion();
    a.remove_frames();
    a.editor.damage_all();
    let damage = a.editor.take_damage();
    a.comp.update(&a.editor, damage, &mut Vec::new());
}

#[test]
fn the_keyboard_belongs_to_the_name_while_naming() {
    let mut a = app(3);
    a.tag_key();
    a.key_down(
        Key::Char(b'N'),
        Mods::default(),
        false,
        pikslar_platform::Window::detached(),
    );
    a.text_typed('n');
    assert_eq!(a.editor.doc().frames.len(), 3, "typed, not a new frame");
    assert_eq!(a.editor.doc().tags[0].name, "n");
}

/// Windows sends the T that starts naming as a character too; it is not part of the name.
#[test]
fn the_key_that_starts_naming_does_not_type_itself() {
    let mut a = app(3);
    crate::test_app::key(&mut a, Key::Char(b'T'), Mods::default());
    assert_eq!(a.timeline.naming, Some(0));
    assert_eq!(a.editor.doc().tags[0].name, "");
    crate::test_app::key(&mut a, Key::Char(b'T'), Mods::default());
    crate::test_app::key(&mut a, Key::Right, Mods::default());
    a.text_typed('x');
    assert_eq!(a.editor.doc().tags[0].name, "tx", "typed while naming");
    // A shortcut key outside naming spends only its own character, not one typed later.
    a.naming_key(Key::Enter, Mods::default());
    crate::test_app::key(&mut a, Key::Right, Mods::default());
    let window = pikslar_platform::Window::detached();
    pikslar_platform::Handler::event(&mut a, pikslar_platform::Event::Move { x: 1, y: 1 }, window);
    // Naming started without a key, as from a menu.
    a.tag_key();
    a.text_typed('y');
    let named = a.timeline.naming.expect("naming");
    assert!(a.editor.doc().tags[named].name.ends_with('y'));
}

#[test]
fn a_released_shortcut_or_a_held_t_leaves_the_name_alone() {
    let window = pikslar_platform::Window::detached();
    let up = |a: &mut App, key| {
        pikslar_platform::Handler::event(a, pikslar_platform::Event::KeyUp { key }, window)
    };
    let mut a = app(3);
    // Posted messages can put T's release ahead of its character.
    a.key_down(Key::Char(b'T'), Mods::default(), false, window);
    up(&mut a, Key::Char(b'T'));
    a.text_typed('t');
    assert_eq!(
        a.editor.doc().tags[0].name,
        "",
        "T released first still types nothing"
    );
    a.naming_key(Key::Escape, Mods::default());
    crate::test_app::key(&mut a, Key::Char(b'T'), Mods::default());
    // T held: its repeats come while naming, and are still T.
    a.key_down(Key::Char(b'T'), Mods::default(), true, window);
    a.text_typed('t');
    assert_eq!(a.editor.doc().tags[0].name, "", "a held T types nothing");
    up(&mut a, Key::Char(b'T'));
    a.naming_key(Key::Escape, Mods::default());
    // A key with no character, released: a character with no key after it is typed.
    crate::test_app::key(&mut a, Key::Right, Mods::default());
    up(&mut a, Key::Right);
    a.tag_key();
    a.text_typed('z');
    let named = a.timeline.naming.expect("naming");
    assert!(a.editor.doc().tags[named].name.ends_with('z'));
}

#[test]
fn shift_with_the_arrows_selects_part_of_the_name() {
    let mut a = app(2);
    a.tag_key();
    type_name(&mut a, "abc");
    a.naming_key(Key::Left, SHIFT);
    a.naming_key(Key::Left, SHIFT);
    a.text_typed('x');
    assert_eq!(a.editor.doc().tags[0].name, "ax");
    a.naming_key(Key::Home, Mods::default());
    a.naming_key(Key::Right, Mods::default());
    a.naming_key(Key::End, SHIFT);
    a.text_typed('y');
    assert_eq!(a.editor.doc().tags[0].name, "ay");
}

#[test]
fn names_stop_at_the_limit() {
    let mut a = app(2);
    a.tag_key();
    type_name(&mut a, &"a".repeat(40));
    assert_eq!(
        a.editor.doc().tags[0].name.len(),
        pikslar_core::MAX_TAG_NAME
    );
}

#[test]
fn shift_t_cycles_the_direction_of_the_looping_tag() {
    let mut a = app(3);
    a.cycle_direction();
    assert!(a.message.is_some(), "says what to do without a tag");
    a.tag_key();
    a.finish_naming();
    a.cycle_direction();
    assert_eq!(a.editor.doc().tags[0].direction, Direction::PingPong);
    a.editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    assert_eq!(a.editor.doc().tags[0].direction, Direction::Forward);
}

#[test]
fn playback_loops_a_tag_in_its_direction() {
    let mut a = app(5);
    select(&mut a, 1, 2);
    a.tag_key();
    a.finish_naming();
    a.set_frame(4);
    a.toggle_play();
    let mut seen = Vec::new();
    for _ in 0..4 {
        a.next_frame_at = Instant::now();
        a.advance_playback();
        seen.push(a.editor.frame);
    }
    assert_eq!(seen, [1, 2, 1, 2]);
}

#[test]
fn playback_keeps_to_its_schedule_when_woken_late() {
    let mut a = app(3);
    a.toggle_play();
    let due = Instant::now();
    a.next_frame_at = due;
    a.advance_playback();
    assert_eq!(a.next_frame_at, due + Duration::from_millis(100));
}

#[test]
fn playback_far_behind_starts_over_rather_than_racing() {
    let mut a = app(3);
    a.toggle_play();
    let long_ago = Instant::now() - Duration::from_secs(5);
    a.next_frame_at = long_ago;
    a.advance_playback();
    assert!(a.next_frame_at > Instant::now());
}

#[test]
fn the_onion_skin_follows_the_loop_and_hides_during_playback() {
    let mut a = app(4);
    a.toggle_onion();
    select(&mut a, 1, 2);
    a.sync_onion();
    let span = Span {
        from: 1,
        to: 2,
        direction: Direction::Forward,
    };
    assert_eq!(a.comp.onion_loop, Some(span));
    a.toggle_play();
    a.sync_onion();
    assert!(a.comp.playing);
}

#[test]
fn onion_depth_cycles_and_turns_the_skin_on() {
    let mut a = app(2);
    assert!(!a.comp.onion);
    a.timeline_action(Action::OnionDepth);
    assert!(a.comp.onion);
    assert_eq!(a.comp.onion_depth, 2);
    a.cycle_onion_depth();
    a.cycle_onion_depth();
    assert_eq!(a.comp.onion_depth, 1);
}

#[test]
fn a_drag_on_the_timing_is_one_undo_and_reaches_the_selection() {
    let mut a = app(4);
    select(&mut a, 1, 2);
    a.set_frame(1);
    draw(&mut a);
    let area = a.timeline.duration_area;
    let depth = a.editor.history().undo_depth();
    let (x, y) = (area.x + 10, area.y + 5);
    a.pointer_down(x, y, Button::Left, Mods::default());
    draw(&mut a);
    for step in 1..=3 {
        a.ui.track(x + step * 6, y);
        draw(&mut a);
    }
    release(&mut a);
    assert_eq!(a.editor.doc().frames[1].duration_ms, 160);
    assert_eq!(a.editor.doc().frames[2].duration_ms, 160);
    assert_eq!(a.editor.doc().frames[3].duration_ms, 100);
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
}

#[test]
fn the_wheel_scrolls_the_strip_and_changes_timing() {
    let mut a = app(200);
    let strip = a.timeline.strip;
    a.timeline_wheel(-3, strip.x + 5, strip.y + 5);
    assert_eq!(a.timeline.scrolled(0, 200, 10), 3);
    let area = a.timeline.duration_area;
    a.timeline_wheel(2, area.x + 5, area.y + 5);
    assert_eq!(a.editor.doc().frames[0].duration_ms, 120);
}

#[test]
fn a_moved_frame_outside_the_view_is_followed() {
    let mut a = app(200);
    a.set_frame(150);
    draw(&mut a);
    let first = a.timeline.scrolled(150, 200, 10);
    assert!(first <= 150);
    assert!(150 < first + (a.timeline.strip.w / 34) as usize);
}

/// The frames playback shows over `steps` steps, or until it stops.
fn play(a: &mut App, steps: usize) -> Vec<usize> {
    a.toggle_play();
    let mut seen = Vec::new();
    for _ in 0..steps {
        a.next_frame_at = Instant::now();
        a.advance_playback();
        if !a.playing {
            break;
        }
        seen.push(a.editor.frame);
    }
    seen
}

#[test]
fn a_tag_played_twice_stops_on_its_last_frame() {
    let mut a = app(5);
    select(&mut a, 1, 2);
    a.tag_key();
    a.finish_naming();
    a.run(pikslar_core::TagCmd::SetRepeat { tag: 0, repeat: 2 });
    a.set_frame(1);
    assert_eq!(play(&mut a, 9), [2, 1, 2]);
    assert!(!a.playing);
    assert_eq!(a.editor.frame, 2);
    assert_eq!(
        play(&mut a, 9),
        [1, 2, 1, 2],
        "playing again starts the tag over"
    );
}

#[test]
fn the_tag_menu_sets_how_many_times_a_tag_plays_in_one_step() {
    let mut a = app(3);
    a.tag_key();
    a.finish_naming();
    let items = a.tag_items(0);
    assert_eq!(items[0].label, "Play forever");
    assert!(!items[0].enabled, "in use");
    let pushes = a.editor.history().pushes();
    a.tag_menu_action(0, 3);
    assert_eq!(a.editor.doc().tags[0].repeat, 3);
    assert_eq!(a.editor.history().pushes(), pushes + 1);
    assert!(!a.tag_items(0)[3].enabled);
    a.tag_menu_action(0, 99);
    a.tag_menu_action(7, 1);
    assert_eq!(
        a.editor.history().pushes(),
        pushes + 1,
        "nothing there, no step"
    );
}

#[test]
fn a_tag_shows_how_many_times_it_plays() {
    let mut a = app(2);
    a.tag_key();
    a.finish_naming();
    let tag = a.editor.doc().tags[0].clone();
    assert!(!crate::panel_timeline::tag_label(&tag, false).contains('×'));
    a.tag_menu_action(0, 2);
    let tag = &a.editor.doc().tags[0];
    assert!(crate::panel_timeline::tag_label(tag, false).ends_with(" ×2"));
}

fn set_rate(a: &mut App, fps: Option<u8>) {
    a.run(pikslar_core::FrameCmd::SetRate { fps });
}

#[test]
fn twenty_four_one_tick_frames_take_exactly_a_second_at_24_a_second() {
    let mut a = app(24);
    set_rate(&mut a, Some(24));
    a.run(pikslar_core::FrameCmd::Retime {
        from: 0,
        durations: vec![42; 24],
    });
    assert_eq!(a.editor.doc().frame_units(5), 1);
    a.toggle_play();
    let total: Duration = (0..24).map(|_| a.next_wait()).sum();
    assert_eq!(total, Duration::from_secs(1));
}

#[test]
fn a_frame_waits_its_milliseconds_in_milliseconds() {
    let mut a = app(2);
    a.toggle_play();
    assert_eq!(a.next_wait(), Duration::from_millis(100));
    assert_eq!(a.next_wait(), Duration::from_millis(100));
}

#[test]
fn a_step_of_duration_is_a_tick_at_a_frame_rate_and_ten_ms_otherwise() {
    let mut a = app(2);
    a.nudge_timing(1);
    assert_eq!(a.editor.doc().frames[0].duration_ms, 110);
    set_rate(&mut a, Some(24));
    assert_eq!(a.editor.doc().frame_units(0), 3, "110 ms is 3 ticks");
    a.nudge_timing(1);
    assert_eq!(a.editor.doc().frame_units(0), 4);
    a.nudge_timing(-9);
    assert_eq!(a.editor.doc().frame_units(0), 1, "never under a tick");
}

#[test]
fn the_duration_shows_ticks_at_a_frame_rate() {
    use crate::panel_timeline::duration_label;
    assert_eq!(duration_label(&[100], None), "100ms");
    assert_eq!(duration_label(&[83], Some(24)), "2f");
    assert_eq!(duration_label(&[83, 125], Some(24)), "~2f x2");
    assert_eq!(duration_label(&[83, 83], Some(24)), "2f x2");
}

#[test]
fn the_rate_menu_sets_the_timing_in_one_step() {
    let mut a = app(2);
    let labels: Vec<_> = a.rate_items().iter().map(|i| i.label).collect();
    assert_eq!(
        labels,
        [
            "Time in milliseconds",
            "12 fps",
            "24 fps",
            "30 fps",
            "60 fps"
        ]
    );
    assert!(!a.rate_items()[0].enabled, "in use");
    let pushes = a.editor.history().pushes();
    a.rate_menu_action(2);
    assert_eq!(a.editor.doc().frame_rate, Some(24));
    assert_eq!(a.editor.history().pushes(), pushes + 1);
    let items = a.rate_items();
    assert!(items[0].enabled && !items[2].enabled);
    a.rate_menu_action(0);
    assert_eq!(a.editor.doc().frame_rate, None);
    a.rate_menu_action(99);
    assert_eq!(a.editor.history().pushes(), pushes + 2, "nothing there");
}

#[test]
fn a_right_click_on_the_duration_opens_the_rate_menu() {
    let mut a = app(2);
    let r = a.timeline.readout;
    assert!(!a.open_rate_menu(0, 0));
    assert!(a.open_rate_menu(r.x + r.w / 2, r.y + r.h / 2));
    assert!(matches!(
        a.menu.map(|m| m.kind),
        Some(crate::frame_menu::MenuKind::Rate)
    ));
}

#[test]
fn a_held_scrub_steps_on_the_injected_clock_not_the_wall_clock() {
    use std::time::{Duration, Instant};
    let mut t = crate::timeline::Timeline::default();
    let base = Instant::now();
    t.clock = Some(base);
    assert!(t.edge_step_due(), "the first is due at once");
    t.clock = Some(base + Duration::from_millis(44));
    assert!(!t.edge_step_due());
    t.clock = Some(base + Duration::from_millis(45));
    assert!(t.edge_step_due());
}
