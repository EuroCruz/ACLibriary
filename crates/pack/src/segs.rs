use crate::flate::{deflate, inflate};
use ac_core::{bad, Endian, Reader, Res, Writer};

const SEG: usize = 0x1_0000;

fn endian(d: &[u8]) -> Option<Endian> {
    match d.get(..4)? {
        b"segs" => Some(Endian::Be),
        b"sges" => Some(Endian::Le),
        _ => None,
    }
}

pub fn is_segs(d: &[u8]) -> bool {
    endian(d).is_some() && d.len() >= 16
}

pub fn decode(d: &[u8]) -> Res<Vec<u8>> {
    let Some(e) = endian(d) else { return bad("not a segs stream") };
    let mut r = Reader::new(d, e);
    r.skip(4)?;
    let (_, n, unc, _) = (r.u16()?, r.u16()? as usize, r.u32()? as usize, r.u32()?);
    let mut o = Vec::with_capacity(unc);
    for _ in 0..n {
        let (cs, us, at) = (r.u16()? as usize, r.u16()? as usize, r.u32()? as usize);
        let us = if us == 0 { SEG } else { us };
        let s = d.get(at & !1..(at & !1) + cs).ok_or(ac_core::Error::Bad("segs segment runs past the end"))?;
        let u = if at & 1 != 0 { inflate(s, us)? } else { s.to_vec() };
        if u.len() != us {
            return bad("segs segment size mismatch");
        }
        o.extend(u);
    }
    if o.len() != unc {
        return bad("segs total size mismatch");
    }
    Ok(o)
}

fn split<'a>(c: &'a [u8], level: u32, out: &mut Vec<(&'a [u8], Vec<u8>, bool)>) {
    let z = deflate(c, level);
    if z.len() < c.len() && z.len() <= 0xffff {
        out.push((c, z, true));
    } else if c.len() <= 0xffff {
        out.push((c, c.to_vec(), false));
    } else {
        let (a, b) = c.split_at(c.len() / 2);
        split(a, level, out);
        split(b, level, out);
    }
}

pub fn encode(d: &[u8], e: Endian, level: u32) -> Vec<u8> {
    let mut segs = Vec::new();
    d.chunks(SEG).for_each(|c| split(c, level, &mut segs));
    let parts: Vec<&Vec<u8>> = segs.iter().map(|s| &s.1).collect();
    let mut at = (16 + parts.len() * 8).next_multiple_of(16);
    let mut w = Writer::new(e);
    let body_end = parts.iter().fold(at, |a, p| (a + p.len()).next_multiple_of(16));
    w.bytes(if e == Endian::Be { b"segs" } else { b"sges" }).u16(5).u16(parts.len() as u16).u32(d.len() as u32).u32(body_end as u32);
    for (c, p, z) in &segs {
        w.u16(p.len() as u16).u16(c.len() as u16).u32(at as u32 | *z as u32);
        at = (at + p.len()).next_multiple_of(16);
    }
    let mut o = w.finish();
    for p in parts {
        o.resize(o.len().next_multiple_of(16), 0);
        o.extend(p);
    }
    o.resize(body_end, 0);
    o
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn roundtrip() {
        for n in [0usize, 1, 1000, SEG, SEG + 1, 3 * SEG + 77] {
            let d: Vec<u8> = (0..n).map(|i| (i * 7 / 13) as u8).collect();
            for e in [Endian::Be, Endian::Le] {
                let z = encode(&d, e, 6);
                assert!(is_segs(&z));
                assert_eq!(z.len() % 16, 0);
                assert_eq!(decode(&z).unwrap(), d, "{n}");
            }
        }
        let mut x = 1u32;
        let rnd: Vec<u8> = (0..3 * SEG).map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        }).collect();
        assert_eq!(decode(&encode(&rnd, Endian::Be, 9)).unwrap(), rnd);
    }
}
