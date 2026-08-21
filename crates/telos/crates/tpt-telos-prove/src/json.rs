//! A small, hand-rolled JSON reader (no `serde` dependency).
//!
//! `telos-prove` accepts constraint groups as JSON on stdin/file. Rather than
//! pull in `serde` + `serde_json` (which the rest of the SDK deliberately
//! avoids), this module implements a minimal recursive-descent parser that
//! produces a [`JsonValue`] tree. Errors carry the byte offset where parsing
//! failed, which [`span::LineIndex`](tpt_telos_parser::span::LineIndex) converts
//! into a 1-based `(line, col)` pair for diagnostic messages.

use tpt_telos_parser::span::LineIndex;

/// A parsed JSON value. Object keys preserve insertion order (iteration order
/// matches document order), which keeps human-readable output stable.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// Look up an object field by key. Returns `None` if this is not an object
    /// or the key is absent.
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Borrow the contents of an object. Panics if called on a non-object.
    #[allow(dead_code)]
    pub fn as_object(&self) -> &[(String, JsonValue)] {
        match self {
            JsonValue::Object(o) => o,
            _ => panic!("JsonValue::as_object called on non-object"),
        }
    }

    /// Borrow the contents of an array. Panics if called on a non-array.
    pub fn as_array(&self) -> &[JsonValue] {
        match self {
            JsonValue::Array(a) => a,
            _ => panic!("JsonValue::as_array called on non-array"),
        }
    }

    /// Borrow the contents of a string. Panics if called on a non-string.
    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        match self {
            JsonValue::Str(s) => s,
            _ => panic!("JsonValue::as_str called on non-string"),
        }
    }

    /// Returns `true` iff this value is an object.
    pub fn is_object(&self) -> bool {
        matches!(self, JsonValue::Object(_))
    }

    /// Returns `true` iff this value is an array.
    pub fn is_array(&self) -> bool {
        matches!(self, JsonValue::Array(_))
    }

    /// Returns `true` iff this value is a string.
    #[allow(dead_code)]
    pub fn is_string(&self) -> bool {
        matches!(self, JsonValue::Str(_))
    }

    /// Returns `true` iff this value is a number (used to gate float casts).
    #[allow(dead_code)]
    pub fn is_number(&self) -> bool {
        matches!(self, JsonValue::Number(_))
    }
}

/// A parse error carrying the byte offset where parsing failed, so the caller
/// can render a `line:col` diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub offset: usize,
    pub message: String,
}

impl JsonError {
    pub fn new(offset: usize, message: impl Into<String>) -> Self {
        JsonError {
            offset,
            message: message.into(),
        }
    }

    /// Render as a `line:col` message against `src` using a [`LineIndex`].
    pub fn display(&self, src: &str) -> String {
        let idx = LineIndex::new(src);
        let (line, col) = idx.line_col(self.offset);
        format!("JSON error at {}:{}: {}", line, col, self.message)
    }
}

/// Parse a complete JSON document from `src`, consuming optional leading/trailing
/// whitespace. Returns an error if `src` does not contain exactly one JSON value.
///
/// # Examples
///
/// ```
/// use tpt_telos_sdk::json::parse_json;
///
/// let v = parse_json(r#"{ "a": [1, 2], "b": true }"#).unwrap();
/// assert!(v.is_object());
/// assert_eq!(v.get("b").unwrap(), &tpt_telos_sdk::json::JsonValue::Bool(true));
/// ```
pub fn parse_json(src: &str) -> Result<JsonValue, JsonError> {
    let bytes: Vec<char> = src.chars().collect();
    let mut p = Parser {
        bytes: &bytes,
        pos: 0,
    };
    p.skip_ws();
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(JsonError::new(
            p.pos,
            "trailing characters after top-level JSON value",
        ));
    }
    Ok(v)
}

struct Parser<'a> {
    bytes: &'a [char],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, JsonError> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => Ok(JsonValue::Str(self.parse_string()?)),
            Some('t') | Some('f') => self.parse_bool(),
            Some('n') => self.parse_null(),
            Some(c) if c == '-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(JsonError::new(
                self.pos,
                format!("unexpected character '{c}' at start of value"),
            )),
            None => Err(JsonError::new(self.pos, "unexpected end of input")),
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), JsonError> {
        match self.bump() {
            Some(c) if c == expected => Ok(()),
            Some(c) => Err(JsonError::new(
                self.pos - 1,
                format!("expected '{expected}', found '{c}'"),
            )),
            None => Err(JsonError::new(self.pos, format!("expected '{expected}'"))),
        }
    }

    fn parse_object(&mut self) -> Result<JsonValue, JsonError> {
        self.expect('{')?;
        let mut fields = Vec::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(JsonValue::Object(fields));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some('"') {
                return Err(JsonError::new(self.pos, "expected string key in object"));
            }
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(':')?;
            let val = self.parse_value()?;
            fields.push((key, val));
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some('}') => break,
                Some(c) => {
                    return Err(JsonError::new(
                        self.pos - 1,
                        format!("expected ',' or '}}' in object, found '{c}'"),
                    ))
                }
                None => return Err(JsonError::new(self.pos, "unterminated object")),
            }
        }
        Ok(JsonValue::Object(fields))
    }

    fn parse_array(&mut self) -> Result<JsonValue, JsonError> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(JsonValue::Array(items));
        }
        loop {
            let val = self.parse_value()?;
            items.push(val);
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some(']') => break,
                Some(c) => {
                    return Err(JsonError::new(
                        self.pos - 1,
                        format!("expected ',' or ']' in array, found '{c}'"),
                    ))
                }
                None => return Err(JsonError::new(self.pos, "unterminated array")),
            }
        }
        Ok(JsonValue::Array(items))
    }

    fn parse_string(&mut self) -> Result<String, JsonError> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(JsonError::new(self.pos, "unterminated string")),
                Some('"') => break,
                Some('\\') => {
                    let esc = self
                        .bump()
                        .ok_or_else(|| JsonError::new(self.pos, "unterminated escape sequence"))?;
                    match esc {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let cp = self.parse_hex4()?;
                            if (0xD800..=0xDBFF).contains(&cp) {
                                if self.bump() != Some('\\') || self.bump() != Some('u') {
                                    return Err(JsonError::new(self.pos, "invalid surrogate pair"));
                                }
                                let low = self.parse_hex4()?;
                                if !(0xDC00..=0xDFFF).contains(&low) {
                                    return Err(JsonError::new(self.pos, "invalid low surrogate"));
                                }
                                let c = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                                match char::from_u32(c) {
                                    Some(ch) => out.push(ch),
                                    None => return Err(JsonError::new(self.pos, "bad code point")),
                                }
                            } else {
                                match char::from_u32(cp) {
                                    Some(ch) => out.push(ch),
                                    None => return Err(JsonError::new(self.pos, "bad code point")),
                                }
                            }
                        }
                        other => {
                            return Err(JsonError::new(
                                self.pos - 1,
                                format!("invalid escape '\\{other}'"),
                            ))
                        }
                    }
                }
                Some(c) => out.push(c),
            }
        }
        Ok(out)
    }

    fn parse_hex4(&mut self) -> Result<u32, JsonError> {
        let mut val: u32 = 0;
        for _ in 0..4 {
            let c = self
                .bump()
                .ok_or_else(|| JsonError::new(self.pos, "short unicode escape"))?;
            let d = c
                .to_digit(16)
                .ok_or_else(|| JsonError::new(self.pos - 1, format!("invalid hex digit '{c}'")))?;
            val = val * 16 + d;
        }
        Ok(val)
    }

    fn parse_bool(&mut self) -> Result<JsonValue, JsonError> {
        if self.try_match("true") {
            Ok(JsonValue::Bool(true))
        } else if self.try_match("false") {
            Ok(JsonValue::Bool(false))
        } else {
            Err(JsonError::new(
                self.pos,
                "invalid literal (expected true/false)",
            ))
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, JsonError> {
        if self.try_match("null") {
            Ok(JsonValue::Null)
        } else {
            Err(JsonError::new(self.pos, "invalid literal (expected null)"))
        }
    }

    fn try_match(&mut self, word: &str) -> bool {
        let chars: Vec<char> = word.chars().collect();
        if self.pos + chars.len() <= self.bytes.len()
            && self.bytes[self.pos..self.pos + chars.len()] == chars
        {
            self.pos += chars.len();
            true
        } else {
            false
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, JsonError> {
        let start = self.pos;
        let bytes = self.bytes;
        if bytes.get(start) == Some(&'-') {
            self.pos += 1;
        }
        let int_start = self.pos;
        while bytes
            .get(self.pos)
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            self.pos += 1;
        }
        if self.pos == int_start {
            return Err(JsonError::new(start, "invalid number: no digits"));
        }
        if bytes.get(self.pos) == Some(&'.') {
            self.pos += 1;
            let frac_start = self.pos;
            while bytes
                .get(self.pos)
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                self.pos += 1;
            }
            if self.pos == frac_start {
                return Err(JsonError::new(
                    start,
                    "invalid number: '.' without fraction",
                ));
            }
        }
        if bytes.get(self.pos) == Some(&'e') || bytes.get(self.pos) == Some(&'E') {
            self.pos += 1;
            if bytes.get(self.pos) == Some(&'+') || bytes.get(self.pos) == Some(&'-') {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while bytes
                .get(self.pos)
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                self.pos += 1;
            }
            if self.pos == exp_start {
                return Err(JsonError::new(
                    start,
                    "invalid number: exponent without digits",
                ));
            }
        }
        let s: String = bytes[start..self.pos].iter().collect();
        match s.parse::<f64>() {
            Ok(n) => Ok(JsonValue::Number(n)),
            Err(_) => Err(JsonError::new(start, "number out of range")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_object_with_array_and_bool() {
        let v = parse_json(r#"{ "a": [1, 2], "b": true }"#).unwrap();
        assert!(v.is_object());
        let a = v.get("a").unwrap().as_array();
        assert_eq!(a.len(), 2);
        assert_eq!(a[0], JsonValue::Number(1.0));
        assert_eq!(v.get("b").unwrap(), &JsonValue::Bool(true));
    }

    #[test]
    fn parses_nested_and_escapes() {
        let v = parse_json(r#"{ "s": "a\"b\nc", "n": { "x": -3.5e2, "y": null } }"#).unwrap();
        assert_eq!(v.get("s").unwrap().as_str(), "a\"b\nc");
        let n = v.get("n").unwrap().as_object();
        assert_eq!(n[0].1, JsonValue::Number(-350.0));
        assert_eq!(n[1].1, JsonValue::Null);
    }

    #[test]
    fn rejects_trailing_characters() {
        let err = parse_json(r#"1 2"#).unwrap_err();
        assert!(err.message.contains("trailing"));
    }

    #[test]
    fn rejects_unterminated_string() {
        let err = parse_json(r#""abc"#).unwrap_err();
        assert!(err.message.contains("unterminated"));
    }

    #[test]
    fn reports_offset_for_error() {
        let src = "{\n  \"a\": 1,\n  bad\n}";
        let err = parse_json(src).unwrap_err();
        let rendered = err.display(src);
        assert!(rendered.starts_with("JSON error at 3:"), "got: {rendered}");
    }

    #[test]
    fn parses_empty_containers() {
        let v = parse_json(r#"{ "a": [], "b": {} }"#).unwrap();
        assert_eq!(v.get("a").unwrap().as_array().len(), 0);
        assert_eq!(v.get("b").unwrap().as_object().len(), 0);
    }

    #[test]
    fn parses_unicode_escape() {
        let v = parse_json(r#""\u00e9""#).unwrap();
        assert_eq!(v.as_str(), "é");
    }
}
