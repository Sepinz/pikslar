//! Size reference: the same tiny host as `size_with`, with no interpreter. Build both with
//! `cargo build --release -p pikslar-script --examples` and compare their sizes.

fn main() {
    let src = std::env::args().nth(1).unwrap_or_default();
    let mut px = vec![0u32; 256 * 256];
    // Stand-in for running a script: touch the canvas with the source's bytes.
    for (i, b) in src.bytes().enumerate() {
        let n = px.len();
        px[i % n] = u32::from(b);
    }
    println!("{}", px.iter().map(|&p| u64::from(p)).sum::<u64>());
}
