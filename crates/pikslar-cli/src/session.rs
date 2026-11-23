//! `session`: JSON lines over standard input and output, for a tool the person launches. One
//! request a line in, one reply a line out, in order. The session ends when standard input
//! does. See `docs/cli-json-lines.md`.

use crate::failure::Failure;
use crate::fields::Refusal;
use crate::json::{self, Value};
use crate::requests::{reply, Session, MAX_LINE_BYTES};
use crate::sandbox::Sandbox;
use std::io::{self, BufRead, Write};
use std::path::Path;

pub fn run(dir: &Path) -> Result<(), Failure> {
    let session = Session::new(Sandbox::new(dir)?);
    serve(&mut io::stdin().lock(), &mut io::stdout().lock(), session)
}

/// One line of input.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    /// A whole line, without its line break (`\n` or `\r\n`).
    Whole(Vec<u8>),
    /// A line longer than [`MAX_LINE_BYTES`], read to its end and dropped.
    TooLong,
    /// The input ended inside a line.
    Unended,
    /// The input ended after a whole line, or held nothing.
    End,
}

/// Reads one line, keeping no more than `max` bytes of it in memory however long it is.
fn next_line(input: &mut impl BufRead, max: usize) -> io::Result<Line> {
    let (mut line, mut long, mut any) = (Vec::new(), false, false);
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok(if any { Line::Unended } else { Line::End });
        }
        any = true;
        let end = buf.iter().position(|&b| b == b'\n');
        let part = &buf[..end.unwrap_or(buf.len())];
        if !long && line.len() + part.len() <= max + 1 {
            line.extend_from_slice(part);
        } else {
            long = true;
            line.clear();
        }
        let used = end.map_or(buf.len(), |at| at + 1);
        input.consume(used);
        if end.is_some() {
            break;
        }
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(match long || line.len() > max {
        true => Line::TooLong,
        false => Line::Whole(line),
    })
}

fn send(output: &mut impl Write, value: &Value) -> Result<(), Failure> {
    let mut text = json::to_text(value);
    text.push('\n');
    output
        .write_all(text.as_bytes())
        .and_then(|()| output.flush())
        .map_err(|e| Failure::Write(format!("Could not write to standard output: {e}")))
}

/// Answers every request line until the input ends. A line that ends without a line break is
/// not carried out: it may be a request cut short.
pub fn serve(
    input: &mut impl BufRead,
    output: &mut impl Write,
    mut session: Session,
) -> Result<(), Failure> {
    loop {
        let line = next_line(input, MAX_LINE_BYTES)
            .map_err(|e| Failure::Read(format!("Could not read standard input: {e}")))?;
        let answer = match line {
            Line::End => return Ok(()),
            Line::Whole(bytes) => session.handle(&bytes),
            Line::TooLong => refused(
                "line_too_long",
                format!("a request line is longer than {MAX_LINE_BYTES} bytes; it was not read"),
            ),
            Line::Unended => {
                let why = "the input ended inside a line; that request was not carried out";
                return send(output, &refused("unended", why.into()));
            }
        };
        send(output, &answer)?;
    }
}

fn refused(code: &'static str, message: String) -> Value {
    reply(Value::Null, Err(Refusal::new(code, message)))
}

#[cfg(test)]
mod line_tests {
    use super::*;

    fn lines(input: &[u8], max: usize) -> Vec<Line> {
        let mut reader = io::BufReader::with_capacity(4, input);
        let mut out = Vec::new();
        loop {
            let line = next_line(&mut reader, max).expect("reads");
            let done = matches!(line, Line::End | Line::Unended);
            out.push(line);
            if done {
                return out;
            }
        }
    }

    #[test]
    fn lines_are_split_kept_short_and_their_end_told() {
        use Line::*;
        let w = |s: &str| Whole(s.as_bytes().to_vec());
        assert_eq!(lines(b"", 8), [End]);
        assert_eq!(lines(b"ab\n\ncd\r\n", 8), [w("ab"), w(""), w("cd"), End]);
        assert_eq!(
            lines(b"12345678\n123456789\nx", 8),
            [w("12345678"), TooLong, Unended]
        );
        assert_eq!(lines(b"1234567\r\n", 7), [w("1234567"), End]);
        assert_eq!(lines(&[b'x'; 100], 8), [Unended]);
        let mut long = vec![b'x'; 100];
        long.extend_from_slice(b"\nok\n");
        assert_eq!(lines(&long, 8), [TooLong, w("ok"), End]);
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
