//! Comparing documents, and naming the first thing that differs.

use pikslar_core::{Document, Layer};

/// The first difference between `a` and the later state `b`.
///
/// Layer identity is not part of a document. Unused entries at the end of the color table change
/// nothing anyone can see, so they are not a difference here. The undo model holds tables to
/// exact equality itself.
pub fn first_difference(a: &Document, b: &Document) -> Option<String> {
    if (a.width, a.height) != (b.width, b.height) {
        return Some("size".to_owned());
    }
    if a.frames != b.frames {
        return Some("frame timings".to_owned());
    }
    if a.frame_rate != b.frame_rate {
        return Some("frame rate".to_owned());
    }
    if a.blending != b.blending {
        return Some("blending".to_owned());
    }
    if a.tags != b.tags {
        return Some("tags".to_owned());
    }
    if a.slices != b.slices {
        return Some("slices".to_owned());
    }
    if a.foreign != b.foreign {
        return Some("foreign sections".to_owned());
    }
    if a.colors.swatches() != b.colors.swatches() {
        return Some("swatches".to_owned());
    }
    if a.colors.groups() != b.colors.groups() {
        return Some("palette groups".to_owned());
    }
    if a.tilesets != b.tilesets {
        return Some("tilesets".to_owned());
    }
    let (older, newer) = (a.colors.colors(), b.colors.colors());
    if !newer.starts_with(older) {
        return Some(format!(
            "color table ({} against {} colors)",
            a.colors.len(),
            b.colors.len()
        ));
    }
    if a.layers().len() != b.layers().len() {
        return Some("layer count".to_owned());
    }
    a.layers()
        .iter()
        .zip(b.layers())
        .enumerate()
        .find(|(_, (x, y))| x != y)
        .map(|(index, (x, y))| format!("layer {index} {}", layer_part(x, y)))
}

/// A copy with its holds numbered the way a freshly opened file's are.
///
/// Saving keeps which frames are held together and not the numbers, so a document and its saved
/// copy are compared through this. A document breaking a rule of the links is refused, naming
/// the rule, so that renumbering can never hide a break: two documents broken alike would
/// otherwise compare equal once renumbering had taken the broken links apart.
pub fn with_holds_numbered_afresh(doc: &Document) -> Result<Document, String> {
    doc.check_links()
        .map_err(|fault| format!("broke a rule of the links: {fault}"))?;
    let mut copy = doc.clone();
    // Numbered by first appearance, from the bottom layer up, so that equal links get equal
    // numbers whatever the ids were.
    let mut numbers: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let links: Vec<Vec<u32>> = doc
        .layers()
        .iter()
        .map(|layer| {
            let row = layer.links().map(|id| match id {
                0 => 0,
                id => {
                    let next = numbers.len() as u32 + 1;
                    *numbers.entry(id).or_insert(next)
                }
            });
            row.collect()
        })
        .collect();
    match copy.set_links(&links) {
        Ok(0) => Ok(copy),
        Ok(broken) => Err(format!("renumbering took {broken} links apart")),
        Err(error) => Err(format!("renumbering refused: {error}")),
    }
}

fn layer_part(a: &Layer, b: &Layer) -> String {
    if a.name != b.name {
        return format!("name ({:?} against {:?})", a.name, b.name);
    }
    if a.links().ne(b.links()) {
        let (left, right): (Vec<u32>, Vec<u32>) = (a.links().collect(), b.links().collect());
        return format!("holds ({left:?} against {right:?})");
    }
    if a.cels() != b.cels() {
        return "pixels".to_owned();
    }
    if (a.kind, a.depth, a.folded) != (b.kind, b.depth, b.folded) {
        let (left, right) = ((a.kind, a.depth, a.folded), (b.kind, b.depth, b.folded));
        return format!("kind, depth and fold ({left:?} against {right:?})");
    }
    "settings".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{rgba, ColorMode, ColorTable};
    use std::sync::Arc;

    fn doc() -> Document {
        Document::new(4, 4, ColorMode::Palette).expect("valid")
    }

    fn with_colors(colors: &[u32]) -> Document {
        let mut d = doc();
        d.colors = Arc::new(ColorTable::from_parts(colors, &[]));
        d
    }

    #[test]
    fn identical_documents_have_no_difference() {
        assert_eq!(first_difference(&doc(), &doc()), None);
    }

    #[test]
    fn the_frame_rate_is_a_difference() {
        let mut other = doc();
        other.frame_rate = Some(24);
        let found = first_difference(&doc(), &other);
        assert_eq!(found.as_deref(), Some("frame rate"));
    }

    fn slice(x: i32) -> pikslar_core::Slice {
        pikslar_core::Slice {
            name: "s".to_owned(),
            color: 0xff00_00ff,
            bounds: pikslar_core::SliceRect {
                x,
                y: 0,
                width: 2,
                height: 2,
            },
            pivot: None,
            center: None,
        }
    }

    #[test]
    fn a_slice_is_a_difference() {
        let mut other = doc();
        other.slices.push(slice(0));
        let found = first_difference(&doc(), &other);
        assert_eq!(found.as_deref(), Some("slices"));
        let mut moved = other.clone();
        moved.slices[0] = slice(1);
        assert_eq!(first_difference(&other, &moved).as_deref(), Some("slices"));
        assert_eq!(first_difference(&other, &other.clone()), None);
    }

    #[test]
    fn unused_colors_at_the_end_are_not_a_difference() {
        let (a, b) = (rgba(1, 2, 3, 255), rgba(4, 5, 6, 255));
        let earlier = with_colors(&[0, a]);
        let later = with_colors(&[0, a, b]);
        assert_eq!(first_difference(&earlier, &later), None);
    }

    #[test]
    fn a_changed_color_is_a_difference() {
        let (a, b) = (rgba(1, 2, 3, 255), rgba(4, 5, 6, 255));
        let found = first_difference(&with_colors(&[0, a]), &with_colors(&[0, b]));
        assert!(found.expect("a difference").contains("color table"));
    }

    fn two_held_frames_on_two_layers() -> Document {
        let mut d = doc();
        d.add_layer(1, ColorMode::Rgba).expect("room");
        d.add_frame(1, false).expect("room");
        d
    }

    #[test]
    fn holds_made_in_another_order_are_the_same_once_numbered_afresh() {
        let (mut a, mut b) = (
            two_held_frames_on_two_layers(),
            two_held_frames_on_two_layers(),
        );
        a.hold_previous(0, 1).expect("frames");
        a.hold_previous(1, 1).expect("frames");
        b.hold_previous(1, 1).expect("frames");
        b.hold_previous(0, 1).expect("frames");
        assert!(
            first_difference(&a, &b).is_some(),
            "the numbers really differ"
        );
        let (a, b) = (
            with_holds_numbered_afresh(&a).expect("sound"),
            with_holds_numbered_afresh(&b).expect("sound"),
        );
        assert_eq!(first_difference(&a, &b), None);
    }

    #[test]
    fn numbering_afresh_does_not_hide_a_changed_picture() {
        let mut a = two_held_frames_on_two_layers();
        a.hold_previous(0, 1).expect("frames");
        let mut b = a.clone();
        let drawn = pikslar_core::Grid::from_cells(4, 4, &[5; 16]);
        b.set_cel(1, 0, drawn).expect("fits");
        let found = first_difference(
            &with_holds_numbered_afresh(&a).expect("sound"),
            &with_holds_numbered_afresh(&b).expect("sound"),
        );
        assert!(found.expect("a difference").contains("pixels"));
    }

    #[test]
    fn numbering_afresh_refuses_a_document_breaking_a_rule_of_the_links() {
        let mut a = two_held_frames_on_two_layers();
        a.hold_previous(1, 1).expect("frames");
        a.layers_mut()[1].kind = pikslar_core::LayerKind::Group;
        let refused = with_holds_numbered_afresh(&a).expect_err("a group cel in a link");
        assert!(refused.contains("L5"), "{refused}");
    }

    #[test]
    fn timing_is_named() {
        let mut other = doc();
        other.frames[0].duration_ms = 500;
        assert_eq!(
            first_difference(&doc(), &other).as_deref(),
            Some("frame timings")
        );
    }
}
