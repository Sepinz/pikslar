//! Soak B's memory growth taken apart (`docs/b1-unattended-report.md`). Its action mix, as
//! `spikes/soak.ps1 -Workload wide` posts it, is driven through the app's own entry points, and
//! at each checkpoint this process's private memory is set beside what the document and the
//! undo history hold. Two workloads: `wide` as Soak B ran it, where a new frame, layer or text
//! layer is sometimes kept, and `fixed`, where each one kept is deleted again (as a person would,
//! so the deletion is itself an undo step) and the document keeps its size.
//!
//! Not part of the suite: it takes minutes. Run it with
//! `cargo test --release -p pikslar --bin pikslar soak_b_study -- --ignored --nocapture`.

use crate::app::App;
use crate::test_app::*;
use pikslar_core::{ColorMode, FrameCmd, LayerCmd};
use pikslar_platform::{Button, Event, Handler, Key, Mods, Window};

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
const CTRL_SHIFT: Mods = Mods {
    ctrl: true,
    shift: true,
    alt: false,
};

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

#[repr(C)]
#[derive(Default)]
struct Counters {
    cb: u32,
    faults: u32,
    peak_working: usize,
    working: usize,
    quota: [usize; 4],
    pagefile: usize,
    peak_pagefile: usize,
    private: usize,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
}

/// This process's private bytes, as Task Manager's "commit" and the soak's `private_mb`.
fn private_bytes() -> usize {
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Counters::default()
    };
    // SAFETY: `c` is a live, correctly sized PROCESS_MEMORY_COUNTERS_EX for the call to fill.
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    c.private
}

fn tap(a: &mut App, k: Key, mods: Mods) {
    key(a, k, mods);
}

fn scribble(a: &mut App, r: &mut Rng) {
    let (x, y) = (2 + r.below(26) as i32, 2 + r.below(26) as i32);
    let from = screen(a, x, y);
    let to = screen(a, (x + 4).min(31), (y + 3).min(31));
    a.pointer_down(from.0, from.1, Button::Left, PLAIN);
    a.event(Event::Move { x: to.0, y: to.1 }, Window::detached());
    a.pointer_up(to.0, to.1, Button::Left);
    draw(a);
}

/// One action of the wide mix, as `ActWide` in `spikes/soak.ps1` posts it.
fn act(a: &mut App, r: &mut Rng) {
    let c = |b: u8| Key::Char(b);
    match r.below(12) {
        0 => (0..2).for_each(|_| scribble(a, r)),
        1 => (0..=r.below(5)).for_each(|_| tap(a, c(b'Z'), CTRL)),
        2 => (0..=r.below(3)).for_each(|_| tap(a, c(b'Z'), CTRL_SHIFT)),
        3 => {
            tap(a, c(b'M'), PLAIN);
            scribble(a, r);
            tap(a, c(b'H'), SHIFT);
            tap(a, c(b'D'), CTRL);
            tap(a, c(b'B'), PLAIN);
        }
        4 => {
            tap(a, c(b'R'), PLAIN);
            tap(a, c(b'R'), SHIFT);
        }
        5 => {
            tap(a, c(b'N'), PLAIN);
            scribble(a, r);
            (0..2).for_each(|_| tap(a, c(b'Z'), CTRL));
        }
        6 => b"OPPO".iter().copied().for_each(|k| tap(a, c(k), PLAIN)),
        7 => {
            tap(a, c(b"BELU"[r.below(4) as usize]), PLAIN);
            scribble(a, r);
            tap(a, c(b'B'), PLAIN);
        }
        8 => {
            tap(a, c(b'A'), CTRL);
            tap(a, c(b'H'), SHIFT);
            tap(a, c(b'D'), CTRL);
        }
        9 => {
            let at = a.editor.layer + 1;
            let mode = ColorMode::Rgba;
            if a.editor.apply(LayerCmd::Add { at, mode }).is_ok() {
                scribble(a, r);
                (0..2).for_each(|_| tap(a, c(b'Z'), CTRL));
            }
        }
        10 => {
            tap(a, c(b'X'), PLAIN);
            tap(a, Key::Enter, PLAIN);
            b"AB".iter().copied().for_each(|k| tap(a, c(k), PLAIN));
            tap(a, Key::Enter, PLAIN);
            tap(a, c(b'B'), PLAIN);
        }
        _ => scribble(a, r),
    }
}

/// Deletes frames and layers past the starting counts, newest first, each as its own step.
fn keep_size(a: &mut App, frames: usize, layers: usize) {
    while a.editor.doc().frames.len() > frames {
        let last = a.editor.doc().frames.len() - 1;
        if a.editor
            .apply(FrameCmd::Remove {
                from: last,
                to: last,
            })
            .is_err()
        {
            break;
        }
    }
    while a.editor.doc().layers().len() > layers {
        let top = a.editor.doc().layers().len() - 1;
        let Some(layer) = a.editor.layer_id(top) else {
            break;
        };
        if a.editor.apply(LayerCmd::Remove { layer }).is_err() {
            break;
        }
    }
    draw(a);
}

fn run(name: &str, fixed: bool, actions: usize, budget: Option<usize>) {
    let mut a = app(1);
    if let Some(budget) = budget {
        a.editor = pikslar_core::Editor::with_undo_budget(a.editor.doc().clone(), budget);
    }
    let (frames, layers) = (a.editor.doc().frames.len(), a.editor.doc().layers().len());
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let base = private_bytes();
    println!("{name}: actions,frames,layers,undo_steps,history_kb,project_kb,private_kb_growth");
    for done in 0..=actions {
        if done % (actions / 20) == 0 {
            let doc = a.editor.doc();
            let project = crate::files::checked_bytes(doc).map_or(0, |b| b.len());
            println!(
                "{name}: {done},{},{},{},{},{},{}",
                doc.frames.len(),
                doc.layers().len(),
                a.editor.history().undo_depth(),
                a.editor.history().memory_used() / 1024,
                project / 1024,
                private_bytes().saturating_sub(base) / 1024
            );
        }
        act(&mut a, &mut r);
        if fixed {
            keep_size(&mut a, frames, layers);
        }
    }
}

#[test]
#[ignore = "a study of minutes, run by hand"]
fn soak_b_study_wide() {
    run("wide", false, 20_000, None);
}

#[test]
#[ignore = "a study of minutes, run by hand"]
fn soak_b_study_fixed() {
    run("fixed", true, 20_000, None);
}

/// `fixed` with a 1 MiB undo budget: whether growth stops where history is bounded.
#[test]
#[ignore = "a study of minutes, run by hand"]
fn soak_b_study_capped() {
    run("capped", true, 20_000, Some(1 << 20));
}
