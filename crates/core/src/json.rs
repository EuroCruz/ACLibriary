use crate::{bad, Res};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(s: &str) -> Res<Json> {
        let mut p = P { b: s.as_bytes(), i: 0, d: 0 };
        let v = p.val()?;
        p.ws();
        if p.i != p.b.len() {
            return bad("json: trailing data");
        }
        Ok(v)
    }

    pub fn obj() -> Json {
        Json::Obj(Vec::new())
    }

    pub fn set(mut self, k: &str, v: impl Into<Json>) -> Json {
        if let Json::Obj(o) = &mut self {
            let v = v.into();
            match o.iter_mut().find(|x| x.0 == k) {
                Some(x) => x.1 = v,
                None => o.push((k.to_string(), v)),
            }
        }
        self
    }

    pub fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Obj(o) => o.iter().find(|x| x.0 == k).map(|x| &x.1),
            _ => None,
        }
    }

    pub fn at(&self, i: usize) -> Option<&Json> {
        match self {
            Json::Arr(a) => a.get(i),
            _ => None,
        }
    }

    pub fn path(&self, p: &str) -> Option<&Json> {
        p.split('.').filter(|s| !s.is_empty()).try_fold(self, |v, k| k.parse::<usize>().ok().and_then(|i| v.at(i)).or_else(|| v.get(k)))
    }

    pub fn str(&self) -> Option<&str> {
        if let Json::Str(s) = self { Some(s) } else { None }
    }

    pub fn num(&self) -> Option<f64> {
        if let Json::Num(n) = self { Some(*n) } else { None }
    }

    pub fn bool(&self) -> Option<bool> {
        if let Json::Bool(b) = self { Some(*b) } else { None }
    }

    pub fn arr(&self) -> &[Json] {
        if let Json::Arr(a) = self { a } else { &[] }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
}

macro_rules! from {
    ($($t:ty => $e:expr),*) => {$(impl From<$t> for Json { fn from(v: $t) -> Json { $e(v) } })*};
}

from!(bool => Json::Bool, f64 => Json::Num, String => Json::Str, Vec<Json> => Json::Arr);

impl From<&str> for Json {
    fn from(v: &str) -> Json {
        Json::Str(v.to_string())
    }
}

impl From<i64> for Json {
    fn from(v: i64) -> Json {
        Json::Num(v as f64)
    }
}

impl From<u64> for Json {
    fn from(v: u64) -> Json {
        Json::Num(v as f64)
    }
}

impl From<u32> for Json {
    fn from(v: u32) -> Json {
        Json::Num(v as f64)
    }
}

impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(v: Option<T>) -> Json {
        v.map_or(Json::Null, Into::into)
    }
}

pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(b) => write!(f, "{b}"),
            Json::Num(n) if !n.is_finite() => f.write_str("null"),
            Json::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => write!(f, "{}", *n as i64),
            Json::Num(n) => write!(f, "{n}"),
            Json::Str(s) => f.write_str(&quote(s)),
            Json::Arr(a) => {
                f.write_str("[")?;
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{v}")?;
                }
                f.write_str("]")
            }
            Json::Obj(o) => {
                f.write_str("{")?;
                for (i, (k, v)) in o.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{}:{v}", quote(k))?;
                }
                f.write_str("}")
            }
        }
    }
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
    d: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.b.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        let ok = self.b.get(self.i) == Some(&c);
        if ok {
            self.i += 1;
        }
        ok
    }

    fn word(&mut self, w: &str, v: Json) -> Res<Json> {
        if self.b[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            bad("json: bad literal")
        }
    }

    fn val(&mut self) -> Res<Json> {
        self.ws();
        self.d += 1;
        if self.d > 256 {
            return bad("json: too deep");
        }
        let v = match self.b.get(self.i) {
            Some(b'n') => self.word("null", Json::Null),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'"') => self.str().map(Json::Str),
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                if !self.eat(b']') {
                    loop {
                        a.push(self.val()?);
                        if self.eat(b']') {
                            break;
                        }
                        if !self.eat(b',') {
                            return bad("json: expected , or ]");
                        }
                    }
                }
                Ok(Json::Arr(a))
            }
            Some(b'{') => {
                self.i += 1;
                let mut o = Vec::new();
                if !self.eat(b'}') {
                    loop {
                        self.ws();
                        if self.b.get(self.i) != Some(&b'"') {
                            return bad("json: expected key");
                        }
                        let k = self.str()?;
                        if !self.eat(b':') {
                            return bad("json: expected :");
                        }
                        o.push((k, self.val()?));
                        if self.eat(b'}') {
                            break;
                        }
                        if !self.eat(b',') {
                            return bad("json: expected , or }");
                        }
                    }
                }
                Ok(Json::Obj(o))
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                let s = self.i;
                while self.b.get(self.i).is_some_and(|c| c.is_ascii_digit() || b"+-.eE".contains(c)) {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[s..self.i]).ok().and_then(|t| t.parse().ok()).map(Json::Num).ok_or(crate::Error::Bad("json: bad number"))
            }
            _ => bad("json: unexpected character"),
        };
        self.d -= 1;
        v
    }

    fn hex4(&mut self) -> Res<u32> {
        let h = self.b.get(self.i..self.i + 4).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u32::from_str_radix(h, 16).ok());
        self.i += 4;
        h.ok_or(crate::Error::Bad("json: bad \\u escape"))
    }

    fn str(&mut self) -> Res<String> {
        self.i += 1;
        let mut o = Vec::new();
        loop {
            match self.b.get(self.i) {
                None => return bad("json: unterminated string"),
                Some(b'"') => {
                    self.i += 1;
                    return String::from_utf8(o).map_err(|_| crate::Error::Bad("json: bad utf-8"));
                }
                Some(b'\\') => {
                    self.i += 2;
                    let c = match self.b.get(self.i - 1) {
                        Some(b'n') => '\n',
                        Some(b't') => '\t',
                        Some(b'r') => '\r',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'u') => {
                            let mut u = self.hex4()?;
                            if (0xd800..0xdc00).contains(&u) && self.b.get(self.i..self.i + 2) == Some(b"\\u") {
                                self.i += 2;
                                let l = self.hex4()?;
                                u = 0x10000 + ((u - 0xd800) << 10) + (l.wrapping_sub(0xdc00) & 0x3ff);
                            }
                            char::from_u32(u).unwrap_or('\u{fffd}')
                        }
                        Some(&c) => c as char,
                        None => return bad("json: bad escape"),
                    };
                    let mut t = [0; 4];
                    o.extend_from_slice(c.encode_utf8(&mut t).as_bytes());
                }
                Some(&c) => {
                    o.push(c);
                    self.i += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = r#" {"a": [1, -2.5, 3e2, true, null], "b": {"c": "x\"y\\nЖ😀"}, "e": {}, "f": []} "#;
        let v = Json::parse(s).unwrap();
        assert_eq!(v.path("a.1").and_then(Json::num), Some(-2.5));
        assert_eq!(v.path("a.2").and_then(Json::num), Some(300.0));
        assert_eq!(v.path("b.c").and_then(Json::str), Some("x\"y\\nЖ😀"));
        assert!(v.path("a.4").unwrap().is_null());
        assert_eq!(v.to_string(), r#"{"a":[1,-2.5,300,true,null],"b":{"c":"x\"y\\nЖ😀"},"e":{},"f":[]}"#);
        assert_eq!(Json::parse(&v.to_string()).unwrap(), v);
        let o = Json::obj().set("cmd", "SET").set("pid", 42u32).set("x", None::<&str>).set("cmd", "GET");
        assert_eq!(o.to_string(), r#"{"cmd":"GET","pid":42,"x":null}"#);
        assert_eq!(quote("a\u{1}\n"), r#""a\u0001\n""#);
        for bad in ["", "{", "[1,]", "{\"a\" 1}", "tru", "\"x", "1 2", "{1:2}", "[1}", &"[".repeat(300)] {
            assert!(Json::parse(bad).is_err(), "{bad}");
        }
    }
}
