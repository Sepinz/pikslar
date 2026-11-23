use super::*;
use std::process::{Command, Stdio};
use std::time::{Duration, UNIX_EPOCH};

const DIR_VAR: &str = "PIKSLAR_CRASH_NOTE_DIR";

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-crashnote-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

fn notes(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|r| {
            r.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(CRASH_PREFIX))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn a_note_is_short_and_names_no_folder_and_no_quoted_text() {
    let at = UNIX_EPOCH + Duration::from_secs(1_795_000_000);
    let message = format!(
        r#"bad layer "Secret layer" in C:\Users\Someone\Private\walk.piks {}"#,
        "x".repeat(2000)
    );
    let text = note_text(at, &message, r"C:\build\crates\pikslar\src\app.rs", 12);
    assert!(text.contains("2026-11-18T11:06:40Z"));
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
    assert!(text.contains("walk.piks"), "{text}");
    assert!(text.contains("location: app.rs:12"), "{text}");
    for secret in ["Secret layer", "Someone", "Private", "build"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    let said = text
        .lines()
        .find(|l| l.starts_with("message: "))
        .expect("message");
    assert!(
        said.chars().count() <= "message: ".len() + MOST_MESSAGE,
        "{said}"
    );
    assert_eq!(note_name(at, 42), "crash-20261118T110640Z-42.txt");
}

#[test]
fn only_the_newest_five_notes_are_kept() {
    let dir = temp_dir("prune");
    for i in 0..8 {
        fs::write(dir.join(format!("crash-2026010{i}T000000Z-1.txt")), b"x").expect("note");
    }
    fs::write(dir.join(SEEN_NAME), b"crash-x").expect("seen");
    prune(&dir);
    let kept = notes(&dir);
    assert_eq!(kept.len(), MOST_NOTES);
    assert_eq!(kept[0], "crash-20260103T000000Z-1.txt", "the oldest went");
    assert!(dir.join(SEEN_NAME).exists());
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn a_new_note_is_mentioned_once() {
    let dir = temp_dir("seen");
    assert_eq!(take_unseen(&dir), None, "none yet");
    fs::write(dir.join("crash-20260101T000000Z-1.txt"), b"x").expect("note");
    assert_eq!(
        take_unseen(&dir).as_deref(),
        Some("crash-20260101T000000Z-1.txt")
    );
    assert_eq!(take_unseen(&dir), None, "already mentioned");
    fs::write(dir.join("crash-20260102T000000Z-1.txt"), b"x").expect("note");
    assert!(take_unseen(&dir).is_some(), "a newer one is");
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
fn an_app_mentions_a_crash_note_on_the_status_line_once() {
    let dir = temp_dir("app");
    fs::write(dir.join("crash-20260101T000000Z-1.txt"), b"x").expect("note");
    let mut a = crate::app::App::launch(None, Some(8), None);
    a.mention_crash(&dir);
    let said = a
        .message
        .as_ref()
        .map(|(t, _)| t.clone())
        .unwrap_or_default();
    assert!(said.contains("crash note"), "{said}");
    let mut b = crate::app::App::launch(None, Some(8), None);
    b.mention_crash(&dir);
    assert!(b.message.is_none(), "said once");
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
#[ignore = "run by a_panic_leaves_a_crash_note"]
fn child_panics() {
    let Some(dir) = std::env::var_os(DIR_VAR).map(PathBuf::from) else {
        return;
    };
    crate::diag::start(Some(dir), true);
    install();
    panic!(r#"boom in C:\Users\Someone\Hidden Folder\walk.piks with "secret words""#);
}

#[test]
fn a_panic_leaves_a_crash_note() {
    let dir = temp_dir("child");
    let status = Command::new(std::env::current_exe().expect("test program"))
        .args(["--exact", "diag_crash::tests::child_panics", "--ignored"])
        .env(DIR_VAR, &dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("child runs");
    assert!(!status.success(), "the child panicked");
    let crash = dir.join(CRASH_DIR);
    let names = notes(&crash);
    assert_eq!(names.len(), 1, "{names:?}");
    let text = fs::read_to_string(crash.join(&names[0])).expect("note");
    assert!(
        text.contains("message: boom in walk.piks with \"...\""),
        "{text}"
    );
    assert!(text.contains("location: diag_crash_tests.rs:"), "{text}");
    for secret in ["Someone", "Hidden", "secret words"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    fs::remove_dir_all(&dir).expect("cleaned");
}

#[test]
#[ignore = "run by a_persisted_off_keeps_no_note_even_at_launch"]
fn child_panics_with_diagnostics_off() {
    let Some(dir) = std::env::var_os(DIR_VAR).map(PathBuf::from) else {
        return;
    };
    // As `main` does: the setting is read before diagnostics start.
    crate::diag_ui::launch_diagnostics(Some(dir));
    install();
    panic!("boom during launch");
}

#[test]
fn a_persisted_off_keeps_no_note_even_at_launch() {
    let dir = temp_dir("off-at-launch");
    fs::write(dir.join("settings.txt"), "diagnostics = no\r\n").expect("settings");
    let status = Command::new(std::env::current_exe().expect("test program"))
        .args([
            "--exact",
            "diag_crash::tests::child_panics_with_diagnostics_off",
            "--ignored",
        ])
        .env(DIR_VAR, &dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("child runs");
    assert!(!status.success(), "the child panicked");
    assert!(
        notes(&dir.join(CRASH_DIR)).is_empty(),
        "no note was written"
    );
    assert!(!dir.join(crate::diag::LOG_DIR).exists(), "no log either");
    assert!(crate::diag_ui::persisted_on(&temp_dir("no-settings")));
    fs::remove_dir_all(&dir).expect("cleaned");
}
