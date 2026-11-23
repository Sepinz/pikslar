use super::*;

fn refused(path: &str) -> bool {
    check_path(Path::new(path)).is_err()
}

#[test]
fn only_plain_absolute_file_paths_are_written_to() {
    assert!(!refused(r"C:\Users\a\Pictures\out.txt"));
    assert!(!refused(r"\\server\share\out.txt"));
    assert!(!refused(r"\\?\C:\Users\a\out.txt"));
    assert!(
        !refused(r"C:\Users\a\con-tour.txt"),
        "only the stem names a device"
    );
    for bad in [
        "out.txt",
        r"C:out.txt",
        r"\out.txt",
        r"C:\a\..\out.txt",
        r"C:\a\.\out.txt",
        r"\\.\C:\out.txt",
        r"\\.\pipe\x",
        r"\\?\GLOBALROOT\Device\x",
        r"C:\a\out.txt:hidden",
        r"C:\a\out.txt:hidden:$DATA",
        r"C:\a\out.",
        r"C:\a\out ",
        r"C:\a\NUL",
        r"C:\a\nul.txt",
        r"C:\a\Con .txt",
        r"C:\a\com1.png",
        r"C:\a\LPT9",
        "C:\\a\\com\u{b9}.txt",
        r"C:\a\CONOUT$",
        r"C:\",
        r"C:\a\report.bat",
        r"C:\a\report.CMD",
        r"C:\a\run.ps1",
        r"C:\a\x.appref-ms",
        r"\\?\C:\a\x.hta",
        r"C:\a\x.pyw",
        r"C:\a\x.pyw.",
        r"C:\a\x.pyw ",
        r"C:\a\x.rdp",
        r"C:\a\x.html",
        r"C:\a\desktop.ini",
        "C:\\a\\x.txt\u{200b}",
    ] {
        assert!(refused(bad), "{bad} was taken");
    }
}

#[test]
fn what_was_there_is_told_apart_without_following_links() {
    let dir = std::env::temp_dir().join(format!("pikslar-look-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("folder")).expect("a folder");
    fs::write(dir.join("file.txt"), "x").expect("written");
    assert_eq!(look(&dir.join("file.txt")), Seen::File);
    assert_eq!(look(&dir.join("missing.txt")), Seen::Absent);
    assert_eq!(look(&dir.join("folder")), Seen::Unusable);
    let _ = fs::remove_dir_all(dir);
}

#[cfg(windows)]
#[test]
fn the_startup_folders_are_never_written_to() {
    let folders = crate::startup::folders();
    assert!(
        !folders.is_empty(),
        "the user's Startup folder was not found"
    );
    for folder in folders {
        let target = folder.join("pikslar-test-never-written.txt");
        let wanted = [Wanted {
            target: &target,
            seen: Seen::Absent,
            bytes: b"x",
        }];
        let got = commit(&wanted, &mut PlainDisk, &[], &|| true);
        assert!(matches!(got, Err(FileError::Refused(_))), "{got:?}");
        assert!(!target.exists());
        // A folder inside it is no way round.
        let inner = folder.join("inside").join("x.txt");
        let wanted = [Wanted {
            target: &inner,
            seen: Seen::Absent,
            bytes: b"x",
        }];
        assert!(commit(&wanted, &mut PlainDisk, &[], &|| true).is_err());
    }
}
