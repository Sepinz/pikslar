use super::*;

fn noisy(w: u16, h: u16, seed: u32) -> Vec<u8> {
    let mut x = seed | 1;
    (0..LibraryBrush::pixel_bytes(w, h))
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn brush(name: &str, w: u16, h: u16, flat: bool) -> LibraryBrush {
    let rgba = if flat {
        [10, 20, 30, 255].repeat(w as usize * h as usize)
    } else {
        noisy(w, h, w as u32 * 31 + h as u32)
    };
    LibraryBrush {
        name: name.to_owned(),
        width: w,
        height: h,
        paint: Paint::OwnColors,
        orientation: 0,
        handle: Handle::Centre,
        rgba,
    }
}

fn three() -> Vec<LibraryBrush> {
    let mut b = vec![
        brush("flat", 8, 8, true),
        brush("noisy \u{e9}\u{1f600}", 5, 3, false),
        brush("", 1, 1, true),
    ];
    b[1].paint = Paint::Shape;
    b[1].orientation = 5;
    b[1].handle = Handle::Corner(3);
    b[2].handle = Handle::Corner(0);
    b
}

/// A file from record bodies, each framed with a correct length and checksum.
fn frame(version: u32, count: u32, bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut f = MAGIC.to_vec();
    f.extend_from_slice(&version.to_le_bytes());
    f.extend_from_slice(&count.to_le_bytes());
    for b in bodies {
        f.extend_from_slice(&(b.len() as u32).to_le_bytes());
        f.extend_from_slice(&crc32(&[b]).to_le_bytes());
        f.extend_from_slice(b);
    }
    f
}

fn bodies_of(brushes: &[LibraryBrush]) -> Vec<Vec<u8>> {
    brushes.iter().map(encode_body).collect()
}

/// Offset of record `i`'s head in a file written by `write_library`.
fn record_at(file: &[u8], i: usize) -> usize {
    let mut pos = HEADER_LEN;
    for _ in 0..i {
        pos += RECORD_HEAD_LEN + u32_le(file, pos) as usize;
    }
    pos
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn names(lib: &Library) -> Vec<&str> {
    lib.brushes.iter().map(|b| b.name.as_str()).collect()
}

#[test]
fn round_trip_keeps_everything() {
    let brushes = three();
    let file = write_library(&brushes).expect("writes");
    let lib = read_library(&file).expect("reads");
    assert_eq!(lib.brushes, brushes);
    assert!(lib.report.is_clean() && !lib.report.newer_version);
}

#[test]
fn flat_brushes_are_deflated_and_noisy_ones_raw() {
    let file = write_library(&three()).expect("writes");
    let encoding = |i: usize| {
        let at = record_at(&file, i) + RECORD_HEAD_LEN;
        let name = u16::from_le_bytes([file[at], file[at + 1]]) as usize;
        file[at + 2 + name + 7]
    };
    assert_eq!((encoding(0), encoding(1)), (1, 0));
}

#[test]
fn an_empty_library_round_trips() {
    let file = write_library(&[]).expect("writes");
    assert_eq!(file.len(), HEADER_LEN);
    let lib = read_library(&file).expect("reads");
    assert!(lib.brushes.is_empty() && lib.report.is_clean());
}

#[test]
fn the_largest_brush_round_trips() {
    let b = vec![brush("big", 256, 256, false), brush("wide", 256, 1, true)];
    let lib = read_library(&write_library(&b).expect("writes")).expect("reads");
    assert_eq!(lib.brushes, b);
}

#[test]
fn every_orientation_and_handle_round_trips() {
    let mut all = Vec::new();
    for o in 0..8u8 {
        for h in [Handle::Centre, Handle::Corner(0), Handle::Corner(3)] {
            let mut b = brush("x", 2, 2, true);
            (b.orientation, b.handle) = (o, h);
            all.push(b);
        }
    }
    let lib = read_library(&write_library(&all).expect("writes")).expect("reads");
    assert_eq!(lib.brushes, all);
}

#[test]
fn a_flipped_body_byte_loses_only_that_record() {
    let file = write_library(&three()).expect("writes");
    for victim in 0..3 {
        let mut bad = file.clone();
        let at = record_at(&bad, victim) + RECORD_HEAD_LEN + 3;
        bad[at] ^= 0x40;
        let lib = read_library(&bad).expect("reads");
        let mut expect: Vec<&str> = vec!["flat", "noisy \u{e9}\u{1f600}", ""];
        expect.remove(victim);
        assert_eq!(names(&lib), expect, "victim {victim}");
        assert_eq!(lib.report.lost, 1);
    }
}

#[test]
fn a_wrong_checksum_loses_only_that_record() {
    let mut bad = write_library(&three()).expect("writes");
    let at = record_at(&bad, 1) + 4;
    bad[at] ^= 1;
    let lib = read_library(&bad).expect("reads");
    assert_eq!(lib.brushes.len(), 2);
    assert_eq!(lib.report.lost, 1);
    assert_eq!(lib.brushes[0], three()[0]);
    assert_eq!(lib.brushes[1], three()[2]);
}

#[test]
fn every_bad_field_with_a_good_checksum_loses_only_itself() {
    let good = encode_body(&brush("ok", 4, 4, false));
    let tail = 2 + 2; // name length and "ok"
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut with = |label: &'static str, f: &dyn Fn(&mut Vec<u8>)| {
        let mut b = good.clone();
        f(&mut b);
        cases.push((label, b));
    };
    with("paint", &|b| b[tail] = 2);
    with("orientation", &|b| b[tail + 1] = 8);
    with("handle", &|b| b[tail + 2] = 5);
    with("zero width", &|b| b[tail + 3..tail + 5].fill(0));
    with("wide", &|b| {
        b[tail + 3..tail + 5].copy_from_slice(&257u16.to_le_bytes())
    });
    with("zero height", &|b| b[tail + 5..tail + 7].fill(0));
    with("tall", &|b| {
        b[tail + 5..tail + 7].copy_from_slice(&257u16.to_le_bytes())
    });
    with("encoding", &|b| b[tail + 7] = 2);
    with("short pixels", &|b| {
        let _ = b.pop();
    });
    with("long pixels", &|b| b.push(0));
    with("name past the end", &|b| {
        b[..2].copy_from_slice(&300u16.to_le_bytes())
    });
    with("name over 256", &|b| {
        b.splice(2..4, vec![b'a'; 257]);
        b[..2].copy_from_slice(&257u16.to_le_bytes());
    });
    with("not utf-8", &|b| b[2] = 0xff);
    with("empty body", &|b| b.clear());
    with("one byte body", &|b| b.truncate(1));
    with("deflate junk", &|b| {
        b[tail + 7] = 1;
        b.truncate(tail + 8);
        b.extend_from_slice(&[0xff, 0xff, 0xff]);
    });
    with("deflate of the wrong size", &|b| {
        b[tail + 7] = 1;
        b.truncate(tail + 8);
        b.extend_from_slice(&deflate::deflate(&[0u8; 10]));
    });
    with("deflate bomb", &|b| {
        b[tail + 7] = 1;
        b.truncate(tail + 8);
        b.extend_from_slice(&deflate::deflate(&vec![0u8; 1 << 20]));
    });
    let after = brush("after", 2, 2, true);
    for (label, bad) in cases {
        let file = frame(1, 3, &[encode_body(&after), bad, encode_body(&after)]);
        let lib = read_library(&file).expect("reads");
        assert_eq!(lib.brushes.len(), 2, "{label}");
        assert_eq!(lib.report.lost, 1, "{label}");
    }
}

#[test]
fn a_damaged_length_loses_the_rest_but_not_the_earlier_records() {
    let file = write_library(&three()).expect("writes");
    for new_len in [0u32, 1, 7, 1 << 20, u32::MAX] {
        let mut bad = file.clone();
        let at = record_at(&bad, 1);
        bad[at..at + 4].copy_from_slice(&new_len.to_le_bytes());
        let lib = read_library(&bad).expect("reads");
        assert_eq!(names(&lib), vec!["flat"], "length {new_len}");
        assert_eq!(lib.report.lost, 2, "length {new_len}");
    }
}

#[test]
fn every_truncation_keeps_the_whole_records_before_it() {
    let brushes = three();
    let file = write_library(&brushes).expect("writes");
    let ends: Vec<usize> = (1..=3)
        .map(|i| {
            if i == 3 {
                file.len()
            } else {
                record_at(&file, i)
            }
        })
        .collect();
    for cut in 0..file.len() {
        let result = read_library(&file[..cut]);
        if cut < HEADER_LEN {
            assert!(result.is_err(), "cut {cut}");
            continue;
        }
        let lib = result.expect("reads");
        let whole = ends.iter().filter(|&&e| e <= cut).count();
        assert_eq!(lib.brushes.len(), whole, "cut {cut}");
        assert_eq!(lib.report.lost, 3 - whole, "cut {cut}");
    }
}

#[test]
fn not_a_library_is_refused() {
    let good = write_library(&three()).expect("writes");
    assert!(read_library(&[]).is_err());
    assert!(read_library(b"PKBR").is_err());
    let mut wrong = good.clone();
    wrong[0] = b'X';
    assert!(read_library(&wrong).is_err());
    assert!(read_library(&frame(0, 0, &[])).is_err());
}

#[test]
fn a_newer_version_is_read_and_marked() {
    let file = frame(2, 3, &bodies_of(&three()));
    let lib = read_library(&file).expect("reads");
    assert_eq!(lib.brushes, three());
    assert!(lib.report.newer_version);
    let far = frame(u32::MAX, 0, &[]);
    assert!(read_library(&far).expect("reads").report.newer_version);
}

#[test]
fn more_than_64_brushes_is_refused_on_write_and_cut_on_read() {
    let many: Vec<LibraryBrush> = (0..65)
        .map(|i| brush(&format!("b{i}"), 1, 1, true))
        .collect();
    assert!(write_library(&many).is_err());
    let lib = read_library(&frame(1, 65, &bodies_of(&many))).expect("reads");
    assert_eq!(lib.brushes.len(), 64);
    assert!(lib.report.over_limit);
    assert_eq!(lib.report.lost, 0);
    assert_eq!(
        write_library(&many[..64]).map(|f| !f.is_empty()).ok(),
        Some(true)
    );
}

#[test]
fn a_huge_declared_count_costs_nothing() {
    let lib = read_library(&frame(1, u32::MAX, &[])).expect("reads");
    assert!(lib.brushes.is_empty() && lib.report.over_limit);
    assert_eq!(lib.report.lost, MAX_BRUSHES);
}

#[test]
fn pixels_over_four_mib_keep_what_fits() {
    // Sixteen full-size noisy brushes are exactly 4 MiB; the seventeenth does not fit.
    let seventeen: Vec<LibraryBrush> = (0..17)
        .map(|i| {
            let mut b = brush("s", 256, 256, false);
            b.rgba = noisy(256, 256, i + 1);
            b
        })
        .collect();
    assert!(write_library(&seventeen).is_err());
    let lib = read_library(&frame(1, 17, &bodies_of(&seventeen))).expect("reads");
    assert_eq!(lib.brushes.len(), 16);
    assert!(lib.report.over_limit);
    assert_eq!(lib.report.lost, 1);
    assert!(write_library(&seventeen[..16]).is_ok());
}

#[test]
fn a_file_over_16_mib_is_cut_and_says_so() {
    let mut file = write_library(&three()).expect("writes");
    file.resize(MAX_FILE_BYTES + 1, 0);
    let lib = read_library(&file).expect("reads");
    assert_eq!(lib.brushes.len(), 3);
    assert!(lib.report.cut && !lib.report.is_clean());
}

#[test]
fn the_writer_refuses_what_the_reader_would_not_accept() {
    let ok = brush("ok", 4, 4, true);
    let bad = |f: &dyn Fn(&mut LibraryBrush)| {
        let mut b = ok.clone();
        f(&mut b);
        write_library(&[b]).is_err()
    };
    assert!(bad(&|b| b.name = "a".repeat(257)));
    assert!(bad(&|b| b.width = 0));
    assert!(bad(&|b| b.height = 257));
    assert!(bad(&|b| b.orientation = 8));
    assert!(bad(&|b| b.handle = Handle::Corner(4)));
    assert!(bad(&|b| b.rgba.pop().map(drop).unwrap_or(())));
    let mut longest = ok;
    longest.name = "a".repeat(256);
    assert!(write_library(&[longest]).is_ok());
}

#[test]
fn save_and_load_through_a_file() {
    let dir = std::env::temp_dir().join(format!("pikslar-brush-lib-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("brushes.pkb");
    save_library(&path, &three()).expect("saves");
    save_library(&path, &three()[..1]).expect("saves again");
    let lib = load_library(&path).expect("loads");
    assert_eq!(lib.brushes, three()[..1]);
    assert!(load_library(&dir.join("missing.pkb")).is_err());
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn records_beyond_an_understated_count_are_reported_lost() {
    let file = frame(1, 1, &bodies_of(&three()));
    let lib = read_library(&file).expect("reads");
    assert_eq!(names(&lib), vec!["flat"]);
    assert_eq!(lib.report.lost, 2);
    assert!(!lib.report.is_clean());
}

#[test]
fn stray_bytes_after_the_last_record_are_reported_lost() {
    let mut file = write_library(&three()).expect("writes");
    file.extend_from_slice(&[1, 2, 3]);
    let lib = read_library(&file).expect("reads");
    assert_eq!(lib.brushes.len(), 3);
    assert_eq!(lib.report.lost, 1);
}

#[test]
fn control_characters_in_a_name_become_spaces() {
    let mut b = brush("a\nb\u{7}c\u{85}d", 1, 1, true);
    b.name = "a\nb\u{7}c\u{85}d".to_owned();
    let lib = read_library(&frame(1, 1, &bodies_of(&[b]))).expect("reads");
    assert_eq!(lib.brushes[0].name, "a b c d");
    assert!(lib.report.is_clean());
}

#[test]
fn direction_and_zero_width_characters_in_a_name_become_spaces() {
    let mut b = brush("x", 1, 1, true);
    b.name = "a\u{202E}b\u{200B}c\u{2066}d".to_owned();
    let lib = read_library(&frame(1, 1, &bodies_of(&[b]))).expect("reads");
    assert_eq!(lib.brushes[0].name, "a b c d");
}
