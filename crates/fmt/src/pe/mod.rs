mod fields;
mod imports;
mod rsrc;
mod sections;
mod sum;

#[cfg(test)]
mod testutil;

pub use imports::{Export, ImpFn, Import};
pub use rsrc::{Id, Node, ICON, GROUP_ICON, MANIFEST, VERSION};
pub use sections::Section;
pub use sum::checksum;

use ac_core::{bad, Res};

#[derive(Clone)]
pub struct Pe {
    d: Vec<u8>,
    pe: usize,
    opt: usize,
    p64: bool,
}

impl Pe {
    pub fn parse(d: Vec<u8>) -> Res<Pe> {
        if d.len() < 0x40 || &d[..2] != b"MZ" {
            return bad("not an MZ file");
        }
        let pe = u32::from_le_bytes(d[0x3c..0x40].try_into().unwrap()) as usize;
        if d.get(pe..pe + 4) != Some(b"PE\0\0") {
            return bad("missing PE signature");
        }
        let opt = pe + 24;
        let magic = d.get(opt..opt + 2).ok_or(ac_core::Error::Eof { at: opt, need: 2 })?;
        let p64 = match u16::from_le_bytes([magic[0], magic[1]]) {
            0x10b => false,
            0x20b => true,
            _ => return bad("unknown optional header magic"),
        };
        let p = Pe { d, pe, opt, p64 };
        let need = p.sec_table() + 40 * p.sec_count();
        if need > p.d.len() || p.dir_base() + 8 * p.dir_count() > p.d.len() {
            return bad("truncated headers");
        }
        Ok(p)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.d
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.d
    }

    pub fn is64(&self) -> bool {
        self.p64
    }

    fn r16(&self, o: usize) -> u16 {
        u16::from_le_bytes(self.d[o..o + 2].try_into().unwrap())
    }

    fn r32(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.d[o..o + 4].try_into().unwrap())
    }

    fn w16(&mut self, o: usize, v: u16) {
        self.d[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn w32(&mut self, o: usize, v: u32) {
        self.d[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn dir_base(&self) -> usize {
        self.opt + if self.p64 { 112 } else { 96 }
    }

    fn dir_count(&self) -> usize {
        (self.r32(self.dir_base() - 4) as usize).min(16)
    }

    fn sec_count(&self) -> usize {
        self.r16(self.pe + 6) as usize
    }

    fn sec_table(&self) -> usize {
        self.opt + self.r16(self.pe + 20) as usize
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::pe::testutil::sample;

    #[test]
    fn parses_and_rejects() {
        let p = Pe::parse(sample()).unwrap();
        assert!(!p.is64());
        assert!(Pe::parse(vec![0; 10]).is_err());
        let mut d = sample();
        d[0x3c] = 0xff;
        assert!(Pe::parse(d).is_err());
        let mut d = sample();
        let o = p.opt;
        d[o] = 0;
        assert!(Pe::parse(d).is_err());
    }

    #[test]
    #[ignore]
    fn real_file() {
        let Ok(path) = std::env::var("AC_PE_SAMPLE") else { return };
        let mut p = Pe::parse(std::fs::read(path).unwrap()).unwrap();
        assert_eq!(p.calc_checksum(), p.checksum());
        let (imps, res) = (p.imports(), p.resources().unwrap());
        println!("sections {:?}", p.sections().iter().map(|s| s.name.clone()).collect::<Vec<_>>());
        println!("imports {:?}", imps.iter().map(|i| (i.dll.clone(), i.funcs.len())).collect::<Vec<_>>());
        println!("resource types {:?} laa {}", res.ids(), p.laa());
        let before: Vec<Vec<u8>> = p.sections().iter().map(|s| p.slice(s.va, s.vsize.min(s.rsize) as usize).unwrap().to_vec()).collect();
        p.set_resources(&res).unwrap();
        assert_eq!(p.resources(), Some(res));
        for fa in [0x1000, 0x200] {
            p.realign(fa).unwrap();
            let after: Vec<Vec<u8>> = p.sections().iter().map(|s| p.slice(s.va, s.vsize.min(s.rsize) as usize).unwrap().to_vec()).collect();
            assert_eq!(after[..3], before[..3]);
        }
        assert!(Pe::parse(p.bytes().to_vec()).is_ok());
    }
}