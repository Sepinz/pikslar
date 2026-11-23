use super::*;

fn read(text: &str) -> Result<Value, JsonError> {
    parse(text.as_bytes(), &LIMITS)
}

fn refused(bytes: &[u8]) -> String {
    match parse(bytes, &LIMITS) {
        Ok(v) => panic!("{:?} was taken as {v:?}", String::from_utf8_lossy(bytes)),
        Err(e) => e.what,
    }
}

#[test]
fn whole_values_are_read() {
    assert_eq!(read(" null ").expect("null"), Value::Null);
    assert_eq!(read("true").expect("true"), Value::Bool(true));
    assert_eq!(read("false").expect("false"), Value::Bool(false));
    assert_eq!(read("0").expect("0"), Value::Int(0));
    assert_eq!(read("-17").expect("-17"), Value::Int(-17));
    assert_eq!(
        read("9223372036854775807").expect("max"),
        Value::Int(i64::MAX)
    );
    assert_eq!(
        read("-9223372036854775808").expect("min"),
        Value::Int(i64::MIN)
    );
    assert_eq!(
        read(r#"{"a":[1,"x",{}],"b":[]}"#).expect("nested"),
        Value::Object(vec![
            (
                "a".into(),
                Value::Array(vec![
                    Value::Int(1),
                    Value::Str("x".into()),
                    Value::Object(vec![])
                ])
            ),
            ("b".into(), Value::Array(vec![])),
        ])
    );
}

#[test]
fn escapes_and_surrogate_pairs_are_read() {
    let v = read(r#""a\"\\\/\b\f\n\r\t\u0041\u00e9\ud83d\ude00""#).expect("escapes");
    assert_eq!(
        v,
        Value::Str("a\"\\/\u{8}\u{c}\n\r\tA\u{e9}\u{1f600}".into())
    );
    assert_eq!(read("\"héllo\"").expect("utf8"), Value::Str("héllo".into()));
}

#[test]
fn numbers_that_are_not_whole_i64s_are_refused() {
    for text in [
        "1.0",
        "1.5",
        "1e2",
        "1E2",
        "1e400",
        "-0",
        "-0.0",
        "01",
        "-01",
        "+1",
        "-",
        "NaN",
        "Infinity",
        "-Infinity",
        "9223372036854775808",
        "-9223372036854775809",
        "99999999999999999999999",
        ".5",
        "0x10",
    ] {
        refused(text.as_bytes());
    }
}

#[test]
fn malformed_text_is_refused() {
    for text in [
        "",
        "   ",
        "{",
        "[",
        "[1,",
        "[1,]",
        "{\"a\":1,}",
        "{\"a\"}",
        "{\"a\" 1}",
        "{a:1}",
        "{'a':1}",
        "\"open",
        "\"bad \\x escape\"",
        "\"\\u12\"",
        "\"\\uZZZZ\"",
        "\"\\ud800\"",
        "\"\\udc00\"",
        "\"\\ud800\\u0041\"",
        "\"tab\there\"",
        "\"nl\nhere\"",
        "tru",
        "nul",
        "True",
        "null x",
        "{} {}",
        "1 2",
        "[1]]",
        "/* c */ 1",
    ] {
        refused(text.as_bytes());
    }
}

#[test]
fn duplicate_keys_bom_and_bad_utf8_are_refused() {
    assert!(refused(br#"{"a":1,"a":2}"#).contains("twice"));
    assert!(refused(b"\xEF\xBB\xBF{}").contains("byte order mark"));
    assert!(refused(b"\"\xff\"").contains("UTF-8"));
    assert!(refused(b"\"\xc0\xaf\"").contains("UTF-8"));
    assert!(refused(b"\"\xed\xa0\x80\"").contains("UTF-8"));
    refused(b"\"a\x00b\"");
    refused(b"\x00");
}

#[test]
fn limits_are_kept() {
    let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    read(&deep(LIMITS.depth)).expect("at the limit");
    assert!(refused(deep(LIMITS.depth + 1).as_bytes()).contains("deep"));
    // Far past the limit is refused without running out of stack.
    refused(deep(1_000_000).as_bytes());
    refused("{\"a\":".repeat(100_000).as_bytes());

    let long = |n: usize| format!("\"{}\"", "é".repeat(n));
    read(&long(LIMITS.string_chars)).expect("at the limit");
    assert!(refused(long(LIMITS.string_chars + 1).as_bytes()).contains("long"));

    let items = |n: usize| format!("[{}]", vec!["0"; n].join(","));
    read(&items(LIMITS.array_items)).expect("at the limit");
    assert!(refused(items(LIMITS.array_items + 1).as_bytes()).contains("items"));

    let members = |n: usize| {
        let list: Vec<String> = (0..n).map(|i| format!("\"k{i}\":0")).collect();
        format!("{{{}}}", list.join(","))
    };
    read(&members(LIMITS.object_members)).expect("at the limit");
    assert!(refused(members(LIMITS.object_members + 1).as_bytes()).contains("members"));
}

#[test]
fn errors_name_the_byte() {
    let e = read("[1, x]").expect_err("x");
    assert_eq!(e.at, 4);
    assert!(e.to_string().starts_with("at byte 4: "));
}

#[test]
fn written_text_reads_back_the_same() {
    let v = Value::Object(vec![
        ("id".into(), Value::Int(i64::MIN)),
        (
            "s".into(),
            Value::Str("q\"b\\n\n\u{0}\u{1f}\u{7f}\u{2028}é😀".into()),
        ),
        (
            "l".into(),
            Value::Array(vec![Value::Null, Value::Bool(true), Value::Bool(false)]),
        ),
    ]);
    let text = to_text(&v);
    assert!(text.is_ascii() || !text.contains('\n'));
    assert!(!text.contains('\n') && !text.contains('\u{2028}') && !text.contains('\u{7f}'));
    assert_eq!(read(&text).expect("reads back"), v);
    assert_eq!(to_text(&Value::Object(vec![])), "{}");
    assert_eq!(
        to_text(&Value::Array(vec![Value::Int(1), Value::Int(-2)])),
        "[1,-2]"
    );
}
