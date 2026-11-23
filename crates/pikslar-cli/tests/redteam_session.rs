//! M12 red team: a session's folder attacked through junctions, device names and short names,
//! and JSON lines the existing corpus does not have. Every line gets one reply, the session
//! goes on, and nothing outside the folder is read or written.

mod common;
use common::{cli_input, folder, listing, project};
use std::fs;
use std::path::Path;

const HELLO: &str = r#"{"id":0,"type":"hello","api":1}"#;
const OPEN: &str = r#"{"id":1,"type":"open","path":"walk.piks"}"#;

/// Runs one session in `dir` and gives its replies, one per line sent.
fn talk(dir: &Path, lines: &[String]) -> Vec<String> {
    let mut input = lines.join("\n").into_bytes();
    input.push(b'\n');
    let run = cli_input(&[&"session", &"--dir", &dir], &input);
    assert_eq!(run.code, 0, "{}", run.err);
    let replies: Vec<String> = run.out.lines().map(String::from).collect();
    assert_eq!(replies.len(), lines.len(), "{}", run.out);
    replies
}

fn ok(reply: &str) -> bool {
    reply.contains(r#""ok":true"#)
}

/// A session folder and, beside it, a folder of the user's with a project that must not be
/// read or changed.
fn folders(label: &str) -> (std::path::PathBuf, std::path::PathBuf, Vec<u8>) {
    let base = folder(label);
    let dir = base.join("session");
    let away = base.join("away");
    fs::create_dir_all(&dir).expect("session folder");
    fs::create_dir_all(&away).expect("away folder");
    project(&dir.join("walk.piks"), 8, 2, 3);
    let victim = project(&away.join("victim.piks"), 8, 2, 4);
    let bytes = fs::read(victim).expect("victim");
    (dir, away, bytes)
}

fn junction(link: &Path, target: &Path) -> bool {
    std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn junctions_inside_the_folder_reach_nothing_outside() {
    if !cfg!(windows) {
        return;
    }
    let (dir, away, victim) = folders("rt-junctions");
    assert!(junction(&dir.join("j"), &away));
    // A junction back to the folder itself stays inside.
    assert!(junction(&dir.join("self"), &dir));
    let mut lines = vec![HELLO.to_string()];
    for given in ["j/victim.piks", "j\\\\victim.piks", "self/j/victim.piks"] {
        lines.push(format!(r#"{{"id":2,"type":"open","path":"{given}"}}"#));
    }
    lines.push(OPEN.to_string());
    lines.push(r#"{"id":3,"type":"run","ops":[{"op":"add_frame","at":0}]}"#.to_string());
    for given in [
        "j/victim.piks",
        "j/new.piks",
        "self/j/new.piks",
        "j",
        "self/../away/new.piks",
    ] {
        lines.push(format!(
            r#"{{"id":4,"type":"save","path":"{given}","overwrite":true}}"#
        ));
    }
    lines.push(r#"{"id":5,"type":"save","path":"self/inside.piks"}"#.to_string());
    let replies = talk(&dir, &lines);
    for (line, reply) in lines.iter().zip(&replies).skip(1).take(3) {
        assert!(!ok(reply), "{line} -> {reply}");
    }
    assert!(ok(&replies[4]) && ok(&replies[5]), "{replies:?}");
    for (line, reply) in lines.iter().zip(&replies).skip(6).take(5) {
        assert!(!ok(reply), "{line} -> {reply}");
    }
    assert!(ok(&replies[11]), "{}", replies[11]);
    assert_eq!(fs::read(away.join("victim.piks")).expect("victim"), victim);
    assert_eq!(listing(&away), ["victim.piks"]);
    assert_eq!(listing(&dir), ["inside.piks", "j", "self", "walk.piks"]);
}

/// Names that are devices, streams or other spellings of a file. A save may refuse, or write
/// a plain file of that name inside the folder; it must never write anywhere else.
#[test]
fn device_names_and_other_spellings_write_only_plain_files_inside() {
    let (dir, away, victim) = folders("rt-devices");
    let names = [
        "conin$",
        "CONOUT$",
        "conin$.piks",
        "com¹.piks",
        "LPT³",
        "nul.piks",
        "con .piks",
        "aux.tar.piks",
        "walk.piks::$DATA",
        "walk.piks:$I30:$INDEX_ALLOCATION",
        "WALKPI~1.PIK",
        "walk.piks\u{200B}",
        "\u{202E}skip.piks",
        "a\\u001bb.piks",
    ];
    let mut lines = vec![HELLO.to_string(), OPEN.to_string()];
    for name in names {
        lines.push(format!(r#"{{"id":2,"type":"save","path":"{name}"}}"#));
        lines.push(format!(r#"{{"id":3,"type":"open","path":"{name}"}}"#));
        lines.push(r#"{"id":4,"type":"close","discard":true}"#.to_string());
        lines.push(OPEN.to_string());
    }
    let replies = talk(&dir, &lines);
    let after = listing(&dir);
    for (i, name) in names.iter().enumerate() {
        let save = &replies[2 + i * 4];
        if ok(save) {
            let unescaped = name.replace("\\u001b", "\u{1b}");
            assert!(
                after.iter().any(|f| f.eq_ignore_ascii_case(&unescaped)),
                "saving {name:?} said yes but wrote no file of that name: {after:?}"
            );
        }
        println!("{name:?}: {save}");
    }
    assert_eq!(fs::read(away.join("victim.piks")).expect("victim"), victim);
    assert_eq!(listing(&away), ["victim.piks"]);
}

/// JSON the existing corpus does not have: keys that are duplicates only after unescaping,
/// overlong and surrogate UTF-8, numbers of any length, ids at their limits and nesting at
/// the limit's edge.
#[test]
fn more_malformed_lines_each_get_one_refusal() {
    let (dir, _, _) = folders("rt-json");
    let before = fs::read(dir.join("walk.piks")).expect("project");
    let digits = "9".repeat(100_000);
    let nest = |n: usize| format!("{}1{}", "[".repeat(n), "]".repeat(n));
    let raw: Vec<Vec<u8>> = vec![
        br#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":0,"at":1}]}"#.to_vec(),
        br#"{"id":5,"type":"run","ops":[],"ops":[]}"#.to_vec(),
        b"{\"id\":5,\"type\":\"run\",\"ops\":[{\"op\":\"add_tag\",\"name\":\"\xc0\xaf\",\"from\":0,\"to\":0}]}".to_vec(),
        b"{\"id\":5,\"type\":\"run\",\"ops\":[{\"op\":\"add_tag\",\"name\":\"\xed\xa0\x80\",\"from\":0,\"to\":0}]}".to_vec(),
        b"{\"id\":5,\"type\":\"run\",\"ops\":[{\"op\":\"add_tag\",\"name\":\"\xf8\x88\x80\x80\x80\",\"from\":0,\"to\":0}]}".to_vec(),
        b"{\"id\":5,\"type\":\"run\",\"ops\":[{\"op\":\"add_tag\",\"name\":\"\xf4\x90\x80\x80\",\"from\":0,\"to\":0}]}".to_vec(),
        br#"{"id":5,"type":"run","ops":[{"op":"add_tag","name":"\udc00\ud800","from":0,"to":0}]}"#.to_vec(),
        br#"{"id":5,"type":"run","ops":[{"op":"add_tag","name":"\u12","from":0,"to":0}]}"#.to_vec(),
        format!(r#"{{"id":5,"type":"run","ops":[{{"op":"add_frame","at":{digits}}}]}}"#).into_bytes(),
        format!(r#"{{"id":-{digits},"type":"hello","api":1}}"#).into_bytes(),
        br#"{"id":9223372036854775808,"type":"hello","api":1}"#.to_vec(),
        br#"{"id":5,"type":"run","ops":[{"op":"add_frame","at":-9223372036854775809}]}"#.to_vec(),
        format!(r#"{{"id":"{}","type":"hello","api":1}}"#, "x".repeat(257)).into_bytes(),
        format!(r#"{{"id":5,"type":"run","ops":[{{"op":"add_frame","at":{}}}]}}"#, nest(7)).into_bytes(),
        format!(r#"{{"id":5,"type":"run","ops":{}}}"#, nest(500_000)).into_bytes(),
        format!(r#"{{"id":5,"type":"run","ops":[],"pad":"{}"}}"#, "a".repeat(1 << 20)).into_bytes(),
        b"\xef\xbb\xbf{\"id\":5,\"type\":\"run\",\"ops\":[]}".to_vec(),
        b"{\"id\":5,\"type\":\"run\",\"ops\":[]}\x00".to_vec(),
        br#"{"id":5,"type":"open","path":"walk.piks\u0000.txt"}"#.to_vec(),
        br#"{"id":5,"type":"open","path":"../walk.piks"}"#.to_vec(),
    ];
    let mut input = Vec::new();
    for line in [HELLO.as_bytes(), OPEN.as_bytes()] {
        input.extend_from_slice(line);
        input.push(b'\n');
    }
    for line in &raw {
        input.extend_from_slice(line);
        input.push(b'\n');
    }
    input.extend_from_slice(br#"{"id":6,"type":"run","ops":[{"op":"version"}]}"#);
    input.push(b'\n');
    let run = cli_input(&[&"session", &"--dir", &dir], &input);
    assert_eq!(run.code, 0, "{}", run.err);
    let replies: Vec<&str> = run.out.lines().collect();
    assert_eq!(replies.len(), raw.len() + 3, "{}", run.out);
    for (line, reply) in raw.iter().zip(&replies[2..]) {
        let shown = String::from_utf8_lossy(&line[..line.len().min(120)]).into_owned();
        assert!(!ok(reply), "{shown} -> {reply}");
        assert!(
            reply.len() < 4096,
            "{shown}: a reply of {} bytes",
            reply.len()
        );
    }
    assert!(ok(replies[replies.len() - 1]), "the session did not go on");
    assert_eq!(fs::read(dir.join("walk.piks")).expect("project"), before);
}
