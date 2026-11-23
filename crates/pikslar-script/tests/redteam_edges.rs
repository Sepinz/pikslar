//! M12 red team: edge cases of numbers, indexes and the parser. Each one must end with the
//! right kind of error, or the right value, and never panic or overflow the stack.

mod common;

use common::*;
use pikslar_script::{compile, ErrorKind, HostTable, Limits, Value};

const MIN: &str = "(-9223372036854775807 - 1)";

fn kind_of(src: &str) -> ErrorKind {
    err_of(src).kind
}

#[test]
fn integer_edges_are_errors_not_panics() {
    let cases: Vec<(String, ErrorKind)> = vec![
        (format!("return {MIN} / -1;"), ErrorKind::Arithmetic),
        (format!("return {MIN} % -1;"), ErrorKind::Arithmetic),
        (format!("return {MIN} * -1;"), ErrorKind::Arithmetic),
        (format!("return -{MIN};"), ErrorKind::Arithmetic),
        (format!("let m = {MIN}; return -m;"), ErrorKind::Arithmetic),
        (format!("return abs({MIN});"), ErrorKind::Arithmetic),
        (format!("return {MIN} - 1;"), ErrorKind::Arithmetic),
        (
            "return 9223372036854775807 + 1;".into(),
            ErrorKind::Arithmetic,
        ),
        ("return 1 % 0;".into(), ErrorKind::Arithmetic),
        ("return 1 / 0;".into(), ErrorKind::Arithmetic),
        ("return 0 % 0;".into(), ErrorKind::Arithmetic),
        ("return 1.0 % 0;".into(), ErrorKind::Arithmetic),
        ("return 1.5 % 0.0;".into(), ErrorKind::Arithmetic),
        ("return 0.0 / 0.0;".into(), ErrorKind::Arithmetic),
        (
            "return int(9223372036854775807.0);".into(),
            ErrorKind::Arithmetic,
        ),
        (
            "return floor(-9223372036854777856.0);".into(),
            ErrorKind::Arithmetic,
        ),
        (
            "return round(9300000000000000000.0);".into(),
            ErrorKind::Arithmetic,
        ),
        (
            "return int(\"9223372036854775808\");".into(),
            ErrorKind::Value,
        ),
        ("return int(\"\");".into(), ErrorKind::Value),
        ("return int(\" 1\");".into(), ErrorKind::Value),
        ("return float(\"nan\");".into(), ErrorKind::Value),
        ("return float(\"inf\");".into(), ErrorKind::Value),
        ("return float(\"-infinity\");".into(), ErrorKind::Value),
        ("return float(\"1e400\");".into(), ErrorKind::Value),
        ("return sqrt(-1);".into(), ErrorKind::Arithmetic),
        ("return sqrt(\"4\");".into(), ErrorKind::Type),
        ("return min(1, \"a\");".into(), ErrorKind::Type),
        ("return 1 < \"a\";".into(), ErrorKind::Type),
        ("return [1] < [2];".into(), ErrorKind::Type),
        ("return -\"a\";".into(), ErrorKind::Type),
        ("return !1;".into(), ErrorKind::Type),
        ("return 1 && true;".into(), ErrorKind::Type),
        ("return 0xffffffffffffffff;".into(), ErrorKind::Syntax),
        ("return 0x;".into(), ErrorKind::Syntax),
        ("return 99999999999999999999;".into(), ErrorKind::Syntax),
    ];
    for (src, want) in cases {
        assert_eq!(kind_of(&src), want, "{src}");
    }
    assert_eq!(int_of(&format!("return {MIN};")), i64::MIN);
    assert_eq!(int_of(&format!("return {MIN} / 1;")), i64::MIN);
    assert_eq!(int_of(&format!("return {MIN} % 7;")), 6);
    assert_eq!(int_of("return int(-9223372036854775808.0);"), i64::MIN);
    assert_eq!(int_of("return int(\"-9223372036854775808\");"), i64::MIN);
    assert_eq!(int_of("return round(-0.5);"), -1);
    assert_eq!(int_of("return int(-0.9);"), 0);
}

#[test]
fn indexes_and_slices_at_every_edge() {
    let cases = [
        ("let l = [1]; return l[-1];", ErrorKind::Index),
        ("let l = [1]; return l[1];", ErrorKind::Index),
        (
            "let l = [1]; return l[9223372036854775807];",
            ErrorKind::Index,
        ),
        ("let l = [1]; l[-1] = 2;", ErrorKind::Index),
        ("let l = [1]; l[1.0] = 2;", ErrorKind::Type),
        ("let l = [1]; return l[\"0\"];", ErrorKind::Type),
        ("let s = \"ab\"; return s[0];", ErrorKind::Type),
        ("let l = [1]; return slice(l, 0, 2);", ErrorKind::Index),
        ("let l = [1]; return slice(l, 1, 0);", ErrorKind::Index),
        ("let l = [1]; return slice(l, -1, 1);", ErrorKind::Index),
        ("return slice(\"\u{e9}\", 0, 1);", ErrorKind::Index),
        ("let l = [1]; insert(l, 2, 0);", ErrorKind::Index),
        ("let l = [1]; insert(l, -1, 0);", ErrorKind::Index),
        ("let l = [1]; remove(l, 1);", ErrorKind::Index),
        ("let l = []; remove(l, 0);", ErrorKind::Index),
        ("let l = []; pop(l);", ErrorKind::Value),
        ("return list(-1, 0);", ErrorKind::Value),
        ("return repeat(\"a\", -1);", ErrorKind::Value),
        ("return list(9223372036854775807, 0);", ErrorKind::Memory),
        (
            "return repeat(\"ab\", 4611686018427387904);",
            ErrorKind::Memory,
        ),
        ("let l = [1]; push(l, l);", ErrorKind::Type),
        ("let l = [1]; l[0] = l;", ErrorKind::Type),
        ("let l = [1]; insert(l, 0, l);", ErrorKind::Type),
        ("let l = [1]; return [l];", ErrorKind::Type),
        ("return list(2, [1]);", ErrorKind::Type),
    ];
    for (src, want) in cases {
        assert_eq!(kind_of(src), want, "{src}");
    }
    assert_eq!(str_of("let l = [1]; insert(l, 1, 2); return l;"), "[1, 2]");
    assert_eq!(str_of("return slice(\"abc\", 3, 3);"), "");
    assert_eq!(int_of("return find([1, 2], [1]);"), -1);
}

/// Deep source nesting of every kind is refused by size, on a normal test thread's stack,
/// never by overflowing it.
#[test]
fn deep_nesting_of_every_kind_is_refused_cleanly() {
    let n = 200_000;
    let shapes: Vec<String> = vec![
        format!("return {}1{};", "(".repeat(n), ")".repeat(n)),
        format!("return {}1{};", "[".repeat(n), "]".repeat(n)),
        format!("return {}1;", "-".repeat(n)),
        format!("return {}true;", "!".repeat(n)),
        format!("return {}1;", "- ".repeat(n)),
        format!("{}{}", "if true { ".repeat(n), "}".repeat(n)),
        format!("{}{}", "while false { ".repeat(n), "}".repeat(n)),
        format!("{}{}", "for i in 0..1 { ".repeat(n), "}".repeat(n)),
        format!(
            "if false {{ }}{} {{ }}",
            " else if false { }".repeat(n / 20)
        ),
        format!("return {}1{};", "abs(".repeat(n), ")".repeat(n)),
        format!("let l = [1]; return l{};", "[0]".repeat(n)),
        format!("return 1{};", " + 1".repeat(n / 5)),
        format!("return true{};", " && true".repeat(n / 8)),
        format!("return 1{};", " * 1".repeat(n / 5)),
    ];
    let limits = Limits {
        source_bytes: usize::MAX,
        memory: 1 << 30,
        ..Limits::default()
    };
    for src in shapes {
        let e = compile(&src, &HostTable::new(), &limits).expect_err(&src[..40.min(src.len())]);
        assert_eq!(e.kind, ErrorKind::TooLarge, "{}: {e}", &src[..40]);
    }
}

/// A float literal too large for a 64-bit float. The language says floats are always finite,
/// so it should be refused like an integer that is too large.
#[test]
fn a_float_literal_too_large_is_refused() {
    let src = format!("return 1{}.0;", "0".repeat(400));
    match eval(&src) {
        Err(e) => assert_eq!(e.kind, ErrorKind::Syntax, "{e}"),
        Ok(v) => panic!("compiled to {v:?}"),
    }
}

/// Error messages carry script data only up to the size of the source.
#[test]
fn error_messages_stay_small() {
    let long = "a".repeat(200_000);
    let e = err_of(&format!("return {long};"));
    assert_eq!(e.kind, ErrorKind::Syntax);
    assert!(e.message.len() <= 200_100);
    let e = err_of("let s = repeat(\"x\", 1000000); return int(s);");
    assert!(e.message.len() < 200, "{}", e.message);
    let e = err_of("let s = repeat(\"x\", 1000000); return s + 1;");
    assert!(e.message.len() < 200, "{}", e.message);
    assert!(matches!(eval("return 1;"), Ok(Value::Int(1))));
}
