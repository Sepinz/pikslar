//! The bytes every frozen project exports to, recorded before export left the app, in `4286db5`.
//!
//! Each line of `export_golden.txt` is a corpus file, then a hash of each frame exported as PNG,
//! then the sheet's PNG and its JSON. A difference means export changed what it writes.
//! `PIKSLAR_BLESS_EXPORTS=1` rewrites the file; that is only right when the change is intended.

use crate::{export_frame, export_sheet};
use pikslar_format::piks;
use std::path::{Path, PathBuf};

const GOLDEN: &str = "src/export_golden.txt";

/// FNV-1a, 64 bits. Stable across Rust versions, unlike the standard hasher.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../pikslar-safety/corpus")
}

fn project_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(corpus())
        .expect("corpus")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".piks"))
        .collect();
    names.sort();
    names
}

fn hashes_for(name: &str, out: &Path) -> Vec<u64> {
    let bytes = std::fs::read(corpus().join(name)).expect("read");
    let doc = piks::load(&bytes).expect("loads");
    let mut hashes = Vec::new();
    for frame in 0..doc.frames.len() {
        let path = out.join("frame.png");
        export_frame(&path, &doc, frame).expect("frame exports");
        hashes.push(fnv(&std::fs::read(&path).expect("frame")));
    }
    let sheet = out.join("golden.png");
    assert!(export_sheet(&sheet, &doc).expect("sheet exports"));
    hashes.push(fnv(&std::fs::read(&sheet).expect("sheet")));
    hashes.push(fnv(&std::fs::read(out.join("golden.json")).expect("json")));
    hashes
}

fn lines() -> String {
    let out = std::env::temp_dir().join(format!("pikslar-golden-{}", std::process::id()));
    std::fs::create_dir_all(&out).expect("temp dir");
    let mut text = String::new();
    for name in project_names() {
        let hashes: Vec<String> = hashes_for(&name, &out)
            .iter()
            .map(|h| format!("{h:016x}"))
            .collect();
        text.push_str(&format!("{name} {}\n", hashes.join(",")));
    }
    std::fs::remove_dir_all(out).expect("cleanup");
    text
}

#[test]
fn every_frozen_project_exports_the_recorded_bytes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    let now = lines();
    if std::env::var_os("PIKSLAR_BLESS_EXPORTS").is_some() {
        std::fs::write(&path, &now).expect("write golden");
        return;
    }
    let recorded = std::fs::read_to_string(&path).expect("golden file");
    assert_eq!(now.lines().count(), 23, "every corpus project is covered");
    for (got, want) in now.lines().zip(recorded.lines()) {
        assert_eq!(got, want, "export bytes changed");
    }
    assert_eq!(now.lines().count(), recorded.lines().count());
}

#[test]
fn the_hash_is_fnv1a() {
    assert_eq!(fnv(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv(b"a"), 0xaf63_dc4c_8601_ec8c);
}
