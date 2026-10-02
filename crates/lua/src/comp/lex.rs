use ac_core::{Error, Res};

#[derive(Clone, Debug, PartialEq)]
pub enum T {
    Eos,
    Name(Vec<u8>),
    Str(Vec<u8>),
    Num(f64),
    And,
    Break,
    Do,
    Else,
    Elseif,
    End,
    False,
    For,
    Function,
    If,
    In,
    Local,
    Nil,
    Not,
    Or,
    Repeat,
    Return,
    Then,
    True,
    Until,
    While,
    Concat,
    Dots,
    Eq,
    Ge,
    Le,
    Ne,
    Ch(u8),
}

const WORDS: [(&[u8], T); 21] = [
    (b"and", T::And),
    (b"break", T::Break),
    (b"do", T::Do),
    (b"else", T::Else),
    (b"elseif", T::Elseif),
    (b"end", T::End),
    (b"false", T::False),
    (b"for", T::For),
    (b"function", T::Function),
    (b"if", T::If),
    (b"in", T::In),
    (b"local", T::Local),
    (b"nil", T::Nil),
    (b"not", T::Not),
    (b"or", T::Or),
    (b"repeat", T::Repeat),
    (b"return", T::Return),
    (b"then", T::Then),
    (b"true", T::True),
    (b"until", T::Until),
    (b"while", T::While),
];

pub fn show(t: &T) -> String {
    match t {
        T::Eos => "<eof>".into(),
        T::Name(s) | T::Str(s) => String::from_utf8_lossy(s).into_owned(),
        T::Num(n) => format!("{n}"),
        T::Concat => "..".into(),
        T::Dots => "...".into(),
        T::Eq => "==".into(),
        T::Ge => ">=".into(),
        T::Le => "<=".into(),
        T::Ne => "~=".into(),
        T::Ch(c) => (*c as char).to_string(),
        w => String::from_utf8_lossy(WORDS.iter().find(|x| &x.1 == w).unwrap().0).into_owned(),
    }
}

pub struct Lex<'a> {
    s: &'a [u8],
    p: usize,
    pub line: u32,
    pub last: u32,
    pub t: T,
    ahead: Option<T>,
    pub chunk: String,
    pub src: Vec<u8>,
    raw: Vec<u8>,
}

fn nl(c: Option<u8>) -> bool {
    matches!(c, Some(b'\n' | b'\r'))
}

impl<'a> Lex<'a> {
    pub fn new(s: &'a [u8], name: &str) -> Lex<'a> {
        let chunk = match name.as_bytes().first() {
            Some(b'@' | b'=') => name[1..].to_string(),
            _ => format!("[string \"{}\"]", name.lines().next().unwrap_or("")),
        };
        let p = if s.first() == Some(&b'#') { s.iter().position(|&c| c == b'\n').unwrap_or(s.len()) } else { 0 };
        Lex { s, p, line: 1, last: 1, t: T::Eos, ahead: None, chunk, src: name.as_bytes().to_vec(), raw: Vec::new() }
    }

    fn c(&self) -> Option<u8> {
        self.s.get(self.p).copied()
    }

    fn at(&self, k: usize) -> Option<u8> {
        self.s.get(self.p + k).copied()
    }

    fn adv(&mut self) {
        self.p += 1;
    }

    fn take(&mut self) {
        if let Some(c) = self.c() {
            self.raw.push(c);
        }
        self.p += 1;
    }

    pub fn err<X>(&self, msg: &str, near: Option<&str>) -> Res<X> {
        let near = near.map_or(String::new(), |n| format!(" near '{n}'"));
        Err(Error::Msg(format!("{}:{}: {msg}{near}", self.chunk, self.line)))
    }

    pub fn syntax<X>(&self, msg: &str) -> Res<X> {
        let near = match &self.t {
            T::Name(_) | T::Str(_) | T::Num(_) => String::from_utf8_lossy(&self.raw).into_owned(),
            t => show(t),
        };
        self.err(msg, Some(&near))
    }

    fn newline(&mut self) -> Res<()> {
        let old = self.c();
        self.adv();
        if nl(self.c()) && self.c() != old {
            self.adv();
        }
        self.line += 1;
        Ok(())
    }

    fn sep(&mut self) -> i32 {
        let s = self.c();
        self.take();
        let mut n = 0;
        while self.c() == Some(b'=') {
            self.take();
            n += 1;
        }
        if self.c() == s { n } else { -n - 1 }
    }

    fn long(&mut self, sep: i32, keep: bool) -> Res<Vec<u8>> {
        self.take();
        if nl(self.c()) {
            self.newline()?;
        }
        let mut o = Vec::new();
        loop {
            match self.c() {
                None => return self.err(if keep { "unfinished long string" } else { "unfinished long comment" }, Some("<eof>")),
                Some(b'[') => {
                    let st = self.p;
                    if self.sep() == sep {
                        if sep == 0 {
                            return self.err("nesting of [[...]] is deprecated", Some("["));
                        }
                        self.take();
                    }
                    o.extend_from_slice(&self.s[st..self.p]);
                }
                Some(b']') => {
                    let st = self.p;
                    if self.sep() == sep {
                        self.take();
                        return Ok(o);
                    }
                    o.extend_from_slice(&self.s[st..self.p]);
                }
                Some(b'\n' | b'\r') => {
                    o.push(b'\n');
                    self.newline()?;
                }
                Some(c) => {
                    if keep {
                        o.push(c);
                    }
                    self.adv();
                }
            }
        }
    }

    fn string(&mut self, del: u8) -> Res<Vec<u8>> {
        self.take();
        let mut o = Vec::new();
        while self.c() != Some(del) {
            match self.c() {
                None => return self.err("unfinished string", Some("<eof>")),
                Some(b'\n' | b'\r') => return self.err("unfinished string", Some(&String::from_utf8_lossy(&self.raw))),
                Some(b'\\') => {
                    self.adv();
                    let c = match self.c() {
                        Some(b'a') => 7,
                        Some(b'b') => 8,
                        Some(b'f') => 12,
                        Some(b'n') => b'\n',
                        Some(b'r') => b'\r',
                        Some(b't') => b'\t',
                        Some(b'v') => 11,
                        Some(b'\n' | b'\r') => {
                            o.push(b'\n');
                            self.newline()?;
                            continue;
                        }
                        None => continue,
                        Some(d) if d.is_ascii_digit() => {
                            let mut v = 0u32;
                            let mut i = 0;
                            while i < 3 && self.c().is_some_and(|x| x.is_ascii_digit()) {
                                v = v * 10 + (self.c().unwrap() - b'0') as u32;
                                self.adv();
                                i += 1;
                            }
                            if v > 255 {
                                return self.err("escape sequence too large", Some(&String::from_utf8_lossy(&self.raw)));
                            }
                            o.push(v as u8);
                            continue;
                        }
                        Some(c) => c,
                    };
                    o.push(c);
                    self.adv();
                }
                Some(c) => {
                    o.push(c);
                    self.take();
                }
            }
        }
        self.take();
        Ok(o)
    }

    fn number(&mut self) -> Res<f64> {
        let st = self.p;
        while self.c().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
            self.adv();
        }
        if matches!(self.c(), Some(b'e' | b'E')) {
            self.adv();
            if matches!(self.c(), Some(b'+' | b'-')) {
                self.adv();
            }
        }
        while self.c().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
            self.adv();
        }
        let t = std::str::from_utf8(&self.s[st..self.p]).unwrap_or("");
        self.raw = t.as_bytes().to_vec();
        let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"));
        let v = match hex {
            Some(h) if !h.is_empty() && h.bytes().all(|c| c.is_ascii_hexdigit()) => Some(u64::from_str_radix(h, 16).map_or(u32::MAX as f64, |x| x.min(u32::MAX as u64) as f64)),
            Some(_) => None,
            None => t.parse::<f64>().ok().filter(|_| t.bytes().all(|c| c.is_ascii_digit() || b".eE+-".contains(&c))),
        };
        match v {
            Some(v) => Ok(v),
            None => self.err("malformed number", Some(t)),
        }
    }

    fn scan(&mut self) -> Res<T> {
        self.raw.clear();
        loop {
            let Some(c) = self.c() else { return Ok(T::Eos) };
            match c {
                b'\n' | b'\r' => self.newline()?,
                b'-' => {
                    self.adv();
                    if self.c() != Some(b'-') {
                        return Ok(T::Ch(b'-'));
                    }
                    self.adv();
                    if self.c() == Some(b'[') {
                        let s = self.sep();
                        self.raw.clear();
                        if s >= 0 {
                            self.long(s, false)?;
                            self.raw.clear();
                            continue;
                        }
                    }
                    while !nl(self.c()) && self.c().is_some() {
                        self.adv();
                    }
                }
                b'[' => {
                    let s = self.sep();
                    if s >= 0 {
                        return Ok(T::Str(self.long(s, true)?));
                    } else if s == -1 {
                        return Ok(T::Ch(b'['));
                    }
                    return self.err("invalid long string delimiter", Some(&String::from_utf8_lossy(&self.raw)));
                }
                b'=' | b'<' | b'>' | b'~' => {
                    self.adv();
                    if self.c() != Some(b'=') {
                        return Ok(T::Ch(c));
                    }
                    self.adv();
                    return Ok(match c {
                        b'=' => T::Eq,
                        b'<' => T::Le,
                        b'>' => T::Ge,
                        _ => T::Ne,
                    });
                }
                b'"' | b'\'' => return Ok(T::Str(self.string(c)?)),
                b'.' => {
                    if self.at(1) == Some(b'.') {
                        self.p += 2;
                        if self.c() == Some(b'.') {
                            self.adv();
                            return Ok(T::Dots);
                        }
                        return Ok(T::Concat);
                    }
                    if !self.at(1).is_some_and(|d| d.is_ascii_digit()) {
                        self.adv();
                        return Ok(T::Ch(b'.'));
                    }
                    return Ok(T::Num(self.number()?));
                }
                b' ' | b'\t' | 11 | 12 => self.adv(),
                c if c.is_ascii_digit() => return Ok(T::Num(self.number()?)),
                c if c.is_ascii_alphabetic() || c == b'_' => {
                    let st = self.p;
                    while self.c().is_some_and(|x| x.is_ascii_alphanumeric() || x == b'_') {
                        self.adv();
                    }
                    let w = &self.s[st..self.p];
                    self.raw = w.to_vec();
                    return Ok(WORDS.iter().find(|x| x.0 == w).map_or_else(|| T::Name(w.to_vec()), |x| x.1.clone()));
                }
                c => {
                    self.adv();
                    return Ok(T::Ch(c));
                }
            }
        }
    }

    pub fn next(&mut self) -> Res<()> {
        self.last = self.line;
        self.t = match self.ahead.take() {
            Some(t) => t,
            None => self.scan()?,
        };
        Ok(())
    }

    pub fn peek(&mut self) -> Res<&T> {
        if self.ahead.is_none() {
            let t = self.scan()?;
            self.ahead = Some(t);
        }
        Ok(self.ahead.as_ref().unwrap())
    }
}

#[cfg(test)]
mod t {
    use super::*;

    fn toks(s: &str) -> Vec<T> {
        let mut l = Lex::new(s.as_bytes(), "=t");
        let mut o = Vec::new();
        loop {
            l.next().unwrap();
            if l.t == T::Eos {
                return o;
            }
            o.push(l.t.clone());
        }
    }

    #[test]
    fn tokens() {
        let n = |s: &str| T::Name(s.as_bytes().to_vec());
        let st = |s: &[u8]| T::Str(s.to_vec());
        assert_eq!(toks("local x = 1 .. 'a\\n\\65\\\\' -- c\n"), [T::Local, n("x"), T::Ch(b'='), T::Num(1.0), T::Concat, st(b"a\nA\\")]);
        assert_eq!(toks("a.b:c(...) ~= <= >= == [==[x]]y]==]"), [n("a"), T::Ch(b'.'), n("b"), T::Ch(b':'), n("c"), T::Ch(b'('), T::Dots, T::Ch(b')'), T::Ne, T::Le, T::Ge, T::Eq, st(b"x]]y")]);
        assert_eq!(toks("0x1F 3.5e2 .5 --[[ long\ncomment ]] [[\nfirst]]"), [T::Num(31.0), T::Num(350.0), T::Num(0.5), st(b"first")]);
        assert_eq!(toks("#!/bin/lua\nx"), [n("x")]);
        let mut l = Lex::new(b"a\r\nb\n\rc\nd", "=t");
        for line in [1, 2, 3, 4] {
            l.next().unwrap();
            assert_eq!(l.line, line);
        }
    }

    #[test]
    fn errors() {
        for s in ["'abc", "3x", "[==", "\"\\300\"", "[[ [[ ]]", "--[[ open"] {
            let mut l = Lex::new(s.as_bytes(), "@f.lua");
            let r = (0..4).try_for_each(|_| l.next());
            assert!(r.unwrap_err().to_string().starts_with("f.lua:1:"), "{s}");
        }
    }
}
