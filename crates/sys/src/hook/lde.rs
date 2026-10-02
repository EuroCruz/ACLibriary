#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub len: usize,
    pub op: u8,
    pub map: u8,
    pub rel: Option<(usize, usize)>,
    pub rip: Option<usize>,
}

#[derive(Clone, Copy, PartialEq)]
enum Imm {
    No,
    B,
    W,
    Z,
    V,
    Enter,
    Moffs,
    Rel8,
    Rel32,
    Far,
}

fn one(op: u8, x64: bool) -> (bool, Imm) {
    use Imm::*;
    match op {
        0x00..=0x3f => match op & 7 {
            0..=3 => (true, No),
            4 => (false, B),
            5 => (false, Z),
            _ => (false, No),
        },
        0x62 | 0x63 => (true, No),
        0x68 => (false, Z),
        0x69 => (true, Z),
        0x6a => (false, B),
        0x6b => (true, B),
        0x70..=0x7f => (false, Rel8),
        0x80 | 0x82 | 0x83 => (true, B),
        0x81 => (true, Z),
        0x84..=0x8f => (true, No),
        0x9a | 0xea => (false, if x64 { No } else { Far }),
        0xa0..=0xa3 => (false, Moffs),
        0xa8 => (false, B),
        0xa9 => (false, Z),
        0xb0..=0xb7 => (false, B),
        0xb8..=0xbf => (false, V),
        0xc0 | 0xc1 | 0xc6 => (true, B),
        0xc2 | 0xca => (false, W),
        0xc4 | 0xc5 => (true, No),
        0xc7 => (true, Z),
        0xc8 => (false, Enter),
        0xcd => (false, B),
        0xd0..=0xd3 | 0xd8..=0xdf => (true, No),
        0xd4 | 0xd5 => (false, B),
        0xe0..=0xe7 | 0xeb => (false, if (0xe4..=0xe7).contains(&op) { B } else { Rel8 }),
        0xe8 | 0xe9 => (false, Rel32),
        0xf6 | 0xf7 | 0xfe | 0xff => (true, No),
        _ => (false, No),
    }
}

fn two(op: u8) -> (bool, Imm) {
    use Imm::*;
    match op {
        0x05..=0x09 | 0x0b | 0x0e | 0x30..=0x37 | 0x77 | 0xa0..=0xa2 | 0xa8..=0xaa | 0xc8..=0xcf => (false, No),
        0x70..=0x73 | 0xa4 | 0xac | 0xba | 0xc2 | 0xc4..=0xc6 | 0x0f => (true, B),
        0x80..=0x8f => (false, Rel32),
        _ => (true, No),
    }
}

pub fn decode(c: &[u8], x64: bool) -> Option<Info> {
    let at = |i: usize| c.get(i).copied();
    let (mut i, mut p66, mut p67, mut rexw) = (0, false, false, false);
    while let Some(b) = at(i) {
        match b {
            0x66 => p66 = true,
            0x67 => p67 = true,
            0xf0 | 0xf2 | 0xf3 | 0x2e | 0x36 | 0x3e | 0x26 | 0x64 | 0x65 => {}
            _ => break,
        }
        i += 1;
        if i > 14 {
            return None;
        }
    }
    while x64 && at(i)? & 0xf0 == 0x40 {
        rexw = at(i)? & 8 != 0;
        i += 1;
    }
    let first = at(i)?;
    i += 1;
    let vex = matches!(first, 0xc4 | 0xc5 | 0x62) && (x64 || at(i)? >> 6 == 3);
    let (mut modrm, mut imm, mut map, mut op) = (true, Imm::No, 0u8, first);
    if vex {
        let (m, o, skip) = match first {
            0xc5 => (1, at(i + 1)?, 2),
            0xc4 => (at(i)? & 0x1f, at(i + 2)?, 3),
            _ => (at(i)? & 7, at(i + 3)?, 4),
        };
        i += skip;
        (map, op) = (m, o);
        modrm = !(map == 1 && op == 0x77);
        if map == 3 || (map == 1 && matches!(op, 0x70..=0x73 | 0xc2 | 0xc4..=0xc6)) {
            imm = Imm::B;
        }
    } else if first == 0x0f {
        op = at(i)?;
        i += 1;
        match op {
            0x38 => {
                map = 2;
                op = at(i)?;
                i += 1;
            }
            0x3a => {
                map = 3;
                op = at(i)?;
                i += 1;
                imm = Imm::B;
            }
            _ => {
                map = 1;
                (modrm, imm) = two(op);
            }
        }
    } else {
        (modrm, imm) = one(op, x64);
    }
    let mut info = Info { len: 0, op, map, rel: None, rip: None };
    if modrm {
        let m = at(i)?;
        i += 1;
        let (md, rm) = (m >> 6, m & 7);
        let a16 = !x64 && p67;
        if md != 3 {
            let mut disp = match md {
                1 => 1,
                2 => if a16 { 2 } else { 4 },
                _ if rm == 5 && !a16 => 4,
                _ if rm == 6 && a16 => 2,
                _ => 0,
            };
            if !a16 && rm == 4 {
                let s = at(i)?;
                i += 1;
                if md == 0 && s & 7 == 5 {
                    disp = 4;
                }
            }
            if x64 && md == 0 && rm == 5 {
                info.rip = Some(i);
            }
            i += disp;
        }
        if map == 0 && !vex && ((op == 0xf6 && (m >> 3) & 7 < 2) || (op == 0xf7 && (m >> 3) & 7 < 2)) {
            imm = if op == 0xf6 { Imm::B } else { Imm::Z };
        }
    }
    let z = if p66 && !rexw { 2 } else { 4 };
    let n = match imm {
        Imm::No => 0,
        Imm::B | Imm::Rel8 => 1,
        Imm::W => 2,
        Imm::Z => z,
        Imm::V => if rexw { 8 } else { z },
        Imm::Enter => 3,
        Imm::Moffs => if x64 { if p67 { 4 } else { 8 } } else if p67 { 2 } else { 4 },
        Imm::Rel32 => if x64 { 4 } else { z },
        Imm::Far => z + 2,
    };
    if matches!(imm, Imm::Rel8 | Imm::Rel32) {
        info.rel = Some((i, n));
    }
    i += n;
    if i > 15 || i > c.len() {
        return None;
    }
    info.len = i;
    Some(info)
}

#[cfg(test)]
mod t {
    use super::*;

    fn l(b: &[u8], x64: bool) -> usize {
        decode(b, x64).unwrap().len
    }

    #[test]
    fn common_instructions() {
        assert_eq!(l(&[0x90], false), 1);
        assert_eq!(l(&[0xb8, 1, 0, 0, 0], false), 5);
        assert_eq!(l(&[0x55], false), 1);
        assert_eq!(l(&[0x8b, 0xec], false), 2);
        assert_eq!(l(&[0x83, 0xec, 0x30], false), 3);
        assert_eq!(l(&[0x8b, 0x45, 0x08], false), 3);
        assert_eq!(l(&[0xe8, 0, 0, 0, 0], false), 5);
        assert_eq!(l(&[0x0f, 0x84, 0, 0, 0, 0], false), 6);
        assert_eq!(l(&[0x66, 0xb8, 1, 0], false), 4);
        assert_eq!(l(&[0xc7, 0x05, 1, 2, 3, 4, 5, 6, 7, 8], false), 10);
        assert_eq!(l(&[0x8d, 0x44, 0x24, 0x10], false), 4);
        assert_eq!(l(&[0xf7, 0xc1, 1, 0, 0, 0], false), 6);
        assert_eq!(l(&[0xf7, 0xd9], false), 2);
        assert_eq!(l(&[0xa1, 1, 2, 3, 4], false), 5);
    }

    #[test]
    fn sixty_four_bit_forms() {
        assert_eq!(l(&[0x48, 0xb8, 1, 2, 3, 4, 5, 6, 7, 8], true), 10);
        assert_eq!(l(&[0x48, 0x8b, 0x05, 1, 2, 3, 4], true), 7);
        assert_eq!(decode(&[0x48, 0x8b, 0x05, 1, 2, 3, 4], true).unwrap().rip, Some(3));
        assert_eq!(l(&[0xc5, 0xf8, 0x77], true), 3);
        assert_eq!(l(&[0xc5, 0xfa, 0x10, 0x44, 0x24, 0x08], true), 6);
        assert_eq!(l(&[0xff, 0x25, 0, 0, 0, 0], true), 6);
        assert_eq!(l(&[0x41, 0x57], true), 2);
    }

    #[test]
    fn relative_info_and_failures() {
        let i = decode(&[0xe9, 1, 2, 3, 4], false).unwrap();
        assert_eq!(i.rel, Some((1, 4)));
        assert_eq!(decode(&[0xeb, 5], false).unwrap().rel, Some((1, 1)));
        assert_eq!(decode(&[0x74, 5], true).unwrap().rel, Some((1, 1)));
        assert!(decode(&[0xe8, 0, 0], false).is_none());
        assert!(decode(&[], false).is_none());
        assert!(decode(&[0x66; 20], false).is_none());
    }

    #[test]
    #[ignore]
    fn against_reference_disassembler() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/");
        for (tag, x64) in [("x86", false), ("x64", true)] {
            let (Ok(code), Ok(r)) = (std::fs::read(format!("{dir}lde_{tag}.code")), std::fs::read(format!("{dir}lde_{tag}.ref"))) else { continue };
            let (mut bad, mut shown) = (0, 0);
            for c in r.chunks(5) {
                let (pos, want) = (u32::from_le_bytes(c[..4].try_into().unwrap()) as usize, c[4] as usize);
                let got = decode(&code[pos..(pos + 16).min(code.len())], x64).map(|i| i.len);
                if got != Some(want) {
                    bad += 1;
                    if shown < 12 {
                        shown += 1;
                        println!("{tag} @{pos:x} want {want} got {got:?} bytes {:02x?}", &code[pos..(pos + want).min(code.len())]);
                    }
                }
            }
            println!("{tag}: {} instructions, {bad} mismatches", r.len() / 5);
        }
    }
}