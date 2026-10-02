use super::block::{self as bk, and, block, cmp, expr, invert, or, reg, test, tru, tset, use_expr, Bk, Bl, Block, Kind, Rg, B};
use super::expr::{bin, gname, k, kexp, logic, nil, un, Decl, Entry, Expr, Tab, D, E};
use super::out::Out;
use super::reg::Regs;
use super::stmt::{Assign, Stmt, Target, A};
use super::var::{infer, Var};
use super::{Opts, R};
use crate::comp::compile;
use crate::luac::{self, o, Const, Header, Proto};
use ac_core::bad;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub struct Func {
    pub globals: HashSet<Vec<u8>>,
    pub ups: Vec<String>,
    pub params: usize,
    pub vararg: u8,
    pub decls: Vec<D>,
    pub outer: Bl,
}

impl Func {
    pub fn print(&self, o: &mut Out) -> R {
        bk::print(&self.outer, o)
    }

    pub fn print_fn(&self, o: &mut Out, first: bool) -> R {
        let s = !first as usize;
        let mut v: Vec<&str> = self.decls.iter().take(self.params).skip(s).map(|d| d.name.as_str()).collect();
        if self.vararg & 1 == 1 {
            v.push("...");
        }
        o.p("(");
        o.p(&v.join(", "));
        o.p(")");
        o.nl();
        o.indent();
        self.print(o)?;
        o.dedent();
        o.p("end");
        Ok(())
    }
}

enum Act {
    Set { line: i32, reg: i32, val: E },
    Call(E),
    Global { name: String, val: E },
    Table { t: E, k: E, v: E, tbl: bool, ts: i32 },
    Up { name: String, val: E },
    Ret(Vec<E>),
    Block(Bl),
    Assign { t: Target, v: E },
    Asg(A),
    IfSet { reg: i32, line: i32, br: B },
    SetFinal(Bl),
    Cmp { line: i32, tgt: i32, br: B },
}

pub struct Dec<'p> {
    p: &'p Proto,
    code: Vec<u32>,
    ks: Rc<[Const]>,
    n: i32,
    len: i32,
    ups: Vec<String>,
    decls: Vec<D>,
    r: Rg,
    outer: Option<Bl>,
    blocks: Vec<Bl>,
    skip: Vec<bool>,
    rev: Vec<bool>,
    backup: Option<Vec<B>>,
    splits: HashSet<i32>,
    need: Option<i32>,
    kids: Vec<Option<Rc<Func>>>,
    fault: Cell<bool>,
    depth: usize,
    synth: Option<Vec<Var>>,
    cx: Rc<Ctx>,
    cands: Vec<(usize, i32)>,
}

pub struct Ctx {
    pub f32: bool,
    known: RefCell<HashMap<usize, Vec<(usize, i32)>>>,
    ver: Cell<u8>,
}

impl Ctx {
    pub fn new(f32: bool) -> Rc<Ctx> {
        Rc::new(Ctx { f32, known: RefCell::new(HashMap::new()), ver: Cell::new(5) })
    }
}

pub fn score(p: &Proto, f: &Func, depth: usize, cx: &Ctx) -> usize {
    let o = Opts::default();
    let mut w = Out::new(&o, cx.f32);
    if depth > 0 {
        if !f.ups.is_empty() {
            w.p(&format!("local {}", f.ups.join(", ")));
            w.nl();
        }
        w.p("return function");
        if f.print_fn(&mut w, true).is_err() {
            return 0;
        }
    } else if f.print(&mut w).is_err() {
        return 0;
    }
    let s = w.done();
    let v = cx.ver.get();
    let mut best = 0;
    for ver in [v, 9 - v] {
        let Some(c) = compile(s.as_bytes(), "=v", Header::default(), ver).ok().and_then(|b| luac::parse(&b).ok()) else { continue };
        let Some(q) = (if depth > 0 { c.main.protos.first() } else { Some(&c.main) }) else { continue };
        let k = if q.code == p.code && q.consts == p.consts && q.protos.len() == p.protos.len() {
            usize::MAX
        } else {
            (0..q.code.len().min(p.code.len())).find(|&i| q.code[i] != p.code[i]).unwrap_or(q.code.len().min(p.code.len()))
        };
        if k == usize::MAX {
            cx.ver.set(ver);
            return k;
        }
        best = best.max(k);
    }
    best
}

pub fn make(p: &Proto, depth: usize, given: &[String], cx: &Rc<Ctx>) -> R<Func> {
    let go = |fl: Vec<(usize, i32)>| Dec::with(p, depth, given.to_vec(), cx.clone(), fl).map(|d| (d.cands.clone(), d.run()));
    if !p.lines.is_empty() || !p.locals.is_empty() {
        return go(Vec::new())?.1;
    }
    let key = p as *const Proto as usize;
    if let Some(fl) = cx.known.borrow().get(&key).cloned() {
        return go(fl)?.1;
    }
    let (mut cands, mut f) = go(Vec::new())?;
    let mut best = f.as_ref().map_or(0, |f| score(p, f, depth, cx));
    let (mut flip, mut tries) = (Vec::new(), 0);
    while best != usize::MAX && tries < 48 {
        let mut cs: Vec<(usize, i32)> = cands.iter().copied().filter(|c| c.0 < usize::MAX - 2 && !flip.contains(c)).collect();
        let near = |r: i32| cs.iter().filter(|c| c.1 == r).map(|c| c.0.abs_diff(best)).min().unwrap_or(usize::MAX);
        let mut ns: Vec<(usize, i32)> = cands.iter().copied().filter(|c| c.0 == usize::MAX - 1 && !flip.contains(c)).collect();
        ns.sort_by_key(|c| near(c.1));
        cs.sort_by_key(|&(pc, _)| pc.abs_diff(best));
        let hs = cands.iter().copied().filter(|c| c.0 == usize::MAX && !flip.contains(c)).take(3).chain(cands.iter().copied().filter(|c| c.0 == usize::MAX - 2 && !flip.contains(c)).take(6));
        let mut up = false;
        let ns: Vec<(usize, i32)> = ns.into_iter().take(3).collect();
        for c in hs.chain(ns).chain(cs.into_iter().take(8)) {
            tries += 1;
            let mut fl = flip.clone();
            fl.push(c);
            let Ok((cn, g)) = go(fl.clone()) else { continue };
            let s = g.as_ref().map_or(0, |g| score(p, g, depth, cx));
            if s > best {
                (best, f, cands, flip, up) = (s, g, cn, fl, true);
                break;
            }
        }
        if !up {
            break;
        }
    }
    cx.known.borrow_mut().insert(key, flip);
    f
}

fn fb2int(x: i32) -> i32 {
    let e = (x >> 3) & 0x1f;
    if e == 0 { x } else { ((x & 7) + 8) << (e - 1) }
}

fn at(v: &[bool], i: i32) -> bool {
    i >= 0 && v.get(i as usize).copied().unwrap_or(false)
}

fn mark(v: &mut [bool], i: i32) {
    if let Some(x) = usize::try_from(i).ok().and_then(|i| v.get_mut(i)) {
        *x = true;
    }
}

fn remove(v: &mut Vec<Bl>, b: &Bl) {
    if let Some(i) = v.iter().position(|x| Rc::ptr_eq(x, b)) {
        v.remove(i);
    }
}

fn locals(p: &Proto, len: i32, synth: &Option<Vec<Var>>) -> Vec<D> {
    if let Some(v) = synth {
        v.iter().map(|x| Decl::at(&x.name, x.start, x.end, x.reg)).collect()
    } else if p.locals.len() >= p.params as usize {
        p.locals.iter().map(|l| Decl::new(&String::from_utf8_lossy(&l.name), l.start as i32, l.end as i32)).collect()
    } else {
        (0..p.params).map(|i| Decl::new(&format!("_ARG_{i}_"), 0, len - 1)).collect()
    }
}

fn names(p: &Proto, out: &mut HashSet<Vec<u8>>) {
    for &i in &p.code {
        if let (o::GETGLOBAL | o::SETGLOBAL, Some(Const::Str(s))) = (luac::op(i), p.consts.get(luac::bx(i) as usize)) {
            out.insert(s.clone());
        }
    }
    p.protos.iter().for_each(|c| names(c, out));
}

fn unthread(c: &[u32]) -> Vec<u32> {
    let mut v = c.to_vec();
    let t = |l: usize| l as i32 + 1 + luac::sbx(c[l]);
    for l in 0..c.len().saturating_sub(3) {
        let lb = |i: usize| luac::op(c[i]) == o::LOADBOOL;
        if luac::op(c[l]) == o::JMP && lb(l + 1) && luac::c(c[l + 1]) != 0 && lb(l + 2) && luac::c(c[l + 2]) == 0 && luac::op(c[l + 3]) == o::JMP && t(l) == t(l + 3) && t(l) != l as i32 + 3 {
            v[l] = luac::asbx(o::JMP, luac::a(c[l]), 2);
        }
    }
    let n = v.len();
    let tv = |v: &[u32], l: usize| l as i32 + 1 + luac::sbx(v[l]);
    for l in 0..n.saturating_sub(1) {
        let (s, x) = (tv(&v, l), l + 1);
        let back = |i: usize| matches!(luac::op(v[i]), o::JMP | o::FORLOOP);
        if !back(l) || s < 0 || s as usize >= l || luac::op(v[x]) != o::JMP || tv(&v, x) == x as i32 || (x..n).any(|i| back(i) && tv(&v, i) == s) {
            continue;
        }
        let to = tv(&v, x);
        for j in s as usize..l {
            if luac::op(v[j]) == o::JMP && tv(&v, j) == to {
                v[j] = luac::asbx(o::JMP, luac::a(v[j]), x as i32 - j as i32 - 1);
            }
        }
    }
    v
}

fn check(p: &Proto) -> R {
    let len = p.code.len() as i32;
    let op = |l: i32| luac::op(p.code[l as usize - 1]);
    if len == 0 || op(len) != o::RETURN {
        return bad("function does not end with a return");
    }
    for l in 1..=len {
        let i = p.code[l as usize - 1];
        let ok = match op(l) {
            o::JMP | o::FORLOOP | o::FORPREP => (1..=len).contains(&(l + 1 + luac::sbx(i))),
            o::EQ | o::LT | o::LE | o::TEST | o::TESTSET => l < len && op(l + 1) == o::JMP,
            o::CLOSURE => p.protos.get(luac::bx(i) as usize).is_some_and(|f| l + f.nups as i32 <= len),
            o::SETLIST => luac::c(i) != 0 || l < len,
            x => x <= o::VARARG,
        };
        if !ok {
            return bad("malformed bytecode");
        }
    }
    Ok(())
}

impl<'p> Dec<'p> {
    fn with(p: &'p Proto, depth: usize, given: Vec<String>, cx: Rc<Ctx>, flip: Vec<(usize, i32)>) -> R<Dec<'p>> {
        let mut cands = Vec::new();
        check(p)?;
        let synth = (p.lines.is_empty() && p.locals.is_empty()).then(|| {
            let mut g = HashSet::new();
            names(p, &mut g);
            let (mut v, c) = infer(p, depth, &flip);
            cands = c;
            for x in v.iter_mut() {
                while g.contains(x.name.as_bytes()) {
                    x.name.push('_');
                }
            }
            v
        });
        let len = p.code.len() as i32;
        let ks: Rc<[Const]> = p.consts.clone().into();
        let decls = locals(p, len, &synth);
        let n = p.stack as i32;
        let ups = (0..p.ups.len().max(p.nups as usize))
            .map(|i| match (p.ups.get(i), given.get(i)) {
                (Some(s), _) if !s.is_empty() => String::from_utf8_lossy(s).into_owned(),
                (_, Some(s)) if !s.is_empty() => s.clone(),
                _ => format!("_UPVALUE{i}_"),
            })
            .collect();
        let r = Rc::new(RefCell::new(Regs::new(n, len, &decls, ks.clone())?));
        Ok(Dec {
            p,
            code: unthread(&p.code),
            ks,
            n,
            len,
            ups,
            decls,
            r,
            outer: None,
            blocks: Vec::new(),
            skip: Vec::new(),
            rev: Vec::new(),
            backup: None,
            splits: HashSet::new(),
            need: None,
            kids: vec![None; p.protos.len()],
            fault: Cell::new(false),
            depth,
            synth,
            cx,
            cands,
        })
    }

    pub fn run(mut self) -> R<Func> {
        for _ in 0..16 {
            self.need = None;
            self.once()?;
            match self.need.take() {
                Some(l) => {
                    self.splits.insert(l);
                }
                None => break,
            }
        }
        if self.fault.get() {
            return bad("instruction index out of range");
        }
        let Some(outer) = self.outer else { return bad("no outer block") };
        let mut globals = HashSet::new();
        names(self.p, &mut globals);
        Ok(Func { globals, ups: self.ups, params: self.p.params as usize, vararg: self.p.vararg, decls: self.decls, outer })
    }

    fn once(&mut self) -> R {
        self.decls = locals(self.p, self.len, &self.synth);
        self.r = Rc::new(RefCell::new(Regs::new(self.n, self.len, &self.decls, self.ks.clone())?));
        self.rev = vec![false; self.len as usize + 2];
        for l in 1..=self.len {
            let s = self.sbx(l);
            if self.op(l) == o::JMP && s < 0 {
                mark(&mut self.rev, l + 1 + s);
            }
        }
        self.branches(true)?;
        self.outer = Some(self.branches(false)?);
        self.seq(1, self.len)
    }

    fn w(&self, l: i32) -> u32 {
        match usize::try_from(l - 1).ok().and_then(|i| self.code.get(i)) {
            Some(&w) => w,
            None => {
                self.fault.set(true);
                0
            }
        }
    }

    fn op(&self, l: i32) -> u8 {
        luac::op(self.w(l))
    }

    fn a(&self, l: i32) -> i32 {
        luac::a(self.w(l)) as i32
    }

    fn b(&self, l: i32) -> i32 {
        luac::b(self.w(l)) as i32
    }

    fn c(&self, l: i32) -> i32 {
        luac::c(self.w(l)) as i32
    }

    fn bx(&self, l: i32) -> i32 {
        luac::bx(self.w(l)) as i32
    }

    fn sbx(&self, l: i32) -> i32 {
        luac::sbx(self.w(l))
    }

    fn up(&self, i: i32) -> String {
        self.ups.get(i as usize).cloned().unwrap_or_else(|| format!("_UPVALUE{i}_"))
    }

    fn kid(&mut self, i: i32, line: i32) -> R<Rc<Func>> {
        let i = i as usize;
        if let Some(Some(f)) = self.kids.get(i) {
            return Ok(f.clone());
        }
        let Some(p) = self.p.protos.get(i) else { return bad("closure index out of range") };
        let k = p.nups as i32;
        let given: Vec<String> = (1..=k)
            .map(|q| {
                let l = line + q;
                match self.op(l) {
                    o::MOVE => {
                        let r = self.r.borrow();
                        let b = self.b(l);
                        r.decl(b, line).or_else(|| (line..=line + k + 1).find_map(|x| r.decl(b, x))).map_or_else(String::new, |d| d.name.clone())
                    }
                    o::GETUPVAL => self.up(self.b(l)),
                    _ => String::new(),
                }
            })
            .collect();
        let f = Rc::new(make(p, self.depth + 1, &given, &self.cx)?);
        self.kids[i] = Some(f.clone());
        Ok(f)
    }

    fn ops(&mut self, line: i32) -> R<Vec<Act>> {
        let mut v = Vec::new();
        let rc = self.r.clone();
        let r = rc.borrow();
        let (a, mut b, mut c, bx) = (self.a(line), self.b(line), self.c(line), self.bx(line));
        let set = |reg: i32, val: E| Act::Set { line, reg, val };
        let g = |x: i32| r.get(x, line);
        let gk = |x: i32| r.getk(x, line);
        match self.op(line) {
            o::MOVE => v.push(set(a, g(b)?)),
            o::LOADK => v.push(set(a, kexp(&self.ks, bx)?)),
            o::LOADBOOL => v.push(set(a, k(Const::Bool(b != 0)))),
            o::LOADNIL => v.extend((a..=b).map(|x| set(x, nil()))),
            o::GETUPVAL => v.push(set(a, Rc::new(Expr::Up(self.up(b))))),
            o::GETGLOBAL => v.push(set(a, Rc::new(Expr::Global { name: gname(&self.ks, bx)?, i: bx }))),
            o::GETTABLE => v.push(set(a, Rc::new(Expr::Index { t: g(b)?, k: gk(c)? }))),
            o::SETUPVAL => v.push(Act::Up { name: self.up(b), val: g(a)? }),
            o::SETGLOBAL => v.push(Act::Global { name: gname(&self.ks, bx)?, val: g(a)? }),
            o::SETTABLE => v.push(Act::Table { t: g(a)?, k: gk(b)?, v: gk(c)?, tbl: true, ts: line }),
            o::NEWTABLE => v.push(set(a, Rc::new(Expr::Table(RefCell::new(Tab { ents: Vec::new(), obj: true, list: true, cap: fb2int(b) + fb2int(c) }))))),
            o::SELF => {
                let t = g(b)?;
                v.push(set(a + 1, t.clone()));
                v.push(set(a, Rc::new(Expr::Index { t, k: gk(c)? })));
            }
            x @ o::ADD..=o::POW => v.push(set(a, bin(["+", "-", "*", "/", "%", "^"][(x - o::ADD) as usize], gk(b)?, gk(c)?))),
            o::UNM => v.push(set(a, un("-", g(b)?))),
            o::NOT => v.push(set(a, un("not ", g(b)?))),
            o::LEN => v.push(set(a, un("#", g(b)?))),
            o::CONCAT => {
                let mut e = g(c)?;
                for x in (b..c).rev() {
                    e = bin("..", g(x)?, e);
                }
                v.push(set(a, e));
            }
            o::CALL | o::TAILCALL => {
                let tail = self.op(line) == o::TAILCALL;
                let multi = tail || c >= 3 || c == 0;
                if b == 0 {
                    b = self.n - a;
                }
                let f = g(a)?;
                let args = (a + 1..a + b).map(g).collect::<R<Vec<E>>>()?;
                let e = Rc::new(Expr::Call { f, args, multi });
                if tail {
                    v.push(Act::Ret(vec![e]));
                    mark(&mut self.skip, line + 1);
                } else {
                    if c == 0 {
                        c = self.n - a + 1;
                    }
                    match c {
                        1 => v.push(Act::Call(e)),
                        2 if !multi => v.push(set(a, e)),
                        _ => v.extend((a..a + c - 1).map(|x| set(x, e.clone()))),
                    }
                }
            }
            o::RETURN => {
                if b == 0 {
                    b = self.n - a + 1;
                }
                v.push(Act::Ret((a..a + b - 1).map(g).collect::<R<Vec<E>>>()?));
            }
            o::SETLIST => {
                if c == 0 {
                    c = self.w(line + 1) as i32;
                    mark(&mut self.skip, line + 1);
                }
                if b == 0 {
                    b = self.n - a - 1;
                }
                let t = r.val(a, line)?;
                for i in 1..=b {
                    let key = k(Const::Num((c as f64 - 1.0) * 50.0 + i as f64));
                    v.push(Act::Table { t: t.clone(), k: key, v: g(a + i)?, tbl: false, ts: r.upd(a + i, line) });
                }
            }
            o::CLOSURE => {
                let f = self.kid(bx, line)?;
                for i in 0..self.p.protos[bx as usize].nups as i32 {
                    mark(&mut self.skip, line + 1 + i);
                }
                let own = (1..=self.p.protos[bx as usize].nups as i32).any(|i| self.op(line + i) == o::MOVE && self.b(line + i) == a);
                v.push(set(a, Rc::new(Expr::Closure { f, own })));
            }
            o::VARARG => {
                if b == 1 {
                    return Ok(v);
                }
                let e = Rc::new(Expr::Vararg(b != 2));
                if b == 0 {
                    b = self.n - a + 1;
                }
                v.extend((a..a + b - 1).map(|x| set(x, e.clone())));
            }
            _ => {}
        }
        Ok(v)
    }

    fn act(&mut self, x: &Act, line: i32, next: i32, blk: &Bl) -> R<Option<A>> {
        let a = match self.stmt(x)? {
            None => return Ok(None),
            Some(Stmt::Assign(a)) => a,
            Some(s) => {
                blk.borrow_mut().add(s)?;
                return Ok(None);
            }
        };
        let multi = a.borrow().first().1.multi();
        if !multi {
            blk.borrow_mut().add(Stmt::Assign(a.clone()))?;
        }
        let mut nx = next;
        if self.op(line) == o::CLOSURE && next == line + 1 {
            nx += self.p.protos.get(self.bx(line) as usize).map_or(0, |p| p.nups as i32);
        }
        while nx < blk.borrow().end && self.is_move(nx) {
            let (t, v) = (self.move_target(nx, line + 1)?, self.move_val(nx, line + 1)?);
            a.borrow_mut().push_front(t, v);
            mark(&mut self.skip, nx);
            nx += 1;
        }
        if multi && !a.borrow().first().1.multi() {
            blk.borrow_mut().add(Stmt::Assign(a.clone()))?;
        }
        let hi = match self.op(line) {
            o::MOVE => self.b(line),
            o::SETGLOBAL | o::SETUPVAL => self.a(line),
            o::SETTABLE => self.c(line),
            _ => -1,
        };
        if next == line + 1 && (0..256).contains(&hi) && !self.r.borrow().local(hi, line) {
            let vs = a.borrow().vs.clone();
            let ex = self.excess(hi, line - 1, line, &vs, blk)?;
            let mut a = a.borrow_mut();
            if ex.is_empty() {
                a.trim();
            }
            ex.into_iter().for_each(|v| a.extra(v));
        }
        Ok(Some(a))
    }

    fn excess(&mut self, hi: i32, tl: i32, line: i32, vs: &[E], blk: &Bl) -> R<Vec<E>> {
        let mut out: Vec<E> = Vec::new();
        let rc = self.r.clone();
        let mut r = hi + 1;
        while r < self.n {
            if tl >= 1 && self.a(tl) == r && self.op(tl) == o::CALL && self.c(tl) == 1 {
                let mut b = blk.borrow_mut();
                let n = b.stmts.len();
                if let Some(i) = (n.saturating_sub(2)..n).rev().find(|&i| matches!(b.stmts[i], Stmt::Call(_))) {
                    if let Stmt::Call(e) = b.stmts.remove(i) {
                        if let Expr::Call { f, args, .. } = &*e {
                            out.push(Rc::new(Expr::Call { f: f.clone(), args: args.clone(), multi: true }));
                        }
                    }
                }
                break;
            }
            if tl >= 1 && self.a(tl) == r && self.op(tl) == o::VARARG && self.b(tl) == 1 {
                out.push(Rc::new(Expr::Vararg(true)));
                break;
            }
            let g = rc.borrow();
            if g.local(r, line) || g.upd(r, line) <= g.upd(r - 1, line) {
                break;
            }
            let v = g.val(r, line + 1)?;
            if vs.iter().chain(&out).any(|x| x.refs(&v)) {
                break;
            }
            out.push(v);
            r += 1;
        }
        Ok(out)
    }

    fn stmt(&mut self, x: &Act) -> R<Option<Stmt>> {
        let rc = self.r.clone();
        let asg = |t: Target, v: &E| -> R<Option<Stmt>> { Ok(Some(Stmt::Assign(Assign::rc(t, v.clone())))) };
        match x {
            Act::Set { line, reg, val } => {
                rc.borrow_mut().set(*reg, *line, val.clone())?;
                let r = rc.borrow();
                if r.assignable(*reg, *line) { asg(r.target(*reg, *line)?, val) } else { Ok(None) }
            }
            Act::Call(c) => Ok(Some(Stmt::Call(c.clone()))),
            Act::Global { name, val } => asg(Target::Global(name.clone()), val),
            Act::Up { name, val } => asg(Target::Up(name.clone()), val),
            Act::Ret(v) => Ok(Some(Stmt::Ret(v.clone()))),
            Act::Table { t, k, v, tbl, ts } => {
                if let (Expr::Table(x), false) = (&**t, k.refs(t) || v.refs(t)) {
                    let mut x = x.borrow_mut();
                    if v.multi() || (x.ents.len() as i32) < x.cap {
                        let list = !tbl;
                        x.obj = x.obj && (list || k.is_ident());
                        x.list = x.list && list;
                        x.ents.push(Entry { key: k.clone(), val: v.clone(), list, ts: *ts });
                        return Ok(None);
                    }
                }
                asg(Target::Index { t: t.clone(), k: k.clone() }, v)
            }
            Act::Block(b) => Ok(Some(Stmt::Block(b.clone()))),
            Act::Assign { t, v } => asg(t.clone(), v),
            Act::Asg(a) => Ok(Some(Stmt::Assign(a.clone()))),
            Act::IfSet { reg, line, br } => {
                let v = expr(br, &rc.borrow())?;
                rc.borrow_mut().set(*reg, *line, v)?;
                Ok(None)
            }
            Act::Cmp { line, tgt, br } => {
                let val = expr(br, &rc.borrow())?;
                self.stmt(&Act::Set { line: *line, reg: *tgt, val })
            }
            Act::SetFinal(b) => self.set_final(b),
        }
    }

    fn set_final(&mut self, b: &Bl) -> R<Option<Stmt>> {
        let rc = self.r.clone();
        let (br, tgt) = match &b.borrow().kind {
            Kind::Set { br, tgt, .. } => (br.clone(), *tgt),
            _ => return bad("set block expected"),
        };
        let (be, bb) = (br.borrow().end, br.borrow().begin);
        let mut e = None;
        {
            let r = rc.borrow();
            if let Some(x) = (0..r.n).find(|&x| r.upd(x, be - 1) == be - 1) {
                e = Some((x, r.val(x, be)?));
            }
        }
        let start = b.borrow().begin;
        let inner = self.blocks.iter().any(|x| !Rc::ptr_eq(x, b) && x.borrow().begin > start && x.borrow().end == be && matches!(x.borrow().kind, Kind::Set { .. } | Kind::Cmp { .. }));
        let (t, e) = if !inner && self.op(be - 2) == o::LOADBOOL && self.c(be - 2) != 0 {
            let t = self.a(be - 2);
            let l = if self.op(be - 3) == o::JMP && self.sbx(be - 3) == 2 { be - 2 } else { bb };
            let v = rc.borrow().val(t, l)?;
            (t, v)
        } else {
            match e {
                Some((_, e)) if tgt >= 0 => (tgt, e),
                Some(x) if inner => x,
                _ => return Ok(None),
            }
        };
        use_expr(&br, &e);
        let v = expr(&br, &rc.borrow())?;
        if rc.borrow().local(t, be - 1) {
            let tg = rc.borrow().target(t, be - 1)?;
            return Ok(Some(Stmt::Assign(Assign::rc(tg, v))));
        }
        rc.borrow_mut().set(t, be - 1, v)?;
        Ok(None)
    }

    fn is_move(&self, l: i32) -> bool {
        let r = self.r.borrow();
        let a = self.a(l);
        match self.op(l) {
            o::MOVE => r.assignable(a, l) && !r.local(self.b(l), l),
            o::SETUPVAL | o::SETGLOBAL => !r.local(a, l),
            o::SETTABLE => self.c(l) < 256 && !r.local(self.c(l), l),
            _ => false,
        }
    }

    fn move_target(&self, l: i32, prev: i32) -> R<Target> {
        let r = self.r.borrow();
        Ok(match self.op(l) {
            o::MOVE => r.target(self.a(l), l)?,
            o::SETUPVAL => Target::Up(self.up(self.b(l))),
            o::SETGLOBAL => Target::Global(gname(&self.ks, self.bx(l))?),
            _ => Target::Index { t: r.get(self.a(l), prev)?, k: r.getk(self.b(l), prev)? },
        })
    }

    fn move_val(&self, l: i32, prev: i32) -> R<E> {
        let r = self.r.borrow();
        match self.op(l) {
            o::MOVE => r.val(self.b(l), prev),
            o::SETTABLE => r.get(self.c(l), prev),
            _ => r.get(self.a(l), prev),
        }
    }

    fn init(&self, st: &[Bl], zero: &[Bl]) -> R {
        let first = self.p.params as usize + (self.p.vararg & 1) as usize;
        let all: Vec<Bl> = st.iter().chain(zero).cloned().collect();
        let mut groups: Vec<Vec<D>> = vec![Vec::new(); all.len()];
        for d in self.decls.iter().skip(first).filter(|d| d.begin == 0) {
            let i = all.iter().rposition(|b| matches!(b.borrow().kind, Kind::Do) && b.borrow().begin == 0 && b.borrow().end == d.end + 1).unwrap_or(0);
            groups[i].push(d.clone());
        }
        let mut out: Vec<(i32, usize, Stmt)> = Vec::new();
        for (i, g) in groups.into_iter().enumerate() {
            if g.is_empty() {
                continue;
            }
            let r = g.iter().map(|d| d.reg.get()).min().unwrap_or(0);
            let mut d = self.declare(&g, 1)?;
            d.head = i < st.len();
            let a = Stmt::Assign(Rc::new(RefCell::new(d)));
            if i < st.len() {
                out.push((r, i, a));
            } else {
                all[i].borrow_mut().add(a)?;
                out.push((r, st.len() - 1, Stmt::Block(all[i].clone())));
            }
        }
        out.sort_by_key(|x| x.0);
        for (_, i, s) in out {
            st[i].borrow_mut().add(s)?;
        }
        Ok(())
    }

    fn declare(&self, nl: &[D], l: i32) -> R<Assign> {
        let mut a = Assign::empty();
        a.declare();
        for d in nl {
            a.push(Target::Var(d.clone()), self.r.borrow().val(d.reg.get(), l)?);
        }
        Ok(a)
    }

    fn seq(&mut self, begin: i32, end: i32) -> R {
        let mut bi = 1;
        let mut st = vec![self.blocks[0].clone()];
        let mut zero: Vec<Bl> = Vec::new();
        self.skip = vec![false; end as usize + 2];
        let rc = self.r.clone();
        let mut line = begin;
        while line <= end {
            let top = |st: &Vec<Bl>| st.last().cloned().ok_or(ac_core::Error::Bad("block stack underflow"));
            let mut h = None;
            if top(&st)?.borrow().end <= line {
                let b = top(&st)?;
                st.pop();
                h = Some(self.block_act(&b)?);
            }
            if h.is_none() {
                while bi < self.blocks.len() && self.blocks[bi].borrow().begin <= line {
                    let b = self.blocks[bi].clone();
                    if line == begin && b.borrow().end <= line && matches!(b.borrow().kind, Kind::Do) {
                        zero.push(b);
                    } else {
                        st.push(b);
                    }
                    bi += 1;
                }
            }
            let blk = top(&st)?;
            rc.borrow_mut().start(line);
            if line == begin && h.is_none() {
                self.init(&st, &zero)?;
            }
            if h.is_none() && at(&self.skip, line) {
                let nl = rc.borrow().new_locals(line);
                if !nl.is_empty() {
                    let mut a = self.declare(&nl, line)?;
                    let hi = nl.iter().map(|d| d.reg.get()).max().unwrap_or(-1);
                    let vs = a.vs.clone();
                    for v in self.excess(hi, 0, line, &vs, &blk)? {
                        a.extra(v);
                    }
                    blk.borrow_mut().add(Stmt::Assign(Rc::new(RefCell::new(a))))?;
                }
                line += 1;
                continue;
            }
            let ops = if h.is_none() { self.ops(line)? } else { Vec::new() };
            let nl = rc.borrow().new_locals(if h.is_none() { line } else { line - 1 });
            let mut asg: Option<A> = None;
            if let Some(x) = &h {
                asg = self.act(x, line, line, &blk)?;
            } else if self.op(line) == o::LOADNIL {
                let a = Rc::new(RefCell::new(Assign::empty()));
                let mut ts = Vec::new();
                for x in &ops {
                    let Act::Set { reg, line: sl, val } = x else { return bad("unexpected loadnil operation") };
                    self.stmt(x)?;
                    if rc.borrow().assignable(*reg, *sl) {
                        ts.push((rc.borrow().target(*reg, *sl)?, val.clone()));
                    }
                }
                let home = |d: &D| st.iter().rposition(|b| b.borrow().scope_end() >= d.end).unwrap_or(0);
                if nl.iter().any(|d| home(d) != home(&nl[0])) {
                    let mut idx: Vec<usize> = nl.iter().map(home).collect();
                    idx.sort();
                    idx.dedup();
                    for i in idx {
                        let g: Vec<D> = nl.iter().filter(|d| home(d) == i).cloned().collect();
                        let a = self.declare(&g, line + 1)?;
                        st[i].borrow_mut().add(Stmt::Assign(Rc::new(RefCell::new(a))))?;
                    }
                    line += 1;
                    continue;
                }
                if ts.len() > 1 && nl.is_empty() {
                    for (t, v) in ts {
                        blk.borrow_mut().add(Stmt::Assign(Assign::rc(t, v)))?;
                    }
                    line += 1;
                    continue;
                }
                let cnt = ts.len();
                ts.into_iter().for_each(|(t, v)| a.borrow_mut().push(t, v));
                if cnt > 0 {
                    blk.borrow_mut().add(Stmt::Assign(a.clone()))?;
                    let mut nx = line + 1;
                    while nl.is_empty() && nx < blk.borrow().end && self.is_move(nx) {
                        let (t, v) = (self.move_target(nx, line + 1)?, self.move_val(nx, line + 1)?);
                        a.borrow_mut().push_front(t, v);
                        mark(&mut self.skip, nx);
                        nx += 1;
                    }
                }
                asg = Some(a);
            } else {
                for x in &ops {
                    if let Some(t) = self.act(x, line, line + 1, &blk)? {
                        asg = Some(t);
                    }
                }
                if let Some(a) = &asg {
                    if a.borrow().first().1.multi() {
                        blk.borrow_mut().add(Stmt::Assign(a.clone()))?;
                    }
                }
            }
            if let (Some(a), false) = (&asg, nl.is_empty()) {
                a.borrow_mut().declare();
                for d in &nl {
                    let v = rc.borrow().val(d.reg.get(), line + 1)?;
                    a.borrow_mut().push(Target::Var(d.clone()), v);
                }
            }
            if h.is_none() && asg.is_none() && !nl.is_empty() && self.op(line) != o::FORPREP && !(self.op(line) == o::JMP && self.op(line + 1 + self.sbx(line)) == o::TFORLOOP) {
                let mut a = self.declare(&nl, line)?;
                let hi = nl.iter().map(|d| d.reg.get()).max().unwrap_or(-1);
                let vs = a.vs.clone();
                for v in self.excess(hi, line, line, &vs, &blk)? {
                    a.extra(v);
                }
                blk.borrow_mut().add(Stmt::Assign(Rc::new(RefCell::new(a))))?;
            }
            if h.is_none() {
                line += 1;
            }
        }
        Ok(())
    }

    fn block_act(&mut self, b: &Bl) -> R<Act> {
        let rc = self.r.clone();
        let end = b.borrow().end;
        let kind = match &b.borrow().kind {
            Kind::If { br, stack } => Some((br.clone(), stack.clone())),
            Kind::Cmp { tgt, br } => return Ok(Act::Cmp { line: end - 1, tgt: *tgt, br: br.clone() }),
            Kind::Set { empty: true, br, .. } => {
                let tg = br.borrow().tgt;
                let e = rc.borrow().get(tg, end)?;
                use_expr(br, &e);
                return Ok(Act::Set { line: end - 1, reg: tg, val: expr(br, &rc.borrow())? });
            }
            Kind::Set { asg: Some(a), br, .. } => {
                let r = self.assigned(end - 1);
                let i = a.borrow().ts.iter().position(|t| matches!(t, Target::Var(d) if d.reg.get() == r)).filter(|&i| i < a.borrow().vs.len()).unwrap_or(0);
                let v0 = a.borrow().vs[i].clone();
                use_expr(br, &v0);
                let v = expr(br, &rc.borrow())?;
                if a.borrow().ts.len() == 1 {
                    return Ok(Act::Assign { t: a.borrow().ts[0].clone(), v });
                }
                a.borrow_mut().vs[i] = v;
                return Ok(Act::Asg(a.clone()));
            }
            Kind::Set { .. } => return Ok(Act::SetFinal(b.clone())),
            _ => None,
        };
        let Some((br, stack)) = kind else { return Ok(Act::Block(b.clone())) };
        let n = b.borrow().stmts.len();
        if n == 1 {
            let single = match &b.borrow().stmts[0] {
                Stmt::Assign(a) if a.borrow().ts.len() == 1 => Some(a.clone()),
                _ => None,
            };
            let x = br.borrow();
            if let (Some(a), Bk::Test { reg, inv }) = (single, &x.kind) {
                if let Some(d) = rc.borrow().decl(*reg, x.line) {
                    let a = a.borrow();
                    let (t, v) = a.first();
                    if t.is(&d) {
                        return Ok(Act::Assign { t: t.clone(), v: logic(!inv, Rc::new(Expr::Local(d)), v.clone()) });
                    }
                }
            }
        } else if let (0, Some(mut stack)) = (n, stack) {
            let (bb, be) = (br.borrow().begin, br.borrow().end);
            let mut t = reg(&br)?;
            if t < 0 && be > 1 {
                let x = self.assigned(be - 1);
                if x >= 0 && rc.borrow().upd(x, be - 1) == be - 1 {
                    t = x;
                }
            }
            if t < 0 {
                let r = rc.borrow();
                for x in 0..r.n {
                    if r.upd(x, be - 1) >= bb {
                        if t >= 0 {
                            t = -1;
                            break;
                        }
                        t = x;
                    }
                }
            }
            if t >= 0 && rc.borrow().upd(t, be - 1) >= bb {
                let right = rc.borrow().val(t, be)?;
                let Some(ae) = stack.last().map(|x| x.borrow().end) else { return bad("empty branch stack") };
                let s = self.pop_set(&mut stack, ae, t)?;
                use_expr(&s, &right);
                if let Kind::If { stack: slot, .. } = &mut b.borrow_mut().kind {
                    *slot = Some(stack);
                }
                return Ok(Act::IfSet { reg: t, line: be - 1, br: s });
            }
        }
        Ok(Act::Block(b.clone()))
    }

    fn break_target(&self, l: i32) -> i32 {
        self.blocks.iter().map(|b| b.borrow()).filter(|b| b.breakable() && b.has(l)).map(|b| b.end).min().unwrap_or(-1)
    }

    fn enc(&self, l: i32, f: fn(&Block) -> bool) -> Bl {
        let mut e = self.blocks[0].clone();
        for x in &self.blocks[1..] {
            let n = x.borrow();
            if f(&n) && e.borrow().contains(&n) && n.has(l) {
                drop(n);
                e = x.clone();
            }
        }
        e
    }

    fn enc_inner(&self, l: i32, f: fn(&Block) -> bool) -> Option<Bl> {
        Some(self.enc(l, f)).filter(|e| !Rc::ptr_eq(e, &self.blocks[0]))
    }

    fn breakable(&self, l: i32) -> Option<Bl> {
        self.enc_inner(l, Block::breakable)
    }

    fn pop(&mut self, st: &mut Vec<B>) -> R<B> {
        let Some(mut x) = st.pop() else { return bad("empty branch stack") };
        if let Some(k) = &mut self.backup {
            k.push(x.clone());
        }
        if matches!(x.borrow().kind, Bk::TestSet { .. }) {
            return bad("unexpected test-set branch");
        }
        let b0 = x.borrow().begin;
        let mut begin = b0;
        if self.op(begin) == o::JMP {
            begin += 1 + self.sbx(begin);
        }
        while let Some(nx) = st.last().cloned() {
            let (nl, ne, ts) = {
                let n = nx.borrow();
                (n.line, n.end, matches!(n.kind, Bk::TestSet { .. }))
            };
            let xl = x.borrow().line;
            if ts {
                break;
            }
            if ne == begin || ne == b0 {
                if self.separated(nl, xl) {
                    break;
                }
                let p = self.pop(st)?;
                x = or(invert(&p)?, x);
            } else if ne == x.borrow().end {
                if self.splits.contains(&nl) || self.separated(nl, xl) {
                    break;
                }
                let p = self.pop(st)?;
                x = and(p, x);
            } else {
                break;
            }
        }
        Ok(x)
    }

    fn pop_set(&mut self, st: &mut Vec<B>, ae: i32, tgt: i32) -> R<B> {
        st.push(bk::assign(ae - 1, ae, ae));
        self.pop_set_at(st, false, ae, tgt)
    }

    fn pop_cmp_set(&mut self, st: &mut Vec<B>, ae: i32, tgt: i32) -> R<B> {
        let Some(top) = st.last() else { return bad("empty branch stack") };
        let inv = self.b(top.borrow().begin) == 0;
        {
            let mut t = top.borrow_mut();
            t.begin = ae;
            t.end = ae;
        }
        self.pop_set_at(st, inv, ae, tgt)
    }

    fn adjust(&self, l: i32, tgt: i32) -> i32 {
        let mut t = l;
        while t >= 1 && self.op(t) == o::LOADBOOL && (tgt == -1 || self.a(t) == tgt) {
            t -= 1;
        }
        if t == l {
            return t;
        }
        t + if self.c(t + 1) != 0 { 3 } else { 2 }
    }

    fn pop_set_at(&mut self, st: &mut Vec<B>, inv: bool, ae: i32, tgt: i32) -> R<B> {
        let Some(mut x) = st.pop() else { return bad("empty branch stack") };
        let (begin, end) = (x.borrow().begin, x.borrow().end);
        let vr = match x.borrow().kind {
            Bk::Test { reg, .. } if tgt < 0 => reg,
            _ => tgt,
        };
        if inv {
            x = invert(&x)?;
        }
        let (begin, end) = (self.adjust(begin, tgt), self.adjust(end, tgt));
        let bt = x.borrow().tgt;
        while let Some(nx) = st.last().cloned() {
            let mut ne = nx.borrow().end;
            let mut ninv;
            let mut pure = false;
            if self.op(ne) == o::LOADBOOL && (tgt == -1 || self.a(ne) == tgt) {
                ninv = self.b(ne) != 0;
                ne = self.adjust(ne, tgt);
            } else {
                pure = match nx.borrow().kind {
                    Bk::Test { reg, .. } => reg != vr || ne < ae,
                    Bk::Cmp { .. } => true,
                    _ => false,
                };
                match nx.borrow().kind {
                    Bk::Test { reg, .. } if tgt >= 0 && reg != tgt && ne >= ae => break,
                    Bk::Test { inv, .. } | Bk::TestSet { inv, .. } => ninv = inv,
                    _ if ne >= ae => break,
                    _ => ninv = false,
                }
            }
            let addr = |n: bool| if n == inv { end } else { begin };
            if pure && ne != addr(ninv) && ne == addr(!ninv) {
                ninv = !ninv;
            }
            if ne == if ninv == inv { end } else { begin } {
                let y = self.pop_set_at(st, ninv, ae, tgt)?;
                x = if ninv { or(y, x) } else { and(y, x) };
                x.borrow_mut().end = ne;
            } else {
                let val = vr >= 0 && nx.borrow().end >= ae && matches!(nx.borrow().kind, Bk::Test { reg, .. } if reg == vr);
                if !val && !matches!(x.borrow().kind, Bk::TestSet { .. }) {
                    st.push(x);
                    x = self.pop(st)?;
                }
                break;
            }
        }
        {
            let mut b = x.borrow_mut();
            b.set = true;
            b.tgt = bt;
        }
        Ok(x)
    }

    fn is_stmt(&self, l: i32, treg: i32) -> bool {
        let r = self.r.borrow();
        let (a, b, c) = (self.a(l), self.b(l), self.c(l));
        let any = |x: i32, y: i32| (x..y).any(|q| r.local(q, l));
        match self.op(l) {
            o::MOVE | o::LOADK | o::LOADBOOL | o::GETUPVAL | o::GETGLOBAL | o::GETTABLE | o::NEWTABLE | o::ADD..=o::CONCAT | o::CLOSURE => r.local(a, l) || a == treg,
            o::LOADNIL => any(a, b + 1),
            o::SETTABLE => r.local(a, l) || !self.ctor(l, a),
            o::SETGLOBAL | o::SETUPVAL | o::JMP | o::TAILCALL | o::RETURN | o::FORLOOP | o::FORPREP | o::TFORLOOP | o::CLOSE => true,
            o::SELF => any(a, a + 2),
            o::CALL => {
                let c = if c == 0 { self.n - a + 1 } else { c };
                self.c(l) == 1 || any(a, a + c - 1) || (c == 2 && a == treg)
            }
            o::VARARG => any(a, a + if b == 0 { self.n - a + 1 } else { b } - 1),
            _ => false,
        }
    }

    fn writes(&self, l: i32) -> (i32, i32) {
        let (a, b, c) = (self.a(l), self.b(l), self.c(l));
        match self.op(l) {
            o::MOVE | o::LOADK | o::LOADBOOL | o::GETUPVAL | o::GETGLOBAL | o::GETTABLE | o::NEWTABLE | o::ADD..=o::CONCAT | o::CLOSURE => (a, a),
            o::LOADNIL => (a, b),
            o::SELF => (a, a + 1),
            o::CALL => (a, if c == 0 { 255 } else { a + c - 2 }),
            o::VARARG => (a, if b == 0 { 255 } else { a + b - 2 }),
            o::TFORLOOP => (a + 3, 255),
            _ => (-1, -2),
        }
    }

    fn ctor(&self, l: i32, r: i32) -> bool {
        (1..l).rev().map(|q| (q, self.writes(q))).find(|&(_, (x, y))| x <= r && r <= y).is_some_and(|(q, _)| self.op(q) == o::NEWTABLE)
    }

    fn assigned(&self, l: i32) -> i32 {
        let (a, b, c) = (self.a(l), self.b(l), self.c(l));
        match self.op(l) {
            o::MOVE | o::LOADK | o::LOADBOOL | o::GETUPVAL | o::GETGLOBAL | o::GETTABLE | o::NEWTABLE | o::ADD..=o::CONCAT | o::CLOSURE => a,
            o::LOADNIL if a == b => a,
            o::CALL if c == 2 => a,
            o::VARARG if c == 2 => b,
            _ => -1,
        }
    }

    fn branches(&mut self, first: bool) -> R<Bl> {
        let rc = self.r.clone();
        let len = self.len;
        let old = std::mem::take(&mut self.blocks);
        let outer = block(Kind::Outer, 0, len + 1, &rc);
        self.blocks.push(outer.clone());
        let n = len as usize + 2;
        let mut brk = vec![false; n];
        let mut removed = vec![false; n];
        if !first {
            for b in &old {
                match b.borrow().kind {
                    Kind::Loop => self.blocks.push(b.clone()),
                    Kind::Break { .. } => {
                        self.blocks.push(b.clone());
                        mark(&mut brk, b.borrow().begin);
                    }
                    _ => {}
                }
            }
            let mut del = Vec::new();
            for b in &self.blocks {
                if !matches!(b.borrow().kind, Kind::Loop) {
                    continue;
                }
                for b2 in &self.blocks {
                    let (x, y) = (b.borrow(), b2.borrow());
                    if !Rc::ptr_eq(b, b2) && x.begin == y.begin {
                        let (d, e) = if x.end < y.end { (b, x.end) } else { (b2, y.end) };
                        del.push(d.clone());
                        mark(&mut removed, e - 1);
                    }
                }
            }
            for b in &del {
                remove(&mut self.blocks, b);
            }
        }
        self.skip = vec![false; n];
        let mut st: Vec<B> = Vec::new();
        let (mut reduce, mut ts, mut tse) = (false, false, -1);
        let mut line = 1;
        while line <= len {
            if !at(&self.skip, line) && self.op(line) == o::JMP && self.sbx(line) < 0 && st.last().is_some_and(|x| x.borrow().begin == line) {
                self.reduce(&mut st, &brk)?;
            }
            if !at(&self.skip, line) {
                let (a, b, c) = (self.a(line), self.b(line), self.c(line));
                match self.op(line) {
                    x @ (o::EQ | o::LT | o::LE | o::TEST | o::TESTSET) => {
                        let e = line + 2 + self.sbx(line + 1);
                        let nd = match x {
                            o::TEST => test(a, c != 0, line, line + 2, e),
                            o::TESTSET => {
                                ts = true;
                                tse = e;
                                tset(a, b, c != 0, line, line + 2, e)
                            }
                            _ => cmp(x, b, c, a != 0, line, line + 2, e),
                        };
                        st.push(nd.clone());
                        mark(&mut self.skip, line + 1);
                        if x < o::TEST && self.op(e) == o::LOADBOOL && (self.c(e) != 0 || (e > 1 && self.op(e - 1) == o::LOADBOOL && self.c(e - 1) != 0)) {
                            let mut x = nd.borrow_mut();
                            x.cset = true;
                            x.tgt = self.a(e);
                        }
                        line += 1;
                        continue;
                    }
                    o::JMP => {
                        reduce = true;
                        let tl = line + 1 + self.sbx(line);
                        if tl >= 2 && self.op(tl - 1) == o::LOADBOOL && self.c(tl - 1) != 0 {
                            st.push(tru(self.a(tl - 1), false, line, line + 1, tl));
                            mark(&mut self.skip, line + 1);
                        } else if self.op(tl) == o::TFORLOOP && !at(&self.skip, tl) {
                            let (ta, tc) = (self.a(tl), self.c(tl));
                            if tc == 0 {
                                return bad("generic for without variables");
                            }
                            {
                                let mut r = rc.borrow_mut();
                                for i in 0..3 {
                                    r.loop_var(ta + i, tl, line + 1, false)?;
                                }
                                for i in 1..=tc {
                                    r.loop_var(ta + 2 + i, line, tl + 2, true)?;
                                }
                            }
                            mark(&mut self.skip, tl);
                            mark(&mut self.skip, tl + 1);
                            self.blocks.push(block(Kind::TFor { reg: ta, n: tc }, line + 1, tl + 2, &rc));
                        } else if self.sbx(line) == 2 && self.op(line + 1) == o::LOADBOOL && self.c(line + 1) != 0 {
                            self.blocks.push(block(Kind::BoolInd, line, line, &rc));
                        } else if self.op(tl) == o::JMP && self.sbx(tl) + tl == line {
                            if first {
                                self.blocks.push(block(Kind::Loop, line, tl + 1, &rc));
                            }
                            mark(&mut self.skip, tl);
                        } else if (first || at(&removed, line) || at(&self.rev, line + 1)) && !at(&brk, line) {
                            if tl > line {
                                mark(&mut brk, line);
                                self.blocks.push(block(Kind::Break { target: tl }, line, line, &rc));
                            } else {
                                let e = self.breakable(line).map(|e| e.borrow().end).filter(|&e| self.op(e) == o::JMP && self.sbx(e) + e + 1 == tl);
                                if let Some(e) = e {
                                    mark(&mut brk, line);
                                    self.blocks.push(block(Kind::Break { target: e }, line, line, &rc));
                                } else if !self.blocks.iter().any(|b| matches!(b.borrow().kind, Kind::Loop) && b.borrow().begin == tl && b.borrow().end == line + 1) {
                                    self.blocks.push(block(Kind::Loop, tl, line + 1, &rc));
                                }
                            }
                        }
                    }
                    o::FORPREP => {
                        reduce = true;
                        let e = line + 2 + self.sbx(line);
                        self.blocks.push(block(Kind::For { reg: a }, line + 1, e, &rc));
                        mark(&mut self.skip, e - 1);
                        let mut r = rc.borrow_mut();
                        for i in 0..4 {
                            r.loop_var(a + i, line, e, i == 3)?;
                        }
                    }
                    o::FORLOOP => return bad("for loop without its preparation"),
                    o::CLOSURE => {
                        let nu = self.p.protos.get(self.bx(line) as usize).map_or(0, |p| p.nups as i32);
                        for i in 1..=nu {
                            mark(&mut self.skip, line + i);
                        }
                        reduce = self.is_stmt(line, -1) || rc.borrow().local(a, line + nu);
                    }
                    _ => reduce = self.is_stmt(line, -1),
                }
            }
            if (line < len && at(&self.rev, line + 1)) || (ts && tse == line + 1) {
                reduce = true;
            }
            if reduce && !st.is_empty() {
                self.reduce(&mut st, &brk)?;
            }
            reduce = false;
            line += 1;
        }
        for d in self.decls.clone() {
            if !d.fl.get() && !d.fx.get() && !self.blocks.iter().any(|b| b.borrow().container() && b.borrow().has(d.begin) && b.borrow().scope_end() == d.end) {
                let mut db = d.begin;
                while let Some(x) = self.blocks.iter().map(|b| b.borrow()).filter(|b| !b.container() && b.begin < db && b.end > db).map(|b| b.begin).min() {
                    db = x;
                }
                self.blocks.push(block(Kind::Do, db, d.end + 1, &rc));
            }
        }
        let skip = &self.skip;
        self.blocks.retain(|b| {
            let b = b.borrow();
            !(at(skip, b.begin) && matches!(b.kind, Kind::Break { .. }))
        });
        if !first {
            self.need = self.ladder_split();
            if self.need.is_none() {
                self.const_conds();
            }
        }
        self.blocks.sort_by(|a, b| a.borrow().order(&b.borrow()));
        self.backup = None;
        Ok(outer)
    }

    fn reduce(&mut self, st: &mut Vec<B>, brk: &[bool]) -> R {
        let rc = self.r.clone();
        let mut conds = Vec::new();
        loop {
            let depth = st.len();
            let Some(top) = st.last().cloned() else { return bad("empty branch stack") };
            let (tb, tl, cset, ttg, k) = {
                let t = top.borrow();
                let k = match t.kind {
                    Bk::TestSet { .. } => 1,
                    Bk::True { .. } => 2,
                    Bk::Test { reg, .. } => 3 + reg,
                    _ => 0,
                };
                (t.begin, t.line, t.cset, t.tgt, k)
            };
            let treg = if k >= 3 { Some(k - 3) } else { None };
            let mut ae = top.borrow().end;
            let (mut an, mut cc, mut areg) = (k == 1, false, ttg);
            let lb = |l: i32| self.op(l) == o::LOADBOOL && self.c(l) != 0;
            let j2 = |l: i32| self.op(l) == o::JMP && self.sbx(l) == 2;
            if k == 2 || (cset && !lb(tb)) {
                an = true;
                cc = true;
                ae += if self.c(ae) != 0 { 2 } else { 1 };
            } else if cset {
            } else if ae - 3 >= 1 && lb(ae - 2) && j2(ae - 3) {
                an = an || treg == Some(self.a(ae - 2));
            } else if ae - 2 >= 1 && lb(ae - 1) && j2(ae - 2) {
                if treg.is_some() {
                    an = true;
                    if areg < 0 {
                        areg = self.a(ae - 1);
                    }
                    ae += 1;
                }
            } else if ae > 1 && lb(ae) && j2(ae - 1) {
                if treg.is_some() {
                    an = true;
                    if areg < 0 {
                        areg = self.a(ae);
                    }
                    ae += 2;
                }
            } else if ae > 1 && ae > tl {
                let x = self.assigned(ae - 1);
                if let Some(d) = rc.borrow().decl(x, ae - 1).filter(|_| x >= 0) {
                    let grp = x > 0 && rc.borrow().decl(x - 1, ae - 1).is_some_and(|e| e.begin == ae - 1);
                    let tail = d.end == ae - 1 && treg == Some(x) && (matches!(self.op(ae), o::JMP | o::FORLOOP | o::TFORLOOP) || grp) && (tb..ae - 1).all(|l| at(&self.skip, l) || !self.is_stmt(l, -1));
                    if d.begin == ae - 1 && (d.end > ae - 1 || tail) && !an {
                        areg = x;
                        an = true;
                    }
                }
            }
            if let (false, Some(r)) = (an, treg) {
                if st.iter().any(|b| matches!(b.borrow().kind, Bk::TestSet { .. }) && b.borrow().tgt == r && b.borrow().end == ae) {
                    an = true;
                    areg = r;
                }
            }
            let (c, back) = if !cc && ae - 1 == tb && lb(tb) {
                self.backup = None;
                let (ae, tg) = (tb + 2, self.a(tb));
                let c = self.pop_cmp_set(st, ae, tg)?;
                (Self::fix(c, tg, tb, ae), None)
            } else if an {
                self.backup = None;
                let c = self.pop_set(st, ae, areg)?;
                (Self::fix(c, areg, tb, ae), None)
            } else {
                self.backup = Some(Vec::new());
                let c = self.pop(st)?;
                let mut b = self.backup.take();
                if let Some(b) = &mut b {
                    b.reverse();
                }
                (c, b)
            };
            conds.push((c, back));
            if st.len() >= depth {
                return bad("branch reduction stalled");
            }
            if st.is_empty() {
                break;
            }
        }
        while let Some((mut cond, back)) = conds.pop() {
            self.place(&mut cond, back, brk)?;
        }
        Ok(())
    }

    fn fix(c: B, tgt: i32, begin: i32, end: i32) -> B {
        {
            let mut x = c.borrow_mut();
            x.tgt = tgt;
            x.end = end;
            x.begin = begin;
        }
        c
    }

    fn place(&mut self, cond: &mut B, back: Option<Vec<B>>, brk: &[bool]) -> R {
        let rc = self.r.clone();
        let cb = cond.borrow().begin;
        let mut bt = self.break_target(cb);
        if bt >= 1 {
            if self.op(bt) == o::JMP && bt != cond.borrow().end {
                bt += 1 + self.sbx(bt);
            }
            if bt == cond.borrow().end {
                let imm = self.enc(cb, Block::container);
                let mut ls = imm.borrow().end;
                if self.breakable(cb).is_some_and(|b| Rc::ptr_eq(&imm, &b)) {
                    ls -= 1;
                }
                let lower = cb.max(imm.borrow().begin);
                if let Some(il) = (lower..=ls).rev().find(|&l| self.op(l) == o::JMP && l + 1 + self.sbx(l) == bt) {
                    cond.borrow_mut().end = il;
                }
            }
        }
        let (begin, mut end) = (cb, cond.borrow().end);
        let (set, stg, is_true) = {
            let c = cond.borrow();
            (c.set, c.tgt, matches!(c.kind, Bk::True { .. }))
        };
        if !set && begin < end && self.op(begin) == o::JMP && self.sbx(begin) > 0 && begin + 1 + self.sbx(begin) == end && !is_true && self.breakable(begin).is_none() {
            cond.borrow_mut().end = begin;
            end = begin;
        }
        let tail_of = |s: &Self, e: i32| if e >= 2 && s.op(e - 1) == o::JMP { Some(e + s.sbx(e - 1)) } else { None };
        let mut tail = tail_of(self, end);
        let otail = tail.unwrap_or(-1);
        if let Some(e) = self.enc_inner(begin, Block::unprotected) {
            let (lb, ee) = (e.borrow().back(), e.borrow().end);
            if lb == end {
                end = ee - 1;
                cond.borrow_mut().end = end;
                tail = tail_of(self, end);
            }
            if tail == Some(lb) {
                tail = Some(ee - 1);
            }
        }
        let push = |s: &mut Self, k: Kind, b: i32, e: i32| s.blocks.push(block(k, b, e, &rc));
        if !set && end == begin + 2 && self.op(begin) == o::CLOSE && self.op(begin + 1) == o::JMP && self.op(end) == o::CLOSE && self.op(end + 1) == o::JMP {
            let s = end + 2 + self.sbx(end + 1);
            let exit = begin + 2 + self.sbx(begin + 1);
            let wh = (s.max(1)..begin).any(|l| matches!(self.op(l), o::EQ | o::LT | o::LE | o::TEST | o::TESTSET) && self.op(l + 1) == o::JMP && l + 2 + self.sbx(l + 1) == exit);
            if s < begin && !wh {
                (begin..end + 2).for_each(|l| mark(&mut self.skip, l));
                push(self, Kind::Repeat { br: cond.clone() }, s, end);
                return Ok(());
            }
        }
        let span = |c: &B| (c.borrow().begin, c.borrow().end);
        if set {
            let empty = begin == end || (self.op(begin) == o::JMP && self.sbx(begin) == 2 && self.op(begin + 1) == o::LOADBOOL && self.c(begin + 1) != 0);
            push(self, Kind::Set { tgt: stg, asg: None, br: cond.clone(), empty, fin: false }, begin - (begin == end) as i32, end);
        } else if self.op(begin) == o::LOADBOOL && self.c(begin) != 0 {
            let br = if self.b(begin) == 0 { invert(cond)? } else { cond.clone() };
            push(self, Kind::Cmp { tgt: self.a(begin), br }, begin, begin + 2);
        } else if end < begin && self.op(begin) == o::JMP && begin + 1 + self.sbx(begin) == end {
            cond.borrow_mut().end = begin;
            self.push_if(cond, back);
        } else if end < begin && self.op(begin) == o::JMP && self.sbx(begin) > 0 && self.op(begin + 1) == o::JMP && begin + 2 + self.sbx(begin + 1) == end {
            cond.borrow_mut().end = begin + 1;
            self.push_if(cond, back);
        } else if end < begin {
            if at(brk, end - 1) && self.op(end - 1) == o::JMP && (end + 1..begin).contains(&(end + self.sbx(end - 1))) {
                mark(&mut self.skip, end - 1);
                let br = invert(cond)?;
                let (b, e) = span(&br);
                push(self, Kind::While { br, back: otail }, b, e);
            } else {
                push(self, Kind::Repeat { br: cond.clone() }, end, begin);
            }
        } else if let Some(tail) = tail {
            let ecj = matches!(self.op(end - 2), o::EQ | o::LE | o::LT | o::TEST | o::TESTSET);
            if tail > end || (tail == end && !ecj) {
                let lb2 = tail + self.sbx(tail - 1);
                if matches!(self.op(tail - 1), o::JMP | o::FORLOOP) && lb2 <= begin && !at(brk, tail - 1) {
                    self.push_if(cond, back);
                } else {
                    mark(&mut self.skip, end - 1);
                    let (b, e) = span(cond);
                    push(self, Kind::IfElse { br: cond.clone(), back: otail, empty: tail == end }, b, e);
                    if tail != end {
                        push(self, Kind::ElseEnd, end, tail);
                    }
                }
            } else if tail >= begin || (tail..begin).any(|l| !at(&self.skip, l) && self.is_stmt(l, -1)) {
                self.push_if(cond, back);
            } else {
                mark(&mut self.skip, end - 1);
                let (b, e) = span(cond);
                push(self, Kind::While { br: cond.clone(), back: otail }, b, e);
            }
        } else {
            self.push_if(cond, back);
        }
        Ok(())
    }

    fn push_if(&mut self, c: &B, stack: Option<Vec<B>>) {
        let (b, e) = (c.borrow().begin, c.borrow().end);
        let (b, e) = if b == e { (b - 1, b - 1) } else { (b, e) };
        self.blocks.push(block(Kind::If { br: c.clone(), stack }, b, e, &self.r));
    }

    fn const_conds(&mut self) {
        let mut v: Vec<(Bl, i32, i32)> = self
            .blocks
            .iter()
            .filter_map(|b| match b.borrow().kind {
                Kind::Break { target } => Some((b.clone(), b.borrow().begin, target)),
                _ => None,
            })
            .collect();
        v.sort_by_key(|x| x.1);
        let rc = self.r.clone();
        for (b, l, tl) in v {
            if !self.blocks.iter().any(|x| Rc::ptr_eq(x, &b)) || self.breakable(l).is_some() {
                continue;
            }
            remove(&mut self.blocks, &b);
            let br = tru(-1, false, l, l + 1, tl);
            let tj = tl - 1;
            if tj > l && self.op(tj) == o::JMP && self.sbx(tj) > 0 {
                let e = tl + self.sbx(tj);
                if let Some(t) = self.blocks.iter().find(|x| x.borrow().begin == tj && matches!(x.borrow().kind, Kind::Break { .. })).cloned() {
                    remove(&mut self.blocks, &t);
                }
                self.blocks.push(block(Kind::IfElse { br, back: e, empty: false }, l + 1, tl, &rc));
                self.blocks.push(block(Kind::ElseEnd, tl, e, &rc));
            } else {
                self.blocks.push(block(Kind::If { br, stack: None }, l + 1, tl, &rc));
            }
        }
    }

    fn ladder_split(&self) -> Option<i32> {
        for b in &self.blocks {
            let (p, t) = match b.borrow().kind {
                Kind::Break { target } => (b.borrow().begin, target),
                _ => continue,
            };
            if self.breakable(p).is_some() || t - p < 2 || !(p + 1..t).all(|q| self.op(q) == o::JMP && q + 1 + self.sbx(q) == t) {
                continue;
            }
            let mut best: Option<(i32, i32)> = None;
            for c in &self.blocks {
                let c = c.borrow();
                if let Kind::IfElse { br, .. } = &c.kind {
                    if let (true, Bk::And(l, _)) = (c.begin <= p && p < c.end, &br.borrow().kind) {
                        if best.is_none_or(|(x, _)| c.begin < x) {
                            best = Some((c.begin, l.borrow().line));
                        }
                    }
                }
            }
            if let Some((_, l)) = best.filter(|(_, l)| !self.splits.contains(l)) {
                return Some(l);
            }
        }
        None
    }

    fn separated(&self, l: i32, r: i32) -> bool {
        self.decls.iter().any(|d| !d.fl.get() && !d.fx.get() && d.end >= l && d.end < r)
            || self.blocks[1..].iter().any(|b| {
            let b = b.borrow();
            b.container() && !matches!(b.kind, Kind::ElseEnd | Kind::Do) && b.begin <= l && l < b.end && b.end <= r
        })
    }
}
