use crate::pe::Pe;
use ac_core::{bad, Res};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub va: u32,
    pub vsize: u32,
    pub ptr: u32,
    pub rsize: u32,
    pub flags: u32,
}

pub const CODE: u32 = 0x20;
pub const INIT: u32 = 0x40;
pub const UNINIT: u32 = 0x80;

fn up(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

impl Section {
    pub fn end_va(&self) -> u32 {
        self.va + self.vsize.max(self.rsize)
    }
}

impl Pe {
    pub fn sections(&self) -> Vec<Section> {
        (0..self.sec_count())
            .map(|i| {
                let o = self.sec_table() + i * 40;
                let n = &self.d[o..o + 8];
                Section {
                    name: String::from_utf8_lossy(&n[..n.iter().position(|&b| b == 0).unwrap_or(8)]).into_owned(),
                    vsize: self.r32(o + 8),
                    va: self.r32(o + 12),
                    rsize: self.r32(o + 16),
                    ptr: self.r32(o + 20),
                    flags: self.r32(o + 36),
                }
            })
            .collect()
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.sections().iter().position(|s| s.name == name)
    }

    pub fn rva_to_off(&self, rva: u32) -> Option<usize> {
        if rva < self.headers_size() {
            return Some(rva as usize);
        }
        self.sections().iter().find(|s| rva >= s.va && rva - s.va < s.rsize && rva - s.va < s.vsize.max(s.rsize)).map(|s| (s.ptr + (rva - s.va)) as usize)
    }

    pub fn off_to_rva(&self, off: usize) -> Option<u32> {
        self.sections().iter().find(|s| off >= s.ptr as usize && off < (s.ptr + s.rsize) as usize).map(|s| s.va + (off as u32 - s.ptr))
    }

    pub fn slice(&self, rva: u32, len: usize) -> Option<&[u8]> {
        let o = self.rva_to_off(rva)?;
        self.d.get(o..o.checked_add(len)?)
    }

    pub fn cstr(&self, rva: u32) -> Option<String> {
        let o = self.rva_to_off(rva)?;
        let t = self.d.get(o..)?;
        Some(String::from_utf8_lossy(&t[..t.iter().position(|&b| b == 0)?]).into_owned())
    }

    pub fn poke(&mut self, rva: u32, b: &[u8]) -> bool {
        match self.rva_to_off(rva) {
            Some(o) if o + b.len() <= self.d.len() => {
                self.d[o..o + b.len()].copy_from_slice(b);
                true
            }
            _ => false,
        }
    }

    pub fn rename(&mut self, i: usize, name: &str) -> Res<()> {
        if i >= self.sec_count() || name.len() > 8 {
            return bad("bad section name or index");
        }
        let o = self.sec_table() + i * 40;
        self.d[o..o + 8].fill(0);
        self.d[o..o + name.len()].copy_from_slice(name.as_bytes());
        Ok(())
    }

    pub fn set_flags(&mut self, i: usize, flags: u32) -> Res<()> {
        if i >= self.sec_count() {
            return bad("bad section index");
        }
        let o = self.sec_table() + i * 40 + 36;
        self.w32(o, flags);
        Ok(())
    }

    pub fn overlay(&self) -> &[u8] {
        let end = self.sections().iter().map(|s| (s.ptr + s.rsize) as usize).max().unwrap_or(self.headers_size() as usize);
        self.d.get(end..).unwrap_or(&[])
    }

    pub fn refresh(&mut self) {
        let s = self.sections();
        let image = up(s.iter().map(Section::end_va).max().unwrap_or(0).max(self.headers_size()), self.sect_align().max(1));
        self.set_image_size(image);
        let sum = |m: u32| s.iter().filter(|x| x.flags & m != 0).map(|x| x.rsize).sum::<u32>();
        self.set_code_size(sum(CODE));
        self.set_init_size(sum(INIT));
        self.set_uninit_size(s.iter().filter(|x| x.flags & UNINIT != 0 && x.flags & INIT == 0).map(|x| x.vsize).sum());
    }

    pub fn remove_last(&mut self) -> Res<Section> {
        let s = self.sections();
        let Some(last) = s.last().cloned() else { return bad("no sections") };
        if s.iter().any(|x| x.va > last.va) {
            return bad("last entry is not the highest section");
        }
        for i in 0..16 {
            let (rva, size) = self.dir(i);
            if rva != 0 && size != 0 && rva >= last.va && rva < last.end_va() {
                return bad("data directory points into the section");
            }
        }
        let o = self.sec_table() + (s.len() - 1) * 40;
        self.d[o..o + 40].fill(0);
        let n = (s.len() - 1) as u16;
        self.w16(self.pe + 6, n);
        let end = (last.ptr + last.rsize) as usize;
        if last.rsize > 0 && end <= self.d.len() && s[..s.len() - 1].iter().all(|x| x.ptr + x.rsize <= last.ptr) {
            self.d.drain(last.ptr as usize..end);
        }
        self.refresh();
        Ok(last)
    }

    pub fn add_section(&mut self, name: &str, data: &[u8], flags: u32) -> Res<usize> {
        if name.len() > 8 {
            return bad("section name too long");
        }
        let s = self.sections();
        let o = self.sec_table() + s.len() * 40;
        let first = s.iter().filter(|x| x.ptr > 0).map(|x| x.ptr).min().unwrap_or(self.headers_size());
        if o + 40 > first as usize {
            return bad("no room for another section header");
        }
        let (fa, sa) = (self.file_align().max(1), self.sect_align().max(1));
        let va = up(s.iter().map(Section::end_va).max().unwrap_or(0).max(self.headers_size()), sa);
        let end = s.iter().map(|x| x.ptr + x.rsize).max().unwrap_or(self.headers_size());
        let ptr = up(end, fa);
        let rsize = up(data.len() as u32, fa);
        let tail = self.d.split_off((end as usize).min(self.d.len()));
        self.d.resize(ptr as usize, 0);
        self.d.extend_from_slice(data);
        self.d.resize((ptr + rsize) as usize, 0);
        self.d.extend_from_slice(&tail);
        self.d[o..o + 40].fill(0);
        self.d[o..o + name.len()].copy_from_slice(name.as_bytes());
        self.w32(o + 8, data.len() as u32);
        self.w32(o + 12, va);
        self.w32(o + 16, rsize);
        self.w32(o + 20, ptr);
        self.w32(o + 36, flags);
        self.w16(self.pe + 6, s.len() as u16 + 1);
        self.refresh();
        Ok(s.len())
    }

    pub fn realign(&mut self, fa: u32) -> Res<()> {
        if !fa.is_power_of_two() || !(0x200..=0x1000).contains(&fa) || fa > self.sect_align() {
            return bad("bad file alignment");
        }
        let s = self.sections();
        let tab_end = (self.sec_table() + s.len() * 40) as u32;
        let hdr = up(tab_end, fa);
        let overlay = self.overlay().to_vec();
        let mut out = self.d[..(tab_end as usize).min(self.d.len())].to_vec();
        out.resize(hdr as usize, 0);
        let mut order: Vec<usize> = (0..s.len()).filter(|&i| s[i].ptr > 0 && s[i].rsize > 0).collect();
        order.sort_by_key(|&i| s[i].ptr);
        let mut upd = Vec::new();
        for &i in &order {
            let x = &s[i];
            let raw = self.d.get(x.ptr as usize..(x.ptr + x.rsize) as usize).ok_or(ac_core::Error::Bad("section outside file"))?;
            let want = up(x.vsize, fa) as usize;
            let keep = if want < raw.len() && raw[want..].iter().all(|&b| b == 0) { want } else { raw.len() };
            let size = up(keep as u32, fa);
            upd.push((i, out.len() as u32, size));
            out.extend_from_slice(&raw[..keep]);
            out.resize(out.len() + (size as usize - keep), 0);
        }
        out.extend_from_slice(&overlay);
        self.d = out;
        for (i, ptr, size) in upd {
            let o = self.sec_table() + i * 40;
            self.w32(o + 16, size);
            self.w32(o + 20, ptr);
        }
        self.set_file_align(fa);
        self.set_headers_size(hdr);
        self.refresh();
        Ok(())
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::pe::testutil::sample;

    #[test]
    fn map_addresses() {
        let p = Pe::parse(sample()).unwrap();
        let s = p.sections();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, ".text");
        assert_eq!(p.rva_to_off(0x1000), Some(s[0].ptr as usize));
        assert_eq!(p.off_to_rva(s[1].ptr as usize + 4), Some(0x2004));
        assert_eq!(p.rva_to_off(0x5000), None);
        assert_eq!(p.slice(0x1000, 2), Some(&[0x90, 0xc3][..]));
        assert_eq!(p.cstr(0x2000).as_deref(), Some("hello"));
        assert_eq!(p.find(".data"), Some(1));
    }

    #[test]
    fn edit_and_remove() {
        let mut p = Pe::parse(sample()).unwrap();
        assert!(p.poke(0x1000, &[0xcc]));
        assert_eq!(p.slice(0x1000, 1), Some(&[0xcc][..]));
        p.rename(1, ".bind").unwrap();
        assert_eq!(p.sections()[1].name, ".bind");
        assert!(p.rename(0, "waytoolongname").is_err());
        let before = p.bytes().len();
        let gone = p.remove_last().unwrap();
        assert_eq!(gone.name, ".bind");
        assert_eq!(p.sections().len(), 1);
        assert_eq!(p.bytes().len(), before - gone.rsize as usize);
        assert_eq!(p.image_size(), 0x2000);
        assert!(Pe::parse(p.bytes().to_vec()).is_ok());
    }

    #[test]
    fn remove_refuses_referenced() {
        let mut p = Pe::parse(sample()).unwrap();
        p.set_dir(1, 0x2000, 8);
        assert!(p.remove_last().is_err());
    }

    #[test]
    fn add_and_realign() {
        let mut p = Pe::parse(sample()).unwrap();
        let i = p.add_section(".new", &[1, 2, 3], 0x6000_0020).unwrap();
        assert_eq!(i, 2);
        let s = p.sections();
        assert_eq!(s[2].va, 0x3000);
        assert_eq!(p.slice(0x3000, 3), Some(&[1, 2, 3][..]));
        assert_eq!(p.code_size(), s[0].rsize + s[2].rsize);
        let text = p.slice(0x1000, 2).unwrap().to_vec();
        p.realign(0x1000).unwrap();
        assert_eq!(p.file_align(), 0x1000);
        assert_eq!(p.slice(0x1000, 2).unwrap(), &text[..]);
        assert_eq!(p.slice(0x3000, 3), Some(&[1, 2, 3][..]));
        p.realign(0x200).unwrap();
        assert_eq!(p.slice(0x3000, 3), Some(&[1, 2, 3][..]));
        assert!(p.realign(0x300).is_err());
        assert!(p.overlay().is_empty());
    }
}
