//! What a document looks like, reduced to one number per frame.

use pikslar_core::{Damage, Document, Editor};
use pikslar_render::Compositor;

fn fnv1a(pixels: &[u32]) -> u32 {
    pixels
        .iter()
        .flat_map(|p| p.to_le_bytes())
        .fold(0x811c_9dc5, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        })
}

/// Composites every frame, with floating pixels in place, and hashes each result. The editor is
/// left on its original frame.
pub fn frame_hashes(editor: &mut Editor) -> Vec<u32> {
    editor.settle_floating();
    let mut compositor = Compositor::new(editor.doc().width, editor.doc().height);
    let keep = editor.frame;
    let hashes = (0..editor.doc().frames.len())
        .map(|frame| {
            editor.frame = frame;
            compositor.update(editor, Damage::All, &mut Vec::new());
            fnv1a(compositor.pixels())
        })
        .collect();
    editor.frame = keep;
    hashes
}

/// One number for everything stored: each cel's place and hold, and every pixel its image
/// holds, on the canvas and past it. A lost off-canvas pixel or a moved cel changes it.
pub fn stored_hash(doc: &Document) -> u32 {
    let mut words = Vec::new();
    for layer in doc.layers() {
        for cel in layer.cels() {
            let (x, y) = cel.position();
            let (x0, y0, tw, th) = cel.image().table();
            words.extend([x as u32, y as u32, cel.link(), x0 as u32, y0 as u32]);
            for ly in y0 * 16..(y0 + th as i32) * 16 {
                for lx in x0 * 16..(x0 + tw as i32) * 16 {
                    words.push(cel.image().get(lx, ly));
                }
            }
        }
    }
    fnv1a(&words)
}
