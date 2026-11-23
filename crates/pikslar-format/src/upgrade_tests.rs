//! Upgrading an older project: every failure leaves it byte-identical.

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

const NEW: &[u8] = b"the new project";
const OLDER: u32 = piks::WRITTEN_VERSION - 1;

fn older_bytes(tail: &[u8]) -> Vec<u8> {
    let mut bytes = piks::MAGIC.to_vec();
    bytes.extend_from_slice(&OLDER.to_le_bytes());
    bytes.extend_from_slice(tail);
    bytes
}

/// A fresh folder holding an older project named `a.piks`.
fn folder() -> (PathBuf, PathBuf) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pikslar-upgrade-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("a.piks");
    fs::write(&path, older_bytes(b"old art")).expect("project");
    (dir, path)
}

fn names(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(dir)
        .expect("listed")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

#[test]
fn only_older_projects_are_older() {
    let (dir, path) = folder();
    assert_eq!(older_version(&path), Some(OLDER));
    let cases: [&[u8]; 4] = [b"PIKS", b"not a project", &[], b"PIKS\0\0\0\0"];
    for bytes in cases {
        fs::write(&path, bytes).expect("written");
        assert_eq!(older_version(&path), None, "{bytes:?}");
    }
    let mut current = piks::MAGIC.to_vec();
    current.extend_from_slice(&piks::WRITTEN_VERSION.to_le_bytes());
    fs::write(&path, current).expect("written");
    assert_eq!(older_version(&path), None);
    assert_eq!(older_version(&dir.join("missing.piks")), None);
    fs::remove_dir_all(&dir).expect("clean up");
}

#[test]
fn the_backup_is_named_for_the_version() {
    let name = backup_path(Path::new("art/a.piks"), 2);
    assert_eq!(name, Path::new("art/a.piks.v2.bak"));
}

#[test]
fn upgrading_keeps_the_original_beside_the_new_project() {
    let (dir, path) = folder();
    upgrade(&path, OLDER, NEW).expect("upgraded");
    assert_eq!(fs::read(&path).expect("project"), NEW);
    let backup = fs::read(backup_path(&path, OLDER)).expect("backup");
    assert_eq!(backup, older_bytes(b"old art"));
    assert_eq!(names(&dir), ["a.piks", &format!("a.piks.v{OLDER}.bak")]);
    fs::remove_dir_all(&dir).expect("clean up");
}

#[test]
fn a_backup_holding_the_original_is_reused_and_anything_else_is_kept() {
    let (dir, path) = folder();
    let backup = backup_path(&path, OLDER);
    fs::write(&backup, older_bytes(b"old art")).expect("same backup");
    assert!(!backup_in_the_way(&path, OLDER));
    upgrade(&path, OLDER, NEW).expect("upgraded");
    let (dir2, path2) = folder();
    let backup2 = backup_path(&path2, OLDER);
    fs::write(&backup2, b"something else").expect("other file");
    assert!(backup_in_the_way(&path2, OLDER));
    assert!(matches!(
        upgrade(&path2, OLDER, NEW),
        Err(UpgradeError::BackupInTheWay(_))
    ));
    assert_eq!(fs::read(&path2).expect("project"), older_bytes(b"old art"));
    assert_eq!(fs::read(&backup2).expect("kept"), b"something else");
    fs::remove_dir_all(&dir).expect("clean up");
    fs::remove_dir_all(&dir2).expect("clean up");
}

#[test]
fn a_folder_with_the_backup_name_is_in_the_way() {
    let (dir, path) = folder();
    fs::create_dir(backup_path(&path, OLDER)).expect("folder");
    assert!(backup_in_the_way(&path, OLDER));
    assert!(upgrade(&path, OLDER, NEW).is_err());
    assert_eq!(fs::read(&path).expect("project"), older_bytes(b"old art"));
    fs::remove_dir_all(&dir).expect("clean up");
}

fn every_step() -> Vec<UpgradeStep> {
    let mut steps = vec![UpgradeStep::Read, UpgradeStep::BackedUp];
    steps.extend(SaveStep::ALL.map(UpgradeStep::Save));
    steps
}

#[test]
fn a_failure_at_any_step_leaves_the_project_byte_identical() {
    for failing in every_step() {
        let (dir, path) = folder();
        let result = upgrade_checked(&path, OLDER, NEW, &mut |step| {
            if step == failing {
                Err(io::Error::other("injected"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err(), "{failing:?}");
        assert_eq!(fs::read(&path).expect("project"), older_bytes(b"old art"));
        let backup = backup_path(&path, OLDER);
        if let Ok(kept) = fs::read(&backup) {
            assert_eq!(kept, older_bytes(b"old art"), "{failing:?}");
        }
        let left = names(&dir);
        assert!(
            left.iter().all(|n| !n.ends_with(TEMP_SUFFIX)),
            "{failing:?} {left:?}"
        );
        // A backup left behind holds the original, so trying again reuses it.
        upgrade(&path, OLDER, NEW).expect("a second try works");
        assert_eq!(fs::read(&path).expect("project"), NEW);
        fs::remove_dir_all(&dir).expect("clean up");
    }
}

#[test]
fn a_project_saved_elsewhere_meanwhile_is_not_replaced() {
    for during in [UpgradeStep::BackedUp, UpgradeStep::Save(SaveStep::Synced)] {
        let (dir, path) = folder();
        let newer = older_bytes(b"newer art from another window");
        let result = upgrade_checked(&path, OLDER, NEW, &mut |step| {
            if step == during {
                fs::write(&path, &newer)?;
            }
            Ok(())
        });
        assert!(matches!(result, Err(UpgradeError::Changed)), "{during:?}");
        assert_eq!(fs::read(&path).expect("project"), newer);
        fs::remove_dir_all(&dir).expect("clean up");
    }
}

#[test]
fn a_project_that_is_no_longer_older_is_not_upgraded() {
    let (dir, path) = folder();
    fs::write(&path, b"not a project").expect("replaced");
    assert!(matches!(
        upgrade(&path, OLDER, NEW),
        Err(UpgradeError::Changed)
    ));
    assert_eq!(fs::read(&path).expect("kept"), b"not a project");
    fs::remove_dir_all(&dir).expect("clean up");
}

#[test]
fn a_read_only_project_is_left_alone() {
    let (dir, path) = folder();
    let mut permissions = fs::metadata(&path).expect("found").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions.clone()).expect("read-only");
    assert!(upgrade(&path, OLDER, NEW).is_err());
    assert_eq!(fs::read(&path).expect("project"), older_bytes(b"old art"));
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).expect("writable");
    fs::remove_dir_all(&dir).expect("clean up");
}

#[test]
fn messages_name_the_backup_and_say_the_project_was_kept() {
    let clash = UpgradeError::BackupInTheWay(PathBuf::from("art/a.piks.v2.bak"));
    assert!(clash
        .to_string()
        .starts_with("a.piks.v2.bak already exists"));
    assert!(UpgradeError::Changed.to_string().contains("left as it was"));
}

#[test]
fn a_link_where_the_backup_goes_is_never_trusted_as_the_backup() {
    let (dir, path) = folder();
    let backup = backup_path(&path, OLDER);
    // Making links needs Developer Mode or an administrator, so this is skipped without them.
    if std::os::windows::fs::symlink_file(&path, &backup).is_err() {
        fs::remove_dir_all(&dir).expect("clean up");
        return;
    }
    let before = fs::read(&path).expect("project");
    assert!(backup_in_the_way(&path, OLDER));
    let result = upgrade(&path, OLDER, NEW);
    assert!(
        matches!(result, Err(UpgradeError::BackupInTheWay(_))),
        "{result:?}"
    );
    assert_eq!(fs::read(&path).expect("project"), before);
    fs::remove_dir_all(&dir).expect("clean up");
}

#[test]
fn a_stale_temporary_copy_never_writes_through_to_another_file() {
    let (dir, path) = folder();
    let other = dir.join("other.piks");
    fs::write(&other, b"someone else's work").expect("other");
    let mut temp = backup_path(&path, OLDER).into_os_string();
    temp.push(TEMP_SUFFIX);
    // Left by a kill between linking the backup in and removing its temporary name.
    fs::hard_link(&other, PathBuf::from(temp)).expect("link");
    upgrade(&path, OLDER, NEW).expect("upgraded");
    assert_eq!(fs::read(&other).expect("other"), b"someone else's work");
    assert_eq!(fs::read(&path).expect("project"), NEW);
    fs::remove_dir_all(&dir).expect("clean up");
}
