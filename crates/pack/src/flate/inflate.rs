use crate::huff::Dec;
use crate::flate::tables::*;
use ac_core::{bad, Error, Res};

struct Bits<'a> {
    d: &'a [u8],
    p: usize,
    acc: u32,
    n: u32,
}

impl<'a> Bits<'a> {
    fn bit(&mut self) -> Res<u32> {
        Ok(self.get(1)?)
    }

    fn get(&mut self, k: u32) -> Res<u32> {
        while self.n < k {
            let b = *self.d.get(self.p).ok_or(Error::Eof { at: self.p, need: 1 })?;
            self.p += 1;
            self.acc |= (b as u32) << self.n;
            self.n += 8;
        }
        let v = self.acc & ((1u64 << k) - 1) as u32;
        self.acc = if k >= 32 { 0 } else { self.acc >> k };
        self.n -= k;
        Ok(v)
    }

    fn align(&mut self) {
        self.acc = 0;
        self.n = 0;
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
        let last = b.bit()?;
        match b.get(2)? {
            0 => stored(&mut b, &mut out, max)?,
            1 => {
                let (l, di) = (Dec::new(&fixed_lit())?, Dec::new(&fixed_dist())?);
                codes(&mut b, &mut out, &l, &di, max)?
            }
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
    let used = b.p - (b.n / 8) as usize;
    Ok((out, used))
}

fn stored(b: &mut Bits, out: &mut Vec<u8>, max: usize) -> Res<()> {
    b.align();
    let h = b.d.get(b.p..b.p + 4).ok_or(Error::Eof { at: b.p, need: 4 })?;
    let (n, c) = (u16::from_le_bytes([h[0], h[1]]), u16::from_le_bytes([h[2], h[3]]));
    if n != !c {
        return bad("stored length");
    }
    b.p += 4;
    let s = b.d.get(b.p..b.p + n as usize).ok_or(Error::Eof { at: b.p, need: n as usize })?;
    if out.len() + s.len() > max {
        return bad("output limit");
    }
    out.extend_from_slice(s);
    b.p += n as usize;
    Ok(())
}

fn dynamic(b: &mut Bits) -> Res<(Dec, Dec)> {
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
    let cd = Dec::new(&cl)?;
    let mut len = vec![0u8; nl + nd];
    let mut i = 0;
    while i < nl + nd {
        let s = cd.decode(|| b.bit())?;
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
    Ok((Dec::new(&len[..nl])?, Dec::new(&len[nl..])?))
}

fn codes(b: &mut Bits, out: &mut Vec<u8>, lit: &Dec, dist: &Dec, max: usize) -> Res<()> {
    loop {
        let s = lit.decode(|| b.bit())? as usize;
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
        let ds = dist.decode(|| b.bit())? as usize;
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
        let start = out.len() - d;
        for k in 0..len {
            out.push(out[start + k]);
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
