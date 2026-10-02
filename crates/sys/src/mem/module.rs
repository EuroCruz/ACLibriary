use crate::mem::readable;
use ac_core::scan::Pat;
use crate::win::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sect {
    pub name: String,
    pub start: usize,
    pub size: usize,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    pub base: usize,
}

impl Module {
    pub fn main() -> Module {
        Module { base: unsafe { GetModuleHandleA(std::ptr::null()) as usize } }
    }

    pub fn named(n: &str) -> Option<Module> {
        let h = module(n) as usize;
        (h != 0).then_some(Module { base: h })
    }

    fn u32at(&self, o: usize) -> u32 {
        unsafe { std::ptr::read_unaligned((self.base + o) as *const u32) }
    }

    pub fn valid(&self) -> bool {
        self.base != 0 && readable(self.base, 0x40) && unsafe { *(self.base as *const u16) } == 0x5a4d
    }

    fn nt(&self) -> usize {
        self.u32at(0x3c) as usize
    }

    pub fn entry(&self) -> usize {
        self.base + self.u32at(self.nt() + 24 + 16) as usize
    }

    pub fn size(&self) -> usize {
        self.u32at(self.nt() + 24 + 56) as usize
    }

    pub fn sections(&self) -> Vec<Sect> {
        if !self.valid() {
            return Vec::new();
        }
        let nt = self.nt();
        let n = unsafe { std::ptr::read_unaligned((self.base + nt + 6) as *const u16) } as usize;
        let opt = unsafe { std::ptr::read_unaligned((self.base + nt + 20) as *const u16) } as usize;
        (0..n)
            .map(|i| {
                let o = nt + 24 + opt + i * 40;
                let name = unsafe { std::slice::from_raw_parts((self.base + o) as *const u8, 8) };
                Sect {
                    name: String::from_utf8_lossy(&name[..name.iter().position(|&b| b == 0).unwrap_or(8)]).into_owned(),
                    size: self.u32at(o + 8) as usize,
                    start: self.base + self.u32at(o + 12) as usize,
                    flags: self.u32at(o + 36),
                }
            })
            .collect()
    }

    pub fn section(&self, name: &str) -> Option<Sect> {
        self.sections().into_iter().find(|s| s.name == name)
    }

    pub fn slice(&self, s: &Sect) -> Option<&'static [u8]> {
        readable(s.start, s.size).then(|| unsafe { std::slice::from_raw_parts(s.start as *const u8, s.size) })
    }

    pub fn find(&self, p: &Pat) -> Option<usize> {
        self.sections().iter().filter(|s| s.flags & 0x4000_0000 != 0).find_map(|s| self.slice(s).and_then(|d| p.find(d)).map(|i| s.start + i))
    }

    pub fn find_all(&self, p: &Pat) -> Vec<usize> {
        self.sections().iter().filter(|s| s.flags & 0x4000_0000 != 0).flat_map(|s| self.slice(s).map(|d| p.all(d).into_iter().map(|i| s.start + i).collect::<Vec<_>>()).unwrap_or_default()).collect()
    }

    pub fn export(&self, name: &str) -> Option<usize> {
        let a = proc_addr(self.base as HMODULE, name);
        (a != 0).then_some(a)
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[inline(never)]
    fn marker(x: u32) -> u32 {
        std::hint::black_box(x.wrapping_mul(0x9e37_79b1) ^ 0xa5a5_5a5a)
    }

    #[test]
    fn main_module() {
        let m = Module::main();
        assert!(m.valid() && m.size() > 0x1000);
        let s = m.sections();
        assert!(s.iter().any(|x| x.name == ".text"));
        assert!(m.section(".text").unwrap().start > m.base);
        assert!(m.entry() > m.base);
        assert!(m.section(".nope").is_none());
    }

    #[test]
    fn finds_own_code() {
        let f = marker as fn(u32) -> u32 as usize;
        let head = crate::mem::get(f, 12).unwrap();
        let hits = Module::main().find_all(&Pat::exact(&head));
        assert!(hits.contains(&f));
        assert_eq!(Module::main().find(&Pat::exact(&head)).map(|a| a <= f), Some(true));
    }

    #[test]
    fn system_modules() {
        let k = Module::named("kernel32.dll").unwrap();
        assert!(k.valid());
        assert!(k.export("VirtualProtect").is_some() && k.export("Nope").is_none());
        assert!(Module::named("no_such_module_xyz.dll").is_none());
    }
}
