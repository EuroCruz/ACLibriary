use ac_core::{bad, Error, Res};

const MIN: usize = 4;
const TAIL: usize = 12;

fn ext(s: &[u8], p: &mut usize) -> Res<usize> {
    let mut n = 0usize;
    loop {
        let b = *s.get(*p).ok_or(Error::Eof { at: *p, need: 1 })?;
        *p += 1;
        n = n.checked_add(b as usize).ok_or(Error::Bad("length overflow"))?;
        if b != 255 {
            return Ok(n);
        }
    }
}

pub fn decode(s: &[u8], max: usize) -> Res<Vec<u8>> {
    let mut o = Vec::with_capacity(max.min(1 << 26));
    let mut p = 0;
    while p < s.len() {
        let t = s[p];
        p += 1;
        let mut lit = (t >> 4) as usize;
        if lit == 15 {
            lit += ext(s, &mut p)?;
        }
        let l = s.get(p..p + lit).ok_or(Error::Eof { at: p, need: lit })?;
        if o.len() + lit > max {
            return bad("output limit");
        }
        o.extend_from_slice(l);
        p += lit;
        if p == s.len() {
            break;
        }
        let off = s.get(p..p + 2).ok_or(Error::Eof { at: p, need: 2 })?;
        let off = u16::from_le_bytes([off[0], off[1]]) as usize;
        p += 2;
        if off == 0 || off > o.len() {
            return bad("bad offset");
        }
        let mut m = (t & 15) as usize;
        if m == 15 {
            m += ext(s, &mut p)?;
        }
        m += MIN;
        if o.len() + m > max {
            return bad("output limit");
        }
        let st = o.len() - off;
        for k in 0..m {
            o.push(o[st + k]);
        }
    }
    Ok(o)
}

fn put_len(o: &mut Vec<u8>, mut n: usize) {
    while n >= 255 {
        o.push(255);
        n -= 255;
    }
    o.push(n as u8);
}

fn seq(o: &mut Vec<u8>, lit: &[u8], m: Option<(usize, usize)>) {
    let ml = m.map_or(0, |(_, l)| l - MIN);
    o.push((lit.len().min(15) as u8) << 4 | ml.min(15) as u8);
    if lit.len() >= 15 {
        put_len(o, lit.len() - 15);
    }
    o.extend_from_slice(lit);
    if let Some((off, _)) = m {
        o.extend_from_slice(&(off as u16).to_le_bytes());
        if ml >= 15 {
            put_len(o, ml - 15);
        }
    }
}

pub fn encode(s: &[u8]) -> Vec<u8> {
    let n = s.len();
    let mut o = Vec::with_capacity(n / 2 + 16);
    let mut tbl = vec![u32::MAX; 1 << 16];
    let at = |i: usize| u32::from_le_bytes(s[i..i + 4].try_into().unwrap());
    let (mut i, mut anchor) = (0, 0);
    while n >= TAIL && i + TAIL <= n {
        let h = (at(i).wrapping_mul(2_654_435_761) >> 16) as usize;
        let c = tbl[h];
        tbl[h] = i as u32;
        if c != u32::MAX && i - c as usize <= 65535 && at(c as usize) == at(i) {
            let c = c as usize;
            let lim = n - 5;
            let mut l = MIN;
            while i + l < lim && s[c + l] == s[i + l] {
                l += 1;
            }
            seq(&mut o, &s[anchor..i], Some((i - c, l)));
            i += l;
            anchor = i;
        } else {
            i += 1;
        }
    }
    seq(&mut o, &s[anchor..], None);
    o
}

pub fn pack(s: &[u8]) -> Vec<u8> {
    let mut o = (s.len() as u32).to_le_bytes().to_vec();
    o.extend(encode(s));
    o
}

pub fn unpack(s: &[u8]) -> Res<Vec<u8>> {
    let h = s.get(..4).ok_or(Error::Eof { at: 0, need: 4 })?;
    let n = u32::from_le_bytes(h.try_into().unwrap()) as usize;
    let v = decode(&s[4..], n)?;
    if v.len() != n {
        return bad("size mismatch");
    }
    Ok(v)
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn vectors() {
        assert_eq!(decode(&[0x50, b'h', b'e', b'l', b'l', b'o'], 5).unwrap(), b"hello");
        assert_eq!(decode(&[0x11, b'a', 1, 0, 0x10, b'b'], 16).unwrap(), b"aaaaaab");
        assert!(decode(&[0x11, b'a', 2, 0, 0x10, b'b'], 16).is_err());
        assert!(decode(&[0x11, b'a', 1, 0, 0x10, b'b'], 3).is_err());
        assert!(decode(&[0x50, b'h'], 5).is_err());
    }

    #[test]
    fn roundtrip() {
        let mut s = 7u32;
        for n in [0usize, 1, 11, 12, 13, 100, 70_000, 300_000] {
            let d: Vec<u8> = (0..n)
                .map(|i| {
                    s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                    if i % 5 < 3 { (i % 17) as u8 } else { (s >> 24) as u8 % 4 }
                })
                .collect();
            assert_eq!(unpack(&pack(&d)).unwrap(), d, "n={n}");
        }
        let rep = vec![9u8; 100_000];
        assert!(encode(&rep).len() < 600);
    }
}
