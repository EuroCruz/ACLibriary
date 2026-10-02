pub fn adler32(d: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for c in d.chunks(5552) {
        for &x in c {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

pub fn djb2(d: &[u8]) -> u32 {
    d.iter().fold(5381u32, |h, &c| h.wrapping_mul(33).wrapping_add(c as u32))
}

pub fn sdbm(d: &[u8]) -> u32 {
    d.iter().fold(0u32, |h, &c| (c as u32).wrapping_add(h << 6).wrapping_add(h << 16).wrapping_sub(h))
}

pub fn oaat(d: &[u8]) -> u32 {
    let mut h = d.iter().fold(0u32, |mut h, &c| {
        h = h.wrapping_add(c as u32);
        h = h.wrapping_add(h << 10);
        h ^ (h >> 6)
    });
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h.wrapping_add(h << 15)
}

pub fn murmur3(d: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    let mut h = seed;
    let mut it = d.chunks_exact(4);
    for c in &mut it {
        let k = u32::from_le_bytes(c.try_into().unwrap()).wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
        h = (h ^ k).rotate_left(13).wrapping_mul(5).wrapping_add(0xe654_6b64);
    }
    let r = it.remainder();
    if !r.is_empty() {
        let k = r.iter().rev().fold(0u32, |k, &b| (k << 8) | b as u32);
        h ^= k.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
    }
    h ^= d.len() as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn vectors() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(djb2(b"hello"), 0x0F92_3099);
        assert_eq!(oaat(b"a"), 0xCA2E_9442);
        assert_eq!(oaat(b"The quick brown fox jumps over the lazy dog"), 0x519E_91F5);
        assert_eq!(murmur3(b"", 0), 0);
        assert_eq!(murmur3(b"", 1), 0x514E_28B7);
        assert_eq!(murmur3(b"test", 0), 0xBA6B_D213);
        assert_eq!(murmur3(b"Hello, world!", 0), 0xC036_3E43);
    }
}
