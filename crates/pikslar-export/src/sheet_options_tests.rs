//! Sheets laid out in columns, padded, and described as a hash, against the compositor: every
//! corpus project, several layouts, scales and kinds of PNG. Each frame must be at the place
//! its description gives, everything else clear, and the description must say the image's size.

use crate::picture_tests::{corpus, expected};
use crate::{
    sheet_files, sheet_plan, write_sheet_png, ExportError, JsonShape, Layers, Layout, Picture,
    SheetOptions,
};
use pikslar_core::{ColorMode, Document};
use pikslar_format::png;

fn options(columns: Option<usize>, padding: usize, json: JsonShape) -> SheetOptions {
    SheetOptions {
        layout: Layout { columns, padding },
        json,
    }
}

/// The rectangle the description gives the frame named `name`, in either shape.
fn rectangle(description: &str, name: &str, shape: JsonShape) -> (usize, usize) {
    let key = match shape {
        JsonShape::Array => format!("{{\"filename\":\"{name}\",\"frame\":{{"),
        JsonShape::Hash => format!("\"{name}\":{{\"frame\":{{"),
    };
    let at = description.find(&key).expect("frame described") + key.len();
    let field = |label: &str| {
        let rest = &description[at..];
        let from = rest.find(label).expect("field") + label.len();
        let digits: String = rest[from..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse::<usize>().expect("a number")
    };
    (field("\"x\":"), field("\"y\":"))
}

fn check(doc: &Document, picture: &Picture<'_>, choice: &SheetOptions, layers: &Layers) -> bool {
    let last = doc.frames.len() - 1;
    let Ok(plan) = sheet_plan(picture, 0..=last, ("s.png", "s"), choice) else {
        return false;
    };
    let mut bytes = Vec::new();
    write_sheet_png(picture, (&plan.sheet, 0), plan.palette.as_ref(), &mut bytes).expect("made");
    let image = png::decode(&bytes).expect("decodes");
    let size = format!("\"size\":{{\"w\":{},\"h\":{}}}", image.width, image.height);
    assert!(plan.description.contains(&size), "{choice:?}");
    let (w, scale) = (picture.width(), picture.scale());
    let mut covered = vec![false; image.px.len()];
    for (frame, &(x, y)) in plan.sheet.places.iter().enumerate() {
        let name = format!("s {frame}");
        assert_eq!(rectangle(&plan.description, &name, choice.json), (x, y));
        for (r, row) in expected(doc, layers, frame, scale)
            .chunks_exact(w)
            .enumerate()
        {
            let at = (y + r) * image.width + x;
            assert_eq!(
                &image.px[at..at + w],
                row,
                "{choice:?} frame {frame} row {r}"
            );
            covered[at..at + w].fill(true);
        }
    }
    let gaps = image.px.iter().zip(&covered).filter(|(_, &c)| !c);
    assert!(gaps.into_iter().all(|(&p, _)| p == 0), "{choice:?}");
    true
}

#[test]
fn every_layout_holds_each_frame_where_its_description_says_and_clear_between() {
    let mut checked = 0;
    let layouts = [
        options(None, 0, JsonShape::Hash),
        options(None, 3, JsonShape::Array),
        options(Some(1), 2, JsonShape::Hash),
        options(Some(2), 0, JsonShape::Array),
        options(Some(3), 1, JsonShape::Hash),
        options(Some(64), 5, JsonShape::Array),
    ];
    for doc in corpus().into_iter().filter(|d| d.frames.len() > 1) {
        let layers = Layers::Visible;
        for scale in [1, 2] {
            for indexed in [false, true] {
                let Ok(picture) = Picture::new(&doc, &layers, scale) else {
                    continue;
                };
                let picture = picture.with_indexed(indexed);
                for choice in &layouts {
                    checked += usize::from(check(&doc, &picture, choice, &layers));
                }
            }
        }
    }
    assert!(checked > 40, "{checked}");
}

fn document(frames: usize, side: usize) -> Document {
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("valid");
    for at in 1..frames {
        doc.add_frame(at, false).expect("room");
    }
    doc
}

#[test]
fn a_hash_names_each_frame_once_and_keeps_the_rest_of_the_description() {
    let mut doc = document(3, 8);
    doc.frames[2].duration_ms = 40;
    doc.add_tag("run", 1, 2).expect("valid");
    let picture = Picture::visible(&doc);
    let hash = sheet_plan(
        &picture,
        0..=2,
        ("a.png", "a"),
        &options(None, 0, JsonShape::Hash),
    )
    .expect("planned")
    .description;
    let array = sheet_files(&doc, 0..=2, ("a.png", "a"))
        .expect("made")
        .description;
    assert!(
        hash.starts_with("{\"frames\":{\"a 0\":{\"frame\":"),
        "{hash}"
    );
    assert!(
        hash.contains("},\"a 1\":{\"frame\":{\"x\":0,\"y\":8,"),
        "{hash}"
    );
    assert!(hash.contains("\"duration\":40}},\"meta\":"), "{hash}");
    assert!(!hash.contains("filename"), "{hash}");
    assert!(array.starts_with("{\"frames\":[{\"filename\":\"a 0\",\"frame\":"));
    assert!(array.contains("\"duration\":40}],\"meta\":"), "{array}");
    let meta = |text: &str| text[text.find("\"meta\"").expect("meta")..].to_string();
    assert_eq!(meta(&hash), meta(&array));
}

#[test]
fn a_layout_too_large_says_what_to_change_at_any_scale() {
    let doc = document(3, 4096);
    for scale in [1, 2] {
        let picture = Picture::new(&doc, &Layers::Visible, scale).expect("drawn");
        let grid = options(Some(3), 0, JsonShape::Array);
        match sheet_plan(&picture, 0..=2, ("a.png", "a"), &grid) {
            Err(ExportError::LayoutTooLarge { scale: s }) => assert_eq!(s, scale),
            other => panic!("{:?}", other.map(|_| ())),
        }
    }
    let message = ExportError::LayoutTooLarge { scale: 1 }.to_string();
    assert!(message.contains("columns and padding"), "{message}");
    assert!(!message.contains("scale"), "{message}");
    let scaled = ExportError::LayoutTooLarge { scale: 2 }.to_string();
    assert!(
        scaled.contains("less padding, a smaller scale or fewer"),
        "{scaled}"
    );
    let picture = Picture::visible(&doc);
    let rows = sheet_plan(&picture, 0..=2, ("a.png", "a"), &SheetOptions::default());
    assert!(rows.is_ok(), "today's layout wraps instead");
}
