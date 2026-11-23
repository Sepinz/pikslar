//! What a run may do, granted by its caller, so that a plugin can be given less than the
//! artist's own scripts.

/// One kind of access a run can be granted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// Reading the document: its size, layers, frames, pixels and swatches.
    ReadDocument,
    /// Changing the document, as one undo step.
    EditDocument,
    /// Asking the user to choose a file, then reading that one file. Only plugins granted
    /// `pick-file-to-read` get it, and only with a picker the host gives the run.
    PickFileToRead,
    /// Asking the user where to save a file, then writing that one file, once, when the run
    /// succeeds. Only plugins granted `pick-file-to-write` get it.
    PickFileToWrite,
}

/// The capabilities granted to one run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    read: bool,
    edit: bool,
    pick_read: bool,
    pick_write: bool,
}

impl Capabilities {
    /// Nothing but the language and `print`.
    pub const NONE: Capabilities = Capabilities {
        read: false,
        edit: false,
        pick_read: false,
        pick_write: false,
    };
    /// Reading only.
    pub const READ: Capabilities = Capabilities {
        read: true,
        edit: false,
        pick_read: false,
        pick_write: false,
    };
    /// Reading and editing: what the artist's own scripts get.
    pub const ALL: Capabilities = Capabilities {
        read: true,
        edit: true,
        pick_read: false,
        pick_write: false,
    };

    /// These capabilities and `cap` as well.
    pub const fn with(self, cap: Capability) -> Capabilities {
        match cap {
            Capability::ReadDocument => Capabilities { read: true, ..self },
            Capability::EditDocument => Capabilities { edit: true, ..self },
            Capability::PickFileToRead => Capabilities {
                pick_read: true,
                ..self
            },
            Capability::PickFileToWrite => Capabilities {
                pick_write: true,
                ..self
            },
        }
    }

    pub const fn has(self, cap: Capability) -> bool {
        match cap {
            Capability::ReadDocument => self.read,
            Capability::EditDocument => self.edit,
            Capability::PickFileToRead => self.pick_read,
            Capability::PickFileToWrite => self.pick_write,
        }
    }
}

/// What a plugin was granted at install, word for word, and nothing more.
impl From<pikslar_plugin::CapabilitySet> for Capabilities {
    fn from(set: pikslar_plugin::CapabilitySet) -> Capabilities {
        use pikslar_plugin::Capability as Word;
        Capabilities {
            read: set.contains(Word::ReadDocument),
            edit: set.contains(Word::EditDocument),
            pick_read: set.contains(Word::PickFileToRead),
            pick_write: set.contains(Word::PickFileToWrite),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_gets_exactly_the_words_it_was_granted() {
        use pikslar_plugin::{Capability as Word, CapabilitySet};
        assert_eq!(Capabilities::from(CapabilitySet::NONE), Capabilities::NONE);
        let read = CapabilitySet::NONE.with(Word::ReadDocument);
        assert_eq!(Capabilities::from(read), Capabilities::READ);
        let edit = Capabilities::from(CapabilitySet::NONE.with(Word::EditDocument));
        assert!(edit.has(Capability::EditDocument) && !edit.has(Capability::ReadDocument));
        let both = read.with(Word::EditDocument);
        assert_eq!(Capabilities::from(both), Capabilities::ALL);
        let pick = Capabilities::from(CapabilitySet::NONE.with(Word::PickFileToRead));
        assert!(pick.has(Capability::PickFileToRead) && !pick.has(Capability::ReadDocument));
        assert!(
            !Capabilities::ALL.has(Capability::PickFileToRead),
            "the panel's scripts"
        );
        let write = Capabilities::from(CapabilitySet::NONE.with(Word::PickFileToWrite));
        assert_eq!(write, Capabilities::NONE.with(Capability::PickFileToWrite));
        assert!(!write.has(Capability::PickFileToRead) && !write.has(Capability::EditDocument));
        assert!(
            !Capabilities::ALL.has(Capability::PickFileToWrite),
            "the panel's scripts"
        );
    }

    #[test]
    fn capabilities_are_granted_one_by_one() {
        assert!(!Capabilities::NONE.has(Capability::ReadDocument));
        assert!(!Capabilities::READ.has(Capability::EditDocument));
        let edit_only = Capabilities::NONE.with(Capability::EditDocument);
        assert!(edit_only.has(Capability::EditDocument));
        assert!(!edit_only.has(Capability::ReadDocument));
        assert_eq!(
            Capabilities::READ.with(Capability::EditDocument),
            Capabilities::ALL
        );
    }
}
