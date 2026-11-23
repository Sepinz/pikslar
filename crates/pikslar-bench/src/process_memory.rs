//! This process's private bytes, as Task Manager's commit size counts them. Elsewhere than
//! Windows, it reads 0.

#[cfg(windows)]
mod sys {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }

    fn read() -> Counters {
        let mut c = Counters {
            cb: std::mem::size_of::<Counters>() as u32,
            ..Counters::default()
        };
        // SAFETY: `c` is a correctly sized PROCESS_MEMORY_COUNTERS_EX that lives for the call,
        // and the pseudo handle needs no closing.
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        c
    }

    pub fn private() -> usize {
        read().private
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn private() -> usize {
        0
    }
}

pub use sys::private;
