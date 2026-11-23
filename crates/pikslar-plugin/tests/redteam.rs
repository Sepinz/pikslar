//! M12 red team: hostile plugin folders, junctions where Pikslar writes and removes, tampering
//! between review and approval, and text that hides what the user reads.

mod common;

use common::{junction, manifest, plugin_folder, TempDir};
use pikslar_plugin::{Capability, Change, Disabled, Manifest, PluginError, PluginStore};
use std::fs;
use std::path::{Path, PathBuf};

const ID: &str = "com.example.outline";

struct Fixture {
    t: TempDir,
    store: PluginStore,
}

impl Fixture {
    fn new(label: &str) -> Fixture {
        let t = TempDir::new(label);
        let store = PluginStore::in_dir(&t.join("plugins"));
        Fixture { t, store }
    }

    fn source(&self, name: &str, version: &str, caps: &[&str]) -> PathBuf {
        plugin_folder(&self.t.join("sources"), name, ID, version, caps)
    }

    fn install(&self, version: &str, caps: &[&str]) {
        let src = self.source(&format!("v{version}"), version, caps);
        let pending = self.store.review(&src).expect("review");
        self.store.approve(pending).expect("approve");
    }

    fn plugins(&self) -> PathBuf {
        self.t.join("plugins")
    }

    /// A folder outside the store with files that must survive anything the store does.
    fn victim(&self, name: &str) -> PathBuf {
        let dir = self.t.join(name);
        fs::create_dir_all(dir.join("inner")).expect("victim");
        fs::write(dir.join("precious.txt"), "keep").expect("victim file");
        fs::write(dir.join("inner").join("deep.txt"), "keep").expect("victim file");
        dir
    }
}

fn intact(victim: &Path) -> bool {
    victim.join("precious.txt").is_file() && victim.join("inner").join("deep.txt").is_file()
}

/// The installed folder replaced by a junction to a folder of the user's.
#[test]
fn uninstall_and_upgrade_never_remove_through_a_junction_in_the_store() {
    let f = Fixture::new("rt-junction-root");
    f.install("1.0.0", &[]);
    let installed = f.plugins().join(ID);
    fs::remove_dir_all(&installed).expect("removed");
    let victim = f.victim("victim-a");
    let windows = cfg!(windows);
    if !junction(&installed, &victim) {
        assert!(!windows, "Windows lets anyone make a junction");
        return;
    }
    assert!(matches!(f.store.load(ID), Err(Disabled::Unreadable(_))));
    let _ = f.store.uninstall(ID);
    assert!(
        intact(&victim),
        "uninstall removed files through a junction"
    );

    // Again, then an upgrade over it, with the staging folders also planted as junctions.
    let _ = fs::remove_dir(&installed);
    assert!(junction(&installed, &victim));
    let others: Vec<PathBuf> = ["new", "old", "del"]
        .iter()
        .map(|kind| {
            let v = f.victim(&format!("victim-{kind}"));
            assert!(junction(&f.plugins().join(format!(".{kind}-{ID}")), &v));
            v
        })
        .collect();
    let src = f.source("v2", "2.0.0", &["edit-document"]);
    let pending = f.store.review(&src).expect("review");
    assert!(matches!(pending.review().change, Change::Repair { .. }));
    let approved = f.store.approve(pending);
    assert!(
        intact(&victim),
        "an upgrade removed files through a junction"
    );
    for v in &others {
        assert!(
            intact(v),
            "a planted staging junction was followed: {}",
            v.display()
        );
    }
    if approved.is_ok() {
        let loaded = f.store.load(ID).expect("the upgrade loads");
        assert!(loaded.granted().contains(Capability::EditDocument));
    }
    let _ = f.store.uninstall(ID);
    assert!(intact(&victim));
    for v in &others {
        assert!(intact(v));
    }
}

/// A junction inside the installed folder, then removal both ways.
#[test]
fn removing_an_installed_copy_never_follows_a_junction_inside_it() {
    let f = Fixture::new("rt-junction-inner");
    f.install("1.0.0", &[]);
    let victim = f.victim("victim-inner");
    if !junction(&f.plugins().join(ID).join("lib"), &victim) {
        return;
    }
    // An upgrade moves the copy aside and removes it.
    f.install("1.1.0", &[]);
    assert!(intact(&victim));
    assert!(junction(&f.plugins().join(ID).join("lib"), &victim));
    f.store.uninstall(ID).expect("uninstalls");
    assert!(intact(&victim));
}

/// The source folder changes after the review: what is installed is what was reviewed.
#[test]
fn a_source_folder_changed_after_review_installs_what_was_reviewed() {
    let f = Fixture::new("rt-source-swap");
    let src = f.source("a", "1.0.0", &["read-document"]);
    let pending = f.store.review(&src).expect("review");
    fs::write(
        src.join("plugin.json"),
        manifest(ID, "1.0.0", &["read-document", "edit-document"]),
    )
    .expect("swapped");
    fs::write(src.join("outline.pks"), "print(\"swapped\");").expect("swapped");
    f.store.approve(pending).expect("approve");
    let loaded = f.store.load(ID).expect("loads");
    assert!(!loaded.granted().contains(Capability::EditDocument));
    let script = loaded.script_for("add-outline").expect("a script");
    assert!(!script.contains("swapped"), "installed bytes not reviewed");
}

/// The installed copy changes between the review of an upgrade and its approval.
#[test]
fn an_installed_copy_changed_after_review_refuses_the_approval() {
    let f = Fixture::new("rt-installed-swap");
    f.install("1.0.0", &["read-document"]);
    let src = f.source("v2", "2.0.0", &["read-document"]);
    let pending = f.store.review(&src).expect("review");
    // Another install of a version with more capabilities, then the old approval.
    f.install("1.5.0", &["read-document", "edit-document"]);
    assert!(matches!(
        f.store.approve(pending),
        Err(PluginError::ChangedSinceReview)
    ));
    // The same with the record removed (a half-finished install) and then put back.
    let pending = f.store.review(&src).expect("review");
    let record = f.plugins().join(ID).join("installed.json");
    let bytes = fs::read(&record).expect("record");
    fs::remove_file(&record).expect("removed");
    fs::write(&record, bytes).expect("put back");
    f.store
        .approve(pending)
        .expect("an identical record is the same copy");
}

/// Folders a user might be handed: names, sizes and file kinds a whitelist must refuse.
#[test]
fn hostile_package_folders_are_refused_without_reading_them_whole() {
    let f = Fixture::new("rt-hostile");
    let base = |name: &str| f.source(name, "1.0.0", &[]);
    let refused = |dir: &Path| f.store.review(dir).is_err();

    // A sparse file far larger than any limit: refused from its size, not read.
    let dir = base("huge");
    let big = fs::File::create(dir.join("outline.pks")).expect("file");
    big.set_len(1 << 30).expect("sparse length");
    drop(big);
    let t = std::time::Instant::now();
    assert!(refused(&dir));
    assert!(t.elapsed().as_secs() < 2, "the huge file was read");

    // Many files.
    let dir = base("many");
    for i in 0..2000 {
        fs::write(dir.join(format!("x{i}.pks")), "").expect("file");
    }
    assert!(refused(&dir));

    // An install record and a manifest in other cases, in the source folder.
    let dir = base("record");
    fs::write(dir.join("installed.json"), "{}").expect("file");
    assert!(refused(&dir));
    let dir = base("upper");
    fs::rename(dir.join("plugin.json"), dir.join("PLUGIN.json")).expect("renamed");
    assert!(refused(&dir));

    // Names Windows can only make through verbatim paths: trailing dots and spaces.
    #[cfg(windows)]
    for odd in ["outline.pks.", "outline.pks ", "plugin.json.", "lib.pks.."] {
        let dir = base(&format!("odd-{}", odd.len()));
        let verbatim = PathBuf::from(format!(r"\\?\{}", dir.display())).join(odd);
        if fs::write(&verbatim, "x").is_ok() {
            assert!(refused(&dir), "{odd:?} taken");
            let _ = fs::remove_file(&verbatim);
        }
    }

    // A name that is not valid Unicode (an unpaired surrogate).
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let dir = base("surrogate");
        let name = std::ffi::OsString::from_wide(&[0x61, 0xD800, 0x2E, 0x70, 0x6B, 0x73]);
        if fs::write(dir.join(&name), "x").is_ok() {
            assert!(refused(&dir));
        }
    }

    // An alternate data stream on a script: not part of the package, so not installed.
    #[cfg(windows)]
    {
        let dir = base("stream");
        if fs::write(dir.join("outline.pks:hidden"), "secret").is_ok() {
            let pending = f.store.review(&dir).expect("a stream is not a file");
            f.store.approve(pending).expect("installs");
            let installed = f.plugins().join(ID).join("outline.pks:hidden");
            assert!(fs::read(installed).is_err(), "the stream was installed");
            f.store.uninstall(ID).expect("uninstalls");
        }
    }
}

/// A manifest whose shown text could hide or reorder what the user reads.
fn with_name(name: &str) -> String {
    manifest(ID, "1.0.0", &[]).replace("\"Outline\"", &format!("\"{name}\""))
}

#[test]
fn display_text_with_the_documented_hidden_kinds_is_refused() {
    assert!(Manifest::parse(with_name("Kontur für Ränder ✓").as_bytes()).is_ok());
    for c in [
        '\u{200B}',
        '\u{202E}',
        '\u{2066}',
        '\u{FEFF}',
        '\u{E0041}',
        '\u{FE0F}',
    ] {
        let text = with_name(&format!("Out{c}line"));
        assert!(
            Manifest::parse(text.as_bytes()).is_err(),
            "U+{:04X}",
            c as u32
        );
    }
}

#[test]
fn display_text_with_other_invisible_format_characters_is_refused() {
    let missed: Vec<String> = [
        '\u{180B}',
        '\u{180C}',
        '\u{180D}',
        '\u{180F}',
        '\u{0890}',
        '\u{0891}',
        '\u{08E2}',
        '\u{110BD}',
        '\u{110CD}',
        '\u{13430}',
        '\u{1343F}',
        '\u{1BCA0}',
        '\u{1BCA3}',
    ]
    .into_iter()
    .filter(|c| Manifest::parse(with_name(&format!("Out{c}line")).as_bytes()).is_ok())
    .map(|c| format!("U+{:04X}", c as u32))
    .collect();
    assert!(missed.is_empty(), "taken: {missed:?}");
}

/// Versions at the edges of what parses, as upgrades and downgrades.
#[test]
fn versions_at_their_edges_neither_wrap_nor_panic() {
    let f = Fixture::new("rt-versions");
    for v in [
        "4294967295.4294967295.4294967295",
        "4294967296.0.0",
        "99999999999999999999.0.0",
        "01.0.0",
        "1.0",
        "1.0.0.0",
        "-1.0.0",
        "1.0.0-beta",
        "١.٠.٠",
    ] {
        let src = f.source(&format!("v{}", v.len()), v, &[]);
        let _ = f.store.review(&src).map(|p| f.store.approve(p));
    }
    if let Ok(loaded) = f.store.load(ID) {
        let src = f.source("low", "0.0.1", &[]);
        assert!(
            matches!(f.store.review(&src), Err(PluginError::Downgrade { .. })),
            "{} then 0.0.1",
            loaded.manifest().version
        );
    }
}
