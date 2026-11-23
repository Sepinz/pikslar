use super::*;

/// An icon file with images of the given (size byte, data) pairs.
fn ico(images: &[(u8, &[u8])]) -> Vec<u8> {
    let mut f = vec![0, 0, 1, 0, images.len() as u8, 0];
    let mut offset = 6 + 16 * images.len();
    for (size, data) in images {
        f.extend_from_slice(&[*size, *size, 0, 0, 1, 0, 32, 0]);
        f.extend_from_slice(&(data.len() as u32).to_le_bytes());
        f.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
    }
    for (_, data) in images {
        f.extend_from_slice(data);
    }
    f
}

#[test]
fn each_image_is_read_with_its_directory_entry() {
    let images = parse(&ico(&[(16, b"small"), (0, b"\x89PNGbig")])).expect("ok");
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].data, b"small");
    assert_eq!(images[1].data, b"\x89PNGbig");
    assert_eq!(images[0].entry[0], 16);
    assert_eq!(&images[1].entry[8..12], &7u32.to_le_bytes());
}

#[test]
fn the_group_repeats_each_entry_with_the_image_resource_id() {
    let res = resources(&ico(&[(16, b"a"), (32, b"bb")]), 1, 5, 0x409).expect("ok");
    assert_eq!(res.len(), 3);
    assert_eq!(
        (res[0].ty, res[0].id, res[0].data.as_slice()),
        (RT_ICON, 5, &b"a"[..])
    );
    assert_eq!(
        (res[1].ty, res[1].id, res[1].data.as_slice()),
        (RT_ICON, 6, &b"bb"[..])
    );
    let group = &res[2];
    assert_eq!((group.ty, group.id, group.lang), (RT_GROUP_ICON, 1, 0x409));
    assert_eq!(&group.data[..6], &[0, 0, 1, 0, 2, 0]);
    assert_eq!(group.data.len(), 6 + 14 * 2);
    assert_eq!(&group.data[6..18], &ico(&[(16, b"a"), (32, b"bb")])[6..18]);
    assert_eq!(&group.data[18..20], &5u16.to_le_bytes());
    assert_eq!(&group.data[32..34], &6u16.to_le_bytes());
}

#[test]
fn a_file_that_is_not_an_icon_is_refused() {
    assert!(parse(b"\x89PNG\r\n\x1a\n").is_err());
    let mut cursor = ico(&[(16, b"a")]);
    cursor[2] = 2;
    assert!(parse(&cursor).expect_err("refused").contains("not an icon"));
}

#[test]
fn an_image_past_the_end_of_the_file_is_refused() {
    let mut f = ico(&[(16, b"abc")]);
    f.truncate(f.len() - 1);
    assert!(parse(&f).is_err());
}

#[test]
fn an_icon_with_no_images_is_refused() {
    assert!(parse(&ico(&[])).expect_err("refused").contains("no images"));
}

#[test]
fn the_repository_icon_has_every_size_the_plan_names() {
    let bytes = include_bytes!("../../pikslar/res/pikslar.ico");
    let sizes: Vec<u8> = parse(bytes)
        .expect("ok")
        .iter()
        .map(|i| i.entry[0])
        .collect();
    assert_eq!(sizes, [16, 20, 24, 32, 40, 48, 64, 0]);
}

#[test]
fn image_ids_that_would_overflow_are_refused() {
    let f = ico(&[(16, b"a"), (32, b"b")]);
    assert!(resources(&f, 1, u16::MAX, 0x409).is_err());
    assert_eq!(resources(&f, 1, u16::MAX - 1, 0x409).expect("ok").len(), 3);
}
