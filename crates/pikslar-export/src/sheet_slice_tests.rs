//! Slices in a sprite sheet's description: scaled, clipped to the canvas, and left out when
//! there are none.

use crate::sheet_files;
use crate::sheet_slices::push_slices;
use pikslar_core::{ColorMode, Document, Slice, SliceRect};

fn rect(x: i32, y: i32, width: u32, height: u32) -> SliceRect {
    SliceRect {
        x,
        y,
        width,
        height,
    }
}

fn slice(name: &str, bounds: SliceRect) -> Slice {
    Slice {
        name: name.to_owned(),
        color: 0x12_34_56_78,
        bounds,
        pivot: None,
        center: None,
    }
}

fn pushed(slices: &[Slice], canvas: (usize, usize), scale: usize) -> String {
    let mut out = String::new();
    push_slices(&mut out, slices, canvas, scale);
    out
}

#[test]
fn a_plain_slice_at_scale_one() {
    let out = pushed(&[slice("a", rect(2, 3, 4, 5))], (16, 16), 1);
    assert_eq!(
        out,
        ",\"slices\":[{\"name\":\"a\",\"color\":\"#12345678\",\"keys\":[{\"frame\":0,\
         \"bounds\":{\"x\":2,\"y\":3,\"w\":4,\"h\":5}}]}]"
    );
}

#[test]
fn pivot_and_centre_at_scale_three() {
    let mut s = slice("b", rect(1, 2, 6, 4));
    s.pivot = Some((2, 1));
    s.center = Some(rect(1, 1, 4, 2));
    // The canvas is as exported would be given: the scale only multiplies.
    let out = pushed(&[s], (16, 16), 3);
    assert_eq!(
        out,
        ",\"slices\":[{\"name\":\"b\",\"color\":\"#12345678\",\"keys\":[{\"frame\":0,\
         \"bounds\":{\"x\":3,\"y\":6,\"w\":18,\"h\":12},\
         \"center\":{\"x\":3,\"y\":3,\"w\":12,\"h\":6},\
         \"pivot\":{\"x\":6,\"y\":3}}]}]"
    );
}

#[test]
fn bounds_are_clipped_and_pivot_and_centre_shifted_by_what_the_clip_removed() {
    let mut s = slice("c", rect(-2, -3, 8, 10));
    s.pivot = Some((5, 4));
    s.center = Some(rect(1, 2, 5, 6));
    // Clipped to x 0..6, y 0..7: the slice lost 2 on the left and 3 on top.
    let out = pushed(&[s], (6, 7), 1);
    assert!(
        out.contains("\"bounds\":{\"x\":0,\"y\":0,\"w\":6,\"h\":7}"),
        "{out}"
    );
    assert!(out.contains("\"pivot\":{\"x\":3,\"y\":1}"), "{out}");
    // The centre moves to x -1, y -1 and is clipped to the new bounds: 0..4 by 0..5... in 6 by 7.
    assert!(
        out.contains("\"center\":{\"x\":0,\"y\":0,\"w\":4,\"h\":5}"),
        "{out}"
    );
}

#[test]
fn clipping_at_the_right_and_bottom_leaves_pivot_and_centre_alone() {
    let mut s = slice("d", rect(4, 4, 10, 10));
    s.pivot = Some((1, 2));
    s.center = Some(rect(2, 2, 8, 8));
    let out = pushed(&[s], (8, 8), 1);
    assert!(
        out.contains("\"bounds\":{\"x\":4,\"y\":4,\"w\":4,\"h\":4}"),
        "{out}"
    );
    assert!(out.contains("\"pivot\":{\"x\":1,\"y\":2}"), "{out}");
    assert!(
        out.contains("\"center\":{\"x\":2,\"y\":2,\"w\":2,\"h\":2}"),
        "{out}"
    );
}

#[test]
fn a_centre_clipped_away_is_left_out_but_the_slice_stays() {
    let mut s = slice("e", rect(0, 0, 10, 10));
    s.center = Some(rect(6, 6, 4, 4));
    let out = pushed(&[s], (4, 4), 1);
    assert!(out.contains("\"bounds\":{\"x\":0,\"y\":0,\"w\":4,\"h\":4}"));
    assert!(!out.contains("center"), "{out}");
}

#[test]
fn a_slice_wholly_outside_the_canvas_is_left_out() {
    for bounds in [
        rect(8, 0, 2, 2),
        rect(0, 8, 2, 2),
        rect(-2, 0, 2, 2),
        rect(0, -5, 2, 5),
    ] {
        let out = pushed(
            &[slice("gone", bounds), slice("in", rect(0, 0, 1, 1))],
            (8, 8),
            1,
        );
        assert!(!out.contains("gone"), "{out}");
        assert!(out.contains("\"in\""), "{out}");
    }
    assert_eq!(pushed(&[slice("gone", rect(9, 9, 1, 1))], (8, 8), 1), "");
}

#[test]
fn nothing_is_written_without_slices() {
    assert_eq!(pushed(&[], (8, 8), 1), "");
}

#[test]
fn names_are_escaped_and_slices_are_comma_separated() {
    let out = pushed(
        &[
            slice("q\"u\\o\nte", rect(0, 0, 1, 1)),
            slice("ünï", rect(1, 1, 1, 1)),
        ],
        (8, 8),
        1,
    );
    assert!(out.contains("\"name\":\"q\\\"u\\\\o\\u000ate\""), "{out}");
    assert!(out.contains("}]},{\"name\":\"ünï\""), "{out}");
}

#[test]
fn the_sheet_description_has_slices_after_the_tags_only_when_there_are_some() {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    let plain = sheet_files(&doc, 0..=0, ("s.png", "s")).expect("sheet");
    assert!(!plain.description.contains("slices"));
    doc.add_slice(slice("hit", rect(1, 1, 2, 2))).expect("fits");
    let with = sheet_files(&doc, 0..=0, ("s.png", "s"))
        .expect("sheet")
        .description;
    assert!(
        with.contains("\"frameTags\":[],\"slices\":[{\"name\":\"hit\""),
        "{with}"
    );
    assert!(with.ends_with("]}}\n"), "{with}");
}

#[test]
fn the_color_keeps_its_leading_zeros() {
    let mut s = slice("z", rect(0, 0, 1, 1));
    s.color = 0x00_0a_00_ff;
    assert!(pushed(&[s], (4, 4), 1).contains("\"color\":\"#000a00ff\""));
}

#[test]
fn a_scaled_sheet_clips_to_the_canvas_and_multiplies_by_the_scale() {
    use crate::{sheet_plan, Layers, Picture, SheetOptions};
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    let mut s = slice("edge", rect(6, 6, 6, 6));
    s.pivot = Some((1, 2));
    doc.add_slice(s).expect("fits");
    doc.add_slice(slice("out", rect(8, 0, 2, 2))).expect("fits");
    let picture = Picture::new(&doc, &Layers::Visible, 3).expect("picture");
    let plan = sheet_plan(&picture, 0..=0, ("s.png", "s"), &SheetOptions::default()).expect("plan");
    let d = plan.description;
    // The canvas is 8 wide, so the slice is cut to 2 by 2, and 3 times that is 6 by 6.
    assert!(
        d.contains("\"bounds\":{\"x\":18,\"y\":18,\"w\":6,\"h\":6}"),
        "{d}"
    );
    assert!(d.contains("\"pivot\":{\"x\":3,\"y\":6}"), "{d}");
    assert!(!d.contains("\"out\""), "{d}");
}
