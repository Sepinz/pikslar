//! The release script's binary checks (`spikes/make-release.ps1`); not shipped.
//!
//! ```text
//! pe-check <program.exe> [--forbid <text>]...
//! ```
//!
//! Prints the PE time stamp and every imported DLL with its functions, then fails (exit 1) if
//! the time stamp is not zero, a DLL is not one every Windows 10 and 11 machine has, or any
//! `--forbid` text (a user name, workspace or tool path; either case, bytes or UTF-16) occurs.

use pikslar_res::pe;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("pe-check: {e}");
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<String>) -> Result<bool, String> {
    let (path, forbid) = parse(&args)?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let mut ok = true;
    let stamp = pe::timestamp(&bytes)?;
    println!("{path}\ntime stamp: {stamp:#010x}");
    if stamp != 0 {
        println!("FAIL: the PE time stamp is set");
        ok = false;
    }
    for import in pe::imports(&bytes)? {
        let system = pe::is_system_dll(&import.dll);
        let mark = if system { "ok  " } else { "FAIL" };
        println!("{mark} {} ({})", import.dll, import.functions.len());
        println!("       {}", import.functions.join(" "));
        ok &= system;
    }
    for text in &forbid {
        let found = pe::find_text(&bytes, text);
        if !found.is_empty() {
            println!(
                "FAIL: {text:?} found {} times, first at {:#x}",
                found.len(),
                found[0]
            );
            ok = false;
        }
    }
    println!("{} forbidden texts checked: {}", forbid.len(), verdict(ok));
    Ok(ok)
}

fn verdict(ok: bool) -> &'static str {
    if ok {
        "PASS"
    } else {
        "FAIL"
    }
}

fn parse(args: &[String]) -> Result<(String, Vec<String>), String> {
    let mut path = None;
    let mut forbid = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--forbid" => {
                let text = args.get(i + 1).ok_or("--forbid needs a text")?;
                if text.trim().is_empty() {
                    return Err("an empty --forbid text".into());
                }
                forbid.push(text.clone());
                i += 1;
            }
            other if path.is_none() => path = Some(other.to_string()),
            other => return Err(format!("unexpected argument {other:?}")),
        }
        i += 1;
    }
    Ok((
        path.ok_or("usage: pe-check <program.exe> [--forbid <text>]...")?,
        forbid,
    ))
}
