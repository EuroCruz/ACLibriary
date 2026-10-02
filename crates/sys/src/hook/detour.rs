use crate::hook::lde::decode;
use ac_core::{bad, Res};
use crate::mem::{alloc_exec, alloc_near, branch, get, put};

const X64: bool = cfg!(target_pointer_width = "64");

fn abs_jump(to: usize) -> Vec<u8> {
    let mut v = vec![0xff, 0x25, 0, 0, 0, 0];
    v.extend_from_slice(&(to as u64).to_le_bytes());
    v
}

fn reach(from: usize, to: usize) -> bool {
    (to as i64 - from as i64).unsigned_abs() < 0x7fff_0000
}

fn jump(at: usize, to: usize) -> Option<Vec<u8>> {
    if reach(at + 5, to) {
        Some(branch(0xE9, at, to).to_vec())
    } else if X64 {
        Some(abs_jump(to))
    } else {
        None
    }
}

fn relocate(code: &[u8], src: usize, dst: usize, need: usize) -> Res<(Vec<u8>, usize)> {
    let (mut out, mut i) = (Vec::new(), 0);
    while i < need {
        let ins = decode(&code[i..], X64).ok_or(ac_core::Error::Bad("undecodable instruction"))?;
        let b = &code[i..i + ins.len];
        let here = dst + out.len();
        if let Some((o, n)) = ins.rel {
            let d = if n == 1 { b[o] as i8 as i64 } else { i32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as i64 };
            let target = (src + i + ins.len).wrapping_add(d as isize as usize);
            if target >= src && target < src + need {
                return bad("branch into stolen bytes");
            }
            let op = if ins.map == 1 { ins.op } else { b[o - 1] };
            let enc = match (ins.map, op, n) {
                (0, 0xE8 | 0xE9, 4) => vec![op],
                (0, 0xEB, 1) => vec![0xE9],
                (0, 0x70..=0x7f, 1) => vec![0x0f, 0x80 | (op & 0xf)],
                (1, 0x80..=0x8f, 4) => vec![0x0f, op],
                _ => return bad("unsupported relative branch"),
            };
            let next = here + enc.len() + 4;
            if !reach(next, target) {
                return bad("relocated branch out of range");
            }
            out.extend_from_slice(&enc);
            out.extend_from_slice(&((target as i64 - next as i64) as i32).to_le_bytes());
        } else if let Some(o) = ins.rip {
            let d = i32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as i64;
            let target = (src + i + ins.len).wrapping_add(d as isize as usize);
            let next = here + ins.len;
            if !reach(next, target) {
                return bad("rip-relative operand out of range");
            }
            out.extend_from_slice(&b[..o]);
            out.extend_from_slice(&((target as i64 - next as i64) as i32).to_le_bytes());
            out.extend_from_slice(&b[o + 4..]);
        } else {
            out.extend_from_slice(b);
        }
        i += ins.len;
    }
    Ok((out, i))
}

pub struct Hook {
    pub target: usize,
    pub tramp: usize,
    saved: Vec<u8>,
    jump: Vec<u8>,
    on: bool,
}

impl Hook {
    pub fn new(target: usize, to: usize) -> Res<Hook> {
        let stub = if X64 && !reach(target + 5, to) { alloc_near(target, 32).map(|s| (s, abs_jump(to))) } else { None };
        let (dest, mut patch, mut need) = match &stub {
            Some((s, code)) if put(*s, code) => (*s, branch(0xE9, target, *s).to_vec(), 5),
            _ => (to, jump(target, to).ok_or(ac_core::Error::Bad("target out of range"))?, 0),
        };
        let _ = dest;
        need = need.max(patch.len());
        let code = get(target, need + 15).ok_or(ac_core::Error::Bad("target not readable"))?;
        let mut len = 0;
        while len < need {
            len += decode(&code[len..], X64).ok_or(ac_core::Error::Bad("undecodable instruction"))?.len;
        }
        let tramp = (if X64 { alloc_near(target, 128) } else { alloc_exec(128) }).ok_or(ac_core::Error::Bad("trampoline allocation failed"))?;
        let (mut body, stolen) = relocate(&code, target, tramp, len)?;
        let back = target + stolen;
        body.extend(jump(tramp + body.len(), back).ok_or(ac_core::Error::Bad("trampoline out of range"))?);
        if !put(tramp, &body) {
            return bad("trampoline write failed");
        }
        let saved = code[..stolen].to_vec();
        patch.resize(stolen, 0x90);
        Ok(Hook { target, tramp, saved, jump: patch, on: false })
    }

    pub fn enable(&mut self) -> Res<()> {
        if !self.on && !put(self.target, &self.jump) {
            return bad("patch write failed");
        }
        self.on = true;
        Ok(())
    }

    pub fn disable(&mut self) -> Res<()> {
        if self.on && !put(self.target, &self.saved) {
            return bad("restore failed");
        }
        self.on = false;
        Ok(())
    }

    pub fn enabled(&self) -> bool {
        self.on
    }

    pub fn original(&self) -> usize {
        self.tramp
    }
}

pub fn detour(target: usize, to: usize) -> Res<Hook> {
    let mut h = Hook::new(target, to)?;
    h.enable()?;
    Ok(h)
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::hook::tests_support::code_page;

    extern "C" fn seven() -> i32 {
        7
    }

    extern "C" fn plus_ten() -> i32 {
        10
    }

    #[test]
    fn simple_detour_and_original() {
        let p = code_page(&[0xb8, 5, 0, 0, 0, 0xc3]);
        let f: extern "C" fn() -> i32 = unsafe { std::mem::transmute(p) };
        assert_eq!(f(), 5);
        let mut h = detour(p, seven as *const () as usize).unwrap();
        assert_eq!(f(), 7);
        let orig: extern "C" fn() -> i32 = unsafe { std::mem::transmute(h.original()) };
        assert_eq!(orig(), 5);
        h.disable().unwrap();
        assert_eq!(f(), 5);
        h.enable().unwrap();
        assert_eq!(f(), 7);
        assert!(h.enabled());
    }

    #[test]
    fn relocates_a_call() {
        let helper = [0xb8, 3, 0, 0, 0, 0xc3];
        let p = code_page(&helper);
        let t = p + 0x100;
        let mut code = vec![0xe8];
        code.extend(((p as i64 - (t as i64 + 5)) as i32).to_le_bytes());
        code.extend([0x83, 0xc0, 0x01, 0xc3]);
        assert!(put(t, &code));
        let f: extern "C" fn() -> i32 = unsafe { std::mem::transmute(t) };
        assert_eq!(f(), 4);
        let h = detour(t, plus_ten as *const () as usize).unwrap();
        assert_eq!(f(), 10);
        let orig: extern "C" fn() -> i32 = unsafe { std::mem::transmute(h.original()) };
        assert_eq!(orig(), 4);
    }

    #[test]
    fn relocates_a_short_jump() {
        let code = [0x31, 0xc0, 0x74, 0x05, 0xb8, 9, 0, 0, 0, 0xb8, 2, 0, 0, 0, 0xc3];
        let p = code_page(&code);
        let f: extern "C" fn() -> i32 = unsafe { std::mem::transmute(p) };
        assert_eq!(f(), 2);
        let h = detour(p, seven as *const () as usize).unwrap();
        assert_eq!(f(), 7);
        let orig: extern "C" fn() -> i32 = unsafe { std::mem::transmute(h.original()) };
        assert_eq!(orig(), 2);
    }

    #[test]
    fn refuses_branch_into_stolen_bytes() {
        let code = [0x90, 0xeb, 0xfd, 0xc3, 0x90, 0x90, 0x90, 0x90];
        let p = code_page(&code);
        assert!(Hook::new(p, seven as *const () as usize).is_err());
    }
}
