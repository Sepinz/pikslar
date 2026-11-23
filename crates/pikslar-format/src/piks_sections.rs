//! Sections of a project that this version does not understand.
//!
//! The last letter of a tag says what an older program may do. Lowercase means the section
//! stands alone and can be carried along unchanged. Anything else may depend on data the older
//! program is about to rewrite, so it is left out and the person is told.

use crate::piks::Section;
use pikslar_core::{Document, ForeignSection};
use std::sync::Arc;

/// Sections a project may have.
pub(crate) const MAX_SECTIONS: usize = 64;
/// Leaves room for every section this version writes itself, which are the known ones.
pub(crate) const MAX_FOREIGN: usize = MAX_SECTIONS - KNOWN.len();
/// More than this is not carried along, and saving over the original then asks for a new name.
const MAX_FOREIGN_BYTES: usize = 16 << 20;
const KNOWN: [&[u8; 4]; 10] = [
    crate::piks_tilesets::TILESETS,
    crate::piks_terrain::TERRAINS,
    crate::piks_palette::GROUPS,
    crate::piks_slices::SLICES,
    crate::piks_anim::TAGS,
    crate::piks_anim::RATE,
    crate::piks_anim::LINKS,
    crate::piks::FLOATING,
    crate::piks_ids::IDS,
    crate::piks_doc_id::DOC_ID,
];

/// What opening a project had to leave behind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadNotes {
    /// Sections that cannot be carried along, so saving would lose them.
    pub not_kept: Vec<[u8; 4]>,
    /// Parts of the project that made no sense and were left out.
    pub damaged: Vec<&'static str>,
    /// Records held fields or values from a newer Pikslar, which this version left out.
    pub newer_fields: bool,
    /// Frame times that were not whole ticks of this frame rate, rounded to the nearest.
    /// Nothing was damaged, but saving would rewrite them.
    pub rounded_to_fps: Option<u8>,
    /// Layer names longer than a name may be, shortened.
    pub names_shortened: bool,
    /// Palette groups named colours that are not swatches, or too many groups, or names too
    /// long, which were dropped or cut. Not damage, but saving would rewrite them.
    pub palette_trimmed: bool,
    /// Terrain slots named tiles that are gone, or terrain sets a tileset that is gone, which
    /// were emptied or left out, or terrain names were cut. Not damage, but saving would rewrite
    /// them.
    pub terrain_trimmed: bool,
    /// Some frames no longer matched a saved frame id, because a build that does not know them
    /// drew on, retimed or added them, so they were given new ones; the others kept theirs.
    /// Nothing is lost that saving would keep, so this does not stop a save over the file, and
    /// `message` tells it without asking for a new name.
    pub frame_ids_renumbered: bool,
    /// Layers a build that does not know the saved ids added or changed may have taken a
    /// removed layer's id: unlisted at or below the saved floor, or listed with a check that no
    /// longer matches. They were given new ones. Like `frame_ids_renumbered`, this does not
    /// stop a save over the file.
    pub layer_ids_renumbered: bool,
}

impl LoadNotes {
    /// False if replacing the file would lose something this version could not read.
    pub fn can_save_over(&self) -> bool {
        self.not_kept.is_empty()
            && self.damaged.is_empty()
            && !self.newer_fields
            && self.rounded_to_fps.is_none()
            && !self.names_shortened
            && !self.palette_trimmed
            && !self.terrain_trimmed
    }

    /// What happened, one clause each, for a line of its own such as a command line warning.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(fps) = self.rounded_to_fps {
            lines.push(format!(
                "frame times were rounded to whole ticks of {fps} fps"
            ));
        }
        if self.names_shortened {
            lines.push("some layer names were too long and were shortened".to_owned());
        }
        if self.palette_trimmed {
            lines.push("some palette group entries named colours that are gone".to_owned());
        }
        if self.terrain_trimmed {
            lines.push("some terrain slots named tiles that are gone".to_owned());
        }
        if self.frame_ids_renumbered {
            lines.push(
                "an older Pikslar changed or added frames, so they were given new frame ids"
                    .to_owned(),
            );
        }
        if self.layer_ids_renumbered {
            lines.push(
                "an older Pikslar added or changed layers, so they were given new layer ids"
                    .to_owned(),
            );
        }
        if !self.damaged.is_empty() {
            lines.push(format!(
                "these damaged parts were left out, the art is fine: {}",
                self.damaged.join(", ")
            ));
        }
        if !self.not_kept.is_empty() || self.newer_fields {
            lines.push("this project has data from a newer Pikslar, which is not used".to_owned());
        }
        lines
    }

    /// A sentence for the person, if there is anything to tell them. Renewed ids are told but
    /// do not stop a save over the file, so they come without the new-name ending unless
    /// something else needs it.
    pub fn message(&self) -> Option<String> {
        let mut text = String::new();
        if self.frame_ids_renumbered {
            text += "Some frames an older Pikslar changed or added were given new frame ids. ";
        }
        if self.layer_ids_renumbered {
            text += "Some layers an older Pikslar added or changed were given new layer ids. ";
        }
        let protect = self.protecting();
        match (text.is_empty(), protect.is_empty()) {
            (true, true) => None,
            (false, true) => Some(text.trim_end().to_owned()),
            (_, false) => Some(
                text + &protect
                    + "Save will ask for a new name, so the original file stays as it was",
            ),
        }
    }

    /// The sentences for what makes Save ask for a new name.
    fn protecting(&self) -> String {
        let mut text = String::new();
        if let Some(fps) = self.rounded_to_fps {
            text += &format!("Frame times were rounded to whole ticks of {fps} fps. ");
        }
        if self.names_shortened {
            text += "Some layer names were too long and were shortened. ";
        }
        if self.palette_trimmed {
            text += "Some palette group entries named colours that are gone and were left out. ";
        }
        if self.terrain_trimmed {
            text += "Some terrain slots named tiles that are gone and were emptied. ";
        }
        if !self.damaged.is_empty() {
            text += &format!(
                "The art is fine, but these damaged parts were left out: {}. ",
                self.damaged.join(", ")
            );
        }
        if !self.not_kept.is_empty() || self.newer_fields {
            text += "This project has data from a newer Pikslar. ";
        }
        text
    }
}

pub(crate) fn is_safe_to_copy(tag: &[u8; 4]) -> bool {
    tag[3].is_ascii_lowercase()
}

/// Sorts the sections nothing else claimed into those carried along and those reported.
pub(crate) fn keep_foreign(doc: &mut Document, sections: &[Section<'_>], notes: &mut LoadNotes) {
    for (tag, data) in sections {
        let Ok(tag) = <[u8; 4]>::try_from(*tag) else {
            continue;
        };
        if KNOWN.contains(&&tag) {
            continue;
        }
        let held: usize = doc.foreign.iter().map(|f| f.data.len()).sum();
        let has_room = doc.foreign.len() < MAX_FOREIGN && held + data.len() <= MAX_FOREIGN_BYTES;
        if is_safe_to_copy(&tag) && has_room {
            doc.foreign.push(ForeignSection {
                tag,
                data: Arc::from(*data),
            });
        } else {
            notes.not_kept.push(tag);
        }
    }
}
