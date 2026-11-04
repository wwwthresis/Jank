//! A minimal JSON parser (RFC 8259) with a nesting-depth limit. It exists to read glTF files.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// The order of object keys is preserved.
    Obj(Vec<(String, Json)>),
}

const MAX_DEPTH: usize = 128;

impl Json {
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = P { s: text.as_bytes(), i: 0 };
        p.ws();
        let v = p.value(0)?;
        p.ws();
        if p.i != p.s.len() {
            return Err(format!("JSON: extra data after the value (position {})", p.i));
        }
        Ok(v)
    }
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn idx(&self, i: usize) -> Option<&Json> {
        match self {
            Json::Arr(a) => a.get(i),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let Json::Num(n) = self { Some(*n) } else { None }
    }
    pub fn as_usize(&self) -> Option<usize> {
        self.as_f64().filter(|n| *n >= 0.0 && n.fract() == 0.0 && *n < 4.0e9).map(|n| n as usize)
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Json::Str(s) = self { Some(s) } else { None }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Json::Bool(b) = self { Some(*b) } else { None }
    }
    pub fn as_array(&self) -> Option<&[Json]> {
        if let Json::Arr(a) = self { Some(a) } else { None }
    }
    pub fn len(&self) -> usize {
        self.as_array().map_or(0, |a| a.len())
    }
    /// An array of numbers as `Vec<f32>` (`None` if any element is not a number).
    pub fn f32_array(&self) -> Option<Vec<f32>> {
        self.as_array()?.iter().map(|v| v.as_f64().map(|n| n as f32)).collect()
    }
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}
impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn err<T>(&self, m: &str) -> Result<T, String> {
        Err(format!("JSON: {m} (position {})", self.i))
    }
    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return self.err("nesting is too deep");
        }
        match self.s.get(self.i) {
            None => self.err("unexpected end of input"),
            Some(b'{') => {
                self.i += 1;
                let mut o = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(o));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return self.err("expected a string key");
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return self.err("expected ':'");
                    }
                    self.i += 1;
                    self.ws();
                    o.push((k, self.value(depth + 1)?));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(o));
                        }
                        _ => return self.err("expected ',' or '}'"),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Arr(a));
                }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(a));
                        }
                        _ => return self.err("expected ',' or ']'"),
                    }
                }
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') if self.s[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Json::Bool(true))
            }
            Some(b'f') if self.s[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Json::Bool(false))
            }
            Some(b'n') if self.s[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Json::Null)
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                let st = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[st..self.i]).ok().and_then(|t| t.parse::<f64>().ok()).filter(|n| n.is_finite()).map(Json::Num).ok_or_else(|| format!("JSON: invalid number (position {st})"))
            }
            _ => self.err("unexpected character"),
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.s.get(self.i..self.i + 4).ok_or("JSON: truncated \\u escape")?;
        self.i += 4;
        u32::from_str_radix(std::str::from_utf8(h).map_err(|_| "JSON: invalid \\u escape")?, 16).map_err(|_| "JSON: invalid \\u escape".to_string())
    }
    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // "
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.s.get(self.i).ok_or("JSON: unterminated string")?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| "JSON: string is not valid UTF-8".to_string()),
                b'\\' => {
                    let e = *self.s.get(self.i).ok_or("JSON: input ends after a backslash")?;
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) {
                                // UTF-16 surrogate pair
                                if self.s.get(self.i..self.i + 2) == Some(b"\\u") {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    } else {
                                        cp = 0xFFFD;
                                    }
                                } else {
                                    cp = 0xFFFD;
                                }
                            } else if (0xDC00..0xE000).contains(&cp) {
                                cp = 0xFFFD;
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            let mut buf = [0u8; 4];
                            out.extend(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return self.err("unknown escape sequence"),
                    }
                }
                c if c < 0x20 => return self.err("control character in string"),
                c => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_nested_and_accessors() {
        let j = Json::parse(r#" {"a":[1,2.5,-3e2,true,null,"x\n\u00e9\ud83d\ude00"],"o":{"k":"v"},"e":[],"z":{}} "#).unwrap();
        assert_eq!(j.get("a").unwrap().len(), 6);
        assert_eq!(j.get("a").unwrap().idx(2).unwrap().as_f64(), Some(-300.0));
        assert_eq!(j.get("a").unwrap().idx(5).unwrap().as_str(), Some("x\né😀"));
        assert_eq!(j.get("o").unwrap().get("k").unwrap().as_str(), Some("v"));
        assert_eq!(j.get("a").unwrap().idx(0).unwrap().as_usize(), Some(1));
        assert_eq!(j.get("a").unwrap().idx(1).unwrap().as_usize(), None);
    }
    #[test]
    fn rejects_malformed() {
        for bad in ["", "{", "[1,]", "{\"a\"}", "{\"a\":1,}", "tru", "\"abc", "[1 2]", "{} x", "01x", "\"\\q\"", "\"a\u{1}b\""] {
            assert!(Json::parse(bad).is_err(), "{bad:?} must be an error");
        }
    }
    #[test]
    fn deep_nesting_is_an_error_not_a_stack_overflow() {
        let s = "[".repeat(100_000);
        assert!(Json::parse(&s).is_err());
    }
}
