//! Which events may join the queue. While a modal loop runs inside the handler nothing drains
//! the queue, so input aimed at the window it cannot use must not pile up without bound.

use crate::Event;

/// The most events the queue holds; beyond it only a close request is let in.
pub(crate) const MAX_QUEUED: usize = 4096;

/// Whether an event undoes or ends something, so that dropping it could leave a key or button
/// held, or the window out of date.
fn restores(event: &Event) -> bool {
    matches!(
        event,
        Event::KeyUp { .. }
            | Event::Up { .. }
            | Event::CaptureLost
            | Event::Leave
            | Event::CloseRequested
            | Event::Resized { .. }
            | Event::LookChanged
            | Event::Dropped(_)
    )
}

/// Adds `event` to the queue, or drops it. While `busy`, input that starts something (moves,
/// presses, wheel, key downs, text) is dropped; what restores state is kept. Consecutive moves
/// collapse into the last, repeated `Leave` and `CaptureLost` into one, and a full queue takes
/// nothing but a close request.
pub(crate) fn admit(events: &mut Vec<Event>, event: Event, busy: bool) {
    if busy && !restores(&event) {
        return;
    }
    if events.len() >= MAX_QUEUED && event != Event::CloseRequested {
        return;
    }
    match (events.last_mut(), &event) {
        (Some(last @ Event::Move { .. }), Event::Move { .. }) => *last = event,
        (Some(last), Event::Leave | Event::CaptureLost) if *last == event => {}
        _ => events.push(event),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Button, Key, Mods};

    fn down() -> Event {
        Event::Down {
            x: 1,
            y: 2,
            button: Button::Left,
            mods: Mods::default(),
        }
    }

    #[test]
    fn idle_input_is_queued_in_order() {
        let mut q = Vec::new();
        admit(&mut q, down(), false);
        admit(&mut q, Event::Text('a'), false);
        assert_eq!(q, vec![down(), Event::Text('a')]);
    }

    #[test]
    fn busy_drops_input_that_starts_something() {
        let mut q = Vec::new();
        let key = Event::KeyDown {
            key: Key::Escape,
            mods: Mods::default(),
            repeat: false,
        };
        for e in [
            Event::Move { x: 1, y: 1 },
            down(),
            Event::Wheel {
                x: 0,
                y: 0,
                ticks: 1,
            },
            key,
            Event::Text('x'),
        ] {
            admit(&mut q, e, true);
        }
        assert!(q.is_empty());
    }

    #[test]
    fn busy_keeps_what_restores_state() {
        let mut q = Vec::new();
        let keep = [
            Event::KeyUp { key: Key::Escape },
            Event::Up {
                x: 0,
                y: 0,
                button: Button::Left,
            },
            Event::CaptureLost,
            Event::Leave,
            Event::LookChanged,
            Event::CloseRequested,
        ];
        for e in keep.clone() {
            admit(&mut q, e, true);
        }
        assert_eq!(q, keep);
    }

    #[test]
    fn moves_collapse_and_repeats_of_leave_fold() {
        let mut q = Vec::new();
        for i in 0..100 {
            admit(&mut q, Event::Move { x: i, y: i }, false);
        }
        assert_eq!(q, vec![Event::Move { x: 99, y: 99 }]);
        admit(&mut q, Event::Leave, true);
        admit(&mut q, Event::Leave, true);
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn a_full_queue_refuses_all_but_close() {
        let mut q = Vec::new();
        for _ in 0..MAX_QUEUED * 2 {
            admit(&mut q, Event::KeyUp { key: Key::Escape }, true);
        }
        assert_eq!(q.len(), MAX_QUEUED);
        admit(&mut q, Event::CloseRequested, true);
        assert_eq!(q.len(), MAX_QUEUED + 1);
    }
}
