use ac_core::{bad, Endian, Error, Reader, Res, Writer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub endian: Endian,
    pub int_sz: u8,
    pub size_sz: u8,
    pub num_sz: u8,
    pub integral: bool,
}

impl Default for Header {
    fn default() -> Header {
        Header { endian: Endian::Le, int_sz: 4, size_sz: 4, num_sz: 8, integral: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub name: Vec<u8>,
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Proto {
    pub source: Option<Vec<u8>>,
    pub line: u64,
    pub last: u64,
    pub nups: u8,
    pub params: u8,
    pub vararg: u8,
    pub stack: u8,
    pub code: Vec<u32>,
    pub consts: Vec<Const>,
    pub protos: Vec<Proto>,
    pub lines: Vec<u64>,
    pub locals: Vec<Local>,
    pub ups: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub h: Header,
    pub main: Proto,
}

const SIG: [u8; 4] = *b"\x1bLua";

fn uint(r: &mut Reader, sz: u8) -> Res<u64> {
    match sz {
        4 => Ok(r.u32()? as u64),
        8 => r.u64(),
        _ => bad("unsupported integer size"),
    }
}

fn count(r: &mut Reader, h: &Header, unit: usize) -> Res<usize> {
    let n = uint(r, h.int_sz)? as usize;
    if n.checked_mul(unit.max(1)).is_none_or(|x| x > r.left()) {
        return bad("count exceeds data");
    }
    Ok(n)
}

fn string(r: &mut Reader, h: &Header) -> Res<Option<Vec<u8>>> {
    let n = uint(r, h.size_sz)? as usize;
    if n == 0 {
        return Ok(None);
    }
    let b = r.take(n)?;
    Ok(Some(b[..n - 1].to_vec()))
}

fn proto(r: &mut Reader, h: &Header, depth: u32) -> Res<Proto> {
    if depth > 200 {
        return bad("nesting too deep");
    }
    let mut p = Proto { source: string(r, h)?, line: uint(r, h.int_sz)?, last: uint(r, h.int_sz)?, nups: r.u8()?, params: r.u8()?, vararg: r.u8()?, stack: r.u8()?, ..Proto::default() };
    let n = count(r, h, 4)?;
    p.code = r.list(n, |r| r.u32())?;
    let n = count(r, h, 1)?;
    p.consts = r.list(n, |r| match r.u8()? {
        0 => Ok(Const::Nil),
        1 => Ok(Const::Bool(r.u8()? != 0)),
        3 => Ok(Const::Num(match (h.integral, h.num_sz) {
            (false, 8) => r.f64()?,
            (false, 4) => r.f32()? as f64,
            (true, 4) => r.i32()? as f64,
            (true, 8) => r.i64()? as f64,
            _ => return bad("unsupported number size"),
        })),
        4 => Ok(Const::Str(string(r, h)?.unwrap_or_default())),
        _ => bad("unknown constant type"),
    })?;
    let n = count(r, h, 1)?;
    p.protos = r.list(n, |r| proto(r, h, depth + 1))?;
    let n = count(r, h, h.int_sz as usize)?;
    p.lines = r.list(n, |r| uint(r, h.int_sz))?;
    let n = count(r, h, 1)?;
    p.locals = r.list(n, |r| Ok(Local { name: string(r, h)?.unwrap_or_default(), start: uint(r, h.int_sz)?, end: uint(r, h.int_sz)? }))?;
    let n = count(r, h, 1)?;
    p.ups = r.list(n, |r| Ok(string(r, h)?.unwrap_or_default()))?;
    Ok(p)
}

pub fn parse(d: &[u8]) -> Res<Chunk> {
    let mut r = Reader::new(d, Endian::Le);
    r.magic(&SIG)?;
    if r.u8()? != 0x51 || r.u8()? != 0 {
        return bad("not a Lua 5.1 chunk");
    }
    let endian = if r.u8()? == 1 { Endian::Le } else { Endian::Be };
    let (int_sz, size_sz, ins, num_sz, integral) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()? != 0);
    if ins != 4 {
        return bad("unsupported instruction size");
    }
    r.e = endian;
    let h = Header { endian, int_sz, size_sz, num_sz, integral };
    let main = proto(&mut r, &h, 0)?;
    Ok(Chunk { h, main })
}

fn put_uint(w: &mut Writer, sz: u8, v: u64) {
    if sz == 4 { w.u32(v as u32) } else { w.u64(v) };
}

fn put_str(w: &mut Writer, h: &Header, s: Option<&[u8]>) {
    match s {
        None => put_uint(w, h.size_sz, 0),
        Some(b) => {
            put_uint(w, h.size_sz, b.len() as u64 + 1);
            w.cstr(b);
        }
    }
}

fn put_proto(w: &mut Writer, h: &Header, p: &Proto) -> Res<()> {
    put_str(w, h, p.source.as_deref());
    put_uint(w, h.int_sz, p.line);
    put_uint(w, h.int_sz, p.last);
    w.u8(p.nups).u8(p.params).u8(p.vararg).u8(p.stack);
    put_uint(w, h.int_sz, p.code.len() as u64);
    p.code.iter().for_each(|&i| {
        w.u32(i);
    });
    put_uint(w, h.int_sz, p.consts.len() as u64);
    for c in &p.consts {
        match c {
            Const::Nil => {
                w.u8(0);
            }
            Const::Bool(b) => {
                w.u8(1).u8(*b as u8);
            }
            Const::Num(n) => {
                w.u8(3);
                match (h.integral, h.num_sz) {
                    (false, 8) => w.f64(*n),
                    (false, 4) => w.f32(*n as f32),
                    (true, 4) => w.i32(*n as i32),
                    (true, 8) => w.i64(*n as i64),
                    _ => return Err(Error::Bad("unsupported number size")),
                };
            }
            Const::Str(s) => {
                w.u8(4);
                put_str(w, h, Some(s));
            }
        }
    }
    put_uint(w, h.int_sz, p.protos.len() as u64);
    for c in &p.protos {
        put_proto(w, h, c)?;
    }
    put_uint(w, h.int_sz, p.lines.len() as u64);
    p.lines.iter().for_each(|&l| put_uint(w, h.int_sz, l));
    put_uint(w, h.int_sz, p.locals.len() as u64);
    for l in &p.locals {
        put_str(w, h, Some(&l.name));
        put_uint(w, h.int_sz, l.start);
        put_uint(w, h.int_sz, l.end);
    }
    put_uint(w, h.int_sz, p.ups.len() as u64);
    p.ups.iter().for_each(|u| put_str(w, h, Some(u)));
    Ok(())
}

pub fn write(c: &Chunk) -> Res<Vec<u8>> {
    let h = &c.h;
    let mut w = Writer::new(h.endian);
    w.bytes(&SIG).u8(0x51).u8(0).u8((h.endian == Endian::Le) as u8).u8(h.int_sz).u8(h.size_sz).u8(4).u8(h.num_sz).u8(h.integral as u8);
    put_proto(&mut w, h, &c.main)?;
    Ok(w.finish())
}

pub mod o {
    pub const MOVE: u8 = 0;
    pub const LOADK: u8 = 1;
    pub const LOADBOOL: u8 = 2;
    pub const LOADNIL: u8 = 3;
    pub const GETUPVAL: u8 = 4;
    pub const GETGLOBAL: u8 = 5;
    pub const GETTABLE: u8 = 6;
    pub const SETGLOBAL: u8 = 7;
    pub const SETUPVAL: u8 = 8;
    pub const SETTABLE: u8 = 9;
    pub const NEWTABLE: u8 = 10;
    pub const SELF: u8 = 11;
    pub const ADD: u8 = 12;
    pub const SUB: u8 = 13;
    pub const MUL: u8 = 14;
    pub const DIV: u8 = 15;
    pub const MOD: u8 = 16;
    pub const POW: u8 = 17;
    pub const UNM: u8 = 18;
    pub const NOT: u8 = 19;
    pub const LEN: u8 = 20;
    pub const CONCAT: u8 = 21;
    pub const JMP: u8 = 22;
    pub const EQ: u8 = 23;
    pub const LT: u8 = 24;
    pub const LE: u8 = 25;
    pub const TEST: u8 = 26;
    pub const TESTSET: u8 = 27;
    pub const CALL: u8 = 28;
    pub const TAILCALL: u8 = 29;
    pub const RETURN: u8 = 30;
    pub const FORLOOP: u8 = 31;
    pub const FORPREP: u8 = 32;
    pub const TFORLOOP: u8 = 33;
    pub const SETLIST: u8 = 34;
    pub const CLOSE: u8 = 35;
    pub const CLOSURE: u8 = 36;
    pub const VARARG: u8 = 37;
}

pub const OPS: [&str; 38] = [
    "MOVE", "LOADK", "LOADBOOL", "LOADNIL", "GETUPVAL", "GETGLOBAL", "GETTABLE", "SETGLOBAL", "SETUPVAL", "SETTABLE", "NEWTABLE", "SELF",
    "ADD", "SUB", "MUL", "DIV", "MOD", "POW", "UNM", "NOT", "LEN", "CONCAT", "JMP", "EQ", "LT", "LE", "TEST", "TESTSET", "CALL", "TAILCALL",
    "RETURN", "FORLOOP", "FORPREP", "TFORLOOP", "SETLIST", "CLOSE", "CLOSURE", "VARARG",
];

pub fn op(i: u32) -> u8 {
    (i & 0x3f) as u8
}

pub fn a(i: u32) -> u32 {
    (i >> 6) & 0xff
}

pub fn b(i: u32) -> u32 {
    i >> 23
}

pub fn c(i: u32) -> u32 {
    (i >> 14) & 0x1ff
}

pub fn bx(i: u32) -> u32 {
    i >> 14
}

pub fn sbx(i: u32) -> i32 {
    bx(i) as i32 - 131071
}

pub fn abc(op: u8, a: u32, b: u32, c: u32) -> u32 {
    op as u32 | a << 6 | c << 14 | b << 23
}

pub fn abx(op: u8, a: u32, bx: u32) -> u32 {
    op as u32 | a << 6 | bx << 14
}

pub fn asbx(op: u8, a: u32, sbx: i32) -> u32 {
    abx(op, a, (sbx + 131071) as u32)
}

pub fn disasm(p: &Proto) -> String {
    let mut o = String::new();
    for (pc, &i) in p.code.iter().enumerate() {
        let name = OPS.get(op(i) as usize).copied().unwrap_or("?");
        let args = match op(i) {
            1 | 5 | 7 | 36 => format!("{} {}", a(i), bx(i)),
            22 | 31 | 32 => format!("{} {}", a(i), sbx(i)),
            _ => format!("{} {} {}", a(i), b(i), c(i)),
        };
        o.push_str(&format!("{pc:5}  {name:<9} {args}\n"));
    }
    o
}

#[cfg(test)]
mod t {
    use super::*;

    fn sample(h: Header) -> Chunk {
        let leaf = Proto { source: None, line: 3, last: 4, nups: 0, params: 1, vararg: 0, stack: 2, code: vec![abc(30, 0, 1, 0)], lines: vec![4], ..Proto::default() };
        let main = Proto {
            source: Some(b"@test.lua".to_vec()),
            line: 0,
            last: 0,
            nups: 0,
            params: 0,
            vararg: 2,
            stack: 3,
            code: vec![abx(5, 0, 0), abx(1, 1, 1), abc(28, 0, 2, 1), asbx(22, 0, -1), abc(30, 0, 1, 0)],
            consts: vec![Const::Str(b"print".to_vec()), Const::Num(1.5), Const::Nil, Const::Bool(true)],
            protos: vec![leaf],
            lines: vec![1, 1, 1, 2, 2],
            locals: vec![Local { name: b"x".to_vec(), start: 1, end: 4 }],
            ups: vec![b"up".to_vec()],
        };
        Chunk { h, main }
    }

    #[test]
    fn roundtrips_all_layouts() {
        for h in [
            Header::default(),
            Header { endian: Endian::Be, ..Header::default() },
            Header { int_sz: 8, size_sz: 8, ..Header::default() },
            Header { num_sz: 4, ..Header::default() },
            Header { integral: true, num_sz: 4, ..Header::default() },
        ] {
            let mut c = sample(h);
            if h.integral {
                c.main.consts[1] = Const::Num(7.0);
            }
            let bytes = write(&c).unwrap();
            assert_eq!(&bytes[..5], b"\x1bLua\x51");
            assert_eq!(parse(&bytes).unwrap(), c);
        }
    }

    #[test]
    fn rejects_bad_input() {
        let good = write(&sample(Header::default())).unwrap();
        assert!(parse(&good[..good.len() - 3]).is_err());
        assert!(parse(b"\x1bLua\x52\0").is_err());
        assert!(parse(b"nope").is_err());
        let mut huge = good.clone();
        huge[38..42].copy_from_slice(&[0xff; 4]);
        assert!(parse(&huge).is_err());
    }

    #[test]
    fn instructions() {
        let i = abc(28, 7, 3, 2);
        assert_eq!((op(i), a(i), b(i), c(i)), (28, 7, 3, 2));
        assert_eq!(sbx(asbx(22, 1, -5)), -5);
        assert_eq!(bx(abx(1, 0, 12345)), 12345);
        let d = disasm(&sample(Header::default()).main);
        assert!(d.contains("GETGLOBAL") && d.contains("CALL") && d.contains("JMP"));
    }
}
