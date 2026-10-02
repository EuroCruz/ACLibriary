#[derive(Clone, Copy)]
pub struct Crc {
    t: [u32; 256],
    init: u32,
    xor: u32,
    rev: bool,
}

impl Crc {
    pub const fn new(poly: u32, init: u32, xor: u32, rev: bool) -> Crc {
        let p = if rev { poly.reverse_bits() } else { poly };
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = if rev { i as u32 } else { (i as u32) << 24 };
            let mut k = 0;
            while k < 8 {
                c = if rev {
                    if c & 1 != 0 { (c >> 1) ^ p } else { c >> 1 }
                } else if c & 0x8000_0000 != 0 {
                    (c << 1) ^ p
                } else {
                    c << 1
                };
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        Crc { t, init, xor, rev }
    }

    pub fn start(&self) -> u32 {
        self.init
    }

    pub fn feed(&self, mut c: u32, d: &[u8]) -> u32 {
        for &b in d {
            c = if self.rev {
                self.t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8)
            } else {
                self.t[((c >> 24) ^ b as u32) as usize & 0xff] ^ (c << 8)
            };
        }
        c
    }

    pub fn end(&self, c: u32) -> u32 {
        c ^ self.xor
    }

    pub fn sum(&self, d: &[u8]) -> u32 {
        self.end(self.feed(self.init, d))
    }
}

pub const IEEE: Crc = Crc::new(0x04C1_1DB7, 0xFFFF_FFFF, 0xFFFF_FFFF, true);
pub const BZIP2: Crc = Crc::new(0x04C1_1DB7, 0xFFFF_FFFF, 0xFFFF_FFFF, false);
pub const MPEG2: Crc = Crc::new(0x04C1_1DB7, 0xFFFF_FFFF, 0, false);
pub const JAMCRC: Crc = Crc::new(0x04C1_1DB7, 0xFFFF_FFFF, 0, true);
pub const CASTAGNOLI: Crc = Crc::new(0x1EDC_6F41, 0xFFFF_FFFF, 0xFFFF_FFFF, true);

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn check_values() {
        let d = b"123456789";
        assert_eq!(IEEE.sum(d), 0xCBF4_3926);
        assert_eq!(BZIP2.sum(d), 0xFC89_1918);
        assert_eq!(MPEG2.sum(d), 0x0376_E6E7);
        assert_eq!(JAMCRC.sum(d), 0x340B_C6D9);
        assert_eq!(CASTAGNOLI.sum(d), 0xE306_9283);
    }

    #[test]
    fn streaming() {
        let c = IEEE.feed(IEEE.feed(IEEE.start(), b"1234"), b"56789");
        assert_eq!(IEEE.end(c), 0xCBF4_3926);
    }
}
