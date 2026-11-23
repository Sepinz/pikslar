//! The time of day on the computer's own clock, for saying when something happened.

#[repr(C)]
#[derive(Default)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLocalTime(time: *mut SystemTime);
}

/// The local hour and minute now.
pub fn local_clock() -> (u16, u16) {
    let mut now = SystemTime::default();
    // SAFETY: `now` is a valid, writable SYSTEMTIME for the call to fill.
    unsafe { GetLocalTime(&mut now) };
    (now.hour, now.minute)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_clock_reads_a_time_of_day() {
        let (h, m) = super::local_clock();
        assert!(h < 24 && m < 60, "{h}:{m}");
    }
}
