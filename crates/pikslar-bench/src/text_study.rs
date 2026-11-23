//! `pikslar-bench text-study`: typing into the text tool.
//!
//! One key makes the text drawn again and swapped for the floating pixels, and the canvas shown
//! again: the path of one event in a text session, from `rasterize` to the pixels ready to
//! present. Sixty-four characters at scale 8 make a clip of about 220,000 pixels on the 512
//! canvas, the heaviest ordinary case; the target is a median under 3 ms an event, as for
//! drawing. This prints the study; `text-check` (text_gate) holds the same path to
//! perf/text-baseline.txt and perf/text-limits.txt. Both are synthetic, in-process timing.

use crate::workloads::{percentile, Canvas};
use pikslar_core::color::rgba;
use pikslar_core::glyphs::{rasterize, Font};
use pikslar_core::{ColorMode, Document, Editor, FloatingCmd};
use pikslar_render::DragPreview;
use std::sync::Arc;
use std::time::Instant;

const SIZE: usize = 512;
const CHARS: usize = 64;
/// The median an event may take, in microseconds, as drawing is held to.
const TARGET_MEDIAN_US: f64 = 3_000.0;

/// What a session's keys took, in microseconds each, and the editor they were typed into.
pub(crate) fn type_text(scale: i32) -> (Vec<f64>, Editor) {
    let mut editor = Editor::new(Document::new(SIZE, SIZE, ColorMode::Rgba).expect("a canvas"));
    editor.color = rgba(230, 120, 30, 255);
    let mut canvas = Canvas::new(SIZE);
    let empty = Arc::new(pikslar_core::Clip::from_image(0, 0, Vec::new()));
    let paste = FloatingCmd::Paste {
        clip: empty,
        x: 2,
        y: 2,
    };
    editor.apply(paste).expect("a session opens");
    let mut text = String::new();
    let mut times = Vec::with_capacity(CHARS);
    for i in 0..CHARS {
        text.push(char::from(b'a' + (i % 26) as u8));
        let start = Instant::now();
        let (clip, _) = rasterize(Font::Pikslar5x7, scale, 1, &text, editor.color);
        let clip = Arc::new(clip);
        editor
            .apply(FloatingCmd::ReplacePasted { clip })
            .expect("the text fits");
        // A new text is a new floating epoch, so the screen-sized preview is made again.
        let mut preview: Option<DragPreview> = None;
        canvas.present_drag(&mut editor, &mut preview);
        times.push(start.elapsed().as_secs_f64() * 1e6);
    }
    (times, editor)
}

pub fn run() {
    println!("Typing {CHARS} characters, the time of each key (median, p99, last), microseconds:");
    for scale in [1, 2, 4, 8] {
        let (mut times, editor) = type_text(scale);
        let last = times.last().copied().unwrap_or(0.0);
        times.sort_by(f64::total_cmp);
        let (median, p99) = (percentile(&times, 0.5), percentile(&times, 0.99));
        let clip = editor.floating_view().map_or(0, |f| f.placed.len());
        let verdict = if median <= TARGET_MEDIAN_US {
            "within"
        } else {
            "OVER"
        };
        println!(
            "scale {scale}: {median:>9.1} {p99:>9.1} {last:>9.1}   clip {clip:>7} cells   {verdict} the {TARGET_MEDIAN_US:.0} us target"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_types_every_character_and_keeps_its_text_until_anchored() {
        let (times, mut editor) = type_text(2);
        assert_eq!(times.len(), CHARS);
        assert!(times.iter().all(|&t| t > 0.0));
        assert!(editor.is_pasting(), "the session is still open");
        let depth = editor.history().undo_depth();
        editor.apply(FloatingCmd::Anchor).expect("anchors");
        assert_eq!(editor.history().undo_depth(), depth + 1, "one step in all");
        let lit = (0..40).any(|x| (0..20).any(|y| editor.color_at(x, y) != 0));
        assert!(lit, "the text is drawn");
    }

    #[test]
    fn the_heaviest_case_is_about_a_quarter_of_a_million_cells() {
        let (_, editor) = type_text(8);
        let cells = editor.floating_view().map_or(0, |f| f.placed.len());
        assert!((200_000..300_000).contains(&cells), "{cells}");
    }
}
