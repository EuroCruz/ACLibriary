use crate::luac::{self, o, Proto};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Var {
    pub name: String,
    pub reg: i32,
    pub start: i32,
    pub end: i32,
}

#[derive(Clone, Copy)]
struct I {
    op: u8,
    a: i32,
    b: i32,
    c: i32,
    sbx: i32,
}

struct F<'p> {
    p: &'p Proto,
    i: Vec<I>,
    n: usize,
    pseudo: Vec<bool>,
    top: Vec<i32>,
}

fn rk(x: i32, v: &mut Vec<i32>) {
    if x < 256 {
        v.push(x);
    }
}

impl<'p> F<'p> {
    fn new(p: &'p Proto) -> F<'p> {
        let i: Vec<I> = p.code.iter().map(|&w| I { op: luac::op(w), a: luac::a(w) as i32, b: luac::b(w) as i32, c: luac::c(w) as i32, sbx: luac::sbx(w) }).collect();
        let n = i.len();
        let mut pseudo = vec![false; n];
        let mut top = vec![-1; n];
        let mut last = -1;
        for pc in 0..n {
            if pseudo[pc] {
                continue;
            }
            let x = i[pc];
            top[pc] = last;
            match x.op {
                o::CLOSURE => {
                    let k = p.protos.get(luac::bx(p.code[pc]) as usize).map_or(0, |c| c.nups as usize);
                    (pc + 1..(pc + 1 + k).min(n)).for_each(|q| pseudo[q] = true);
                }
                o::SETLIST if x.c == 0 && pc + 1 < n => pseudo[pc + 1] = true,
                _ => {}
            }
            match x.op {
                o::CALL if x.c == 0 => last = x.a,
                o::VARARG if x.b == 0 => last = x.a,
                o::CALL | o::TAILCALL | o::RETURN | o::SETLIST => {}
                _ => {}
            }
        }
        F { p, i, n, pseudo, top }
    }

    fn succ(&self, pc: usize) -> Vec<usize> {
        let x = self.i[pc];
        let t = |d: i32| (pc as i32 + 1 + d) as usize;
        let v = match x.op {
            o::JMP => vec![t(x.sbx)],
            o::EQ | o::LT | o::LE | o::TEST | o::TESTSET | o::TFORLOOP => vec![pc + 1, pc + 2],
            o::LOADBOOL if x.c != 0 => vec![pc + 2],
            o::FORLOOP => vec![t(x.sbx), pc + 1],
            o::FORPREP => vec![t(x.sbx)],
            o::RETURN | o::TAILCALL => vec![],
            o::CLOSURE => {
                let k = self.p.protos.get(luac::bx(self.p.code[pc]) as usize).map_or(0, |c| c.nups as usize);
                vec![pc + 1 + k]
            }
            o::SETLIST if x.c == 0 => vec![pc + 2],
            _ => vec![pc + 1],
        };
        v.into_iter().filter(|&q| q < self.n).collect()
    }

    fn upto(&self, pc: usize, a: i32) -> i32 {
        self.top[pc].max(a)
    }

    fn reads(&self, pc: usize) -> Vec<i32> {
        let x = self.i[pc];
        let mut v = Vec::new();
        match x.op {
            o::MOVE | o::UNM | o::NOT | o::LEN | o::TESTSET => v.push(x.b),
            o::GETTABLE | o::SELF => {
                v.push(x.b);
                rk(x.c, &mut v);
            }
            o::SETGLOBAL | o::SETUPVAL | o::TEST => v.push(x.a),
            o::SETTABLE => {
                v.push(x.a);
                rk(x.b, &mut v);
                rk(x.c, &mut v);
            }
            o::ADD..=o::POW | o::EQ | o::LT | o::LE => {
                rk(x.b, &mut v);
                rk(x.c, &mut v);
            }
            o::CONCAT => v.extend(x.b..=x.c),
            o::CALL | o::TAILCALL => v.extend(x.a..if x.b == 0 { self.upto(pc, x.a) + 1 } else { x.a + x.b }),
            o::RETURN => v.extend(x.a..if x.b == 0 { self.upto(pc, x.a) + 1 } else { x.a + x.b - 1 }),
            o::FORLOOP | o::TFORLOOP => v.extend([x.a, x.a + 1, x.a + 2]),
            o::FORPREP => v.extend([x.a, x.a + 2]),
            o::SETLIST => {
                v.push(x.a);
                v.extend(x.a + 1..=if x.b == 0 { self.upto(pc, x.a + 1) } else { x.a + x.b });
            }
            o::CLOSURE => {
                let k = self.p.protos.get(luac::bx(self.p.code[pc]) as usize).map_or(0, |c| c.nups as usize);
                for q in pc + 1..(pc + 1 + k).min(self.n) {
                    if self.i[q].op == o::MOVE && self.i[q].b != x.a {
                        v.push(self.i[q].b);
                    }
                }
            }
            _ => {}
        }
        v
    }

    fn writes(&self, pc: usize) -> Vec<i32> {
        let x = self.i[pc];
        match x.op {
            o::MOVE | o::LOADK | o::LOADBOOL | o::GETUPVAL | o::GETGLOBAL | o::GETTABLE | o::NEWTABLE | o::ADD..=o::CONCAT | o::CLOSURE | o::TESTSET | o::FORPREP => vec![x.a],
            o::LOADNIL => (x.a..=x.b).collect(),
            o::SELF => vec![x.a, x.a + 1],
            o::CALL => if x.c == 0 { vec![x.a] } else { (x.a..x.a + x.c - 1).collect() },
            o::VARARG => if x.b == 0 { vec![x.a] } else { (x.a..x.a + x.b - 1).collect() },
            o::FORLOOP => vec![x.a, x.a + 3],
            o::TFORLOOP => (x.a + 2..=x.a + 2 + x.c).collect(),
            _ => Vec::new(),
        }
    }

    fn uses(&self, from: &[usize], r: i32, seen: &mut [bool]) -> Vec<usize> {
        seen.iter_mut().for_each(|s| *s = false);
        let (mut st, mut out) = (from.to_vec(), Vec::new());
        while let Some(q) = st.pop() {
            if q >= self.n || seen[q] {
                continue;
            }
            seen[q] = true;
            if self.reads(q).contains(&r) {
                out.push(q);
            }
            if self.i[q].op == o::TESTSET && self.i[q].a == r {
                st.push(q + 2);
                continue;
            }
            if self.writes(q).contains(&r) {
                continue;
            }
            st.extend(self.succ(q));
        }
        out.sort();
        out
    }
}

struct Def {
    pc: usize,
    r: i32,
    uses: Vec<usize>,
    cons: Vec<usize>,
    fills: Vec<usize>,
    local: bool,
    lp: bool,
}

pub fn infer(p: &Proto, depth: usize, flip: &[(usize, i32)]) -> (Vec<Var>, Vec<(usize, i32)>) {
    let f = F::new(p);
    let n = f.n;
    let fixed = p.params as i32 + (p.vararg & 1) as i32;
    let mut seen = vec![false; n];
    let mut defs: Vec<Def> = Vec::new();
    for pc in 0..n {
        if f.pseudo[pc] {
            continue;
        }
        let x = f.i[pc];
        for r in f.writes(pc) {
            let from = if x.op == o::TESTSET { vec![pc + 1] } else { f.succ(pc) };
            let uses = f.uses(&from, r, &mut seen);
            let fill = |u: usize| x.op == o::NEWTABLE && matches!(f.i[u].op, o::SETTABLE | o::SETLIST) && f.i[u].a == r && !(f.i[u].op == o::SETTABLE && (f.i[u].b == r || f.i[u].c == r));
            let cons: Vec<usize> = uses.iter().copied().filter(|&u| !(f.i[u].op == o::TEST && f.i[u].a == r) && !fill(u)).collect();
            let fills: Vec<usize> = uses.iter().copied().filter(|&u| fill(u)).collect();
            let lp = matches!(x.op, o::FORLOOP | o::TFORLOOP | o::FORPREP);
            defs.push(Def { pc, r, uses, cons, fills, local: false, lp });
        }
    }
    let loops: Vec<(usize, i32)> = (0..n).filter(|&pc| !f.pseudo[pc]).filter_map(|pc| match f.i[pc].op {
        o::FORPREP => Some((pc, f.i[pc].a)),
        o::TFORLOOP => Some((pc, f.i[pc].a)),
        _ => None,
    }).collect();
    let init = |d: &Def| !d.cons.is_empty() && d.cons.iter().all(|&u| matches!(f.i[u].op, o::FORPREP | o::FORLOOP | o::TFORLOOP) && (f.i[u].a..f.i[u].a + 3).contains(&d.r));
    let mut inits = vec![false; defs.len()];
    for (k, d) in defs.iter().enumerate() {
        inits[k] = !d.lp && init(d);
    }
    let mut first = vec![n; p.consts.len()];
    for pc in 0..n {
        if f.pseudo[pc] {
            continue;
        }
        let x = f.i[pc];
        let mut ks: Vec<i32> = Vec::new();
        match x.op {
            o::LOADK | o::GETGLOBAL | o::SETGLOBAL => ks.push(luac::bx(p.code[pc]) as i32),
            o::GETTABLE | o::SELF => ks.push(x.c - 256),
            o::SETTABLE | o::ADD..=o::POW | o::EQ | o::LT | o::LE => ks.extend([x.b - 256, x.c - 256]),
            _ => {}
        }
        for k in ks {
            if let Some(s) = usize::try_from(k).ok().and_then(|k| first.get_mut(k)) {
                *s = (*s).min(pc);
            }
        }
    }
    let fills: Vec<usize> = defs.iter().flat_map(|d| d.fills.iter().copied()).collect();
    let store = |q: usize| !f.pseudo[q] && (matches!(f.i[q].op, o::SETTABLE | o::SETGLOBAL | o::SETUPVAL) || (f.i[q].op == o::MOVE && f.i[q].a < f.i[q].b));
    let selfcap = |pc: usize| f.i[pc].op == o::CLOSURE && (pc + 1..n).take_while(|&q| f.pseudo[q]).any(|q| f.i[q].op == o::MOVE && f.i[q].b == f.i[pc].a);
    let next_write = |after: usize| (after + 1..n).filter(|&q| !f.pseudo[q]).find(|&q| !f.writes(q).is_empty());
    let ph0 = (0..n).find(|&q| !f.pseudo[q]).and_then(|q| f.writes(q).into_iter().min()).unwrap_or(fixed);
    let ph = flip.iter().filter(|x| x.0 == usize::MAX || x.0 == usize::MAX - 2).map(|x| x.1 + 1).fold(ph0, i32::max);
    let pa = flip.iter().filter(|x| x.0 == usize::MAX - 2).map(|x| x.1 + 1).fold(ph0, i32::max);
    let elast: Vec<usize> = (0..p.stack as i32).map(|r| f.uses(&[0], r, &mut seen).into_iter().max().unwrap_or(0)).collect();
    let ifs = |a: usize, b: usize, r: i32, lim: usize| (a + 1..b).any(|q| !f.pseudo[q] && f.i[q].op == o::JMP && q > 0 && matches!(f.i[q - 1].op, o::EQ | o::LT | o::LE | o::TEST | o::TESTSET) && !f.reads(q - 1).contains(&r) && !f.writes(q - 1).contains(&r) && (q as i32 + 1 + f.i[q].sbx) as usize > lim);
    let cls: Vec<bool> = defs
        .iter()
        .enumerate()
        .map(|(k, d)| {
            if d.lp || inits[k] {
                return false;
            }
            if d.r >= fixed && d.r < pa {
                return true;
            }
            if d.r >= fixed && f.i[d.pc].op == o::LOADNIL && d.uses.iter().any(|&q| f.i[q].op == o::TEST && f.i[q].a == d.r) {
                return true;
            }
            if d.r >= fixed && elast.get(d.r as usize).is_some_and(|&l| l > d.pc) {
                return true;
            }
            if d.r >= fixed && f.writes(d.pc) == [d.r] && !matches!(f.i[d.pc].op, o::CALL | o::TAILCALL | o::TFORLOOP | o::FORLOOP | o::FORPREP | o::VARARG | o::LOADNIL) && !f.reads(d.pc).contains(&d.r) && f.reads(d.pc).iter().any(|&x| x > d.r && x < 256) {
                return true;
            }
            if d.r >= fixed && d.cons.is_empty() && d.uses.len() == 1 && d.uses[0] > d.pc && f.i[d.uses[0]].op == o::TEST && f.writes(d.pc).len() == 1 && (d.pc + 1..d.uses[0]).all(|q| f.pseudo[q]) {
                return false;
            }
            if d.r >= fixed && d.cons.is_empty() && d.uses.len() >= 2 && d.uses[0] == d.pc + 1 && d.uses.iter().all(|&q| f.i[q].op == o::TEST && f.i[q].a == d.r) && d.uses[1..].iter().all(|q| defs.iter().any(|e| e.r == d.r && e.pc != d.pc && e.uses.contains(q))) {
                return false;
            }
            let m = f.i[d.pc];
            if m.op == o::MOVE && m.b < m.a && d.r >= fixed && !d.cons.is_empty() && d.cons.iter().all(|&u| f.i[u].op == o::SETTABLE) && (d.pc + 1..d.cons[0]).any(|q| f.writes(q).contains(&m.b)) {
                return false;
            }
            let fb = |v: i32| { let e = (v >> 3) & 31; if e == 0 { v } else { ((v & 7) + 8) << (e - 1) } };
            if f.i[d.pc].op == o::NEWTABLE && d.fills.iter().filter(|&&u| f.i[u].op == o::SETTABLE).count() as i32 > fb(f.i[d.pc].c) {
                return true;
            }
            if d.r < fixed || d.cons.len() != 1 || selfcap(d.pc) {
                return true;
            }
            let u = d.cons[0];
            let ux = f.i[u];
            let up = !matches!(ux.op, o::CALL | o::TAILCALL | o::FORLOOP | o::FORPREP | o::TFORLOOP) && f.writes(u).iter().any(|&w| w > d.r) && !f.writes(u).contains(&d.r);
            let nilx = f.i[d.pc].op == o::LOADNIL && (matches!(ux.op, o::SETTABLE) && ux.a == d.r || matches!(ux.op, o::GETTABLE | o::SELF) && ux.b == d.r || matches!(ux.op, o::CALL) && ux.a == d.r);
            if u <= d.pc || up || nilx || ux.op == o::CLOSURE {
                return true;
            }
            let mut grp: Vec<usize> = defs.iter().filter(|e| e.r == d.r && e.cons.contains(&u)).map(|e| e.pc).collect();
            grp.sort();
            if grp.len() == 1 && d.uses.iter().any(|&q| q > d.pc && q < u && f.i[q].op == o::TEST && f.i[q].a == d.r) {
                return true;
            }
            if grp.len() >= 2 {
                let (lo, hi) = (grp[0], grp[grp.len() - 1]);
                let tg = |q: usize| (q as i32 + 1 + f.i[q].sbx) as usize;
                if grp.windows(2).any(|w| ifs(w[0], w[1], d.r, hi)) {
                    return true;
                }
                let cj = (0..lo).rev().find(|&q| !f.pseudo[q] && f.i[q].op == o::JMP && q > 0 && matches!(f.i[q - 1].op, o::EQ | o::LT | o::LE | o::TEST | o::TESTSET) && tg(q) > lo && tg(q) <= hi);
                if let Some(q) = cj {
                    if (q + 1..lo).any(|x| !f.pseudo[x] && (f.writes(x).iter().any(|&w| w < d.r) || (f.i[x].op == o::CALL && f.i[x].c == 1) || matches!(f.i[x].op, o::SETTABLE | o::SETGLOBAL | o::SETUPVAL))) {
                        return true;
                    }
                }
            }
            let hi = |x: i32| x < 256 && x > d.r;
            if (ux.op == o::SETTABLE && (ux.c == d.r || ux.b == d.r) && hi(ux.a)) || (matches!(ux.op, o::ADD..=o::POW | o::EQ | o::GETTABLE) && ux.c == d.r && hi(ux.b)) {
                return true;
            }
            let rs = (0..u).rev().take_while(|&q| store(q)).last().unwrap_or(u);
            if (d.pc + 1..rs).filter(|&q| !f.pseudo[q]).any(|q| { let x = f.i[q]; (x.op == o::CALL && x.c == 1) || (matches!(x.op, o::SETTABLE | o::SETGLOBAL | o::SETUPVAL) && !fills.contains(&q) && f.reads(q).iter().all(|&w| w < d.r)) }) {
                return true;
            }
            let re = (u..n).take_while(|&q| store(q)).last().unwrap_or(u);
            if defs.iter().any(|e| e.pc > d.pc && e.pc < u && e.r > d.r && !e.lp && ((e.cons.is_empty() && e.uses.len() != 1) || e.cons.iter().any(|&c| c > re))) {
                return true;
            }
            let kk = match ux.op {
                o::SETTABLE if ux.c == d.r && ux.b >= 256 => Some((ux.b - 256) as usize),
                o::SETGLOBAL if ux.a == d.r => Some(luac::bx(p.code[u]) as usize),
                _ => None,
            };
            let lone = !(u > 0 && store(u - 1)) && !(u + 1 < n && store(u + 1));
            if let (Some(kk), true) = (kk.filter(|&k| first.get(k) == Some(&u)), lone) {
                let mut lo = d.pc;
                while lo > 0 && !f.pseudo[lo - 1] && !f.writes(lo - 1).is_empty() && f.writes(lo - 1).iter().all(|&w| w >= d.r) {
                    lo -= 1;
                }
                if (0..kk).any(|k| (lo..=d.pc).contains(&first[k])) {
                    return true;
                }
            }
            if let (Some(q), true) = (next_write(u), f.writes(u).iter().all(|&w| w > d.r)) {
                if f.writes(q).iter().all(|&w| w > d.r) && !matches!(f.i[q].op, o::FORLOOP | o::TFORLOOP) {
                    return true;
                }
            }
            false
        })
        .collect();
    for (k, d) in defs.iter_mut().enumerate() {
        d.local = cls[k] ^ flip.contains(&(d.pc, d.r));
    }
    let mut vars: Vec<Var> = Vec::new();
    let mut cnt = 0;
    let stack = p.stack as i32;
    for r in 0..fixed {
        vars.push(Var { name: if (p.vararg & 1) != 0 && r == p.params as i32 { "arg".into() } else { format!("p{depth}_{r}") }, reg: r, start: 0, end: n as i32 - 1 });
    }
    let block_end = |start: usize, reg: i32| -> usize {
        let mut e = n.saturating_sub(1);
        for pc in 0..n {
            if f.pseudo[pc] {
                continue;
            }
            let x = f.i[pc];
            let tgt = |q: usize| (q as i32 + 1 + f.i[q].sbx) as usize;
            match x.op {
                o::JMP if x.sbx < 0 => {
                    let s = tgt(pc);
                    if (pc + 1..n).any(|q| !f.pseudo[q] && f.i[q].op == o::JMP && f.i[q].sbx < 0 && tgt(q) == s) {
                        continue;
                    }
                    let cond = pc > 0 && matches!(f.i[pc - 1].op, o::EQ | o::LT | o::LE | o::TEST | o::TESTSET);
                    let tfor = pc > 0 && f.i[pc - 1].op == o::TFORLOOP;
                    let b = if cond { pc + 1 } else if tfor || (pc > 0 && f.i[pc - 1].op == o::CLOSE) { pc - 1 } else { pc };
                    if s < start && start <= pc {
                        e = e.min(b);
                    }
                }
                o::FORLOOP => {
                    let s = tgt(pc);
                    if s <= start && start <= pc {
                        e = e.min(pc);
                    }
                }
                o::CLOSE if start <= pc && !(pc + 1 < n && f.i[pc + 1].op == o::JMP && (f.i[pc + 1].sbx > 0 || (pc + 2 < n && f.i[pc + 2].op == o::CLOSE && f.i[pc + 2].a == x.a))) && x.a <= reg => e = e.min(pc),
                o::JMP if x.sbx > 0 && pc > 0 && matches!(f.i[pc - 1].op, o::EQ | o::LT | o::LE | o::TEST | o::TESTSET) => {
                    let t = tgt(pc);
                    if pc < start && start < t {
                        let b = if t >= 1 && f.i[t - 1].op == o::JMP && f.i[t - 1].sbx > 0 && t - 1 > pc { t - 1 } else { t };
                        if start <= b {
                            e = e.min(b);
                        }
                    }
                }
                o::JMP if x.sbx > 0 => {
                    let t = tgt(pc);
                    let prev = (0..pc).rev().find(|&q| !f.pseudo[q]);
                    let is_else = || (0..pc).any(|q| !f.pseudo[q] && f.i[q].op == o::JMP && f.i[q].sbx > 0 && q > 0 && matches!(f.i[q - 1].op, o::EQ | o::LT | o::LE | o::TEST | o::TESTSET) && tgt(q) == pc + 1);
                    if pc < start && start < t && prev.is_some_and(|q| f.i[q].op != o::JMP) && is_else() {
                        e = e.min(t);
                    }
                    if pc == start && is_else() {
                        e = e.min(pc);
                    }
                }
                _ => {}
            }
        }
        e
    };
    for r in fixed..stack {
        let inloop = |pc: usize| loops.iter().any(|&(lp, a)| {
            let x = f.i[lp];
            let (s, e, k) = if x.op == o::FORPREP { (lp, (lp as i32 + 1 + x.sbx) as usize + 1, 4) } else { ((0..lp).find(|&q| f.i[q].op == o::JMP && (q as i32 + 1 + f.i[q].sbx) as usize == lp).unwrap_or(lp), lp + 2, 3 + x.c) };
            r >= a && r < a + k && pc >= s && pc < e
        });
        let mut ds: Vec<&Def> = defs.iter().filter(|d| d.r == r && d.local && !inloop(d.pc)).collect();
        ds.sort_by_key(|d| d.pc);
        let mut entry = f.uses(&[0], r, &mut seen);
        let ec: Vec<usize> = entry.iter().copied().filter(|&u| !(f.i[u].op == o::TEST && f.i[u].a == r)).collect();
        if entry.len() == 1 && ec.len() == 1 && (0..ec[0]).all(|q| !matches!(f.i[q].op, o::JMP | o::EQ | o::LT | o::LE | o::TEST | o::TESTSET | o::FORPREP | o::FORLOOP | o::TFORLOOP | o::LOADBOOL)) && !(f.i[ec[0]].op == o::MOVE && f.i[ec[0]].a > r) && f.i[ec[0]].op != o::CLOSURE {
            entry.clear();
        }
        let temps: Vec<usize> = defs.iter().filter(|d| d.r <= r && !d.local).map(|d| d.pc).collect();
        let tie = |x: &Def, d: &Def| { let o1 = f.i[x.pc]; (o1.op == o::LOADBOOL && o1.c != 0 && d.pc == x.pc + 1) || (x.pc + 1 < d.pc && f.i[x.pc + 1].op == o::JMP && f.i[x.pc + 1].sbx > 0 && (x.pc as i32 + 2 + f.i[x.pc + 1].sbx) as usize > d.pc && (x.pc + 1..d.pc).all(|q| !f.writes(q).contains(&r) && !f.reads(q).contains(&r)) && (d.pc + 1..(x.pc as i32 + 2 + f.i[x.pc + 1].sbx) as usize).all(|q| !(f.i[q].op == o::CALL && f.i[q].c == 1) && !matches!(f.i[q].op, o::SETTABLE | o::SETGLOBAL | o::SETUPVAL | o::RETURN))) };
        let mut groups: Vec<(usize, Vec<&Def>)> = Vec::new();
        if !entry.is_empty() || r < ph {
            groups.push((usize::MAX, Vec::new()));
        }
        let mut last_use: Option<usize> = if entry.is_empty() { None } else { entry.last().copied() };
        for d in ds {
            let cut = match (groups.last(), last_use) {
                (None, _) => true,
                (Some(_), _) if flip.contains(&(usize::MAX - 1, r)) => false,
                (Some(_), lu) => {
                    let tied = groups.last().and_then(|g| g.1.last()).is_some_and(|x| tie(x, d));
                    let dead = groups.last().and_then(|g| g.1.last()).is_some_and(|x| x.uses.is_empty() && (0..x.pc).any(|q| f.i[q].op == o::JMP && q > 0 && matches!(f.i[q - 1].op, o::EQ | o::LT | o::LE | o::TEST | o::TESTSET) && (q as i32 + 1 + f.i[q].sbx) as usize == x.pc + 1));
                    let from = lu.unwrap_or(0).max(groups.last().map_or(0, |g| g.1.iter().map(|x| x.pc).max().unwrap_or(0)));
                    dead || !tied && (temps.iter().any(|&t| t > from && t < d.pc) || loops.iter().any(|&(lpc, a)| a <= r && lpc > from && lpc < d.pc) || groups.last().is_some_and(|g| g.0 != usize::MAX && d.pc >= block_end(g.0 + 1, r)))
                }
            };
            if cut {
                groups.push((d.pc, Vec::new()));
                last_use = None;
            }
            let g = groups.last_mut().unwrap();
            g.1.push(d);
            last_use = last_use.max(d.uses.last().copied()).max(Some(d.pc));
        }
        let nexts: Vec<usize> = groups.iter().skip(1).map(|g| g.0).chain([n]).collect();
        for ((first, g), nx) in groups.into_iter().zip(nexts) {
            let (start, lastu) = if first == usize::MAX {
                (0, g.iter().flat_map(|d| d.uses.iter().copied().chain([d.pc])).chain(entry.iter().copied()).max().unwrap_or(0))
            } else {
                let d0 = g[0];
                let alt = |x: &Def| { let o1 = f.i[x.pc]; o1.op == o::TESTSET || (o1.op == o::LOADBOOL && o1.c != 0) || (x.pc + 1 < n && (f.i[x.pc + 1].op == o::JMP || (f.i[x.pc + 1].op == o::TEST && f.i[x.pc + 1].a == x.r && o1.op != o::MOVE))) };
                let mut grp = vec![d0];
                loop {
                    let add: Vec<&Def> = g.iter().copied().filter(|d| !grp.iter().any(|x| x.pc == d.pc) && (grp.iter().any(|x| tie(x, d)) || grp.iter().all(|x| alt(x) && !ifs(x.pc.min(d.pc), x.pc.max(d.pc), r, g.iter().map(|e| e.pc).max().unwrap_or(0)) && !(0..n).any(|q| f.i[q].op == o::JMP && f.i[q].sbx < 0 && { let t = (q as i32 + 1 + f.i[q].sbx) as usize; t > x.pc.min(d.pc) && t <= x.pc.max(d.pc) })) &&grp.iter().any(|x| x.cons.iter().any(|c| *c > x.pc && *c > d.pc && d.cons.contains(c))) && !grp.iter().any(|x| x.cons.iter().any(|&c| c > x.pc && c <= d.pc)))).collect();
                    if add.is_empty() {
                        break;
                    }
                    grp.extend(add);
                }
                let s = grp.iter().map(|d| d.pc).max().unwrap_or(d0.pc);
                let fb = |v: i32| { let e = (v >> 3) & 31; if e == 0 { v } else { ((v & 7) + 8) << (e - 1) } };
                let cap = fb(f.i[s].c) as usize;
                let mut m = s;
                for d in grp.iter().filter(|d| d.pc == s) {
                    let mut k = 0;
                    for &u in &d.fills {
                        if f.i[u].op == o::SETTABLE {
                            k += 1;
                            if k > cap {
                                continue;
                            }
                        }
                        m = m.max(u);
                    }
                }
                let s = m;
                let s = s + 1 + if f.i[s].op == o::CLOSURE { p.protos.get(luac::bx(p.code[s]) as usize).map_or(0, |c| c.nups as usize) } else { 0 };
                (s, g.iter().flat_map(|d| d.uses.iter().copied().chain([d.pc])).max().unwrap_or(d0.pc))
            };
            let mut end = block_end(start, r).max(start);
            if let Some(t) = temps.iter().copied().chain(loops.iter().filter(|&&(_, a)| a <= r).map(|&(lpc, _)| lpc)).filter(|&t| t > lastu && t >= start).min() {
                end = end.min(t);
            }
            let end = end.max(lastu.min(n - 1) + if lastu >= start { 1 } else { 0 }).max(start).min(n - 1);
            let end = end.min(nx.max(start));
            cnt += 1;
            vars.push(Var { name: format!("l{depth}_{cnt}"), reg: r, start: start as i32, end: end as i32 });
        }
    }
    for (pc, a) in &loops {
        let x = f.i[*pc];
        let tgt = |q: usize| (q as i32 + 1 + f.i[q].sbx) as usize;
        let ((s0, e0), (s1, e1), k) = if x.op == o::FORPREP {
            let l = tgt(*pc);
            ((*pc, l + 1), (*pc + 1, l), 1)
        } else {
            let j = (0..*pc).find(|&q| f.i[q].op == o::JMP && tgt(q) == *pc).unwrap_or(*pc);
            ((j, *pc + 2), (j + 1, *pc), x.c as usize)
        };
        for q in 0..3 {
            vars.push(Var { name: format!("(for {q})"), reg: a + q, start: s0 as i32, end: e0 as i32 });
        }
        for q in 0..k {
            cnt += 1;
            vars.push(Var { name: format!("i{depth}_{cnt}"), reg: a + 3 + q as i32, start: s1 as i32, end: e1 as i32 });
        }
    }
    let fj = (0..n).find(|&q| matches!(f.i[q].op, o::JMP | o::EQ | o::LT | o::LE | o::TEST | o::TESTSET | o::FORPREP | o::TFORLOOP | o::LOADBOOL | o::RETURN | o::TAILCALL)).unwrap_or(n);
    let mut hv: Vec<i32> = (0..fj).filter(|&q| !f.pseudo[q]).filter_map(|q| f.writes(q).into_iter().min()).map(|w| w - 1).filter(|&r| r >= ph && r < stack).collect();
    hv.sort();
    hv.dedup();
    hv.reverse();
    let mut v = vars;
    v.sort_by_key(|x| (x.start, x.reg));
    let snapshot = v.clone();
    for x in v.iter_mut() {
        for u in &snapshot {
            if u.reg < x.reg && u.start <= x.start && x.start < u.end.max(u.start + 1) && u.end < x.end {
                x.end = u.end.max(x.start);
            }
        }
    }
    (v, (ph..stack).map(|r| (usize::MAX, r)).chain(hv.into_iter().map(|r| (usize::MAX - 2, r))).chain((fixed..stack).map(|r| (usize::MAX - 1, r))).chain(defs.iter().filter(|d| d.r >= fixed && !d.lp).map(|d| (d.pc, d.r))).collect())
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::luac::{disasm, parse};
    use std::path::Path;

    pub fn real(p: &Proto) -> Vec<(i32, i32, i32)> {
        let mut v: Vec<(i32, i32, i32)> = Vec::new();
        for (k, l) in p.locals.iter().enumerate() {
            let r = p.locals[..k].iter().filter(|x| x.start <= l.start && (x.end > l.start || (x.end == l.start && l.end == l.start))).count() as i32;
            v.push((r, l.start as i32, l.end as i32));
        }
        v.sort();
        v
    }

    fn walk(p: &Proto, depth: usize, out: &mut Vec<(Proto, Vec<(i32, i32, i32)>, Vec<(i32, i32, i32)>)>) {
        let mut s = p.clone();
        s.locals.clear();
        s.lines.clear();
        let mut got: Vec<(i32, i32, i32)> = infer(&s, depth, &[]).0.iter().map(|x| (x.reg, x.start, x.end)).collect();
        got.sort();
        out.push((p.clone(), real(p), got));
        p.protos.iter().for_each(|c| walk(c, depth + 1, out));
    }

    #[test]
    #[ignore]
    fn against_debug() {
        let Ok(g) = std::env::var("AC_GOLDEN") else { return };
        let show: usize = std::env::var("AC_SHOW").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
        let mut all = Vec::new();
        for d in ["lua", "game"] {
            for e in std::fs::read_dir(Path::new(&g).join(d)).unwrap().flatten() {
                if e.path().extension().is_some_and(|x| x == "luac") {
                    walk(&parse(&std::fs::read(e.path()).unwrap()).unwrap().main, 0, &mut all);
                }
            }
        }
        let bad: Vec<_> = all.iter().filter(|(_, a, b)| a != b).collect();
        println!("functions exact: {}/{}", all.len() - bad.len(), all.len());
        let mut sorted = bad.clone();
        sorted.sort_by_key(|(p, _, _)| p.code.len());
        for (p, a, b) in sorted.iter().take(show) {
            let miss: Vec<_> = a.iter().filter(|x| !b.contains(x)).collect();
            let extra: Vec<_> = b.iter().filter(|x| !a.contains(x)).collect();
            println!("---- params {} vararg {}\n{}real {a:?}\nmissing {miss:?}\nextra {extra:?}", p.params, p.vararg, disasm(p));
        }
    }
}

