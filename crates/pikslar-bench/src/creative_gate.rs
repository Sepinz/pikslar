//! The creative baseline file: judged like the main one, but only ever appended to.

use crate::gate::{parse_baseline, Kind, Metric};

const HEADER: &str = "# Baseline of the creative tools. `creative-update` appends the lines that are missing\n# and never rewrites one that exists. Each value is the best of three pinned release runs.\n";

fn tag(kind: Kind) -> &'static str {
    match kind {
        Kind::Micros => "us",
        Kind::Bytes => "bytes",
        Kind::Allocs => "allocs",
        Kind::Percent => "%",
    }
}

/// What to append to `existing` (the file's text, or empty if there is none yet) so that every
/// metric has a line. Lines already there are left alone, whatever they say. Empty when nothing
/// is missing.
pub fn lines_to_append(existing: &str, metrics: &[Metric]) -> String {
    let have = parse_baseline(existing);
    let mut out = String::new();
    for m in metrics.iter().filter(|m| !have.contains_key(&m.name)) {
        out.push_str(&format!("{} {:.1} {}\n", m.name, m.value, tag(m.kind)));
    }
    if out.is_empty() {
        return out;
    }
    if existing.is_empty() {
        out.insert_str(0, HEADER);
    } else if !existing.ends_with('\n') {
        out.insert(0, '\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric(name: &str, value: f64) -> Metric {
        Metric {
            name: name.into(),
            kind: Kind::Micros,
            value,
        }
    }

    #[test]
    fn a_new_file_gets_a_header_and_every_line() {
        let text = lines_to_append("", &[metric("a", 1.0), metric("b", 2.0)]);
        assert!(text.starts_with('#'));
        assert!(text.contains("a 1.0 us\n") && text.contains("b 2.0 us\n"));
    }

    #[test]
    fn existing_lines_are_never_touched_and_only_missing_ones_are_added() {
        let existing = "# note\na 5.0 us\n";
        let text = lines_to_append(existing, &[metric("a", 99.0), metric("b", 2.0)]);
        assert_eq!(text, "b 2.0 us\n");
    }

    #[test]
    fn nothing_missing_means_nothing_to_write() {
        assert_eq!(lines_to_append("a 5.0 us\n", &[metric("a", 1.0)]), "");
    }

    #[test]
    fn a_file_without_a_final_newline_is_ended_before_appending() {
        assert_eq!(
            lines_to_append("a 5.0 us", &[metric("b", 2.0)]),
            "\nb 2.0 us\n"
        );
    }
}
