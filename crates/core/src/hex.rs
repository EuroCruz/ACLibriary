use crate::{bad, Res};

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Res<Vec<u8>> {
    let d: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    if d.len() % 2 != 0 {
        return bad("odd hex length");
    }
    d.chunks(2)
        .map(|c| {
            let h = |x: u8| (x as char).to_digit(16);
            match (h(c[0]), h(c[1])) {
                (Some(a), Some(b)) => Ok((a * 16 + b) as u8),
                _ => bad("hex digit"),
            }
        })
        .collect()
}

pub fn dump(b: &[u8], base: usize) -> String {
    let mut o = String::new();
    for (i, row) in b.chunks(16).enumerate() {
        let h: Vec<String> = row.iter().map(|x| format!("{x:02x}")).collect();
        let a: String = row.iter().map(|&x| if (0x20..0x7f).contains(&x) { x as char } else { '.' }).collect();
        o.push_str(&format!("{:08x}  {:<47}  {a}\n", base + i * 16, h.join(" ")));
    }
    o
}

pub const fn tag(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

pub fn untag(t: u32) -> [u8; 4] {
    t.to_le_bytes()
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
        assert_eq!(unhex("00 FF 10").unwrap(), [0, 255, 16]);
        assert!(unhex("0").is_err());
        assert!(unhex("zz").is_err());
    }

    #[test]
    fn tags() {
        assert_eq!(untag(tag(b"segs")), *b"segs");
        assert!(dump(b"hello", 0x10).starts_with("00000010  68 65 6c 6c 6f"));
    }
}
