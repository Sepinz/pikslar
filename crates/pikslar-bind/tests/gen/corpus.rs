//! Example scripts for the fuzzer to mutate, in the shapes real scripts and plugins take.

use super::{statement, Rng, Scope};
use pikslar_bind::Capabilities;

pub const CORPUS: [&str; 10] = [
    "let l = layers()[0]; let f = frames()[0];\n\
     for y in 0..height() { for x in 0..width() { set_pixel(l, f, x, y, 0xff0000ff + x); } }\n\
     return get_pixel(l, f, 3, 3);",
    "let l = layers()[0]; let f = frames()[0];\n\
     let px = read_rect(l, f, 0, 0, width(), height());\n\
     for i in 0..len(px) { if px[i] != 0 { px[i] = 0xff00ff00; } }\n\
     write_rect(l, f, 0, 0, width(), px);",
    "let n = add_layer(\"outline\"); rename_layer(n, \"edge\"); set_opacity(n, 128);\n\
     set_blend(n, \"Multiply\"); move_layer(n, 0); let d = duplicate_layer(n);\n\
     remove_layer(d); return layer_name(n) + str(layer_index(n));",
    "let a = add_frame(1); let b = copy_frame(0); set_duration(a, 250);\n\
     add_tag(\"walk\", frames()[0], b); remove_frame(a); return len(frames());",
    "let s = add_swatch(0xff112233); recolor(s, 0xff332211); print(swatch_color(s));\n\
     remove_swatch(s); return swatches();",
    "let l = layers()[0]; let f = frames()[0];\n\
     draw_line(l, f, 0, 0, width() - 1, height() - 1, 0xffffffff);\n\
     fill(l, f, 5, 0, 0xff0000ff); fill_rect(l, f, 1, 1, 4, 4, 0);\n\
     erase_pixel(l, f, 2, 3); return get_pixel(l, f, 1, 1);",
    "fn shade(c, k) { return c * k / 4; }\n\
     let row = list(16, 0); for x in 0..16 { row[x] = shade(x, 3); }\n\
     let t = \"\"; for v in row { t = t + str(v) + \",\"; } return slice(t, 0, 10);",
    "let l = layers()[1]; let f = frames()[1]; set_visible(l, false);\n\
     for i in 0..20 { set_pixel(l, f, i, i, 0xff000000 + i); }\n\
     set_visible(l, true); return layer_visible(l);",
    "let total = 0; for l in layers() { total = total + layer_opacity(l);\n\
     print(layer_kind(l) + \" \" + layer_blend(l) + \" \" + str(layer_locked(l))); }\n\
     for f in frames() { total = total + frame_duration(f); } return total;",
    "let s = repeat(\"ab\", 1000); let i = find(s, \"ba\"); let l = [];\n\
     while len(l) < 100 { push(l, i); insert(l, 0, len(l)); } remove(l, 5); pop(l);\n\
     return [min(1, 2.0), max(-1, 3), floor(2.7), round(-2.5), sqrt(16), int(\"12\")];",
];

/// Number-like or name-like runs of the source, as byte ranges.
fn words(src: &str) -> Vec<(usize, usize)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphanumeric() || b[i] == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push((start, i));
        } else {
            i += 1;
        }
    }
    out
}

/// Byte places just after a `;` or `{`, where a statement can start.
fn statement_starts(src: &str) -> Vec<usize> {
    src.bytes()
        .enumerate()
        .filter(|(_, b)| *b == b';' || *b == b'{')
        .map(|(i, _)| i + 1)
        .chain(std::iter::once(0))
        .collect()
}

const SWAPS: [&str; 12] = [
    "0",
    "-1",
    "65536",
    "9223372036854775807",
    "4294967296",
    "layers",
    "frames",
    "width",
    "while",
    "true",
    "nil",
    "height",
];

const BREAKS: [&str; 10] = ["(", ")", "{", "}", "[", "]", ";", ",", "\"", ".."];

/// One mutation of `src`.
pub fn mutate(src: &str, rng: &mut Rng, caps: Capabilities) -> String {
    let mut s = src.to_string();
    for _ in 0..1 + rng.below(3) {
        s = match rng.below(6) {
            0 | 1 => splice_statement(&s, rng, caps),
            2 | 3 => swap_word(&s, rng),
            4 => wrap_in_loop(&s, rng),
            _ => break_source(&s, rng),
        };
    }
    s
}

fn splice_statement(s: &str, rng: &mut Rng, caps: Capabilities) -> String {
    let starts = statement_starts(s);
    let at = *rng.pick(&starts);
    let mut scope = Scope::new(caps);
    let budget = 1 + rng.below(3);
    let st = statement(rng, &mut scope, budget);
    format!("{} {st} {}", &s[..at], &s[at..])
}

fn swap_word(s: &str, rng: &mut Rng) -> String {
    let ws = words(s);
    if ws.is_empty() {
        return s.to_string();
    }
    let (a, b) = *rng.pick(&ws);
    format!("{}{}{}", &s[..a], rng.pick(&SWAPS), &s[b..])
}

fn wrap_in_loop(s: &str, rng: &mut Rng) -> String {
    let head = rng.pick(&[
        "while true { ",
        "for q in 0..100000 { ",
        "for q in layers() { ",
    ]);
    format!("{head}{s}\n}}")
}

fn break_source(s: &str, rng: &mut Rng) -> String {
    let mut at = rng.below(s.len() + 1);
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    match rng.chance(50) {
        true => format!("{}{}{}", &s[..at], rng.pick(&BREAKS), &s[at..]),
        false => {
            let mut end = (at + 1 + rng.below(8)).min(s.len());
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}{}", &s[..at], &s[end..])
        }
    }
}
