use super::block::{self, Bl, Kind};
use super::expr::{index, seq, Expr, D, E};
use super::out::Out;
use super::R;
use ac_core::bad;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub enum Target {
    Global(String),
    Index { t: E, k: E },
    Up(String),
    Var(D),
}

impl Target {
    pub fn print(&self, o: &mut Out) -> R {
        match self {
            Target::Global(n) | Target::Up(n) => o.p(n),
            Target::Index { t, k } => index(o, t, k)?,
            Target::Var(d) => o.p(&d.name),
        }
        Ok(())
    }

    pub fn print_method(&self, o: &mut Out) -> R {
        let Target::Index { t, k } = self else { return bad("method target expected") };
        t.print(o)?;
        o.p(":");
        o.p(&k.name());
        Ok(())
    }

    pub fn is(&self, d: &D) -> bool {
        matches!(self, Target::Var(x) if Rc::ptr_eq(x, d))
    }

    fn fname(&self) -> bool {
        match self {
            Target::Index { t, k } => k.is_ident() && t.dotted(),
            _ => true,
        }
    }

    fn paren(&self) -> bool {
        match self {
            Target::Index { t, .. } => t.ungrouped() || t.paren(),
            _ => false,
        }
    }

    fn same(&self, x: &Target) -> bool {
        matches!((self, x), (Target::Var(a), Target::Var(b)) if Rc::ptr_eq(a, b))
    }
}

pub struct Assign {
    pub ts: Vec<Target>,
    pub vs: Vec<E>,
    nil: bool,
    decl: bool,
    pub head: bool,
}

pub type A = Rc<RefCell<Assign>>;

impl Assign {
    pub fn empty() -> Assign {
        Assign { ts: Vec::new(), vs: Vec::new(), nil: true, decl: false, head: false }
    }

    pub fn new(t: Target, v: E) -> Assign {
        Assign { nil: v.is_nil(), ts: vec![t], vs: vec![v], decl: false, head: false }
    }

    pub fn rc(t: Target, v: E) -> A {
        Rc::new(RefCell::new(Assign::new(t, v)))
    }

    pub fn first(&self) -> (&Target, &E) {
        (&self.ts[0], &self.vs[0])
    }

    pub fn push_front(&mut self, t: Target, v: E) {
        self.nil = self.nil && v.is_nil();
        self.ts.insert(0, t);
        self.vs.insert(0, v);
    }

    pub fn push(&mut self, t: Target, mut v: E) {
        while self.vs.len() < self.ts.len() {
            self.vs.push(super::expr::nil());
        }
        if let Some(i) = self.ts.iter().position(|x| x.same(&t)) {
            self.ts.remove(i);
            v = self.vs.remove(i);
        }
        self.nil = self.nil && v.is_nil();
        self.ts.push(t);
        self.vs.push(v);
    }

    pub fn trim(&mut self) {
        while self.vs.len() > 1 && self.vs[self.vs.len() - 1].is_nil() && !self.vs[self.vs.len() - 2].multi() {
            self.vs.pop();
        }
    }

    pub fn extra(&mut self, v: E) {
        self.nil = self.nil && v.is_nil();
        self.vs.push(v);
    }

    pub fn declare(&mut self) {
        self.decl = true;
    }

    pub fn is_fn(&self) -> bool {
        self.ts.len() == 1
            && self.vs.len() == 1
            && self.ts[0].fname()
            && match &*self.vs[0] {
                Expr::Closure { f, own } => !self.decl || *own || !matches!(&self.ts[0], Target::Var(d) if f.globals.contains(d.name.as_bytes()) || f.ups.contains(&d.name)),
                _ => false,
            }
    }

    pub fn print(&self, o: &mut Out) -> R {
        if self.ts.is_empty() {
            return Ok(());
        }
        if self.decl {
            o.p("local ");
        }
        if self.is_fn() {
            return self.vs[0].print_closure(o, &self.ts[0]);
        }
        for (i, t) in self.ts.iter().enumerate() {
            if i > 0 {
                o.p(", ");
            }
            t.print(o)?;
        }
        if !self.decl || !self.nil {
            o.eq();
            seq(o, &self.vs, false, self.vs.len() != self.ts.len())?;
        }
        Ok(())
    }
}

pub enum Stmt {
    Assign(A),
    Call(E),
    Ret(Vec<E>),
    Block(Bl),
}

impl Stmt {
    fn paren(&self) -> bool {
        match self {
            Stmt::Assign(a) => a.borrow().ts[0].paren(),
            Stmt::Call(c) => c.paren(),
            _ => false,
        }
    }

    pub fn print(&self, o: &mut Out) -> R {
        match self {
            Stmt::Assign(a) => a.borrow().print(o),
            Stmt::Call(c) => c.print(o),
            Stmt::Ret(v) => {
                o.p("do ");
                ret(o, v)?;
                o.p(" end");
                Ok(())
            }
            Stmt::Block(b) => block::print(b, o),
        }
    }

    fn print_tail(&self, o: &mut Out) -> R {
        match self {
            Stmt::Ret(v) => ret(o, v),
            Stmt::Block(b) if matches!(b.borrow().kind, Kind::Break { .. }) => Ok(o.p("break")),
            _ => self.print(o),
        }
    }

    fn kind(&self, f: fn(&Kind) -> bool) -> bool {
        matches!(self, Stmt::Block(b) if f(&b.borrow().kind))
    }

    fn is_fn(&self) -> bool {
        matches!(self, Stmt::Assign(a) if a.borrow().is_fn())
    }
}

fn ret(o: &mut Out, v: &[E]) -> R {
    o.p("return");
    if !v.is_empty() {
        o.p(" ");
        seq(o, v, false, true)?;
    }
    Ok(())
}

fn blank(prev: &Stmt, cur: &Stmt, spaced: bool) -> bool {
    if matches!(prev, Stmt::Assign(a) if a.borrow().head) {
        return false;
    }
    let else_end = |s: &Stmt| s.kind(|k| matches!(k, Kind::ElseEnd));
    let compound = |s: &Stmt| s.kind(Kind::compound);
    !else_end(cur) && (prev.is_fn() || (spaced && (compound(prev) || else_end(prev))) || cur.is_fn() || (spaced && compound(cur)))
}

pub fn print_all(o: &mut Out, v: &[Stmt], spaced: bool) -> R {
    for (i, s) in v.iter().enumerate() {
        if i > 0 && blank(&v[i - 1], s, spaced) {
            o.nl();
        }
        if i > 0 && s.paren() {
            o.p(";");
        }
        if i + 1 == v.len() {
            s.print_tail(o)?;
        } else {
            s.print(o)?;
        }
        if !s.kind(|k| matches!(k, Kind::IfElse { .. })) {
            o.nl();
        }
    }
    Ok(())
}
