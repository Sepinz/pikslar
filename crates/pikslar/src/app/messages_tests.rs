//! News that comes while an error is showing waits for it rather than being dropped.

use crate::test_app::*;

#[test]
fn news_during_an_error_is_said_once_the_error_is_taken_down() {
    let mut a = app(1);
    a.notify_error("the disk is full".into());
    a.notify("Saved art.piks".into());
    assert_eq!(said(&a), "Error: the disk is full", "the error stays");
    a.dismiss_error();
    assert_eq!(said(&a), "Saved art.piks", "the news is not lost");
}

#[test]
fn only_the_latest_news_waits_and_a_new_error_still_shows() {
    let mut a = app(1);
    a.notify_error("first".into());
    a.notify("Saved a".into());
    a.notify("Saved b".into());
    a.notify_error("second".into());
    assert_eq!(said(&a), "Error: second");
    a.dismiss_error();
    assert_eq!(said(&a), "Saved b");
}

#[test]
fn news_with_no_error_showing_is_said_at_once() {
    let mut a = app(1);
    a.notify("Saved a".into());
    assert_eq!(said(&a), "Saved a");
    assert!(a.held_news.is_none());
}

#[test]
fn saved_news_held_behind_an_error_goes_when_a_later_save_starts_or_fails() {
    let mut a = app(1);
    a.notify_error("something else".into());
    a.notify("Saved art.piks".into());
    a.save_started();
    a.save_failed("the disk is full".into());
    a.dismiss_error();
    assert!(
        !said(&a).starts_with("Saved"),
        "no Saved after a save that failed: {}",
        said(&a)
    );
    a.notify_error("again".into());
    a.notify("Saved art.piks".into());
    a.save_failed("the disk is full".into());
    a.dismiss_error();
    assert!(!said(&a).starts_with("Saved"), "{}", said(&a));
}
