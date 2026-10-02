use super::fun::Func;
use super::out::Out;
use super::stmt::Target;
use super::R;
use crate::luac::Const;
use ac_core::bad;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub const P_OR: u8 = 1;
pub const P_AND: u8 = 2;
pub const P_CMP: u8 = 3;
const P_CAT: u8 = 8;
const P_ADD: u8 = 9;
const P_MUL: u8 = 10;
const P_UN: u8 = 11;
const P_POW: u8 = 12;
const P_ATOM: u8 = 13;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum As {
    No,
    L,
    R,
}

pub type E = Rc<Expr>;
pub type D = Rc<Decl>;

pub struct Decl {
    pub name: String,
    pub begin: i32,
    pub end: i32,
    pub reg: Cell<i32>,
    pub fl: Cell<bool>,
    pub fx: Cell<bool>,
}

impl Decl {
    pub fn new(name: &str, begin: i32, end: i32) -> D {
        Decl::at(name, begin, end, -1)
    }

    pub fn at(name: &str, begin: i32, end: i32, reg: i32) -> D {
        Rc::new(Decl { name: name.to_string(), begin, end, reg: Cell::new(reg), fl: Cell::new(false), fx: Cell::new(false) })
    }
}

#[derive(Clone)]
pub struct Entry {
    pub key: E,
    pub val: E,
    pub list: bool,
    pub ts: i32,
}

pub struct Tab {
    pub ents: Vec<Entry>,
    pub obj: bool,
    pub list: bool,
    pub cap: i32,
}

pub enum Expr {
    K { k: Const, i: i32 },
    Global { name: String, i: i32 },
    Local(D),
    Up(String),
    Vararg(bool),
    Index { t: E, k: E },
    Call { f: E, args: Vec<E>, multi: bool },
    Bin { op: &'static str, l: E, r: E, prec: u8, assoc: As },
    Un { op: &'static str, e: E },
    Table(RefCell<Tab>),
    Closure { f: Rc<Func>, own: bool },
}

const RESERVED: [&str; 21] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

pub fn ident(s: &[u8]) -> bool {
    !s.is_empty()
        && std::str::from_utf8(s).map_or(true, |t| !RESERVED.contains(&t))
        && (s[0] == b'_' || s[0].is_ascii_alphabetic())
        && s[1..].iter().all(|&c| c == b'_' || c.is_ascii_alphanumeric())
}

pub fn k(k: Const) -> E {
    Rc::new(Expr::K { k, i: -1 })
}

pub fn nil() -> E {
    k(Const::Nil)
}

pub fn bin(op: &'static str, l: E, r: E) -> E {
    let (prec, assoc) = match op {
        ".." => (P_CAT, As::R),
        "+" | "-" => (P_ADD, As::L),
        "*" | "/" | "%" => (P_MUL, As::L),
        "^" => (P_POW, As::R),
        _ => (P_CMP, As::L),
    };
    Rc::new(Expr::Bin { op, l, r, prec, assoc })
}

pub fn logic(and: bool, l: E, r: E) -> E {
    let (op, prec) = if and { ("and", P_AND) } else { ("or", P_OR) };
    Rc::new(Expr::Bin { op, l, r, prec, assoc: As::No })
}

pub fn un(op: &'static str, e: E) -> E {
    Rc::new(Expr::Un { op, e })
}

pub fn kexp(ks: &[Const], i: i32) -> R<E> {
    match ks.get(i as usize) {
        Some(c) => Ok(Rc::new(Expr::K { k: c.clone(), i })),
        None => bad("constant index out of range"),
    }
}

pub fn gname(ks: &[Const], i: i32) -> R<String> {
    match ks.get(i as usize) {
        Some(Const::Str(s)) => Ok(String::from_utf8_lossy(s).into_owned()),
        _ => bad("global name is not a string constant"),
    }
}

fn num(n: f64, f32: bool) -> String {
    if n.is_nan() {
        "(0/0)".into()
    } else if n.is_infinite() {
        if n > 0.0 { "1e999" } else { "-1e999" }.into()
    } else if f32 {
        format!("{}", n as f32)
    } else if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n:?}")
    }
}

fn konst(c: &Const, o: &mut Out, braced: bool) {
    match c {
        Const::Nil => o.p("nil"),
        Const::Bool(b) => o.p(if *b { "true" } else { "false" }),
        Const::Num(n) => {
            let s = num(*n, o.f32);
            o.p(&s)
        }
        Const::Str(s) => string(s, o, braced),
    }
}

fn string(s: &[u8], o: &mut Out, braced: bool) {
    let nls = s.iter().filter(|&&c| c == b'\n').count();
    let bad = s.iter().filter(|&&c| c != b'\n' && ((c <= 31 && c != b'\t') || c >= 127)).count();
    let first = s.iter().position(|&c| c == b'\n');
    if bad == 0 && (nls > 1 || (nls == 1 && first != Some(s.len() - 1))) {
        let mut pipe = (s.last() == Some(&b']')) as usize;
        let text = String::from_utf8_lossy(s).into_owned();
        let mut close = String::from("]]");
        while text.contains(&close) {
            pipe += 1;
            close = format!("]{}]", "=".repeat(pipe));
        }
        let eq = "=".repeat(pipe);
        if braced {
            o.p("(");
        }
        o.p("[");
        o.p(&eq);
        o.p("[");
        let l = o.level();
        o.set_level(0);
        o.nl();
        o.p(&text);
        o.p("]");
        o.p(&eq);
        o.p("]");
        if braced {
            o.p(")");
        }
        o.set_level(l);
    } else {
        let mut t = String::from("\"");
        for &c in s {
            match c {
                7 => t.push_str("\\a"),
                8 => t.push_str("\\b"),
                12 => t.push_str("\\f"),
                10 => t.push_str("\\n"),
                13 => t.push_str("\\r"),
                9 => t.push_str("\\t"),
                11 => t.push_str("\\v"),
                0..=31 | 127.. => t.push_str(&format!("\\{c:03}")),
                b'"' => t.push_str("\\\""),
                b'\\' => t.push_str("\\\\"),
                _ => t.push(c as char),
            }
        }
        t.push('"');
        o.p(&t);
    }
}

fn lgroup(prec: u8, assoc: As, l: &Expr) -> bool {
    prec > l.prec() || (prec == l.prec() && assoc == As::R)
}

fn rgroup(prec: u8, assoc: As, r: &Expr) -> bool {
    prec > r.prec() || (prec == r.prec() && assoc == As::L)
}

fn wrapped(o: &mut Out, e: &Expr, g: bool) -> R {
    if g {
        o.p("(");
    }
    e.print(o)?;
    if g {
        o.p(")");
    }
    Ok(())
}

impl Expr {
    pub fn prec(&self) -> u8 {
        match self {
            Expr::Bin { prec, .. } => *prec,
            Expr::Un { .. } => P_UN,
            _ => P_ATOM,
        }
    }

    pub fn kidx(&self) -> i32 {
        match self {
            Expr::K { i, .. } | Expr::Global { i, .. } => *i,
            Expr::Bin { l, r, .. } => l.kidx().max(r.kidx()),
            Expr::Un { e, .. } => e.kidx(),
            Expr::Index { t, k } => t.kidx().max(k.kidx()),
            Expr::Call { f, args, .. } => args.iter().fold(f.kidx(), |i, a| i.max(a.kidx())),
            Expr::Table(t) => t.borrow().ents.iter().fold(-1, |i, e| i.max(e.key.kidx()).max(e.val.kidx())),
            _ => -1,
        }
    }

    pub fn multi(&self) -> bool {
        matches!(self, Expr::Vararg(true) | Expr::Call { multi: true, .. })
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Expr::K { k: Const::Nil, .. })
    }

    pub fn is_k(&self) -> bool {
        matches!(self, Expr::K { .. })
    }

    pub fn int(&self) -> Option<i32> {
        match self {
            Expr::K { k: Const::Num(n), .. } if *n == n.round() => Some(*n as i32),
            _ => None,
        }
    }

    pub fn is_ident(&self) -> bool {
        matches!(self, Expr::K { k: Const::Str(s), .. } if ident(s))
    }

    pub fn name(&self) -> String {
        match self {
            Expr::K { k: Const::Str(s), .. } => String::from_utf8_lossy(s).into_owned(),
            _ => String::new(),
        }
    }

    pub fn dotted(&self) -> bool {
        match self {
            Expr::Global { .. } | Expr::Local(_) | Expr::Up(_) => true,
            Expr::Index { t, k } => k.is_ident() && t.dotted(),
            _ => false,
        }
    }

    pub fn brief(&self) -> bool {
        match self {
            Expr::K { k: Const::Str(s), .. } => s.len() <= 32,
            Expr::K { .. } | Expr::Global { .. } | Expr::Local(_) | Expr::Up(_) => true,
            Expr::Table(t) => {
                let t = t.borrow();
                t.ents.len() <= 3 && t.ents.iter().all(|e| e.val.brief() && !e.val.multi())
            }
            _ => false,
        }
    }

    pub fn ungrouped(&self) -> bool {
        match self {
            Expr::K { .. } | Expr::Un { .. } | Expr::Table(_) | Expr::Closure { .. } => true,
            Expr::Bin { .. } => !self.paren(),
            _ => false,
        }
    }

    pub fn paren(&self) -> bool {
        match self {
            Expr::Bin { l, prec, assoc, .. } => lgroup(*prec, *assoc, l) || l.paren(),
            Expr::Index { t, .. } => t.ungrouped() || t.paren(),
            Expr::Call { f, args, .. } => {
                let x = method(f, args).unwrap_or(f);
                x.ungrouped() || x.paren()
            }
            _ => false,
        }
    }

    pub fn refs(&self, x: &Expr) -> bool {
        std::ptr::eq(self, x)
            || match self {
                Expr::Index { t, k } => t.refs(x) || k.refs(x),
                Expr::Call { f, args, .. } => f.refs(x) || args.iter().any(|a| a.refs(x)),
                Expr::Bin { l, r, .. } => l.refs(x) || r.refs(x),
                Expr::Un { e, .. } => e.refs(x),
                Expr::Table(t) => t.try_borrow().map_or(true, |t| t.ents.iter().any(|e| e.key.refs(x) || e.val.refs(x))),
                _ => false,
            }
    }

    fn has_closure(&self) -> bool {
        match self {
            Expr::Closure { .. } => true,
            Expr::Index { t, k } => t.has_closure() || k.has_closure(),
            Expr::Call { f, args, .. } => f.has_closure() || args.iter().any(|a| a.has_closure()),
            Expr::Bin { l, r, .. } => l.has_closure() || r.has_closure(),
            Expr::Un { e, .. } => e.has_closure(),
            Expr::Table(t) => t.borrow().ents.iter().any(|x| x.key.has_closure() || x.val.has_closure()),
            _ => false,
        }
    }

    fn wide(&self, o: &Out) -> R<bool> {
        Ok(o.wraps() && !self.has_closure() && o.col() + width(self, o)? > o.lim())
    }

    pub fn print(&self, o: &mut Out) -> R {
        match self {
            Expr::K { k, .. } => konst(k, o, false),
            Expr::Global { name, .. } | Expr::Up(name) => o.p(name),
            Expr::Local(d) => o.p(&d.name),
            Expr::Vararg(m) => o.p(if *m { "..." } else { "(...)" }),
            Expr::Bin { op, l, r, prec, assoc } => {
                if matches!(*op, "and" | "or" | "..") && self.wide(o)? {
                    let mut v = Vec::new();
                    chain(self, op, &mut v);
                    return wrap_chain(o, op, *prec, &v);
                }
                wrapped(o, l, lgroup(*prec, *assoc, l))?;
                o.p(" ");
                o.p(op);
                o.p(" ");
                wrapped(o, r, rgroup(*prec, *assoc, r))?;
            }
            Expr::Un { op, e } => {
                o.p(op);
                wrapped(o, e, P_UN > e.prec())?;
            }
            Expr::Index { t, k } => index(o, t, k)?,
            Expr::Call { f, args, .. } => {
                let wide = self.wide(o)?;
                let rest = match method(f, args) {
                    Some(t) => {
                        wrapped(o, t, t.ungrouped())?;
                        o.p(":");
                        if let Expr::Index { k, .. } = &**f {
                            o.p(&k.name());
                        }
                        &args[1..]
                    }
                    None => {
                        wrapped(o, f, f.ungrouped())?;
                        &args[..]
                    }
                };
                o.p("(");
                if wide && !rest.is_empty() {
                    wrap_args(o, rest)?;
                } else {
                    seq(o, rest, false, true)?;
                }
                o.p(")");
            }
            Expr::Table(t) => table(t, o)?,
            Expr::Closure { f, .. } => {
                o.p("function");
                f.print_fn(o, true)?;
            }
        }
        Ok(())
    }

    pub fn print_braced(&self, o: &mut Out) -> R {
        match self {
            Expr::K { k, .. } => Ok(konst(k, o, true)),
            _ => self.print(o),
        }
    }

    pub fn print_multi(&self, o: &mut Out) -> R {
        wrapped(o, self, matches!(self, Expr::Call { multi: false, .. }))
    }

    pub fn print_closure(&self, o: &mut Out, name: &Target) -> R {
        let Expr::Closure { f, .. } = self else { return bad("closure expected") };
        o.p("function ");
        if f.params >= 1 && f.decls[0].name == "self" && matches!(name, Target::Index { .. }) {
            name.print_method(o)?;
            f.print_fn(o, false)
        } else {
            name.print(o)?;
            f.print_fn(o, true)
        }
    }
}

fn width(e: &Expr, o: &Out) -> R<usize> {
    let mut f = o.flat();
    e.print(&mut f)?;
    Ok(f.done().split(['\r', '\n']).next().map_or(0, |l| l.chars().count()))
}

fn chain<'a>(e: &'a Expr, op: &str, v: &mut Vec<&'a Expr>) {
    match e {
        Expr::Bin { op: x, l, r, .. } if *x == op => {
            if op == ".." {
                v.push(l);
            } else {
                chain(l, op, v);
            }
            chain(r, op, v);
        }
        _ => v.push(e),
    }
}

fn operand(o: &mut Out, e: &Expr, prec: u8) -> R {
    wrapped(o, e, e.prec() <= prec && matches!(e, Expr::Bin { .. }))
}

fn wrap_chain(o: &mut Out, op: &str, prec: u8, v: &[&Expr]) -> R {
    operand(o, v[0], prec)?;
    let n = if op == ".." { 1 } else { 2 };
    (0..n).for_each(|_| o.indent());
    for e in &v[1..] {
        if op == ".." {
            o.p(" ..");
            o.nl();
        } else {
            o.nl();
            o.p(op);
            o.p(" ");
        }
        operand(o, e, prec)?;
    }
    (0..n).for_each(|_| o.dedent());
    Ok(())
}

fn wrap_args(o: &mut Out, args: &[E]) -> R {
    o.nl();
    o.indent();
    for (i, a) in args.iter().enumerate() {
        if i + 1 == args.len() || a.multi() {
            a.print_multi(o)?;
            break;
        }
        a.print(o)?;
        o.p(",");
        o.nl();
    }
    o.nl();
    o.dedent();
    Ok(())
}

pub fn method<'a>(f: &'a E, args: &[E]) -> Option<&'a E> {
    match &**f {
        Expr::Index { t, k } if k.is_ident() && args.first().is_some_and(|a| Rc::ptr_eq(t, a)) => Some(t),
        _ => None,
    }
}

pub fn index(o: &mut Out, t: &E, k: &E) -> R {
    wrapped(o, t, t.ungrouped())?;
    if k.is_ident() {
        o.p(".");
        o.p(&k.name());
        Ok(())
    } else {
        o.p("[");
        k.print_braced(o)?;
        o.p("]");
        Ok(())
    }
}

pub fn seq(o: &mut Out, v: &[E], brk: bool, multi: bool) -> R {
    for (i, e) in v.iter().enumerate() {
        if i + 1 == v.len() || e.multi() {
            return if multi { e.print_multi(o) } else { e.print(o) };
        }
        e.print(o)?;
        o.p(",");
        if brk {
            o.nl();
        } else {
            o.p(" ");
        }
    }
    Ok(())
}

fn table(t: &RefCell<Tab>, o: &mut Out) -> R {
    let (v, obj, list) = {
        let mut t = t.borrow_mut();
        t.ents.sort_by_key(|e| e.ts);
        (t.ents.clone(), t.obj, t.list)
    };
    if v.is_empty() {
        o.p("{}");
        return Ok(());
    }
    let brk = (list && v.len() > 5) || (obj && v.len() > 2) || !obj || v.iter().any(|e| !e.val.brief());
    o.p(if brk || !o.o.pad { "{" } else { "{ " });
    if brk {
        o.nl();
        o.indent();
    }
    if brk && numbers(&v) {
        packed(&v, o)?;
        if o.o.trail {
            o.p(",");
        }
        o.nl();
        o.dedent();
        o.p("}");
        return Ok(());
    }
    let mut n = 1;
    entry(&v, 0, o, obj, &mut n)?;
    if !v[0].val.multi() {
        for i in 1..v.len() {
            o.p(",");
            if brk {
                o.nl();
                if record(&v[i - 1]) && record(&v[i]) {
                    o.nl();
                }
            } else {
                o.p(" ");
            }
            entry(&v, i, o, obj, &mut n)?;
            if v[i].val.multi() {
                break;
            }
        }
    }
    if brk {
        if o.o.trail {
            o.p(",");
        }
        o.nl();
        o.dedent();
    } else if o.o.pad {
        o.p(" ");
    }
    o.p("}");
    Ok(())
}

fn record(e: &Entry) -> bool {
    matches!(&*e.val, Expr::Table(_)) && !e.val.brief()
}

fn numbers(v: &[Entry]) -> bool {
    v.len() > 5 && v.iter().enumerate().all(|(i, e)| e.list && e.key.int() == Some(i as i32 + 1) && matches!(&*e.val, Expr::K { k: Const::Num(_), .. }))
}

fn packed(v: &[Entry], o: &mut Out) -> R {
    let mut first = true;
    for (i, e) in v.iter().enumerate() {
        let mut f = o.flat();
        e.val.print(&mut f)?;
        let mut s = f.done();
        if i + 1 < v.len() {
            s.push(',');
        }
        if !first && o.col() + 1 + s.len() > o.lim() {
            o.nl();
            first = true;
        }
        if !first {
            o.p(" ");
        }
        o.p(&s);
        first = false;
    }
    Ok(())
}

fn entry(v: &[Entry], i: usize, o: &mut Out, obj: bool, n: &mut i32) -> R {
    let e = &v[i];
    if e.list && e.key.int() == Some(*n) {
        *n += 1;
        if i + 1 >= v.len() || e.val.multi() {
            e.val.print_multi(o)
        } else {
            e.val.print(o)
        }
    } else if obj && e.key.is_ident() {
        o.p(&e.key.name());
        o.eq();
        e.val.print(o)
    } else {
        o.p("[");
        e.key.print_braced(o)?;
        o.p("]");
        o.eq();
        e.val.print(o)
    }
}
