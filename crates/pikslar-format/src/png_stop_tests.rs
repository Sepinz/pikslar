//! A PNG being decoded asks whether to stop as it goes: while it is unpacked, unfiltered and
//! turned into pixels, each mebibyte.

use super::*;
use std::cell::Cell;

/// A 1024 by 1024 picture: four mebibytes of rows, unpacked, unfiltered and converted.
fn large() -> Vec<u8> {
    let px = (0..1024 * 1024u32)
        .map(|i| rgba(i as u8, (i >> 10) as u8, 7, 255))
        .collect();
    encode(&Image {
        width: 1024,
        height: 1024,
        px,
    })
    .expect("encodes")
}

#[test]
fn decoding_asks_whether_to_stop_each_mebibyte_of_every_stage() {
    let bytes = large();
    let asked = Cell::new(0usize);
    let count = || {
        asked.set(asked.get() + 1);
        false
    };
    let image = decode_until(&bytes, &count).expect("decodes");
    assert!(image.px == decode(&bytes).expect("decodes").px);
    // Four mebibytes unpacked, four unfiltered and four of rows converted.
    let questions = asked.get();
    assert!(questions >= 12, "asked {questions} times");
    for after in [0, questions / 2, questions - 1] {
        let made = Cell::new(0usize);
        let stop = || {
            made.set(made.get() + 1);
            made.get() > after
        };
        assert!(
            matches!(decode_until(&bytes, &stop), Err(FormatError::Stopped)),
            "after {after}"
        );
    }
}

#[test]
fn an_interlaced_picture_asks_too() {
    let mut chunks = read_chunks(&large()).expect("chunks");
    let mut header = chunks.header.take().expect("header");
    header.interlaced = true;
    let asked = Cell::new(0usize);
    let count = || {
        asked.set(asked.get() + 1);
        false
    };
    let mut stop = RowStop::new(&count);
    let expected = ADAM7
        .iter()
        .filter_map(|&p| header.pass(p))
        .map(|s| s.stream_bytes())
        .sum();
    // Every row unfiltered as it is, of filter 0.
    let mut raw = vec![0u8; expected];
    let px = decode_progressive(&mut raw, &header, &chunks, &mut stop).expect("decodes");
    assert_eq!(px.len(), 1024 * 1024);
    // Four mebibytes unfiltered and four of rows converted.
    assert!(asked.get() >= 8, "asked {} times", asked.get());
}
