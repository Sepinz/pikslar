//! What happens to sections of a project that this version does not understand.

use pikslar_core::{rgba, ColorMode, Document, Editor, Mask, SelectOp, Tool};
use pikslar_core::{DrawCmd, FloatingCmd, SelectionCmd};
use pikslar_format::piks;
use pikslar_safety::craft::{resealed, tags_section, with_sections};
use pikslar_safety::picture::frame_hashes;

fn drawing() -> Document {
    let mut editor = Editor::new(Document::new(24, 16, ColorMode::Palette).expect("valid"));
    editor.color = rgba(200, 30, 10, 255);
    editor
        .apply(DrawCmd::Begin {
            tool: Tool::Line,
            x: 0,
            y: 0,
        })
        .expect("begin");
    editor.apply(DrawCmd::To { x: 23, y: 15 }).expect("drag");
    editor.apply(DrawCmd::Commit).expect("commit");
    editor.doc().clone()
}

fn plain() -> Vec<u8> {
    piks::save(&drawing())
}

fn contains(bytes: &[u8], tag: &[u8; 4]) -> bool {
    bytes.windows(4).any(|w| w == tag)
}

#[test]
fn a_section_marked_safe_survives_saving_unchanged() {
    let file = with_sections(&plain(), &[(b"NEXt", &[1, 2, 3, 250])]);
    let (doc, notes) = piks::load_noted(&file).expect("opens");
    assert_eq!(notes.message(), None);
    assert!(notes.can_save_over());
    assert_eq!(piks::save(&doc), file, "the file comes back byte for byte");
}

#[test]
fn a_section_not_marked_safe_is_reported_and_protects_the_file() {
    let file = with_sections(&plain(), &[(b"NEXT", &[9, 9])]);
    let (doc, notes) = piks::load_noted(&file).expect("opens");
    assert_eq!(notes.not_kept, vec![*b"NEXT"]);
    assert!(!notes.can_save_over());
    assert!(notes.message().expect("a message").contains("newer"));
    assert!(!contains(&piks::save(&doc), b"NEXT"));
}

#[test]
fn recovery_copies_carry_sections_marked_safe() {
    let file = with_sections(&plain(), &[(b"NEXt", &[7])]);
    let doc = piks::load(&file).expect("opens");
    let copy = piks::save_recovery(&doc, None);
    assert_eq!(
        piks::load_recovery(&copy).expect("opens").doc.foreign,
        doc.foreign
    );
}

#[test]
fn floating_pixels_never_reach_a_saved_project() {
    let mut editor = Editor::new(drawing());
    editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(1, 1, 8, 8)),
            op: SelectOp::Replace,
        })
        .expect("select");
    editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    editor
        .apply(FloatingCmd::Move {
            dx: 2,
            dy: 1,
            join: true,
        })
        .expect("move");
    let snapshot = editor.floating_snapshot().expect("floating");
    let copy = piks::save_recovery(editor.doc(), Some(&snapshot));
    assert!(contains(&copy, b"FLOT"));
    let recovered = piks::load_recovery(&copy).expect("opens");
    assert!(!contains(&piks::save(&recovered.doc), b"FLOT"));
    let (plain_open, notes) = piks::load_noted(&copy).expect("opens");
    assert!(notes.can_save_over(), "recovery data is not foreign data");
    assert!(!contains(&piks::save(&plain_open), b"FLOT"));
}

#[test]
fn too_many_safe_sections_still_leave_a_file_that_opens() {
    let body = [1u8];
    let many: Vec<(&[u8; 4], &[u8])> = (0..64).map(|_| (b"NEXt", &body[..])).collect();
    let file = with_sections(&plain(), &many);
    let (mut doc, notes) = piks::load_noted(&file).expect("opens");
    assert!(!notes.not_kept.is_empty(), "the ones left out are reported");
    doc.add_tag("walk", 0, 0).expect("valid");
    let again = piks::load(&piks::save(&doc)).expect("the saved file opens");
    assert_eq!(again.foreign, doc.foreign);
    assert_eq!(again.tags, doc.tags);
}

#[test]
fn damaged_tags_are_reported_and_cost_no_art() {
    let bad = tags_section(&[(9, 0, 0, b"x")]);
    let file = with_sections(&plain(), &[(b"TAGS", &bad)]);
    let (doc, notes) = piks::load_noted(&file).expect("the art opens");
    assert_eq!(
        frame_hashes(&mut Editor::new(doc.clone())),
        frame_hashes(&mut Editor::new(drawing()))
    );
    assert!(doc.tags.is_empty());
    assert_eq!(notes.damaged, vec!["tags"]);
    assert!(
        !notes.can_save_over(),
        "the damaged part may still be repairable by hand, so the original is kept"
    );
    assert!(notes.message().expect("a message").contains("damaged"));
}

#[test]
fn damaged_holds_are_reported_in_version_two_and_ignored_after() {
    let entries = pikslar_safety::corpus::entries().expect("corpus");
    let v2 = entries
        .iter()
        .find(|e| e.name == "v2-palette.piks")
        .expect("in the corpus");
    let file = with_sections(&v2.bytes().expect("frozen"), &[(b"LINK", &[1])]);
    let (_, notes) = piks::load_noted(&file).expect("the art opens");
    assert_eq!(notes.damaged, vec!["holds"]);
    let file = with_sections(&plain(), &[(b"LINK", &[1])]);
    let (_, notes) = piks::load_noted(&file).expect("the art opens");
    assert!(notes.can_save_over(), "holds are in the cel records now");
}

#[test]
fn good_tags_produce_no_message() {
    let tags = tags_section(&[(0, 0, 0, b"walk")]);
    let file = with_sections(&plain(), &[(b"TAGS", &tags)]);
    let (doc, notes) = piks::load_noted(&file).expect("opens");
    assert_eq!(doc.tags.len(), 1);
    assert_eq!(notes.message(), None);
}

#[test]
fn impossible_frame_timings_are_brought_into_range() {
    for (stored, expected) in [(0u32, 10u32), (u32::MAX, 60_000), (250, 250)] {
        let mut file = plain();
        // Required features, width, height, then the two short lists come before the durations.
        let colors = u32::from_le_bytes([file[20], file[21], file[22], file[23]]) as usize;
        let at = 24 + colors * 4;
        let swatches =
            u32::from_le_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
        let first_duration = at + 4 + swatches * 4 + 4;
        file[first_duration..first_duration + 4].copy_from_slice(&stored.to_le_bytes());
        let doc = piks::load(&resealed(file)).expect("opens");
        assert_eq!(doc.frames[0].duration_ms, expected);
    }
}
