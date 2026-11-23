//! Absolute ceilings read from the built program, unlike the baseline, which is relative.
//!
//! A ratio against a baseline lets the program get slower a little at a time. These do not.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Going over fails the gate.
    Hard,
    /// Going over warns and needs a written note. It never fails the gate.
    Soft,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    pub name: String,
    pub severity: Severity,
    pub ceiling: f64,
    pub unit: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Within,
    Over,
    Warning,
    NotMeasured,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub limit: Limit,
    pub value: Option<f64>,
    pub verdict: Verdict,
}

/// One limit per line: `name hard|soft ceiling unit`. A malformed line is an error, because a
/// limit that silently vanished would look exactly like a limit that passed.
pub fn parse_limits(text: &str) -> Result<Vec<Limit>, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> Result<Limit, String> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [name, severity, ceiling, unit] = fields[..] else {
        return Err(format!("expected four fields: {line}"));
    };
    let severity = match severity {
        "hard" => Severity::Hard,
        "soft" => Severity::Soft,
        other => return Err(format!("{name}: unknown severity {other}")),
    };
    let ceiling = ceiling.parse::<f64>().map_err(|e| format!("{name}: {e}"))?;
    Ok(Limit {
        name: name.to_owned(),
        severity,
        ceiling,
        unit: unit.to_owned(),
    })
}

fn judge(limit: &Limit, value: Option<f64>) -> Verdict {
    match (value, limit.severity) {
        (None, _) => Verdict::NotMeasured,
        // NaN compares false, so it must be caught here or it would count as within.
        (Some(v), Severity::Hard) if v.is_nan() || v > limit.ceiling => Verdict::Over,
        (Some(v), Severity::Soft) if v.is_nan() || v > limit.ceiling => Verdict::Warning,
        (Some(_), _) => Verdict::Within,
    }
}

pub fn evaluate(limits: &[Limit], measured: &BTreeMap<String, f64>) -> Vec<Outcome> {
    limits
        .iter()
        .map(|limit| {
            let value = measured.get(&limit.name).copied();
            Outcome {
                limit: limit.clone(),
                value,
                verdict: judge(limit, value),
            }
        })
        .collect()
}

/// 0 when every hard limit was measured and met, 1 when one was exceeded, 3 when one was not
/// measured. Exceeding outranks missing: a known failure must never read as merely incomplete.
pub fn exit_code(outcomes: &[Outcome]) -> u8 {
    let hard = |verdict| {
        outcomes
            .iter()
            .any(|o| o.limit.severity == Severity::Hard && o.verdict == verdict)
    };
    if hard(Verdict::Over) {
        1
    } else if hard(Verdict::NotMeasured) {
        3
    } else {
        0
    }
}

pub fn report_line(outcome: &Outcome) -> String {
    let status = match outcome.verdict {
        Verdict::Within => "ok",
        Verdict::Over => "OVER LIMIT",
        Verdict::Warning => "WARNING",
        Verdict::NotMeasured => "NOT MEASURED",
    };
    let value = outcome
        .value
        .map_or_else(|| "-".to_owned(), |v| format!("{v:.2}"));
    format!(
        "{status:<13}{:<26}{value:>12} of {} {}",
        outcome.limit.name, outcome.limit.ceiling, outcome.limit.unit
    )
}

/// Pushes each hard limit over its ceiling in turn. Every one must fail the gate on its own,
/// which shows that no limit has been left in the file without doing anything.
pub fn prove_each_limit_fails(
    limits: &[Limit],
    measured: &BTreeMap<String, f64>,
) -> Result<usize, String> {
    let hard = limits.iter().filter(|l| l.severity == Severity::Hard);
    let mut proved = 0;
    for limit in hard {
        let mut pushed = measured.clone();
        pushed.insert(limit.name.clone(), limit.ceiling + 1.0);
        for other in limits.iter().filter(|o| o.name != limit.name) {
            pushed.insert(other.name.clone(), other.ceiling);
        }
        let outcomes = evaluate(limits, &pushed);
        let over: Vec<&str> = outcomes
            .iter()
            .filter(|o| o.verdict == Verdict::Over)
            .map(|o| o.limit.name.as_str())
            .collect();
        if over != [limit.name.as_str()] || exit_code(&outcomes) != 1 {
            return Err(format!("{}: exceeding it gave {over:?}", limit.name));
        }
        proved += 1;
    }
    Ok(proved)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# comment\n\nlaunch hard 100 ms\nsize soft 1024 KB\n";

    fn measured(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(n, v)| ((*n).to_owned(), *v)).collect()
    }

    fn run(pairs: &[(&str, f64)]) -> u8 {
        let limits = parse_limits(TEXT).expect("valid");
        exit_code(&evaluate(&limits, &measured(pairs)))
    }

    #[test]
    fn within_every_limit_passes() {
        assert_eq!(run(&[("launch", 41.0), ("size", 725.0)]), 0);
    }

    #[test]
    fn exactly_at_a_ceiling_passes() {
        assert_eq!(run(&[("launch", 100.0), ("size", 1.0)]), 0);
    }

    #[test]
    fn over_a_hard_limit_fails() {
        assert_eq!(run(&[("launch", 100.1), ("size", 1.0)]), 1);
    }

    #[test]
    fn over_a_soft_limit_only_warns() {
        let limits = parse_limits(TEXT).expect("valid");
        let outcomes = evaluate(&limits, &measured(&[("launch", 1.0), ("size", 4000.0)]));
        assert_eq!(outcomes[1].verdict, Verdict::Warning);
        assert_eq!(exit_code(&outcomes), 0);
    }

    #[test]
    fn a_hard_limit_nobody_measured_is_not_a_pass() {
        assert_eq!(run(&[("size", 1.0)]), 3);
    }

    #[test]
    fn exceeding_outranks_a_missing_measurement() {
        let limits = parse_limits("a hard 1 ms\nb hard 1 ms\n").expect("valid");
        assert_eq!(exit_code(&evaluate(&limits, &measured(&[("a", 2.0)]))), 1);
    }

    #[test]
    fn not_a_number_is_not_within() {
        assert_eq!(run(&[("launch", f64::NAN), ("size", 1.0)]), 1);
    }

    #[test]
    fn a_malformed_line_is_an_error_not_a_lost_limit() {
        assert!(parse_limits("launch hard ms\n").is_err());
        assert!(parse_limits("launch firm 100 ms\n").is_err());
        assert!(parse_limits("launch hard many ms\n").is_err());
    }

    #[test]
    fn every_hard_limit_can_fail_the_gate_alone() {
        let limits = parse_limits(TEXT).expect("valid");
        assert_eq!(
            prove_each_limit_fails(&limits, &BTreeMap::new()),
            Ok(1),
            "one hard limit in the sample"
        );
    }

    #[test]
    fn the_committed_limits_file_is_valid_and_every_limit_works() {
        let text = include_str!("../../../perf/limits.txt");
        let limits = parse_limits(text).expect("valid");
        assert!(limits.len() >= 10, "{}", limits.len());
        assert!(prove_each_limit_fails(&limits, &BTreeMap::new()).is_ok());
    }
}
