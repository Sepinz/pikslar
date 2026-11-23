//! Turns source text into tokens, one at a time, so no token list is ever held in memory.

use crate::error::{syntax, Pos, Trap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kw {
    Let,
    Fn,
    If,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Return,
    True,
    False,
    Nil,
}

const KEYWORDS: [(&str, Kw); 13] = [
    ("let", Kw::Let),
    ("fn", Kw::Fn),
    ("if", Kw::If),
    ("else", Kw::Else),
    ("while", Kw::While),
    ("for", Kw::For),
    ("in", Kw::In),
    ("break", Kw::Break),
    ("continue", Kw::Continue),
    ("return", Kw::Return),
    ("true", Kw::True),
    ("false", Kw::False),
    ("nil", Kw::Nil),
];

pub(crate) fn is_keyword(name: &str) -> bool {
    KEYWORDS.iter().any(|(k, _)| *k == name)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum P {
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    DotDot,
}

/// Two-character punctuation first, so `<=` is not read as `<` then `=`.
const PUNCT: [(&str, P); 24] = [
    ("==", P::Eq),
    ("!=", P::Ne),
    ("<=", P::Le),
    (">=", P::Ge),
    ("&&", P::AndAnd),
    ("||", P::OrOr),
    ("..", P::DotDot),
    ("(", P::LParen),
    (")", P::RParen),
    ("{", P::LBrace),
    ("}", P::RBrace),
    ("[", P::LBracket),
    ("]", P::RBracket),
    (",", P::Comma),
    (";", P::Semi),
    ("=", P::Assign),
    ("+", P::Plus),
    ("-", P::Minus),
    ("*", P::Star),
    ("/", P::Slash),
    ("%", P::Percent),
    ("<", P::Lt),
    (">", P::Gt),
    ("!", P::Bang),
];

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok<'a> {
    Int(i64),
    Float(f64),
    Str(String),
    Ident(&'a str),
    Kw(Kw),
    P(P),
    Eof,
}

pub(crate) struct Lexer<'a> {
    src: &'a str,
    at: usize,
    pos: Pos,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Lexer<'a> {
        Lexer {
            src,
            at: 0,
            pos: Pos::START,
        }
    }

    fn peek_byte(&self, ahead: usize) -> Option<u8> {
        self.src.as_bytes().get(self.at + ahead).copied()
    }

    /// Steps over one byte, counting lines, and columns in characters.
    fn bump(&mut self) {
        if let Some(b) = self.peek_byte(0) {
            self.at += 1;
            if b == b'\n' {
                self.pos.line = self.pos.line.saturating_add(1);
                self.pos.column = 1;
            } else if b & 0xC0 != 0x80 {
                self.pos.column = self.pos.column.saturating_add(1);
            }
        }
    }

    fn skip_space(&mut self) {
        while let Some(b) = self.peek_byte(0) {
            if b == b'#' {
                while !matches!(self.peek_byte(0), None | Some(b'\n')) {
                    self.bump();
                }
            } else if b.is_ascii_whitespace() {
                self.bump();
            } else {
                return;
            }
        }
    }

    /// The next token and where it starts.
    pub fn next(&mut self) -> Result<(Tok<'a>, Pos), Trap> {
        self.skip_space();
        let start = self.pos;
        let Some(b) = self.peek_byte(0) else {
            return Ok((Tok::Eof, start));
        };
        let tok = if b.is_ascii_digit() {
            self.number(start)?
        } else if b.is_ascii_alphabetic() || b == b'_' {
            self.word()
        } else if b == b'"' {
            self.string(start)?
        } else {
            self.punct(start)?
        };
        Ok((tok, start))
    }

    fn take_while(&mut self, keep: impl Fn(u8) -> bool) -> &'a str {
        let from = self.at;
        while self.peek_byte(0).is_some_and(&keep) {
            self.bump();
        }
        &self.src[from..self.at]
    }

    fn word(&mut self) -> Tok<'a> {
        let w = self.take_while(|b| b.is_ascii_alphanumeric() || b == b'_');
        match KEYWORDS.iter().find(|(k, _)| *k == w) {
            Some((_, kw)) => Tok::Kw(*kw),
            None => Tok::Ident(w),
        }
    }

    fn number(&mut self, start: Pos) -> Result<Tok<'a>, Trap> {
        if self.peek_byte(0) == Some(b'0') && matches!(self.peek_byte(1), Some(b'x' | b'X')) {
            self.bump();
            self.bump();
            let digits = self.take_while(|b| b.is_ascii_hexdigit());
            return i64::from_str_radix(digits, 16)
                .map(Tok::Int)
                .map_err(|_| syntax(start, "bad hexadecimal number"));
        }
        let from = self.at;
        self.take_while(|b| b.is_ascii_digit());
        // A point starts a fraction only before a digit, so `0..10` is a range.
        let float = self.peek_byte(0) == Some(b'.')
            && self.peek_byte(1).is_some_and(|b| b.is_ascii_digit());
        if float {
            self.bump();
            self.take_while(|b| b.is_ascii_digit());
        }
        if self
            .peek_byte(0)
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        {
            return Err(syntax(start, "a number cannot run into a name"));
        }
        let text = &self.src[from..self.at];
        if float {
            // Floats are always finite: a literal past the largest float is refused here.
            text.parse::<f64>()
                .ok()
                .filter(|x| x.is_finite())
                .map(Tok::Float)
                .ok_or_else(|| syntax(start, "number too large"))
        } else {
            text.parse()
                .map(Tok::Int)
                .map_err(|_| syntax(start, "integer too large"))
        }
    }

    fn string(&mut self, start: Pos) -> Result<Tok<'a>, Trap> {
        self.bump();
        let mut out = String::new();
        loop {
            let from = self.at;
            self.take_while(|b| b != b'"' && b != b'\\');
            out.push_str(&self.src[from..self.at]);
            match self.peek_byte(0) {
                None => return Err(syntax(start, "unterminated string")),
                Some(b'"') => {
                    self.bump();
                    return Ok(Tok::Str(out));
                }
                _ => out.push(self.escape()?),
            }
        }
    }

    fn escape(&mut self) -> Result<char, Trap> {
        let at = self.pos;
        self.bump();
        let c = match self.peek_byte(0) {
            Some(b'n') => '\n',
            Some(b't') => '\t',
            Some(b'"') => '"',
            Some(b'\\') => '\\',
            _ => return Err(syntax(at, "unknown escape; use \\n, \\t, \\\" or \\\\")),
        };
        self.bump();
        Ok(c)
    }

    fn punct(&mut self, start: Pos) -> Result<Tok<'a>, Trap> {
        let rest = &self.src[self.at..];
        let Some((text, p)) = PUNCT.iter().find(|(t, _)| rest.starts_with(*t)) else {
            let c = rest.chars().next().unwrap_or(' ');
            return Err(syntax(start, format!("unexpected character '{c}'")));
        };
        for _ in 0..text.len() {
            self.bump();
        }
        Ok(Tok::P(*p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(src: &str) -> Result<Vec<(Tok<'_>, Pos)>, Trap> {
        let mut l = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = l.next()?;
            if t.0 == Tok::Eof {
                return Ok(out);
            }
            out.push(t);
        }
    }

    fn toks(src: &str) -> Vec<Tok<'_>> {
        all(src).expect("lexes").into_iter().map(|t| t.0).collect()
    }

    #[test]
    fn reads_numbers_ranges_and_hex() {
        assert_eq!(
            toks("0..10 1.5 0xff"),
            [
                Tok::Int(0),
                Tok::P(P::DotDot),
                Tok::Int(10),
                Tok::Float(1.5),
                Tok::Int(255)
            ]
        );
    }

    #[test]
    fn reads_words_and_punctuation() {
        assert_eq!(
            toks("let x <= y; # note\n"),
            [
                Tok::Kw(Kw::Let),
                Tok::Ident("x"),
                Tok::P(P::Le),
                Tok::Ident("y"),
                Tok::P(P::Semi)
            ]
        );
    }

    #[test]
    fn reads_strings_with_escapes() {
        assert_eq!(toks(r#""a\n\"b""#), [Tok::Str("a\n\"b".into())]);
    }

    #[test]
    fn positions_count_lines_and_characters() {
        let t = all("\"é\" x\n  y").expect("lexes");
        assert_eq!(t[0].1, Pos { line: 1, column: 1 });
        assert_eq!(t[1].1, Pos { line: 1, column: 5 });
        assert_eq!(t[2].1, Pos { line: 2, column: 3 });
    }

    #[test]
    fn rejects_bad_input_with_a_place() {
        let e = all("x = 99999999999999999999;").expect_err("too large");
        assert_eq!(e.pos, Pos { line: 1, column: 5 });
        assert!(all("\"open").is_err());
        assert!(all("\"\\q\"").is_err());
        assert!(all("a @ b").is_err());
        assert!(all("12ab").is_err());
    }
}
