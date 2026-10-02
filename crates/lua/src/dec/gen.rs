use crate::comp::compile;
use crate::luac::{parse, Header};

struct G {
    s: u64,
    o: String,
    sc: Vec<Vec<String>>,
    n: usize,
    lp: bool,
    va: bool,
    ind: usize,
}

impl G {
    fn r(&mut self, n: usize) -> usize {
        self.s ^= self.s << 13;
        self.s ^= self.s >> 7;
        self.s ^= self.s << 17;
        (self.s % n.max(1) as u64) as usize
    }

    fn var(&mut self) -> String {
        let l: Vec<String> = self.sc.iter().flatten().cloned().collect();
        if !l.is_empty() && self.r(3) > 0 {
            let i = self.r(l.len());
            l[i].clone()
        } else {
            format!("g{}", self.r(12))
        }
    }

    fn atom(&mut self, d: u32) -> String {
        match self.r(9) {
            0..=3 => self.var(),
            4 => format!("{}.k{}", self.var(), self.r(3)),
            5 if d > 0 => format!("{}[{}]", self.var(), self.expr(d - 1)),
            6 if d > 0 => self.call(d - 1),
            7 => format!("{}", self.r(100)),
            _ => format!("\"s{}\"", self.r(4)),
        }
    }

    fn cond(&mut self, d: u32) -> String {
        if d == 0 {
            return self.var();
        }
        match self.r(8) {
            0 | 1 => {
                let (a, mut b) = (self.cond(d - 1), self.cond(d - 1));
                for _ in 0..4 {
                    if a != b {
                        break;
                    }
                    b = self.cond(d - 1);
                }
                format!("{a} {} {b}", if self.r(2) == 0 { "and" } else { "or" })
            }
            2 => format!("not {}", self.cond(d - 1)),
            3 => format!("({} or {}) and {}", self.cond(d - 1), self.cond(d - 1), self.cond(d - 1)),
            4 => {
                let op = ["<", "<=", ">", ">=", "==", "~="][self.r(6)];
                format!("{} {op} {}", self.var(), self.atom(d - 1))
            }
            5 => self.call(d - 1),
            _ => self.var(),
        }
    }

    fn call(&mut self, d: u32) -> String {
        let n = self.r(3);
        let a: Vec<String> = (0..n).map(|_| self.expr(d)).collect();
        if self.r(4) == 0 {
            format!("{}:m{}({})", self.var(), self.r(2), a.join(", "))
        } else {
            format!("f{}({})", self.r(3), a.join(", "))
        }
    }

    fn expr(&mut self, d: u32) -> String {
        if d == 0 {
            return self.atom(0);
        }
        match self.r(14) {
            0 => format!("{} + {}", self.atom(d - 1), self.expr(d - 1)),
            1 => format!("{} * {}", self.atom(d - 1), self.atom(d - 1)),
            2 => format!("{} - {}", self.var(), self.atom(d - 1)),
            3 => format!("{} .. {}", self.atom(d - 1), self.expr(d - 1)),
            4 => self.cond(d),
            5 => {
                let n = self.r(4);
                let v: Vec<String> = (0..n)
                    .map(|i| match self.r(3) {
                        0 => format!("k{i} = {}", self.expr(d - 1)),
                        1 => format!("[{}] = {}", self.atom(0), self.expr(d - 1)),
                        _ => self.expr(d - 1),
                    })
                    .collect();
                format!("{{{}}}", v.join(", "))
            }
            6 if self.n < 2 => self.func(d - 1),
            7 => format!("#{}", self.var()),
            8 => format!("-{}", self.var()),
            9 if self.va => "...".into(),
            10 => format!("({})", self.call(d - 1)),
            _ => self.atom(d),
        }
    }

    fn func(&mut self, d: u32) -> String {
        let np = self.r(3);
        let ps: Vec<String> = (0..np).map(|i| format!("p{}_{i}", self.n)).collect();
        let va = self.r(3) == 0;
        let mut h = ps.join(", ");
        if va {
            h.push_str(if np > 0 { ", ..." } else { "..." });
        }
        let (olp, ova) = (self.lp, self.va);
        self.n += 1;
        self.lp = false;
        self.va = va;
        self.sc.push(ps);
        let save = std::mem::take(&mut self.o);
        self.ind += 1;
        self.block(d.min(1), 3);
        self.ind -= 1;
        let body = std::mem::replace(&mut self.o, save);
        self.sc.pop();
        self.n -= 1;
        self.lp = olp;
        self.va = ova;
        format!("function({h})\n{body}{}end", "  ".repeat(self.ind))
    }

    fn line(&mut self, s: &str) {
        self.o.push_str(&"  ".repeat(self.ind));
        self.o.push_str(s);
        self.o.push('\n');
    }

    fn name(&mut self) -> String {
        format!("l{}", self.r(1000))
    }

    fn sub(&mut self, d: u32, head: &str, extra: Vec<String>, lp: bool) {
        self.line(head);
        let olp = self.lp;
        self.lp = self.lp || lp;
        self.sc.push(extra);
        self.ind += 1;
        self.block(d, 3);
        self.ind -= 1;
        self.sc.pop();
        self.lp = olp;
    }

    fn stmt(&mut self, d: u32) {
        let e = 1 + self.r(2) as u32;
        match self.r(if d > 0 { 16 } else { 8 }) {
            0 | 1 => {
                let k = 1 + self.r(3);
                let ns: Vec<String> = (0..k).map(|_| self.name()).collect();
                let m = self.r(k + 1);
                let vs: Vec<String> = (0..m).map(|_| self.expr(e)).collect();
                if vs.is_empty() {
                    self.line(&format!("local {}", ns.join(", ")));
                } else {
                    self.line(&format!("local {} = {}", ns.join(", "), vs.join(", ")));
                }
                self.sc.last_mut().unwrap().extend(ns);
            }
            2 | 3 => {
                let k = 1 + self.r(3);
                let mut ts: Vec<String> = Vec::new();
                for _ in 0..k {
                    let t = match self.r(4) {
                        0 => format!("{}.k{}", self.var(), self.r(3)),
                        1 => format!("{}[{}]", self.var(), self.atom(0)),
                        _ => self.var(),
                    };
                    if !ts.contains(&t) {
                        ts.push(t);
                    }
                }
                let m = 1 + self.r(ts.len());
                let vs: Vec<String> = (0..m).map(|i| loop {
                    let v = self.expr(e);
                    if ts.get(i) != Some(&v) {
                        break v;
                    }
                }).collect();
                self.line(&format!("{} = {}", ts.join(", "), vs.join(", ")));
            }
            4 | 5 => {
                let c = self.call(e);
                self.line(&c);
            }
            6 => {
                let n = self.name();
                self.sc.last_mut().unwrap().push(n.clone());
                let f = self.func(d.saturating_sub(1));
                self.line(&format!("local {n} = {f}"));
            }
            7 => {
                let v = format!("g{}", self.r(12));
                let f = self.func(d.saturating_sub(1));
                let k = self.r(3);
                self.line(&format!("{v}.k{k} = {f}"));
            }
            8 | 9 => {
                let c = self.cond(2);
                self.sub(d - 1, &format!("if {c} then"), vec![], false);
                for _ in 0..self.r(3) {
                    let c = self.cond(2);
                    self.sub(d - 1, &format!("elseif {c} then"), vec![], false);
                }
                if self.r(2) == 0 {
                    self.sub(d - 1, "else", vec![], false);
                }
                self.line("end");
            }
            10 => {
                let c = self.cond(2);
                self.sub(d - 1, &format!("while {c} do"), vec![], true);
                self.line("end");
            }
            11 => {
                let i = self.name();
                let (a, b) = (self.atom(1), self.atom(1));
                let st = if self.r(2) == 0 { format!(", {}", self.atom(0)) } else { String::new() };
                self.sub(d - 1, &format!("for {i} = {a}, {b}{st} do"), vec![i], true);
                self.line("end");
            }
            12 => {
                let (k, v) = (self.name(), self.name());
                let t = self.var();
                self.sub(d - 1, &format!("for {k}, {v} in pairs({t}) do"), vec![k, v], true);
                self.line("end");
            }
            13 => {
                self.line("repeat");
                let olp = self.lp;
                self.lp = true;
                self.sc.push(vec![]);
                self.ind += 1;
                self.block(d - 1, 5);
                self.ind -= 1;
                let c = self.cond(2);
                self.sc.pop();
                self.lp = olp;
                self.line(&format!("until {c}"));
            }
            14 => {
                let n = self.o.len();
                self.sub(d - 1, "do", vec![], false);
                if self.o.len() == n + "do
".len() + 2 * self.ind {
                    self.o.truncate(n);
                } else {
                    self.line("end");
                }
            }
            _ => {
                let n = self.name();
                self.sc.last_mut().unwrap().push(n.clone());
                let f = self.func(d.saturating_sub(1));
                self.line(&format!("local function {n}{}", &f["function".len()..]));
            }
        }
    }

    fn block(&mut self, d: u32, max: usize) {
        let n = if self.r(16) == 0 { 0 } else { 1 + self.r(max) };
        for _ in 0..n {
            self.stmt(d);
        }
        match self.r(6) {
            0 if self.lp => {
                let c = self.cond(1);
                self.line(&format!("if {c} then break end"));
            }
            1 => {
                let k = self.r(3);
                let v: Vec<String> = (0..k).map(|_| self.expr(2)).collect();
                self.line(&format!("return {}", v.join(", ")));
            }
            _ => {}
        }
    }
}

pub fn program(seed: u64) -> String {
    let mut g = G { s: seed.wrapping_mul(0x9e3779b97f4a7c15) | 1, o: String::new(), sc: vec![vec![]], n: 0, lp: false, va: true, ind: 0 };
    for _ in 0..4 {
        g.r(2);
    }
    g.block(2, 5);
    g.o
}

fn strip(p: &mut crate::luac::Proto) {
    p.lines.clear();
    p.locals.clear();
    p.ups.clear();
    p.protos.iter_mut().for_each(strip);
}

fn check(src: &str) -> Option<String> {
    let mut c = parse(&compile(src.as_bytes(), "=g", Header::default(), 5).ok()?).unwrap();
    if std::env::var("AC_GEN_STRIP").is_ok() {
        strip(&mut c.main);
    }
    let r = super::source(&c).and_then(|o| compile(o.as_bytes(), "=g", Header::default(), 5).map(|b| (o, b)));
    match r {
        Ok((_, b)) if super::same(&parse(&b).unwrap().main, &c.main) => None,
        Ok((o, _)) => Some(o),
        Err(e) => Some(format!("ERR {e}")),
    }
}

fn shrink(src: &str) -> String {
    let mut v: Vec<String> = src.lines().map(String::from).collect();
    let mut chunk = v.len() / 2;
    while chunk >= 1 {
        let mut i = 0;
        while i < v.len() {
            let mut w = v.clone();
            w.drain(i..(i + chunk).min(w.len()));
            if check(&w.join("\n")).is_some() {
                v = w;
            } else {
                i += 1;
            }
        }
        chunk /= 2;
    }
    v.join("\n")
}

#[test]
#[ignore]
fn random() {
    let n: u64 = std::env::var("AC_GEN").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let from: u64 = std::env::var("AC_GEN_FROM").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let show: usize = std::env::var("AC_GEN_SHOW").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let (mut bad, mut skip, mut seen) = (0, 0, std::collections::HashSet::new());
    for seed in from..from + n {
        let src = program(seed);
        if compile(src.as_bytes(), "=g", Header::default(), 5).is_err() {
            skip += 1;
            continue;
        }
        if check(&src).is_some() {
            bad += 1;
            if seen.len() < show {
                let m = shrink(&src);
                if seen.insert(m.clone()) {
                    println!("##### seed {seed}\n{m}\n----- out\n{}", check(&m).unwrap_or_default());
                }
            }
        }
    }
    println!("bad {bad}/{} (skipped {skip})", n - skip);
}

#[test]
#[ignore]
fn src() {
    let Ok(f) = std::env::var("AC_SRC") else { return };
    for s in std::fs::read_to_string(f).unwrap().split("\n--\n") {
        let mut c = parse(&compile(s.as_bytes(), "=g", Header::default(), 5).unwrap()).unwrap();
        strip(&mut c.main);
        let o = super::source(&c).unwrap_or_else(|e| format!("ERR {e}"));
        let bad = check(s).is_some();
        println!("=== {}\n{o}--- {}", s.trim(), if bad { "DIFF" } else { "OK" });
        if let (true, Ok(m)) = (bad, compile(o.as_bytes(), "=g", Header::default(), 5).and_then(|b| parse(&b))) {
            diff(&c.main, &m.main);
        }
    }
}

fn diff(a: &crate::luac::Proto, b: &crate::luac::Proto) -> bool {
    if a.protos.len() == b.protos.len() && a.protos.iter().zip(&b.protos).any(|(x, y)| diff(x, y)) {
        return true;
    }
    if a.code != b.code || a.consts != b.consts {
        println!("ORIG:\n{}MINE:\n{}", crate::luac::disasm(a), crate::luac::disasm(b));
        return true;
    }
    false
}
