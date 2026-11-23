//! A line of text for a console, from a program built for the Windows subsystem
//! (`pikslar.exe --version`).
//!
//! Such a program starts with no console. When its output is redirected (a pipe or a file) the
//! standard output handle is already set and is written as usual. When it was started from a
//! console window (Command Prompt, PowerShell, Windows Terminal) it attaches to that console
//! and writes there; the shell does not wait for a Windows program, so its prompt may already
//! be shown and the line appears after it. Started from Explorer there is no console and
//! nothing to write to: nothing is shown and no window opens.

use std::io::Write;

const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

#[link(name = "kernel32")]
extern "system" {
    fn GetStdHandle(which: u32) -> isize;
    fn AttachConsole(process: u32) -> i32;
    fn FreeConsole() -> i32;
}

/// Where the line went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    /// To the standard output handle the program was given (a pipe, a file or a console).
    StandardOutput,
    /// To the console of the program that started this one.
    ParentConsole,
    /// Nowhere: no output handle and no console to attach to (started from Explorer).
    Nowhere,
}

/// Writes `line` and a line break to standard output, or to the parent's console.
pub fn print_line(line: &str) -> Written {
    // SAFETY: no arguments to check; the handle is only compared and handed to std.
    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if handle != 0 && handle != -1 {
        let mut out = std::io::stdout().lock();
        if writeln!(out, "{line}").and_then(|()| out.flush()).is_ok() {
            return Written::StandardOutput;
        }
    }
    // SAFETY: attaching to the parent's console has no preconditions; it fails harmlessly.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return Written::Nowhere;
    }
    let done = std::fs::OpenOptions::new()
        .write(true)
        .open("CONOUT$")
        .and_then(|mut console| write!(console, "\r\n{line}\r\n"));
    // SAFETY: detaches the console attached above.
    unsafe { FreeConsole() };
    if done.is_ok() {
        Written::ParentConsole
    } else {
        Written::Nowhere
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_run_has_standard_output() {
        assert_eq!(print_line("console test line"), Written::StandardOutput);
    }
}
