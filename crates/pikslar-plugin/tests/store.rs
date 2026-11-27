//! Installing, upgrading, loading and uninstalling plugins in a store folder.

mod common;

use common::{everything_under, file_symlink, junction, plugin_folder, TempDir};
use pikslar_plugin::{Capability, Change, Disabled, PluginError, PluginStore};
use std::fs;
use std::path::PathBuf;

const ID: &str = "com.example.outline";

/// Something done to an installed plugin's folder behind Pikslar's back.
type Tamper = fn(&PathBuf);

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

    fn installed(&self) -> PathBuf {
        self.t.join("plugins").join(ID)
    }
}

#[test]
fn review_shows_everything_and_writes_nothing() {
    let f = Fixture::new("review");
    let src = f.source("a", "1.0.0", &["read-document", "edit-document"]);
    let pending = f.store.review(&src).expect("review");
    let r = pending.review();
    assert_eq!((r.id.as_str(), r.name.as_str()), (ID, "Outline"));
    assert_eq!(r.version.to_string(), "1.0.0");
    assert_eq!(r.api, 1);
    assert_eq!(r.change, Change::Fresh);
    assert_eq!(r.commands, ["Add outline"]);
    let caps: Vec<_> = r.capabilities.iter().map(|l| l.capability).collect();
    assert_eq!(caps, [Capability::ReadDocument, Capability::EditDocument]);
    assert!(r
        .capabilities
        .iter()
        .all(|l| !l.new && l.words.ends_with('.')));
    let lines = r.lines().join("\n");
    assert!(lines.contains("Outline 1.0.0"), "{lines}");
    assert!(
        lines.contains(Capability::EditDocument.plain_words()),
        "{lines}"
    );
    assert!(lines.contains("cannot open files"), "{lines}");
    assert_eq!(r.hash.len(), 64);
    let print = pikslar_plugin::fingerprint(&r.hash);
    assert!(
        lines.contains(&format!("Fingerprint (SHA-256 of its files): {print}")),
        "{lines}"
    );
    assert!(lines.contains(pikslar_plugin::FINGERPRINT_MEANS), "{lines}");
    assert!(
        !pikslar_plugin::FINGERPRINT_MEANS.contains("  "),
        "the words run on with single spaces"
    );
    assert!(
        lines.contains("Author: Example Studio (unverified: as the plugin states it)"),
        "{lines}"
    );
    assert!(lines.contains(&format!("Id: {ID}.")), "{lines}");
    assert!(
        !f.t.join("plugins").exists(),
        "a review must not create anything"
    );
    pending.deny();
    assert!(!f.t.join("plugins").exists());
    assert!(f.store.list().is_empty());
}

/// No review or disabled reason says the content hash defends against anyone: it detects
/// changes, and says nothing about who made the plugin.
#[test]
fn identity_wording_claims_no_protection_from_a_tamperer() {
    let f = Fixture::new("wording");
    let src = f.source("a", "1.0.0", &["read-document"]);
    let pending = f.store.review(&src).expect("review");
    let lines = pending.review().lines();
    let at = |needle: &str| lines.iter().position(|l| l.contains(needle));
    let asks = at("It asks to:").expect("capabilities");
    let print = at("Fingerprint").expect("fingerprint");
    assert!(asks < print, "capabilities come before the fingerprint");
    let words = [
        lines.join("\n"),
        pikslar_plugin::Disabled::Changed.to_string(),
    ]
    .join("\n")
    .to_lowercase();
    for claim in [
        "trust",
        "tamper",
        "secure",
        "verified by",
        "guarantee",
        "safe",
    ] {
        assert!(!words.contains(claim), "{claim:?} in {words}");
    }
    assert!(words.contains("does not prove who made the plugin"));
    let reviewed = pikslar_plugin::fingerprint(&pending.review().hash);
    f.store.approve(pending).expect("approve");
    let loaded = f.store.load(ID).map(|p| p.fingerprint());
    assert_eq!(
        loaded,
        Ok(reviewed),
        "the menu's fingerprint is the review's"
    );
}

#[test]
fn review_lines_keep_their_place_for_a_plugin_without_an_author() {
    let f = Fixture::new("noauthor");
    let src = f.source("a", "1.0.0", &["read-document"]);
    let m = fs::read_to_string(src.join("plugin.json")).expect("manifest");
    let m = m.replace(",\n  \"author\": \"Example Studio\"", "");
    fs::write(src.join("plugin.json"), &m).expect("write");
    let pending = f.store.review(&src).expect("review");
    assert!(pending.review().author.is_none(), "{m}");
    let lines = pending.review().lines();
    assert!(lines.iter().any(|l| l == "Author: not stated"), "{lines:?}");
    assert!(
        !f.t.join("plugins").exists(),
        "a review must not create anything"
    );
    pending.deny();
    assert!(!f.t.join("plugins").exists());
    assert!(f.store.list().is_empty());
}

#[test]
fn approve_installs_exactly_what_was_reviewed() {
    let f = Fixture::new("approve");
    let src = f.source("a", "1.0.0", &["read-document"]);
    let pending = f.store.review(&src).expect("review");
    // The source changes after the review: what was reviewed is what is installed.
    fs::write(src.join("outline.pks"), "let evil = 1\n").expect("edit");
    f.store.approve(pending).expect("approve");
    let loaded = f.store.load(ID).expect("load");
    assert!(loaded
        .script_for("add-outline")
        .is_some_and(|s| s.contains("let x = 1")));
    assert!(loaded.granted().contains(Capability::ReadDocument));
    assert!(!loaded.granted().contains(Capability::EditDocument));
    assert_eq!(loaded.manifest().version.to_string(), "1.0.0");
    let listed = f.store.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, ID);
    assert!(listed[0].state.is_ok());
}

#[test]
fn upgrade_rules() {
    let f = Fixture::new("upgrade");
    f.install("1.0.0", &["read-document"]);

    let newer = f.source("b", "1.1.0", &["read-document", "edit-document"]);
    let pending = f.store.review(&newer).expect("upgrade review");
    let r = pending.review();
    assert_eq!(
        r.change,
        Change::Upgrade {
            from: "1.0.0".parse_version(),
            previous_name: "Outline".to_string(),
            previous_author: Some("Example Studio".to_string()),
        }
    );
    assert!(!r.lines().join("\n").contains("Warning"));
    let new: Vec<_> = r
        .capabilities
        .iter()
        .filter(|l| l.new)
        .map(|l| l.capability)
        .collect();
    assert_eq!(new, [Capability::EditDocument]);
    assert!(r.lines().join("\n").contains("(new in this version)"));
    f.store.approve(pending).expect("upgrade");
    assert!(f
        .store
        .load(ID)
        .expect("load")
        .granted()
        .contains(Capability::EditDocument));

    let older = f.source("c", "1.0.5", &[]);
    assert!(matches!(
        f.store.review(&older),
        Err(PluginError::Downgrade { .. })
    ));

    let same_changed = f.source("d", "1.1.0", &["read-document"]);
    assert!(matches!(
        f.store.review(&same_changed),
        Err(PluginError::SameVersionChanged { .. })
    ));

    let same = f.store.review(&newer).expect("same files again");
    assert_eq!(same.review().change, Change::Reinstall);
    f.store.approve(same).expect("reinstall");
    assert_eq!(f.store.list().len(), 1);
    assert_no_leftovers(&f);
}

#[test]
fn an_approval_is_refused_when_the_store_changed_since_review() {
    let f = Fixture::new("stale");
    let a = f
        .store
        .review(&f.source("a", "1.0.0", &[]))
        .expect("review a");
    let b = f
        .store
        .review(&f.source("b", "2.0.0", &[]))
        .expect("review b");
    f.store.approve(b).expect("approve b");
    assert_eq!(
        f.store.approve(a).err(),
        Some(PluginError::ChangedSinceReview)
    );
    assert_eq!(
        f.store
            .load(ID)
            .expect("load")
            .manifest()
            .version
            .to_string(),
        "2.0.0"
    );
}

#[test]
fn tampering_disables_and_never_half_loads() {
    let cases: [(&str, Tamper); 6] = [
        ("edited script", |d| {
            fs::write(d.join("outline.pks"), "let changed = 1\n").expect("edit")
        }),
        ("edited manifest", |d| {
            let m = fs::read_to_string(d.join("plugin.json")).expect("read");
            fs::write(
                d.join("plugin.json"),
                m.replace("\"read-document\"", "\"edit-document\""),
            )
            .expect("edit")
        }),
        ("added script", |d| {
            fs::write(d.join("extra.pks"), "let x = 1\n").expect("add")
        }),
        ("added other file", |d| {
            fs::write(d.join("payload.dll"), "MZ").expect("add")
        }),
        ("removed script", |d| {
            fs::remove_file(d.join("outline.pks")).expect("remove")
        }),
        ("removed record", |d| {
            fs::remove_file(d.join("installed.json")).expect("remove")
        }),
    ];
    for (what, tamper) in cases {
        let f = Fixture::new("tamper");
        f.install("1.0.0", &["read-document"]);
        tamper(&f.installed());
        let reason = match f.store.load(ID) {
            Ok(_) => panic!("{what}: loaded after tampering"),
            Err(reason) => reason,
        };
        assert!(!reason.to_string().is_empty(), "{what}");
        let listed = f.store.list();
        assert_eq!(listed.len(), 1, "{what}");
        assert!(listed[0].state.is_err(), "{what}: listed as enabled");
        // Installing the right files again repairs it.
        let src = f.source("again", "1.0.0", &["read-document"]);
        let pending = f.store.review(&src).expect("repair review");
        assert!(
            matches!(pending.review().change, Change::Repair { .. }),
            "{what}"
        );
        f.store.approve(pending).expect("repair");
        assert!(f.store.load(ID).is_ok(), "{what}: not repaired");
    }
}

#[test]
fn a_changed_record_or_a_tampered_capability_grant_is_caught() {
    let f = Fixture::new("record");
    f.install("1.0.0", &["read-document"]);
    let record = f.installed().join("installed.json");
    let text = fs::read_to_string(&record).expect("record");
    // Granting itself more in the record: not the record the person approved.
    fs::write(
        &record,
        text.replace("\"read-document\"", "\"read-document\",\"edit-document\""),
    )
    .expect("edit");
    assert_eq!(f.store.load(ID).err(), Some(Disabled::NotApproved));
    fs::write(&record, "{not json").expect("edit");
    assert_eq!(f.store.load(ID).err(), Some(Disabled::NotApproved));
    fs::write(&record, text.replace("1.0.0", "9.0.0")).expect("edit");
    assert_eq!(f.store.load(ID).err(), Some(Disabled::NotApproved));
    fs::write(&record, &text).expect("put back");
    assert!(f.store.load(ID).is_ok(), "the approved record loads again");
}

/// Review finding S1 (RC3): a plugin folder put in place by hand, with a record that matches
/// its files exactly, was loaded with the permissions it lists, never shown to anyone. Only a
/// record approved through the install review loads.
#[test]
fn a_plugin_folder_placed_by_hand_never_loads_without_the_install_review() {
    let f = Fixture::new("by-hand");
    f.install("1.0.0", &["read-document", "edit-document"]);
    assert!(f.store.load(ID).is_ok());
    // The same folder, record and all, copied into another Pikslar's plugins folder.
    let other = Fixture::new("by-hand-other");
    let into = other.store.folder().expect("a store").join(ID);
    fs::create_dir_all(&into).expect("dir");
    for entry in fs::read_dir(f.installed()).expect("listed").flatten() {
        fs::copy(entry.path(), into.join(entry.file_name())).expect("copied");
    }
    assert_eq!(other.store.load(ID).err(), Some(Disabled::NotApproved));
    let listed = other.store.list();
    assert!(
        listed.iter().all(|l| l.state.is_err()),
        "never listed as enabled"
    );
    // Reviewing and approving it makes it the person's own.
    let src = other.source("reviewed", "1.0.0", &["read-document", "edit-document"]);
    let pending = other.store.review(&src).expect("review");
    assert!(matches!(pending.review().change, Change::Repair { .. }));
    other.store.approve(pending).expect("approved");
    assert!(other.store.load(ID).is_ok());
    // Uninstalling forgets the approval: the same folder placed again is not approved.
    other.store.uninstall(ID).expect("uninstall");
    fs::create_dir_all(&into).expect("dir");
    for entry in fs::read_dir(f.installed()).expect("listed").flatten() {
        fs::copy(entry.path(), into.join(entry.file_name())).expect("copied");
    }
    assert_eq!(other.store.load(ID).err(), Some(Disabled::NotApproved));
}

#[test]
fn a_junction_planted_in_an_installed_plugin_disables_it() {
    let f = Fixture::new("plant");
    f.install("1.0.0", &[]);
    let outside = f.t.join("outside");
    fs::create_dir(&outside).expect("dir");
    if junction(&f.installed().join("lib.pks"), &outside) || cfg!(windows) {
        assert!(f.store.load(ID).is_err());
        f.store.uninstall(ID).expect("uninstall");
        assert!(outside.exists(), "uninstall must not follow the junction");
    }
}

#[test]
fn uninstall_leaves_nothing() {
    let f = Fixture::new("uninstall");
    f.install("1.0.0", &["edit-document"]);
    f.install("1.2.0", &["edit-document"]);
    f.store.uninstall(ID).expect("uninstall");
    assert_eq!(everything_under(&f.t.join("plugins")), Vec::<String>::new());
    assert!(f.store.list().is_empty());
    assert_eq!(
        f.store.uninstall(ID).err(),
        Some(PluginError::NotInstalled(ID.to_string()))
    );
    assert!(matches!(f.store.load(ID), Err(Disabled::NotInstalled)));
    assert!(matches!(
        f.store.load("../evil"),
        Err(Disabled::NotInstalled)
    ));
    assert!(f.store.uninstall("..").is_err());
}

#[test]
fn a_store_that_is_off_installs_nothing() {
    let t = TempDir::new("off");
    let store = PluginStore::disabled();
    let src = plugin_folder(t.path(), "a", ID, "1.0.0", &[]);
    assert_eq!(store.review(&src).err(), Some(PluginError::NoStore));
    assert!(store.list().is_empty());
    assert_eq!(
        PluginStore::for_settings_dir(None).review(&src).err(),
        Some(PluginError::NoStore)
    );
    let on = PluginStore::for_settings_dir(Some(t.path()));
    on.approve(on.review(&src).expect("review"))
        .expect("approve");
    assert!(t.join("plugins").join(ID).join("plugin.json").exists());
}

#[test]
fn stray_folders_in_the_store_are_listed_as_disabled_or_ignored() {
    let f = Fixture::new("stray");
    f.install("1.0.0", &[]);
    let root = f.t.join("plugins");
    fs::create_dir(root.join("Not An Id")).expect("dir");
    fs::create_dir(root.join(".new-com.example.half")).expect("dir");
    fs::write(root.join("notes.txt"), "x").expect("file");
    let listed = f.store.list();
    let ids: Vec<_> = listed.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["Not An Id", ID]);
    assert!(listed[0].state.is_err());
}

fn assert_no_leftovers(f: &Fixture) {
    let root = f.t.join("plugins");
    let names: Vec<String> = fs::read_dir(&root)
        .expect("store")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [ID], "staging or old copies were left");
}

trait ParseVersion {
    fn parse_version(&self) -> pikslar_plugin::Version;
}

impl ParseVersion for &str {
    fn parse_version(&self) -> pikslar_plugin::Version {
        pikslar_plugin::Version::parse(self).expect("version")
    }
}

#[test]
fn an_upgrade_from_someone_else_is_shown_as_a_warning() {
    let f = Fixture::new("hijack");
    f.install("1.0.0", &[]);
    let src = f.source("other", "5.0.0", &["edit-document"]);
    let m = fs::read_to_string(src.join("plugin.json")).expect("manifest");
    let m = m
        .replace("\"Outline\"", "\"Free Brushes\"")
        .replace("Example Studio", "Someone");
    fs::write(src.join("plugin.json"), m).expect("write");
    let pending = f.store.review(&src).expect("review");
    let lines = pending.review().lines().join("\n");
    assert!(lines.contains("Warning"), "{lines}");
    assert!(lines.contains("\"Outline\" by Example Studio"), "{lines}");
    assert!(lines.contains("(new in this version)"), "{lines}");
}

#[test]
fn a_review_for_one_store_cannot_be_approved_in_another() {
    let f = Fixture::new("two");
    let other = PluginStore::in_dir(&f.t.join("other"));
    let pending = f
        .store
        .review(&f.source("a", "1.0.0", &[]))
        .expect("review");
    assert_eq!(
        other.approve(pending).err(),
        Some(PluginError::ChangedSinceReview)
    );
    assert!(!f.t.join("other").exists());
}

#[test]
fn a_record_swapped_for_a_link_after_review_refuses_the_approval() {
    let f = Fixture::new("recordlink");
    f.install("1.0.0", &[]);
    let pending = f
        .store
        .review(&f.source("b", "2.0.0", &[]))
        .expect("review");
    let record = f.installed().join(pikslar_plugin::RECORD_FILE);
    let copy = f.t.join("record-copy.json");
    fs::copy(&record, &copy).expect("copied");
    fs::remove_file(&record).expect("removed");
    if !file_symlink(&record, &copy) {
        eprintln!("file links are not allowed here; skipped");
        return;
    }
    assert_eq!(
        f.store.approve(pending).err(),
        Some(PluginError::ChangedSinceReview),
        "the same bytes through a link are not the record that was reviewed"
    );
}

#[test]
fn uninstall_clears_what_an_unfinished_uninstall_left() {
    let f = Fixture::new("leftover");
    let root = f.t.join("plugins");
    for side in [".del-", ".old-", ".new-"] {
        let dir = root.join(format!("{side}{ID}"));
        fs::create_dir_all(&dir).expect("dir");
        fs::write(dir.join("outline.pks"), "x").expect("file");
    }
    f.store.uninstall(ID).expect("the leftovers are cleared");
    assert_eq!(everything_under(&root), Vec::<String>::new());
    assert_eq!(
        f.store.uninstall(ID).err(),
        Some(PluginError::NotInstalled(ID.to_string()))
    );
}

#[test]
fn a_record_planted_as_a_junction_after_review_refuses_the_approval() {
    let f = Fixture::new("recordjunction");
    f.install("1.0.0", &[]);
    let record = f.installed().join(pikslar_plugin::RECORD_FILE);
    fs::remove_file(&record).expect("removed");
    let pending = f
        .store
        .review(&f.source("b", "2.0.0", &[]))
        .expect("review of a repair");
    let outside = f.t.join("outside");
    fs::create_dir(&outside).expect("dir");
    if !junction(&record, &outside) {
        return;
    }
    assert_eq!(
        f.store.approve(pending).err(),
        Some(PluginError::ChangedSinceReview),
        "a link where the record was is a change"
    );
}

/// An upgrade that cannot be put in place (a file of the installed copy held open, as an
/// editor or a scanner might) leaves the copy from before loading, approval and all.
#[cfg(windows)]
#[test]
fn an_upgrade_that_fails_part_way_keeps_the_reviewed_copy_loading() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new("upgrade-fails");
    f.install("1.0.0", &["read-document"]);
    let newer = f.source("b", "1.1.0", &["read-document"]);
    let pending = f.store.review(&newer).expect("upgrade review");
    let held_name = fs::read_dir(f.installed())
        .expect("listed")
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file())
        .expect("a file");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&held_name)
        .expect("held");
    assert!(f.store.approve(pending).is_err(), "the swap is refused");
    drop(held);
    let loaded = f.store.load(ID).expect("the reviewed copy still loads");
    assert_eq!(loaded.manifest().version, "1.0.0".parse_version());
}

/// A plugin an earlier build installed, with no approval noted, is repaired by reviewing a copy
/// of its installed files, without their install record: no source package is needed.
#[test]
fn a_plugin_without_an_approval_is_repaired_from_its_own_folder() {
    let f = Fixture::new("no-approval");
    f.install("1.0.0", &["read-document"]);
    let approvals = f.t.join("plugin-approvals.txt");
    fs::remove_file(&approvals).expect("as an earlier build left it");
    assert_eq!(f.store.load(ID).err(), Some(Disabled::NotApproved));
    let copy = f.t.join("copied-out");
    fs::create_dir_all(&copy).expect("dir");
    // Its files, without the record an install writes.
    for entry in fs::read_dir(f.installed()).expect("listed").flatten() {
        if entry.file_name() != "installed.json" {
            fs::copy(entry.path(), copy.join(entry.file_name())).expect("copied");
        }
    }
    let pending = f.store.review(&copy).expect("its own files review");
    f.store.approve(pending).expect("approved");
    assert!(f.store.load(ID).is_ok());
}
