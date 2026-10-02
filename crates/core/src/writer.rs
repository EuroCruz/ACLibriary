use crate::{Endian, Num};

#[derive(Clone, Default)]
pub struct Writer {
    v: Vec<u8>,
    pub e: Endian,
}

macro_rules! putters {
    ($($n:ident: $t:ty),*) => {$(
        pub fn $n(&mut self, x: $t) -> &mut Self {
            self.put(x)
        }
    )*};
}

impl Writer {
    pub fn new(e: Endian) -> Self {
        Writer { v: Vec::new(), e }
    }

    pub fn pos(&self) -> usize {
        self.v.len()
    }

    pub fn put<T: Num>(&mut self, x: T) -> &mut Self {
        x.wr(self.e, &mut self.v);
        self
    }

    putters!(u8: u8, i8: i8, u16: u16, i16: i16, u32: u32, i32: i32, u64: u64, i64: i64, f32: f32, f64: f64);

    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.v.extend_from_slice(b);
        self
    }

    pub fn cstr(&mut self, s: &[u8]) -> &mut Self {
        self.bytes(s).u8(0)
    }

    pub fn pad(&mut self, n: usize) -> &mut Self {
        self.v.resize(self.v.len() + n, 0);
        self
    }

    pub fn align(&mut self, n: usize) -> &mut Self {
        self.v.resize(self.v.len().next_multiple_of(n), 0);
        self
    }

    pub fn set<T: Num>(&mut self, at: usize, x: T) -> &mut Self {
        let mut t = Vec::with_capacity(T::N);
        x.wr(self.e, &mut t);
        self.v[at..at + T::N].copy_from_slice(&t);
        self
    }

    pub fn finish(self) -> Vec<u8> {
        self.v
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.v
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::Reader;

    #[test]
    fn writes() {
        let mut w = Writer::new(Endian::Be);
        w.u16(1).u32(0).cstr(b"ab").align(4).f32(2.0);
        let at = 2;
        w.set(at, 7u32);
        let v = w.finish();
        let mut r = Reader::new(&v, Endian::Be);
        assert_eq!(r.u16().unwrap(), 1);
        assert_eq!(r.u32().unwrap(), 7);
        assert_eq!(r.cstr().unwrap(), b"ab");
        r.align(4).unwrap();
        assert_eq!(r.f32().unwrap(), 2.0);
    }
}
