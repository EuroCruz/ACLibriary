const Q1: u64 = 0x9E37_79B1_85EB_CA87;
const Q2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const Q3: u64 = 0x1656_67B1_9E37_79F9;
const Q4: u64 = 0x85EB_CA77_C2B2_AE63;
const Q5: u64 = 0x27D4_EB2F_1656_67C5;
const P1: u32 = 0x9E37_79B1;
const P2: u32 = 0x85EB_CA77;
const P3: u32 = 0xC2B2_AE3D;
const P4: u32 = 0x27D4_EB2F;
const P5: u32 = 0x1656_67B1;

fn u64at(d: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(d[i..i + 8].try_into().unwrap())
}

fn u32at(d: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(d[i..i + 4].try_into().unwrap())
}

fn round64(v: u64, x: u64) -> u64 {
    v.wrapping_add(x.wrapping_mul(Q2)).rotate_left(31).wrapping_mul(Q1)
}

pub fn xxh64(d: &[u8], seed: u64) -> u64 {
    let n = d.len();
    let mut i = 0;
    let mut h = if n >= 32 {
        let mut v = [seed.wrapping_add(Q1).wrapping_add(Q2), seed.wrapping_add(Q2), seed, seed.wrapping_sub(Q1)];
        while i + 32 <= n {
            for (k, x) in v.iter_mut().enumerate() {
                *x = round64(*x, u64at(d, i + 8 * k));
            }
            i += 32;
        }
        let mut h = v[0].rotate_left(1).wrapping_add(v[1].rotate_left(7)).wrapping_add(v[2].rotate_left(12)).wrapping_add(v[3].rotate_left(18));
        for x in v {
            h = (h ^ round64(0, x)).wrapping_mul(Q1).wrapping_add(Q4);
        }
        h
    } else {
        seed.wrapping_add(Q5)
    };
    h = h.wrapping_add(n as u64);
    while i + 8 <= n {
        h = (h ^ round64(0, u64at(d, i))).rotate_left(27).wrapping_mul(Q1).wrapping_add(Q4);
        i += 8;
    }
    if i + 4 <= n {
        h = (h ^ (u32at(d, i) as u64).wrapping_mul(Q1)).rotate_left(23).wrapping_mul(Q2).wrapping_add(Q3);
        i += 4;
    }
    for &b in &d[i..] {
        h = (h ^ (b as u64).wrapping_mul(Q5)).rotate_left(11).wrapping_mul(Q1);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(Q2);
    h ^= h >> 29;
    h = h.wrapping_mul(Q3);
    h ^ (h >> 32)
}

pub fn xxh32(d: &[u8], seed: u32) -> u32 {
    let n = d.len();
    let mut i = 0;
    let round = |v: u32, x: u32| v.wrapping_add(x.wrapping_mul(P2)).rotate_left(13).wrapping_mul(P1);
    let mut h = if n >= 16 {
        let mut v = [seed.wrapping_add(P1).wrapping_add(P2), seed.wrapping_add(P2), seed, seed.wrapping_sub(P1)];
        while i + 16 <= n {
            for (k, x) in v.iter_mut().enumerate() {
                *x = round(*x, u32at(d, i + 4 * k));
            }
            i += 16;
        }
        v[0].rotate_left(1).wrapping_add(v[1].rotate_left(7)).wrapping_add(v[2].rotate_left(12)).wrapping_add(v[3].rotate_left(18))
    } else {
        seed.wrapping_add(P5)
    };
    h = h.wrapping_add(n as u32);
    while i + 4 <= n {
        h = h.wrapping_add(u32at(d, i).wrapping_mul(P3)).rotate_left(17).wrapping_mul(P4);
        i += 4;
    }
    for &b in &d[i..] {
        h = h.wrapping_add((b as u32).wrapping_mul(P5)).rotate_left(11).wrapping_mul(P1);
    }
    h ^= h >> 15;
    h = h.wrapping_mul(P2);
    h ^= h >> 13;
    h = h.wrapping_mul(P3);
    h ^ (h >> 16)
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn vectors() {
        assert_eq!(xxh64(b"", 0), 0xEF46_DB37_51D8_E999);
        assert_eq!(xxh64(b"abc", 0), 0x44BC_2CF5_AD77_0999);
        assert_eq!(xxh32(b"", 0), 0x02CC_5D05);
        assert_eq!(xxh32(b"abc", 0), 0x32D1_53FF);
    }
}
