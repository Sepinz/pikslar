use super::*;
use std::fs;
use std::time::{Duration, Instant, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-diag-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn a_line_has_the_time_the_version_and_only_what_was_given() {
    let at = UNIX_EPOCH + Duration::from_secs(1_795_000_000);
    let e = Event::new("save", "fail")
        .phase("write")
        .code(Some(112))
        .file(Path::new(
            r"C:\Users\Someone\Private Folder\walk cycle.piks",
        ));
    let line = line_at(at, &e);
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        line,
        format!("2026-11-18T11:06:40Z {version} save fail phase=write code=112 file=walk_cycle.piks\r\n")
    );
    let plain = line_at(at, &Event::new("start", "ok"));
    assert_eq!(
        plain,
        format!("2026-11-18T11:06:40Z {version} start ok\r\n")
    );
    let timed = Event::new("save", "ok").took(Instant::now() - Duration::from_millis(41));
    assert!(
        line_at(at, &timed).contains(" ms=41"),
        "{}",
        line_at(at, &timed)
    );
}

#[test]
fn utc_time_reads_as_a_calendar_date() {
    assert_eq!(utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    // A leap day, and the end of a leap year.
    assert_eq!(
        utc(UNIX_EPOCH + Duration::from_secs(951_782_400)),
        "2000-02-29T00:00:00Z"
    );
    assert_eq!(
        utc(UNIX_EPOCH + Duration::from_secs(1_735_689_599)),
        "2024-12-31T23:59:59Z"
    );
}

#[test]
fn a_file_is_named_by_its_base_name_only() {
    assert_eq!(
        base_name(Path::new(r"C:\Users\Jack\art\walk.piks")),
        "walk.piks"
    );
    assert_eq!(base_name(Path::new("/home/x/y.png")), "y.png");
    assert_eq!(base_name(Path::new(r"C:\")), "-");
    assert_eq!(base_name(Path::new("odd\nname\t.piks")), "odd_name_.piks");
    let long = "a".repeat(200) + ".piks";
    assert!(base_name(Path::new(&long)).chars().count() <= MOST_NAME);
}

#[test]
fn paths_in_free_text_are_cut_to_their_base_names() {
    let said = reduce_paths(r"Could not open C:\Users\Jack Smith\Secret\art.piks: denied");
    assert_eq!(said, "Could not open art.piks: denied");
    assert_eq!(reduce_paths("/home/someone/x.rs:12"), "x.rs:12");
    assert_eq!(
        reduce_paths(r#"bad name "layer of Jack" in \\server\share\a.piks"#),
        r#"bad name "..." in a.piks"#
    );
    assert_eq!(reduce_paths("no path here"), "no path here");
}

#[test]
fn the_log_never_passes_its_bound_and_keeps_one_older_file() {
    let dir = temp_dir("rotate");
    let mut log = Log::at(dir.clone());
    let line = format!("{}\r\n", "x".repeat(98));
    for _ in 0..2000 {
        log.append(&line);
        let size = fs::metadata(dir.join(LOG_NAME)).map_or(0, |m| m.len());
        assert!(size <= MOST_BYTES, "{size}");
    }
    let older = fs::metadata(dir.join(OLDER_NAME)).expect("rotated").len();
    assert!(older <= MOST_BYTES && older > MOST_BYTES - 200, "{older}");
    let names: Vec<_> = fs::read_dir(&dir).expect("dir").flatten().collect();
    assert_eq!(names.len(), 2, "log.txt and log.1.txt, nothing else");
    assert!(log.problem().is_none());
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn an_unusable_folder_turns_the_log_off_quietly_and_says_why() {
    let dir = temp_dir("blocked");
    fs::create_dir_all(&dir).expect("dir");
    // A file where the log folder would be.
    let blocked = dir.join("log");
    fs::write(&blocked, b"in the way").expect("file");
    let mut log = Log::at(blocked.clone());
    log.append("one\r\n");
    log.append("two\r\n");
    assert!(log.problem().is_some(), "the reason is kept");
    assert_eq!(read(&blocked), "in the way", "nothing written over it");
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn a_disabled_log_writes_nothing() {
    let dir = temp_dir("off");
    {
        let _capture = Capture::to(dir.clone(), false);
        note(Event::new("save", "ok").file(Path::new("a.piks")));
    }
    assert!(!dir.exists(), "no folder, no file");
    {
        let _capture = Capture::to(dir.clone(), true);
        note(Event::new("save", "ok").file(Path::new("a.piks")));
    }
    assert!(read(&dir.join(LOG_NAME)).contains("save ok file=a.piks"));
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn deleting_removes_the_log_and_the_notes() {
    let dir = temp_dir("delete");
    let mut log = Log::at(dir.join("log"));
    log.append("one\r\n");
    fs::create_dir_all(dir.join("crash")).expect("dir");
    fs::write(dir.join("crash").join("crash-1.txt"), b"x").expect("note");
    fs::write(dir.join("keep.txt"), b"not ours").expect("other");
    drop(log);
    delete_all(&dir).expect("deleted");
    assert!(!dir.join("log").exists());
    assert!(!dir.join("crash").exists());
    assert!(dir.join("keep.txt").exists(), "nothing else is touched");
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn save_failures_name_their_phase_and_code() {
    use crate::files::FileError;
    use pikslar_format::FormatError;
    let io = FileError::Format(FormatError::Io(std::io::Error::from_raw_os_error(112)));
    assert_eq!(save_failure(&io), ("write", Some(112)));
    assert_eq!(save_failure(&FileError::Unverified), ("verify", None));
}

/// The cost of one line: run by hand with `--ignored --nocapture`.
#[test]
#[ignore = "a measurement"]
fn append_cost() {
    let dir = temp_dir("cost");
    let _capture = Capture::to(dir.clone(), true);
    let path = Path::new(r"C:\Users\Someone\art\walk.piks");
    let mut times = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let started = Instant::now();
        note(Event::new("save", "ok").took(started).file(path));
        times.push(started.elapsed());
    }
    times.sort();
    println!(
        "append: median {:?}, p99 {:?}, max {:?}",
        times[times.len() / 2],
        times[times.len() * 99 / 100],
        times[times.len() - 1]
    );
    drop(_capture);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_session_with_a_script_logs_events_but_no_script_text_names_or_folders() {
    use pikslar_platform::Window;
    let dir = temp_dir("session");
    let folder = dir.join("SECRET_FOLDER of Someone");
    fs::create_dir_all(&folder).expect("dir");
    let log = dir.join("log");
    let capture = Capture::to(log.clone(), true);
    let mut a = crate::app::App::launch(None, Some(8), None);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let out = a.run_script(
        r#"add_layer("SECRET_LAYER_NAME");"#,
        pikslar_bind::Capabilities::ALL,
        &cancel,
    );
    assert!(out.result.is_ok(), "{:?}", out.result);
    let path = folder.join("walk.piks");
    assert!(a.save_to(Window::detached(), path.clone()));
    let mut b = crate::app::App::launch(None, Some(8), None);
    let opened = b.open_now(Window::detached(), path);
    assert_eq!(opened, crate::session::Opened::Now);
    drop(capture);
    let text = read(&log.join(LOG_NAME));
    for want in [
        "save start file=walk.piks",
        "save ok ms=",
        "open ok file=walk.piks",
    ] {
        assert!(text.contains(want), "{want} in {text}");
    }
    for secret in ["SECRET", "Someone", "add_layer", "pikslar-diag"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn a_folder_or_a_name_without_an_extension_is_never_kept() {
    assert_eq!(base_name(Path::new(r"C:\Users\Someone")), "...");
    assert_eq!(base_name(Path::new(r"C:\art\.hidden")), "...");
    assert_eq!(base_name(Path::new(r"C:\art\walk.piks")), "walk.piks");
    assert_eq!(reduce_paths(r"C:\Users\name: denied"), "...: denied");
    assert_eq!(
        reduce_paths(r"open C:\Users\Ann (Work), Ltd\Sub,dir\walk.piks (os error 5)"),
        "open walk.piks (os error 5)"
    );
    assert_eq!(
        reduce_paths(r"in C:\Users\Ann (Work)\Private Folder: denied"),
        "in ...: denied"
    );
    let said = reduce_paths(r"copy C:\a (b)\x.piks to D:\c, d\y.piks failed");
    assert!(!said.contains('(') && !said.contains("c, d"), "{said}");
}

#[test]
fn the_users_name_and_profile_are_replaced_wherever_they_appear() {
    use crate::diag_redact::{identity_words, scrub_words, USER};
    let words = identity_words(
        Some(r"C:\Users\jack.SMITH".to_string()),
        Some("Jack".to_string()),
    );
    assert_eq!(words[0], r"C:\Users\jack.SMITH", "longest first");
    let said = scrub_words(r"jack.smith-art.piks C:\USERS\JACK.SMITH\x JACK", &words);
    assert!(!said.to_ascii_lowercase().contains("jack"), "{said}");
    assert!(said.contains(USER), "{said}");
    assert!(
        identity_words(None, Some("a".to_string())).is_empty(),
        "too short"
    );
}

#[test]
fn recording_never_waits_for_a_writer_that_is_stuck() {
    use crate::diag_writer::{Sink, Writer, QUEUE};
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::{Arc, Mutex};
    struct Stuck(Arc<Mutex<Receiver<()>>>);
    impl Sink for Stuck {
        fn line(&mut self, _: &str) {
            // Blocks until the test lets it go, as a disk that does not answer would.
            let _ = self.0.lock().expect("lock").recv();
        }
        fn dropped(&mut self, _: u64) {}
        fn delete(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn problem(&self) -> Option<String> {
            None
        }
    }
    let (release, wait) = channel();
    let writer = Writer::spawn(Stuck(Arc::new(Mutex::new(wait)))).expect("thread");
    let started = Instant::now();
    let mut slowest = Duration::ZERO;
    for n in 0..QUEUE * 4 {
        let one = Instant::now();
        writer.send(format!("line {n}"));
        slowest = slowest.max(one.elapsed());
    }
    assert!(slowest < Duration::from_millis(50), "{slowest:?}");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(writer.dropped() > 0, "a full queue drops and counts");
    assert!(
        !writer.flush(Duration::from_millis(50)),
        "a flush waits a bounded time"
    );
    assert!(
        writer.delete(Duration::from_millis(50)).is_err(),
        "so does a delete"
    );
    drop(release);
}

#[test]
fn a_failed_save_logs_the_step_it_failed_at() {
    use crate::files::{failed_step, save_traced};
    use pikslar_format::atomic::SaveStep;
    assert_eq!(failed_step(None), "create");
    assert_eq!(failed_step(Some(SaveStep::HalfWritten)), "write");
    assert_eq!(failed_step(Some(SaveStep::Written)), "flush");
    assert_eq!(failed_step(Some(SaveStep::Synced)), "read-back");
    assert_eq!(failed_step(Some(SaveStep::Verified)), "publish");
    let dir = temp_dir("step");
    fs::create_dir_all(&dir).expect("dir");
    let doc = pikslar_core::Document::new(4, 4, pikslar_core::ColorMode::Rgba).expect("doc");
    // A folder at the target's name: everything is written, and putting it in place fails.
    let target = dir.join("in the way.piks");
    fs::create_dir_all(&target).expect("folder");
    let failed = save_traced(&target, &doc).expect_err("cannot replace a folder");
    assert_eq!(failed.phase, "publish");
    let log = dir.join("log");
    {
        let _capture = Capture::to(log.clone(), true);
        note(Event::new("save", "fail").failed_at(&failed).file(&target));
    }
    assert!(read(&log.join(LOG_NAME)).contains("phase=publish"));
    fs::remove_dir_all(&dir).expect("cleaned");
}
