//! A sequence export killed part way. Whatever moment it stops, a description is there only
//! over a whole export, the one it describes, and the next run can start at once: it takes the
//! killed run's lock over and removes the temporary files that lock says were its own.

mod common;
use common::{cli, folder, listing, project};
use pikslar_format::atomic::TEMP_SUFFIX;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const FRAMES: usize = 300;

/// When to stop the export, judged from its folder.
type Stop = &'static dyn Fn(&Path) -> bool;

fn export(project: &Path, out: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pikslar-cli"));
    c.args(["export".as_ref(), project.as_os_str(), "--format".as_ref()])
        .args(["sequence".as_ref(), "-o".as_ref(), out.as_os_str()])
        .arg("--overwrite")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

fn temps(d: &Path) -> usize {
    listing(d)
        .iter()
        .filter(|n| n.ends_with(TEMP_SUFFIX))
        .count()
}

/// The files of a sequence as a reference folder holds them.
fn same_as(d: &Path, reference: &Path) -> bool {
    listing(reference)
        .iter()
        .all(|n| fs::read(d.join(n)).ok() == fs::read(reference.join(n)).ok())
}

/// A description is either absent or sits over the whole export it describes.
fn check_invariant(d: &Path, refs: [&Path; 2], moment: &str) {
    let Ok(json) = fs::read(d.join("walk.json")) else {
        return;
    };
    let whole = refs
        .iter()
        .any(|r| fs::read(r.join("walk.json")).ok().as_ref() == Some(&json) && same_as(d, r));
    assert!(whole, "{moment}: a description over a mixed set");
}

#[test]
fn a_killed_export_never_leaves_a_mixed_set_described_as_whole() {
    let base = folder("killed");
    let (ref_a, ref_b) = (base.join("ref-a"), base.join("ref-b"));
    for d in [&ref_a, &ref_b] {
        fs::create_dir(d).expect("folder");
    }
    let a = project(&base.join("a.piks"), 160, FRAMES, 11);
    let b = project(&base.join("b.piks"), 160, FRAMES, 12);
    for (p, r) in [(&a, &ref_a), (&b, &ref_b)] {
        assert_eq!(
            export(p, &r.join("walk.png"))
                .status()
                .expect("runs")
                .code(),
            Some(0)
        );
    }
    // Moments to stop at: the first file written, many written, and the old description gone.
    let mut left_in_all = 0;
    let moments: [(&str, Stop); 3] = [
        ("one written", &|d| temps(d) >= 1),
        ("half written", &|d| temps(d) >= FRAMES / 2),
        ("renaming", &|d| !d.join("walk.json").exists()),
    ];
    for (moment, stop) in moments {
        let d = base.join(moment.replace(' ', "-"));
        fs::create_dir(&d).expect("folder");
        let out = d.join("walk.png");
        assert_eq!(export(&a, &out).status().expect("runs").code(), Some(0));
        let mut child = export(&b, &out).spawn().expect("starts");
        let started = Instant::now();
        while !stop(&d) && child.try_wait().expect("polls").is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(120),
                "{moment}: never got there"
            );
        }
        let still_running = child.try_wait().expect("polls").is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            still_running,
            "{moment}: the export finished before it could be stopped"
        );
        check_invariant(&d, [&ref_a, &ref_b], moment);
        left_in_all += temps(&d);
        let rerun = cli(&[
            &"export",
            &b,
            &"--format",
            &"sequence",
            &"-o",
            &out,
            &"--overwrite",
        ]);
        assert_eq!(
            rerun.code, 0,
            "{moment}: a retry works at once: {}",
            rerun.err
        );
        assert_eq!(temps(&d), 0, "{moment}: the killed run's files went");
        assert!(same_as(&d, &ref_b), "{moment}: the rerun finishes the job");
        assert!(
            !pikslar_format::set_lock::lock_path(&d.join("walk.json")).exists(),
            "{moment}: the killed export's lock was taken over and removed"
        );
        assert_eq!(listing(&d).len(), listing(&ref_b).len(), "{moment}");
    }
    assert!(
        left_in_all > 0,
        "some moment left temporary files to remove"
    );
    fs::remove_dir_all(base).expect("cleanup");
}
