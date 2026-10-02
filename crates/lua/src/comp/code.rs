use crate::luac::{a, abc, abx, b, bx, c, op, Const, Local, Proto};
use std::collections::HashMap;

pub const NO_JUMP: i32 = -1;
pub const NO_REG: i32 = 255;
pub const MAXSTACK: i32 = 250;
const MAX_SBX: i32 = 131071;
const MAX_RK: i32 = 255;
const RK: i32 = 256;
pub const MULTRET: i32 = -1;
pub const FIELDS_PER_FLUSH: i32 = 50;

pub use crate::luac::o;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum K {
    Void,
    Nil,
    True,
    False,
    Const,
    Num,
    Local,
    Upval,
    Global,
    Indexed,
    Jmp,
    Reloc,
    NonReloc,
    Call,
    Vararg,
}

#[derive(Clone, Copy, Debug)]
pub struct E {
    pub k: K,
    pub info: i32,
    pub aux: i32,
    pub nval: f64,
    pub t: i32,
    pub f: i32,
}

impl E {
    pub fn new(k: K, info: i32) -> E {
        E { k, info, aux: 0, nval: 0.0, t: NO_JUMP, f: NO_JUMP }
    }

    pub fn num(v: f64) -> E {
        E { nval: v, ..E::new(K::Num, 0) }
    }

    fn jumps(&self) -> bool {
        self.t != self.f
    }

    fn numeral(&self) -> bool {
        self.k == K::Num && self.t == NO_JUMP && self.f == NO_JUMP
    }

    pub fn multret(&self) -> bool {
        matches!(self.k, K::Call | K::Vararg)
    }
}

#[derive(Hash, PartialEq, Eq)]
enum Key {
    Nil,
    Bool(bool),
    Num(u64),
    Str(Vec<u8>),
}

pub struct Bl {
    pub brk: i32,
    pub nact: i32,
    pub upval: bool,
    pub loop_: bool,
}

pub struct Fs {
    pub p: Proto,
    h: HashMap<Key, i32>,
    pub last_target: i32,
    pub jpc: i32,
    pub free: i32,
    pub nact: i32,
    pub act: Vec<usize>,
    pub upv: Vec<(K, i32)>,
    pub bl: Vec<Bl>,
    pub ver: u8,
}

fn set_a(i: &mut u32, v: i32) {
    *i = (*i & !(0xff << 6)) | ((v as u32 & 0xff) << 6);
}

fn set_b(i: &mut u32, v: i32) {
    *i = (*i & !(0x1ff << 23)) | ((v as u32 & 0x1ff) << 23);
}

fn set_c(i: &mut u32, v: i32) {
    *i = (*i & !(0x1ff << 14)) | ((v as u32 & 0x1ff) << 14);
}

fn set_sbx(i: &mut u32, v: i32) {
    *i = (*i & 0x3fff) | (((v + MAX_SBX) as u32) << 14);
}

fn get_sbx(i: u32) -> i32 {
    bx(i) as i32 - MAX_SBX
}

fn tmode(op_: u8) -> bool {
    matches!(op_, o::EQ | o::LT | o::LE | o::TEST | o::TESTSET)
}

pub fn int2fb(mut x: u32) -> i32 {
    let mut e = 0;
    while x >= 16 {
        x = (x + 1) >> 1;
        e += 1;
    }
    if x < 8 { x as i32 } else { ((e + 1) << 3) | (x as i32 - 8) }
}

impl Fs {
    pub fn new(source: Option<Vec<u8>>) -> Fs {
        Fs {
            p: Proto { source, stack: 2, ..Proto::default() },
            h: HashMap::new(),
            last_target: -1,
            jpc: NO_JUMP,
            free: 0,
            nact: 0,
            act: Vec::new(),
            upv: Vec::new(),
            bl: Vec::new(),
            ver: 5,
        }
    }

    pub fn pc(&self) -> i32 {
        self.p.code.len() as i32
    }

    pub fn code_at(&mut self, e: &E) -> &mut u32 {
        &mut self.p.code[e.info as usize]
    }

    pub fn loc(&mut self, i: i32) -> &mut Local {
        let k = self.act[i as usize];
        &mut self.p.locals[k]
    }

    fn emit(&mut self, i: u32, line: u32) -> i32 {
        self.discharge_jpc();
        self.p.code.push(i);
        self.p.lines.push(line as u64);
        self.pc() - 1
    }

    pub fn abc(&mut self, op_: u8, a_: i32, b_: i32, c_: i32, line: u32) -> i32 {
        self.emit(abc(op_, a_ as u32, b_ as u32, c_ as u32), line)
    }

    pub fn abx(&mut self, op_: u8, a_: i32, bx_: i32, line: u32) -> i32 {
        self.emit(abx(op_, a_ as u32, bx_ as u32), line)
    }

    pub fn asbx(&mut self, op_: u8, a_: i32, sbx: i32, line: u32) -> i32 {
        self.abx(op_, a_, sbx + MAX_SBX, line)
    }

    pub fn raw(&mut self, i: u32, line: u32) -> i32 {
        self.emit(i, line)
    }

    pub fn fix_line(&mut self, line: u32) {
        let n = self.p.lines.len() - 1;
        self.p.lines[n] = line as u64;
    }

    pub fn nil(&mut self, from: i32, n: i32, line: u32) {
        if self.pc() > self.last_target {
            if self.pc() == 0 {
                if self.ver <= 1 {
                    return;
                }
                if from >= self.nact {
                    return;
                }
            } else {
                let pc = self.pc() as usize - 1;
                let prev = self.p.code[pc];
                if op(prev) == o::LOADNIL {
                    let (pf, pt) = (a(prev) as i32, b(prev) as i32);
                    if pf <= from && from <= pt + 1 {
                        if from + n - 1 > pt {
                            set_b(&mut self.p.code[pc], from + n - 1);
                        }
                        return;
                    }
                }
            }
        }
        self.abc(o::LOADNIL, from, from + n - 1, 0, line);
    }

    pub fn jump(&mut self, line: u32) -> i32 {
        let jpc = self.jpc;
        self.jpc = NO_JUMP;
        let mut j = self.asbx(o::JMP, 0, NO_JUMP, line);
        self.concat(&mut j, jpc);
        j
    }

    pub fn ret(&mut self, first: i32, n: i32, line: u32) {
        self.abc(o::RETURN, first, n + 1, 0, line);
    }

    fn cond_jump(&mut self, op_: u8, a_: i32, b_: i32, c_: i32, line: u32) -> i32 {
        self.abc(op_, a_, b_, c_, line);
        self.jump(line)
    }

    pub fn fix_jump(&mut self, pc: i32, dest: i32) -> Result<(), &'static str> {
        let off = dest - (pc + 1);
        if off.abs() > MAX_SBX {
            return Err("control structure too long");
        }
        set_sbx(&mut self.p.code[pc as usize], off);
        Ok(())
    }

    pub fn label(&mut self) -> i32 {
        self.last_target = self.pc();
        self.pc()
    }

    fn get_jump(&self, pc: i32) -> i32 {
        let off = get_sbx(self.p.code[pc as usize]);
        if off == NO_JUMP { NO_JUMP } else { pc + 1 + off }
    }

    fn ctl(&self, pc: i32) -> usize {
        if pc >= 1 && tmode(op(self.p.code[pc as usize - 1])) { pc as usize - 1 } else { pc as usize }
    }

    fn need_value(&self, mut l: i32) -> bool {
        while l != NO_JUMP {
            if op(self.p.code[self.ctl(l)]) != o::TESTSET {
                return true;
            }
            l = self.get_jump(l);
        }
        false
    }

    fn patch_test(&mut self, node: i32, reg: i32) -> bool {
        let at = self.ctl(node);
        let i = self.p.code[at];
        if op(i) != o::TESTSET {
            return false;
        }
        if reg != NO_REG && reg != b(i) as i32 {
            set_a(&mut self.p.code[at], reg);
        } else {
            self.p.code[at] = abc(o::TEST, b(i), 0, c(i));
        }
        true
    }

    fn remove_values(&mut self, mut l: i32) {
        while l != NO_JUMP {
            self.patch_test(l, NO_REG);
            l = self.get_jump(l);
        }
    }

    fn patch_aux(&mut self, mut l: i32, vt: i32, reg: i32, dt: i32) {
        while l != NO_JUMP {
            let next = self.get_jump(l);
            let t = if self.patch_test(l, reg) { vt } else { dt };
            let _ = self.fix_jump(l, t);
            l = next;
        }
    }

    fn discharge_jpc(&mut self) {
        let (j, pc) = (self.jpc, self.pc());
        self.patch_aux(j, pc, NO_REG, pc);
        self.jpc = NO_JUMP;
    }

    pub fn patch_list(&mut self, l: i32, target: i32) {
        if target == self.pc() {
            self.patch_here(l);
        } else {
            self.patch_aux(l, target, NO_REG, target);
        }
    }

    pub fn patch_here(&mut self, l: i32) {
        self.label();
        let mut j = self.jpc;
        self.concat(&mut j, l);
        self.jpc = j;
    }

    pub fn concat(&mut self, l1: &mut i32, l2: i32) {
        if l2 == NO_JUMP {
            return;
        }
        if *l1 == NO_JUMP {
            *l1 = l2;
            return;
        }
        let mut l = *l1;
        loop {
            let n = self.get_jump(l);
            if n == NO_JUMP {
                break;
            }
            l = n;
        }
        let _ = self.fix_jump(l, l2);
    }

    pub fn check_stack(&mut self, n: i32) -> Result<(), &'static str> {
        let ns = self.free + n;
        if ns > self.p.stack as i32 {
            if ns >= MAXSTACK {
                return Err("function or expression too complex");
            }
            self.p.stack = ns as u8;
        }
        Ok(())
    }

    pub fn reserve(&mut self, n: i32) -> Result<(), &'static str> {
        self.check_stack(n)?;
        self.free += n;
        Ok(())
    }

    fn free_reg(&mut self, r: i32) {
        if r & RK == 0 && r >= self.nact {
            self.free -= 1;
        }
    }

    pub fn free_exp(&mut self, e: &E) {
        if e.k == K::NonReloc {
            self.free_reg(e.info);
        }
    }

    fn add_k(&mut self, k: Key, v: Const) -> i32 {
        if let Some(&i) = self.h.get(&k) {
            return i;
        }
        let i = self.p.consts.len() as i32;
        self.p.consts.push(v);
        self.h.insert(k, i);
        i
    }

    pub fn str_k(&mut self, s: &[u8]) -> i32 {
        self.add_k(Key::Str(s.to_vec()), Const::Str(s.to_vec()))
    }

    pub fn num_k(&mut self, v: f64) -> i32 {
        let bits = if v == 0.0 { 0.0f64.to_bits() } else { v.to_bits() };
        self.add_k(Key::Num(bits), Const::Num(v))
    }

    fn bool_k(&mut self, v: bool) -> i32 {
        self.add_k(Key::Bool(v), Const::Bool(v))
    }

    fn nil_k(&mut self) -> i32 {
        self.add_k(Key::Nil, Const::Nil)
    }

    pub fn set_returns(&mut self, e: &E, n: i32) -> Result<(), &'static str> {
        match e.k {
            K::Call => set_c(self.code_at(e), n + 1),
            K::Vararg => {
                let f = self.free;
                let i = self.code_at(e);
                set_b(i, n + 1);
                set_a(i, f);
                self.reserve(1)?;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn set_one_ret(&mut self, e: &mut E) {
        match e.k {
            K::Call => {
                e.k = K::NonReloc;
                e.info = a(*self.code_at(e)) as i32;
            }
            K::Vararg => {
                set_b(self.code_at(e), 2);
                e.k = K::Reloc;
            }
            _ => {}
        }
    }

    pub fn discharge_vars(&mut self, e: &mut E, line: u32) {
        match e.k {
            K::Local => e.k = K::NonReloc,
            K::Upval => {
                e.info = self.abc(o::GETUPVAL, 0, e.info, 0, line);
                e.k = K::Reloc;
            }
            K::Global => {
                e.info = self.abx(o::GETGLOBAL, 0, e.info, line);
                e.k = K::Reloc;
            }
            K::Indexed => {
                self.free_reg(e.aux);
                self.free_reg(e.info);
                e.info = self.abc(o::GETTABLE, 0, e.info, e.aux, line);
                e.k = K::Reloc;
            }
            K::Vararg | K::Call => self.set_one_ret(e),
            _ => {}
        }
    }

    fn code_label(&mut self, a_: i32, b_: i32, j: i32, line: u32) -> i32 {
        self.label();
        self.abc(o::LOADBOOL, a_, b_, j, line)
    }

    fn discharge2reg(&mut self, e: &mut E, reg: i32, line: u32) {
        self.discharge_vars(e, line);
        match e.k {
            K::Nil => self.nil(reg, 1, line),
            K::False | K::True => {
                self.abc(o::LOADBOOL, reg, (e.k == K::True) as i32, 0, line);
            }
            K::Const => {
                self.abx(o::LOADK, reg, e.info, line);
            }
            K::Num => {
                let k = self.num_k(e.nval);
                self.abx(o::LOADK, reg, k, line);
            }
            K::Reloc => set_a(self.code_at(e), reg),
            K::NonReloc => {
                if reg != e.info {
                    self.abc(o::MOVE, reg, e.info, 0, line);
                }
            }
            _ => return,
        }
        e.info = reg;
        e.k = K::NonReloc;
    }

    fn discharge2any(&mut self, e: &mut E, line: u32) -> Result<(), &'static str> {
        if e.k != K::NonReloc {
            self.reserve(1)?;
            let r = self.free - 1;
            self.discharge2reg(e, r, line);
        }
        Ok(())
    }

    fn exp2reg(&mut self, e: &mut E, reg: i32, line: u32) {
        self.discharge2reg(e, reg, line);
        if e.k == K::Jmp {
            let mut t = e.t;
            self.concat(&mut t, e.info);
            e.t = t;
        }
        if e.jumps() {
            let (mut pf, mut pt) = (NO_JUMP, NO_JUMP);
            if self.need_value(e.t) || self.need_value(e.f) {
                let fj = if e.k == K::Jmp { NO_JUMP } else { self.jump(line) };
                pf = self.code_label(reg, 0, 1, line);
                pt = self.code_label(reg, 1, 0, line);
                self.patch_here(fj);
            }
            let fin = self.label();
            self.patch_aux(e.f, fin, reg, pf);
            self.patch_aux(e.t, fin, reg, pt);
        }
        e.f = NO_JUMP;
        e.t = NO_JUMP;
        e.info = reg;
        e.k = K::NonReloc;
    }

    pub fn exp2next(&mut self, e: &mut E, line: u32) -> Result<(), &'static str> {
        self.discharge_vars(e, line);
        self.free_exp(e);
        self.reserve(1)?;
        let r = self.free - 1;
        self.exp2reg(e, r, line);
        Ok(())
    }

    pub fn exp2any(&mut self, e: &mut E, line: u32) -> Result<i32, &'static str> {
        self.discharge_vars(e, line);
        if e.k == K::NonReloc {
            if !e.jumps() {
                return Ok(e.info);
            }
            if e.info >= self.nact {
                let r = e.info;
                self.exp2reg(e, r, line);
                return Ok(e.info);
            }
        }
        self.exp2next(e, line)?;
        Ok(e.info)
    }

    pub fn exp2val(&mut self, e: &mut E, line: u32) -> Result<(), &'static str> {
        if e.jumps() {
            self.exp2any(e, line)?;
        } else {
            self.discharge_vars(e, line);
        }
        Ok(())
    }

    pub fn exp2rk(&mut self, e: &mut E, line: u32) -> Result<i32, &'static str> {
        self.exp2val(e, line)?;
        match e.k {
            K::Num | K::True | K::False | K::Nil if self.p.consts.len() as i32 <= MAX_RK => {
                e.info = match e.k {
                    K::Nil => self.nil_k(),
                    K::Num => self.num_k(e.nval),
                    k => self.bool_k(k == K::True),
                };
                e.k = K::Const;
                return Ok(e.info | RK);
            }
            K::Const if e.info <= MAX_RK => return Ok(e.info | RK),
            _ => {}
        }
        self.exp2any(e, line)
    }

    pub fn store(&mut self, var: &E, ex: &mut E, line: u32) -> Result<(), &'static str> {
        match var.k {
            K::Local => {
                self.free_exp(ex);
                self.exp2reg(ex, var.info, line);
                return Ok(());
            }
            K::Upval => {
                let e = self.exp2any(ex, line)?;
                self.abc(o::SETUPVAL, e, var.info, 0, line);
            }
            K::Global => {
                let e = self.exp2any(ex, line)?;
                self.abx(o::SETGLOBAL, e, var.info, line);
            }
            K::Indexed => {
                let e = self.exp2rk(ex, line)?;
                self.abc(o::SETTABLE, var.info, var.aux, e, line);
            }
            _ => {}
        }
        self.free_exp(ex);
        Ok(())
    }

    pub fn self_(&mut self, e: &mut E, key: &mut E, line: u32) -> Result<(), &'static str> {
        self.exp2any(e, line)?;
        self.free_exp(e);
        let f = self.free;
        self.reserve(2)?;
        let k = self.exp2rk(key, line)?;
        self.abc(o::SELF, f, e.info, k, line);
        self.free_exp(key);
        e.info = f;
        e.k = K::NonReloc;
        Ok(())
    }

    fn invert(&mut self, e: &E) {
        let at = self.ctl(e.info);
        let v = a(self.p.code[at]) as i32;
        set_a(&mut self.p.code[at], (v == 0) as i32);
    }

    fn jump_on_cond(&mut self, e: &mut E, cond: i32, line: u32) -> Result<i32, &'static str> {
        if e.k == K::Reloc {
            let ie = *self.code_at(e);
            if op(ie) == o::NOT {
                self.p.code.pop();
                self.p.lines.pop();
                return Ok(self.cond_jump(o::TEST, b(ie) as i32, 0, (cond == 0) as i32, line));
            }
        }
        self.discharge2any(e, line)?;
        self.free_exp(e);
        Ok(self.cond_jump(o::TESTSET, NO_REG, e.info, cond, line))
    }

    pub fn go_if_true(&mut self, e: &mut E, line: u32) -> Result<(), &'static str> {
        self.discharge_vars(e, line);
        let pc = match e.k {
            K::Const | K::Num | K::True => NO_JUMP,
            K::False if self.ver <= 4 => self.jump(line),
            K::Jmp => {
                self.invert(e);
                e.info
            }
            _ => self.jump_on_cond(e, 0, line)?,
        };
        let mut f = e.f;
        self.concat(&mut f, pc);
        e.f = f;
        self.patch_here(e.t);
        e.t = NO_JUMP;
        Ok(())
    }

    fn go_if_false(&mut self, e: &mut E, line: u32) -> Result<(), &'static str> {
        self.discharge_vars(e, line);
        let pc = match e.k {
            K::Nil | K::False => NO_JUMP,
            K::True if self.ver <= 4 => self.jump(line),
            K::Jmp => e.info,
            _ => self.jump_on_cond(e, 1, line)?,
        };
        let mut t = e.t;
        self.concat(&mut t, pc);
        e.t = t;
        self.patch_here(e.f);
        e.f = NO_JUMP;
        Ok(())
    }

    fn code_not(&mut self, e: &mut E, line: u32) -> Result<(), &'static str> {
        self.discharge_vars(e, line);
        match e.k {
            K::Nil | K::False => e.k = K::True,
            K::Const | K::Num | K::True => e.k = K::False,
            K::Jmp => self.invert(e),
            K::Reloc | K::NonReloc => {
                self.discharge2any(e, line)?;
                self.free_exp(e);
                e.info = self.abc(o::NOT, 0, e.info, 0, line);
                e.k = K::Reloc;
            }
            _ => {}
        }
        std::mem::swap(&mut e.f, &mut e.t);
        self.remove_values(e.f);
        self.remove_values(e.t);
        Ok(())
    }

    pub fn indexed(&mut self, t: &mut E, k: &mut E, line: u32) -> Result<(), &'static str> {
        t.aux = self.exp2rk(k, line)?;
        t.k = K::Indexed;
        Ok(())
    }

    fn fold(op_: u8, e1: &mut E, e2: &E) -> bool {
        if !e1.numeral() || !e2.numeral() {
            return false;
        }
        let (v1, v2) = (e1.nval, e2.nval);
        let r = match op_ {
            o::ADD => v1 + v2,
            o::SUB => v1 - v2,
            o::MUL => v1 * v2,
            o::DIV if v2 != 0.0 => v1 / v2,
            o::MOD if v2 != 0.0 => v1 - (v1 / v2).floor() * v2,
            o::POW => v1.powf(v2),
            o::UNM => -v1,
            _ => return false,
        };
        if r.is_nan() {
            return false;
        }
        e1.nval = r;
        true
    }

    fn arith(&mut self, op_: u8, e1: &mut E, e2: &mut E, line: u32) -> Result<(), &'static str> {
        if Self::fold(op_, e1, e2) {
            return Ok(());
        }
        let o2 = if op_ != o::UNM && op_ != o::LEN { self.exp2rk(e2, line)? } else { 0 };
        let o1 = self.exp2rk(e1, line)?;
        if o1 > o2 {
            self.free_exp(e1);
            self.free_exp(e2);
        } else {
            self.free_exp(e2);
            self.free_exp(e1);
        }
        e1.info = self.abc(op_, 0, o1, o2, line);
        e1.k = K::Reloc;
        Ok(())
    }

    fn comp(&mut self, op_: u8, mut cond: i32, e1: &mut E, e2: &mut E, line: u32) -> Result<(), &'static str> {
        let mut o1 = self.exp2rk(e1, line)?;
        let mut o2 = self.exp2rk(e2, line)?;
        self.free_exp(e2);
        self.free_exp(e1);
        if cond == 0 && op_ != o::EQ {
            std::mem::swap(&mut o1, &mut o2);
            cond = 1;
        }
        e1.info = self.cond_jump(op_, cond, o1, o2, line);
        e1.k = K::Jmp;
        Ok(())
    }

    pub fn prefix(&mut self, u: Un, e: &mut E, line: u32) -> Result<(), &'static str> {
        let mut e2 = E::num(0.0);
        match u {
            Un::Minus => {
                if (self.ver <= 2 && e.k == K::Const) || (self.ver > 2 && !e.numeral()) {
                    self.exp2any(e, line)?;
                }
                self.arith(o::UNM, e, &mut e2, line)
            }
            Un::Not => self.code_not(e, line),
            Un::Len => {
                self.exp2any(e, line)?;
                self.arith(o::LEN, e, &mut e2, line)
            }
        }
    }

    pub fn infix(&mut self, b_: Bin, v: &mut E, line: u32) -> Result<(), &'static str> {
        match b_ {
            Bin::And => self.go_if_true(v, line),
            Bin::Or => self.go_if_false(v, line),
            Bin::Concat => self.exp2next(v, line),
            Bin::Add | Bin::Sub | Bin::Mul | Bin::Div | Bin::Mod | Bin::Pow => {
                if !v.numeral() {
                    self.exp2rk(v, line)?;
                }
                Ok(())
            }
            _ => self.exp2rk(v, line).map(drop),
        }
    }

    pub fn posfix(&mut self, b_: Bin, e1: &mut E, e2: &mut E, line: u32) -> Result<(), &'static str> {
        match b_ {
            Bin::And => {
                self.discharge_vars(e2, line);
                let mut f = e2.f;
                self.concat(&mut f, e1.f);
                e2.f = f;
                *e1 = *e2;
                Ok(())
            }
            Bin::Or => {
                self.discharge_vars(e2, line);
                let mut t = e2.t;
                self.concat(&mut t, e1.t);
                e2.t = t;
                *e1 = *e2;
                Ok(())
            }
            Bin::Concat => {
                self.exp2val(e2, line)?;
                if e2.k == K::Reloc && op(*self.code_at(e2)) == o::CONCAT {
                    self.free_exp(e1);
                    set_b(self.code_at(e2), e1.info);
                    e1.k = K::Reloc;
                    e1.info = e2.info;
                    Ok(())
                } else {
                    self.exp2next(e2, line)?;
                    self.arith(o::CONCAT, e1, e2, line)
                }
            }
            Bin::Add => self.arith(o::ADD, e1, e2, line),
            Bin::Sub => self.arith(o::SUB, e1, e2, line),
            Bin::Mul => self.arith(o::MUL, e1, e2, line),
            Bin::Div => self.arith(o::DIV, e1, e2, line),
            Bin::Mod => self.arith(o::MOD, e1, e2, line),
            Bin::Pow => self.arith(o::POW, e1, e2, line),
            Bin::Eq => self.comp(o::EQ, 1, e1, e2, line),
            Bin::Ne => self.comp(o::EQ, 0, e1, e2, line),
            Bin::Lt => self.comp(o::LT, 1, e1, e2, line),
            Bin::Le => self.comp(o::LE, 1, e1, e2, line),
            Bin::Gt => self.comp(o::LT, 0, e1, e2, line),
            Bin::Ge => self.comp(o::LE, 0, e1, e2, line),
        }
    }

    pub fn set_list(&mut self, base: i32, n: i32, tostore: i32, line: u32) {
        let c_ = (n - 1) / FIELDS_PER_FLUSH + 1;
        let b_ = if tostore == MULTRET { 0 } else { tostore };
        if c_ <= 511 {
            self.abc(o::SETLIST, base, b_, c_, line);
        } else {
            self.abc(o::SETLIST, base, b_, 0, line);
            self.raw(c_ as u32, line);
        }
        self.free = base + 1;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Un {
    Minus,
    Not,
    Len,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Ne,
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl Bin {
    pub fn prio(self) -> (u32, u32) {
        match self {
            Bin::Add | Bin::Sub => (6, 6),
            Bin::Mul | Bin::Div | Bin::Mod => (7, 7),
            Bin::Pow => (10, 9),
            Bin::Concat => (5, 4),
            Bin::And => (2, 2),
            Bin::Or => (1, 1),
            _ => (3, 3),
        }
    }
}
