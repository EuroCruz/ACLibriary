use std::fmt::{self, Display};

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Nil,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    Raw(String),
    Tbl(Vec<(Option<String>, Val)>),
}

const KEYWORDS: [&str; 22] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil", "not", "or", "repeat",
    "return", "then", "true", "until", "while", "goto",
];

pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for b in s.bytes() {
        match b {
            b'"' => o.push_str("\\\""),
            b'\\' => o.push_str("\\\\"),
            b'\n' => o.push_str("\\n"),
            b'\r' => o.push_str("\\r"),
            b'\t' => o.push_str("\\t"),
            0x20..=0x7e => o.push(b as char),
            _ => o.push_str(&format!("\\{b:03}")),
        }
    }
    o.push('"');
    o
}

fn ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_') && !KEYWORDS.contains(&s)
}

impl Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Val::Nil => f.write_str("nil"),
            Val::Bool(b) => write!(f, "{b}"),
            Val::Int(i) => write!(f, "{i}"),
            Val::Num(n) if n.is_nan() => f.write_str("(0/0)"),
            Val::Num(n) if n.is_infinite() => f.write_str(if *n > 0.0 { "math.huge" } else { "(-math.huge)" }),
            Val::Num(n) => write!(f, "{n:?}"),
            Val::Str(s) => f.write_str(&quote(s)),
            Val::Raw(s) => f.write_str(s),
            Val::Tbl(items) => {
                f.write_str("{")?;
                for (i, (k, v)) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    match k {
                        Some(k) if ident(k) => write!(f, "{k} = {v}")?,
                        Some(k) => write!(f, "[{}] = {v}", quote(k))?,
                        None => write!(f, "{v}")?,
                    }
                }
                f.write_str("}")
            }
        }
    }
}

macro_rules! from {
    ($($t:ty => $v:ident as $c:ty),*) => {$(
        impl From<$t> for Val {
            fn from(x: $t) -> Val {
                Val::$v(x as $c)
            }
        }
    )*};
}

from!(i32 => Int as i64, i64 => Int as i64, u32 => Int as i64, u8 => Int as i64, f32 => Num as f64, f64 => Num as f64);

impl From<bool> for Val {
    fn from(b: bool) -> Val {
        Val::Bool(b)
    }
}

impl From<&str> for Val {
    fn from(s: &str) -> Val {
        Val::Str(s.to_string())
    }
}

impl From<String> for Val {
    fn from(s: String) -> Val {
        Val::Str(s)
    }
}

impl<T: Into<Val>> From<Vec<T>> for Val {
    fn from(v: Vec<T>) -> Val {
        Val::Tbl(v.into_iter().map(|x| (None, x.into())).collect())
    }
}

impl<T: Into<Val>> From<Option<T>> for Val {
    fn from(o: Option<T>) -> Val {
        o.map_or(Val::Nil, Into::into)
    }
}

pub fn call(name: &str, args: &[Val]) -> String {
    format!("{name}({})", args.iter().map(Val::to_string).collect::<Vec<_>>().join(", "))
}

pub fn guard(body: &str) -> String {
    format!("local ok, err = pcall(function()\n{body}\nend)\nif not ok then print(\"Error: \"..tostring(err)) end")
}

#[derive(Default, Clone)]
pub struct Script {
    l: Vec<String>,
}

impl Script {
    pub fn new() -> Script {
        Script::default()
    }

    pub fn line(&mut self, s: impl Into<String>) -> &mut Self {
        self.l.push(s.into());
        self
    }

    pub fn local(&mut self, name: &str, v: impl Into<Val>) -> &mut Self {
        self.line(format!("local {name} = {}", v.into()))
    }

    pub fn set(&mut self, name: &str, v: impl Into<Val>) -> &mut Self {
        self.line(format!("{name} = {}", v.into()))
    }

    pub fn call(&mut self, name: &str, args: &[Val]) -> &mut Self {
        self.line(call(name, args))
    }

    pub fn build(&self) -> String {
        self.l.join("\n")
    }

    pub fn guarded(&self) -> String {
        guard(&self.build())
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn literals() {
        assert_eq!(quote("a\"b\\c\n\u{e9}"), "\"a\\\"b\\\\c\\n\\195\\169\"");
        assert_eq!(Val::from(true).to_string(), "true");
        assert_eq!(Val::from(1.5f32).to_string(), "1.5");
        assert_eq!(Val::from(2.0f64).to_string(), "2.0");
        assert_eq!(Val::Num(f64::NAN).to_string(), "(0/0)");
        assert_eq!(Val::Num(f64::NEG_INFINITY).to_string(), "(-math.huge)");
        assert_eq!(Val::from(None::<i32>).to_string(), "nil");
    }

    #[test]
    fn tables() {
        let t = Val::Tbl(vec![(Some("x".into()), 1.into()), (Some("end".into()), 2.into()), (Some("a b".into()), "s".into()), (None, vec![1, 2].into())]);
        assert_eq!(t.to_string(), "{x = 1, [\"end\"] = 2, [\"a b\"] = \"s\", {1, 2}}");
    }

    #[test]
    fn scripts() {
        let mut s = Script::new();
        s.local("h", Val::Raw("World.Find(\"Player\")".into())).call("Entity.SetGod", &[Val::Raw("h".into()), true.into()]);
        assert_eq!(s.build(), "local h = World.Find(\"Player\")\nEntity.SetGod(h, true)");
        assert!(s.guarded().starts_with("local ok, err = pcall(function()\n"));
        assert_eq!(call("f", &[]), "f()");
    }
}
