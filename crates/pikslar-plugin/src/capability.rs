//! What a plugin may ask for. The list is small and closed: a word Pikslar does not know
//! refuses the install, so a plugin cannot ask for something the user was never shown.
//!
//! There is deliberately no capability for a path, a folder, the network, a process, the
//! clipboard or the environment. Files reach a plugin only through a picker the user answers
//! (see [`crate::files`]).

/// One thing a plugin may do, granted for every run of that plugin once the user approves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    /// Read the open document: size, frames, layers, pixels, swatches.
    ReadDocument,
    /// Change the open document. Each run is one undo step.
    EditDocument,
    /// Ask the user to choose a file, and read that one file's contents.
    PickFileToRead,
    /// Ask the user to choose where to save, and create or replace that one file.
    PickFileToWrite,
}

impl Capability {
    /// Every capability, in the order an install review lists them.
    pub const ALL: [Capability; 4] = [
        Capability::ReadDocument,
        Capability::EditDocument,
        Capability::PickFileToRead,
        Capability::PickFileToWrite,
    ];

    /// The word a manifest uses. Stable: it is part of the plugin format.
    pub fn word(self) -> &'static str {
        match self {
            Capability::ReadDocument => "read-document",
            Capability::EditDocument => "edit-document",
            Capability::PickFileToRead => "pick-file-to-read",
            Capability::PickFileToWrite => "pick-file-to-write",
        }
    }

    /// The capability a manifest word names, if Pikslar knows it. Exact and case-sensitive.
    #[inline(never)]
    pub fn from_word(word: &str) -> Option<Capability> {
        Capability::ALL.into_iter().find(|c| c.word() == word)
    }

    /// What the capability lets the plugin do, in plain words for the install review.
    pub fn plain_words(self) -> &'static str {
        match self {
            Capability::ReadDocument => {
                "See the open document: its frames, layers, pixels and palette."
            }
            Capability::EditDocument => {
                "Change the open document. Each run can be undone in one step."
            }
            Capability::PickFileToRead => {
                "Ask you to choose a file, then read that one file. It never sees where the file is."
            }
            Capability::PickFileToWrite => {
                "Ask you to choose where to save, then create or replace the one file you pick, if it is plain data or a picture (like .txt or .png). It never sees where."
            }
        }
    }

    fn bit(self) -> u8 {
        match self {
            Capability::ReadDocument => 1,
            Capability::EditDocument => 2,
            Capability::PickFileToRead => 4,
            Capability::PickFileToWrite => 8,
        }
    }
}

/// A set of capabilities: what a plugin asked for, or what a run was granted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CapabilitySet(u8);

impl CapabilitySet {
    pub const NONE: CapabilitySet = CapabilitySet(0);

    pub fn contains(self, c: Capability) -> bool {
        self.0 & c.bit() != 0
    }

    /// A new set with `c` added.
    pub fn with(self, c: Capability) -> CapabilitySet {
        CapabilitySet(self.0 | c.bit())
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The members, in [`Capability::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = Capability> {
        Capability::ALL
            .into_iter()
            .filter(move |&c| self.contains(c))
    }

    /// Those in `self` that `before` lacks: what an upgrade newly asks for.
    pub fn added_since(self, before: CapabilitySet) -> CapabilitySet {
        CapabilitySet(self.0 & !before.0)
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = Capability>>(items: I) -> Self {
        items
            .into_iter()
            .fold(CapabilitySet::NONE, CapabilitySet::with)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_round_trip_and_unknown_words_are_none() {
        for c in Capability::ALL {
            assert_eq!(Capability::from_word(c.word()), Some(c));
            assert!(c.plain_words().ends_with('.'));
        }
        for word in [
            "",
            "Read-Document",
            "read-document ",
            "files",
            "network",
            "path",
        ] {
            assert_eq!(Capability::from_word(word), None, "{word:?}");
        }
    }

    #[test]
    fn sets_hold_and_compare() {
        let read = CapabilitySet::NONE.with(Capability::ReadDocument);
        let both = read.with(Capability::EditDocument);
        assert!(both.contains(Capability::EditDocument));
        assert!(!read.contains(Capability::EditDocument));
        assert_eq!(
            both.added_since(read).iter().collect::<Vec<_>>(),
            [Capability::EditDocument]
        );
        assert!(read.added_since(both).is_empty());
        let all: CapabilitySet = Capability::ALL.into_iter().collect();
        assert_eq!(all.iter().count(), 4);
    }
}
