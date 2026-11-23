//! Saving over a project an older version made (L7): the same publish as any save
//! (`ReplaceFileW`, which keeps the file's streams), and the same refusal when another program
//! changed the file since it was last looked at. The version backup is kept either way.

use crate::files;
use crate::outside::Seen;
use pikslar_format::piks;
use pikslar_format::upgrade::backup_path;
use std::fs;
use std::path::PathBuf;

const OLDER: u32 = piks::WRITTEN_VERSION - 1;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pikslar-upsave-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).expect("dir");
    d
}

/// A project header of the older version, then `tail`.
fn older(tail: &[u8]) -> Vec<u8> {
    let mut bytes = b"PIKS".to_vec();
    bytes.extend_from_slice(&OLDER.to_le_bytes());
    bytes.extend_from_slice(tail);
    bytes
}

fn doc() -> pikslar_core::Document {
    pikslar_core::Document::new(4, 4, pikslar_core::ColorMode::Rgba).expect("doc")
}

#[test]
fn an_upgrade_save_keeps_the_projects_streams_and_its_version_backup() {
    let d = dir("streams");
    let art = d.join("art.piks");
    fs::write(&art, older(b"old art")).expect("older project");
    let stream = d.join("art.piks:note");
    fs::write(&stream, "kept").expect("a named stream");
    let expect = Seen::of(&art, None);
    files::save_upgrading(&art, OLDER, &doc(), expect).expect("upgraded");
    assert_eq!(
        fs::read(&stream).expect("the stream"),
        b"kept",
        "replaced the system's way"
    );
    assert_eq!(
        fs::read(backup_path(&art, OLDER)).expect("version backup"),
        older(b"old art")
    );
    assert!(!pikslar_format::publish::backup_path(&art).exists());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn an_upgrade_save_refuses_a_project_another_program_changed_since_it_was_seen() {
    let d = dir("changed");
    let art = d.join("art.piks");
    fs::write(&art, older(b"old art")).expect("older project");
    let expect = Seen::of(&art, Some(pikslar_rewind::hash128(&older(b"old art"))));
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&art, older(b"another program's art, longer")).expect("changed outside");
    let result = files::save_upgrading(&art, OLDER, &doc(), expect);
    assert!(result.is_err(), "refused");
    assert_eq!(
        fs::read(&art).expect("left"),
        older(b"another program's art, longer"),
        "left as the other program made it"
    );
    let _ = fs::remove_dir_all(&d);
}
