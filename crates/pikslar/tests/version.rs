//! `pikslar.exe --version` prints one line and exits, without a window or any file.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_pikslar");

#[test]
fn version_prints_the_version_and_commit_and_exits_without_touching_anything() {
    let home = std::env::temp_dir().join(format!("pikslar-version-{}", std::process::id()));
    std::fs::create_dir_all(&home).expect("a scratch folder");
    let mut child = Command::new(EXE)
        .arg("--version")
        .env("LOCALAPPDATA", &home)
        .env("APPDATA", &home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the program starts");
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("a status") {
            break status;
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("--version did not exit: it must not open a window");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let out = child.wait_with_output().expect("the output");
    assert!(status.success());
    let text = String::from_utf8(out.stdout).expect("UTF-8");
    let commit = option_env!("PIKSLAR_COMMIT")
        .filter(|c| !c.trim().is_empty())
        .unwrap_or("unknown");
    let want = format!(
        "Pikslar {} ({})\n",
        env!("CARGO_PKG_VERSION"),
        commit.trim()
    );
    assert_eq!(text, want);
    let left: Vec<_> = std::fs::read_dir(&home).expect("the folder").collect();
    assert!(left.is_empty(), "nothing is written: {left:?}");
    let _ = std::fs::remove_dir(&home);
}
