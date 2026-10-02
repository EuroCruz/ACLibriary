use super::expr::{bin, k, logic, un, E};
use super::out::Out;
use super::reg::Regs;
use super::stmt::{print_all, Assign, Stmt, A};
use super::R;
use crate::luac::{o, Const};
use ac_core::bad;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

pub type B = Rc<RefCell<Br>>;
pub type Bl = Rc<RefCell<Block>>;
pub type Rg = Rc<RefCell<Regs>>;

pub enum Bk {
    And(B, B),
    Or(B, B),
    Test { reg: i32, inv: bool },
    TestSet { reg: i32, inv: bool },
    Cmp { op: u8, l: i32, r: i32, inv: bool },
    True { reg: i32, inv: bool },
    Assign(Option<E>),
}

pub struct Br {
    pub line: i32,
    pub begin: i32,
    pub end: i32,
    pub set: bool,
    pub cset: bool,
    pub tgt: i32,
    pub kind: Bk,
}

fn br(line: i32, begin: i32, end: i32, tgt: i32, kind: Bk) -> B {
    Rc::new(RefCell::new(Br { line, begin, end, set: false, cset: false, tgt, kind }))
}

pub fn test(reg: i32, inv: bool, line: i32, begin: i32, end: i32) -> B {
    br(line, begin, end, -1, Bk::Test { reg, inv })
}

pub fn tset(tgt: i32, reg: i32, inv: bool, line: i32, begin: i32, end: i32) -> B {
    br(line, begin, end, tgt, Bk::TestSet { reg, inv })
}

pub fn cmp(op: u8, l: i32, r: i32, inv: bool, line: i32, begin: i32, end: i32) -> B {
    br(line, begin, end, -1, Bk::Cmp { op, l, r, inv })
}

pub fn tru(reg: i32, inv: bool, line: i32, begin: i32, end: i32) -> B {
    br(line, begin, end, reg, Bk::True { reg, inv })
}

pub fn assign(line: i32, begin: i32, end: i32) -> B {
    br(line, begin, end, -1, Bk::Assign(None))
}

fn join(and: bool, l: B, r: B) -> B {
    let (line, begin, end) = {
        let x = r.borrow();
        (x.line, x.begin, x.end)
    };
    br(line, begin, end, -1, if and { Bk::And(l, r) } else { Bk::Or(l, r) })
}

pub fn and(l: B, r: B) -> B {
    join(true, l, r)
}

pub fn or(l: B, r: B) -> B {
    join(false, l, r)
}

pub fn invert(b: &B) -> R<B> {
    let x = b.borrow();
    let (line, s, e) = (x.line, x.end, x.begin);
    Ok(match &x.kind {
        Bk::And(l, r) => or(invert(l)?, invert(r)?),
        Bk::Or(l, r) => and(invert(l)?, invert(r)?),
        Bk::Test { reg, inv } => test(*reg, !inv, line, s, e),
        Bk::TestSet { reg, inv } => tset(x.tgt, *reg, !inv, line, s, e),
        Bk::Cmp { op, l, r, inv } => cmp(*op, *l, *r, !inv, line, s, e),
        Bk::True { reg, inv } => tru(*reg, !inv, line, s, e),
        Bk::Assign(_) => return bad("cannot invert an assign node"),
    })
}

pub fn reg(b: &B) -> R<i32> {
    let x = b.borrow();
    Ok(match &x.kind {
        Bk::And(l, r) | Bk::Or(l, r) => {
            let (a, c) = (reg(l)?, reg(r)?);
            if a == c { a } else { -1 }
        }
        Bk::Test { reg, .. } | Bk::True { reg, .. } => *reg,
        Bk::TestSet { .. } => x.tgt,
        Bk::Cmp { .. } => -1,
        Bk::Assign(_) => return bad("assign node has no register"),
    })
}

pub fn expr(b: &B, r: &Regs) -> R<E> {
    let x = b.borrow();
    Ok(match &x.kind {
        Bk::And(l, q) => logic(true, expr(l, r)?, expr(q, r)?),
        Bk::Or(l, q) => logic(false, expr(l, r)?, expr(q, r)?),
        Bk::Test { reg, inv: true } => un("not ", r.get(*reg, x.line)?),
        Bk::Test { reg, .. } | Bk::TestSet { reg, .. } => r.get(*reg, x.line)?,
        Bk::Cmp { op: o::EQ, l, r: q, inv } => bin(if *inv { "~=" } else { "==" }, r.getk(*l, x.line)?, r.getk(*q, x.line)?),
        Bk::Cmp { op, l, r: q, inv } => {
            let (a, c) = (r.getk(*l, x.line)?, r.getk(*q, x.line)?);
            let swap = if !a.is_k() && !c.is_k() { r.upd(*l, x.line) > r.upd(*q, x.line) } else { c.kidx() < a.kidx() };
            let op = match (*op == o::LT, swap) {
                (true, false) => "<",
                (true, true) => ">",
                (false, false) => "<=",
                (false, true) => ">=",
            };
            let e = if swap { bin(op, c, a) } else { bin(op, a, c) };
            if *inv { un("not ", e) } else { e }
        }
        Bk::True { inv, .. } => k(Const::Bool(*inv)),
        Bk::Assign(Some(e)) => e.clone(),
        Bk::Assign(None) => return bad("assign node has no expression"),
    })
}

pub fn use_expr(b: &B, e: &E) {
    let kids = match &mut b.borrow_mut().kind {
        Bk::And(l, r) | Bk::Or(l, r) => Some((l.clone(), r.clone())),
        Bk::Assign(s) => {
            *s = Some(e.clone());
            None
        }
        _ => None,
    };
    if let Some((l, r)) = kids {
        use_expr(&l, e);
        use_expr(&r, e);
    }
}

pub enum Kind {
    Outer,
    Break { target: i32 },
    Loop,
    Do,
    If { br: B, stack: Option<Vec<B>> },
    IfElse { br: B, back: i32, empty: bool },
    ElseEnd,
    While { br: B, back: i32 },
    Repeat { br: B },
    For { reg: i32 },
    TFor { reg: i32, n: i32 },
    Set { tgt: i32, asg: Option<A>, br: B, empty: bool, fin: bool },
    Cmp { tgt: i32, br: B },
    BoolInd,
}

impl Kind {
    pub fn compound(&self) -> bool {
        matches!(self, Kind::If { .. } | Kind::IfElse { .. } | Kind::While { .. } | Kind::Loop | Kind::Repeat { .. } | Kind::For { .. } | Kind::TFor { .. } | Kind::Do)
    }
}

pub struct Block {
    pub begin: i32,
    pub end: i32,
    pub stmts: Vec<Stmt>,
    pub kind: Kind,
    pub r: Rg,
}

pub fn block(kind: Kind, begin: i32, end: i32, r: &Rg) -> Bl {
    Rc::new(RefCell::new(Block { begin, end, stmts: Vec::new(), kind, r: r.clone() }))
}

impl Block {
    pub fn contains(&self, x: &Block) -> bool {
        self.begin <= x.begin && self.end >= x.end
    }

    pub fn has(&self, line: i32) -> bool {
        self.begin <= line && line < self.end
    }

    pub fn scope_end(&self) -> i32 {
        match self.kind {
            Kind::Outer | Kind::Loop | Kind::IfElse { .. } | Kind::While { .. } | Kind::For { .. } => self.end - 2,
            Kind::TFor { .. } => self.end - 3,
            _ => self.end - 1,
        }
    }

    pub fn unprotected(&self) -> bool {
        matches!(self.kind, Kind::Loop | Kind::IfElse { .. } | Kind::While { .. })
    }

    pub fn back(&self) -> i32 {
        match &self.kind {
            Kind::IfElse { back, .. } | Kind::While { back, .. } => *back,
            _ => self.begin,
        }
    }

    pub fn breakable(&self) -> bool {
        matches!(self.kind, Kind::Loop | Kind::While { .. } | Kind::Repeat { .. } | Kind::For { .. } | Kind::TFor { .. })
    }

    pub fn container(&self) -> bool {
        !matches!(self.kind, Kind::Break { .. } | Kind::Set { .. } | Kind::Cmp { .. } | Kind::BoolInd)
    }

    pub fn add(&mut self, s: Stmt) -> R {
        match &mut self.kind {
            Kind::Break { .. } => return bad("statement added to a break"),
            Kind::Cmp { .. } | Kind::BoolInd => {}
            Kind::Set { asg, fin, .. } => match &s {
                Stmt::Assign(a) if !*fin => *asg = Some(a.clone()),
                Stmt::Block(b) if matches!(b.borrow().kind, Kind::BoolInd) => *fin = true,
                _ => {}
            },
            _ => self.stmts.push(s),
        }
        Ok(())
    }

    pub fn order(&self, x: &Block) -> Ordering {
        let lp = |b: &Block| b.breakable();
        self.begin.cmp(&x.begin).then(x.end.cmp(&self.end)).then(x.container().cmp(&self.container())).then(lp(self).cmp(&lp(x)))
    }
}

fn body(o: &mut Out, v: &[Stmt]) -> R {
    o.indent();
    print_all(o, v, false)?;
    o.dedent();
    Ok(())
}

fn head(o: &mut Out, a: &str, b: &B, r: &Regs, z: &str) -> R {
    o.p(a);
    expr(b, r)?.print(o)?;
    o.p(z);
    o.nl();
    Ok(())
}

pub fn print(b: &Bl, o: &mut Out) -> R {
    let x = b.borrow();
    let rc = x.r.clone();
    let r = rc.borrow();
    let s = &x.stmts;
    let is = |i: usize, f: fn(&Kind) -> bool| matches!(&s[i], Stmt::Block(y) if f(&y.borrow().kind));
    match &x.kind {
        Kind::Outer => match s.last() {
            Some(Stmt::Ret(_)) => return print_all(o, &s[..s.len() - 1], true),
            _ => return bad("outer block does not end with a return"),
        },
        Kind::Break { .. } => return Ok(o.p("do break end")),
        Kind::Loop => o.line("while true do"),
        Kind::Do => o.line("do"),
        Kind::If { br, .. } => head(o, "if ", br, &r, " then")?,
        Kind::IfElse { br, back, empty } => {
            head(o, "if ", br, &r, " then")?;
            o.indent();
            if let [Stmt::Block(y)] = &s[..] {
                if matches!(y.borrow().kind, Kind::Break { target } if target == *back) {
                    o.dedent();
                    return Ok(());
                }
            }
            print_all(o, s, false)?;
            o.dedent();
            if *empty {
                o.line("else");
                o.line("end");
            }
            return Ok(());
        }
        Kind::ElseEnd => {
            o.p("else");
            if s.len() == 1 && is(0, |k| matches!(k, Kind::If { .. })) {
                return s[0].print(o);
            }
            if s.len() == 2 && is(0, |k| matches!(k, Kind::IfElse { .. })) && is(1, |k| matches!(k, Kind::ElseEnd)) {
                s[0].print(o)?;
                return s[1].print(o);
            }
            o.nl();
        }
        Kind::While { br, .. } => head(o, "while ", br, &r, " do")?,
        Kind::Repeat { br } => {
            o.p("repeat");
            o.nl();
            body(o, s)?;
            o.p("until ");
            return expr(br, &r)?.print(o);
        }
        Kind::For { reg } => {
            let l = x.begin - 1;
            o.p("for ");
            r.target(reg + 3, l)?.print(o)?;
            o.eq();
            r.val(*reg, l)?.print(o)?;
            o.p(", ");
            r.val(reg + 1, l)?.print(o)?;
            let step = r.val(reg + 2, l)?;
            if step.int() != Some(1) {
                o.p(", ");
                step.print(o)?;
            }
            o.p(" do");
            o.nl();
        }
        Kind::TFor { reg, n } => {
            let l = x.begin - 1;
            o.p("for ");
            r.target(reg + 3, l)?.print(o)?;
            for q in reg + 4..=reg + 2 + n {
                o.p(", ");
                r.target(q, l)?.print(o)?;
            }
            o.p(" in ");
            for q in *reg..reg + 3 {
                if q > *reg {
                    o.p(", ");
                }
                let v = r.val(q, l)?;
                v.print(o)?;
                if v.multi() {
                    break;
                }
            }
            o.p(" do");
            o.nl();
        }
        Kind::Set { asg, br, .. } => {
            return match asg.as_ref().and_then(|a| a.borrow().ts.first().cloned()) {
                Some(t) => Assign::new(t, expr(br, &r)?).print(o),
                None => bad("unhandled set block"),
            };
        }
        Kind::Cmp { .. } => return bad("unhandled compare block"),
        Kind::BoolInd => return bad("unhandled boolean indicator"),
    }
    body(o, s)?;
    o.p("end");
    Ok(())
}
