use super::*;
use std::ffi::OsString;
use std::time::{Duration, Instant};

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-frame-log-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn times(total: u32) -> FrameTimes {
    FrameTimes {
        total_us: total,
        handled_us: 1,
        frame_us: 2,
        blit_us: 3,
    }
}

#[test]
fn is_off_unless_the_variable_names_a_file() {
    assert_eq!(path_from(None), None);
    assert_eq!(path_from(Some(OsString::new())), None);
    assert_eq!(
        path_from(Some(OsString::from("C:/x/frames.txt"))),
        Some(PathBuf::from("C:/x/frames.txt"))
    );
    assert_eq!(VAR, "PIKSLAR_FRAME_LOG");
}

#[test]
fn input_is_keys_and_the_pointer_only() {
    for msg in [0x100, 0x101, 0x102, 0x105, 0x200, 0x201, 0x20A] {
        assert!(is_input(msg), "{msg:#x}");
    }
    for msg in [0x0000, 0x000F, 0x0005, 0x0400, 0x02A3] {
        assert!(!is_input(msg), "{msg:#x}");
    }
}

#[test]
fn splits_a_frame_into_its_parts_in_microseconds() {
    let input = Instant::now();
    let handled = input + Duration::from_micros(150);
    let framed = handled + Duration::from_micros(2_400);
    let shown = framed + Duration::from_micros(90);
    let t = FrameTimes::between(input, handled, framed, shown);
    assert_eq!(
        t,
        FrameTimes {
            total_us: 2_640,
            handled_us: 150,
            frame_us: 2_400,
            blit_us: 90,
        }
    );
}

#[test]
fn writes_a_header_then_one_line_per_frame() {
    let mut log = FrameLog::new(temp_file("unused.txt"));
    log.lines.push(times(2_640));
    log.lines.push(times(75));
    let mut out = Vec::new();
    log.write_to(&mut out).expect("write");
    let text = String::from_utf8(out).expect("utf-8");
    assert_eq!(
        text,
        "# total_us handled_us frame_us blit_us\n2640 1 2 3\n75 1 2 3\n"
    );
    assert!(log.lines.is_empty());
    // The header is written once per run, not once per batch.
    log.lines.push(times(9));
    let mut more = Vec::new();
    log.write_to(&mut more).expect("write");
    assert_eq!(String::from_utf8(more).expect("utf-8"), "9 1 2 3\n");
}

#[test]
fn keeps_frames_in_memory_until_the_buffer_is_full() {
    let path = temp_file("buffered.txt");
    let mut log = FrameLog::new(path.clone());
    let reserved = log.lines.capacity();
    assert!(reserved >= CAPACITY);
    for i in 0..CAPACITY - 1 {
        log.record(times(i as u32)).expect("record");
    }
    assert!(!path.exists(), "nothing is written before the buffer fills");
    log.record(times(7)).expect("record");
    let text = std::fs::read_to_string(&path).expect("written when full");
    assert_eq!(text.lines().count(), CAPACITY + 1);
    assert!(log.lines.is_empty());
    assert_eq!(
        log.lines.capacity(),
        reserved,
        "the buffer is reused, not reallocated"
    );
}

#[test]
fn writes_what_is_left_when_dropped_and_appends_across_runs() {
    let path = temp_file("runs.txt");
    {
        let mut log = FrameLog::new(path.clone());
        log.record(times(10)).expect("record");
        log.record(times(20)).expect("record");
    }
    {
        let mut log = FrameLog::new(path.clone());
        log.record(times(30)).expect("record");
    }
    let text = std::fs::read_to_string(&path).expect("written on drop");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        [
            "# total_us handled_us frame_us blit_us",
            "10 1 2 3",
            "20 1 2 3",
            "# total_us handled_us frame_us blit_us",
            "30 1 2 3",
        ]
    );
}

#[test]
fn a_run_with_no_frames_writes_nothing() {
    let path = temp_file("empty.txt");
    drop(FrameLog::new(path.clone()));
    assert!(!path.exists());
}

#[test]
fn a_file_that_cannot_be_written_is_an_error() {
    let dir = temp_file("a-folder");
    std::fs::create_dir_all(&dir).expect("dir");
    let mut log = FrameLog::new(dir);
    log.lines.push(times(1));
    assert!(log.flush().is_err());
}
