//! Fault injection for the saving-stall investigation (docs/b1-stall-investigation.md).
//!
//! With `PIKSLAR_TEST_PAUSE_AFTER_SAVE_MS` set to a number of milliseconds (1 to 60000), the
//! window's thread sleeps that long straight after each successful save, as a busy disk or a
//! paused process would make it, so a harness can be tested against a window that stops
//! reading its queue. The variable is read once. Unset, empty or out of range, it does
//! nothing, and saving behaves exactly as without it.
//!
//! The variable is read only in a build with the `fault-injection` feature, which is off by
//! default: a release build cannot pause however its environment is set.

#[cfg(any(test, feature = "fault-injection"))]
use std::ffi::OsString;
#[cfg(feature = "fault-injection")]
use std::sync::OnceLock;
use std::time::Duration;

/// The environment variable that names the pause.
#[cfg(any(test, feature = "fault-injection"))]
pub(crate) const VAR: &str = "PIKSLAR_TEST_PAUSE_AFTER_SAVE_MS";
/// The longest pause accepted, so a typing mistake cannot freeze the program for long.
#[cfg(any(test, feature = "fault-injection"))]
const LONGEST_MS: u64 = 60_000;

#[cfg(feature = "fault-injection")]
static PAUSE: OnceLock<Option<Duration>> = OnceLock::new();

/// The pause a value of the variable asks for, if it asks for one.
#[cfg(any(test, feature = "fault-injection"))]
pub(crate) fn pause_from(value: Option<OsString>) -> Option<Duration> {
    let ms: u64 = value?.to_str()?.trim().parse().ok()?;
    (1..=LONGEST_MS)
        .contains(&ms)
        .then(|| Duration::from_millis(ms))
}

/// Sleeps for the injected pause, if one was asked for when the program started. Does nothing
/// without the `fault-injection` feature.
pub(crate) fn pause_after_save() {
    if let Some(pause) = injected() {
        std::thread::sleep(pause);
    }
}

#[cfg(feature = "fault-injection")]
fn injected() -> Option<Duration> {
    *PAUSE.get_or_init(|| pause_from(std::env::var_os(VAR)))
}

#[cfg(not(feature = "fault-injection"))]
fn injected() -> Option<Duration> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pause(text: &str) -> Option<Duration> {
        pause_from(Some(OsString::from(text)))
    }

    #[test]
    fn without_the_variable_nothing_pauses() {
        assert_eq!(pause_from(None), None);
        assert_eq!(pause(""), None);
    }

    #[test]
    fn a_number_of_milliseconds_is_the_pause() {
        assert_eq!(pause("6000"), Some(Duration::from_millis(6000)));
        assert_eq!(pause(" 12 "), Some(Duration::from_millis(12)));
        assert_eq!(pause("60000"), Some(Duration::from_secs(60)));
    }

    #[test]
    fn zero_too_long_or_not_a_number_does_nothing() {
        for text in ["0", "60001", "-5", "6s", "1e3", "abc"] {
            assert_eq!(pause(text), None, "{text}");
        }
    }

    #[test]
    #[cfg(not(feature = "fault-injection"))]
    fn a_build_without_fault_injection_never_reads_the_variable() {
        std::env::set_var(VAR, "300");
        let started = std::time::Instant::now();
        pause_after_save();
        assert!(started.elapsed() < Duration::from_millis(300));
        assert_eq!(injected(), None);
        std::env::remove_var(VAR);
    }

    #[test]
    fn the_name_is_fixed() {
        assert_eq!(VAR, "PIKSLAR_TEST_PAUSE_AFTER_SAVE_MS");
    }
}
