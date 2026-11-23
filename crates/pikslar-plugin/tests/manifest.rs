//! The manifest reader against good manifests and a corpus of malformed ones.

use pikslar_plugin::{Capability, Manifest, Version};

const GOOD: &str = r#"{
  "manifest": 1,
  "id": "com.example.outline",
  "name": "Outline",
  "version": "1.2.0",
  "api": 1,
  "capabilities": ["read-document", "edit-document"],
  "commands": [
    {"id": "add-outline", "title": "Add outline", "script": "outline.pks"},
    {"id": "thin", "title": "Thin outline", "script": "thin.pks"}
  ],
  "description": "Draws a one-pixel outline around the selection.",
  "author": "Example Studio"
}"#;

fn refused(text: &str) -> String {
    match Manifest::parse(text.as_bytes()) {
        Ok(m) => panic!("taken as {m:?}:\n{text}"),
        Err(e) => e.to_string(),
    }
}

/// `GOOD` with one member's text replaced.
fn with(from: &str, to: &str) -> String {
    assert!(GOOD.contains(from), "{from}");
    GOOD.replacen(from, to, 1)
}

#[test]
fn a_good_manifest_is_read_whole() {
    let m = Manifest::parse(GOOD.as_bytes()).expect("good manifest");
    assert_eq!(m.id, "com.example.outline");
    assert_eq!(m.name, "Outline");
    assert_eq!(
        m.version,
        Version {
            major: 1,
            minor: 2,
            patch: 0
        }
    );
    assert_eq!(m.api, 1);
    assert_eq!(
        m.capabilities.iter().collect::<Vec<_>>(),
        [Capability::ReadDocument, Capability::EditDocument]
    );
    assert_eq!(m.commands.len(), 2);
    assert_eq!(
        m.command("thin").map(|c| c.script.as_str()),
        Some("thin.pks")
    );
    assert_eq!(m.author.as_deref(), Some("Example Studio"));
}

#[test]
fn optional_members_may_be_left_out_and_capabilities_may_be_empty() {
    let text = r#"{"manifest":1,"id":"a.b","name":"X","version":"0.0.1","api":1,
        "capabilities":[],"commands":[{"id":"go","title":"Go","script":"go.pks"}]}"#;
    let m = Manifest::parse(text.as_bytes()).expect("minimal manifest");
    assert!(m.capabilities.is_empty());
    assert_eq!(m.description, None);
}

#[test]
fn versions_compare_as_numbers() {
    let v = |t: &str| Version::parse(t).expect(t);
    assert!(v("1.10.0") > v("1.9.9"));
    assert!(v("2.0.0") > v("1.99999.99999"));
    assert_eq!(v("0.0.0").to_string(), "0.0.0");
    for bad in [
        "",
        "1",
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.2.-3",
        "+1.2.3",
        "1.2.3-beta",
        "1.2.3+b",
        " 1.2.3",
        "1.2.3 ",
        "1..3",
        "100000.0.0",
        "1.2.x",
        "１.2.3",
    ] {
        assert!(Version::parse(bad).is_err(), "{bad:?} taken");
    }
}

#[test]
fn structural_problems_are_refused() {
    let corpus: Vec<(String, &str)> = vec![
        (String::new(), "byte 0"),
        ("[]".into(), "must be an object"),
        ("{}".into(), "\"manifest\" is missing"),
        (format!("\u{FEFF}{GOOD}"), "byte order mark"),
        (format!("{GOOD} {{}}"), "after the value"),
        (with("\"author\"", "\"Author\""), "does not know"),
        (with("\"author\"", "\"script\""), "does not know"),
        (with("\"api\": 1,", "\"api\": 1, \"api\": 1,"), "twice"),
        (
            with("\"api\": 1,", "\"extra\": true, \"api\": 1,"),
            "does not know",
        ),
        (with("\"manifest\": 1", "\"manifest\": 2"), "format 2"),
        (
            with("\"manifest\": 1", "\"manifest\": \"1\""),
            "whole number",
        ),
        (with("\"api\": 1", "\"api\": 2"), "scripting API 2"),
        (with("\"api\": 1", "\"api\": 0"), "scripting API 0"),
        (with("\"api\": 1", "\"api\": -1"), "scripting API -1"),
        (with("\"api\": 1", "\"api\": 1.0"), "byte"),
        (with("\"api\": 1", "\"api\": 4294967297"), "scripting API"),
        (
            with("\"version\": \"1.2.0\"", "\"version\": 1"),
            "must be a string",
        ),
        (with("1.2.0", "1.2"), "three whole numbers"),
        (with("\"name\": \"Outline\",", ""), "\"name\" is missing"),
        (
            with("\"name\": \"Outline\"", "\"name\": null"),
            "must be a string",
        ),
        (
            with("\"description\": \"Draws", "\"description\": [\"Draws"),
            "byte",
        ),
    ];
    for (text, expected) in corpus {
        let why = refused(&text);
        assert!(
            why.contains(expected),
            "{why:?} lacks {expected:?} for\n{text}"
        );
    }
}

#[test]
fn bad_fields_are_refused() {
    let corpus = [
        (with("com.example.outline", "../../evil"), "id part"),
        (with("com.example.outline", "Com.Example"), "id part"),
        (with("com.example.outline", "con.example"), "device"),
        (with("com.example.outline", "outline"), "2 to 6 parts"),
        (with("\"Outline\"", "\"\""), "name is empty"),
        (with("\"Outline\"", "\" Outline\""), "space"),
        (with("\"Outline\"", "\"Out\\u202Eline\""), "U+202E"),
        (with("\"Outline\"", "\"Out\\nline\""), "U+000A"),
        (
            with("\"Outline\"", &format!("\"{}\"", "x".repeat(49))),
            "longer",
        ),
        (with("Example Studio", "Ex\\u200Bample"), "U+200B"),
    ];
    for (text, expected) in &corpus {
        let why = refused(text);
        assert!(why.contains(expected), "{why:?} lacks {expected:?}");
    }
    let long_description = with(
        "Draws a one-pixel outline around the selection.",
        &"d".repeat(401),
    );
    assert!(refused(&long_description).contains("longer"));
}

#[test]
fn capabilities_are_a_closed_list() {
    let corpus = [
        (
            "\"read-document\", \"edit-document\"",
            "\"network\"",
            "not one Pikslar knows",
        ),
        (
            "\"read-document\", \"edit-document\"",
            "\"files\"",
            "not one Pikslar knows",
        ),
        (
            "\"read-document\", \"edit-document\"",
            "\"READ-DOCUMENT\"",
            "not one Pikslar knows",
        ),
        (
            "\"read-document\", \"edit-document\"",
            "\"read-document\", \"read-document\"",
            "twice",
        ),
        (
            "\"read-document\", \"edit-document\"",
            "7",
            "must be a word",
        ),
        (
            "[\"read-document\", \"edit-document\"]",
            "\"read-document\"",
            "list of words",
        ),
        (
            "[\"read-document\", \"edit-document\"]",
            "null",
            "list of words",
        ),
    ];
    for (from, to, expected) in corpus {
        let why = refused(&with(from, to));
        assert!(why.contains(expected), "{why:?} lacks {expected:?}");
    }
}

#[test]
fn commands_are_checked_one_by_one() {
    let corpus = [
        ("\"outline.pks\"", "\"../outline.pks\"", "not allowed"),
        ("\"outline.pks\"", "\"C:\\\\x.pks\"", "not allowed"),
        ("\"outline.pks\"", "\"outline.pks:ads\"", "not allowed"),
        ("\"outline.pks\"", "\"nul.pks\"", "device"),
        ("\"outline.pks\"", "\"Outline.pks\"", "not allowed"),
        ("\"add-outline\"", "\"thin\"", "used twice"),
        ("\"Add outline\"", "\"Thin outline\"", "used twice"),
        ("\"add-outline\"", "\"Add\"", "command id"),
        ("\"Add outline\"", "\"\"", "empty"),
        ("\"title\"", "\"label\"", "does not know"),
    ];
    for (from, to, expected) in corpus {
        let why = refused(&with(from, to));
        assert!(why.contains(expected), "{why:?} lacks {expected:?}");
    }
    let empty = r#"{"manifest":1,"id":"a.b","name":"X","version":"0.0.1","api":1,
        "capabilities":[],"commands":[]}"#;
    assert!(refused(empty).contains("1 to 16"));
    let many: Vec<String> = (0..17)
        .map(|i| format!("{{\"id\":\"c{i}\",\"title\":\"C{i}\",\"script\":\"c{i}.pks\"}}"))
        .collect();
    let too_many = format!(
        r#"{{"manifest":1,"id":"a.b","name":"X","version":"0.0.1","api":1,
        "capabilities":[],"commands":[{}]}}"#,
        many.join(",")
    );
    let why = refused(&too_many);
    assert!(why.contains("16"), "{why}");
}

#[test]
fn size_and_nesting_limits_hold() {
    let huge = format!("{GOOD}{}", " ".repeat(16 * 1024));
    assert!(refused(&huge).contains("bytes"));
    let deep = with(
        "\"author\": \"Example Studio\"",
        "\"author\": [[[[\"x\"]]]]",
    );
    assert!(refused(&deep).contains("byte"));
    let long_string = with("Example Studio", &"a".repeat(600));
    assert!(refused(&long_string).contains("byte"));
    assert!(Manifest::parse(&[0xFF, 0xFE]).is_err());
}
