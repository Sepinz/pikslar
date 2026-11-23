//! Names that come in from an Aseprite file, made safe to show, and the notes about many things
//! of one kind that are told as one warning. See [`crate::ase_read`].

use std::borrow::Cow;

/// What replaces a character a name may not hold.
const REPLACEMENT: char = '\u{FFFD}';

/// A name as it is kept, and what had to be done to it.
pub(crate) struct Name {
    pub text: String,
    /// It was longer than the most the thing may be called, and was cut.
    pub cut: bool,
    /// It was not valid UTF-8, and bad bytes were replaced.
    pub lossy: bool,
    /// It held control or text-direction characters, which were replaced.
    pub cleaned: bool,
}

/// Control characters (C0, DEL and C1), the bidirectional overrides and isolates, and the
/// zero-width and direction marks, which can make a name show as something else or leave
/// nothing to see. The rule is the one every name of the project follows.
fn unsafe_char(c: char) -> bool {
    pikslar_core::names::is_hidden(c)
}

/// Makes a name from the text read, cut to `max` characters.
pub(crate) fn tidy(raw: Cow<'_, str>, max: usize) -> Name {
    let lossy = matches!(&raw, Cow::Owned(_));
    let cleaned = raw.chars().any(unsafe_char);
    let text: String = raw
        .chars()
        .map(|c| if unsafe_char(c) { REPLACEMENT } else { c })
        .take(max)
        .collect();
    let cut = raw.chars().count() > max;
    Name {
        text,
        cut,
        lossy,
        cleaned,
    }
}

/// The kinds of thing that are counted and told in one warning, however many there are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tally {
    /// Slices that do not fit a document, or are past the most, left out.
    SliceDropped,
    /// Slice names cut to the length a slice name may have.
    SliceName,
    /// Slices whose nine-slice centre or pivot does not fit, left off.
    SliceParts,
    /// Slices that change from frame to frame, kept as in one key.
    SliceKeys,
    /// Slices whose first usable key comes after the first frame.
    SliceLate,
    /// Slice chunks that end before their keys do.
    SliceCut,
    /// Tag names cut to the length a tag name may have.
    TagName,
    /// Names that were not valid text.
    LossyName,
    /// Names that held control or text-direction characters.
    CleanedName,
}

impl Tally {
    /// The warning for `count` of this kind, the first being `first` (with `frame`, counted from
    /// 1, for the kinds that name one).
    pub(crate) fn say(self, count: usize, first: &str, frame: usize) -> String {
        let one = count == 1;
        match self {
            Tally::SliceDropped if one => format!("Slice \"{first}\" was left out"),
            Tally::SliceDropped => {
                format!("{count} slices were left out, the first \"{first}\"")
            }
            Tally::SliceName if one => format!("Slice \"{first}\" has a shortened name"),
            Tally::SliceName => {
                format!("{count} slice names were shortened, the first \"{first}\"")
            }
            Tally::SliceParts if one => format!(
                "Slice \"{first}\": its centre or pivot did not fit and was left off"
            ),
            Tally::SliceParts => format!(
                "{count} slices had a centre or pivot that did not fit and was left off, the first \"{first}\""
            ),
            Tally::SliceKeys if one => format!(
                "Slice \"{first}\" changes from frame to frame; it is kept as it is in frame {frame}"
            ),
            Tally::SliceKeys => format!(
                "{count} slices change from frame to frame and are kept as they are in one frame each, the first, \"{first}\", in frame {frame}"
            ),
            Tally::SliceLate if one => format!(
                "Slice \"{first}\" starts later in the file, at frame {frame}, and here covers every frame"
            ),
            Tally::SliceLate => format!(
                "{count} slices start later in the file and here cover every frame, the first, \"{first}\", at frame {frame}"
            ),
            Tally::SliceCut if one => format!(
                "Slice \"{first}\" is cut short in the file; what could be read was kept"
            ),
            Tally::SliceCut => format!(
                "{count} slices are cut short in the file and what could be read was kept, the first \"{first}\""
            ),
            Tally::TagName if one => format!("Tag \"{first}\" has a shortened name"),
            Tally::TagName => {
                format!("{count} tag names were shortened, the first \"{first}\"")
            }
            Tally::LossyName if one => {
                format!("The name \"{first}\" is not valid text; the bad parts were replaced")
            }
            Tally::LossyName => format!(
                "{count} names are not valid text and had the bad parts replaced, the first \"{first}\""
            ),
            Tally::CleanedName if one => format!(
                "The name \"{first}\" held control or text-direction characters, which were replaced"
            ),
            Tally::CleanedName => format!(
                "{count} names held control or text-direction characters, which were replaced, the first \"{first}\""
            ),
        }
    }
}

/// How many of one kind there were, and the first of them.
pub(crate) struct Counted {
    pub kind: Tally,
    pub count: usize,
    pub first: String,
    pub frame: usize,
}
