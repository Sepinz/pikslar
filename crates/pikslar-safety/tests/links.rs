//! Hold numbers from a file are renumbered once and then stay put.

use pikslar_core::tidy_links;

fn tidied(links: &[u32]) -> Vec<u32> {
    let mut out = links.to_vec();
    tidy_links(&mut out);
    out
}

#[test]
fn a_hold_of_one_frame_leaves_no_gap_in_the_numbers() {
    assert_eq!(tidied(&[5, 7, 7]), vec![0, 1, 1]);
}

#[test]
fn numbers_are_counted_from_one_in_order() {
    assert_eq!(tidied(&[9, 9, 0, 4, 4]), vec![2, 2, 0, 1, 1]);
}

#[test]
fn tidying_twice_changes_nothing() {
    let mut x = 77u32;
    for _ in 0..2_000 {
        let links: Vec<u32> = (0..12)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 24) % 7
            })
            .collect();
        let once = tidied(&links);
        assert_eq!(tidied(&once), once, "from {links:?}");
    }
}
