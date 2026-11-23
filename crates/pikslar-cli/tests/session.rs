//! `session` as a launched tool sees it: JSON lines over the real standard streams, and the
//! folder given with --dir. The requests themselves are tested in the program's own tests.

mod common;
use common::{bottom_layer, cli, cli_input, folder, listing, project};
use std::fs;

#[test]
fn a_session_over_standard_streams_edits_and_saves() {
    let dir = folder("session-streams");
    let p = project(&dir.join("walk.piks"), 8, 2, 11);
    let layer = bottom_layer(&p);
    let input = format!(
        "{}\n{}\r\n{}\n{}\n",
        r#"{"id":1,"type":"hello","api":1}"#,
        r#"{"id":2,"type":"open","path":"walk.piks"}"#,
        format_args!(
            r#"{{"id":3,"type":"run","ops":[{{"op":"document"}},{{"op":"set_pixel","layer":{layer},"frame":{{"result":0,"place":0}},"x":1,"y":1,"color":4278190335}}]}}"#
        ),
        r#"{"id":4,"type":"save","path":"edited.piks"}"#,
    );
    let run = cli_input(&[&"session", &"--dir", &dir], input.as_bytes());
    assert_eq!(run.code, 0, "{}", run.err);
    let replies: Vec<&str> = run.out.lines().collect();
    assert_eq!(replies.len(), 4, "{}", run.out);
    for (i, reply) in replies.iter().enumerate() {
        assert!(
            reply.starts_with(&format!(r#"{{"id":{},"ok":true"#, i + 1)),
            "{reply}"
        );
    }
    assert_eq!(listing(&dir), ["edited.piks", "walk.piks"]);
}

#[test]
fn input_ending_mid_line_is_answered_and_ends_cleanly() {
    let dir = folder("session-unended");
    let p = project(&dir.join("walk.piks"), 8, 1, 12);
    let before = fs::read(&p).expect("read");
    let input = concat!(
        r#"{"id":1,"type":"hello","api":1}"#,
        "\n",
        r#"{"id":2,"type":"open","path":"walk.piks"}"#,
        "\n",
        r#"{"id":3,"type":"run","ops":[{"op":"add_frame","at":0}]}"#,
        "\n",
        r#"{"id":4,"type":"save"}"#
    );
    let run = cli_input(&[&"session", &"--dir", &dir], input.as_bytes());
    assert_eq!(run.code, 0, "{}", run.err);
    let last = run.out.lines().last().expect("replies");
    assert!(last.contains(r#""code":"unended""#), "{last}");
    assert_eq!(fs::read(&p).expect("read"), before);
}

#[test]
fn the_folder_must_be_given_and_exist() {
    let dir = folder("session-args");
    assert_eq!(cli(&[&"session"]).code, 1);
    assert_eq!(cli(&[&"session", &"--dir"]).code, 1);
    assert_eq!(cli(&[&"session", &"--dir", &dir, &"--dir", &dir]).code, 1);
    assert_eq!(cli(&[&"session", &"--dir", &dir, &"--net"]).code, 1);
    assert_eq!(cli(&[&"session", &"--dir", &dir.join("missing")]).code, 2);
    let file = project(&dir.join("a.piks"), 4, 1, 1);
    assert_eq!(cli(&[&"session", &"--dir", &file]).code, 2);
    let help = cli(&[&"help", &"session"]);
    assert_eq!(help.code, 0);
    assert!(help.out.contains("--dir"), "{}", help.out);
    let help = cli(&[&"help", &"batch"]);
    assert!(help.out.contains("\"ops\""), "{}", help.out);
}
