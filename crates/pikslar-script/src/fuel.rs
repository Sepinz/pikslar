//! Fuel: the work a run may do, and the host's cancel flag, read as fuel is burnt.

use crate::error::{ErrorKind, Fault};
use crate::limits::CANCEL_INTERVAL;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct Fuel<'c> {
    left: u64,
    used: u64,
    until_check: u64,
    cancel: &'c AtomicBool,
}

impl<'c> Fuel<'c> {
    pub fn new(allowance: u64, cancel: &'c AtomicBool) -> Fuel<'c> {
        Fuel {
            left: allowance,
            used: 0,
            until_check: 0,
            cancel,
        }
    }

    /// Burns `units` before the work they pay for is done. Fails with nothing burnt when
    /// too little is left, or when the cancel flag is found set; the flag is read at least
    /// once every [`CANCEL_INTERVAL`] units, and on the first burn.
    #[inline]
    pub fn burn(&mut self, units: u64) -> Result<(), Fault> {
        if units > self.left {
            self.left = 0;
            return Err(Fault::new(ErrorKind::Fuel, "the fuel allowance ran out"));
        }
        if units >= self.until_check {
            self.check_cancel()?;
            self.until_check = CANCEL_INTERVAL;
        } else {
            self.until_check -= units;
        }
        self.left -= units;
        self.used += units;
        Ok(())
    }

    fn check_cancel(&self) -> Result<(), Fault> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Fault::new(ErrorKind::Cancelled, "the run was cancelled"))
        } else {
            Ok(())
        }
    }

    pub fn left(&self) -> u64 {
        self.left
    }

    pub fn used(&self) -> u64 {
        self.used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burns_down_to_zero_then_stops() {
        let flag = AtomicBool::new(false);
        let mut f = Fuel::new(10, &flag);
        f.burn(4).expect("fits");
        f.burn(6).expect("fits");
        let e = f.burn(1).expect_err("empty");
        assert_eq!(e.kind, ErrorKind::Fuel);
        assert_eq!(f.used(), 10);
    }

    #[test]
    fn a_burn_too_large_burns_nothing_useful() {
        let flag = AtomicBool::new(false);
        let mut f = Fuel::new(10, &flag);
        assert!(f.burn(11).is_err());
        assert_eq!(f.used(), 0);
    }

    #[test]
    fn cancel_is_seen_within_the_interval() {
        let flag = AtomicBool::new(false);
        let mut f = Fuel::new(u64::MAX, &flag);
        f.burn(1).expect("fits");
        flag.store(true, Ordering::Relaxed);
        let mut burnt = 0;
        let e = loop {
            match f.burn(1) {
                Ok(()) => burnt += 1,
                Err(e) => break e,
            }
        };
        assert_eq!(e.kind, ErrorKind::Cancelled);
        assert!(burnt <= CANCEL_INTERVAL);
    }

    #[test]
    fn a_set_flag_stops_the_first_burn() {
        let flag = AtomicBool::new(true);
        let mut f = Fuel::new(100, &flag);
        assert_eq!(f.burn(1).expect_err("cancelled").kind, ErrorKind::Cancelled);
    }
}
