use crate::Res;
use std::fmt::Display;
use std::path::Path;
use std::str::FromStr;

#[derive(Clone, Debug)]
enum Line {
    Raw(String),
    Sec(String),
    Kv { key: String, val: String, quoted: bool, tail: String, raw: Option<String> },
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    l: Vec<Line>,
}

fn split_val(s: &str) -> (String, String, bool) {
    let s = s.trim_start();
    if let Some(r) = s.strip_prefix('"') {
        let (mut v, mut esc) = (String::new(), false);
        for (i, c) in r.char_indices() {
            match (esc, c) {
                (true, 'n') => v.push('\n'),
                (true, c) => v.push(c),
                (false, '\\') => {
                    esc = true;
                    continue;
                }
                (false, '"') => return (v, r[i + 1..].trim_end().to_string(), true),
                (false, c) => v.push(c),
            }
            esc = false;
        }
        return (v, String::new(), true);
    }
    match s.find('#') {
        Some(i) => (s[..i].trim().to_string(), format!(" {}", s[i..].trim_end()), false),
        None => (s.trim().to_string(), String::new(), false),
    }
}

fn quote(v: &str) -> String {
    let mut o = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn flag(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

impl Doc {
    pub fn parse(text: &str) -> Doc {
        let l = text
            .lines()
            .map(|ln| {
                let t = ln.trim();
                if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
                    Line::Raw(ln.to_string())
                } else if let Some(n) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                    Line::Sec(n.trim().to_string())
                } else if let Some((k, v)) = ln.split_once('=') {
                    let (val, tail, quoted) = split_val(v);
                    Line::Kv { key: k.trim().to_string(), val, quoted, tail, raw: Some(ln.to_string()) }
                } else {
                    Line::Raw(ln.to_string())
                }
            })
            .collect();
        Doc { l }
    }

    fn find(&self, sec: &str, key: &str) -> Option<usize> {
        let mut cur = "";
        for (i, l) in self.l.iter().enumerate() {
            match l {
                Line::Sec(n) => cur = n,
                Line::Kv { key: k, .. } if same(cur, sec) && same(k, key) => return Some(i),
                _ => {}
            }
        }
        None
    }

    pub fn get(&self, sec: &str, key: &str) -> Option<&str> {
        match self.l.get(self.find(sec, key)?)? {
            Line::Kv { val, .. } => Some(val),
            _ => None,
        }
    }

    pub fn parse_as<T: FromStr>(&self, sec: &str, key: &str) -> Option<T> {
        self.get(sec, key)?.parse().ok()
    }

    pub fn bool(&self, sec: &str, key: &str) -> Option<bool> {
        flag(self.get(sec, key)?)
    }

    pub fn int(&self, sec: &str, key: &str) -> Option<i64> {
        self.parse_as(sec, key)
    }

    pub fn float(&self, sec: &str, key: &str) -> Option<f64> {
        self.parse_as(sec, key)
    }

    pub fn or<T: FromStr>(&self, sec: &str, key: &str, d: T) -> T {
        self.parse_as(sec, key).unwrap_or(d)
    }

    pub fn flag_or(&self, sec: &str, key: &str, d: bool) -> bool {
        self.bool(sec, key).unwrap_or(d)
    }

    pub fn sections(&self) -> Vec<&str> {
        self.l.iter().filter_map(|l| if let Line::Sec(n) = l { Some(n.as_str()) } else { None }).collect()
    }

    pub fn keys(&self, sec: &str) -> Vec<&str> {
        let mut cur = "";
        let mut o = Vec::new();
        for l in &self.l {
            match l {
                Line::Sec(n) => cur = n,
                Line::Kv { key, .. } if same(cur, sec) => o.push(key.as_str()),
                _ => {}
            }
        }
        o
    }

    fn put(&mut self, sec: &str, key: &str, val: String, quoted: bool) {
        if let Some(i) = self.find(sec, key) {
            if let Line::Kv { val: v, quoted: q, raw, .. } = &mut self.l[i] {
                *v = val;
                *q = quoted;
                *raw = None;
            }
            return;
        }
        let at = self.slot(sec);
        self.l.insert(at, Line::Kv { key: key.to_string(), val, quoted, tail: String::new(), raw: None });
    }

    fn slot(&mut self, sec: &str) -> usize {
        self.sec_end(sec).unwrap_or_else(|| {
            if !sec.is_empty() {
                self.l.push(Line::Sec(sec.to_string()));
            }
            self.l.len()
        })
    }
    fn sec_end(&self, sec: &str) -> Option<usize> {
        let mut cur = "";
        let mut found = sec.is_empty();
        let mut end = if sec.is_empty() { Some(self.l.iter().position(|l| matches!(l, Line::Sec(_))).unwrap_or(self.l.len())) } else { None };
        for (i, l) in self.l.iter().enumerate() {
            match l {
                Line::Sec(n) => {
                    if found && !sec.is_empty() {
                        break;
                    }
                    cur = n;
                    if same(cur, sec) {
                        found = true;
                        end = Some(i + 1);
                    }
                }
                Line::Kv { .. } if found && same(cur, sec) => end = Some(i + 1),
                _ => {}
            }
        }
        end
    }

    pub fn set(&mut self, sec: &str, key: &str, v: impl Display) {
        self.put(sec, key, v.to_string(), false);
    }

    pub fn set_str(&mut self, sec: &str, key: &str, v: &str) {
        self.put(sec, key, v.to_string(), true);
    }

    pub fn remove(&mut self, sec: &str, key: &str) -> bool {
        self.find(sec, key).map(|i| self.l.remove(i)).is_some()
    }

    pub fn merge(&mut self, defaults: &Doc) -> usize {
        let mut n = 0;
        let mut cur = String::new();
        for l in &defaults.l {
            match l {
                Line::Sec(s) => cur = s.clone(),
                Line::Kv { key, .. } if self.find(&cur, key).is_none() => {
                    let at = self.slot(&cur);
                    self.l.insert(at, l.clone());
                    n += 1;
                }
                _ => {}
            }
        }
        n
    }
    pub fn load(path: &Path, defaults: &str) -> Res<Doc> {
        let def = Doc::parse(defaults);
        let mut doc = match std::fs::read_to_string(path) {
            Ok(t) => Doc::parse(&t),
            Err(_) => {
                if let Some(p) = path.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::write(path, defaults)?;
                return Ok(def);
            }
        };
        if doc.merge(&def) > 0 {
            doc.save(path)?;
        }
        Ok(doc)
    }

    pub fn save(&self, path: &Path) -> Res<()> {
        Ok(std::fs::write(path, self.to_string())?)
    }
}

impl Display for Doc {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for l in &self.l {
            match l {
                Line::Raw(s) => writeln!(f, "{s}")?,
                Line::Sec(n) => writeln!(f, "[{n}]")?,
                Line::Kv { raw: Some(r), .. } => writeln!(f, "{r}")?,
                Line::Kv { key, val, quoted, tail, .. } => {
                    let v = if *quoted { quote(val) } else { val.clone() };
                    writeln!(f, "{key} = {v}{tail}")?
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod t {
    use super::*;

    const TXT: &str = "top = 1\n# note\n[Main]\nFPS = 60\t# cap\nLang = \"en\"  # lang\n\n[Keys]\nConsole = \"`\"\n";

    #[test]
    fn reads() {
        let d = Doc::parse(TXT);
        assert_eq!(d.int("", "top"), Some(1));
        assert_eq!(d.int("main", "fps"), Some(60));
        assert_eq!(d.get("Main", "Lang"), Some("en"));
        assert_eq!(d.get("Keys", "Console"), Some("`"));
        assert_eq!(d.or("Main", "missing", 5), 5);
        assert!(d.flag_or("Main", "nope", true));
        assert_eq!(d.sections(), ["Main", "Keys"]);
        assert_eq!(d.keys("Main"), ["FPS", "Lang"]);
    }

    #[test]
    fn roundtrip_preserves_text() {
        assert_eq!(Doc::parse(TXT).to_string(), TXT);
    }

    #[test]
    fn edits() {
        let mut d = Doc::parse(TXT);
        d.set("Main", "FPS", 120);
        d.set_str("Main", "Name", "a \"b\"");
        d.set("New", "x", true);
        d.set("", "late", 2);
        let r = Doc::parse(&d.to_string());
        assert_eq!(r.int("Main", "FPS"), Some(120));
        assert_eq!(r.get("Main", "Name"), Some("a \"b\""));
        assert_eq!(r.bool("New", "x"), Some(true));
        assert_eq!(r.int("", "late"), Some(2));
        assert!(d.to_string().contains("# cap"));
        assert!(d.remove("Main", "Lang") && !d.remove("Main", "Lang"));
    }

    #[test]
    fn merging() {
        let mut d = Doc::parse("[Main]\nFPS = 30\n");
        let n = d.merge(&Doc::parse(TXT));
        assert_eq!(n, 3);
        assert_eq!(d.int("Main", "FPS"), Some(30));
        assert_eq!(d.get("Keys", "Console"), Some("`"));
        assert_eq!(d.int("", "top"), Some(1));
    }

    #[test]
    fn files() {
        let p = std::env::temp_dir().join(format!("accfg_{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let d = Doc::load(&p, TXT).unwrap();
        assert_eq!(d.int("Main", "FPS"), Some(60));
        std::fs::write(&p, "[Main]\nFPS = 5\n").unwrap();
        let d = Doc::load(&p, TXT).unwrap();
        assert_eq!(d.int("Main", "FPS"), Some(5));
        assert_eq!(Doc::parse(&std::fs::read_to_string(&p).unwrap()).get("Keys", "Console"), Some("`"));
        let _ = std::fs::remove_file(&p);
    }
}
