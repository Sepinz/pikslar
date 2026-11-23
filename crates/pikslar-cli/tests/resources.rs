//! `pikslar-cli.exe` carries the same version information as `pikslar.exe`, and its own console
//! manifest. `PIKSLAR_EXE` points the check at another built file (the release script).

use pikslar_res::{read, RT_GROUP_ICON, RT_MANIFEST, RT_VERSION};

fn exe() -> String {
    std::env::var("PIKSLAR_CLI_EXE").unwrap_or_else(|_| env!("CARGO_BIN_EXE_pikslar-cli").into())
}

#[test]
fn the_version_block_matches_the_programs() {
    let bytes = std::fs::read(exe()).expect("the built exe");
    let all = read::pe_resources(&bytes).expect("a readable resource tree");
    let versions: Vec<_> = all.iter().filter(|(l, _)| l.ty == RT_VERSION).collect();
    assert_eq!(versions.len(), 1);
    let (_, strings) = read::version_strings(&versions[0].1).expect("a version block");
    let get = |k: &str| {
        strings
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("ProductName"), Some("Pikslar"));
    assert_eq!(get("ProductVersion"), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(get("OriginalFilename"), Some("pikslar-cli.exe"));
    assert!(get("Comments").is_some_and(|c| c.starts_with("Commit ")));
    assert!(all.iter().all(|(l, _)| l.ty != RT_GROUP_ICON), "no icon");
    for (_, manifest) in all.iter().filter(|(l, _)| l.ty == RT_MANIFEST) {
        let text = String::from_utf8_lossy(manifest);
        assert!(
            !text.contains("PerMonitorV2"),
            "not the window program's manifest"
        );
    }
}
