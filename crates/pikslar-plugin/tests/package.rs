//! Reading plugin folders, good and hostile.

mod common;

use common::{file_symlink, junction, plugin_folder, TempDir};
use pikslar_plugin::{Package, PluginError, MOST_FILES, MOST_SCRIPT_BYTES};
use std::fs;

const ID: &str = "com.example.outline";

fn refused(dir: &std::path::Path) -> String {
    match Package::read_folder(dir) {
        Ok(p) => panic!("taken: {:?}", p.manifest()),
        Err(e) => e.to_string(),
    }
}

#[test]
fn a_good_folder_is_read_and_hashed() {
    let t = TempDir::new("good");
    let dir = plugin_folder(t.path(), "p", ID, "1.0.0", &["read-document"]);
    let p = Package::read_folder(&dir).expect("good package");
    assert_eq!(p.manifest().id, ID);
    assert_eq!(p.files().len(), 2);
    assert!(p
        .script_for("add-outline")
        .is_some_and(|s| s.contains("let x")));
    let again = Package::read_folder(&dir).expect("again");
    assert_eq!(p.hash(), again.hash());
    fs::write(dir.join("outline.pks"), "let x = 2\n").expect("edit");
    assert_ne!(Package::read_folder(&dir).expect("edited").hash(), p.hash());
}

#[test]
fn extra_missing_and_unused_files_are_refused() {
    let t = TempDir::new("extra");
    let dir = plugin_folder(t.path(), "p", ID, "1.0.0", &[]);
    fs::write(dir.join("readme.txt"), "hi").expect("write");
    assert!(refused(&dir).contains("readme.txt"));
    fs::remove_file(dir.join("readme.txt")).expect("remove");

    fs::write(dir.join("spare.pks"), "let y = 1").expect("write");
    assert!(refused(&dir).contains("not run by any command"));
    fs::remove_file(dir.join("spare.pks")).expect("remove");

    fs::remove_file(dir.join("outline.pks")).expect("remove");
    assert!(refused(&dir).contains("not in the folder"));

    let empty = t.join("empty");
    fs::create_dir(&empty).expect("dir");
    assert!(refused(&empty).contains("no plugin.json"));
}

#[test]
fn names_that_differ_by_case_or_look_like_devices_are_refused() {
    let t = TempDir::new("case");
    let dir = plugin_folder(t.path(), "p", ID, "1.0.0", &[]);
    fs::remove_file(dir.join("outline.pks")).expect("remove");
    fs::write(dir.join("Outline.pks"), "let x = 1").expect("write");
    // Only lower-case names are allowed, so two names can never differ only by case.
    assert!(refused(&dir).contains("\"Outline.pks\" is not allowed"));

    let dir = plugin_folder(t.path(), "q", ID, "1.0.0", &[]);
    fs::rename(dir.join("plugin.json"), dir.join("PLUGIN.JSON")).expect("rename");
    assert!(refused(&dir).contains("PLUGIN.JSON"));
}

#[test]
fn folders_inside_are_refused() {
    let t = TempDir::new("nested");
    let dir = plugin_folder(t.path(), "p", ID, "1.0.0", &[]);
    fs::create_dir(dir.join("lib")).expect("dir");
    assert!(refused(&dir).contains("lib"));
    fs::remove_dir(dir.join("lib")).expect("remove");
    fs::create_dir(dir.join("lib.pks")).expect("dir named like a script");
    assert!(refused(&dir).contains("not a plain file"));
}

#[test]
fn too_many_files_and_too_many_bytes_are_refused() {
    let t = TempDir::new("many");
    let dir = plugin_folder(t.path(), "p", ID, "1.0.0", &[]);
    for i in 0..MOST_FILES {
        fs::write(dir.join(format!("s{i}.pks")), "x").expect("write");
    }
    assert!(refused(&dir).contains("more than 16 files"));

    let dir = plugin_folder(t.path(), "q", ID, "1.0.0", &[]);
    let big = vec![b'#'; usize::try_from(MOST_SCRIPT_BYTES).expect("fits") + 1];
    fs::write(dir.join("outline.pks"), big).expect("write");
    assert!(refused(&dir).contains("larger than"));

    let dir = plugin_folder(t.path(), "r", ID, "1.0.0", &[]);
    let huge_manifest = format!(
        "{}{}",
        fs::read_to_string(dir.join("plugin.json")).expect("m"),
        " ".repeat(20_000)
    );
    fs::write(dir.join("plugin.json"), huge_manifest).expect("write");
    assert!(refused(&dir).contains("bytes"));
}

#[test]
fn a_package_too_large_in_total_is_refused() {
    let t = TempDir::new("total");
    let dir = t.join("p");
    fs::create_dir(&dir).expect("dir");
    let commands: Vec<String> = (0..5)
        .map(|i| format!(r#"{{"id":"c{i}","title":"C{i}","script":"c{i}.pks"}}"#))
        .collect();
    let manifest = format!(
        r#"{{"manifest":1,"id":"{ID}","name":"Big","version":"1.0.0","api":1,
        "capabilities":[],"commands":[{}]}}"#,
        commands.join(",")
    );
    fs::write(dir.join("plugin.json"), manifest).expect("manifest");
    let chunk = vec![b'#'; 250 * 1024];
    for i in 0..5 {
        fs::write(dir.join(format!("c{i}.pks")), &chunk).expect("write");
    }
    assert!(refused(&dir).contains("together"));
}

#[test]
fn scripts_must_be_text() {
    let t = TempDir::new("binary");
    let dir = plugin_folder(t.path(), "p", ID, "1.0.0", &[]);
    fs::write(dir.join("outline.pks"), [0xFF, 0xFE, 0x00]).expect("write");
    assert!(refused(&dir).contains("UTF-8"));
}

#[test]
fn links_and_junctions_are_refused() {
    let t = TempDir::new("links");
    let outside = t.join("outside");
    fs::create_dir(&outside).expect("dir");
    fs::write(outside.join("secret.pks"), "let s = 1").expect("write");

    let dir = plugin_folder(t.path(), "p", ID, "1.0.0", &[]);
    // Windows lets anyone make a junction, so these cases always run there.
    let windows = cfg!(windows);
    if junction(&dir.join("inner"), &outside) || windows {
        assert!(refused(&dir).contains("inner"));
        fs::remove_dir(dir.join("inner")).expect("remove junction");
    }
    // A junction with a script's name: refused for what it is, not for its name.
    if junction(&dir.join("lib.pks"), &outside) || windows {
        assert!(refused(&dir).contains("lib.pks is a link or junction"));
        fs::remove_dir(dir.join("lib.pks")).expect("remove junction");
    }
    let linked = plugin_folder(t.path(), "real", ID, "1.0.0", &[]);
    if junction(&t.join("via-junction"), &linked) || windows {
        assert!(refused(&t.join("via-junction")).contains("link or junction"));
        fs::remove_dir(t.join("via-junction")).expect("remove junction");
    }
    fs::remove_file(dir.join("outline.pks")).expect("remove");
    if file_symlink(&dir.join("outline.pks"), &outside.join("secret.pks")) {
        assert!(refused(&dir).contains("link or junction"));
    } else {
        eprintln!("file symbolic links are not allowed here; that case was not run");
    }
}

#[test]
fn a_missing_folder_or_a_file_is_refused() {
    let t = TempDir::new("missing");
    assert!(matches!(
        Package::read_folder(&t.join("nothing")),
        Err(PluginError::Io(_))
    ));
    fs::write(t.join("file"), "x").expect("write");
    assert!(refused(&t.join("file")).contains("not a folder"));
}
