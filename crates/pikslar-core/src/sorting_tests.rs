use super::*;

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

#[test]
fn sorting_by_a_key_keeps_ties_in_their_order_as_the_stable_sort_does() {
    let mut seed = 7;
    let items: Vec<(u8, u32)> = (0..500)
        .map(|at| ((lcg(&mut seed) % 9) as u8, at))
        .collect();
    let mut want = items.clone();
    want.sort_by_key(|&(key, _)| key);
    let mut got = items;
    sort_by_u64(&mut got, |&(key, _)| u64::from(key));
    assert_eq!(got, want);
}

#[test]
fn sorting_by_a_key_moves_items_that_cannot_be_copied() {
    let mut got = vec![vec![3], vec![1, 9], vec![2], vec![1]];
    sort_by_u64(&mut got, |v| u64::from(v.iter().sum::<u32>() % 3));
    assert_eq!(got, vec![vec![3], vec![1, 9], vec![1], vec![2]]);
}

#[test]
fn packed_sorts_give_the_order_of_the_items_own_sort() {
    let mut seed = 11;
    let pairs: Vec<(i32, i32)> = (0..400)
        .map(|_| {
            (
                (lcg(&mut seed) as i32) % 50 - 25,
                (lcg(&mut seed) as i32) % 7 - 3,
            )
        })
        .chain([(i32::MIN, i32::MAX), (i32::MAX, i32::MIN), (0, -1), (-1, 0)])
        .collect();
    let mut want = pairs.clone();
    want.sort_unstable();
    let mut got = pairs.clone();
    sort_packed(&mut got, |&(a, b)| pair_key(a, b), unpair_key);
    assert_eq!(got, want);
    let mut wide = pairs;
    sort_packed_wide(
        &mut wide,
        |&(a, b)| u128::from(pair_key(a, b)) << 64,
        |k| unpair_key((k >> 64) as u64),
    );
    assert_eq!(wide, want);
}

#[test]
fn pair_keys_order_as_the_pairs_do_and_give_them_back() {
    let values = [i32::MIN, -70_000, -1, 0, 1, 65_536, i32::MAX];
    for &a in &values {
        for &b in &values {
            assert_eq!(unpair_key(pair_key(a, b)), (a, b));
            for &c in &values {
                for &d in &values {
                    assert_eq!(pair_key(a, b).cmp(&pair_key(c, d)), (a, b).cmp(&(c, d)));
                }
            }
        }
    }
}

#[test]
fn float_keys_order_as_total_cmp_does_and_give_the_same_bits_back() {
    let values = [
        f32::NEG_INFINITY,
        -2.5,
        -0.0,
        0.0,
        1e-40,
        3.25,
        f32::INFINITY,
        f32::NAN,
        -f32::NAN,
    ];
    for &a in &values {
        assert_eq!(unfloat_key(float_key(a)).to_bits(), a.to_bits());
        for &b in &values {
            assert_eq!(float_key(a).cmp(&float_key(b)), a.total_cmp(&b));
        }
    }
}

#[test]
fn floats_sort_as_by_total_cmp_bit_for_bit() {
    let mut seed = 3;
    let mut xs: Vec<f32> = (0..300)
        .map(|_| (lcg(&mut seed) as f32 - 1.0e9) / 7.0)
        .chain([-0.0, 0.0, f32::NAN, -f32::INFINITY, 0.0, -0.0])
        .collect();
    let mut want = xs.clone();
    want.sort_by(f32::total_cmp);
    sort_floats(&mut xs);
    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&xs), bits(&want));
}

#[test]
fn texts_sort_as_strings_do_with_ties_in_their_order() {
    let mut seed = 5;
    let words = ["b", "a", "", "ab", "a\u{e9}", "B", "\u{1f600}", "aa", "a"];
    let items: Vec<(String, u32)> = (0..300)
        .map(|at| (words[(lcg(&mut seed) % 9) as usize].to_string(), at))
        .collect();
    let mut want = items.clone();
    want.sort_by(|a, b| a.0.cmp(&b.0));
    let mut got = items;
    sort_by_text(&mut got, |(text, _)| text);
    assert_eq!(got, want);
}

#[test]
fn text_order_gives_positions_by_text() {
    assert_eq!(text_order(&["c", "a", "b", "a"]), vec![1, 3, 2, 0]);
    assert!(text_order(&[]).is_empty());
}
