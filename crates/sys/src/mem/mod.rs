mod alloc;
mod module;
mod patch;

pub use alloc::{alloc_exec, alloc_near, free_exec};
pub use module::{Module, Sect};
pub use patch::{Group, Patch, State};

use crate::win::*;

pub fn protect<R>(a: usize, n: usize, f: impl FnOnce() -> R) -> Option<R> {
    let mut old = 0u32;
    unsafe {
        if VirtualProtect(a as *mut _, n.max(1), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return None;
        }
        let r = f();
        VirtualProtect(a as *mut _, n.max(1), old, &mut old);
        Some(r)
    }
}

pub fn readable(a: usize, n: usize) -> bool {
    let mut m = std::mem::MaybeUninit::<MemInfo>::zeroed();
    let (mut at, end) = (a, a.saturating_add(n));
    while at < end {
        let ok = unsafe { VirtualQuery(at as *const _, m.as_mut_ptr(), std::mem::size_of::<MemInfo>()) } != 0;
        let m = unsafe { m.assume_init() };
        if !ok || m.state != MEM_COMMIT || m.prot & 0x101 != 0 || m.prot & 0xff == 0 {
            return false;
        }
        at = m.base + m.size;
    }
    true
}

pub fn get(a: usize, n: usize) -> Option<Vec<u8>> {
    readable(a, n).then(|| unsafe { std::slice::from_raw_parts(a as *const u8, n).to_vec() })
}

pub fn put(a: usize, b: &[u8]) -> bool {
    protect(a, b.len(), || unsafe {
        std::ptr::copy_nonoverlapping(b.as_ptr(), a as *mut u8, b.len());
        FlushInstructionCache(GetCurrentProcess(), a as *const _, b.len());
    })
    .is_some()
}

pub fn fill(a: usize, v: u8, n: usize) -> bool {
    put(a, &vec![v; n])
}

pub fn nop(a: usize, n: usize) -> bool {
    fill(a, 0x90, n)
}

pub fn read<T: Copy>(a: usize) -> Option<T> {
    readable(a, std::mem::size_of::<T>()).then(|| unsafe { std::ptr::read_unaligned(a as *const T) })
}

pub fn write<T: Copy>(a: usize, v: T) -> bool {
    let b = unsafe { std::slice::from_raw_parts(&v as *const T as *const u8, std::mem::size_of::<T>()) };
    put(a, b)
}

pub fn rel32(next: usize, to: usize) -> u32 {
    to.wrapping_sub(next) as u32
}

pub fn branch(op: u8, at: usize, to: usize) -> [u8; 5] {
    let r = rel32(at + 5, to).to_le_bytes();
    [op, r[0], r[1], r[2], r[3]]
}

pub fn follow(a: usize) -> Option<usize> {
    let b = get(a, 5)?;
    (b[0] == 0xE8 || b[0] == 0xE9).then(|| (a + 5).wrapping_add(i32::from_le_bytes(b[1..5].try_into().unwrap()) as isize as usize))
}

pub fn hook_call(at: usize, to: usize) -> Option<usize> {
    let old = follow(at)?;
    put(at, &branch(0xE8, at, to)).then_some(old)
}

#[cfg(test)]
mod t {
    use super::*;

    fn page() -> usize {
        alloc_exec(0x1000).unwrap()
    }

    #[test]
    fn read_write_roundtrip() {
        let p = page();
        assert!(write(p, 0xdead_beefu32) && write(p + 5, 7u16));
        assert_eq!(read::<u32>(p), Some(0xdead_beef));
        assert_eq!(get(p + 5, 2), Some(vec![7, 0]));
        assert!(fill(p + 16, 0xcc, 4) && nop(p + 20, 2));
        assert_eq!(get(p + 16, 6), Some(vec![0xcc, 0xcc, 0xcc, 0xcc, 0x90, 0x90]));
        assert!(!readable(0, 4) && readable(p, 0x1000) && !readable(p, 0x3000) && read::<u32>(0).is_none());
        free_exec(p);
    }

    #[test]
    fn branches() {
        assert_eq!(branch(0xE8, 0x1000, 0x1010), [0xE8, 11, 0, 0, 0]);
        assert_eq!(branch(0xE9, 0x1000, 0x0ff0), [0xE9, 0xeb, 0xff, 0xff, 0xff]);
        let p = page();
        put(p, &branch(0xE9, p, p + 0x40));
        assert_eq!(follow(p), Some(p + 0x40));
        assert_eq!(follow(p + 8), None);
        put(p + 0x10, &branch(0xE8, p + 0x10, p + 0x80));
        assert_eq!(hook_call(p + 0x10, p + 0x90), Some(p + 0x80));
        assert_eq!(follow(p + 0x10), Some(p + 0x90));
        free_exec(p);
    }
}
