//! How long the status line keeps what it says.

use crate::test_app::*;
use pikslar_platform::{Key, Mods};
use std::time::{Duration, Instant};

fn said(a: &crate::app::App) -> Option<&str> {
    a.message.as_ref().map(|(text, _)| text.as_str())
}

#[test]
fn an_error_stays_past_the_news_lifetime_and_goes_on_the_next_key() {
    let mut a = app(1);
    a.notify_error("That name is taken".to_string());
    assert_eq!(said(&a), Some("Error: That name is taken"));
    a.expire_message_at(Instant::now() + Duration::from_secs(10));
    assert!(said(&a).is_some(), "an error outlives ten seconds");
    key(&mut a, Key::Right, Mods::default());
    assert_eq!(said(&a), None, "the next key dismisses it");
}

#[test]
fn an_error_goes_on_the_next_click_too() {
    let mut a = app(1);
    a.notify_error("No".to_string());
    a.pointer_down(5, 5, pikslar_platform::Button::Left, Mods::default());
    assert_eq!(said(&a), None);
}

#[test]
fn news_still_expires_after_its_lifetime() {
    let mut a = app(1);
    a.notify("Saved".to_string());
    a.expire_message_at(Instant::now() + Duration::from_secs(2));
    assert_eq!(said(&a), Some("Saved"));
    a.expire_message_at(Instant::now() + Duration::from_secs(10));
    assert_eq!(said(&a), None);
}

#[test]
fn long_news_lives_longer_than_short_news() {
    let mut a = app(1);
    a.notify("x".repeat(160));
    a.expire_message_at(Instant::now() + Duration::from_secs(5));
    assert!(said(&a).is_some(), "a long sentence takes longer to read");
}

#[test]
fn a_real_key_dismisses_an_error_where_a_bare_modifier_makes_no_event() {
    // The platform makes no event for a bare Shift, Ctrl, Alt or Win press (see the keymap
    // test), so the app is never asked. A real key with a modifier is a real key.
    let mut a = app(1);
    a.notify_error("No".to_string());
    key(&mut a, Key::Char(b'Q'), Mods::default());
    assert_eq!(said(&a), None);
}

#[test]
fn starting_to_open_a_file_clears_an_error_and_its_mark() {
    let mut a = app(1);
    a.notify_error("No".to_string());
    a.begin_open(
        pikslar_platform::Window::detached(),
        std::env::temp_dir().join("pikslar-a11y-none.ase"),
        crate::loading::Then::Nothing,
    );
    assert!(!a.message_is_error, "the mark goes with the message");
    a.notify("Saved".to_string());
    a.expire_message_at(Instant::now() + Duration::from_secs(10));
    assert_eq!(
        said(&a),
        None,
        "the next news expires as news, not as an error"
    );
}

#[test]
fn news_does_not_replace_an_unread_error() {
    let mut a = app(1);
    a.notify_error("Could not back up your work".to_string());
    a.notify("Saved sprite.piks".to_string());
    assert_eq!(said(&a), Some("Error: Could not back up your work"));
    assert!(a.message_is_error, "it is still an error that stays");
    a.expire_message_at(Instant::now() + Duration::from_secs(10));
    assert!(said(&a).is_some(), "and the news did not shorten its life");
}

#[test]
fn a_newer_error_replaces_an_older_one_and_news_flows_again_after_a_key() {
    let mut a = app(1);
    a.notify_error("First".to_string());
    a.notify_error("Second".to_string());
    assert_eq!(said(&a), Some("Error: Second"));
    key(&mut a, Key::Char(b'Q'), Mods::default());
    a.notify("Saved".to_string());
    assert_eq!(said(&a), Some("Saved"));
}
