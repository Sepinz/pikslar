//! GIF's LZW compression, as the GIF89a specification gives it: variable-width codes of up to
//! 12 bits, packed from the lowest bit, a clear code first and whenever the table is full, and
//! an end code last. What is held is the table of at most 4096 codes and the bytes coded so far
//! that the caller has not yet taken.

/// The most codes a table holds: 12 bits.
const CODES: u16 = 4096;
/// Places in the table of strings, twice the codes it holds so a lookup is rarely more than one.
const SLOTS: usize = 8192;
const EMPTY: u32 = u32::MAX;

/// An encoder of one image's indices.
pub(crate) struct Lzw {
    /// Bits in an index as the image declares them, 2 to 8.
    min: u8,
    /// Bits in the next code written.
    width: u8,
    /// The code the next new string gets.
    next: u16,
    /// The code of the string read so far but not yet written, if any.
    prefix: Option<u16>,
    /// Each string as its prefix's code and last index (20 bits), above its own code (12).
    slots: Box<[u32]>,
    /// Bits written but not yet a whole byte, and how many.
    bits: (u32, u8),
    /// Whole bytes coded and not yet taken.
    pub(crate) out: Vec<u8>,
}

impl Lzw {
    /// An encoder for indices of `min` bits, 2 to 8, which has written its first clear code.
    pub(crate) fn new(min: u8) -> Self {
        let mut lzw = Lzw {
            min,
            width: min + 1,
            next: 0,
            prefix: None,
            slots: vec![EMPTY; SLOTS].into_boxed_slice(),
            bits: (0, 0),
            out: Vec::new(),
        };
        lzw.clear();
        lzw
    }

    fn clear_code(&self) -> u16 {
        1 << self.min
    }

    /// Writes the clear code and empties the table.
    fn clear(&mut self) {
        self.emit(self.clear_code());
        self.slots.fill(EMPTY);
        self.width = self.min + 1;
        self.next = self.clear_code() + 2;
    }

    fn emit(&mut self, code: u16) {
        let (mut held, mut count) = self.bits;
        held |= u32::from(code) << count;
        count += self.width;
        while count >= 8 {
            self.out.push(held as u8);
            held >>= 8;
            count -= 8;
        }
        self.bits = (held, count);
    }

    fn slot_of(key: u32) -> usize {
        (key.wrapping_mul(2_654_435_761) >> 19) as usize & (SLOTS - 1)
    }

    /// The code of the string `key`, a prefix's code and one index, if the table has it.
    fn find(&self, key: u32) -> Option<u16> {
        let mut at = Self::slot_of(key);
        loop {
            let slot = self.slots[at];
            if slot == EMPTY {
                return None;
            }
            if slot >> 12 == key {
                return Some((slot & 0xfff) as u16);
            }
            at = (at + 1) & (SLOTS - 1);
        }
    }

    fn insert(&mut self, key: u32, code: u16) {
        let mut at = Self::slot_of(key);
        while self.slots[at] != EMPTY {
            at = (at + 1) & (SLOTS - 1);
        }
        self.slots[at] = key << 12 | u32::from(code);
    }

    /// Codes `indices`, each below `1 << min`.
    pub(crate) fn push(&mut self, indices: &[u8]) {
        for &index in indices {
            let Some(prefix) = self.prefix else {
                self.prefix = Some(u16::from(index));
                continue;
            };
            let key = u32::from(prefix) << 8 | u32::from(index);
            if let Some(code) = self.find(key) {
                self.prefix = Some(code);
                continue;
            }
            self.emit(prefix);
            if self.next < CODES {
                if self.next == 1 << self.width {
                    self.width += 1;
                }
                self.insert(key, self.next);
                self.next += 1;
            } else {
                self.clear();
            }
            self.prefix = Some(u16::from(index));
        }
    }

    /// Writes what is left, the end code and the last bits, padded to a byte. A reader adds a
    /// string on the last code as on any other, so if that fills the width it reads the end
    /// code one bit wider, and so it is written.
    pub(crate) fn finish(&mut self) {
        if let Some(prefix) = self.prefix.take() {
            self.emit(prefix);
            if self.next < CODES && self.next == 1 << self.width {
                self.width += 1;
            }
        }
        self.emit(self.clear_code() + 1);
        if self.bits.1 > 0 {
            self.out.push(self.bits.0 as u8);
            self.bits = (0, 0);
        }
    }
}
