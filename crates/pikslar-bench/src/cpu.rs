//! Processor time beside elapsed time, as `spikes/limits.ps1` reads it from the real program:
//! the cycles every thread of the process ran, turned into milliseconds by the cycles one busy
//! thread runs in a millisecond. Windows charges `GetProcessTimes` at timer ticks, so that one
//! is printed too, as a check. Elsewhere than Windows, no processor time is read.

use std::time::Instant;

#[cfg(windows)]
mod sys {
    #[repr(C)]
    #[derive(Default)]
    pub struct FileTime {
        low: u32,
        high: u32,
    }

    impl FileTime {
        pub fn ticks(&self) -> u64 {
            (u64::from(self.high) << 32) | u64::from(self.low)
        }
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetCurrentThread() -> isize;
        fn QueryProcessCycleTime(process: isize, cycles: *mut u64) -> i32;
        fn QueryThreadCycleTime(thread: isize, cycles: *mut u64) -> i32;
        fn GetProcessTimes(
            process: isize,
            created: *mut FileTime,
            ended: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    /// The cycles every thread of this process has run.
    pub fn process_cycles() -> u64 {
        let mut cycles = 0;
        // SAFETY: the pseudo handle needs no closing and `cycles` outlives the call.
        unsafe { QueryProcessCycleTime(GetCurrentProcess(), &mut cycles) };
        cycles
    }

    /// The cycles the calling thread has run.
    pub fn thread_cycles() -> u64 {
        let mut cycles = 0;
        // SAFETY: as above, for the calling thread's pseudo handle.
        unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) };
        cycles
    }

    /// Kernel and user time charged to this process, in 100 ns units.
    pub fn process_ticks() -> u64 {
        let mut t: [FileTime; 4] = Default::default();
        let [created, ended, kernel, user] = &mut t;
        // SAFETY: the four out pointers are valid for the call.
        unsafe { GetProcessTimes(GetCurrentProcess(), created, ended, kernel, user) };
        kernel.ticks() + user.ticks()
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn process_cycles() -> u64 {
        0
    }
    pub fn thread_cycles() -> u64 {
        0
    }
    pub fn process_ticks() -> u64 {
        0
    }
}

/// How many cycles one busy thread runs in a millisecond, measured over 300 ms as
/// `limits.ps1` measures it.
pub fn cycles_per_ms() -> f64 {
    let start = Instant::now();
    let first = sys::thread_cycles();
    while start.elapsed().as_millis() < 300 {
        std::hint::spin_loop();
    }
    let ms = start.elapsed().as_secs_f64() * 1e3;
    (sys::thread_cycles() - first) as f64 / ms
}

/// A reading of the clocks.
#[derive(Clone, Copy)]
pub struct Mark {
    at: Instant,
    process: u64,
    thread: u64,
    ticks: u64,
}

/// What was spent between two marks, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spent {
    /// Elapsed time.
    pub wall: f64,
    /// Every thread's cycles, as `limits.ps1` counts them.
    pub cpu: f64,
    /// The calling thread's cycles alone.
    pub main: f64,
    /// Kernel and user time charged at timer ticks.
    pub charged: f64,
}

impl Spent {
    /// Each part divided by `n`.
    pub fn per(self, n: usize) -> Spent {
        let n = n.max(1) as f64;
        Spent {
            wall: self.wall / n,
            cpu: self.cpu / n,
            main: self.main / n,
            charged: self.charged / n,
        }
    }
}

impl Mark {
    pub fn now() -> Mark {
        Mark {
            at: Instant::now(),
            process: sys::process_cycles(),
            thread: sys::thread_cycles(),
            ticks: sys::process_ticks(),
        }
    }

    /// From this mark to now, with cycles turned into ms by `per_ms`.
    pub fn spent(&self, per_ms: f64) -> Spent {
        let now = Mark::now();
        Spent {
            wall: (now.at - self.at).as_secs_f64() * 1e3,
            cpu: now.process.saturating_sub(self.process) as f64 / per_ms,
            main: now.thread.saturating_sub(self.thread) as f64 / per_ms,
            charged: now.ticks.saturating_sub(self.ticks) as f64 / 1e4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_thread_spends_about_as_much_processor_time_as_it_waits() {
        let per_ms = cycles_per_ms();
        let mark = Mark::now();
        let start = Instant::now();
        while start.elapsed().as_millis() < 50 {
            std::hint::spin_loop();
        }
        let spent = mark.spent(per_ms);
        assert!(spent.wall >= 50.0);
        if cfg!(windows) {
            assert!(
                spent.main > 0.0 && spent.cpu >= spent.main * 0.99,
                "{spent:?}"
            );
        }
        assert_eq!(spent.per(2).wall, spent.wall / 2.0);
    }

    #[test]
    fn a_sleeping_thread_spends_little() {
        let per_ms = cycles_per_ms();
        let mark = Mark::now();
        std::thread::sleep(std::time::Duration::from_millis(40));
        let spent = mark.spent(per_ms);
        // The calling thread alone: other tests run in this process at the same time.
        assert!(spent.main < spent.wall / 2.0, "{spent:?}");
    }
}
