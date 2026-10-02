use crate::mem::{read, write, Module};

const W: usize = std::mem::size_of::<usize>();

pub fn vtable(obj: usize) -> Option<usize> {
    read::<usize>(obj)
}

pub fn slot(obj: usize, i: usize) -> Option<usize> {
    read::<usize>(vtable(obj)? + i * W)
}

pub fn hook_slot(obj: usize, i: usize, new: usize) -> Option<usize> {
    let a = vtable(obj)? + i * W;
    let old = read::<usize>(a)?;
    write(a, new).then_some(old)
}

pub fn hook_table(table: usize, i: usize, new: usize) -> Option<usize> {
    let a = table + i * W;
    let old = read::<usize>(a)?;
    write(a, new).then_some(old)
}

pub fn iat_slot(m: Module, dll: &str, func: &str) -> Option<usize> {
    if !m.valid() {
        return None;
    }
    let nt = read::<u32>(m.base + 0x3c)? as usize;
    let p64 = read::<u16>(m.base + nt + 24)? == 0x20b;
    let dir = m.base + nt + 24 + if p64 { 112 } else { 96 } + 8;
    let rva = read::<u32>(dir)? as usize;
    if rva == 0 {
        return None;
    }
    let flag = if p64 { 1u64 << 63 } else { 1 << 31 };
    for i in 0.. {
        let d = m.base + rva + i * 20;
        let (oft, name, ft) = (read::<u32>(d)? as usize, read::<u32>(d + 12)? as usize, read::<u32>(d + 16)? as usize);
        if name == 0 {
            return None;
        }
        if !cstr(m.base + name)?.eq_ignore_ascii_case(dll) {
            continue;
        }
        let table = if oft != 0 { oft } else { ft };
        for k in 0.. {
            let v = if p64 { read::<u64>(m.base + table + k * 8)? } else { read::<u32>(m.base + table + k * 4)? as u64 };
            if v == 0 {
                break;
            }
            if v & flag == 0 && cstr(m.base + v as usize + 2)? == func {
                return Some(m.base + ft + k * W);
            }
        }
    }
    None
}

pub fn hook_iat(m: Module, dll: &str, func: &str, new: usize) -> Option<usize> {
    let a = iat_slot(m, dll, func)?;
    let old = read::<usize>(a)?;
    write(a, new).then_some(old)
}

fn cstr(a: usize) -> Option<String> {
    let mut v = Vec::new();
    for i in 0..260 {
        let b = read::<u8>(a + i)?;
        if b == 0 {
            return Some(String::from_utf8_lossy(&v).into_owned());
        }
        v.push(b);
    }
    None
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::mem::{alloc_exec, free_exec};
    use std::sync::atomic::{AtomicU32, Ordering};

    static HITS: AtomicU32 = AtomicU32::new(0);

    extern "system" fn fake_sleep(_: u32) {
        HITS.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn vtable_hooking() {
        let p = alloc_exec(0x1000).unwrap();
        write(p, p + 0x100);
        write(p + 0x100, 0x1111usize);
        write(p + 0x100 + W, 0x2222usize);
        assert_eq!(slot(p, 1), Some(0x2222));
        assert_eq!(hook_slot(p, 1, 0x3333), Some(0x2222));
        assert_eq!(slot(p, 1), Some(0x3333));
        assert_eq!(hook_table(p + 0x100, 0, 0x4444), Some(0x1111));
        assert_eq!(slot(p, 0), Some(0x4444));
        assert_eq!(slot(0, 0), None);
        free_exec(p);
    }

    #[test]
    fn iat_hooking_of_this_executable() {
        let m = Module::main();
        let a = iat_slot(m, "KERNEL32.dll", "Sleep");
        assert!(a.is_some());
        assert!(iat_slot(m, "kernel32.DLL", "Sleep").is_some());
        assert!(iat_slot(m, "kernel32.dll", "NoSuchFunction").is_none());
        let old = hook_iat(m, "kernel32.dll", "Sleep", fake_sleep as *const () as usize).unwrap();
        unsafe { crate::win::Sleep(1) };
        assert_eq!(HITS.load(Ordering::SeqCst), 1);
        assert_eq!(hook_iat(m, "kernel32.dll", "Sleep", old), Some(fake_sleep as *const () as usize));
        unsafe { crate::win::Sleep(0) };
        assert_eq!(HITS.load(Ordering::SeqCst), 1);
    }
}
