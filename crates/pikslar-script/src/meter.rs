//! Live memory accounting for one run.
//!
//! Every heap object a script can create is charged here before it is allocated, and refunds
//! its charge when it is dropped. The meter never lets the live total pass the cap.

use crate::error::Fault;
use crate::limits::alloc_charge;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Debug)]
pub(crate) struct Meter {
    used: Cell<usize>,
    peak: Cell<usize>,
    cap: usize,
}

impl Meter {
    pub fn new(cap: usize) -> Rc<Meter> {
        Rc::new(Meter {
            used: Cell::new(0),
            peak: Cell::new(0),
            cap,
        })
    }

    /// Charges `bytes`, or refuses without charging anything when the cap would be passed.
    pub fn charge(&self, bytes: usize) -> Result<(), Fault> {
        let used = self
            .used
            .get()
            .checked_add(bytes)
            .filter(|&u| u <= self.cap)
            .ok_or_else(Fault::memory)?;
        self.used.set(used);
        if used > self.peak.get() {
            self.peak.set(used);
        }
        Ok(())
    }

    /// Charges what an allocation of `bytes` costs, by [`alloc_charge`], and returns it.
    pub fn charge_alloc(&self, bytes: usize) -> Result<usize, Fault> {
        let cost = alloc_charge(bytes).ok_or_else(Fault::memory)?;
        self.charge(cost)?;
        Ok(cost)
    }

    /// True when an allocation of `bytes` would be within the cap now. Used to report a
    /// too-large request as memory before burning the fuel for filling it.
    pub fn fits(&self, bytes: usize) -> bool {
        alloc_charge(bytes)
            .and_then(|c| self.used.get().checked_add(c))
            .is_some_and(|u| u <= self.cap)
    }

    pub fn refund(&self, bytes: usize) {
        self.used.set(self.used.get().saturating_sub(bytes));
    }

    #[cfg(test)]
    pub fn used(&self) -> usize {
        self.used.get()
    }

    pub fn peak(&self) -> usize {
        self.peak.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    #[test]
    fn charges_up_to_the_cap_and_no_further() {
        let m = Meter::new(100);
        assert!(m.charge(60).is_ok());
        assert!(m.charge(40).is_ok());
        let e = m.charge(1).expect_err("over the cap");
        assert_eq!(e.kind, ErrorKind::Memory);
        assert_eq!(m.used(), 100);
    }

    #[test]
    fn refunds_lower_use_but_keep_the_peak() {
        let m = Meter::new(100);
        m.charge(70).expect("fits");
        m.refund(50);
        assert_eq!(m.used(), 20);
        assert_eq!(m.peak(), 70);
        m.charge(80).expect("fits after the refund");
    }

    #[test]
    fn overflowing_charges_are_refused() {
        let m = Meter::new(usize::MAX);
        m.charge(10).expect("fits");
        assert!(m.charge(usize::MAX).is_err());
        assert!(m.charge_alloc(usize::MAX).is_err());
        assert_eq!(m.used(), 10);
    }
}
