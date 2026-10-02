pub struct Br<'a> {
    d: &'a [u8],
    pub pos: usize,
}

impl<'a> Br<'a> {
    pub fn new(d: &'a [u8]) -> Br<'a> {
        Br { d, pos: 0 }
    }

    pub fn bit(&mut self) -> Option<u32> {
        let b = *self.d.get(self.pos >> 3)?;
        let v = (b >> (self.pos & 7)) & 1;
        self.pos += 1;
        Some(v as u32)
    }

    pub fn get(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Some(v)
    }

    pub fn flag(&mut self) -> Option<bool> {
        self.bit().map(|b| b == 1)
    }
}

#[derive(Default)]
pub struct Bw {
    pub d: Vec<u8>,
    n: usize,
}

impl Bw {
    pub fn new() -> Bw {
        Bw::default()
    }

    pub fn put(&mut self, v: u32, n: u32) {
        for i in 0..n {
            if self.n & 7 == 0 {
                self.d.push(0);
            }
            if (v >> i) & 1 == 1 {
                *self.d.last_mut().unwrap() |= 1 << (self.n & 7);
            }
            self.n += 1;
        }
    }

    pub fn flag(&mut self, b: bool) {
        self.put(b as u32, 1);
    }
}

pub fn ilog(v: u32) -> u32 {
    32 - v.leading_zeros()
}

pub fn float32(x: u32) -> f32 {
    let m = (x & 0x1fffff) as f64;
    let e = ((x & 0x7fe00000) >> 21) as i32 - 788;
    let v = m * 2f64.powi(e);
    (if x & 0x8000_0000 != 0 { -v } else { v }) as f32
}

pub fn pack32(v: f32) -> u32 {
    if v == 0.0 {
        return 0;
    }
    let (s, mut a) = (v < 0.0, v.abs() as f64);
    let mut e = 0i32;
    while a >= (1 << 21) as f64 {
        a /= 2.0;
        e += 1;
    }
    while a < (1 << 20) as f64 {
        a *= 2.0;
        e -= 1;
    }
    (s as u32) << 31 | ((e + 788) as u32) << 21 | a.round() as u32 & 0x1fffff
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn bits_roundtrip() {
        let mut w = Bw::new();
        w.put(5, 3);
        w.put(0x1234, 13);
        w.flag(true);
        w.put(0xffff_ffff, 32);
        assert_eq!(w.d.len(), 7);
        let mut r = Br::new(&w.d);
        assert_eq!((r.get(3), r.get(13), r.flag(), r.get(32)), (Some(5), Some(0x1234), Some(true), Some(0xffff_ffff)));
        assert_eq!(r.get(16), None);
        assert_eq!((ilog(0), ilog(1), ilog(7), ilog(8)), (0, 1, 3, 4));
        for v in [1.0f32, -2.5, 0.0, 0.125, 123456.0, -0.75, -15.0] {
            assert_eq!(float32(pack32(v)), v, "{v}");
        }
    }
}
