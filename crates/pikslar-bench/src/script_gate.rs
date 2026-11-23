//! The scripting gate: `script-measure`, `script-check` and `script-update`.
//!
//! Times are held to `perf/script-baseline.txt` by a ratio, as the other gates are; hostile
//! stop times and peaks are also held to absolute ceilings in `perf/script-limits.txt`
//! (`name hard ceiling unit`, as `perf/limits.txt`). Both files are only ever appended to by
//! `script-update`: a line that exists is never rewritten. The files of the other gates are
//! not read or written here.

use crate::gate::parse_baseline;
use crate::limits::{evaluate, exit_code, parse_limits, report_line};
use crate::script_cases::CASES;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// A time may read this many times its baseline, plus [`TIME_SLACK_MS`], before it fails.
/// Chosen from five whole runs on the recording machine (2026-11-17): 30 of the 35 lines
/// spread by at most 1.10 times between their fastest and slowest run; the widest were
/// script_pixels_1024_large_ms (1.24), script_timeline_ordinary_ms (1.25) and
/// script_hostile_lines_stop_ms (1.40, one slow run of five). A check keeps the best of up to
/// four measurements, so one slow run does not fail it, and 1.5 still catches a real slowdown
/// of half again.
pub const TIME_RATIO: f64 = 1.5;
/// Added to every time limit so lines of a millisecond or less are not judged by jitter.
pub const TIME_SLACK_MS: f64 = 1.0;
/// The most a hostile script may take to be stopped by its limits, on any document.
pub const STOP_CEILING_MS: f64 = 2000.0;
/// A failing check is measured again this many times in all, each in new processes, keeping
/// the best reading of each line: a real slowdown is slow every time.
const CHECK_ATTEMPTS: usize = 4;
/// `script-update` records the median of this many measurements of each line.
const UPDATE_RUNS: usize = 3;

const BASELINE_HEADER: &str = "\
# Baseline of the scripting benchmarks (`pikslar-bench script-check`). `script-update`
# appends the lines that are missing and never rewrites one that exists.
# Recorded 2026-11-17 on the development machine (Windows 11, the machine of perf/baseline.txt)
# at wip/fu-limits, release build, default limits (ScriptLimits::DEFAULT), as the first
# record of these lines: the owner asked for scripting benchmarks in the tracked performance
# gate. Each value is the median of three measurements; each measurement runs every case in a
# process of its own, with one untimed warm-up run and the median of the timed runs.
# A line passes while it is at most 1.5 times its value plus 1 ms (script_gate::TIME_RATIO).
# Spread over five runs: 30 of 35 lines within 1.10 (slowest/fastest), widest 1.40.
";

const LIMITS_HEADER: &str = "\
# Absolute ceilings of the scripting benchmarks, read by `pikslar-bench script-check`.
# name  hard|soft  ceiling  unit
# stop_ms: a hostile script is stopped by its limits within 2,000 ms on any document.
# peak_private_kb: the default script memory (16 MiB) plus twice the larger of the
# document's largest cel and what the document holds (script_bench::peak_ceiling_kb).
";

pub(crate) fn perf_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../perf")
        .join(name)
}

/// Every case, each in a new process, as one table of lines; `None` if a case failed.
pub fn measure() -> Option<BTreeMap<String, f64>> {
    let exe = std::env::current_exe().ok()?;
    let mut all = BTreeMap::new();
    for (name, _) in CASES {
        let out = Command::new(&exe)
            .args(["script-case", name])
            .output()
            .ok()?;
        if !out.status.success() {
            eprintln!("{name} failed: {}", String::from_utf8_lossy(&out.stderr));
            return None;
        }
        all.extend(parse_baseline(&String::from_utf8_lossy(&out.stdout)));
    }
    Some(all)
}

/// The most a time line may read.
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

/// The better (lower) reading of each line of two measurements.
pub fn best_of(a: &BTreeMap<String, f64>, b: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    a.iter()
        .map(|(k, &v)| (k.clone(), b.get(k).map_or(v, |&w| v.min(w))))
        .collect()
}

/// The median of each line over several measurements.
pub fn median_of(runs: &[BTreeMap<String, f64>]) -> BTreeMap<String, f64> {
    let Some(first) = runs.first() else {
        return BTreeMap::new();
    };
    first
        .keys()
        .filter_map(|k| {
            let mut v: Vec<f64> = runs.iter().filter_map(|r| r.get(k).copied()).collect();
            v.sort_by(f64::total_cmp);
            v.get(v.len() / 2).map(|&m| (k.clone(), m))
        })
        .collect()
}

/// The time lines the baseline keeps: every `_ms` line.
fn is_timed(name: &str) -> bool {
    name.ends_with("_ms")
}

/// Baseline lines to append to `existing` for the time lines it lacks.
pub fn baseline_to_append(existing: &str, measured: &BTreeMap<String, f64>) -> String {
    let have = parse_baseline(existing);
    let lines: String = measured
        .iter()
        .filter(|(k, _)| is_timed(k) && !have.contains_key(*k))
        .map(|(k, v)| format!("{k} {v:.3} ms\n"))
        .collect();
    with_header(existing, lines, BASELINE_HEADER)
}

/// Ceiling lines to append to `existing` for the hostile lines it lacks.
pub fn limits_to_append(existing: &str, measured: &BTreeMap<String, f64>) -> String {
    let have: Vec<String> = existing
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect();
    let mut lines = String::new();
    for (k, v) in measured {
        let line = match k.strip_suffix("_peak_ceiling_kb") {
            Some(case) => format!("{case}_peak_private_kb hard {v:.0} KB"),
            None if k.starts_with("script_hostile_") && k.ends_with("_stop_ms") => {
                format!("{k} hard {STOP_CEILING_MS:.0} ms")
            }
            None => continue,
        };
        let name = line.split(' ').next().unwrap_or_default();
        if !have.iter().any(|h| h == name) {
            lines.push_str(&line);
            lines.push('\n');
        }
    }
    with_header(existing, lines, LIMITS_HEADER)
}

pub(crate) fn with_header(existing: &str, mut lines: String, header: &str) -> String {
    if lines.is_empty() {
        return lines;
    }
    if existing.is_empty() {
        lines.insert_str(0, header);
    } else if !existing.ends_with('\n') {
        lines.insert(0, '\n');
    }
    lines
}

/// `script-measure`, `script-check` or `script-update`.
pub fn run(mode: &str) -> ExitCode {
    let Some(first) = measure() else {
        return ExitCode::FAILURE;
    };
    for (k, v) in &first {
        println!("{k:<48} {v:>12.3}");
    }
    match mode {
        "script-measure" => ExitCode::SUCCESS,
        "script-update" => update(first),
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
            "script-baseline.txt",
            baseline_to_append(&read("script-baseline.txt"), &median),
        ),
        (
            "script-limits.txt",
            limits_to_append(&read("script-limits.txt"), &median),
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

pub(crate) fn append(name: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(perf_file(name))
        .and_then(|mut file| file.write_all(text.as_bytes()))
}

/// Failures of `measured` against both files, and the exit code of the ceilings.
fn judge(
    measured: &BTreeMap<String, f64>,
    baseline: &BTreeMap<String, f64>,
    limits: &str,
) -> (Vec<String>, u8) {
    let mut failures = regressions(measured, baseline);
    match parse_limits(limits) {
        Ok(limits) => {
            let outcomes = evaluate(&limits, measured);
            let code = exit_code(&outcomes);
            if code != 0 {
                failures.extend(
                    outcomes
                        .iter()
                        .map(report_line)
                        .filter(|l| !l.starts_with("ok")),
                );
            }
            (failures, code)
        }
        Err(e) => {
            failures.push(format!("perf/script-limits.txt: {e}"));
            (failures, 2)
        }
    }
}

fn check(mut measured: BTreeMap<String, f64>) -> ExitCode {
    let (base_text, limits) = (read("script-baseline.txt"), read("script-limits.txt"));
    if base_text.is_empty() || limits.is_empty() {
        eprintln!("No scripting baseline or limits yet (run script-update).");
        return ExitCode::FAILURE;
    }
    let baseline = parse_baseline(&base_text);
    for _ in 1..CHECK_ATTEMPTS {
        if judge(&measured, &baseline, &limits).0.is_empty() {
            break;
        }
        let Some(again) = measure() else {
            break;
        };
        measured = best_of(&measured, &again);
    }
    let (failures, _) = judge(&measured, &baseline, &limits);
    if failures.is_empty() {
        println!("No regressions; every ceiling met.");
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
    fn times_pass_within_the_ratio_and_fail_beyond_it() {
        let base = table(&[("script_a_ms", 100.0)]);
        assert!(regressions(&table(&[("script_a_ms", 150.0)]), &base).is_empty());
        assert_eq!(
            regressions(&table(&[("script_a_ms", 152.0)]), &base).len(),
            1
        );
        assert_eq!(
            regressions(&BTreeMap::new(), &base),
            ["script_a_ms: not measured"]
        );
        // A line of well under a millisecond is not failed by jitter.
        let tiny = table(&[("script_t_ms", 0.2)]);
        assert!(regressions(&table(&[("script_t_ms", 1.0)]), &tiny).is_empty());
    }

    #[test]
    fn ceilings_fail_the_check_even_when_the_baseline_passes() {
        let base = table(&[("script_hostile_x_stop_ms", 1500.0)]);
        let limits =
            "script_hostile_x_stop_ms hard 2000 ms\nscript_hostile_x_peak_private_kb hard 100 KB\n";
        let ok = table(&[
            ("script_hostile_x_stop_ms", 1900.0),
            ("script_hostile_x_peak_private_kb", 90.0),
        ]);
        assert_eq!(judge(&ok, &base, limits), (Vec::new(), 0));
        let slow = table(&[
            ("script_hostile_x_stop_ms", 2100.0),
            ("script_hostile_x_peak_private_kb", 90.0),
        ]);
        assert_eq!(judge(&slow, &base, limits).1, 1);
        let big = table(&[
            ("script_hostile_x_stop_ms", 1000.0),
            ("script_hostile_x_peak_private_kb", 101.0),
        ]);
        let (failures, code) = judge(&big, &base, limits);
        assert_eq!(code, 1);
        assert!(
            failures.iter().any(|f| f.contains("peak_private_kb")),
            "{failures:?}"
        );
    }

    #[test]
    fn update_appends_only_missing_lines_and_never_rewrites_one() {
        let measured = table(&[
            ("script_a_ms", 1.5),
            ("script_a_peak_private_kb", 10.0),
            ("script_hostile_h_stop_ms", 300.0),
            ("script_hostile_h_peak_ceiling_kb", 4096.0),
        ]);
        let fresh = baseline_to_append("", &measured);
        assert!(fresh.starts_with('#') && fresh.contains("script_a_ms 1.500 ms\n"));
        assert!(!fresh.contains("peak"), "{fresh}");
        let existing = "# note\nscript_a_ms 9.000 ms\n";
        assert_eq!(
            baseline_to_append(existing, &measured),
            "script_hostile_h_stop_ms 300.000 ms\n"
        );
        let limits = limits_to_append("", &measured);
        assert!(
            limits.contains("script_hostile_h_stop_ms hard 2000 ms\n"),
            "{limits}"
        );
        assert!(
            limits.contains("script_hostile_h_peak_private_kb hard 4096 KB\n"),
            "{limits}"
        );
        assert!(!limits.contains("script_a_ms"), "{limits}");
        assert_eq!(limits_to_append(&limits, &measured), "");
        assert!(parse_limits(&limits).is_ok());
    }

    #[test]
    fn medians_and_bests_are_taken_line_by_line() {
        let runs = [
            table(&[("a", 3.0)]),
            table(&[("a", 1.0)]),
            table(&[("a", 2.0)]),
        ];
        assert_eq!(median_of(&runs)["a"], 2.0);
        assert_eq!(best_of(&runs[0], &runs[1])["a"], 1.0);
    }

    #[test]
    fn the_committed_files_parse_and_hold_every_hostile_ceiling() {
        let limits = parse_limits(&read("script-limits.txt")).expect("parses");
        let baseline = parse_baseline(&read("script-baseline.txt"));
        for (name, _) in baseline
            .iter()
            .filter(|(k, _)| k.starts_with("script_hostile_"))
        {
            assert!(
                limits.iter().any(|l| &l.name == name),
                "{name} has no ceiling"
            );
            let peak = name.replace("_stop_ms", "_peak_private_kb");
            assert!(
                limits.iter().any(|l| l.name == peak),
                "{peak} has no ceiling"
            );
        }
        assert!(limits
            .iter()
            .filter(|l| l.name.ends_with("_stop_ms"))
            .all(|l| l.ceiling == STOP_CEILING_MS));
    }
}
