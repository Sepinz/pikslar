//! TMJ from the export panel: offered only with a tilemap layer, it names what it leaves out,
//! and writes the map with an atlas for each tileset, changing nothing in the project.

use super::*;
use crate::export_dialog::ExportChoice;
use crate::test_app::{app, draw_whole, key, map_app, PLAIN};
use pikslar_platform::Key;
use std::fs;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-app-tmj-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn has(a: &App, id: u32) -> bool {
    a.ui.widgets().iter().any(|&(_, seen)| seen == id)
}

/// The TMJ button's id: after the five kinds.
const ID_MAP: u32 = 500 + 5;

#[test]
fn tmj_is_offered_only_while_the_document_has_a_tilemap_layer() {
    let mut a = app(1);
    a.open_export_dialog(false);
    draw_whole(&mut a);
    assert!(!a.has_tilemaps() && !has(&a, ID_MAP));
    key(&mut a, Key::Char(b'6'), PLAIN);
    assert_eq!(
        a.export_dialog.map(|c| c.map),
        Some(false),
        "6 does nothing"
    );
    let (mut a, _) = map_app();
    a.open_export_dialog(false);
    draw_whole(&mut a);
    assert!(has(&a, ID_MAP));
    key(&mut a, Key::Char(b'6'), PLAIN);
    assert_eq!(a.export_dialog.map(|c| c.map), Some(true));
    key(&mut a, Key::Char(b'2'), PLAIN);
    let back = a.export_dialog.expect("open");
    assert!(!back.map, "a kind chosen again ends the map");
}

#[test]
fn the_panel_names_the_layers_a_map_leaves_out() {
    let (mut a, _) = map_app();
    assert_eq!(a.map_left_out(), vec!["Layer 1".to_string()]);
    a.editor.layer = 0;
    a.layer_action(0);
    let names = a.map_left_out();
    assert_eq!(names.len(), 2, "{names:?}");
    a.export_dialog = Some(ExportChoice::default().with_map());
    draw_whole(&mut a);
    assert!(!has(&a, 500 + 20), "no layer choice for a map");
}

#[test]
fn a_map_is_written_with_its_atlas_and_says_what_it_left_out() {
    let (mut a, set) = map_app();
    let tile = a.editor.doc().tileset(set).expect("set").tiles()[0].id;
    a.editor.set_current_tile(set, tile).expect("chosen");
    a.editor
        .apply(pikslar_core::TileCmd::Stroke {
            tool: pikslar_core::TileTool::Place,
            path: vec![(1, 1)],
        })
        .expect("placed");
    let before = a.editor.doc().clone();
    let depth = a.editor.history().undo_depth();
    let dir = folder("write");
    let path = dir.join("level.tmj");
    let done = a
        .export_map_as(path.clone(), &mut |_| true)
        .expect("written")
        .expect("not declined");
    assert!(done.starts_with("Exported"), "{done}");
    assert!(done.contains("1 paint layer(s)"), "{done}");
    let text = fs::read_to_string(&path).expect("the map");
    assert!(text.contains("\"tilelayer\""), "{text}");
    assert!(dir.join("level-tileset1.png").exists(), "the atlas");
    assert_eq!(a.editor.doc(), &before, "the project is as it was");
    assert_eq!(a.editor.history().undo_depth(), depth);
    // A second export asks before replacing the atlas; a no writes nothing.
    let declined = a.export_map_as(path, &mut |_| false).expect("no error");
    assert_eq!(declined, None);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_document_without_maps_is_refused_in_words() {
    let mut a = app(1);
    let dir = folder("none");
    let refused = a.export_map_as(dir.join("x.tmj"), &mut |_| true);
    assert!(refused.is_err_and(|e| e.len() > 10));
    let _ = fs::remove_dir_all(&dir);
}
