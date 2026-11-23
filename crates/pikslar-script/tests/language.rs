//! The language's behaviour: syntax, semantics and the errors it reports.

mod common;

use common::*;
use pikslar_script::{compile, ErrorKind, HostTable, Limits, Pos};

#[test]
fn arithmetic_and_precedence() {
    assert_eq!(int_of("return 1 + 2 * 3 - 4 / 2;"), 5);
    assert_eq!(int_of("return (1 + 2) * 3;"), 9);
    assert_eq!(int_of("return -7 % 3;"), 2);
    assert_eq!(int_of("return 0xff00ff;"), 0xff00ff);
    assert_eq!(str_of("return 1 + 0.5;"), "1.5");
    assert_eq!(str_of("return 6 / 4.0;"), "1.5");
    assert_eq!(str_of("return -(2.5);"), "-2.5");
}

#[test]
fn variables_assignment_and_scopes() {
    assert_eq!(int_of("let x = 1; x = x + 1; return x;"), 2);
    assert_eq!(
        int_of("let x = 1; if true { let x = 5; x = 6; } return x;"),
        1
    );
    assert_eq!(int_of("let x = 1; let x = x + 10; return x;"), 11);
    let e = err_of("if true { let y = 1; } return y;");
    assert_eq!(e.kind, ErrorKind::Syntax);
    assert!(e.message.contains("unknown variable 'y'"));
}

#[test]
fn if_else_chains() {
    let src = "fn f(n) { if n < 0 { return -1; } else if n == 0 { return 0; } else { return 1; } }
               return f(-5) * 100 + f(0) * 10 + f(9);";
    assert_eq!(int_of(src), -99);
}

#[test]
fn loops_with_break_and_continue() {
    let src = "let s = 0; let i = 0;
               while true { i = i + 1; if i > 10 { break; } if i % 2 == 0 { continue; } s = s + i; }
               return s;";
    assert_eq!(int_of(src), 25);
    assert_eq!(
        int_of("let s = 0; for i in 0..5 { s = s + i; } return s;"),
        10
    );
    assert_eq!(
        int_of("let s = 0; for i in 5..0 { s = s + 1; } return s;"),
        0
    );
    let src = "let s = 0; for v in [3, 4, 5] { if v == 4 { continue; } s = s + v; } return s;";
    assert_eq!(int_of(src), 8);
    let src = "let n = 0; for y in 0..10 { for x in 0..10 { if x == 3 { break; } n = n + 1; } } return n;";
    assert_eq!(int_of(src), 30);
}

#[test]
fn for_each_sees_changes_to_the_list() {
    let src =
        "let l = [1]; let n = 0; for v in l { n = n + 1; if v < 5 { push(l, v + 1); } } return n;";
    assert_eq!(int_of(src), 5);
}

#[test]
fn functions_recursion_and_forward_calls() {
    let src =
        "return fib(15); fn fib(n) { if n < 2 { return n; } return fib(n - 1) + fib(n - 2); }";
    assert_eq!(int_of(src), 610);
    assert_eq!(str_of("fn nothing() { } return nothing();"), "nil");
    assert_eq!(str_of("fn early() { return; } return early();"), "nil");
}

#[test]
fn functions_do_not_see_top_level_variables() {
    let e = err_of("let g = 1; fn f() { return g; } return f();");
    assert!(e.message.contains("unknown variable 'g'"));
}

#[test]
fn strings_and_builtins() {
    assert_eq!(str_of(r#"return "pix" + "lar";"#), "pixlar");
    assert_eq!(int_of(r#"return len("héllo");"#), 6);
    assert_eq!(str_of(r#"return slice("pikslar", 1, 4);"#), "iks");
    assert_eq!(int_of(r#"return find("pikslar", "sl");"#), 3);
    assert_eq!(int_of(r#"return find("pikslar", "zz");"#), -1);
    assert_eq!(str_of(r#"return repeat("ab", 3);"#), "ababab");
    assert_eq!(
        str_of(r#"return str(12) + "/" + str(1.0) + str(nil) + str(true);"#),
        "12/1.0niltrue"
    );
    assert_eq!(
        int_of(r#"return int("-42") + int(3.9) + floor(-0.5) + round(2.5);"#),
        -39 + -1 + 3
    );
    assert_eq!(str_of(r#"return float("2.25") + sqrt(16);"#), "6.25");
    assert_eq!(
        str_of("return type(1) + type(1.5) + type(\"\") + type([]) + type(nil) + type(true);"),
        "intfloatstringlistnilbool"
    );
    assert_eq!(int_of("return abs(-3) + min(4, 9) + max(4, 9);"), 16);
    assert_eq!(str_of("return min(2, 1.5);"), "1.5");
}

#[test]
fn lists() {
    assert_eq!(
        str_of("let l = [1, 2]; push(l, 3); l[0] = 9; return l;"),
        "[9, 2, 3]"
    );
    assert_eq!(int_of("let l = list(4, 7); return len(l) + l[3];"), 11);
    assert_eq!(
        str_of("let l = [1, 2, 3]; insert(l, 1, 5); return str(remove(l, 0)) + str(l);"),
        "1[5, 2, 3]"
    );
    assert_eq!(
        str_of("let l = [1, 2, 3]; let p = pop(l); return str(p) + str(l);"),
        "3[1, 2]"
    );
    assert_eq!(str_of("return slice([1, 2, 3, 4], 1, 3);"), "[2, 3]");
    assert_eq!(int_of("return find([\"a\", \"b\"], \"b\");"), 1);
    assert_eq!(
        str_of("let a = [1]; let b = a; push(b, 2); return a;"),
        "[1, 2]"
    );
    assert_eq!(str_of("return [1, 2] == [1, 2.0];"), "true");
    assert_eq!(str_of("return [1, 2] == [2, 1];"), "false");
}

#[test]
fn lists_cannot_hold_lists() {
    for src in [
        "let a = []; push(a, a);",
        "let a = [[1]];",
        "let a = [1]; a[0] = [];",
        "let a = list(2, []);",
    ] {
        let e = err_of(src);
        assert_eq!(e.kind, ErrorKind::Type, "{src}");
        assert!(e.message.contains("lists cannot contain lists"), "{src}");
    }
}

#[test]
fn conditions_must_be_booleans() {
    for src in [
        "if 1 { }",
        "while nil { }",
        "let x = 1 && true;",
        "let x = !0;",
    ] {
        assert_eq!(err_of(src).kind, ErrorKind::Type, "{src}");
    }
    assert_eq!(str_of("return false && 1;"), "false");
    assert_eq!(str_of("return true || 1;"), "true");
}

#[test]
fn runtime_errors_carry_line_and_column() {
    let e = err_of("let a = 1;\nlet b = 0;\nlet c =   a / b;");
    assert_eq!(e.kind, ErrorKind::Arithmetic);
    assert_eq!(
        e.pos,
        Pos {
            line: 3,
            column: 13
        }
    );
    assert_eq!(e.to_string(), "line 3, column 13: division by zero");
    let e = err_of("let l = [1];\n  return l[5];");
    assert_eq!(
        (e.kind, e.pos),
        (
            ErrorKind::Index,
            Pos {
                line: 2,
                column: 12
            }
        )
    );
    let e = err_of("fn f() {\n  return 1 + \"x\";\n}\nf();");
    assert_eq!(
        (e.kind, e.pos),
        (
            ErrorKind::Type,
            Pos {
                line: 2,
                column: 12
            }
        )
    );
    let e = err_of("return 9223372036854775807 + 1;");
    assert_eq!((e.kind, e.pos.column), (ErrorKind::Arithmetic, 28));
    let e = err_of("let l = []; pop(l);");
    assert_eq!((e.kind, e.pos.column), (ErrorKind::Value, 13));
}

#[test]
fn compile_errors_carry_line_and_column() {
    let cases = [
        ("let = 1;", ErrorKind::Syntax, 1, 5),
        ("let x = 1\nlet y = 2;", ErrorKind::Syntax, 2, 1),
        ("x = 1;", ErrorKind::Syntax, 1, 1),
        ("break;", ErrorKind::Syntax, 1, 1),
        ("if true { fn f() { } }", ErrorKind::Syntax, 1, 11),
        ("f(1);", ErrorKind::Syntax, 1, 1),
        ("fn f(a) { } f();", ErrorKind::Syntax, 1, 13),
        ("fn f() { } fn f() { }", ErrorKind::Syntax, 1, 15),
        ("fn len(a) { }", ErrorKind::Syntax, 1, 4),
        ("fn set_pixel() { }", ErrorKind::Syntax, 1, 4),
        ("len(1, 2);", ErrorKind::Syntax, 1, 1),
        ("let x = 1 < 2 < 3;", ErrorKind::Syntax, 1, 15),
        ("1 + 2 = 3;", ErrorKind::Syntax, 1, 1),
        ("fn f(a, a) { }", ErrorKind::Syntax, 1, 9),
        ("let s = \"open;", ErrorKind::Syntax, 1, 9),
        ("while true { ", ErrorKind::Syntax, 1, 14),
    ];
    for (src, kind, line, column) in cases {
        let e = err_of(src);
        assert_eq!((e.kind, e.pos), (kind, Pos { line, column }), "{src}: {e}");
    }
}

#[test]
fn host_functions_are_the_only_way_out() {
    let mut c = Canvas::new();
    let r = report_with(
        "set_pixel(3, 4, 99); print(get_pixel(3, 4)); fill_rect(0, 0, 2, 2, 5);",
        &Limits::default(),
        &mut c,
        &Default::default(),
    );
    assert!(r.result.is_ok());
    assert_eq!(c.printed, ["99"]);
    assert_eq!(c.px[(4 * W + 3) as usize], 99);
    assert_eq!(c.px[(W + 1) as usize], 5);
    for name in [
        "open",
        "exec",
        "read_file",
        "getenv",
        "time",
        "random",
        "system",
        "import",
        "require",
        "eval",
        "load",
    ] {
        let src = format!("{name}(\"x\");");
        let e = compile(&src, &table(), &Limits::default()).expect_err("no such function");
        assert!(e.message.contains("unknown function"), "{name}: {e}");
        let e = compile(&src, &HostTable::new(), &Limits::default()).expect_err("none at all");
        assert_eq!(e.kind, ErrorKind::Syntax);
    }
}

#[test]
fn host_errors_are_reported_at_the_call() {
    let e = err_of("set_pixel(1, \"y\", 2);");
    assert_eq!((e.kind, e.pos.column), (ErrorKind::Host, 1));
    assert!(e.message.contains("expected an integer"));
}

#[test]
fn runs_are_deterministic() {
    let src = "let l = []; for i in 0..200 { push(l, str(i * 7 % 13) + \"-\" + str(i / 3.0)); } return l;";
    let first = str_of(src);
    for _ in 0..3 {
        assert_eq!(str_of(src), first);
    }
}

#[test]
fn a_program_runs_many_times() {
    let p = compile("print(1);", &table(), &Limits::default()).expect("compiles");
    let mut c = Canvas::new();
    for _ in 0..3 {
        let r = pikslar_script::run(&p, &mut c, &Limits::default(), &Default::default());
        assert!(r.result.is_ok());
    }
    assert_eq!(c.printed.len(), 3);
}

#[test]
fn printed_output_comes_from_the_host() {
    assert_eq!(printed("print([1, \"a\", 2.5]);"), ["[1, a, 2.5]"]);
}

#[test]
fn programs_can_be_sent_to_the_thread_that_runs_them() {
    fn send_sync<T: Send + Sync>(_: &T) {}
    let p = compile(
        "let s = \"text\"; return s + s;",
        &table(),
        &Limits::default(),
    )
    .expect("compiles");
    send_sync(&p);
    let out = on_script_thread(move || {
        let r = pikslar_script::run(
            &p,
            &mut Canvas::new(),
            &Limits::default(),
            &Default::default(),
        );
        r.result.map(|v| v.to_string()).ok()
    });
    assert_eq!(out.as_deref(), Some("texttext"));
}

#[test]
fn float_literals_too_large_are_refused() {
    let big = format!("return {}.0;", "1".repeat(400));
    let e = err_of(&big);
    assert_eq!(e.kind, ErrorKind::Syntax);
    assert!(e.message.contains("number"), "{e}");
}

#[test]
fn script_data_in_error_messages_is_cut_short() {
    let long = "a".repeat(100_000);
    for src in [
        format!("return {long};"),
        format!("{long}(1);"),
        format!("fn {long}() {{ }} fn {long}() {{ }}"),
        format!("fn f({long}, {long}) {{ }}"),
    ] {
        let e = err_of(&src);
        assert!(
            e.message.len() <= 512,
            "{} bytes: {}",
            e.message.len(),
            &e.message[..80]
        );
        assert!(e.message.contains('\u{2026}'), "{}", &e.message[..80]);
    }
}
