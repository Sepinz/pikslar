//! Sorting through a few shared instances of the standard sort rather than one for every type
//! sorted: each instance is several kilobytes of program, and most sorts here are of a few
//! small numbers packed into one. Every function gives exactly the order the sort it stands
//! for would.
//!
//! The sorts of `u32`, `u64` and `u128` are the ones shared, and one sort of positions by
//! text for lists of names.

/// Sorts `items` by `key`, items of equal keys staying in their order: what `sort_by_key` gives.
#[inline(never)]
pub fn sort_by_u64<T>(items: &mut Vec<T>, key: impl Fn(&T) -> u64) {
    // The position breaks ties, so no two entries are equal and the unstable sort is stable.
    let mut order: Vec<u128> = (items.iter().enumerate())
        .map(|(at, item)| (u128::from(key(item)) << 64) | at as u128)
        .collect();
    order.sort_unstable();
    let mut slots: Vec<Option<T>> = std::mem::take(items).into_iter().map(Some).collect();
    items.extend(
        (order.iter()).filter_map(|&k| slots.get_mut(k as u64 as usize).and_then(Option::take)),
    );
}

/// Sorts `items` by the order of their packed keys, which must tell every two different items
/// apart, each key giving its item back. With equal keys only for equal items, this is the order
/// `sort_unstable` gives for the order the keys stand for.
#[inline(never)]
pub fn sort_packed<T>(items: &mut [T], pack: impl Fn(&T) -> u64, unpack: impl Fn(u64) -> T) {
    let mut keys: Vec<u64> = items.iter().map(pack).collect();
    keys.sort_unstable();
    for (item, key) in items.iter_mut().zip(keys) {
        *item = unpack(key);
    }
}

/// [`sort_packed`] for keys of up to 128 bits.
#[inline(never)]
pub fn sort_packed_wide<T>(items: &mut [T], pack: impl Fn(&T) -> u128, unpack: impl Fn(u128) -> T) {
    let mut keys: Vec<u128> = items.iter().map(pack).collect();
    keys.sort_unstable();
    for (item, key) in items.iter_mut().zip(keys) {
        *item = unpack(key);
    }
}

/// The positions of `keys` in the order of their texts, compared as `str` compares them, equal
/// texts keeping their order: one sort for every list of names, whatever holds the names.
///
/// A plain merge sort, n log n comparisons: the standard sort's instance for this comparison
/// would be more than ten times its size, for lists of names that are rarely long.
#[inline(never)]
pub fn text_order(keys: &[&str]) -> Vec<usize> {
    let len = keys.len();
    let mut order: Vec<usize> = (0..len).collect();
    let mut merged = vec![0; len];
    let mut width = 1;
    while width < len {
        for start in (0..len).step_by(2 * width) {
            let mid = (start + width).min(len);
            let end = (start + 2 * width).min(len);
            let (mut left, mut right) = (start, mid);
            for slot in &mut merged[start..end] {
                // The left run first on equal texts, so ties keep their order.
                let take_left =
                    right == end || (left < mid && keys[order[left]] <= keys[order[right]]);
                let from = if take_left { &mut left } else { &mut right };
                *slot = order[*from];
                *from += 1;
            }
        }
        std::mem::swap(&mut order, &mut merged);
        width *= 2;
    }
    order
}

/// Sorts `items` by the text `key` gives, items of equal texts staying in their order: what
/// `sort_by` on the texts gives. For names, which are unique, it is also what `sort_unstable_by`
/// gives.
pub fn sort_by_text<T>(items: &mut Vec<T>, key: impl Fn(&T) -> &str) {
    let order = text_order(&items.iter().map(key).collect::<Vec<_>>());
    let mut slots: Vec<Option<T>> = std::mem::take(items).into_iter().map(Some).collect();
    items.extend((order.iter()).filter_map(|&at| slots.get_mut(at).and_then(Option::take)));
}

const SIGN: u32 = 0x8000_0000;

/// Two signed numbers as one key, ordered as the pair is: by `a`, then by `b`.
pub const fn pair_key(a: i32, b: i32) -> u64 {
    (((a as u32 ^ SIGN) as u64) << 32) | (b as u32 ^ SIGN) as u64
}

/// The pair [`pair_key`] made the key of.
pub const fn unpair_key(key: u64) -> (i32, i32) {
    (
        ((key >> 32) as u32 ^ SIGN) as i32,
        (key as u32 ^ SIGN) as i32,
    )
}

/// A float as a key ordered as [`f32::total_cmp`] orders floats.
pub fn float_key(x: f32) -> u32 {
    let bits = x.to_bits();
    if bits & SIGN == 0 {
        bits | SIGN
    } else {
        !bits
    }
}

/// Sorts floats as `sort_by(f32::total_cmp)` does, bit for bit: floats that compare equal so
/// have the same bits.
#[inline(never)]
pub fn sort_floats(xs: &mut [f32]) {
    let mut keys: Vec<u32> = xs.iter().map(|&x| float_key(x)).collect();
    keys.sort_unstable();
    for (x, key) in xs.iter_mut().zip(keys) {
        *x = unfloat_key(key);
    }
}

/// The float, bit for bit, that [`float_key`] made the key of.
pub fn unfloat_key(key: u32) -> f32 {
    f32::from_bits(if key & SIGN != 0 { key & !SIGN } else { !key })
}

#[cfg(test)]
#[path = "sorting_tests.rs"]
mod tests;
