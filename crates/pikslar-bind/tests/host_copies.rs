//! The copies `write_rect` and `read_rect` make outside the script's values are charged to
//! the run's memory while they live, so the script's list and the host's copy of it together
//! stay within the limit (`docs/scripting-api.md`, Memory).

mod common;

use common::{run_with, save};
use pikslar_bind::{Capabilities, ScriptLimits};
use pikslar_core::{ColorMode, Document, Editor};
use pikslar_script::ErrorKind;

fn canvas() -> Editor {
    Editor::new(Document::new(256, 256, ColorMode::Rgba).expect("a canvas"))
}

fn one_mib() -> ScriptLimits {
    ScriptLimits {
        memory: 1 << 20,
        ..ScriptLimits::default()
    }
}

/// 50,000 colors: the list is 800,000 bytes of values, the host's copy 400,000 more.
#[test]
fn write_rect_charges_its_copy_of_the_colors() {
    let mut e = canvas();
    let before = save(&e);
    let src = "write_rect(layers()[0], frames()[0], 0, 0, 250, list(50000, 4278190335));";
    let out = run_with(&mut e, src, Capabilities::ALL, one_mib());
    assert_eq!(
        out.result.map_err(|f| f.kind()),
        Err(Some(ErrorKind::Memory))
    );
    assert_eq!(save(&e), before, "taken back");
    // Half as many fit with their copy.
    let src = "write_rect(layers()[0], frames()[0], 0, 0, 250, list(25000, 4278190335));";
    let out = run_with(&mut e, src, Capabilities::ALL, one_mib());
    assert!(out.result.is_ok(), "{:?}", out.result);
}

/// 60,000 pixels: the list is 960,000 bytes of values, the API's read 240,000 more.
#[test]
fn read_rect_charges_the_colors_it_reads_before_its_list() {
    let mut e = canvas();
    let src = "let b = read_rect(layers()[0], frames()[0], 0, 0, 240, 250); return len(b);";
    let out = run_with(&mut e, src, Capabilities::ALL, one_mib());
    assert_eq!(
        out.result.map_err(|f| f.kind()),
        Err(Some(ErrorKind::Memory))
    );
    let src = "let b = read_rect(layers()[0], frames()[0], 0, 0, 200, 200); return len(b);";
    let out = run_with(&mut e, src, Capabilities::ALL, one_mib());
    assert_eq!(out.result, Ok("40000".to_string()));
}

/// The charge lasts only while the copy does: a loop of reads and writes the size of the
/// canvas fits a run whose memory holds one list and one copy.
#[test]
fn the_copies_are_given_back_after_each_call() {
    let mut e = canvas();
    let src = "let l = layers()[0]; let f = frames()[0];\n\
               for i in 0..20 { let b = read_rect(l, f, 0, 0, 160, 160); write_rect(l, f, 0, 0, 160, b); }";
    let out = run_with(&mut e, src, Capabilities::ALL, one_mib());
    assert!(out.result.is_ok(), "{:?}", out.result);
}
