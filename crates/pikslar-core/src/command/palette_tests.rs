//! Swatch order and groups through the command core: one undo step each, nothing repainted.

use super::*;
use crate::color::rgba;
use crate::command::{Command, DrawCmd, StepCmd};
use crate::document::{ColorMode, Document};
use crate::editor::Tool;

const RED: u32 = rgba(255, 0, 0, 255);
const GREEN: u32 = rgba(0, 255, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const GREY: u32 = rgba(128, 128, 128, 255);

/// A palette document with four swatches in the order grey, blue, red, green, and a drawing:
/// one red pixel, two green, three blue.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Palette).expect("valid"));
    for color in [GREY, BLUE, RED, GREEN] {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    let dots = [(RED, 1), (GREEN, 2), (BLUE, 3)];
    for (row, (color, count)) in dots.into_iter().enumerate() {
        e.color = color;
        let path = (0..count).map(|x| (x, row as i32)).collect();
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        })
        .expect("stroke");
    }
    e
}

fn shown(e: &Editor) -> Vec<u32> {
    let t = &e.doc.colors;
    t.swatches().iter().map(|&id| t.color(id)).collect()
}

fn pixels(e: &Editor) -> Vec<u32> {
    let cel = e.doc.layers[0].cel(0);
    (0..16 * 16).map(|i| cel.get(i % 16, i / 16)).collect()
}

fn ids(e: &Editor, colors: &[u32]) -> Arc<[u32]> {
    colors
        .iter()
        .map(|&c| e.doc.colors.id_for_existing(c).expect("known"))
        .collect()
}

#[test]
fn moving_a_swatch_is_one_step_and_changes_no_id_or_pixel() {
    let mut e = editor();
    let (colors, pix, version) = (
        e.doc.colors.colors().to_vec(),
        pixels(&e),
        e.doc.colors.version(),
    );
    let depth = e.history.undo_depth();
    let outcome = e
        .apply(PaletteCmd::MoveSwatch { from: 0, to: 3 })
        .expect("moved");
    assert!(outcome.changed);
    assert_eq!(shown(&e), [BLUE, RED, GREEN, GREY]);
    assert_eq!(e.doc.colors.colors(), colors);
    assert_eq!(pixels(&e), pix);
    assert_eq!(e.doc.colors.version(), version, "nothing needs repainting");
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert!(e.undo());
    assert_eq!(shown(&e), [GREY, BLUE, RED, GREEN]);
    assert!(e.redo());
    assert_eq!(shown(&e), [BLUE, RED, GREEN, GREY]);
}

#[test]
fn a_move_nowhere_is_no_step_and_one_past_the_end_is_refused() {
    let mut e = editor();
    let (depth, revision) = (e.history.undo_depth(), e.revision);
    let same = e
        .apply(PaletteCmd::MoveSwatch { from: 2, to: 2 })
        .expect("ok");
    assert!(!same.changed);
    let past = e.apply(PaletteCmd::MoveSwatch { from: 0, to: 4 });
    assert_eq!(past, Err(LimitError::NoSuchItem));
    assert_eq!((e.history.undo_depth(), e.revision), (depth, revision));
}

#[test]
fn sorting_by_hue_and_lightness_gives_the_documented_order() {
    let mut e = editor();
    e.apply(PaletteCmd::Sort { by: SortBy::Hue })
        .expect("sorted");
    assert_eq!(shown(&e), [GREY, RED, GREEN, BLUE]);
    e.apply(PaletteCmd::Sort {
        by: SortBy::Lightness,
    })
    .expect("sorted");
    assert_eq!(shown(&e), [BLUE, RED, GREY, GREEN]);
}

#[test]
fn sorting_by_usage_puts_the_most_used_first_and_unused_ties_go_by_id() {
    let mut e = editor();
    e.apply(PaletteCmd::Sort { by: SortBy::Usage })
        .expect("sorted");
    // Blue three pixels, green two, red one, grey none.
    assert_eq!(shown(&e), [BLUE, GREEN, RED, GREY]);
}

#[test]
fn a_sort_is_one_step_and_a_second_sort_changes_nothing() {
    let mut e = editor();
    let pix = pixels(&e);
    let depth = e.history.undo_depth();
    e.apply(PaletteCmd::Sort { by: SortBy::Hue })
        .expect("sorted");
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert_eq!(pixels(&e), pix);
    let again = e.apply(PaletteCmd::Sort { by: SortBy::Hue }).expect("ok");
    assert!(!again.changed);
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert!(e.undo());
    assert_eq!(shown(&e), [GREY, BLUE, RED, GREEN]);
}

#[test]
fn a_group_names_swatches_in_order_and_is_one_step() {
    let mut e = editor();
    let depth = e.history.undo_depth();
    let group = ids(&e, &[GREEN, RED]);
    let out = e
        .apply(PaletteCmd::AddGroup {
            name: "warm".into(),
            ids: group.clone(),
        })
        .expect("added");
    assert_eq!(out.created, Some(0));
    let made = &e.doc.colors.groups()[0];
    assert_eq!((made.name.as_str(), made.kind), ("warm", GroupKind::Group));
    assert_eq!(made.ids, group.to_vec());
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert!(e.undo());
    assert!(e.doc.colors.groups().is_empty());
    assert!(e.redo());
    assert_eq!(e.doc.colors.groups().len(), 1);
}

#[test]
fn a_group_needs_swatches_and_room() {
    let mut e = editor();
    let none: Arc<[u32]> = Arc::from(Vec::new());
    let add = |ids: Arc<[u32]>| PaletteCmd::AddGroup {
        name: "g".into(),
        ids,
    };
    assert_eq!(e.apply(add(none)), Err(LimitError::NoSuchItem));
    assert_eq!(
        e.apply(add(Arc::from(vec![99]))),
        Err(LimitError::NoSuchItem)
    );
    // A colour in the table that is not a swatch is not groupable either.
    let hidden = rgba(1, 2, 3, 255);
    let id = Arc::make_mut(&mut e.doc.colors)
        .id_for(hidden)
        .expect("room");
    assert_eq!(
        e.apply(add(Arc::from(vec![id]))),
        Err(LimitError::NoSuchItem)
    );
    let one = ids(&e, &[RED]);
    for _ in 0..crate::color::MAX_GROUPS {
        e.apply(add(one.clone())).expect("room");
    }
    assert_eq!(e.apply(add(one)), Err(LimitError::TooManyGroups));
}

#[test]
fn repeated_ids_in_a_group_are_kept_once() {
    let mut e = editor();
    let red = ids(&e, &[RED])[0];
    e.apply(PaletteCmd::AddGroup {
        name: "g".into(),
        ids: Arc::from(vec![red, red]),
    })
    .expect("added");
    assert_eq!(e.doc.colors.groups()[0].ids, [red]);
}

#[test]
fn typing_a_group_name_is_one_step() {
    let mut e = editor();
    e.apply(PaletteCmd::AddGroup {
        name: String::new(),
        ids: ids(&e, &[RED]),
    })
    .expect("added");
    e.apply(StepCmd::EndDrag).expect("end");
    let depth = e.history.undo_depth();
    for name in ["s", "sk", "skin"] {
        e.apply(PaletteCmd::RenameGroup {
            group: 0,
            name: name.into(),
        })
        .expect("renamed");
    }
    e.apply(StepCmd::EndDrag).expect("end");
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert_eq!(e.doc.colors.groups()[0].name, "skin");
    assert!(e.undo());
    assert_eq!(e.doc.colors.groups()[0].name, "");
}

#[test]
fn renaming_to_the_same_name_or_a_missing_group_changes_nothing() {
    let mut e = editor();
    e.apply(PaletteCmd::AddGroup {
        name: "a".into(),
        ids: ids(&e, &[RED]),
    })
    .expect("added");
    let rename = |group, name: &str| PaletteCmd::RenameGroup {
        group,
        name: name.into(),
    };
    assert!(!e.apply(rename(0, "a")).expect("ok").changed);
    assert_eq!(e.apply(rename(3, "b")), Err(LimitError::NoSuchItem));
}

#[test]
fn removing_a_group_keeps_its_swatches_and_undoes() {
    let mut e = editor();
    e.apply(PaletteCmd::AddGroup {
        name: "a".into(),
        ids: ids(&e, &[RED, BLUE]),
    })
    .expect("added");
    let swatches = e.doc.colors.swatches().to_vec();
    e.apply(PaletteCmd::RemoveGroup { group: 0 })
        .expect("removed");
    assert!(e.doc.colors.groups().is_empty());
    assert_eq!(e.doc.colors.swatches(), swatches);
    assert_eq!(
        e.apply(PaletteCmd::RemoveGroup { group: 0 }),
        Err(LimitError::NoSuchItem)
    );
    assert!(e.undo());
    assert_eq!(e.doc.colors.groups().len(), 1);
}

#[test]
fn removing_a_swatch_takes_it_from_groups_and_undo_gives_it_back() {
    let mut e = editor();
    let both = ids(&e, &[RED, BLUE]);
    e.apply(PaletteCmd::AddGroup {
        name: "a".into(),
        ids: both.clone(),
    })
    .expect("added");
    e.apply(PaletteCmd::RemoveSwatch { id: both[0] })
        .expect("removed");
    assert_eq!(e.doc.colors.groups()[0].ids, [both[1]]);
    assert!(e.undo());
    assert_eq!(e.doc.colors.groups()[0].ids, both.to_vec());
}

#[test]
fn order_and_groups_do_not_touch_what_the_document_shows() {
    let mut e = editor();
    let before = (pixels(&e), e.doc.colors.colors().to_vec());
    let commands: Vec<Command> = vec![
        PaletteCmd::Sort { by: SortBy::Usage }.into(),
        PaletteCmd::MoveSwatch { from: 3, to: 0 }.into(),
        PaletteCmd::AddGroup {
            name: "x".into(),
            ids: ids(&e, &[RED]),
        }
        .into(),
        PaletteCmd::Sort {
            by: SortBy::Lightness,
        }
        .into(),
    ];
    for command in commands {
        e.apply(command).expect("applied");
        assert_eq!((pixels(&e), e.doc.colors.colors().to_vec()), before);
    }
}

#[test]
fn a_group_command_is_small_and_never_paints() {
    assert!(std::mem::size_of::<Command>() <= super::super::MOST_BYTES_IN_A_COMMAND);
    let all: Vec<Command> = vec![
        PaletteCmd::MoveSwatch { from: 0, to: 1 }.into(),
        PaletteCmd::Sort { by: SortBy::Hue }.into(),
        PaletteCmd::AddGroup {
            name: "a".into(),
            ids: Arc::from(vec![1]),
        }
        .into(),
        PaletteCmd::RenameGroup {
            group: 0,
            name: "b".into(),
        }
        .into(),
        PaletteCmd::RemoveGroup { group: 0 }.into(),
    ];
    assert!(all.iter().all(|c| !c.may_paint()));
}

/// A name is shown in the panel and saved in the file: control characters become spaces, as
/// palette file names do.
#[test]
fn group_names_have_control_characters_turned_to_spaces() {
    let mut e = editor();
    e.apply(PaletteCmd::AddGroup {
        name: "a\nb\u{7}".into(),
        ids: Arc::from(vec![1]),
    })
    .expect("group");
    assert_eq!(e.doc.colors.groups()[0].name, "a b ");
    e.apply(PaletteCmd::RenameGroup {
        group: 0,
        name: "c\td".into(),
    })
    .expect("renamed");
    assert_eq!(e.doc.colors.groups()[0].name, "c d");
}

/// A group the removal leaves empty goes, as one Replace import leaves; undo brings both back.
#[test]
fn removing_a_groups_last_swatch_removes_the_group_and_undo_restores_it() {
    let mut e = editor();
    for ids in [vec![1], vec![1, 2]] {
        e.apply(PaletteCmd::AddGroup {
            name: "g".into(),
            ids: Arc::from(ids),
        })
        .expect("group");
    }
    let before = e.doc.colors.clone();
    e.apply(PaletteCmd::RemoveSwatch { id: 1 })
        .expect("removed");
    let groups = e.doc.colors.groups();
    assert_eq!(groups.len(), 1, "the emptied group went");
    assert_eq!(groups[0].ids, [2]);
    assert!(e.undo());
    assert_eq!(e.doc.colors, before);
}

/// Tiles of a palette tileset show colours too, so usage counts them, once each, as Remove
/// unused does.
#[test]
fn sorting_by_usage_counts_the_pixels_of_palette_tiles() {
    use crate::command::{TileCmd, TileSource};
    let mut e = editor();
    let made = e
        .apply(TileCmd::NewTileset {
            name: "T".into(),
            mode: ColorMode::Palette,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("room");
    let set = crate::tileset::TilesetId::new(made.created.expect("an id"));
    let grey = e.doc.colors.id_for_existing(GREY).expect("a swatch");
    let image = Arc::new(crate::grid::Grid::from_cells(4, 4, &[grey; 16]).into_image());
    e.apply(TileCmd::AddTile {
        tileset: set,
        from: TileSource::Image(image),
    })
    .expect("fits");
    e.apply(PaletteCmd::Sort { by: SortBy::Usage })
        .expect("sorted");
    // Grey sixteen tile pixels, blue three, green two, red one.
    assert_eq!(shown(&e), [GREY, BLUE, GREEN, RED]);
}
