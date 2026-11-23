//! Swatch order, sorting keys and groups of the colour table.

use super::*;

const RED: u32 = rgba(255, 0, 0, 255);
const YELLOW: u32 = rgba(255, 255, 0, 255);
const GREEN: u32 = rgba(0, 255, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const BLACK: u32 = rgba(0, 0, 0, 255);
const GREY: u32 = rgba(128, 128, 128, 255);

fn table(colors: &[u32]) -> ColorTable {
    let mut t = ColorTable::new();
    for &c in colors {
        t.add_swatch(c).expect("room");
    }
    t
}

fn shown(t: &ColorTable) -> Vec<u32> {
    t.swatches().iter().map(|&id| t.color(id)).collect()
}

fn group(name: &str, ids: &[u32]) -> PaletteGroup {
    PaletteGroup {
        name: name.into(),
        kind: GroupKind::Group,
        ids: ids.to_vec(),
    }
}

#[test]
fn moving_a_swatch_changes_only_the_order() {
    let mut t = table(&[RED, GREEN, BLUE]);
    let (colors, version) = (t.colors().to_vec(), t.version());
    assert!(t.move_swatch(0, 2));
    assert_eq!(shown(&t), [GREEN, BLUE, RED]);
    assert_eq!(t.colors(), colors, "ids and colours stay");
    assert_eq!(t.version(), version, "nothing repaints");
    assert!(t.move_swatch(2, 0));
    assert_eq!(shown(&t), [RED, GREEN, BLUE]);
}

#[test]
fn moving_nowhere_or_past_the_end_changes_nothing() {
    let mut t = table(&[RED, GREEN]);
    assert!(!t.move_swatch(1, 1));
    assert!(!t.move_swatch(0, 2));
    assert!(!t.move_swatch(5, 0));
    assert_eq!(shown(&t), [RED, GREEN]);
}

#[test]
fn sorting_by_hue_gives_the_wheel() {
    let mut t = table(&[BLUE, GREEN, GREY, YELLOW, RED, BLACK]);
    let colors = t.colors().to_vec();
    let order = t.sorted_swatches(|id| hue_order(colors[id as usize]));
    t.set_swatch_order(order);
    assert_eq!(shown(&t), [BLACK, GREY, RED, YELLOW, GREEN, BLUE]);
}

/// A colour picked from every corner and edge of the cube as well as at random.
fn some_colors(rng: &mut u64) -> Vec<u32> {
    let mut next = || {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng
    };
    let ends = [0u8, 1, 127, 128, 254, 255];
    let mut out = Vec::new();
    for &r in &ends {
        for &g in &ends {
            for &b in &ends {
                out.push(rgba(r, g, b, 255));
            }
        }
    }
    out.extend((0..4000).map(|_| next() as u32 | 0xFF00_0000));
    out
}

#[test]
fn packed_sort_keys_order_colours_as_the_pairs_they_pack() {
    let mut rng = 0x9E37_79B9_7F4A_7C15;
    let colors = some_colors(&mut rng);
    let n = colors.len();
    for (i, &a) in colors.iter().enumerate() {
        let b = colors[(i * 7919 + 13) % n];
        let hue = |c| (hue_key(c), lightness_key(c));
        let light = |c| (lightness_key(c), hue_key(c));
        assert_eq!(
            hue_order(a).cmp(&hue_order(b)),
            hue(a).cmp(&hue(b)),
            "{a:08x} {b:08x}"
        );
        assert_eq!(
            light_order(a).cmp(&light_order(b)),
            light(a).cmp(&light(b)),
            "{a:08x} {b:08x}"
        );
    }
    let mut by_hue = colors.clone();
    by_hue.sort_by_key(|&c| (hue_key(c), lightness_key(c)));
    let mut packed = colors.clone();
    packed.sort_by_key(|&c| hue_order(c));
    assert_eq!(by_hue, packed);
    by_hue.sort_by_key(|&c| (lightness_key(c), hue_key(c)));
    packed.sort_by_key(|&c| light_order(c));
    assert_eq!(by_hue, packed);
}

#[test]
fn sorting_by_lightness_is_darkest_first_and_ties_go_by_id() {
    let twin = rgba(10, 10, 10, 255);
    let mut t = table(&[GREY, BLACK, twin, BLUE]);
    let colors = t.colors().to_vec();
    let order = t.sorted_swatches(|id| lightness_key(colors[id as usize]).into());
    t.set_swatch_order(order);
    // Black, the near black one, blue, then grey.
    assert_eq!(shown(&t), [BLACK, twin, BLUE, GREY]);
    let again = t.sorted_swatches(|id| lightness_key(colors[id as usize]).into());
    assert_eq!(
        again,
        t.swatches(),
        "an ordered palette is not changed by sorting"
    );
}

#[test]
fn equal_keys_keep_id_order_whatever_the_start() {
    let mut a = table(&[RED, GREEN, BLUE]);
    let mut b = table(&[RED, GREEN, BLUE]);
    b.move_swatch(0, 2);
    let (oa, ob) = (a.sorted_swatches(|_| 0), b.sorted_swatches(|_| 0));
    a.set_swatch_order(oa);
    b.set_swatch_order(ob);
    assert_eq!(a.swatches(), b.swatches());
    assert_eq!(a.swatches(), [1, 2, 3]);
}

#[test]
fn hue_keys_are_whole_numbers_in_range() {
    assert_eq!(hue_key(RED), (1, 0));
    assert_eq!(hue_key(YELLOW), (1, 1024));
    assert_eq!(hue_key(GREEN), (1, 2048));
    assert_eq!(hue_key(BLUE), (1, 4096));
    assert_eq!(hue_key(rgba(255, 0, 255, 255)), (1, 5120));
    assert_eq!(hue_key(rgba(255, 0, 1, 255)), (1, 6139));
    assert_eq!(hue_key(GREY), (0, 0));
    assert_eq!(lightness_key(BLACK), 0);
    assert_eq!(lightness_key(rgba(255, 255, 255, 255)), 2_550_000);
}

#[test]
fn removing_a_swatch_takes_it_out_of_its_groups() {
    let mut t = table(&[RED, GREEN, BLUE]);
    t.push_group(group("warm", &[1, 2]));
    t.remove_swatch(2);
    assert_eq!(t.groups()[0].ids, [1]);
    assert_eq!(t.groups()[0].name, "warm", "the group stays, even if empty");
}

#[test]
fn set_groups_drops_what_names_no_swatch_and_says_so() {
    let mut t = table(&[RED, GREEN]);
    let dropped = t.set_groups(vec![group("a", &[1, 2]), group("b", &[2, 9, 2, 0])]);
    assert!(dropped);
    assert_eq!(t.groups()[0].ids, [1, 2]);
    assert_eq!(
        t.groups()[1].ids,
        [2],
        "past the table, id 0 and repeats go"
    );
    assert!(!t.set_groups(vec![group("a", &[1])]));
}

#[test]
fn set_groups_keeps_at_most_the_limit_and_cuts_names() {
    let mut t = table(&[RED]);
    let many: Vec<PaletteGroup> = (0..MAX_GROUPS + 5).map(|_| group("g", &[1])).collect();
    assert!(t.set_groups(many));
    assert_eq!(t.groups().len(), MAX_GROUPS);
    let long = "x".repeat(MAX_GROUP_NAME + 20);
    t.set_groups(vec![group(&long, &[1])]);
    assert_eq!(t.groups()[0].name.chars().count(), MAX_GROUP_NAME);
}

#[test]
fn groups_are_part_of_what_a_kept_table_costs() {
    let mut t = table(&[RED, GREEN]);
    let bare = t.kept_bytes();
    t.set_groups(vec![group(&"n".repeat(40), &[1, 2]); 10]);
    assert!(t.kept_bytes() >= bare + 10 * (40 + 8));
}

#[test]
fn a_table_without_groups_is_equal_to_one_never_given_any() {
    let mut t = table(&[RED]);
    let plain = t.clone();
    t.set_groups(vec![group("a", &[1])]);
    t.set_groups(Vec::new());
    assert_eq!(t, plain);
}

#[test]
fn group_names_lose_direction_and_zero_width_characters_to_spaces() {
    assert_eq!(clean_group_name("Sk\u{202E}in\u{200B}s"), "Sk in s");
    assert_eq!(clean_group_name("Skins"), "Skins");
}
