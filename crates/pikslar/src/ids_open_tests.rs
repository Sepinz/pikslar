//! Opening a file whose saved ids an older build made stale: the app says that ids were
//! renewed, and still saves over the file, since nothing that saving keeps is lost.

use crate::app::App;
use pikslar_core::{rgba, ColorMode, Document, DrawCmd, Editor, ForeignSection, FrameCmd, Tool};
use pikslar_format::piks;
use std::path::PathBuf;
use std::sync::Arc;

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-ids-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("folder");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("written");
    path
}

fn section_of(bytes: &[u8]) -> Vec<u8> {
    let at = bytes
        .windows(4)
        .position(|w| w == b"Nids")
        .expect("written");
    let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("four"));
    bytes[at + 8..at + 8 + len as usize].to_vec()
}

/// Three frames, one removed, so the file carries its frame ids; then the frames as a build
/// that does not know them leaves the file after drawing on the first frame: its own sections
/// and the carried one unchanged.
fn drawn_on_in_an_older_build() -> Vec<u8> {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    for at in 1..4 {
        e.apply(FrameCmd::Add { at, copy: false }).expect("room");
    }
    e.apply(FrameCmd::Remove { from: 1, to: 1 })
        .expect("removes");
    let carried = section_of(&piks::save(e.doc()));
    let mut older = Editor::new(piks::load(&piks::save_without_ids(e.doc())).expect("opens"));
    older.color = rgba(1, 2, 3, 255);
    older
        .apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(2, 2)],
        })
        .expect("stroke");
    let mut doc = older.doc().clone();
    doc.foreign.push(ForeignSection {
        tag: *b"Nids",
        data: Arc::from(&carried[..]),
    });
    piks::save_without_ids(&doc)
}

#[test]
fn renewed_ids_are_told_and_save_still_writes_over_the_file() {
    let path = scratch("older.piks", &drawn_on_in_an_older_build());
    let app = App::launch(Some(path.clone()), None, None);
    let (text, _) = app.message.as_ref().expect("a notice");
    assert!(text.contains("new frame ids"), "{text}");
    assert!(!text.contains("new name"), "{text}");
    assert_eq!(
        app.path.as_deref(),
        Some(path.as_path()),
        "Save writes over it"
    );
    let opened = crate::files::open(&path).expect("opens");
    assert_eq!(opened.notice().as_deref(), Some(text.as_str()));
    assert_eq!(crate::files::protected_path(&path, &opened.notes), None);
}
