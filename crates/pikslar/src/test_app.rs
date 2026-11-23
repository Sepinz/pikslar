//! An app under test, driven through the same entry points the window uses.

use crate::app::App;
use pikslar_core::{DrawCmd, SelectionCmd};
use pikslar_core::{Mask, SelectOp, Tool};
use pikslar_platform::{Button, Key, Mods, Window};
use pikslar_render::Surface;

/// Tests that make a recovery write count as under way (a process-wide count the session end
/// waits on) take this, so that no other test sees it.
pub(crate) static SESSION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) const WINDOW: (i32, i32) = (1200, 700);
pub(crate) const PLAIN: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
};
pub(crate) const SHIFT: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: false,
};
pub(crate) const ALT: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: true,
};

pub(crate) fn app(frames: usize) -> App {
    let mut a = App::launch(None, Some(32), None);
    a.resized(WINDOW.0, WINDOW.1, 1);
    for _ in 1..frames {
        a.new_frame(false);
    }
    a.set_frame(0);
    draw(&mut a);
    a
}

/// Draws the whole interface, side panels included.
pub(crate) fn draw_whole(a: &mut App) {
    draw_whole_at(a, WINDOW.0, WINDOW.1);
}

/// Draws the whole interface on a window `w` by `h` pixels.
pub(crate) fn draw_whole_at(a: &mut App, w: i32, h: i32) {
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = Surface { px: &mut px, w, h };
    let window = pikslar_platform::Window::detached();
    a.draw_interface(&mut surface, window, &mut Vec::new());
}

pub(crate) fn draw(a: &mut App) {
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let mut surface = Surface {
        px: &mut px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    a.draw_for_test(&mut surface);
}

pub(crate) fn key(a: &mut App, key: Key, mods: Mods) {
    a.key_down(key, mods, false, Window::detached());
    if let Some(c) = typed_char(key, mods) {
        a.text_typed(c);
    }
    draw(a);
}

/// The character Windows sends after a key on a US layout, as `WM_CHAR` does: none with Ctrl,
/// whose characters are control codes, nor with Alt, which sends a system character.
pub(crate) fn typed_char(key: Key, mods: Mods) -> Option<char> {
    if mods.ctrl || mods.alt {
        return None;
    }
    let c = match key {
        Key::Char(c) if c.is_ascii_alphabetic() && !mods.shift => c.to_ascii_lowercase(),
        Key::Char(c) if c.is_ascii_alphabetic() => c,
        Key::Char(c) if !mods.shift => c,
        Key::Space => b' ',
        Key::Minus if mods.shift => b'_',
        Key::Minus => b'-',
        Key::Comma if mods.shift => b'<',
        Key::Comma => b',',
        Key::Period if mods.shift => b'>',
        Key::Period => b'.',
        _ => return None,
    };
    Some(c as char)
}

pub(crate) fn dot(a: &mut App, x: i32, y: i32) {
    a.editor
        .apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x,
            y,
        })
        .expect("begin");
    a.editor.apply(DrawCmd::Commit).expect("commit");
}

pub(crate) fn screen(a: &App, x: i32, y: i32) -> (i32, i32) {
    let cell = a.view.num / a.view.den;
    (
        a.view.x + x * cell + cell / 2,
        a.view.y + y * cell + cell / 2,
    )
}

pub(crate) fn click(a: &mut App, at: (i32, i32), mods: Mods) {
    a.pointer_down(at.0, at.1, Button::Left, mods);
    draw(a);
    a.pointer_up(at.0, at.1, Button::Left);
    draw(a);
}

pub(crate) fn select(a: &mut App) {
    a.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(2, 2, 4, 4)),
            op: SelectOp::Replace,
        })
        .expect("select");
}

pub(crate) fn range(a: &mut App, from: usize, to: usize) {
    for (frame, mods) in [(from, PLAIN), (to, SHIFT)] {
        let at = (
            a.timeline.strip.x + frame as i32 * 34 + 10,
            a.timeline.strip.y + 5,
        );
        click(a, at, mods);
    }
}

pub(crate) fn durations(a: &App) -> Vec<u32> {
    a.editor
        .doc()
        .frames
        .iter()
        .map(|f| f.duration_ms)
        .collect()
}

pub(crate) fn wheel_timing(a: &mut App, ticks: i32) {
    let area = a.timeline.duration_area;
    a.timeline_wheel(ticks, area.x + 2, area.y + 2);
    draw(a);
}

/// The colours of the tiles [`map_app`] makes: tile 1 red, 2 green, 3 blue.
pub(crate) const TILE_COLORS: [u32; 3] = [0xffff_0000, 0xff00_ff00, 0xff00_00ff];

/// An app whose active layer is a tilemap layer over the paint layer, with a tileset of three
/// 4x4 tiles of one colour each, drawn whole.
pub(crate) fn map_app() -> (App, pikslar_core::tileset::TilesetId) {
    use pikslar_core::{ColorMode, Grid, TileCmd, TileSource};
    let mut a = app(1);
    let made = a
        .editor
        .apply(TileCmd::NewMapLayer {
            at: 1,
            name: "Floor".into(),
            mode: ColorMode::Rgba,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("room");
    let set = pikslar_core::tileset::TilesetId::new(made.created.expect("the tileset"));
    for color in TILE_COLORS {
        let image = std::sync::Arc::new(Grid::from_cells(4, 4, &[color; 16]).into_image());
        let from = TileSource::Image(image);
        a.editor
            .apply(TileCmd::AddTile { tileset: set, from })
            .expect("a tile");
    }
    draw_whole(&mut a);
    (a, set)
}

/// A click drawn with the whole interface, so the side panels see it.
pub(crate) fn click_whole(a: &mut App, at: (i32, i32), mods: Mods) {
    a.pointer_down(at.0, at.1, Button::Left, mods);
    draw_whole(a);
    a.pointer_up(at.0, at.1, Button::Left);
    draw_whole(a);
}

/// The middle of a widget last drawn.
pub(crate) fn centre_of(a: &App, id: u32) -> (i32, i32) {
    let found = a.ui.widgets().iter().find(|&&(_, seen)| seen == id);
    let (r, _) = found
        .copied()
        .unwrap_or_else(|| panic!("widget {id} is not drawn"));
    (r.x + r.w / 2, r.y + r.h / 2)
}

/// Waits for saves under way, as the frame would take them once their thread ends.
pub(crate) fn finish_saves(a: &mut App) {
    let within = std::time::Duration::from_secs(20);
    let done = a.wait_for_saves(pikslar_platform::Window::detached(), within);
    assert!(done, "the save ended");
}

/// What the status line says now.
pub(crate) fn said(a: &App) -> String {
    a.message
        .as_ref()
        .map(|(t, _)| t.clone())
        .unwrap_or_default()
}

/// Runs `f`, and ends the whole test run loudly if it takes longer than `secs`: a hang cannot
/// fail a test any other way.
pub(crate) fn within<T>(secs: u64, what: &str, f: impl FnOnce() -> T) -> T {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    let done = Arc::new(AtomicBool::new(false));
    let seen = done.clone();
    let what = what.to_string();
    std::thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            if seen.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        eprintln!("{what} did not finish within {secs} s");
        std::process::abort();
    });
    let out = f();
    done.store(true, Ordering::Relaxed);
    out
}

/// Draws interface words in Pikslar Mono, doubled, as the *Large text* option does, for the
/// tests of how a screen gives way when words are wide.
pub(crate) fn large_text() {
    pikslar_ui::font::set_face(pikslar_ui::font::Face::Mono);
}
