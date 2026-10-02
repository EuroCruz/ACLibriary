use crate::pe::Pe;

macro_rules! scalar {
    ($get:ident, $set:ident, $r:ident, $w:ident, $t:ty, $off:expr) => {
        pub fn $get(&self) -> $t {
            self.$r($off(self))
        }

        pub fn $set(&mut self, v: $t) {
            let o = $off(self);
            self.$w(o, v)
        }
    };
}

pub const LAA: u16 = 0x20;
pub const DYNAMIC_BASE: u16 = 0x40;
pub const NX_COMPAT: u16 = 0x100;

impl Pe {
    scalar!(machine, set_machine, r16, w16, u16, |p: &Pe| p.pe + 4);
    scalar!(stamp, set_stamp, r32, w32, u32, |p: &Pe| p.pe + 8);
    scalar!(chars, set_chars, r16, w16, u16, |p: &Pe| p.pe + 22);
    scalar!(code_size, set_code_size, r32, w32, u32, |p: &Pe| p.opt + 4);
    scalar!(init_size, set_init_size, r32, w32, u32, |p: &Pe| p.opt + 8);
    scalar!(uninit_size, set_uninit_size, r32, w32, u32, |p: &Pe| p.opt + 12);
    scalar!(entry, set_entry, r32, w32, u32, |p: &Pe| p.opt + 16);
    scalar!(sect_align, set_sect_align, r32, w32, u32, |p: &Pe| p.opt + 32);
    scalar!(file_align, set_file_align, r32, w32, u32, |p: &Pe| p.opt + 36);
    scalar!(image_size, set_image_size, r32, w32, u32, |p: &Pe| p.opt + 56);
    scalar!(headers_size, set_headers_size, r32, w32, u32, |p: &Pe| p.opt + 60);
    scalar!(checksum, set_checksum, r32, w32, u32, |p: &Pe| p.opt + 64);
    scalar!(subsystem, set_subsystem, r16, w16, u16, |p: &Pe| p.opt + 68);
    scalar!(dll_chars, set_dll_chars, r16, w16, u16, |p: &Pe| p.opt + 70);

    pub fn image_base(&self) -> u64 {
        if self.p64 {
            u64::from_le_bytes(self.d[self.opt + 24..self.opt + 32].try_into().unwrap())
        } else {
            self.r32(self.opt + 28) as u64
        }
    }

    pub fn set_image_base(&mut self, v: u64) {
        if self.p64 {
            self.d[self.opt + 24..self.opt + 32].copy_from_slice(&v.to_le_bytes());
        } else {
            self.w32(self.opt + 28, v as u32);
        }
    }

    pub fn laa(&self) -> bool {
        self.chars() & LAA != 0
    }

    pub fn set_laa(&mut self, on: bool) {
        let c = self.chars();
        self.set_chars(if on { c | LAA } else { c & !LAA });
    }

    pub fn aslr(&self) -> bool {
        self.dll_chars() & DYNAMIC_BASE != 0
    }

    pub fn set_aslr(&mut self, on: bool) {
        let c = self.dll_chars();
        self.set_dll_chars(if on { c | DYNAMIC_BASE } else { c & !DYNAMIC_BASE });
    }

    pub fn dep(&self) -> bool {
        self.dll_chars() & NX_COMPAT != 0
    }

    pub fn set_dep(&mut self, on: bool) {
        let c = self.dll_chars();
        self.set_dll_chars(if on { c | NX_COMPAT } else { c & !NX_COMPAT });
    }

    pub fn dir(&self, i: usize) -> (u32, u32) {
        if i >= self.dir_count() {
            return (0, 0);
        }
        let o = self.dir_base() + i * 8;
        (self.r32(o), self.r32(o + 4))
    }

    pub fn set_dir(&mut self, i: usize, rva: u32, size: u32) -> bool {
        if i >= self.dir_count() {
            return false;
        }
        let o = self.dir_base() + i * 8;
        self.w32(o, rva);
        self.w32(o + 4, size);
        true
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::pe::testutil::sample;

    #[test]
    fn flags_and_fields() {
        let mut p = Pe::parse(sample()).unwrap();
        assert_eq!(p.image_base(), 0x40_0000);
        assert_eq!(p.entry(), 0x1000);
        assert!(!p.laa());
        p.set_laa(true);
        assert!(p.laa() && p.chars() & 0x100 != 0);
        p.set_laa(false);
        assert!(!p.laa());
        p.set_aslr(true);
        p.set_dep(true);
        assert!(p.aslr() && p.dep());
        p.set_aslr(false);
        assert!(!p.aslr() && p.dep());
        assert_eq!(p.dir(2), (0, 0));
        assert!(p.set_dir(2, 5, 6) && p.dir(2) == (5, 6));
        assert!(!p.set_dir(40, 1, 1) && p.dir(40) == (0, 0));
        p.set_image_base(0x1000_0000);
        assert_eq!(p.image_base(), 0x1000_0000);
    }
}
