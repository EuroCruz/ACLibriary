use crate::flate::{deflate, inflate::inflate_at};
use ac_core::{bad, Error, Res};
use ac_core::hash::{adler32, IEEE};

pub fn zlib(d: &[u8], level: u32) -> Vec<u8> {
    let flg = match level {
        0 | 1 => 0x01,
        2..=5 => 0x5e,
        6 => 0x9c,
        _ => 0xda,
    };
    let mut o = vec![0x78, flg];
    o.extend(deflate(d, level));
    o.extend(adler32(d).to_be_bytes());
    o
}

pub fn unzlib(d: &[u8]) -> Res<Vec<u8>> {
    if d.len() < 6 || d[0] & 0x0f != 8 || (d[0] as u32 * 256 + d[1] as u32) % 31 != 0 {
        return bad("bad zlib header");
    }
    if d[1] & 0x20 != 0 {
        return bad("zlib dictionary unsupported");
    }
    let (v, used) = inflate_at(&d[2..], 0, usize::MAX)?;
    let t = d.get(2 + used..6 + used).ok_or(Error::Eof { at: d.len(), need: 4 })?;
    if u32::from_be_bytes(t.try_into().unwrap()) != adler32(&v) {
        return bad("adler32 mismatch");
    }
    Ok(v)
}

pub fn gzip(d: &[u8], level: u32) -> Vec<u8> {
    let mut o = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, if level >= 9 { 2 } else { 0 }, 255];
    o.extend(deflate(d, level));
    o.extend(IEEE.sum(d).to_le_bytes());
    o.extend((d.len() as u32).to_le_bytes());
    o
}

pub fn gunzip(d: &[u8]) -> Res<Vec<u8>> {
    if d.len() < 18 || d[0] != 0x1f || d[1] != 0x8b || d[2] != 8 {
        return bad("bad gzip header");
    }
    let f = d[3];
    let mut p = 10;
    let eof = |p: usize| Error::Eof { at: p, need: 1 };
    if f & 4 != 0 {
        let n = u16::from_le_bytes(d.get(p..p + 2).ok_or(eof(p))?.try_into().unwrap()) as usize;
        p += 2 + n;
    }
    for bit in [8, 16] {
        if f & bit != 0 {
            p += d.get(p..).ok_or(eof(p))?.iter().position(|&b| b == 0).ok_or(eof(p))? + 1;
        }
    }
    if f & 2 != 0 {
        p += 2;
    }
    let (v, used) = inflate_at(d.get(p..).ok_or(eof(p))?, 0, usize::MAX)?;
    let t = d.get(p + used..p + used + 8).ok_or(eof(p + used))?;
    if u32::from_le_bytes(t[..4].try_into().unwrap()) != IEEE.sum(&v) || u32::from_le_bytes(t[4..].try_into().unwrap()) != v.len() as u32 {
        return bad("gzip checksum mismatch");
    }
    Ok(v)
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn zlib_vector() {
        let z = [0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15];
        assert_eq!(unzlib(&z).unwrap(), b"hello");
        let mut bad = z;
        bad[12] ^= 1;
        assert!(unzlib(&bad).is_err());
        assert!(unzlib(&z[..5]).is_err());
    }

    #[test]
    fn roundtrip() {
        let d: Vec<u8> = (0..10_000u32).map(|i| (i / 3 % 251) as u8).collect();
        assert_eq!(unzlib(&zlib(&d, 6)).unwrap(), d);
        assert_eq!(gunzip(&gzip(&d, 9)).unwrap(), d);
        assert!(gunzip(&[0u8; 20]).is_err());
    }
}
