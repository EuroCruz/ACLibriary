use crate::val::{ident, Val};
use ac_core::{Error, Res};

#[derive(Clone, Debug)]
pub struct Style {
    pub width: usize,
    pub tab: &'static str,
    pub eol: &'static str,
    pub utf8: bool,
}

impl Default for Style {
    fn default() -> Style {
        Style { width: 120, tab: "\t", eol: "\n", utf8: true }
    }
}

pub fn quote(s: &str, utf8: bool) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            ' '..='~' => o.push(c),
            c if utf8 && !c.is_control() => o.push(c),
            c => c.encode_utf8(&mut [0; 4]).bytes().for_each(|b| o.push_str(&format!("\\{b:03}"))),
        }
    }
    o.push('"');
    o
}

impl Val {
    pub fn int(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn f64(&self) -> Option<f64> {
        match self {
            Val::Int(i) => Some(*i as f64),
            Val::Num(n) => Some(*n),
            Val::Raw(s) => s.parse().ok(),
            _ => None,
        }
    }

    pub fn f32(&self) -> Option<f32> {
        match self {
            Val::Raw(s) => s.parse().ok(),
            v => v.f64().map(|x| x as f32),
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn items(&self) -> &[(Option<String>, Val)] {
        match self {
            Val::Tbl(t) => t,
            _ => &[],
        }
    }

    pub fn key(&self, k: &str) -> Option<&Val> {
        self.items().iter().find(|(n, _)| n.as_deref() == Some(k)).map(|(_, v)| v)
    }
}

struct Pr<'a> {
    s: &'a Style,
    o: String,
}

fn width(s: &str, tab: usize) -> usize {
    s.chars().map(|c| if c == '\t' { tab } else { 1 }).sum()
}

fn simple(v: &Val) -> bool {
    match v {
        Val::Tbl(t) => t.is_empty(),
        Val::Note(_) => false,
        Val::Call(_, a) => a.iter().all(simple),
        _ => true,
    }
}

fn key(k: &str, u: bool) -> String {
    if ident(k) {
        k.to_string()
    } else {
        format!("[{}]", quote(k, u))
    }
}

impl Pr<'_> {
    fn flat(&self, v: &Val) -> Option<String> {
        let u = self.s.utf8;
        Some(match v {
            Val::Str(s) => quote(s, u),
            Val::Note(_) => return None,
            Val::Call(n, a) => match a.as_slice() {
                [Val::Str(s)] => format!("{n}{}", quote(s, u)),
                _ => format!("{n}({})", a.iter().map(|x| self.flat(x)).collect::<Option<Vec<_>>>()?.join(", ")),
            },
            Val::Tbl(t) if t.is_empty() => "{}".into(),
            Val::Tbl(t) => {
                let mut p = Vec::with_capacity(t.len());
                for (k, x) in t {
                    let f = self.flat(x)?;
                    p.push(match k {
                        Some(k) => format!("{} = {f}", key(k, u)),
                        None => f,
                    });
                }
                format!("{{ {} }}", p.join(", "))
            }
            v => v.to_string(),
        })
    }

    fn ind(&mut self, n: usize) {
        for _ in 0..n {
            self.o.push_str(self.s.tab);
        }
    }

    fn tw(&self) -> usize {
        width(self.s.tab, 4)
    }

    fn val(&mut self, v: &Val, ind: usize, col: usize) {
        if let Some(f) = self.flat(v) {
            if col + width(&f, 4) <= self.s.width || !matches!(v, Val::Tbl(_)) {
                self.o.push_str(&f);
                return;
            }
        }
        let Val::Tbl(t) = v else { return };
        let eol = self.s.eol;
        self.o.push('{');
        self.o.push_str(eol);
        if t.iter().all(|(k, x)| k.is_none() && simple(x)) {
            let lim = self.s.width.saturating_sub((ind + 1) * self.tw()).max(16);
            let mut line = String::new();
            for (_, x) in t {
                let f = self.flat(x).unwrap_or_default();
                if !line.is_empty() && width(&line, 4) + f.len() + 1 > lim {
                    self.ind(ind + 1);
                    self.o.push_str(line.trim_end());
                    self.o.push_str(eol);
                    line.clear();
                }
                line.push_str(&f);
                line.push_str(", ");
            }
            if !line.is_empty() {
                self.ind(ind + 1);
                self.o.push_str(line.trim_end());
                self.o.push_str(eol);
            }
        } else {
            for (k, x) in t {
                self.ind(ind + 1);
                if let Val::Note(n) = x {
                    self.note(n, ind + 1);
                    continue;
                }
                let mut c = (ind + 1) * self.tw();
                if let Some(k) = k {
                    let k = key(k, self.s.utf8);
                    c += k.len() + 3;
                    self.o.push_str(&k);
                    self.o.push_str(" = ");
                }
                self.val(x, ind + 1, c);
                self.o.push(',');
                self.o.push_str(eol);
            }
        }
        self.ind(ind);
        self.o.push('}');
    }

    fn note(&mut self, n: &str, ind: usize) {
        for (i, l) in n.lines().enumerate() {
            if i > 0 {
                self.ind(ind);
            }
            self.o.push_str("--");
            if !l.is_empty() {
                self.o.push(' ');
                self.o.push_str(l);
            }
            self.o.push_str(self.s.eol);
        }
    }
}

pub fn pretty(v: &Val, s: &Style) -> String {
    let mut p = Pr { s, o: String::new() };
    p.val(v, 0, 0);
    p.o
}

pub fn chunk(v: &Val, s: &Style) -> String {
    let mut p = Pr { s, o: String::new() };
    let mut gap = false;
    for (k, x) in v.items() {
        if let Val::Note(n) = x {
            p.note(n, 0);
            continue;
        }
        let k = k.as_deref().unwrap_or("_");
        let at = p.o.len();
        p.o.push_str(k);
        p.o.push_str(" = ");
        p.val(x, 0, k.len() + 3);
        let multi = p.o[at..].contains('\n');
        if (multi || gap) && at > 0 && !p.o[..at].ends_with(&format!("{0}{0}", s.eol)) {
            p.o.insert_str(at, s.eol);
        }
        gap = multi;
        p.o.push_str(s.eol);
    }
    p.o
}

struct Lx<'a> {
    s: &'a [u8],
    p: usize,
    line: usize,
}

impl Lx<'_> {
    fn err<T>(&self, m: impl std::fmt::Display) -> Res<T> {
        Err(Error::Msg(format!("line {}: {m}", self.line)))
    }

    fn at(&self, o: usize) -> u8 {
        self.s.get(self.p + o).copied().unwrap_or(0)
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        let ok = self.at(0) == c;
        self.p += ok as usize;
        ok
    }

    fn need(&mut self, c: u8) -> Res<()> {
        if self.eat(c) {
            Ok(())
        } else {
            let f = self.at(0);
            self.err(format!("expected '{}', found {}", c as char, if f == 0 { "end of file".into() } else { format!("'{}'", f as char) }))
        }
    }

    fn level(&self) -> Option<usize> {
        if self.at(0) != b'[' {
            return None;
        }
        let n = (1..).take_while(|&i| self.at(i) == b'=').count();
        (self.at(n + 1) == b'[').then_some(n)
    }

    fn long(&mut self, n: usize) -> Res<String> {
        self.p += n + 2;
        if self.at(0) == b'\r' {
            self.p += 1;
        }
        if self.at(0) == b'\n' {
            self.p += 1;
            self.line += 1;
        }
        let close: Vec<u8> = [b"]".as_slice(), &vec![b'='; n], b"]"].concat();
        let st = self.p;
        while self.p < self.s.len() {
            if self.s[self.p..].starts_with(&close) {
                let t = String::from_utf8_lossy(&self.s[st..self.p]).into_owned();
                self.p += close.len();
                return Ok(t);
            }
            self.line += (self.s[self.p] == b'\n') as usize;
            self.p += 1;
        }
        self.err("unfinished long string")
    }

    fn ws(&mut self) {
        loop {
            match self.at(0) {
                b'\n' => {
                    self.line += 1;
                    self.p += 1;
                }
                b' ' | b'\t' | b'\r' => self.p += 1,
                0xef if self.at(1) == 0xbb && self.at(2) == 0xbf => self.p += 3,
                b'-' if self.at(1) == b'-' => {
                    self.p += 2;
                    if let Some(n) = self.level() {
                        let _ = self.long(n);
                    } else {
                        while !matches!(self.at(0), b'\n' | 0) {
                            self.p += 1;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    fn name(&mut self) -> Option<String> {
        self.ws();
        let st = self.p;
        while self.at(0).is_ascii_alphanumeric() || self.at(0) == b'_' || (self.at(0) == b'.' && self.p > st && (self.at(1).is_ascii_alphabetic() || self.at(1) == b'_')) {
            self.p += 1;
        }
        if self.p == st || self.s[st].is_ascii_digit() {
            self.p = st;
            return None;
        }
        Some(String::from_utf8_lossy(&self.s[st..self.p]).into_owned())
    }

    fn string(&mut self) -> Res<String> {
        let q = self.at(0);
        self.p += 1;
        let mut o = Vec::new();
        loop {
            let c = self.at(0);
            self.p += 1;
            match c {
                0 | b'\n' => return self.err("unfinished string"),
                c if c == q => break,
                b'\\' => {
                    let e = self.at(0);
                    self.p += 1;
                    match e {
                        b'n' => o.push(b'\n'),
                        b'r' => o.push(b'\r'),
                        b't' => o.push(b'\t'),
                        b'a' => o.push(7),
                        b'b' => o.push(8),
                        b'f' => o.push(12),
                        b'v' => o.push(11),
                        b'\n' => {
                            o.push(b'\n');
                            self.line += 1;
                        }
                        b'x' => {
                            let h = std::str::from_utf8(self.s.get(self.p..self.p + 2).unwrap_or(b"")).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                            let Some(h) = h else { return self.err("bad \\x escape") };
                            o.push(h);
                            self.p += 2;
                        }
                        b'0'..=b'9' => {
                            let mut v = (e - b'0') as u32;
                            for _ in 0..2 {
                                if !self.at(0).is_ascii_digit() {
                                    break;
                                }
                                v = v * 10 + (self.at(0) - b'0') as u32;
                                self.p += 1;
                            }
                            if v > 255 {
                                return self.err("escape too large");
                            }
                            o.push(v as u8);
                        }
                        0 => return self.err("unfinished string"),
                        e => o.push(e),
                    }
                }
                c => o.push(c),
            }
        }
        Ok(String::from_utf8(o).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
    }

    fn number(&mut self, neg: bool) -> Res<Val> {
        let st = self.p;
        if self.at(0) == b'0' && matches!(self.at(1), b'x' | b'X') {
            self.p += 2;
            let h = self.p;
            while self.at(0).is_ascii_hexdigit() {
                self.p += 1;
            }
            let Ok(v) = u64::from_str_radix(std::str::from_utf8(&self.s[h..self.p]).unwrap_or(""), 16) else { return self.err("bad hex number") };
            return Ok(Val::Int(if neg { (v as i64).wrapping_neg() } else { v as i64 }));
        }
        while self.at(0).is_ascii_digit() || self.at(0) == b'.' || (matches!(self.at(0), b'e' | b'E')) || (matches!(self.at(0), b'+' | b'-') && matches!(self.s[self.p - 1], b'e' | b'E')) {
            self.p += 1;
        }
        let t = format!("{}{}", if neg { "-" } else { "" }, String::from_utf8_lossy(&self.s[st..self.p]));
        if let Ok(i) = t.parse::<i64>() {
            return Ok(Val::Int(i));
        }
        if t.parse::<f64>().is_err() {
            return self.err(format!("bad number '{t}'"));
        }
        Ok(Val::Raw(t))
    }

    fn table(&mut self) -> Res<Val> {
        let mut t = Vec::new();
        loop {
            if self.eat(b'}') {
                return Ok(Val::Tbl(t));
            }
            self.ws();
            let st = (self.p, self.line);
            let k = if self.at(0) == b'[' && self.level().is_none() {
                self.p += 1;
                let k = match self.expr()? {
                    Val::Str(s) => s,
                    Val::Int(i) => i.to_string(),
                    _ => return self.err("table key must be a string or an integer"),
                };
                self.need(b']')?;
                self.need(b'=')?;
                Some(k)
            } else {
                match self.name() {
                    Some(n) if self.eat(b'=') && self.at(0) != b'=' => Some(n),
                    _ => {
                        (self.p, self.line) = st;
                        None
                    }
                }
            };
            t.push((k, self.expr()?));
            if !self.eat(b',') && !self.eat(b';') {
                self.need(b'}')?;
                return Ok(Val::Tbl(t));
            }
        }
    }

    fn expr(&mut self) -> Res<Val> {
        self.ws();
        match self.at(0) {
            b'{' => {
                self.p += 1;
                self.table()
            }
            b'"' | b'\'' => self.string().map(Val::Str),
            b'[' if self.level().is_some() => {
                let n = self.level().unwrap();
                self.long(n).map(Val::Str)
            }
            b'-' => {
                self.p += 1;
                self.ws();
                if self.at(0).is_ascii_digit() || self.at(0) == b'.' {
                    return self.number(true);
                }
                match self.expr()? {
                    Val::Num(n) => Ok(Val::Num(-n)),
                    _ => self.err("'-' needs a number"),
                }
            }
            c if c.is_ascii_digit() || c == b'.' => self.number(false),
            b'(' => {
                self.p += 1;
                let a = self.expr()?;
                let v = if self.eat(b'/') {
                    let b = self.expr()?;
                    match (a.f64(), b.f64()) {
                        (Some(a), Some(b)) => Val::Num(a / b),
                        _ => return self.err("'/' needs numbers"),
                    }
                } else {
                    a
                };
                self.need(b')')?;
                Ok(v)
            }
            _ => {
                let Some(n) = self.name() else {
                    let c = self.at(0);
                    return self.err(if c == 0 { "unexpected end of file".to_string() } else { format!("unexpected '{}'", c as char) });
                };
                match n.as_str() {
                    "nil" => return Ok(Val::Nil),
                    "true" => return Ok(Val::Bool(true)),
                    "false" => return Ok(Val::Bool(false)),
                    "math.huge" => return Ok(Val::Num(f64::INFINITY)),
                    _ => {}
                }
                self.ws();
                match self.at(0) {
                    b'(' => {
                        self.p += 1;
                        let mut a = Vec::new();
                        if !self.eat(b')') {
                            loop {
                                a.push(self.expr()?);
                                if !self.eat(b',') {
                                    break;
                                }
                            }
                            self.need(b')')?;
                        }
                        Ok(Val::Call(n, a))
                    }
                    b'"' | b'\'' => Ok(Val::Call(n, vec![Val::Str(self.string()?)])),
                    b'{' => {
                        self.p += 1;
                        Ok(Val::Call(n, vec![self.table()?]))
                    }
                    _ => self.err(format!("unexpected name '{n}' (text values need quotes)")),
                }
            }
        }
    }
}

pub fn parse(src: &str) -> Res<Val> {
    let mut l = Lx { s: src.as_bytes(), p: 0, line: 1 };
    l.expr()
}

pub fn parse_chunk(src: &str) -> Res<Val> {
    let mut l = Lx { s: src.as_bytes(), p: 0, line: 1 };
    let mut t = Vec::new();
    loop {
        l.ws();
        if l.p >= l.s.len() {
            return Ok(Val::Tbl(t));
        }
        if l.s[l.p..].starts_with(b"return") && !l.at(6).is_ascii_alphanumeric() {
            l.p += 6;
            return Ok(l.expr()?);
        }
        let mut n = l.name();
        if n.as_deref() == Some("local") {
            n = l.name();
        }
        let Some(n) = n else { return l.err("expected 'name = value'") };
        l.need(b'=')?;
        t.push((Some(n), l.expr()?));
        l.eat(b';');
    }
}

#[cfg(test)]
mod t {
    use super::*;

    fn kv(k: &str, v: Val) -> (Option<String>, Val) {
        (Some(k.into()), v)
    }

    #[test]
    fn reads() {
        let v = parse_chunk("-- hdr\nA = 1 B = -0x10;\nC = { 1.5, x = 'q\\n\\65', [\"a b\"] = { }, -2e3, }\nD = hex\"00ff\" E = f(1, nil) --[[ c ]] F = [==[\nlong]]x]==] G = (0/0)\n").unwrap();
        assert_eq!(v.key("A"), Some(&Val::Int(1)));
        assert_eq!(v.key("B"), Some(&Val::Int(-16)));
        let c = v.key("C").unwrap();
        assert_eq!(c.items()[0].1.f32(), Some(1.5));
        assert_eq!(c.key("x").and_then(Val::str), Some("q\nA"));
        assert_eq!(c.key("a b"), Some(&Val::Tbl(vec![])));
        assert_eq!(c.items()[3].1, Val::Raw("-2e3".into()));
        assert_eq!(v.key("D"), Some(&Val::Call("hex".into(), vec!["00ff".into()])));
        assert_eq!(v.key("E"), Some(&Val::Call("f".into(), vec![1.into(), Val::Nil])));
        assert_eq!(v.key("F").and_then(Val::str), Some("long]]x"));
        assert!(v.key("G").and_then(Val::f64).unwrap().is_nan());
        assert!(parse_chunk("A = ").unwrap_err().to_string().starts_with("line 1"));
        assert!(parse_chunk("A = 1\n\nB = {1,").unwrap_err().to_string().starts_with("line 3"));
        assert!(parse_chunk("A = abc").is_err());
        assert_eq!(parse("{ x = 1 }").unwrap(), Val::Tbl(vec![kv("x", 1.into())]));
    }

    #[test]
    fn floats_keep_their_text() {
        for f in [0.1f32, 1.0e-7, 3.4028235e38, -0.0, 16777217.0, 1.17549435e-38] {
            let s = format!("{f:?}");
            let v = parse(&s).unwrap();
            assert_eq!(v.f32().unwrap().to_bits(), f.to_bits(), "{s}");
        }
    }

    #[test]
    fn writes_and_reads_back() {
        let s = Style::default();
        let rows: Vec<Val> = (0..60).map(|i| Val::Int(i * 1000)).collect();
        let v = Val::Tbl(vec![
            (None, Val::Note("WildStarTool\nsecond".into())),
            kv("Name", "Привет \"x\"".into()),
            kv("Pos", Val::Tbl(vec![(None, Val::Raw("1.5".into())), (None, Val::Int(2)), (None, Val::Raw("-0.0".into()))])),
            kv("Grid", Val::Tbl(rows.into_iter().map(|x| (None, x)).collect())),
            kv(
                "Items",
                Val::Tbl(vec![
                    (None, Val::Note("first".into())),
                    (None, Val::Tbl(vec![kv("Id", Val::Call("hash".into(), vec!["a\\b".into()])), kv("end", true.into())])),
                ]),
            ),
        ]);
        let t = chunk(&v, &s);
        assert!(t.starts_with("-- WildStarTool\n-- second\nName = \"Привет \\\"x\\\"\"\nPos = { 1.5, 2, -0.0 }\n\nGrid = {\n\t0, 1000,"), "{t}");
        assert!(t.contains("\t-- first\n\t{ Id = hash\"a\\\\b\", [\"end\"] = true },\n}"), "{t}");
        assert!(t.lines().all(|l| width(l, 4) <= 120));
        let back = parse_chunk(&t).unwrap();
        let strip = |v: &Val| -> Vec<(Option<String>, Val)> { v.items().iter().filter(|(_, x)| !matches!(x, Val::Note(_))).cloned().collect() };
        assert_eq!(strip(&back), strip(&v).into_iter().map(|(k, x)| (k, if let Val::Tbl(t) = x { Val::Tbl(t.into_iter().filter(|(_, y)| !matches!(y, Val::Note(_))).collect()) } else { x })).collect::<Vec<_>>());
        assert_eq!(quote("é\u{1}", false), "\"\\195\\169\\001\"");
    }
}
