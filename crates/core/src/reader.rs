use crate::{bad, Endian, Error, Num, Res};

#[derive(Clone)]
pub struct Reader<'a> {
    d: &'a [u8],
    p: usize,
    pub e: Endian,
}

macro_rules! getters {
    ($($n:ident: $t:ty),*) => {$(
        pub fn $n(&mut self) -> Res<$t> {
            self.get::<$t>()
        }
    )*};
}

impl<'a> Reader<'a> {
    pub fn new(d: &'a [u8], e: Endian) -> Self {
        Reader { d, p: 0, e }
    }

    pub fn at(d: &'a [u8], e: Endian, p: usize) -> Res<Self> {
        let mut r = Reader::new(d, e);
        r.seek(p)?;
        Ok(r)
    }

    pub fn pos(&self) -> usize {
        self.p
    }

    pub fn len(&self) -> usize {
        self.d.len()
    }

    pub fn is_empty(&self) -> bool {
        self.d.is_empty()
    }

    pub fn left(&self) -> usize {
        self.d.len() - self.p
    }

    pub fn seek(&mut self, p: usize) -> Res<()> {
        if p > self.d.len() {
            return Err(Error::Eof { at: p, need: 0 });
        }
        self.p = p;
        Ok(())
    }

    pub fn skip(&mut self, n: usize) -> Res<()> {
        self.take(n).map(drop)
    }

    pub fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self.p.checked_add(n).filter(|&x| x <= self.d.len());
        let end = end.ok_or(Error::Eof { at: self.p, need: n })?;
        let s = &self.d[self.p..end];
        self.p = end;
        Ok(s)
    }

    pub fn get<T: Num>(&mut self) -> Res<T> {
        let e = self.e;
        self.take(T::N).map(|b| T::rd(e, b))
    }

    pub fn peek<T: Num>(&self) -> Res<T> {
        self.clone().get()
    }

    getters!(u8: u8, i8: i8, u16: u16, i16: i16, u32: u32, i32: i32, u64: u64, i64: i64, f32: f32, f64: f64);

    pub fn arr<const N: usize>(&mut self) -> Res<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    pub fn magic(&mut self, m: &[u8]) -> Res<()> {
        if self.take(m.len())? == m {
            Ok(())
        } else {
            bad("magic")
        }
    }

    pub fn cstr(&mut self) -> Res<&'a [u8]> {
        let n = self.d[self.p..].iter().position(|&b| b == 0).ok_or(Error::Eof { at: self.d.len(), need: 1 })?;
        let s = self.take(n)?;
        self.p += 1;
        Ok(s)
    }

    pub fn text(&mut self, n: usize) -> Res<String> {
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    pub fn align(&mut self, n: usize) -> Res<()> {
        self.seek(self.p.next_multiple_of(n))
    }

    pub fn sub(&mut self, n: usize) -> Res<Reader<'a>> {
        Ok(Reader::new(self.take(n)?, self.e))
    }

    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.d[self.p..];
        self.p = self.d.len();
        s
    }

    pub fn list<T>(&mut self, n: usize, mut f: impl FnMut(&mut Self) -> Res<T>) -> Res<Vec<T>> {
        (0..n).map(|_| f(self)).collect()
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn reads() {
        let d = [1, 0, 2, 0, 0, 0, b'h', b'i', 0, 9];
        let mut r = Reader::new(&d, Endian::Le);
        assert_eq!(r.u16().unwrap(), 1);
        assert_eq!(r.u32().unwrap(), 2);
        assert_eq!(r.cstr().unwrap(), b"hi");
        assert_eq!(r.peek::<u8>().unwrap(), 9);
        assert_eq!(r.left(), 1);
        assert!(r.u16().is_err());
        assert_eq!(r.pos(), 9);
    }

    #[test]
    fn windows() {
        let d = [0, 1, 2, 3, 4, 5, 6, 7];
        let mut r = Reader::new(&d, Endian::Be);
        r.skip(1).unwrap();
        r.align(4).unwrap();
        assert_eq!(r.pos(), 4);
        let mut s = r.sub(2).unwrap();
        assert_eq!(s.u16().unwrap(), 0x0405);
        assert!(s.u8().is_err());
        assert_eq!(r.list(2, |r| r.u8()).unwrap(), [6, 7]);
        assert!(r.magic(b"x").is_err());
    }
}
