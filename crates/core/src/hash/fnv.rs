#[derive(Clone, Copy)]
pub struct Fnv {
    pub basis: u32,
    pub prime: u32,
    pub fold: u8,
    pub salt: Option<u8>,
}

pub const FNV1A: Fnv = Fnv { basis: 0x811C_9DC5, prime: 0x0100_0193, fold: 0, salt: None };

impl Fnv {
    pub const fn lower(self) -> Fnv {
        Fnv { fold: 0x20, ..self }
    }

    pub const fn salted(self, s: u8) -> Fnv {
        Fnv { salt: Some(s), ..self }
    }

    pub fn hash(&self, s: &[u8]) -> u32 {
        let mut h = s.iter().fold(self.basis, |h, &c| ((c | self.fold) as u32 ^ h).wrapping_mul(self.prime));
        if let Some(x) = self.salt {
            h = (h ^ x as u32).wrapping_mul(self.prime);
        }
        h
    }
}

pub fn fnv64(s: &[u8]) -> u64 {
    s.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &c| (h ^ c as u64).wrapping_mul(0x0000_0100_0000_01B3))
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn vectors() {
        assert_eq!(FNV1A.hash(b""), 0x811C_9DC5);
        assert_eq!(FNV1A.hash(b"a"), 0xE40C_292C);
        assert_eq!(FNV1A.hash(b"foobar"), 0xBF9C_F968);
        assert_eq!(fnv64(b"a"), 0xAF63_DC4C_8601_EC8C);
    }

    #[test]
    fn folded_and_salted() {
        let p = FNV1A.lower().salted(0x2A);
        assert_eq!(p.hash(b"foobar"), (FNV1A.hash(b"foobar") ^ 0x2A).wrapping_mul(0x0100_0193));
        assert_ne!(p.hash(b"foobar"), FNV1A.hash(b"foobar"));
        assert_eq!(p.hash(b"FooBar"), p.hash(b"foobar"));
    }
}
