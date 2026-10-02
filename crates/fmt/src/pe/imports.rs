use crate::pe::Pe;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImpFn {
    pub name: Option<String>,
    pub ordinal: Option<u16>,
    pub iat: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub dll: String,
    pub funcs: Vec<ImpFn>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub name: Option<String>,
    pub ordinal: u32,
    pub rva: u32,
}

impl Pe {
    fn word(&self, rva: u32) -> Option<u64> {
        let b = self.slice(rva, if self.p64 { 8 } else { 4 })?;
        Some(if self.p64 { u64::from_le_bytes(b.try_into().unwrap()) } else { u32::from_le_bytes(b.try_into().unwrap()) as u64 })
    }

    pub fn imports(&self) -> Vec<Import> {
        let (rva, _) = self.dir(1);
        if rva == 0 {
            return Vec::new();
        }
        let (w, flag) = if self.p64 { (8u32, 1u64 << 63) } else { (4, 1u64 << 31) };
        let mut out = Vec::new();
        for i in 0..4096u32 {
            let Some(d) = self.slice(rva + i * 20, 20) else { break };
            let f = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
            if d.iter().all(|&b| b == 0) {
                break;
            }
            let (oft, name, ft) = (f(0), f(12), f(16));
            let Some(dll) = self.cstr(name) else { break };
            let table = if oft != 0 { oft } else { ft };
            let mut funcs = Vec::new();
            for k in 0..65536u32 {
                match self.word(table + k * w) {
                    Some(0) | None => break,
                    Some(v) if v & flag != 0 => funcs.push(ImpFn { name: None, ordinal: Some(v as u16), iat: ft + k * w }),
                    Some(v) => funcs.push(ImpFn { name: self.cstr(v as u32 + 2), ordinal: None, iat: ft + k * w }),
                }
            }
            out.push(Import { dll, funcs });
        }
        out
    }

    pub fn exports(&self) -> Vec<Export> {
        let (rva, _) = self.dir(0);
        if rva == 0 {
            return Vec::new();
        }
        let Some(d) = self.slice(rva, 40) else { return Vec::new() };
        let f = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
        let (base, nf, nn, af, an, ao) = (f(16), f(20).min(65536), f(24).min(65536), f(28), f(32), f(36));
        let names: Vec<(u16, String)> = (0..nn)
            .filter_map(|i| {
                let n = u32::from_le_bytes(self.slice(an + i * 4, 4)?.try_into().unwrap());
                let o = u16::from_le_bytes(self.slice(ao + i * 2, 2)?.try_into().unwrap());
                Some((o, self.cstr(n)?))
            })
            .collect();
        (0..nf)
            .filter_map(|i| {
                let r = u32::from_le_bytes(self.slice(af + i * 4, 4)?.try_into().unwrap());
                Some(Export { name: names.iter().find(|(o, _)| *o as u32 == i).map(|(_, n)| n.clone()), ordinal: base + i, rva: r })
            })
            .collect()
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::pe::testutil::{sample, with_tables};

    #[test]
    fn reads_imports() {
        let p = Pe::parse(with_tables()).unwrap();
        let i = p.imports();
        assert_eq!(i.len(), 1);
        assert_eq!(i[0].dll, "kernel32.dll");
        assert_eq!(i[0].funcs[0], ImpFn { name: Some("ExitProcess".into()), ordinal: None, iat: 0x2040 });
        assert_eq!(i[0].funcs[1], ImpFn { name: None, ordinal: Some(5), iat: 0x2044 });
        assert!(Pe::parse(sample()).unwrap().imports().is_empty());
    }

    #[test]
    fn reads_exports() {
        let e = Pe::parse(with_tables()).unwrap().exports();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0], Export { name: Some("Foo".into()), ordinal: 1, rva: 0x1000 });
        assert_eq!(e[1], Export { name: Some("Bar".into()), ordinal: 2, rva: 0x1001 });
        assert!(Pe::parse(sample()).unwrap().exports().is_empty());
    }
}
