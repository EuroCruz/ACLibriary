use super::expr::{kexp, nil, Decl, Expr, D, E};
use super::stmt::Target;
use super::R;
use crate::luac::Const;
use ac_core::bad;
use std::rc::Rc;

pub struct Regs {
    pub n: i32,
    decls: Vec<Vec<Option<D>>>,
    vals: Vec<Vec<Option<E>>>,
    upd: Vec<Vec<i32>>,
    ks: Rc<[Const]>,
}

impl Regs {
    pub fn new(n: i32, len: i32, decls: &[D], ks: Rc<[Const]>) -> R<Regs> {
        let (nr, nl) = (n as usize, len as usize + 1);
        let mut v: Vec<Vec<Option<D>>> = vec![vec![None; nl]; nr];
        for d in decls {
            let b = d.begin as usize;
            let pre = usize::try_from(d.reg.get()).ok().filter(|&r| r < nr);
            let Some(r) = pre.or_else(|| (0..nr).find(|&r| b >= nl || v[r][b].is_none())) else { return bad("no free register for a local") };
            d.reg.set(r as i32);
            for l in (b..nl).take_while(|&l| l as i32 <= d.end) {
                v[r][l] = Some(d.clone());
            }
        }
        let mut vals = vec![vec![None; nl]; nr];
        vals.iter_mut().for_each(|x| x[0] = Some(nil()));
        Ok(Regs { n, decls: v, vals, upd: vec![vec![0; nl]; nr], ks })
    }

    pub fn decl(&self, r: i32, l: i32) -> Option<D> {
        self.decls.get(r as usize)?.get(l as usize)?.clone()
    }

    pub fn local(&self, r: i32, l: i32) -> bool {
        r >= 0 && self.decl(r, l).is_some()
    }

    pub fn assignable(&self, r: i32, l: i32) -> bool {
        r >= 0 && self.decl(r, l).is_some_and(|d| !d.fl.get())
    }

    fn new_local(&self, r: i32, l: i32) -> Option<D> {
        self.decl(r, l).filter(|d| d.begin == l && !d.fl.get())
    }

    pub fn new_locals(&self, l: i32) -> Vec<D> {
        (0..self.n).filter_map(|r| self.new_local(r, l)).collect()
    }

    pub fn start(&mut self, l: i32) {
        let l = l as usize;
        for r in 0..self.n as usize {
            self.vals[r][l] = self.vals[r][l - 1].clone();
            self.upd[r][l] = self.upd[r][l - 1];
        }
    }

    pub fn get(&self, r: i32, l: i32) -> R<E> {
        match self.decl(r, l - 1) {
            Some(d) if r >= 0 => Ok(Rc::new(Expr::Local(d))),
            _ => self.val(r, l),
        }
    }

    pub fn getk(&self, r: i32, l: i32) -> R<E> {
        if r >= 256 { kexp(&self.ks, r - 256) } else { self.get(r, l) }
    }

    pub fn val(&self, r: i32, l: i32) -> R<E> {
        match self.vals.get(r as usize).and_then(|v| v.get((l - 1) as usize)) {
            Some(Some(e)) => Ok(e.clone()),
            _ => bad("register has no value"),
        }
    }

    pub fn upd(&self, r: i32, l: i32) -> i32 {
        self.upd.get(r as usize).and_then(|v| v.get(l as usize)).copied().unwrap_or(0)
    }

    pub fn set(&mut self, r: i32, l: i32, e: E) -> R {
        match self.vals.get_mut(r as usize).and_then(|v| v.get_mut(l as usize)) {
            Some(s) => *s = Some(e),
            None => return bad("register out of range"),
        }
        self.upd[r as usize][l as usize] = l;
        Ok(())
    }

    pub fn target(&self, r: i32, l: i32) -> R<Target> {
        match self.decl(r, l) {
            Some(d) => Ok(Target::Var(d)),
            None => bad("no declaration in the register"),
        }
    }

    pub fn loop_var(&mut self, r: i32, begin: i32, end: i32, explicit: bool) -> R {
        let d = match self.decl(r, begin) {
            Some(d) => d,
            None => {
                if r < 0 || r >= self.n {
                    return bad("loop register out of range");
                }
                let d = Decl::new(&if explicit { format!("_FORV_{r}_") } else { "_FOR_".into() }, begin, end);
                d.reg.set(r);
                for l in begin.max(0)..=end {
                    if let Some(s) = self.decls[r as usize].get_mut(l as usize) {
                        *s = Some(d.clone());
                    }
                }
                d
            }
        };
        if explicit { d.fx.set(true) } else { d.fl.set(true) }
        Ok(())
    }
}
