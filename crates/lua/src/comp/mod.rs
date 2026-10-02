mod code;
mod lex;

use crate::luac::{write, Chunk, Header, Local, Proto};
use ac_core::{Error, Res};
use code::{int2fb, o, Bin, Bl, Fs, Un, E, K, FIELDS_PER_FLUSH, MULTRET, NO_JUMP, NO_REG};
use lex::{show, Lex, T};

const MAX_VARS: i32 = 200;
const MAX_UPS: usize = 60;
const MAX_DEPTH: u32 = 200;

struct P<'a> {
    lx: Lex<'a>,
    fs: Vec<Fs>,
    depth: u32,
    ver: u8,
}

type R<X = ()> = Res<X>;

impl<'a> P<'a> {
    fn f(&mut self) -> &mut Fs {
        self.fs.last_mut().unwrap()
    }

    fn ln(&self) -> u32 {
        self.lx.last
    }

    fn ck<X>(&self, r: Result<X, &'static str>) -> R<X> {
        r.or_else(|m| self.lx.syntax(m))
    }

    fn next(&mut self) -> R {
        self.lx.next()
    }

    fn is(&self, t: &T) -> bool {
        &self.lx.t == t
    }

    fn test(&mut self, t: &T) -> R<bool> {
        if self.is(t) {
            self.next()?;
            return Ok(true);
        }
        Ok(false)
    }

    fn expect(&self, t: &T) -> R {
        if self.is(t) {
            Ok(())
        } else {
            self.lx.syntax(&format!("'{}' expected", show(t)))
        }
    }

    fn check_next(&mut self, t: &T) -> R {
        self.expect(t)?;
        self.next()
    }

    fn check_match(&mut self, what: &T, who: &T, line: u32) -> R {
        if self.test(what)? {
            return Ok(());
        }
        if line == self.lx.line {
            return self.expect(what);
        }
        self.lx.syntax(&format!("'{}' expected (to close '{}' at line {line})", show(what), show(who)))
    }

    fn name(&mut self) -> R<Vec<u8>> {
        match &self.lx.t {
            T::Name(n) => {
                let n = n.clone();
                self.next()?;
                Ok(n)
            }
            _ => self.lx.syntax("'<name>' expected"),
        }
    }

    fn enter(&mut self) -> R {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.lx.err("chunk has too many syntax levels", None);
        }
        Ok(())
    }

    fn limit(&self, v: i32, l: i32, what: &str) -> R {
        if v <= l {
            return Ok(());
        }
        let f = self.fs.last().unwrap();
        let m = if f.p.line == 0 { format!("main function has more than {l} {what}") } else { format!("function at line {} has more than {l} {what}", f.p.line) };
        self.lx.err(&m, None)
    }

    fn new_local(&mut self, name: Vec<u8>, n: i32) -> R {
        let nact = self.f().nact;
        self.limit(nact + n + 1, MAX_VARS, "local variables")?;
        let f = self.f();
        f.p.locals.push(Local { name, start: 0, end: 0 });
        let i = f.p.locals.len() - 1;
        let at = (f.nact + n) as usize;
        if f.act.len() <= at {
            f.act.resize(at + 1, 0);
        }
        f.act[at] = i;
        Ok(())
    }

    fn adjust_locals(&mut self, n: i32) {
        let f = self.f();
        f.nact += n;
        let pc = f.pc() as u64;
        for k in (f.nact - n)..f.nact {
            f.loc(k).start = pc;
        }
    }

    fn remove_vars(&mut self, to: i32) {
        let f = self.f();
        let pc = f.pc() as u64;
        while f.nact > to {
            f.nact -= 1;
            let n = f.nact;
            f.loc(n).end = pc;
        }
    }

    fn index_up(&mut self, lvl: usize, name: &[u8], v: &E) -> R<i32> {
        let f = &mut self.fs[lvl];
        if let Some(i) = f.upv.iter().position(|&(k, info)| k == v.k && info == v.info) {
            return Ok(i as i32);
        }
        if f.upv.len() + 1 > MAX_UPS {
            let l = f.p.line;
            let m = if l == 0 { format!("main function has more than {MAX_UPS} upvalues") } else { format!("function at line {l} has more than {MAX_UPS} upvalues") };
            return self.lx.err(&m, None);
        }
        f.p.ups.push(name.to_vec());
        f.upv.push((v.k, v.info));
        f.p.nups = f.upv.len() as u8;
        Ok(f.upv.len() as i32 - 1)
    }

    fn var_aux(&mut self, lvl: usize, name: &[u8], base: bool) -> R<E> {
        let f = &mut self.fs[lvl];
        let found = (0..f.nact).rev().find(|&i| f.p.locals[f.act[i as usize]].name == name);
        if let Some(v) = found {
            if !base {
                if let Some(b) = f.bl.iter_mut().rev().find(|b| b.nact <= v) {
                    b.upval = true;
                }
            }
            return Ok(E::new(K::Local, v));
        }
        if lvl == 0 {
            return Ok(E::new(K::Global, NO_REG));
        }
        let up = self.var_aux(lvl - 1, name, false)?;
        if up.k == K::Global {
            return Ok(up);
        }
        let i = self.index_up(lvl, name, &up)?;
        Ok(E::new(K::Upval, i))
    }

    fn single_var(&mut self) -> R<E> {
        let n = self.name()?;
        let lvl = self.fs.len() - 1;
        let mut e = self.var_aux(lvl, &n, true)?;
        if e.k == K::Global {
            e.info = self.f().str_k(&n);
        }
        Ok(e)
    }

    fn adjust_assign(&mut self, nvars: i32, nexps: i32, e: &mut E) -> R {
        let extra = nvars - nexps;
        let ln = self.ln();
        if e.multret() {
            let x = (extra + 1).max(0);
            let r = self.f().set_returns(e, x);
            self.ck(r)?;
            if x > 1 {
                let r = self.f().reserve(x - 1);
                self.ck(r)?;
            }
        } else {
            if e.k != K::Void {
                let r = self.f().exp2next(e, ln);
                self.ck(r)?;
            }
            if extra > 0 {
                let reg = self.f().free;
                let r = self.f().reserve(extra);
                self.ck(r)?;
                self.f().nil(reg, extra, ln);
            }
        }
        Ok(())
    }

    fn enter_block(&mut self, loop_: bool) {
        let f = self.f();
        let nact = f.nact;
        f.bl.push(Bl { brk: NO_JUMP, nact, upval: false, loop_ });
    }

    fn leave_block(&mut self) {
        let b = self.f().bl.pop().unwrap();
        self.remove_vars(b.nact);
        let ln = self.ln();
        let f = self.f();
        if b.upval {
            f.abc(o::CLOSE, b.nact, 0, 0, ln);
        }
        f.free = f.nact;
        f.patch_here(b.brk);
    }

    fn open_func(&mut self) {
        let src = if self.fs.is_empty() { Some(self.lx.src.clone()) } else { None };
        let mut f = Fs::new(src);
        f.ver = self.ver;
        self.fs.push(f);
    }

    fn close_func(&mut self) -> Proto {
        self.remove_vars(0);
        let ln = self.ln();
        self.f().ret(0, 0, ln);
        let mut f = self.fs.pop().unwrap();
        f.p.nups = f.upv.len() as u8;
        f.p
    }

    fn push_closure(&mut self, child: Proto, upv: Vec<(K, i32)>) -> R<E> {
        let ln = self.ln();
        let f = self.f();
        f.p.protos.push(child);
        let n = f.p.protos.len() as i32 - 1;
        let e = E::new(K::Reloc, f.abx(o::CLOSURE, 0, n, ln));
        for (k, info) in upv {
            f.abc(if k == K::Local { o::MOVE } else { o::GETUPVAL }, 0, info, 0, ln);
        }
        Ok(e)
    }

    fn field(&mut self, v: &mut E) -> R {
        let ln = self.ln();
        let r = self.f().exp2any(v, ln);
        self.ck(r)?;
        self.next()?;
        let n = self.name()?;
        let mut k = E::new(K::Const, self.f().str_k(&n));
        let ln = self.ln();
        let r = self.f().indexed(v, &mut k, ln);
        self.ck(r)
    }

    fn yindex(&mut self) -> R<E> {
        self.next()?;
        let mut v = self.expr()?;
        let ln = self.ln();
        let r = self.f().exp2val(&mut v, ln);
        self.ck(r)?;
        self.check_next(&T::Ch(b']'))?;
        Ok(v)
    }

    fn constructor(&mut self) -> R<E> {
        let line = self.lx.line;
        let ln = self.ln();
        let pc = self.f().abc(o::NEWTABLE, 0, 0, 0, ln);
        let mut t = E::new(K::Reloc, pc);
        let mut v = E::new(K::Void, 0);
        let (mut na, mut nh, mut tostore) = (0i32, 0i32, 0i32);
        let r = self.f().exp2next(&mut t, ln);
        self.ck(r)?;
        self.check_next(&T::Ch(b'{'))?;
        loop {
            if self.is(&T::Ch(b'}')) {
                break;
            }
            if v.k != K::Void {
                let ln = self.ln();
                let r = self.f().exp2next(&mut v, ln);
                self.ck(r)?;
                v.k = K::Void;
                if tostore == FIELDS_PER_FLUSH {
                    let ln = self.ln();
                    self.f().set_list(t.info, na, tostore, ln);
                    tostore = 0;
                }
            }
            let rec = match &self.lx.t {
                T::Name(_) => self.lx.peek()? == &T::Ch(b'='),
                T::Ch(b'[') => true,
                _ => false,
            };
            if rec {
                let reg = self.f().free;
                let mut key = if let T::Name(_) = self.lx.t {
                    let n = self.name()?;
                    E::new(K::Const, self.f().str_k(&n))
                } else {
                    self.yindex()?
                };
                nh += 1;
                self.check_next(&T::Ch(b'='))?;
                let ln = self.ln();
                let r = self.f().exp2rk(&mut key, ln);
                let rk = self.ck(r)?;
                let mut val = self.expr()?;
                let ln = self.ln();
                let r = self.f().exp2rk(&mut val, ln);
                let rv = self.ck(r)?;
                let f = self.f();
                f.abc(o::SETTABLE, t.info, rk, rv, ln);
                f.free = reg;
            } else {
                v = self.expr()?;
                na += 1;
                tostore += 1;
            }
            if !(self.test(&T::Ch(b','))? || self.test(&T::Ch(b';'))?) {
                break;
            }
        }
        self.check_match(&T::Ch(b'}'), &T::Ch(b'{'), line)?;
        if tostore > 0 {
            let ln = self.ln();
            if v.multret() {
                let r = self.f().set_returns(&v, MULTRET);
                self.ck(r)?;
                self.f().set_list(t.info, na, MULTRET, ln);
                na -= 1;
            } else {
                if v.k != K::Void {
                    let r = self.f().exp2next(&mut v, ln);
                    self.ck(r)?;
                }
                self.f().set_list(t.info, na, tostore, ln);
            }
        }
        let f = self.f();
        let i = &mut f.p.code[pc as usize];
        *i = (*i & !(0x1ff << 23) & !(0x1ff << 14)) | (int2fb(na as u32) as u32) << 23 | (int2fb(nh as u32) as u32) << 14;
        Ok(t)
    }

    fn par_list(&mut self) -> R {
        let mut n = 0;
        let mut va = 0u8;
        if !self.is(&T::Ch(b')')) {
            loop {
                match &self.lx.t {
                    T::Name(_) => {
                        let nm = self.name()?;
                        self.new_local(nm, n)?;
                        n += 1;
                    }
                    T::Dots => {
                        self.next()?;
                        self.new_local(b"arg".to_vec(), n)?;
                        n += 1;
                        va = 7;
                    }
                    _ => return self.lx.syntax("<name> or '...' expected"),
                }
                if va != 0 || !self.test(&T::Ch(b','))? {
                    break;
                }
            }
        }
        self.adjust_locals(n);
        let f = self.f();
        f.p.vararg = va;
        f.p.params = (f.nact - (va & 1) as i32) as u8;
        let k = f.nact;
        let r = f.reserve(k);
        self.ck(r)
    }

    fn body(&mut self, needself: bool, line: u32) -> R<E> {
        self.open_func();
        self.f().p.line = line as u64;
        self.check_next(&T::Ch(b'('))?;
        if needself {
            self.new_local(b"self".to_vec(), 0)?;
            self.adjust_locals(1);
        }
        self.par_list()?;
        self.check_next(&T::Ch(b')'))?;
        self.chunk()?;
        self.f().p.last = self.lx.line as u64;
        self.check_match(&T::End, &T::Function, line)?;
        let upv = self.fs.last().unwrap().upv.clone();
        let p = self.close_func();
        self.push_closure(p, upv)
    }

    fn exp_list(&mut self) -> R<(i32, E)> {
        let mut n = 1;
        let mut v = self.expr()?;
        while self.test(&T::Ch(b','))? {
            let ln = self.ln();
            let r = self.f().exp2next(&mut v, ln);
            self.ck(r)?;
            v = self.expr()?;
            n += 1;
        }
        Ok((n, v))
    }

    fn func_args(&mut self, f: &mut E) -> R {
        let line = self.lx.line;
        let mut args = match self.lx.t.clone() {
            T::Ch(b'(') => {
                if line != self.lx.last {
                    return self.lx.syntax("ambiguous syntax (function call x new statement)");
                }
                self.next()?;
                let a = if self.is(&T::Ch(b')')) {
                    E::new(K::Void, 0)
                } else {
                    let (_, a) = self.exp_list()?;
                    let r = self.f().set_returns(&a, MULTRET);
                    self.ck(r)?;
                    a
                };
                self.check_match(&T::Ch(b')'), &T::Ch(b'('), line)?;
                a
            }
            T::Ch(b'{') => self.constructor()?,
            T::Str(s) => {
                let e = E::new(K::Const, self.f().str_k(&s));
                self.next()?;
                e
            }
            _ => return self.lx.syntax("function arguments expected"),
        };
        let base = f.info;
        let np = if args.multret() {
            MULTRET
        } else {
            if args.k != K::Void {
                let ln = self.ln();
                let r = self.f().exp2next(&mut args, ln);
                self.ck(r)?;
            }
            self.f().free - (base + 1)
        };
        let ln = self.ln();
        let fs = self.f();
        *f = E::new(K::Call, fs.abc(o::CALL, base, np + 1, 2, ln));
        fs.fix_line(line);
        fs.free = base + 1;
        Ok(())
    }

    fn prefix_exp(&mut self) -> R<E> {
        match self.lx.t {
            T::Ch(b'(') => {
                let line = self.lx.line;
                self.next()?;
                let mut v = self.expr()?;
                self.check_match(&T::Ch(b')'), &T::Ch(b'('), line)?;
                let ln = self.ln();
                self.f().discharge_vars(&mut v, ln);
                Ok(v)
            }
            T::Name(_) => self.single_var(),
            _ => self.lx.syntax("unexpected symbol"),
        }
    }

    fn primary_exp(&mut self) -> R<E> {
        let mut v = self.prefix_exp()?;
        loop {
            match self.lx.t {
                T::Ch(b'.') => self.field(&mut v)?,
                T::Ch(b'[') => {
                    let ln = self.ln();
                    let r = self.f().exp2any(&mut v, ln);
                    self.ck(r)?;
                    let mut k = self.yindex()?;
                    let ln = self.ln();
                    let r = self.f().indexed(&mut v, &mut k, ln);
                    self.ck(r)?;
                }
                T::Ch(b':') => {
                    self.next()?;
                    let n = self.name()?;
                    let mut k = E::new(K::Const, self.f().str_k(&n));
                    let ln = self.ln();
                    let r = self.f().self_(&mut v, &mut k, ln);
                    self.ck(r)?;
                    self.func_args(&mut v)?;
                }
                T::Ch(b'(') | T::Str(_) | T::Ch(b'{') => {
                    let ln = self.ln();
                    let r = self.f().exp2next(&mut v, ln);
                    self.ck(r)?;
                    self.func_args(&mut v)?;
                }
                _ => return Ok(v),
            }
        }
    }

    fn simple_exp(&mut self) -> R<E> {
        let v = match self.lx.t.clone() {
            T::Num(n) => E::num(n),
            T::Str(s) => E::new(K::Const, self.f().str_k(&s)),
            T::Nil => E::new(K::Nil, 0),
            T::True => E::new(K::True, 0),
            T::False => E::new(K::False, 0),
            T::Dots => {
                if self.f().p.vararg == 0 {
                    return self.lx.syntax("cannot use '...' outside a vararg function");
                }
                let ln = self.ln();
                let f = self.f();
                f.p.vararg &= !4;
                E::new(K::Vararg, f.abc(o::VARARG, 0, 1, 0, ln))
            }
            T::Ch(b'{') => return self.constructor(),
            T::Function => {
                self.next()?;
                let l = self.lx.line;
                return self.body(false, l);
            }
            _ => return self.primary_exp(),
        };
        self.next()?;
        Ok(v)
    }

    fn unop(t: &T) -> Option<Un> {
        match t {
            T::Not => Some(Un::Not),
            T::Ch(b'-') => Some(Un::Minus),
            T::Ch(b'#') => Some(Un::Len),
            _ => None,
        }
    }

    fn binop(t: &T) -> Option<Bin> {
        Some(match t {
            T::Ch(b'+') => Bin::Add,
            T::Ch(b'-') => Bin::Sub,
            T::Ch(b'*') => Bin::Mul,
            T::Ch(b'/') => Bin::Div,
            T::Ch(b'%') => Bin::Mod,
            T::Ch(b'^') => Bin::Pow,
            T::Concat => Bin::Concat,
            T::Ne => Bin::Ne,
            T::Eq => Bin::Eq,
            T::Ch(b'<') => Bin::Lt,
            T::Le => Bin::Le,
            T::Ch(b'>') => Bin::Gt,
            T::Ge => Bin::Ge,
            T::And => Bin::And,
            T::Or => Bin::Or,
            _ => return None,
        })
    }

    fn sub_exp(&mut self, limit: u32) -> R<(E, Option<Bin>)> {
        self.enter()?;
        let mut v = match Self::unop(&self.lx.t) {
            Some(u) => {
                self.next()?;
                let (mut v, _) = self.sub_exp(8)?;
                let ln = self.ln();
                let r = self.f().prefix(u, &mut v, ln);
                self.ck(r)?;
                v
            }
            None => self.simple_exp()?,
        };
        let mut op_ = Self::binop(&self.lx.t);
        while let Some(b) = op_ {
            if b.prio().0 <= limit {
                break;
            }
            self.next()?;
            let ln = self.ln();
            let r = self.f().infix(b, &mut v, ln);
            self.ck(r)?;
            let (mut v2, nx) = self.sub_exp(b.prio().1)?;
            let ln = self.ln();
            let r = self.f().posfix(b, &mut v, &mut v2, ln);
            self.ck(r)?;
            op_ = nx;
        }
        self.depth -= 1;
        Ok((v, op_))
    }

    fn expr(&mut self) -> R<E> {
        Ok(self.sub_exp(0)?.0)
    }

    fn follow(&self) -> bool {
        matches!(self.lx.t, T::Else | T::Elseif | T::End | T::Until | T::Eos)
    }

    fn block(&mut self) -> R {
        self.enter_block(false);
        self.chunk()?;
        self.leave_block();
        Ok(())
    }

    fn assignment(&mut self, lh: &mut Vec<E>, nvars: i32) -> R {
        let last = *lh.last().unwrap();
        if !matches!(last.k, K::Local | K::Upval | K::Global | K::Indexed) {
            return self.lx.syntax("syntax error");
        }
        if self.test(&T::Ch(b','))? {
            let nv = self.primary_exp()?;
            if nv.k == K::Local {
                let extra = self.f().free;
                let mut conflict = false;
                for e in lh.iter_mut() {
                    if e.k == K::Indexed {
                        if e.info == nv.info {
                            conflict = true;
                            e.info = extra;
                        }
                        if e.aux == nv.info {
                            conflict = true;
                            e.aux = extra;
                        }
                    }
                }
                if conflict {
                    let ln = self.ln();
                    let f = self.f();
                    let fr = f.free;
                    f.abc(o::MOVE, fr, nv.info, 0, ln);
                    let r = f.reserve(1);
                    self.ck(r)?;
                }
            }
            self.limit(nvars, MAX_DEPTH as i32 - self.depth as i32, "variables in assignment")?;
            lh.push(nv);
            self.assignment(lh, nvars + 1)?;
            lh.pop();
        } else {
            self.check_next(&T::Ch(b'='))?;
            let (nexps, mut e) = self.exp_list()?;
            if nexps != nvars {
                self.adjust_assign(nvars, nexps, &mut e)?;
                if nexps > nvars {
                    self.f().free -= nexps - nvars;
                }
            } else {
                self.f().set_one_ret(&mut e);
                let ln = self.ln();
                let r = self.f().store(&last, &mut e, ln);
                return self.ck(r);
            }
        }
        let mut e = E::new(K::NonReloc, self.f().free - 1);
        let last = *lh.last().unwrap();
        let ln = self.ln();
        let r = self.f().store(&last, &mut e, ln);
        self.ck(r)
    }

    fn cond(&mut self) -> R<i32> {
        let mut v = self.expr()?;
        if v.k == K::Nil {
            v.k = K::False;
        }
        let ln = self.ln();
        let r = self.f().go_if_true(&mut v, ln);
        self.ck(r)?;
        Ok(v.f)
    }

    fn break_stat(&mut self) -> R {
        let ln = self.ln();
        let f = self.f();
        let mut upval = false;
        let Some(i) = f.bl.iter().rposition(|b| {
            if !b.loop_ {
                upval |= b.upval;
            }
            b.loop_
        }) else {
            return self.lx.syntax("no loop to break");
        };
        if upval {
            let n = f.bl[i].nact;
            f.abc(o::CLOSE, n, 0, 0, ln);
        }
        let j = f.jump(ln);
        let mut b = f.bl[i].brk;
        f.concat(&mut b, j);
        f.bl[i].brk = b;
        Ok(())
    }

    fn while_stat(&mut self, line: u32) -> R {
        self.next()?;
        let init = self.f().label();
        let exit = self.cond()?;
        self.enter_block(true);
        self.check_next(&T::Do)?;
        self.block()?;
        let ln = self.ln();
        let j = self.f().jump(ln);
        self.f().patch_list(j, init);
        self.check_match(&T::End, &T::While, line)?;
        self.leave_block();
        self.f().patch_here(exit);
        Ok(())
    }

    fn repeat_stat(&mut self, line: u32) -> R {
        let init = self.f().label();
        self.enter_block(true);
        self.enter_block(false);
        self.next()?;
        self.chunk()?;
        self.check_match(&T::Until, &T::Repeat, line)?;
        let exit = self.cond()?;
        if !self.fs.last().unwrap().bl.last().unwrap().upval {
            self.leave_block();
            self.f().patch_list(exit, init);
        } else {
            self.break_stat()?;
            self.f().patch_here(exit);
            self.leave_block();
            let ln = self.ln();
            let j = self.f().jump(ln);
            self.f().patch_list(j, init);
        }
        self.leave_block();
        Ok(())
    }

    fn exp1(&mut self) -> R {
        let mut e = self.expr()?;
        let ln = self.ln();
        let r = self.f().exp2next(&mut e, ln);
        self.ck(r)
    }

    fn for_body(&mut self, base: i32, line: u32, nvars: i32, num: bool) -> R {
        self.adjust_locals(3);
        self.check_next(&T::Do)?;
        let ln = self.ln();
        let prep = if num { self.f().asbx(o::FORPREP, base, NO_JUMP, ln) } else { self.f().jump(ln) };
        self.enter_block(false);
        self.adjust_locals(nvars);
        let r = self.f().reserve(nvars);
        self.ck(r)?;
        self.block()?;
        self.leave_block();
        self.f().patch_here(prep);
        let ln = self.ln();
        let f = self.f();
        let end = if num { f.asbx(o::FORLOOP, base, NO_JUMP, ln) } else { f.abc(o::TFORLOOP, base, 0, nvars, ln) };
        f.fix_line(line);
        let l = if num { end } else { f.jump(ln) };
        f.patch_list(l, prep + 1);
        Ok(())
    }

    fn for_num(&mut self, var: Vec<u8>, line: u32) -> R {
        let base = self.f().free;
        self.new_local(b"(for index)".to_vec(), 0)?;
        self.new_local(b"(for limit)".to_vec(), 1)?;
        self.new_local(b"(for step)".to_vec(), 2)?;
        self.new_local(var, 3)?;
        self.check_next(&T::Ch(b'='))?;
        self.exp1()?;
        self.check_next(&T::Ch(b','))?;
        self.exp1()?;
        if self.test(&T::Ch(b','))? {
            self.exp1()?;
        } else {
            let ln = self.ln();
            let f = self.f();
            let (fr, k) = (f.free, f.num_k(1.0));
            f.abx(o::LOADK, fr, k, ln);
            let r = f.reserve(1);
            self.ck(r)?;
        }
        self.for_body(base, line, 1, true)
    }

    fn for_list(&mut self, first: Vec<u8>) -> R {
        let base = self.f().free;
        self.new_local(b"(for generator)".to_vec(), 0)?;
        self.new_local(b"(for state)".to_vec(), 1)?;
        self.new_local(b"(for control)".to_vec(), 2)?;
        self.new_local(first, 3)?;
        let mut n = 4;
        while self.test(&T::Ch(b','))? {
            let nm = self.name()?;
            self.new_local(nm, n)?;
            n += 1;
        }
        self.check_next(&T::In)?;
        let line = self.lx.line;
        let (ne, mut e) = self.exp_list()?;
        self.adjust_assign(3, ne, &mut e)?;
        let r = self.f().check_stack(3);
        self.ck(r)?;
        self.for_body(base, line, n - 3, false)
    }

    fn for_stat(&mut self, line: u32) -> R {
        self.enter_block(true);
        self.next()?;
        let var = self.name()?;
        match self.lx.t {
            T::Ch(b'=') => self.for_num(var, line)?,
            T::Ch(b',') | T::In => self.for_list(var)?,
            _ => return self.lx.syntax("'=' or 'in' expected"),
        }
        self.check_match(&T::End, &T::For, line)?;
        self.leave_block();
        Ok(())
    }

    fn test_then(&mut self) -> R<i32> {
        self.next()?;
        let exit = self.cond()?;
        self.check_next(&T::Then)?;
        self.block()?;
        Ok(exit)
    }

    fn if_stat(&mut self, line: u32) -> R {
        let mut esc = NO_JUMP;
        let mut fl = self.test_then()?;
        while self.is(&T::Elseif) {
            let ln = self.ln();
            let j = self.f().jump(ln);
            self.f().concat(&mut esc, j);
            self.f().patch_here(fl);
            fl = self.test_then()?;
        }
        if self.is(&T::Else) {
            let ln = self.ln();
            let j = self.f().jump(ln);
            self.f().concat(&mut esc, j);
            self.f().patch_here(fl);
            self.next()?;
            self.block()?;
        } else {
            self.f().concat(&mut esc, fl);
        }
        self.f().patch_here(esc);
        self.check_match(&T::End, &T::If, line)
    }

    fn local_func(&mut self) -> R {
        let n = self.name()?;
        self.new_local(n, 0)?;
        let v = E::new(K::Local, self.f().free);
        let r = self.f().reserve(1);
        self.ck(r)?;
        self.adjust_locals(1);
        let l = self.lx.line;
        let mut b = self.body(false, l)?;
        let ln = self.ln();
        let r = self.f().store(&v, &mut b, ln);
        self.ck(r)?;
        let f = self.f();
        let (k, pc) = (f.nact - 1, f.pc() as u64);
        f.loc(k).start = pc;
        Ok(())
    }

    fn local_stat(&mut self) -> R {
        let mut n = 0;
        loop {
            let nm = self.name()?;
            self.new_local(nm, n)?;
            n += 1;
            if !self.test(&T::Ch(b','))? {
                break;
            }
        }
        let (ne, mut e) = if self.test(&T::Ch(b'='))? { self.exp_list()? } else { (0, E::new(K::Void, 0)) };
        self.adjust_assign(n, ne, &mut e)?;
        self.adjust_locals(n);
        Ok(())
    }

    fn func_stat(&mut self, line: u32) -> R {
        self.next()?;
        let mut v = self.single_var()?;
        let mut needself = false;
        while self.is(&T::Ch(b'.')) {
            self.field(&mut v)?;
        }
        if self.is(&T::Ch(b':')) {
            needself = true;
            self.field(&mut v)?;
        }
        let mut b = self.body(needself, line)?;
        let ln = self.ln();
        let r = self.f().store(&v, &mut b, ln);
        self.ck(r)?;
        self.f().fix_line(line);
        Ok(())
    }

    fn expr_stat(&mut self) -> R {
        let v = self.primary_exp()?;
        if v.k == K::Call {
            let i = self.f().code_at(&v);
            *i = (*i & !(0x1ff << 14)) | 1 << 14;
            return Ok(());
        }
        let mut lh = vec![v];
        self.assignment(&mut lh, 1)
    }

    fn ret_stat(&mut self) -> R {
        self.next()?;
        let ln;
        let (first, nret) = if self.follow() || self.is(&T::Ch(b';')) {
            ln = self.ln();
            (0, 0)
        } else {
            let (n, mut e) = self.exp_list()?;
            ln = self.ln();
            if e.multret() {
                let r = self.f().set_returns(&e, MULTRET);
                self.ck(r)?;
                let f = self.f();
                if e.k == K::Call && n == 1 {
                    let i = f.code_at(&e);
                    *i = (*i & !0x3f) | o::TAILCALL as u32;
                }
                (f.nact, MULTRET)
            } else if n == 1 {
                let r = self.f().exp2any(&mut e, ln);
                (self.ck(r)?, 1)
            } else {
                let r = self.f().exp2next(&mut e, ln);
                self.ck(r)?;
                (self.f().nact, n)
            }
        };
        self.f().ret(first, nret, ln);
        Ok(())
    }

    fn statement(&mut self) -> R<bool> {
        let line = self.lx.line;
        match self.lx.t {
            T::If => self.if_stat(line)?,
            T::While => self.while_stat(line)?,
            T::Do => {
                self.next()?;
                self.block()?;
                self.check_match(&T::End, &T::Do, line)?;
            }
            T::For => self.for_stat(line)?,
            T::Repeat => self.repeat_stat(line)?,
            T::Function => self.func_stat(line)?,
            T::Local => {
                self.next()?;
                if self.test(&T::Function)? {
                    self.local_func()?;
                } else {
                    self.local_stat()?;
                }
            }
            T::Return => {
                self.ret_stat()?;
                return Ok(true);
            }
            T::Break => {
                self.next()?;
                self.break_stat()?;
                return Ok(true);
            }
            _ => self.expr_stat()?,
        }
        Ok(false)
    }

    fn chunk(&mut self) -> R {
        self.enter()?;
        let mut last = false;
        while !last && !self.follow() {
            last = self.statement()?;
            self.test(&T::Ch(b';'))?;
            let f = self.f();
            f.free = f.nact;
        }
        self.depth -= 1;
        Ok(())
    }
}

pub fn parse(src: &[u8], name: &str) -> Res<Proto> {
    parse_ver(src, name, 5)
}

pub fn parse_ver(src: &[u8], name: &str, ver: u8) -> Res<Proto> {
    let mut p = P { lx: Lex::new(src, name), fs: Vec::new(), depth: 0, ver };
    p.open_func();
    p.f().p.vararg = 2;
    p.next()?;
    p.chunk()?;
    if !p.is(&T::Eos) {
        return p.lx.syntax("'<eof>' expected");
    }
    Ok(p.close_func())
}

pub fn compile(src: &[u8], name: &str, h: Header, ver: u8) -> Res<Vec<u8>> {
    write(&Chunk { h, main: parse_ver(src, name, ver)? })
}

pub fn check(src: &[u8], name: &str) -> Result<(), Error> {
    parse(src, name).map(drop)
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::luac::{abc, abx, asbx, Const};

    #[test]
    fn matches_luac_listing() {
        let p = parse(b"local t = {} for i=1,3 do t[i]=i*2 end print(#t, t[3])", "@t.lua").unwrap();
        let want = [
            abc(10, 0, 0, 0),
            abx(1, 1, 0),
            abx(1, 2, 1),
            abx(1, 3, 0),
            asbx(32, 1, 2),
            abc(14, 5, 4, 258),
            abc(9, 0, 4, 5),
            asbx(31, 1, -3),
            abx(5, 1, 3),
            abc(20, 2, 0, 0),
            abc(6, 3, 0, 257),
            abc(28, 1, 3, 1),
            abc(30, 0, 1, 0),
        ];
        assert_eq!(p.code, want);
        assert_eq!(p.consts, [Const::Num(1.0), Const::Num(3.0), Const::Num(2.0), Const::Str(b"print".to_vec())]);
        assert_eq!((p.stack, p.vararg, p.source.as_deref()), (6, 2, Some(&b"@t.lua"[..])));
        let names: Vec<&[u8]> = p.locals.iter().map(|l| &l.name[..]).collect();
        assert_eq!(names, [&b"t"[..], b"(for index)", b"(for limit)", b"(for step)", b"i"]);
        assert_eq!((p.locals[4].start, p.locals[4].end), (5, 7));
    }

    #[test]
    fn functions_and_upvalues() {
        let p = parse(b"local a = 1\nfunction f(x, ...) a = x return ... end\nlocal function g() return g end", "=t").unwrap();
        let (f, g) = (&p.protos[0], &p.protos[1]);
        assert_eq!((f.params, f.vararg, f.nups, f.line, f.last), (1, 3, 1, 2, 2));
        assert_eq!(f.ups, [b"a".to_vec()]);
        assert_eq!((g.nups, g.ups[0].as_slice()), (1, &b"g"[..]));
        assert!(f.source.is_none());
        let v = parse(b"function h(...) return arg end", "=t").unwrap();
        assert_eq!(v.protos[0].vararg, 7);
        assert!(compile(b"return 1", "=x", Header::default(), 5).unwrap().starts_with(b"\x1bLua\x51"));
    }

    #[test]
    fn errors() {
        for (s, m) in [
            ("x = = 1", "t:1: unexpected symbol near '='"),
            ("break", "t:1: no loop to break near '<eof>'"),
            ("for i = 1 do end", "t:1: ',' expected near 'do'"),
            ("if x then\n\nfoo()", "t:3: 'end' expected (to close 'if' at line 1) near '<eof>'"),
            ("return ...", ""),
            ("function f() return ... end", "t:1: cannot use '...' outside a vararg function near '...'"),
            ("f()\n(g)()", "t:2: ambiguous syntax (function call x new statement) near '('"),
            ("x = 'abc", "t:1: unfinished string near '<eof>'"),
            ("local t = {1, 2", "t:1: '}' expected near '<eof>'"),
        ] {
            let r = check(s.as_bytes(), "=t");
            if m.is_empty() {
                assert!(r.is_ok(), "{s}");
            } else {
                assert_eq!(r.unwrap_err().to_string(), m, "{s}");
            }
        }
        let deep = format!("x = {}1{}", "(".repeat(300), ")".repeat(300));
        assert!(check(deep.as_bytes(), "=t").is_err());
    }
}

#[cfg(test)]
mod real {
    use super::*;
    use crate::luac::{disasm, parse as load};

    fn first_diff(a: &Proto, b: &Proto, path: &str) -> Option<String> {
        if a.code != b.code || a.consts != b.consts || a.lines != b.lines || a.locals != b.locals || (a.nups, a.params, a.vararg, a.stack) != (b.nups, b.params, b.vararg, b.stack) || a.ups != b.ups || (a.line, a.last) != (b.line, b.last) {
            return Some(format!("{path} line {}:\nmine:\n{}\nref:\n{}\nmine k {:?}\nref k {:?}\n{:?} {:?}", a.line, disasm(a), disasm(b), a.consts, b.consts, (a.nups, a.params, a.vararg, a.stack, a.line, a.last), (b.nups, b.params, b.vararg, b.stack, b.line, b.last)));
        }
        if a.protos.len() != b.protos.len() {
            return Some(format!("{path}: proto count"));
        }
        a.protos.iter().zip(&b.protos).enumerate().find_map(|(i, (x, y))| first_diff(x, y, &format!("{path}/{i}")))
    }

    #[test]
    #[ignore]
    fn reference_luac() {
        let Ok(g) = std::env::var("AC_GOLDEN") else { return };
        let mut v: Vec<_> = std::fs::read_dir(std::path::Path::new(&g).join("lua")).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "lua")).collect();
        v.sort();
        let (mut same, mut fails) = (0, Vec::new());
        for p in &v {
            let src = std::fs::read(p).unwrap();
            let name = format!("@{}", p.file_name().unwrap().to_string_lossy());
            let want = std::fs::read(p.with_extension("luac")).unwrap();
            match compile(&src, &name, load(&want).unwrap().h, 5) {
                Ok(got) if got == want => same += 1,
                Ok(got) => {
                    let (a, b) = (load(&got).unwrap().main, load(&want).unwrap().main);
                    fails.push(first_diff(&a, &b, &name).unwrap_or_else(|| format!("{name}: header differs")));
                }
                Err(e) => fails.push(format!("{name}: {e}")),
            }
        }
        println!("identical {same}/{}", v.len());
        for f in fails.iter().take(3) {
            println!("{f}");
        }
        assert!(fails.is_empty(), "{} files differ", fails.len());
    }
}

