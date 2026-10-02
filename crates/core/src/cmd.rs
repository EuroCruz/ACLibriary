use crate::{bad, Error, Res};
use std::collections::VecDeque;
use std::str::FromStr;

pub type Run<C> = Box<dyn Fn(&mut C, &[String]) -> Res<String>>;

struct Cmd<C> {
    name: String,
    help: String,
    words: Vec<String>,
    run: Run<C>,
}

pub struct Console<C> {
    cmds: Vec<Cmd<C>>,
    fallback: Option<Run<C>>,
    out: VecDeque<String>,
    cap: usize,
    hist: Vec<String>,
    at: usize,
}

pub fn split(line: &str) -> Vec<String> {
    let (mut o, mut cur, mut q, mut has, mut esc) = (Vec::new(), String::new(), false, false, false);
    for c in line.chars() {
        match (esc, q, c) {
            (true, _, c) => {
                cur.push(c);
                esc = false;
            }
            (false, true, '\\') => esc = true,
            (false, _, '"') => {
                q = !q;
                has = true;
            }
            (false, false, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    o.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (false, _, c) => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        o.push(cur);
    }
    o
}

pub fn arg<T: FromStr>(a: &[String], i: usize) -> Res<T> {
    a.get(i).ok_or(Error::Bad("missing argument"))?.parse().map_err(|_| Error::Bad("bad argument"))
}

pub fn arg_or<T: FromStr>(a: &[String], i: usize, d: T) -> T {
    a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d)
}

pub fn common_prefix(w: &[String]) -> String {
    let Some(f) = w.first() else { return String::new() };
    let n = w[1..].iter().fold(f.chars().count(), |n, x| n.min(f.chars().zip(x.chars()).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).count()));
    f.chars().take(n).collect()
}

impl<C> Console<C> {
    pub fn new(cap: usize) -> Self {
        Console { cmds: Vec::new(), fallback: None, out: VecDeque::new(), cap, hist: Vec::new(), at: 0 }
    }

    pub fn add(&mut self, name: &str, help: &str, f: impl Fn(&mut C, &[String]) -> Res<String> + 'static) -> &mut Self {
        self.cmds.push(Cmd { name: name.to_string(), help: help.to_string(), words: Vec::new(), run: Box::new(f) });
        self
    }

    pub fn words(&mut self, name: &str, w: Vec<String>) -> &mut Self {
        if let Some(c) = self.cmds.iter_mut().find(|c| c.name.eq_ignore_ascii_case(name)) {
            c.words = w;
        }
        self
    }

    pub fn fallback(&mut self, f: impl Fn(&mut C, &[String]) -> Res<String> + 'static) -> &mut Self {
        self.fallback = Some(Box::new(f));
        self
    }

    pub fn names(&self) -> Vec<&str> {
        self.cmds.iter().map(|c| c.name.as_str()).collect()
    }

    pub fn help(&self) -> String {
        self.cmds.iter().map(|c| format!("{} - {}", c.name, c.help)).collect::<Vec<_>>().join("\n")
    }

    pub fn print(&mut self, s: &str) {
        for l in s.lines() {
            if self.out.len() == self.cap {
                self.out.pop_front();
            }
            self.out.push_back(l.to_string());
        }
    }

    pub fn lines(&self) -> impl Iterator<Item = &String> {
        self.out.iter()
    }

    pub fn clear(&mut self) {
        self.out.clear();
    }

    pub fn exec(&mut self, ctx: &mut C, line: &str) -> Res<String> {
        let line = line.trim();
        if line.is_empty() {
            return bad("empty command");
        }
        if self.hist.last().map(String::as_str) != Some(line) {
            self.hist.push(line.to_string());
        }
        self.at = self.hist.len();
        let t = split(line);
        let r = match self.cmds.iter().find(|c| c.name.eq_ignore_ascii_case(&t[0])) {
            Some(c) => (c.run)(ctx, &t[1..]),
            None => match &self.fallback {
                Some(f) => f(ctx, &[line.to_string()]),
                None => Err(Error::Msg(format!("unknown command: {}", t[0]))),
            },
        };
        match &r {
            Ok(s) => self.print(s),
            Err(e) => self.print(&format!("error: {e}")),
        }
        r
    }

    pub fn prev(&mut self) -> Option<&str> {
        self.at = self.at.checked_sub(1)?;
        self.hist.get(self.at).map(String::as_str)
    }

    pub fn next(&mut self) -> Option<&str> {
        if self.at + 1 >= self.hist.len() {
            self.at = self.hist.len();
            return None;
        }
        self.at += 1;
        self.hist.get(self.at).map(String::as_str)
    }

    pub fn complete(&self, line: &str) -> (usize, Vec<String>) {
        let start = line.rfind(char::is_whitespace).map_or(0, |i| i + 1);
        let pre = line[start..].to_ascii_lowercase();
        let pool: Vec<String> = if start == 0 {
            self.cmds.iter().map(|c| c.name.clone()).collect()
        } else {
            let name = line.split_whitespace().next().unwrap_or("");
            self.cmds.iter().find(|c| c.name.eq_ignore_ascii_case(name)).map_or_else(Vec::new, |c| c.words.clone())
        };
        (start, pool.into_iter().filter(|w| w.to_ascii_lowercase().starts_with(&pre)).collect())
    }

    pub fn apply(&self, line: &str) -> String {
        let (s, c) = self.complete(line);
        if c.is_empty() {
            return line.to_string();
        }
        let p = if c.len() == 1 { format!("{} ", c[0]) } else { common_prefix(&c) };
        format!("{}{p}", &line[..s])
    }
}

#[cfg(test)]
mod t {
    use super::*;

    fn con() -> Console<Vec<i32>> {
        let mut c = Console::new(4);
        c.add("add", "push number", |v: &mut Vec<i32>, a| {
            v.push(arg(a, 0)?);
            Ok(format!("ok {}", v.len()))
        });
        c.add("clear", "clear list", |v, _| {
            v.clear();
            Ok(String::new())
        });
        c.add("car", "spawn car", |_, a| Ok(a.join("|")));
        c.words("car", vec!["alfa".into(), "audi".into(), "bmw".into()]);
        c
    }

    #[test]
    fn tokens() {
        assert_eq!(split(r#"a  "b c" d "" f"#), ["a", "b c", "d", "", "f"]);
        assert_eq!(split(r"C:\dir\file x"), [r"C:\dir\file", "x"]);
        assert_eq!(split("  "), Vec::<String>::new());
        assert_eq!(split(r#"x "q\"r""#), ["x", "q\"r"]);
    }

    #[test]
    fn running() {
        let (mut c, mut v) = (con(), Vec::new());
        assert_eq!(c.exec(&mut v, "ADD 5").unwrap(), "ok 1");
        assert!(c.exec(&mut v, "add x").is_err());
        assert!(c.exec(&mut v, "nope").is_err());
        assert_eq!(c.exec(&mut v, r#"car "a b" c"#).unwrap(), "a b|c");
        assert_eq!(v, [5]);
        assert_eq!(c.lines().count(), 4);
        c.exec(&mut v, "clear").unwrap();
        assert!(c.lines().count() <= 4);
        c.fallback(|_, a| Ok(format!("lua:{}", a[0])));
        assert_eq!(c.exec(&mut v, "print(1)").unwrap(), "lua:print(1)");
    }

    #[test]
    fn completion() {
        let c = con();
        assert_eq!(c.complete("ca").1, ["car"]);
        assert_eq!(c.apply("ad"), "add ");
        assert_eq!(c.apply("c"), "c");
        assert_eq!(c.complete("car a").1, ["alfa", "audi"]);
        assert_eq!(c.apply("car a"), "car a");
        assert_eq!(c.apply("car b"), "car bmw ");
        assert_eq!(common_prefix(&["Alfa".into(), "alpha".into()]), "Al");
    }

    #[test]
    fn history() {
        let (mut c, mut v) = (con(), Vec::new());
        c.exec(&mut v, "add 1").unwrap();
        c.exec(&mut v, "add 2").unwrap();
        assert_eq!(c.prev(), Some("add 2"));
        assert_eq!(c.prev(), Some("add 1"));
        assert_eq!(c.prev(), None);
        assert_eq!(c.next(), Some("add 2"));
        assert_eq!(c.next(), None);
    }
}
