//! Reading one JSON text, strictly, within limits.

use super::{JsonError, Limits, Value};

/// Reads `bytes` as exactly one JSON value, with only whitespace around it.
pub fn parse(bytes: &[u8], limits: &Limits) -> Result<Value, JsonError> {
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        return Err(error(
            0,
            "the text begins with a byte order mark, which JSON lines refuse",
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|e| error(e.valid_up_to(), "the text is not valid UTF-8"))?;
    let mut reader = Reader {
        text,
        at: 0,
        limits,
    };
    let value = reader.value(0)?;
    reader.skip_space();
    match reader.at == text.len() {
        true => Ok(value),
        false => Err(reader.fail("unexpected characters after the value")),
    }
}

fn error(at: usize, what: impl Into<String>) -> JsonError {
    JsonError {
        at,
        what: what.into(),
    }
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
    limits: &'a Limits,
}

impl Reader<'_> {
    fn fail(&self, what: impl Into<String>) -> JsonError {
        error(self.at, what)
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    /// Steps over `byte`, or fails saying what was expected.
    fn expect(&mut self, byte: u8, what: &str) -> Result<(), JsonError> {
        match self.peek() == Some(byte) {
            true => {
                self.at += 1;
                Ok(())
            }
            false => Err(self.fail(format!("expected {what}"))),
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.skip_space();
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'-' | b'0'..=b'9') => self.number().map(Value::Int),
            Some(b't' | b'f' | b'n') => self.word(),
            Some(_) => Err(self.fail("unexpected character where a value was expected")),
            None => Err(self.fail("the text ended where a value was expected")),
        }
    }

    fn deeper(&self, depth: usize) -> Result<(), JsonError> {
        match depth > self.limits.depth {
            true => Err(self.fail(format!(
                "nested too deep: more than {} arrays and objects inside one another",
                self.limits.depth
            ))),
            false => Ok(()),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.deeper(depth)?;
        self.at += 1;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value(depth)?);
            if items.len() > self.limits.array_items {
                let most = self.limits.array_items;
                return Err(self.fail(format!("an array has more than {most} items")));
            }
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => break,
                _ => return Err(self.fail("expected , or ] in an array")),
            }
        }
        self.at += 1;
        Ok(Value::Array(items))
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.deeper(depth)?;
        self.at += 1;
        let mut members: Vec<(String, Value)> = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_space();
            let start = self.at;
            if self.peek() != Some(b'"') {
                return Err(self.fail("expected a key in double quotes"));
            }
            let key = self.string()?;
            if members.iter().any(|(k, _)| *k == key) {
                return Err(error(start, format!("the key {key:?} is given twice")));
            }
            self.skip_space();
            self.expect(b':', ": after a key")?;
            members.push((key, self.value(depth)?));
            if members.len() > self.limits.object_members {
                let most = self.limits.object_members;
                return Err(self.fail(format!("an object has more than {most} members")));
            }
            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => break,
                _ => return Err(self.fail("expected , or } in an object")),
            }
        }
        self.at += 1;
        Ok(Value::Object(members))
    }

    fn word(&mut self) -> Result<Value, JsonError> {
        let rest = &self.text[self.at..];
        let (value, len) = if rest.starts_with("true") {
            (Value::Bool(true), 4)
        } else if rest.starts_with("false") {
            (Value::Bool(false), 5)
        } else if rest.starts_with("null") {
            (Value::Null, 4)
        } else {
            return Err(self.fail("unexpected word; JSON has true, false and null"));
        };
        self.at += len;
        Ok(value)
    }

    /// A whole number: `-?(0|[1-9][0-9]*)`, with no fraction or exponent, fitting an `i64`.
    fn number(&mut self) -> Result<i64, JsonError> {
        let start = self.at;
        let negative = self.peek() == Some(b'-');
        self.at += usize::from(negative);
        let digits = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        let text = &self.text[digits..self.at];
        if text.is_empty() {
            return Err(self.fail("a number needs digits; NaN and Infinity are not JSON"));
        }
        if text.len() > 1 && text.starts_with('0') {
            return Err(error(digits, "a number may not start with 0"));
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(error(
                start,
                "not a whole number; only whole numbers are taken",
            ));
        }
        let mut value: i64 = 0;
        for d in text.bytes() {
            let digit = i64::from(d - b'0');
            value = value
                .checked_mul(10)
                .and_then(|v| v.checked_sub(digit))
                .ok_or_else(|| error(start, "the number does not fit in 64 bits"))?;
        }
        match (negative, value) {
            (true, 0) => Err(error(start, "-0 is refused; write 0")),
            (true, v) => Ok(v),
            (false, v) => v
                .checked_neg()
                .ok_or_else(|| error(start, "the number does not fit in 64 bits")),
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1;
        let mut out = String::new();
        let mut count = 0usize;
        loop {
            let Some(c) = self.text[self.at..].chars().next() else {
                return Err(self.fail("the text ended inside a string"));
            };
            let c = match c {
                '"' => {
                    self.at += 1;
                    return Ok(out);
                }
                '\\' => self.escape()?,
                c if c < ' ' => {
                    return Err(self.fail("a control character in a string must be escaped"))
                }
                c => {
                    self.at += c.len_utf8();
                    c
                }
            };
            count += 1;
            if count > self.limits.string_chars {
                let most = self.limits.string_chars;
                return Err(self.fail(format!("a string is longer than {most} characters")));
            }
            out.push(c);
        }
    }

    fn escape(&mut self) -> Result<char, JsonError> {
        self.at += 1;
        let c = match self.peek() {
            Some(b'"') => '"',
            Some(b'\\') => '\\',
            Some(b'/') => '/',
            Some(b'b') => '\u{8}',
            Some(b'f') => '\u{c}',
            Some(b'n') => '\n',
            Some(b'r') => '\r',
            Some(b't') => '\t',
            Some(b'u') => return self.unicode(),
            _ => return Err(self.fail("unknown escape in a string")),
        };
        self.at += 1;
        Ok(c)
    }

    /// `\uXXXX`, where a surrogate must be a high one followed at once by a low one.
    fn unicode(&mut self) -> Result<char, JsonError> {
        let start = self.at - 1;
        let lone = || error(start, "a surrogate escape without its pair");
        let high = self.hex4()?;
        let code = match high {
            0xD800..=0xDBFF => {
                if !self.text[self.at..].starts_with("\\u") {
                    return Err(lone());
                }
                self.at += 1;
                let low = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(lone());
                }
                0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(lone()),
            code => code,
        };
        char::from_u32(code).ok_or_else(lone)
    }

    /// The four hex digits after a `u`, which `self.at` is on.
    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self.text.get(self.at + 1..self.at + 5);
        let code = digits
            .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|d| u32::from_str_radix(d, 16).ok())
            .ok_or_else(|| self.fail("\\u needs four hex digits"))?;
        self.at += 5;
        Ok(code)
    }
}
