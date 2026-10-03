use crate::flate::tables::*;
use ac_core::{bad, Error, Res};

struct Bits<'a> {
    d: &'a [u8],
    p: usize,
    acc: u64,
    n: u32,
}

impl Bits<'_> {
    fn fill(&mut self) {
        while self.n <= 56 {
            self.acc |= (self.d.get(self.p).copied().unwrap_or(0) as u64) << self.n;
            self.p += 1;
            self.n += 8;
        }
    }

    fn used(&self) -> usize {
        self.p - (self.n / 8) as usize
    }

    fn check(&self) -> Res<()> {
        if self.p * 8 - self.n as usize > self.d.len() * 8 {
            return Err(Error::Eof { at: self.d.len(), need: 1 });
        }
        Ok(())
    }

    fn get(&mut self, k: u32) -> Res<u32> {
        if self.n < k {
            self.fill();
        }
        let v = (self.acc & ((1u64 << k) - 1)) as u32;
        self.acc >>= k;
        self.n -= k;
        self.check()?;
        Ok(v)
    }

    fn sym(&mut self, t: &Tab) -> Res<usize> {
        if self.n < 15 {
            self.fill();
        }
        let e = t.t[(self.acc & t.mask) as usize];
        let k = (e & 15) as u32;
        if k == 0 {
            return bad("bad huffman code");
        }
        self.acc >>= k;
        self.n -= k;
        self.check()?;
        Ok((e >> 4) as usize)
    }

    fn align(&mut self) {
        let k = self.n % 8;
        self.acc >>= k;
        self.n -= k;
    }
}

struct Tab {
    t: Vec<u16>,
    mask: u64,
}

impl Tab {
    fn new(len: &[u8]) -> Res<Tab> {
        let max = len.iter().copied().max().unwrap_or(0) as u32;
        if max > 15 {
            return bad("code too long");
        }
        let mut count = [0u32; 16];
        len.iter().for_each(|&l| count[l as usize] += 1);
        count[0] = 0;
        let (mut code, mut next, mut left) = (0u32, [0u32; 16], 1i32);
        for l in 1..16 {
            left = (left << 1) - count[l] as i32;
            if left < 0 {
                return bad("over-subscribed code");
            }
            code = (code + count[l - 1]) << 1;
            next[l] = code;
        }
        let bits = max.max(1);
        let mut t = vec![0u16; 1 << bits];
        for (s, &l) in len.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            let r = (c.reverse_bits() >> (32 - l as u32)) as usize;
            for j in (r..t.len()).step_by(1 << l) {
                t[j] = (s as u16) << 4 | l as u16;
            }
        }
        Ok(Tab { t, mask: (1u64 << bits) - 1 })
    }
}

pub fn inflate(d: &[u8], hint: usize) -> Res<Vec<u8>> {
    inflate_max(d, hint, usize::MAX)
}

pub fn inflate_max(d: &[u8], hint: usize, max: usize) -> Res<Vec<u8>> {
    inflate_at(d, hint, max).map(|(v, _)| v)
}

pub(crate) fn inflate_at(d: &[u8], hint: usize, max: usize) -> Res<(Vec<u8>, usize)> {
    let mut b = Bits { d, p: 0, acc: 0, n: 0 };
    let mut out = Vec::with_capacity(hint.min(max));
    loop {
        let last = b.get(1)?;
        match b.get(2)? {
            0 => stored(&mut b, &mut out, max)?,
            1 => codes(&mut b, &mut out, &Tab::new(&fixed_lit())?, &Tab::new(&fixed_dist())?, max)?,
            2 => {
                let (l, di) = dynamic(&mut b)?;
                codes(&mut b, &mut out, &l, &di, max)?
            }
            _ => return bad("bad block type"),
        }
        if last == 1 {
            break;
        }
    }
    Ok((out, b.used()))
}

fn stored(b: &mut Bits, out: &mut Vec<u8>, max: usize) -> Res<()> {
    b.align();
    let (n, c) = (b.get(16)?, b.get(16)?);
    if n != !c & 0xffff {
        return bad("stored length");
    }
    if out.len() + n as usize > max {
        return bad("output limit");
    }
    let mut left = n as usize;
    while left > 0 && b.n >= 8 {
        out.push(b.get(8)? as u8);
        left -= 1;
    }
    let at = b.used();
    let s = b.d.get(at..at + left).ok_or(Error::Eof { at, need: left })?;
    out.extend_from_slice(s);
    *b = Bits { d: b.d, p: at + left, acc: 0, n: 0 };
    Ok(())
}

fn dynamic(b: &mut Bits) -> Res<(Tab, Tab)> {
    let nl = b.get(5)? as usize + 257;
    let nd = b.get(5)? as usize + 1;
    let nc = b.get(4)? as usize + 4;
    if nl > 286 || nd > 30 {
        return bad("bad counts");
    }
    let mut cl = [0u8; 19];
    for &i in CL_ORDER.iter().take(nc) {
        cl[i] = b.get(3)? as u8;
    }
    let cd = Tab::new(&cl)?;
    let mut len = vec![0u8; nl + nd];
    let mut i = 0;
    while i < nl + nd {
        let s = b.sym(&cd)?;
        if s < 16 {
            len[i] = s as u8;
            i += 1;
            continue;
        }
        let (v, r) = match s {
            16 => {
                if i == 0 {
                    return bad("repeat without previous");
                }
                (len[i - 1], 3 + b.get(2)?)
            }
            17 => (0, 3 + b.get(3)?),
            _ => (0, 11 + b.get(7)?),
        };
        if i + r as usize > nl + nd {
            return bad("repeat overflow");
        }
        len[i..i + r as usize].fill(v);
        i += r as usize;
    }
    if len[256] == 0 {
        return bad("missing end of block");
    }
    Ok((Tab::new(&len[..nl])?, Tab::new(&len[nl..])?))
}

fn codes(b: &mut Bits, out: &mut Vec<u8>, lit: &Tab, dist: &Tab, max: usize) -> Res<()> {
    loop {
        let s = b.sym(lit)?;
        if s < 256 {
            if out.len() >= max {
                return bad("output limit");
            }
            out.push(s as u8);
            continue;
        }
        if s == 256 {
            return Ok(());
        }
        let s = s - 257;
        if s >= 29 {
            return bad("bad length symbol");
        }
        let len = LEN_BASE[s] as usize + b.get(LEN_EXTRA[s] as u32)? as usize;
        let ds = b.sym(dist)?;
        if ds >= 30 {
            return bad("bad distance symbol");
        }
        let d = DIST_BASE[ds] as usize + b.get(DIST_EXTRA[ds] as u32)? as usize;
        if d > out.len() {
            return bad("distance too far");
        }
        if out.len() + len > max {
            return bad("output limit");
        }
        let (mut s, mut left) = (out.len() - d, len);
        while left > 0 {
            let k = left.min(out.len() - s);
            out.extend_from_within(s..s + k);
            s += k;
            left -= k;
        }
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn fixed_block() {
        assert_eq!(inflate(&[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00], 0).unwrap(), b"hello");
    }

    #[test]
    fn stored_block() {
        let d = [1, 3, 0, 0xfc, 0xff, b'a', b'b', b'c'];
        assert_eq!(inflate(&d, 0).unwrap(), b"abc");
        assert!(inflate(&[1, 3, 0, 0, 0, 1, 2, 3], 0).is_err());
    }

    #[test]
    fn limits_and_errors() {
        assert!(inflate_max(&[0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00], 0, 3).is_err());
        assert!(inflate(&[0xcb, 0x48], 0).is_err());
        assert!(inflate(&[0x07], 0).is_err());
    }
}
