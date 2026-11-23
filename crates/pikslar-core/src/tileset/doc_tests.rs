use super::*;
use crate::grid::Grid;

fn doc() -> Document {
    Document::new(16, 16, ColorMode::Rgba).expect("size")
}

fn drawn(w: usize, h: usize, value: u32) -> Arc<Image> {
    Arc::new(Grid::from_cells(w, h, &vec![value; w * h]).into_image())
}

#[test]
fn a_new_document_has_no_tilesets() {
    assert!(doc().tilesets.is_empty());
}

#[test]
fn a_tileset_is_added_named_and_found_by_its_id() {
    let mut d = doc();
    let id = d
        .add_tileset("  Floor ", ColorMode::Rgba, (8, 8))
        .expect("room");
    assert_eq!(d.tileset(id).expect("there").name(), "Floor");
    let unnamed = d.add_tileset("\n", ColorMode::Rgba, (8, 8)).expect("room");
    assert_eq!(d.tileset(unnamed).expect("there").name(), "Tileset 2");
    assert_eq!(
        d.tileset(TilesetId::new(99)).err(),
        Some(LimitError::NoSuchItem)
    );
}

#[test]
fn tiles_are_added_duplicated_moved_and_removed() {
    let mut d = doc();
    let set = d.add_tileset("T", ColorMode::Rgba, (4, 4)).expect("room");
    let a = d.add_tile(set, drawn(4, 4, 1)).expect("fits");
    let b = d
        .add_tile(set, Arc::new(Image::blank()))
        .expect("blank fits");
    let copy = d.duplicate_tile(set, a).expect("there");
    let order = |d: &Document| -> Vec<u32> {
        let tiles = d.tileset(set).expect("there").tiles();
        tiles.iter().map(|e| e.id.get()).collect()
    };
    assert_eq!(order(&d), [a.get(), copy.get(), b.get()]);
    let tiles = d.tileset(set).expect("there");
    assert!(Arc::ptr_eq(
        &tiles.tile(a).expect("a").image,
        &tiles.tile(copy).expect("copy").image
    ));
    assert!(d.move_tile(set, b, 0).expect("there"));
    assert_eq!(order(&d), [b.get(), a.get(), copy.get()]);
    d.remove_tile(set, a).expect("there");
    assert_eq!(order(&d), [b.get(), copy.get()]);
    assert_eq!(d.remove_tile(set, a), Err(LimitError::NoSuchItem));
    assert_eq!(d.duplicate_tile(set, a).err(), Some(LimitError::NoSuchItem));
}

#[test]
fn an_image_larger_than_the_tile_is_refused() {
    let mut d = doc();
    let set = d.add_tileset("T", ColorMode::Rgba, (4, 4)).expect("room");
    assert_eq!(
        d.add_tile(set, drawn(5, 4, 1)).err(),
        Some(LimitError::TileSize)
    );
    assert!(d.tileset(set).expect("there").is_empty());
}

#[test]
fn a_palette_tile_may_name_only_colours_the_palette_has() {
    let mut d = Document::new(8, 8, ColorMode::Palette).expect("size");
    let set = d
        .add_tileset("T", ColorMode::Palette, (2, 2))
        .expect("room");
    let id = Arc::make_mut(&mut d.colors)
        .add_swatch(0xff00_00ff)
        .expect("room");
    assert!(d.add_tile(set, drawn(2, 2, id)).is_ok());
    assert_eq!(
        d.add_tile(set, drawn(2, 2, id + 5)).err(),
        Some(LimitError::NoSuchItem)
    );
}

#[test]
fn removing_a_tileset_and_adding_another_gives_a_new_id() {
    let mut d = doc();
    let a = d.add_tileset("A", ColorMode::Rgba, (8, 8)).expect("room");
    d.remove_tileset(a).expect("unused");
    assert_eq!(d.remove_tileset(a), Err(LimitError::NoSuchItem));
    let b = d.add_tileset("B", ColorMode::Rgba, (8, 8)).expect("room");
    assert_ne!(a, b);
}

#[test]
fn renaming_to_the_same_or_an_empty_name_changes_nothing() {
    let mut d = doc();
    let set = d.add_tileset("A", ColorMode::Rgba, (8, 8)).expect("room");
    assert_eq!(d.rename_tileset(set, "A"), Ok(false));
    assert_eq!(d.rename_tileset(set, " \n"), Ok(false));
    assert_eq!(d.rename_tileset(set, "B"), Ok(true));
    assert_eq!(d.tileset(set).expect("there").name(), "B");
}

#[test]
fn swapping_images_twice_puts_them_back_and_skips_missing_tiles() {
    let mut d = doc();
    let set = d.add_tileset("T", ColorMode::Rgba, (4, 4)).expect("room");
    let a = d.add_tile(set, drawn(4, 4, 1)).expect("fits");
    let gone = TileId::new(77).expect("an id");
    let mut list = vec![(a, drawn(4, 4, 2)), (gone, drawn(4, 4, 3))];
    d.swap_tile_images(set, &mut list);
    assert_eq!(
        d.tileset(set)
            .expect("there")
            .tile(a)
            .expect("a")
            .image
            .get(0, 0),
        2
    );
    assert_eq!(list[0].1.get(0, 0), 1);
    d.swap_tile_images(set, &mut list);
    assert_eq!(
        d.tileset(set)
            .expect("there")
            .tile(a)
            .expect("a")
            .image
            .get(0, 0),
        1
    );
}
