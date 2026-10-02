use crate::win::*;

pub fn alloc_exec(n: usize) -> Option<usize> {
    let p = unsafe { VirtualAlloc(std::ptr::null_mut(), n, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) };
    (!p.is_null()).then_some(p as usize)
}

pub fn free_exec(a: usize) {
    unsafe { VirtualFree(a as *mut _, 0, MEM_RELEASE) };
}

pub fn alloc_near(a: usize, n: usize) -> Option<usize> {
    let mut si = SysInfo::default();
    unsafe { GetSystemInfo(&mut si) };
    let g = si.granularity as usize;
    let (lo, hi) = (a.saturating_sub(0x7fff_0000).max(g), a.saturating_add(0x7fff_0000));
    let mut at = lo;
    let mut m = std::mem::MaybeUninit::<MemInfo>::zeroed();
    while at < hi {
        if unsafe { VirtualQuery(at as *const _, m.as_mut_ptr(), std::mem::size_of::<MemInfo>()) } == 0 {
            break;
        }
        let m = unsafe { m.assume_init() };
        if m.state == MEM_FREE {
            let start = m.base.next_multiple_of(g);
            if start + n <= m.base + m.size && start >= lo && start + n <= hi {
                let p = unsafe { VirtualAlloc(start as *mut _, n, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) };
                if !p.is_null() {
                    return Some(p as usize);
                }
            }
        }
        at = m.base + m.size;
    }
    None
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn near_allocation_is_within_range() {
        let anchor = alloc_exec(0x1000).unwrap();
        let n = alloc_near(anchor, 0x100).unwrap();
        assert!(n.abs_diff(anchor) < 0x8000_0000);
        assert_eq!(n % 0x1000, 0);
        free_exec(n);
        free_exec(anchor);
    }
}
