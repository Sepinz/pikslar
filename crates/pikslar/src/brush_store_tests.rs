use super::*;
use pikslar_core::custom_brush::Brush;
use pikslar_format::brush_lib::write_library;
use std::time::SystemTime;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-brushes-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn brush(name: &str, seed: u8) -> LibraryBrush {
    LibraryBrush {
        name: name.to_string(),
        width: 2,
        height: 1,
        paint: Paint::OwnColors,
        orientation: 0,
        handle: brush_lib::Handle::Centre,
        rgba: vec![seed, 2, 3, 255, 0, 0, 0, 0],
    }
}

fn file_in(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

fn put(dir: &Path, brushes: &[LibraryBrush]) -> Vec<u8> {
    fs::create_dir_all(dir).expect("dir");
    let bytes = write_library(brushes).expect("written");
    fs::write(file_in(dir), &bytes).expect("file");
    bytes
}

#[test]
fn red_green_blue_alpha_bytes_become_the_cell_colour_and_back() {
    let entry = LibraryBrush {
        width: 2,
        rgba: vec![0x11, 0x22, 0x33, 0xff, 9, 9, 9, 0],
        ..brush("b", 0)
    };
    let made = from_library(&entry).expect("brush");
    // The cell format keeps the red byte lowest, as `rgba` packs it; clear is exactly 0.
    assert_eq!(made.colors(), [0xff33_2211, 0]);
    let back = to_library(&made);
    assert_eq!(back.rgba, vec![0x11, 0x22, 0x33, 0xff, 0, 0, 0, 0]);
}

#[test]
fn a_turned_brush_with_a_corner_handle_round_trips() {
    for index in 0..8u8 {
        let entry = LibraryBrush {
            orientation: index,
            handle: brush_lib::Handle::Corner(index % 4),
            paint: Paint::Shape,
            ..brush("t", 7)
        };
        let made = from_library(&entry).expect("brush");
        assert_eq!(made.orientation().index(), index);
        assert_eq!(to_library(&made), entry);
    }
}

#[test]
fn an_empty_entry_or_a_bad_orientation_is_not_a_brush() {
    let clear = LibraryBrush {
        rgba: vec![0; 8],
        ..brush("c", 0)
    };
    assert!(from_library(&clear).is_none());
    let bad = LibraryBrush {
        orientation: 9,
        ..brush("o", 1)
    };
    assert!(from_library(&bad).is_none());
}

#[test]
fn a_session_brush_converts_with_its_name_cut_to_the_limit() {
    let long = "é".repeat(200);
    let b = Brush::from_parts(
        &long,
        1,
        1,
        &[rgba(1, 2, 3, 255)],
        None,
        BrushPaint::OwnColors,
    )
    .expect("brush");
    let lib = to_library(&b);
    assert!(lib.name.len() <= MAX_NAME_BYTES && lib.check().is_ok());
}

#[test]
fn nothing_is_read_until_asked() {
    let dir = temp_dir("lazy");
    put(&dir, &[brush("a", 1)]);
    let store = BrushStore::at(Some(file_in(&dir)));
    assert!(!store.is_read());
    assert!(store.brushes().is_empty());
}

#[test]
fn a_missing_file_is_an_empty_library_and_the_first_save_makes_it() {
    let dir = temp_dir("missing");
    let mut store = BrushStore::at(Some(file_in(&dir)));
    assert_eq!(store.ensure_read(), None);
    assert!(store.brushes().is_empty());
    assert_eq!(store.add(brush("a", 1)), Ok(Saved::Added("a".to_string())));
    let mut again = BrushStore::at(Some(file_in(&dir)));
    again.ensure_read();
    assert_eq!(again.brushes(), [brush("a", 1)]);
}

#[test]
fn the_same_brush_twice_is_kept_once_and_a_name_clash_is_numbered() {
    let dir = temp_dir("names");
    let mut store = BrushStore::at(Some(file_in(&dir)));
    store.ensure_read();
    store.add(brush("a", 1)).expect("first");
    assert_eq!(store.add(brush("a", 1)), Ok(Saved::AlreadyThere));
    assert_eq!(
        store.add(brush("a", 2)),
        Ok(Saved::Added("a 2".to_string()))
    );
    assert_eq!(store.brushes().len(), 2);
}

#[test]
fn removing_writes_the_rest() {
    let dir = temp_dir("remove");
    put(&dir, &[brush("a", 1), brush("b", 2)]);
    let mut store = BrushStore::at(Some(file_in(&dir)));
    assert_eq!(store.ensure_read(), None);
    assert_eq!(store.remove(0), Ok("a".to_string()));
    let mut again = BrushStore::at(Some(file_in(&dir)));
    again.ensure_read();
    assert_eq!(again.brushes(), [brush("b", 2)]);
}

#[test]
fn a_full_library_refuses_one_more() {
    let dir = temp_dir("full");
    let many: Vec<_> = (0..MAX_BRUSHES)
        .map(|i| brush(&format!("b{i}"), 1))
        .collect();
    put(&dir, &many);
    let mut store = BrushStore::at(Some(file_in(&dir)));
    store.ensure_read();
    assert!(store.add(brush("extra", 9)).is_err());
    assert_eq!(store.brushes().len(), MAX_BRUSHES);
}

#[test]
fn a_damaged_record_is_kept_aside_once_and_only_it_is_lost() {
    let dir = temp_dir("damaged");
    let mut bytes = put(&dir, &[brush("a", 1), brush("b", 2)]);
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(file_in(&dir), &bytes).expect("damage");
    let mut store = BrushStore::at(Some(file_in(&dir)));
    let said = store.ensure_read().expect("a message");
    assert!(
        said.contains("1 brush") && said.contains(DAMAGED_NAME),
        "{said}"
    );
    assert_eq!(store.brushes(), [brush("a", 1)]);
    assert_eq!(fs::read(dir.join(DAMAGED_NAME)).expect("kept"), bytes);
    // A second run with the same file keeps the same copy and says the same.
    let mut next = BrushStore::at(Some(file_in(&dir)));
    assert!(next.ensure_read().is_some());
    assert_eq!(fs::read(dir.join(DAMAGED_NAME)).expect("kept"), bytes);
    // Saving now writes a clean library.
    store.add(brush("c", 3)).expect("saved");
    let mut clean = BrushStore::at(Some(file_in(&dir)));
    assert_eq!(clean.ensure_read(), None);
    assert_eq!(clean.brushes().len(), 2);
}

#[test]
fn a_file_that_is_no_library_moves_aside_and_the_library_starts_empty() {
    let dir = temp_dir("garbage");
    fs::create_dir_all(&dir).expect("dir");
    fs::write(file_in(&dir), b"this is not a brush library at all").expect("file");
    let mut store = BrushStore::at(Some(file_in(&dir)));
    let said = store.ensure_read().expect("a message");
    assert!(said.contains(DAMAGED_NAME), "{said}");
    assert!(!file_in(&dir).exists());
    assert!(dir.join(DAMAGED_NAME).exists());
    assert!(store.add(brush("a", 1)).is_ok());
}

#[test]
fn a_newer_version_is_read_but_never_written_over() {
    let dir = temp_dir("newer");
    let mut bytes = put(&dir, &[brush("a", 1)]);
    bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
    fs::write(file_in(&dir), &bytes).expect("newer");
    let mut store = BrushStore::at(Some(file_in(&dir)));
    let said = store.ensure_read().expect("a message");
    assert!(said.contains("newer"), "{said}");
    assert_eq!(store.brushes(), [brush("a", 1)]);
    assert!(store.add(brush("b", 2)).is_err());
    assert!(store.remove(0).is_err());
    assert_eq!(fs::read(file_in(&dir)).expect("file"), bytes);
    assert!(!dir.join(DAMAGED_NAME).exists());
}

#[test]
fn with_no_folder_there_is_no_library_and_saving_says_so() {
    let mut store = BrushStore::at(None);
    assert_eq!(store.ensure_read(), None);
    assert!(store.is_read() && !store.is_enabled());
    assert!(store.add(brush("a", 1)).is_err());
}

fn damaged_file(dir: &Path, seed: u8) -> Vec<u8> {
    let mut bytes = put(dir, &[brush("a", seed), brush("b", 2)]);
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(file_in(dir), &bytes).expect("damage");
    bytes
}

#[test]
fn an_earlier_kept_copy_is_never_deleted_or_replaced() {
    let dir = temp_dir("aside-names");
    fs::create_dir_all(&dir).expect("dir");
    fs::write(dir.join(DAMAGED_NAME), b"precious earlier copy").expect("earlier");
    fs::write(file_in(&dir), b"this is not a brush library at all").expect("file");
    let mut store = BrushStore::at(Some(file_in(&dir)));
    let said = store.ensure_read().expect("a message");
    assert!(said.contains("brushes.damaged.2.pkb"), "{said}");
    assert_eq!(
        fs::read(dir.join(DAMAGED_NAME)).expect("kept"),
        b"precious earlier copy"
    );
    assert!(dir.join("brushes.damaged.2.pkb").exists());
}

#[test]
fn when_every_aside_name_is_taken_the_bad_file_is_left_alone_and_it_says_so() {
    let dir = temp_dir("aside-full");
    fs::create_dir_all(&dir).expect("dir");
    for n in 1..=MAX_ASIDE {
        fs::write(dir.join(aside_name(n)), format!("copy {n}")).expect("copy");
    }
    fs::write(file_in(&dir), b"this is not a brush library at all").expect("file");
    let mut store = BrushStore::at(Some(file_in(&dir)));
    let said = store.ensure_read().expect("a message");
    assert!(
        said.contains("left alone") && said.contains("taken"),
        "{said}"
    );
    assert_eq!(
        fs::read(file_in(&dir)).expect("still there"),
        b"this is not a brush library at all"
    );
    assert!(store.add(brush("a", 1)).is_err());
    for n in 1..=MAX_ASIDE {
        let kept = fs::read(dir.join(aside_name(n))).expect("copy");
        assert_eq!(kept, format!("copy {n}").into_bytes());
    }
}

#[test]
fn a_different_damaged_file_gets_the_next_name_and_the_same_one_is_reused() {
    let dir = temp_dir("aside-copy");
    let first = damaged_file(&dir, 1);
    BrushStore::at(Some(file_in(&dir))).ensure_read();
    let second = damaged_file(&dir, 5);
    let said = BrushStore::at(Some(file_in(&dir)))
        .ensure_read()
        .expect("message");
    assert!(said.contains("brushes.damaged.2.pkb"), "{said}");
    assert_eq!(fs::read(dir.join(DAMAGED_NAME)).expect("one"), first);
    assert_eq!(
        fs::read(dir.join("brushes.damaged.2.pkb")).expect("two"),
        second
    );
    // The second file again: reused, no third copy.
    let said = BrushStore::at(Some(file_in(&dir)))
        .ensure_read()
        .expect("message");
    assert!(said.contains("brushes.damaged.2.pkb"), "{said}");
    assert!(!dir.join("brushes.damaged.3.pkb").exists());
}

#[test]
fn a_second_instances_save_is_merged_not_lost() {
    let dir = temp_dir("two-instances");
    put(&dir, &[brush("a", 1)]);
    let mut one = BrushStore::at(Some(file_in(&dir)));
    let mut two = BrushStore::at(Some(file_in(&dir)));
    one.ensure_read();
    two.ensure_read();
    two.add(brush("b", 2)).expect("two saves");
    one.add(brush("c", 3)).expect("one saves");
    assert_eq!(one.brushes().len(), 3);
    let mut fresh = BrushStore::at(Some(file_in(&dir)));
    fresh.ensure_read();
    assert_eq!(fresh.brushes().len(), 3);
}

#[test]
fn a_change_of_time_alone_is_noticed() {
    let dir = temp_dir("time-only");
    put(&dir, &[brush("a", 1)]);
    let mut one = BrushStore::at(Some(file_in(&dir)));
    one.ensure_read();
    // Same length, different brush, different time.
    put(&dir, &[brush("a", 9)]);
    let later = SystemTime::now() + std::time::Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(file_in(&dir))
        .and_then(|f| f.set_modified(later))
        .expect("time");
    one.add(brush("b", 2)).expect("saved");
    assert_eq!(one.brushes(), [brush("a", 9), brush("b", 2)]);
}

#[test]
fn removing_finds_the_same_brush_after_another_instance_changed_the_list() {
    let dir = temp_dir("remove-merge");
    put(&dir, &[brush("a", 1), brush("b", 2)]);
    let mut one = BrushStore::at(Some(file_in(&dir)));
    one.ensure_read();
    put(&dir, &[brush("z", 7), brush("a", 1), brush("b", 2)]);
    assert_eq!(one.remove(1), Ok("b".to_string()));
    assert_eq!(one.brushes(), [brush("z", 7), brush("a", 1)]);
    // Gone from under it entirely.
    put(&dir, &[brush("z", 7)]);
    assert!(one.remove(1).is_err());
    assert_eq!(one.brushes(), [brush("z", 7)]);
}

#[test]
fn a_file_upgraded_by_a_newer_build_since_it_was_read_is_not_written_over() {
    let dir = temp_dir("upgraded");
    let mut bytes = put(&dir, &[brush("a", 1)]);
    let mut one = BrushStore::at(Some(file_in(&dir)));
    one.ensure_read();
    bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(b"newer section");
    fs::write(file_in(&dir), &bytes).expect("newer");
    let why = one.add(brush("b", 2)).expect_err("refused");
    assert!(why.contains("newer"), "{why}");
    assert!(one.remove(0).is_err());
    assert_eq!(fs::read(file_in(&dir)).expect("file"), bytes);
}

#[test]
fn removing_a_stale_index_is_an_error_with_no_name() {
    let dir = temp_dir("stale");
    put(&dir, &[brush("a", 1)]);
    let mut store = BrushStore::at(Some(file_in(&dir)));
    store.ensure_read();
    assert!(store.remove(5).is_err());
    assert_eq!(store.brushes().len(), 1);
}

#[test]
fn a_renamed_brush_is_written_and_read_back_after_a_restart() {
    let dir = temp_dir("rename");
    put(&dir, &[brush("a", 1), brush("b", 2)]);
    let mut store = BrushStore::at(Some(file_in(&dir)));
    store.ensure_read();
    assert_eq!(store.rename(1, "Leaves"), Ok(Some("Leaves".to_string())));
    assert_eq!(store.brushes()[1].name, "Leaves");
    let mut again = BrushStore::at(Some(file_in(&dir)));
    assert_eq!(again.ensure_read(), None);
    let names: Vec<_> = again.brushes().iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["a", "Leaves"]);
    // Only the name changed.
    assert_eq!(again.brushes()[1].rgba, brush("b", 2).rgba);
}

#[test]
fn a_rename_to_an_empty_or_the_same_name_writes_nothing() {
    let dir = temp_dir("rename-same");
    let bytes = put(&dir, &[brush("a", 1)]);
    let mut store = BrushStore::at(Some(file_in(&dir)));
    store.ensure_read();
    let before = store.version();
    for name in ["", "   ", "\u{200B}\u{7}", "a", "  a  "] {
        assert_eq!(store.rename(0, name), Ok(None), "{name:?}");
    }
    assert_eq!(fs::read(file_in(&dir)).expect("file"), bytes);
    assert_eq!(store.version(), before, "nothing to draw again");
}

#[test]
fn a_rename_follows_the_shared_name_rule_and_the_length_limit() {
    // As a layer's name: hidden characters (a tab, an override) are left out, then it is trimmed.
    assert_eq!(clean_name("  Moss\u{202E}y\tleaf  "), "Mossyleaf");
    assert_eq!(clean_name(" Mossy leaf "), "Mossy leaf");
    assert_eq!(clean_name("\u{2066}x\u{2069}"), "x");
    let long = "ä".repeat(MAX_BRUSH_NAME + 10);
    let cut = clean_name(&long);
    assert_eq!(cut.chars().count(), MAX_BRUSH_NAME);
    assert!(cut.len() <= MAX_NAME_BYTES);
    let wide = "\u{10348}".repeat(MAX_BRUSH_NAME);
    assert!(
        clean_name(&wide).len() <= MAX_NAME_BYTES,
        "four bytes a letter fit"
    );
    assert_eq!(
        clean_name(&format!("{} x", "b".repeat(MAX_BRUSH_NAME - 1))),
        "b".repeat(MAX_BRUSH_NAME - 1)
    );
}

#[test]
fn a_rename_to_another_brushs_name_is_numbered_as_saving_is() {
    let dir = temp_dir("rename-clash");
    put(&dir, &[brush("a", 1), brush("b", 2), brush("a 2", 3)]);
    let mut store = BrushStore::at(Some(file_in(&dir)));
    store.ensure_read();
    assert_eq!(store.rename(1, "a"), Ok(Some("a 3".to_string())));
    // Its own name is not a clash: renaming "a 3" to "a 3 " keeps it.
    assert_eq!(store.rename(1, "a 3 "), Ok(None));
    let long = "c".repeat(MAX_BRUSH_NAME);
    assert_eq!(store.rename(0, &long), Ok(Some(long.clone())));
    let numbered = store.rename(2, &long).expect("renamed").expect("changed");
    assert!(
        numbered.ends_with(" 2") && numbered.chars().count() <= MAX_BRUSH_NAME,
        "{numbered}"
    );
}

#[test]
fn a_rename_finds_the_brush_after_another_instance_changed_the_list() {
    let dir = temp_dir("rename-merge");
    put(&dir, &[brush("a", 1), brush("b", 2)]);
    let mut one = BrushStore::at(Some(file_in(&dir)));
    one.ensure_read();
    put(&dir, &[brush("z", 7), brush("a", 1), brush("b", 2)]);
    assert_eq!(one.rename(1, "bee"), Ok(Some("bee".to_string())));
    let names: Vec<_> = one.brushes().iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["z", "a", "bee"]);
    put(&dir, &[brush("z", 7)]);
    assert!(one.rename(1, "gone").is_err(), "a is no longer there");
    assert_eq!(one.brushes(), [brush("z", 7)]);
    assert!(one.rename(9, "stale").is_err());
}

#[test]
fn a_rename_is_refused_without_settings_or_for_a_newer_file() {
    let mut off = BrushStore::at(None);
    off.ensure_read();
    assert!(off.rename(0, "x").is_err());
    let dir = temp_dir("rename-newer");
    let mut bytes = put(&dir, &[brush("a", 1)]);
    bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
    fs::write(file_in(&dir), &bytes).expect("newer");
    let mut store = BrushStore::at(Some(file_in(&dir)));
    store.ensure_read();
    assert!(store.rename(0, "b").is_err());
    assert_eq!(fs::read(file_in(&dir)).expect("file"), bytes);
}

#[test]
fn the_version_changes_whenever_the_brushes_do() {
    let dir = temp_dir("version");
    put(&dir, &[brush("a", 1)]);
    let mut store = BrushStore::at(Some(file_in(&dir)));
    assert_eq!(store.version(), 0, "not read: nothing to show");
    store.ensure_read();
    let read = store.version();
    assert_ne!(read, 0);
    store.add(brush("b", 2)).expect("added");
    let added = store.version();
    assert_ne!(added, read);
    store.rename(1, "c").expect("renamed");
    let renamed = store.version();
    assert_ne!(renamed, added);
    store.remove(0).expect("removed");
    assert_ne!(store.version(), renamed);
    // Another store never shares a version, so a cache cannot mistake one for the other.
    let mut other = BrushStore::at(Some(file_in(&dir)));
    other.ensure_read();
    assert_ne!(other.version(), store.version());
}
