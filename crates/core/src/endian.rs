#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Endian {
    #[default]
    Le,
    Be,
}

pub trait Num: Copy {
    const N: usize;
    fn rd(e: Endian, b: &[u8]) -> Self;
    fn wr(self, e: Endian, o: &mut Vec<u8>);
}

macro_rules! num {
    ($($t:ty),*) => {$(
        impl Num for $t {
            const N: usize = std::mem::size_of::<$t>();
            fn rd(e: Endian, b: &[u8]) -> Self {
                let a = b[..Self::N].try_into().unwrap();
                match e {
                    Endian::Le => <$t>::from_le_bytes(a),
                    Endian::Be => <$t>::from_be_bytes(a),
                }
            }
            fn wr(self, e: Endian, o: &mut Vec<u8>) {
                o.extend_from_slice(&match e {
                    Endian::Le => self.to_le_bytes(),
                    Endian::Be => self.to_be_bytes(),
                })
            }
        }
    )*};
}

num!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

impl Endian {
    pub fn get<T: Num>(self, b: &[u8]) -> Option<T> {
        (b.len() >= T::N).then(|| T::rd(self, b))
    }

    pub fn put<T: Num>(self, v: T) -> Vec<u8> {
        let mut o = Vec::with_capacity(T::N);
        v.wr(self, &mut o);
        o
    }

    pub fn flip(self) -> Endian {
        match self {
            Endian::Le => Endian::Be,
            Endian::Be => Endian::Le,
        }
    }
}

pub fn swap16(b: &mut [u8]) {
    b.chunks_exact_mut(2).for_each(|c| c.swap(0, 1));
}

pub fn swap32(b: &mut [u8]) {
    b.chunks_exact_mut(4).for_each(|c| c.reverse());
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn roundtrip() {
        for e in [Endian::Le, Endian::Be] {
            assert_eq!(e.get::<u32>(&e.put(0xdead_beefu32)), Some(0xdead_beef));
            assert_eq!(e.get::<f32>(&e.put(1.5f32)), Some(1.5));
            assert_eq!(e.get::<i16>(&e.put(-2i16)), Some(-2));
        }
        assert_eq!(Endian::Be.put(1u16), [0, 1]);
        assert_eq!(Endian::Le.get::<u32>(&[1, 2]), None);
    }

    #[test]
    fn swaps() {
        let mut b = [1, 2, 3, 4, 5];
        swap16(&mut b);
        assert_eq!(b, [2, 1, 4, 3, 5]);
        swap32(&mut b);
        assert_eq!(b, [3, 4, 1, 2, 5]);
    }
}
