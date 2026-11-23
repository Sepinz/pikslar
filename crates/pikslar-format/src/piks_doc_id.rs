//! The `Ndoc` section: which saved document this is, so Rewind finds the history kept for it
//! (`docs/rewind.md`). Optional, and carried unchanged by builds from before it, as its last
//! letter is lowercase. A section that does not read is left out: the id is Rewind's alone, and
//! a document without one only starts a new history.

use crate::piks::Section;
use pikslar_core::{Document, DocumentId};

pub(crate) const DOC_ID: &[u8; 4] = b"Ndoc";
const VERSION: u8 = 1;
const LEN: usize = 17;

/// The section, for a document that has an id.
pub(crate) fn doc_id_section(doc: &Document) -> Option<Vec<u8>> {
    let id = doc.document_id.get()?;
    let mut bytes = Vec::with_capacity(LEN);
    bytes.push(VERSION);
    bytes.extend_from_slice(&id.to_le_bytes());
    Some(bytes)
}

/// Gives the document the id its section holds, if it holds one this version reads.
pub(crate) fn apply_doc_id(doc: &mut Document, sections: &[Section<'_>]) {
    let found = sections.iter().find(|(tag, _)| *tag == DOC_ID);
    let Some((_, data)) = found else {
        return;
    };
    if data.len() != LEN || data[0] != VERSION {
        return;
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&data[1..]);
    doc.document_id = DocumentId::new(u128::from_le_bytes(id));
}

#[cfg(test)]
mod tests {
    use crate::piks;
    use pikslar_core::{ColorMode, Document, DocumentId};

    fn doc() -> Document {
        Document::new(8, 8, ColorMode::Palette).expect("valid")
    }

    #[test]
    fn an_id_is_saved_and_read_back_and_a_document_without_one_has_none() {
        assert_eq!(
            piks::load(&piks::save(&doc()))
                .expect("opens")
                .document_id
                .get(),
            None
        );
        let mut named = doc();
        named.document_id = DocumentId::new(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210);
        let bytes = piks::save(&named);
        let back = piks::load(&bytes).expect("opens");
        assert_eq!(back.document_id.get(), named.document_id.get());
        assert!(back.foreign.is_empty(), "known, so not carried as foreign");
        assert_eq!(piks::save(&back), bytes, "saves the same again");
    }

    #[test]
    fn the_id_is_not_part_of_what_the_document_is() {
        let mut named = doc();
        named.document_id = DocumentId::new(7);
        assert_eq!(named, doc());
        let plain = piks::save_without_ids(&named);
        assert_eq!(plain, piks::save_without_ids(&doc()));
    }

    #[test]
    fn a_section_that_does_not_read_is_left_out_and_the_project_opens() {
        let mut named = doc();
        named.document_id = DocumentId::new(7);
        for bad in [&[1u8][..], &[2u8; 17][..], &[1u8; 18][..]] {
            let bytes = with_section(&piks::save(&doc()), bad);
            let (back, notes) = piks::load_noted(&bytes).expect("opens");
            assert_eq!(back.document_id.get(), None);
            assert!(notes.can_save_over());
        }
    }

    /// The project with one more section holding `data` under the id's tag.
    fn with_section(project: &[u8], data: &[u8]) -> Vec<u8> {
        let mut doc = piks::load(project).expect("opens");
        doc.foreign.push(pikslar_core::ForeignSection {
            tag: *super::DOC_ID,
            data: std::sync::Arc::from(data),
        });
        piks::save(&doc)
    }
}
