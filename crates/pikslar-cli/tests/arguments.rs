//! Arguments, help and exit codes.

mod common;
use common::{cli, corpus, folder, listing};

#[test]
fn help_and_version_succeed_and_name_the_exit_codes() {
    let none = cli(&[]);
    assert_eq!(none.code, 0);
    assert!(none.out.contains("Exit codes:") && none.out.contains("4  the output"));
    for args in [["help", "export"], ["export", "--help"]] {
        let run = cli(&[&args[0], &args[1]]);
        assert_eq!(run.code, 0);
        assert!(run.out.contains("--overwrite"), "{}", run.out);
    }
    assert!(cli(&[&"info", &"-h"]).out.contains("--json"));
    let version = cli(&[&"--version"]);
    assert_eq!(
        (version.code, version.out.trim()),
        (0, concat!("pikslar-cli ", env!("CARGO_PKG_VERSION")))
    );
}

#[test]
fn arguments_that_make_no_sense_exit_1_with_a_hint() {
    let p = corpus("v3-animated.piks");
    // A folder that does not exist, so a case that wrongly gets past the arguments writes nothing.
    let nowhere = std::env::temp_dir()
        .join("pikslar-cli-no-such-folder")
        .join("x.png");
    let cases: Vec<Vec<&dyn AsRef<std::ffi::OsStr>>> = vec![
        vec![&"bogus"],
        vec![&"export"],
        vec![&"info"],
        vec![&"export", &p, &"--bogus"],
        vec![&"export", &p, &"--format"],
        vec![&"export", &p, &"--format", &"webp"],
        vec![
            &"export",
            &p,
            &"--format",
            &"png",
            &"--format",
            &"sheet",
            &"-o",
            &nowhere,
        ],
        vec![&"export", &p, &"--frame", &"0"],
        vec![&"export", &p, &"--frame", &"x"],
        vec![&"export", &p, &"--frames", &"3"],
        vec![
            &"export", &p, &"--frame", &"1", &"--tag", &"walk", &"-o", &nowhere,
        ],
        vec![&"export", &p, &p],
        vec![&"export", &p, &"-o", &"a.png", &"-o", &"b.png"],
        vec![&"export", &p, &"-o", &"a.piks"],
        vec![&"info", &p, &"--bogus"],
        vec![&"export", &p, &"--columns", &"2", &"-o", &nowhere],
        vec![
            &"export",
            &p,
            &"--format",
            &"gif",
            &"--padding",
            &"1",
            &"-o",
            &nowhere,
        ],
        vec![
            &"export",
            &p,
            &"--format",
            &"sequence",
            &"--json",
            &"hash",
            &"-o",
            &nowhere,
        ],
        vec![
            &"export",
            &p,
            &"--format",
            &"sheet",
            &"--columns",
            &"0",
            &"-o",
            &nowhere,
        ],
        vec![
            &"export",
            &p,
            &"--format",
            &"sheet",
            &"--columns",
            &"x",
            &"-o",
            &nowhere,
        ],
        vec![
            &"export",
            &p,
            &"--format",
            &"sheet",
            &"--padding",
            &"-1",
            &"-o",
            &nowhere,
        ],
        vec![
            &"export",
            &p,
            &"--format",
            &"sheet",
            &"--json",
            &"xml",
            &"-o",
            &nowhere,
        ],
        vec![&"export", &p, &"--format", &"sheet", &"--json"],
        vec![
            &"export",
            &p,
            &"--format",
            &"sheet",
            &"--padding",
            &"1",
            &"--padding",
            &"2",
            &"-o",
            &nowhere,
        ],
    ];
    for args in cases {
        let run = cli(&args);
        assert_eq!(
            run.code,
            1,
            "{:?}: {}",
            args.iter().map(|a| a.as_ref()).collect::<Vec<_>>(),
            run.err
        );
        assert!(run.err.starts_with("pikslar-cli: ") && run.err.contains("pikslar-cli help"));
        assert!(run.out.is_empty());
    }
}

#[test]
fn choices_the_project_cannot_meet_exit_3_and_name_the_problem() {
    let p = corpus("v3-animated.piks");
    let d = folder("unmet");
    let out = d.join("never.png");
    let cases: [(&[&str], &str); 5] = [
        (&["--frame", "6"], "no frame 6"),
        (&["--tag", "run"], "no tag named \"run\""),
        (&["--frames", "3-2"], "end before they start"),
        (&["--tag", "walk"], "3 were chosen"),
        (&["--frames", "1-2"], "2 were chosen"),
    ];
    for (extra, says) in cases {
        let mut args: Vec<&dyn AsRef<std::ffi::OsStr>> = vec![&"export", &p, &"-o", &out];
        args.extend(extra.iter().map(|a| a as &dyn AsRef<std::ffi::OsStr>));
        let run = cli(&args);
        assert_eq!(run.code, 3, "{extra:?}: {}", run.err);
        assert!(run.err.contains(says), "{extra:?}: {}", run.err);
        assert!(listing(&d).is_empty(), "{extra:?} wrote a file");
    }
    std::fs::remove_dir(d).expect("cleanup");
}
