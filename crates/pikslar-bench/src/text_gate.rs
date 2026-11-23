//! The text-typing gate: `text-measure`, `text-check` and `text-update`.
//!
//! SYNTHETIC timing. It times, in one process, the path of one key in a text session
//! (`text_study::type_text`): `rasterize`, the swap of the floating pixels and the screen-sized
//! preview made again. There is no input path, no message loop and no presentation, so it is
//! not the latency a person feels; physical-input latency is measured by the latency harness
//! (`spikes/run-latency.ps1`) and is not this. The target stays the 3 ms of drawing.
//!
//! Lines are held to `perf/text-baseline.txt` by a ratio, and to the absolute ceilings in
//! `perf/text-limits.txt` (`name hard ceiling unit`). Both files are only appended to by
//! `text-update`: a line that exists is never rewritten.

use crate::gate::parse_baseline;
use crate::limits::{evaluate, exit_code, parse_limits, report_line};
use crate::script_gate::{append, best_of, median_of, perf_file, with_header};
use crate::text_study::type_text;
use std::collections::BTreeMap;
use std::process::{Command, ExitCode};

/// A line passes while it is at most this many times its baseline, plus [`TIME_SLACK_MS`].
pub const TIME_RATIO: f64 = 1.5;
/// Added to every limit, so a reading is not judged by jitter alone.
pub const TIME_SLACK_MS: f64 = 0.25;
/// The most a key may take, median or worst, as drawing is held to.
pub const KEY_CEILING_MS: f64 = 3.0;
/// The text sizes measured; 8 is the heaviest ordinary case.
const SCALES: [i32; 4] = [1, 2, 4, 8];
/// Sessions timed per size in one measurement, after one untimed warm-up session.
const SESSIONS: usize = 5;
/// A failing check is measured again this many times in all, keeping the best of each line.
const CHECK_ATTEMPTS: usize = 4;
/// `text-update` records the median of this many measurements, each in a process of its own.
const UPDATE_RUNS: usize = 5;

const BASELINE_HEADER: &str = "\
# Baseline of typing into the text tool (`pikslar-bench text-check`). SYNTHETIC timing: one
# process, the path of one key from `rasterize` to the pixels ready to present, with no input,
# message loop or presentation. It is not physical-input latency (see the latency harness).
# `text-update` appends the lines that are missing and never rewrites one that exists.
# Recorded 2026-11-19 on the development machine (Windows 11, the machine of perf/baseline.txt)
# at wip/gauge, release build, otherwise idle. 64 keys per session at text sizes 1, 2, 4 and 8
# on a 512 canvas; key_median is over every key of five sessions after a warm-up session,
# key_worst is the median over those sessions of the slowest key. Each value is the median of
# five measurements, each in a process of its own.
# A line passes while it is at most 1.5 times its value plus 0.25 ms (text_gate::TIME_RATIO).
";

const LIMITS_HEADER: &str = "\
# Absolute ceilings of the text-typing benchmark, read by `pikslar-bench text-check`.
# name  hard|soft  ceiling  unit
# SYNTHETIC timing (in-process, no input or presentation path), not physical-input latency.
# A key takes at most 3 ms, median and worst, at every size: the target of drawing.
";

fn key_name(scale: i32, what: &str) -> String {
    format!("text_synthetic_s{scale}_key_{what}_ms")
}

/// The median key over every session, and the median over sessions of the slowest key, as lines.
fn summarize(scale: i32, sessions: &[Vec<f64>]) -> BTreeMap<String, f64> {
    let mut all: Vec<f64> = sessions.iter().flatten().copied().collect();
    all.sort_by(f64::total_cmp);
    let mut worst: Vec<f64> = sessions
        .iter()
        .map(|s| s.iter().copied().fold(0.0, f64::max))
        .collect();
    worst.sort_by(f64::total_cmp);
    let mid = |v: &[f64]| v.get(v.len() / 2).copied().unwrap_or(f64::NAN) / 1e3;
    BTreeMap::from([
        (key_name(scale, "median"), mid(&all)),
        (key_name(scale, "worst"), mid(&worst)),
    ])
}

/// One measurement of every size, in this process.
pub fn measure_here() -> BTreeMap<String, f64> {
    let mut lines = BTreeMap::new();
    for scale in SCALES {
        drop(type_text(scale));
        let sessions: Vec<Vec<f64>> = (0..SESSIONS).map(|_| type_text(scale).0).collect();
        lines.extend(summarize(scale, &sessions));
    }
    lines
}

/// One measurement in a new process; `None` if it failed.
fn measure() -> Option<BTreeMap<String, f64>> {
    let exe = std::env::current_exe().ok()?;
    let out = Command::new(exe).arg("text-case").output().ok()?;
    if !out.status.success() {
        eprintln!("text-case failed: {}", String::from_utf8_lossy(&out.stderr));
        return None;
    }
    Some(parse_baseline(&String::from_utf8_lossy(&out.stdout)))
}

/// `text-case`: print one measurement, for the parent process to read.
pub fn run_case() -> ExitCode {
    for (k, v) in measure_here() {
        println!("{k} {v:.4}");
    }
    ExitCode::SUCCESS
}

/// The most a line may read.
pub fn time_limit(baseline: f64) -> f64 {
    baseline * TIME_RATIO + TIME_SLACK_MS
}

/// Each baseline line that is over its limit or was not measured.
pub fn regressions(
    measured: &BTreeMap<String, f64>,
    baseline: &BTreeMap<String, f64>,
) -> Vec<String> {
    baseline
        .iter()
        .filter_map(|(name, &base)| match measured.get(name) {
            None => Some(format!("{name}: not measured")),
            Some(&v) if v.is_nan() || v > time_limit(base) => Some(format!(
                "{name}: {v:.3} ms is over the limit of {:.3} (baseline {base:.3})",
                time_limit(base)
            )),
            Some(_) => None,
        })
        .collect()
}

/// Baseline lines to append to `existing` for the lines it lacks.
pub fn baseline_to_append(existing: &str, measured: &BTreeMap<String, f64>) -> String {
    let have = parse_baseline(existing);
    let lines: String = measured
        .iter()
        .filter(|(k, _)| !have.contains_key(*k))
        .map(|(k, v)| format!("{k} {v:.3} ms\n"))
        .collect();
    with_header(existing, lines, BASELINE_HEADER)
}

/// Ceiling lines to append to `existing` for the lines it lacks.
pub fn limits_to_append(existing: &str, measured: &BTreeMap<String, f64>) -> String {
    let have: Vec<&str> = existing
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    let lines: String = measured
        .keys()
        .filter(|k| !have.contains(&k.as_str()))
        .map(|k| format!("{k} hard {KEY_CEILING_MS:.0} ms\n"))
        .collect();
    with_header(existing, lines, LIMITS_HEADER)
}

/// `text-measure`, `text-check` or `text-update`.
pub fn run(mode: &str) -> ExitCode {
    let Some(first) = measure() else {
        return ExitCode::FAILURE;
    };
    for (k, v) in &first {
        println!("{k:<40} {v:>9.4}");
    }
    match mode {
        "text-measure" => ExitCode::SUCCESS,
        "text-update" => update(first),
        _ => check(first),
    }
}

fn read(name: &str) -> String {
    std::fs::read_to_string(perf_file(name)).unwrap_or_default()
}

fn update(first: BTreeMap<String, f64>) -> ExitCode {
    let mut runs = vec![first];
    while runs.len() < UPDATE_RUNS {
        match measure() {
            Some(again) => runs.push(again),
            None => return ExitCode::FAILURE,
        }
    }
    let median = median_of(&runs);
    let appended = [
        (
            "text-baseline.txt",
            baseline_to_append(&read("text-baseline.txt"), &median),
        ),
        (
            "text-limits.txt",
            limits_to_append(&read("text-limits.txt"), &median),
        ),
    ];
    for (name, text) in appended {
        if let Err(e) = append(name, &text) {
            eprintln!("Could not append to perf/{name}: {e}");
            return ExitCode::FAILURE;
        }
        let count = text.lines().filter(|l| !l.starts_with('#')).count();
        println!("perf/{name}: {count} lines appended.");
    }
    ExitCode::SUCCESS
}

/// Failures of `measured` against both files.
fn judge(
    measured: &BTreeMap<String, f64>,
    baseline: &BTreeMap<String, f64>,
    limits: &str,
) -> Vec<String> {
    let mut failures = regressions(measured, baseline);
    match parse_limits(limits) {
        Ok(limits) => {
            let outcomes = evaluate(&limits, measured);
            if exit_code(&outcomes) != 0 {
                failures.extend(
                    outcomes
                        .iter()
                        .map(report_line)
                        .filter(|l| !l.starts_with("ok")),
                );
            }
        }
        Err(e) => failures.push(format!("perf/text-limits.txt: {e}")),
    }
    failures
}

fn check(mut measured: BTreeMap<String, f64>) -> ExitCode {
    let (base_text, limits) = (read("text-baseline.txt"), read("text-limits.txt"));
    if base_text.is_empty() || limits.is_empty() {
        eprintln!("No text baseline or limits yet (run text-update).");
        return ExitCode::FAILURE;
    }
    let baseline = parse_baseline(&base_text);
    for _ in 1..CHECK_ATTEMPTS {
        if judge(&measured, &baseline, &limits).is_empty() {
            break;
        }
        let Some(again) = measure() else {
            break;
        };
        measured = best_of(&measured, &again);
    }
    let failures = judge(&measured, &baseline, &limits);
    if failures.is_empty() {
        println!("No regressions; every ceiling met (synthetic timing).");
        return ExitCode::SUCCESS;
    }
    failures.iter().for_each(|f| eprintln!("REGRESSION  {f}"));
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|&(k, v)| (k.to_string(), v)).collect()
    }

    #[test]
    fn a_measurement_has_a_median_and_a_worst_line_per_size_in_milliseconds() {
        let sessions = vec![vec![1000.0, 2000.0, 3000.0], vec![500.0, 900.0, 4000.0]];
        let lines = summarize(8, &sessions);
        // Median of the six keys (sorted 500 900 1000 2000 3000 4000, index 3), worst is the
        // median of the session maxima (3000, 4000; index 1).
        assert_eq!(lines["text_synthetic_s8_key_median_ms"], 2.0);
        assert_eq!(lines["text_synthetic_s8_key_worst_ms"], 4.0);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn measure_here_covers_every_size_under_the_ceiling_names() {
        let lines = measure_here();
        assert_eq!(lines.len(), SCALES.len() * 2);
        assert!(lines.values().all(|&v| v > 0.0 && v.is_finite()));
        assert!(lines.keys().all(|k| k.starts_with("text_synthetic_s")));
    }

    #[test]
    fn times_pass_within_the_ratio_and_fail_beyond_it() {
        let name = "text_synthetic_s8_key_median_ms";
        let base = table(&[(name, 0.8)]);
        assert!(regressions(&table(&[(name, 1.45)]), &base).is_empty());
        assert_eq!(regressions(&table(&[(name, 1.46)]), &base).len(), 1);
        assert_eq!(
            regressions(&BTreeMap::new(), &base),
            [format!("{name}: not measured")]
        );
        assert_eq!(regressions(&table(&[(name, f64::NAN)]), &base).len(), 1);
    }

    #[test]
    fn the_ceiling_fails_the_check_even_when_the_baseline_passes() {
        let name = "text_synthetic_s8_key_worst_ms";
        let base = table(&[(name, 2.9)]);
        let limits = "text_synthetic_s8_key_worst_ms hard 3 ms\n";
        assert!(judge(&table(&[(name, 3.0)]), &base, limits).is_empty());
        let failures = judge(&table(&[(name, 3.2)]), &base, limits);
        assert!(
            failures.iter().any(|f| f.contains("OVER LIMIT")),
            "{failures:?}"
        );
        assert!(!judge(&table(&[(name, 1.0)]), &base, "bad line\n").is_empty());
    }

    #[test]
    fn update_appends_only_missing_lines_and_never_rewrites_one() {
        let measured = table(&[
            ("text_synthetic_s1_key_median_ms", 0.1),
            ("text_synthetic_s1_key_worst_ms", 0.5),
        ]);
        let fresh = baseline_to_append("", &measured);
        assert!(fresh.starts_with('#') && fresh.contains("SYNTHETIC"));
        assert!(fresh.contains("text_synthetic_s1_key_median_ms 0.100 ms\n"));
        let existing = "# note\ntext_synthetic_s1_key_median_ms 9.000 ms\n";
        assert_eq!(
            baseline_to_append(existing, &measured),
            "text_synthetic_s1_key_worst_ms 0.500 ms\n"
        );
        let limits = limits_to_append("", &measured);
        assert!(limits.contains("text_synthetic_s1_key_median_ms hard 3 ms\n"));
        assert!(limits.contains("text_synthetic_s1_key_worst_ms hard 3 ms\n"));
        assert_eq!(limits_to_append(&limits, &measured), "");
        assert_eq!(baseline_to_append(&fresh, &measured), "");
        assert!(parse_limits(&limits).is_ok());
    }

    #[test]
    fn the_committed_files_agree_and_keep_the_three_millisecond_ceiling() {
        let limits = parse_limits(&read("text-limits.txt")).expect("parses");
        let baseline = parse_baseline(&read("text-baseline.txt"));
        assert_eq!(baseline.len(), SCALES.len() * 2, "{baseline:?}");
        for name in baseline.keys() {
            assert!(limits.iter().any(|l| &l.name == name), "{name} no ceiling");
        }
        assert!(limits
            .iter()
            .all(|l| l.ceiling == KEY_CEILING_MS && l.unit == "ms"));
        assert!(read("text-baseline.txt").contains("SYNTHETIC"));
    }
}
