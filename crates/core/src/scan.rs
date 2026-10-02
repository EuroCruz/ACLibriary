use crate::{bad, Res};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pat {
    b: Vec<u8>,
    m: Vec<u8>,
}

impl Pat {
    pub fn parse(s: &str) -> Res<Pat> {
        let (mut b, mut m) = (Vec::new(), Vec::new());
        for t in s.split_whitespace() {
            if t.chars().all(|c| c == '?') && (1..=2).contains(&t.len()) {
                b.push(0);
                m.push(0);
            } else if t.len() == 2 {
                match u8::from_str_radix(t, 16) {
                    Ok(v) => {
                        b.push(v);
                        m.push(0xff);
                    }
                    Err(_) => return bad("bad pattern byte"),
                }
            } else {
                return bad("bad pattern token");
            }
        }
        if b.is_empty() {
            return bad("empty pattern");
        }
        Ok(Pat { b, m })
    }

    pub fn exact(b: &[u8]) -> Pat {
        Pat { b: b.to_vec(), m: vec![0xff; b.len()] }
    }

    pub fn len(&self) -> usize {
        self.b.len()
    }

    pub fn is_empty(&self) -> bool {
        self.b.is_empty()
    }

    pub fn at(&self, d: &[u8]) -> bool {
        d.len() >= self.b.len() && self.b.iter().zip(&self.m).zip(d).all(|((&b, &m), &x)| x & m == b)
    }

    fn anchor(&self) -> Option<usize> {
        self.m.iter().position(|&m| m == 0xff)
    }

    pub fn find(&self, d: &[u8]) -> Option<usize> {
        self.find_from(d, 0)
    }

    pub fn find_from(&self, d: &[u8], from: usize) -> Option<usize> {
        if d.len() < self.b.len() || from > d.len() - self.b.len() {
            return None;
        }
        let last = d.len() - self.b.len();
        match self.anchor() {
            Some(a) => {
                let v = self.b[a];
                let mut i = from;
                while i <= last {
                    let off = d[i + a..=last + a].iter().position(|&x| x == v)?;
                    i += off;
                    if self.at(&d[i..]) {
                        return Some(i);
                    }
                    i += 1;
                }
                None
            }
            None => Some(from),
        }
    }

    pub fn all(&self, d: &[u8]) -> Vec<usize> {
        let mut o = Vec::new();
        let mut p = 0;
        while let Some(i) = self.find_from(d, p) {
            o.push(i);
            p = i + 1;
        }
        o
    }
}

pub fn ascii(d: &[u8], s: &str) -> Vec<usize> {
    Pat::exact(s.as_bytes()).all(d)
}

pub fn utf16(d: &[u8], s: &str) -> Vec<usize> {
    let b: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    Pat::exact(&b).all(d)
}

pub fn ptr32(d: &[u8], v: u32) -> Vec<usize> {
    Pat::exact(&v.to_le_bytes()).all(d)
}

pub fn rel32(d: &[u8], at: usize) -> Option<i64> {
    let b = d.get(at..at + 4)?;
    Some(i32::from_le_bytes(b.try_into().unwrap()) as i64)
}

pub fn branch_target(d: &[u8], at: usize, base: u64) -> Option<u64> {
    let op = *d.get(at)?;
    if op != 0xE8 && op != 0xE9 {
        return None;
    }
    Some((base + at as u64 + 5).wrapping_add(rel32(d, at + 1)? as u64))
}

pub fn calls_to(d: &[u8], base: u64, target: u64) -> Vec<usize> {
    (0..d.len().saturating_sub(4))
        .filter(|&i| (d[i] == 0xE8 || d[i] == 0xE9) && branch_target(d, i, base) == Some(target))
        .collect()
}

pub fn strings(d: &[u8], min: usize) -> Vec<(usize, String)> {
    let mut o = Vec::new();
    let mut s = 0;
    for i in 0..=d.len() {
        let ok = d.get(i).is_some_and(|&b| (0x20..0x7f).contains(&b));
        if !ok {
            if i - s >= min {
                o.push((s, String::from_utf8_lossy(&d[s..i]).into_owned()));
            }
            s = i + 1;
        }
    }
    o
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn patterns() {
        let d = [0x90, 0xE8, 0x10, 0x00, 0x00, 0x00, 0xC3, 0xE8, 0x01, 0x02, 0x03, 0x04];
        let p = Pat::parse("E8 ?? ?? ?? ??").unwrap();
        assert_eq!(p.all(&d), [1, 7]);
        assert_eq!(Pat::parse("E8 ? 00").unwrap().find(&d), Some(1));
        assert_eq!(Pat::parse("C3 E8").unwrap().find(&d), Some(6));
        assert_eq!(Pat::parse("?? ??").unwrap().find(&d), Some(0));
        assert_eq!(Pat::parse("FF").unwrap().find(&d), None);
        assert!(Pat::parse("E8 G0").is_err());
        assert!(Pat::parse("").is_err());
        assert!(Pat::parse("E8 123").is_err());
    }

    #[test]
    fn xrefs() {
        let d = [0x90, 0xE8, 0x10, 0x00, 0x00, 0x00, 0xC3];
        assert_eq!(branch_target(&d, 1, 0x1000), Some(0x1000 + 6 + 0x10));
        assert_eq!(calls_to(&d, 0x1000, 0x1016), [1]);
        assert_eq!(branch_target(&d, 0, 0), None);
        assert_eq!(ptr32(&[1, 0x78, 0x56, 0x34, 0x12], 0x1234_5678), [1]);
    }

    #[test]
    fn text() {
        let d = b"\0\0hello\0ab\0world!\0";
        assert_eq!(strings(d, 4), [(2, "hello".to_string()), (11, "world!".to_string())]);
        assert_eq!(ascii(d, "wor"), [11]);
        let w: Vec<u8> = "hi".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(utf16(&w, "hi"), [0]);
    }
}
