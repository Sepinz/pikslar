//! A 128-bit hash for content prints and record checks. Fast, not cryptographic: it finds
//! damage and tells files apart, and is no defence against someone who can write the store.

const K: [u64; 4] = [
    0x9E37_79B9_7F4A_7C15,
    0xC2B2_AE3D_27D4_EB4F,
    0x1656_67B1_9E37_79F9,
    0xD6E8_FEB8_6659_FD93,
];

fn mix(a: u64, b: u64) -> u64 {
    let m = u128::from(a) * u128::from(b);
    (m as u64) ^ ((m >> 64) as u64)
}

fn word(bytes: &[u8]) -> u64 {
    let mut w = [0u8; 8];
    w[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(w)
}

/// The hash of `bytes`, in two independent halves.
pub fn hash128(bytes: &[u8]) -> u128 {
    let (mut a, mut b) = (
        K[0] ^ bytes.len() as u64,
        K[1].rotate_left(17) ^ bytes.len() as u64,
    );
    let mut chunks = bytes.chunks_exact(16);
    for chunk in &mut chunks {
        let (x, y) = (word(&chunk[..8]), word(&chunk[8..]));
        a = mix(x ^ K[0], y ^ a);
        b = mix(y ^ K[2], x ^ b);
    }
    let rest = chunks.remainder();
    let (x, y) = (
        word(&rest[..rest.len().min(8)]),
        word(&rest[rest.len().min(8)..]),
    );
    a = mix(x ^ K[1], y ^ a ^ rest.len() as u64);
    b = mix(y ^ K[3], x ^ b);
    let (fa, fb) = (mix(a ^ K[2], b ^ K[3]), mix(b ^ K[0], a ^ K[1]));
    (u128::from(fa) << 64) | u128::from(fb)
}

/// The check written after records and heads: the low half of [`hash128`].
pub fn check64(bytes: &[u8]) -> u64 {
    hash128(bytes) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_byte_and_the_length_count() {
        let base: Vec<u8> = (0..100u8).collect();
        let print = hash128(&base);
        for at in 0..base.len() {
            let mut flipped = base.clone();
            flipped[at] ^= 1;
            assert_ne!(hash128(&flipped), print, "byte {at}");
        }
        assert_ne!(hash128(&base[..99]), print);
        assert_ne!(hash128(&[0]), hash128(&[0, 0]));
        assert_ne!(hash128(&[]), hash128(&[0]));
        assert_eq!(hash128(&base), print, "the same bytes print the same");
    }
}
