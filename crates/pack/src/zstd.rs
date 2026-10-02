use crate::huff::lengths;
use ac_core::hash::xxh64;
use ac_core::{bad, Error, Res};

const MAGIC: u32 = 0xFD2F_B528;
const BLOCK: usize = 1 << 17;

const LL_DIST: [i16; 36] = [4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 1, 1, 1, 1, 1, -1, -1, -1, -1];
const OF_DIST: [i16; 29] = [1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1, -1, -1, -1, -1];
const ML_DIST: [i16; 53] = [
    1, 4, 3, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    -1, -1, -1, -1, -1, -1, -1,
];
const LL_BASE: [u32; 36] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24, 28, 32, 40, 48, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384,
    32768, 65536,
];
const LL_BITS: [u8; 36] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const ML_BASE: [u32; 53] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 37, 39, 41, 43,
    47, 51, 59, 67, 83, 99, 131, 259, 515, 1027, 2051, 4099, 8195, 16387, 32771, 65539,
];
const ML_BITS: [u8; 53] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3, 4, 4, 5, 7, 8, 9,
    10, 11, 12, 13, 14, 15, 16,
];

fn hb(x: u64) -> u32 {
    63 - x.leading_zeros()
}

fn eof(at: usize, need: usize) -> Error {
    Error::Eof { at, need }
}

fn le(d: &[u8], at: usize, n: usize) -> u64 {
    (0..n).fold(0u64, |v, i| v | (d[at + i] as u64) << (8 * i))
}

struct Back<'a> {
    d: &'a [u8],
    off: i64,
}

impl<'a> Back<'a> {
    fn new(d: &'a [u8]) -> Res<Back<'a>> {
        match d.last() {
            Some(&b) if b != 0 => Ok(Back { d, off: (d.len() as i64 - 1) * 8 + hb(b as u64) as i64 }),
            _ => bad("bad zstd bitstream end"),
        }
    }

    fn bits(&mut self, n: u32) -> u64 {
        if n == 0 {
            return 0;
        }
        self.off -= n as i64;
        let (at, n, sh) = if self.off < 0 { (0u64, (n as i64 + self.off).max(0) as u32, (-self.off) as u32) } else { (self.off as u64, n, 0) };
        if n == 0 {
            return 0;
        }
        let (byte, bit) = ((at / 8) as usize, (at % 8) as u32);
        let k = ((bit + n + 7) / 8) as usize;
        let v = (le(self.d, byte, k.min(8)) >> bit) & ((1u64 << n) - 1);
        if sh >= 64 { 0 } else { v << sh }
    }
}

struct Fwd<'a> {
    d: &'a [u8],
    bit: usize,
}

impl Fwd<'_> {
    fn bits(&mut self, n: u32) -> Res<u32> {
        let end = self.bit + n as usize;
        if end > self.d.len() * 8 {
            return Err(eof(self.d.len(), 1));
        }
        let (byte, bit) = (self.bit / 8, (self.bit % 8) as u32);
        let k = ((bit + n + 7) / 8) as usize;
        self.bit = end;
        Ok(((le(self.d, byte, k) >> bit) & ((1u64 << n) - 1)) as u32)
    }

    fn bytes(&self) -> usize {
        self.bit.div_ceil(8)
    }
}

#[derive(Clone)]
struct Fse {
    sym: Vec<u8>,
    nb: Vec<u8>,
    base: Vec<u16>,
    log: u32,
}

fn spread(norm: &[i16], log: u32) -> Res<Vec<u8>> {
    let size = 1usize << log;
    let mut sym = vec![0u8; size];
    let mut high = size;
    for (s, &n) in norm.iter().enumerate() {
        if n == -1 {
            high = high.checked_sub(1).ok_or(Error::Bad("fse table overflow"))?;
            sym[high] = s as u8;
        }
    }
    let (step, mask) = ((size >> 1) + (size >> 3) + 3, size - 1);
    let mut pos = 0;
    for (s, &n) in norm.iter().enumerate() {
        for _ in 0..n.max(0) {
            sym[pos] = s as u8;
            pos = (pos + step) & mask;
            while pos >= high {
                pos = (pos + step) & mask;
            }
        }
    }
    if pos != 0 {
        return bad("bad fse distribution");
    }
    Ok(sym)
}

impl Fse {
    fn new(norm: &[i16], log: u32) -> Res<Fse> {
        let size = 1usize << log;
        let sym = spread(norm, log)?;
        let mut next: Vec<u32> = norm.iter().map(|&n| if n == -1 { 1 } else { n.max(0) as u32 }).collect();
        let (mut nb, mut base) = (vec![0u8; size], vec![0u16; size]);
        for i in 0..size {
            let s = sym[i] as usize;
            let d = next[s];
            next[s] += 1;
            nb[i] = (log - hb(d as u64)) as u8;
            base[i] = ((d << nb[i]) as usize - size) as u16;
        }
        Ok(Fse { sym, nb, base, log })
    }

    fn rle(s: u8) -> Fse {
        Fse { sym: vec![s], nb: vec![0], base: vec![0], log: 0 }
    }

    fn read(d: &[u8], max: u32) -> Res<(Fse, usize)> {
        let mut r = Fwd { d, bit: 0 };
        let log = 5 + r.bits(4)?;
        if log > max {
            return bad("fse accuracy too large");
        }
        let mut left = 1i32 << log;
        let mut norm: Vec<i16> = Vec::new();
        while left > 0 {
            if norm.len() >= 256 {
                return bad("too many fse symbols");
            }
            let n = hb(left as u64 + 1) + 1;
            let low = (1u32 << (n - 1)) - 1;
            let thr = (1u32 << n) - 1 - (left as u32 + 1);
            let mut v = r.bits(n - 1)?;
            if v >= thr {
                v |= r.bits(1)? << (n - 1);
                if v > low {
                    v -= thr;
                }
            }
            let p = v as i16 - 1;
            left -= p.unsigned_abs() as i32;
            norm.push(p);
            if p == 0 {
                loop {
                    let k = r.bits(2)?;
                    norm.extend(std::iter::repeat_n(0, k as usize));
                    if k != 3 {
                        break;
                    }
                }
            }
        }
        if left != 0 || norm.len() > 256 {
            return bad("bad fse header");
        }
        Ok((Fse::new(&norm, log)?, r.bytes()))
    }

    fn init(&self, b: &mut Back) -> usize {
        b.bits(self.log) as usize
    }

    fn step(&self, st: &mut usize, b: &mut Back) {
        *st = self.base[*st] as usize + b.bits(self.nb[*st] as u32) as usize;
    }
}

struct Huf {
    sym: Vec<u8>,
    nb: Vec<u8>,
    max: u32,
}

impl Huf {
    fn from_weights(w: &[u8]) -> Res<Huf> {
        if w.len() > 255 || w.iter().any(|&x| x > 11) {
            return bad("bad huffman weights");
        }
        let sum: u64 = w.iter().map(|&x| if x > 0 { 1u64 << (x - 1) } else { 0 }).sum();
        if sum == 0 {
            return bad("empty huffman weights");
        }
        let max = hb(sum) + 1;
        let rest = (1u64 << max) - sum;
        if rest & (rest - 1) != 0 {
            return bad("bad huffman weights");
        }
        let mut bits: Vec<u8> = w.iter().map(|&x| if x > 0 { (max + 1 - x as u32) as u8 } else { 0 }).collect();
        bits.push((max - hb(rest)) as u8);
        Huf::from_bits(&bits)
    }

    fn from_bits(bits: &[u8]) -> Res<Huf> {
        let max = *bits.iter().max().unwrap_or(&0) as u32;
        if max == 0 || max > 11 {
            return bad("bad huffman depth");
        }
        let size = 1usize << max;
        let mut cnt = [0usize; 12];
        bits.iter().for_each(|&b| cnt[b as usize] += 1);
        let mut idx = [0usize; 13];
        for i in (1..=max as usize).rev() {
            idx[i - 1] = idx[i] + cnt[i] * (1 << (max as usize - i));
        }
        if idx[0] != size {
            return bad("incomplete huffman table");
        }
        let (mut sym, mut nb) = (vec![0u8; size], vec![0u8; size]);
        for (s, &b) in bits.iter().enumerate() {
            if b != 0 {
                let (at, n) = (idx[b as usize], 1usize << (max - b as u32));
                sym[at..at + n].fill(s as u8);
                nb[at..at + n].fill(b);
                idx[b as usize] += n;
            }
        }
        Ok(Huf { sym, nb, max })
    }

    fn read(d: &[u8]) -> Res<(Huf, usize)> {
        let h = *d.first().ok_or(eof(0, 1))? as usize;
        if h >= 128 {
            let n = h - 127;
            let b = d.get(1..1 + n.div_ceil(2)).ok_or(eof(1, n.div_ceil(2)))?;
            let w: Vec<u8> = (0..n).map(|i| if i % 2 == 0 { b[i / 2] >> 4 } else { b[i / 2] & 15 }).collect();
            return Ok((Huf::from_weights(&w)?, 1 + b.len()));
        }
        let s = d.get(1..1 + h).ok_or(eof(1, h))?;
        let (f, used) = Fse::read(s, 7)?;
        let mut b = Back::new(s.get(used..).filter(|x| !x.is_empty()).ok_or(eof(used, 1))?)?;
        let (mut s1, mut s2) = (f.init(&mut b), f.init(&mut b));
        let mut w = Vec::new();
        loop {
            if w.len() > 255 {
                return bad("too many huffman weights");
            }
            w.push(f.sym[s1]);
            f.step(&mut s1, &mut b);
            if b.off < 0 {
                w.push(f.sym[s2]);
                break;
            }
            w.push(f.sym[s2]);
            f.step(&mut s2, &mut b);
            if b.off < 0 {
                w.push(f.sym[s1]);
                break;
            }
        }
        Ok((Huf::from_weights(&w)?, 1 + h))
    }

    fn stream(&self, d: &[u8], o: &mut Vec<u8>, max: usize) -> Res<()> {
        let mut b = Back::new(d)?;
        let mask = (1usize << self.max) - 1;
        let mut st = b.bits(self.max) as usize;
        while b.off > -(self.max as i64) {
            if o.len() >= max {
                return bad("too many literals");
            }
            o.push(self.sym[st]);
            let n = self.nb[st] as u32;
            st = ((st << n) + b.bits(n) as usize) & mask;
        }
        if b.off != -(self.max as i64) {
            return bad("bad huffman stream");
        }
        Ok(())
    }
}

#[derive(Default)]
struct Ctx {
    rep: [usize; 3],
    huf: Option<Huf>,
    tabs: [Option<Fse>; 3],
}

fn rep(v: usize, ll: usize, r: &mut [usize; 3]) -> usize {
    if v > 3 {
        *r = [v - 3, r[0], r[1]];
        return v - 3;
    }
    let i = v - 1 + (ll == 0) as usize;
    if i == 0 {
        return r[0];
    }
    let o = if i < 3 { r[i] } else { r[0].wrapping_sub(1) };
    if i > 1 {
        r[2] = r[1];
    }
    r[1] = r[0];
    r[0] = o;
    o
}

fn literals(c: &mut Ctx, d: &[u8]) -> Res<(Vec<u8>, usize)> {
    let b0 = *d.first().ok_or(eof(0, 1))?;
    let (kind, sf) = (b0 & 3, (b0 >> 2) & 3);
    if kind < 2 {
        let (h, n) = match sf {
            0 | 2 => (1, (b0 >> 3) as usize),
            1 => (2, le(d.get(..2).ok_or(eof(0, 2))?, 0, 2) as usize >> 4),
            _ => (3, le(d.get(..3).ok_or(eof(0, 3))?, 0, 3) as usize >> 4),
        };
        if n > BLOCK {
            return bad("literals too large");
        }
        return Ok(if kind == 0 {
            (d.get(h..h + n).ok_or(eof(h, n))?.to_vec(), h + n)
        } else {
            (vec![*d.get(h).ok_or(eof(h, 1))?; n], h + 1)
        });
    }
    let (h, bits, four) = match sf {
        0 => (3, 10, false),
        1 => (3, 10, true),
        2 => (4, 14, true),
        _ => (5, 18, true),
    };
    let v = le(d.get(..h).ok_or(eof(0, h))?, 0, h) >> 4;
    let (regen, comp) = ((v & ((1 << bits) - 1)) as usize, (v >> bits & ((1 << bits) - 1)) as usize);
    if regen > BLOCK {
        return bad("literals too large");
    }
    let mut s = d.get(h..h + comp).ok_or(eof(h, comp))?;
    if kind == 2 {
        let (t, used) = Huf::read(s)?;
        c.huf = Some(t);
        s = &s[used..];
    }
    let t = c.huf.as_ref().ok_or(Error::Bad("missing huffman table"))?;
    let mut o = Vec::with_capacity(regen);
    if four {
        if s.len() < 6 {
            return Err(eof(0, 6));
        }
        let sz: Vec<usize> = (0..3).map(|i| le(s, 2 * i, 2) as usize).collect();
        let mut p = 6;
        for n in [sz[0], sz[1], sz[2], s.len().saturating_sub(6 + sz[0] + sz[1] + sz[2])] {
            t.stream(s.get(p..p + n).ok_or(eof(p, n))?, &mut o, regen)?;
            p += n;
        }
    } else {
        t.stream(s, &mut o, regen)?;
    }
    if o.len() != regen {
        return bad("literal count mismatch");
    }
    Ok((o, h + comp))
}

fn table(c: &mut Ctx, k: usize, mode: u8, d: &[u8]) -> Res<usize> {
    let (dist, log, max): (&[i16], u32, u32) = [(&LL_DIST[..], 6, 9), (&OF_DIST[..], 5, 8), (&ML_DIST[..], 6, 9)][k];
    let (t, used) = match mode {
        0 => (Fse::new(dist, log)?, 0),
        1 => (Fse::rle(*d.first().ok_or(eof(0, 1))?), 1),
        2 => Fse::read(d, max)?,
        _ => return if c.tabs[k].is_some() { Ok(0) } else { bad("missing repeat table") },
    };
    c.tabs[k] = Some(t);
    Ok(used)
}

fn block(c: &mut Ctx, d: &[u8], o: &mut Vec<u8>) -> Res<()> {
    let (lit, mut p) = literals(c, d)?;
    let h = *d.get(p).ok_or(eof(p, 1))? as usize;
    let n = match h {
        0..=127 => {
            p += 1;
            h
        }
        128..=254 => {
            p += 2;
            ((h - 128) << 8) + *d.get(p - 1).ok_or(eof(p, 1))? as usize
        }
        _ => {
            p += 3;
            le(d.get(p - 2..p).ok_or(eof(p, 2))?, 0, 2) as usize + 0x7F00
        }
    };
    if n == 0 {
        o.extend(lit);
        return Ok(());
    }
    let m = *d.get(p).ok_or(eof(p, 1))?;
    p += 1;
    if m & 3 != 0 {
        return bad("reserved sequence mode bits");
    }
    for (k, sh) in [(0, 6), (1, 4), (2, 2)] {
        p += table(c, k, (m >> sh) & 3, d.get(p..).unwrap_or(&[]))?;
    }
    let [ll, of, ml] = [0, 1, 2].map(|k| c.tabs[k].clone().unwrap());
    let mut b = Back::new(d.get(p..).ok_or(eof(p, 1))?)?;
    let (mut sl, mut so, mut sm) = (ll.init(&mut b), of.init(&mut b), ml.init(&mut b));
    let mut lp = 0;
    for i in 0..n {
        let (lc, oc, mc) = (ll.sym[sl] as usize, of.sym[so] as u32, ml.sym[sm] as usize);
        if lc > 35 || mc > 52 || oc > 31 {
            return bad("bad sequence code");
        }
        let ov = (1usize << oc) + b.bits(oc) as usize;
        let mlen = ML_BASE[mc] as usize + b.bits(ML_BITS[mc] as u32) as usize;
        let llen = LL_BASE[lc] as usize + b.bits(LL_BITS[lc] as u32) as usize;
        if i + 1 < n {
            ll.step(&mut sl, &mut b);
            ml.step(&mut sm, &mut b);
            of.step(&mut so, &mut b);
        }
        let l = lit.get(lp..lp + llen).ok_or(Error::Bad("literal overrun"))?;
        o.extend_from_slice(l);
        lp += llen;
        let off = rep(ov, llen, &mut c.rep);
        if off == 0 || off > o.len() {
            return bad("bad match offset");
        }
        let st = o.len() - off;
        if off >= mlen {
            o.extend_from_within(st..st + mlen);
        } else {
            for k in 0..mlen {
                o.push(o[st + k]);
            }
        }
    }
    if b.off != 0 {
        return bad("sequence bitstream not consumed");
    }
    o.extend_from_slice(&lit[lp..]);
    Ok(())
}

fn frame(d: &[u8], o: &mut Vec<u8>, max: usize) -> Res<usize> {
    let fd = *d.get(4).ok_or(eof(4, 1))?;
    let (fcs, single, check, did) = (fd >> 6, fd >> 5 & 1 == 1, fd >> 2 & 1 == 1, fd & 3);
    if fd & 8 != 0 {
        return bad("reserved frame bit");
    }
    let mut p = 5 + !single as usize + [0, 1, 2, 4][did as usize];
    let fn_ = [single as usize, 2, 4, 8][fcs as usize];
    let size = d.get(p..p + fn_).ok_or(eof(p, fn_))?;
    let size = match fn_ {
        0 => None,
        2 => Some(le(size, 0, 2) as usize + 256),
        n => Some(le(size, 0, n) as usize),
    };
    p += fn_;
    let start = o.len();
    let mut c = Ctx { rep: [1, 4, 8], ..Ctx::default() };
    loop {
        let h = le(d.get(p..p + 3).ok_or(eof(p, 3))?, 0, 3) as usize;
        p += 3;
        let (last, kind, n) = (h & 1 == 1, h >> 1 & 3, h >> 3);
        match kind {
            0 => o.extend_from_slice(d.get(p..p + n).ok_or(eof(p, n))?),
            1 => {
                let b = *d.get(p).ok_or(eof(p, 1))?;
                o.resize(o.len() + n, b);
            }
            2 => {
                if n > BLOCK {
                    return bad("block too large");
                }
                let mut v = std::mem::take(o);
                let r = block(&mut c, d.get(p..p + n).ok_or(eof(p, n))?, &mut v);
                *o = v;
                r?;
            }
            _ => return bad("reserved block type"),
        }
        p += if kind == 1 { 1 } else { n };
        if o.len() - start > max {
            return bad("output limit");
        }
        if last {
            break;
        }
    }
    if size.is_some_and(|s| s != o.len() - start) {
        return bad("frame size mismatch");
    }
    if check {
        let s = d.get(p..p + 4).ok_or(eof(p, 4))?;
        if le(s, 0, 4) as u32 != xxh64(&o[start..], 0) as u32 {
            return bad("zstd checksum mismatch");
        }
        p += 4;
    }
    Ok(p)
}

pub fn decode_max(d: &[u8], max: usize) -> Res<Vec<u8>> {
    let mut o = Vec::new();
    let mut p = 0;
    while p < d.len() {
        let m = le(d.get(p..p + 4).ok_or(eof(p, 4))?, 0, 4) as u32;
        if m & 0xFFFF_FFF0 == 0x184D_2A50 {
            let n = le(d.get(p + 4..p + 8).ok_or(eof(p + 4, 4))?, 0, 4) as usize;
            p += 8 + n;
            continue;
        }
        if m != MAGIC {
            return bad("bad zstd magic");
        }
        let left = max - o.len();
        p += frame(&d[p..], &mut o, left)?;
    }
    if p > d.len() {
        return Err(eof(d.len(), p - d.len()));
    }
    Ok(o)
}

pub fn decode(d: &[u8]) -> Res<Vec<u8>> {
    decode_max(d, usize::MAX)
}

struct Bw {
    o: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bw {
    fn new() -> Bw {
        Bw { o: Vec::new(), acc: 0, n: 0 }
    }

    fn put(&mut self, v: u64, k: u32) {
        if k == 0 {
            return;
        }
        self.acc |= (v & ((1u64 << k) - 1)) << self.n;
        self.n += k;
        while self.n >= 8 {
            self.o.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    fn close(mut self) -> Vec<u8> {
        self.put(1, 1);
        if self.n > 0 {
            self.o.push(self.acc as u8);
        }
        self.o
    }
}

struct Ct {
    table: Vec<u16>,
    dnb: Vec<u32>,
    dfs: Vec<i32>,
    log: u32,
}

impl Ct {
    fn new(norm: &[i16], log: u32) -> Ct {
        let size = 1u32 << log;
        let sym = spread(norm, log).unwrap();
        let mut cum = vec![0u32; norm.len() + 1];
        for (s, &n) in norm.iter().enumerate() {
            cum[s + 1] = cum[s] + if n == -1 { 1 } else { n.max(0) as u32 };
        }
        let mut at = cum.clone();
        let mut table = vec![0u16; size as usize];
        for (u, &s) in sym.iter().enumerate() {
            table[at[s as usize] as usize] = (size + u as u32) as u16;
            at[s as usize] += 1;
        }
        let (mut dnb, mut dfs) = (vec![0u32; norm.len()], vec![0i32; norm.len()]);
        for (s, &n) in norm.iter().enumerate() {
            match n {
                0 => {}
                -1 | 1 => {
                    dnb[s] = (log << 16).wrapping_sub(size);
                    dfs[s] = cum[s] as i32 - 1;
                }
                _ => {
                    let mb = log - hb(n as u64 - 1);
                    dnb[s] = (mb << 16).wrapping_sub((n as u32) << mb);
                    dfs[s] = cum[s] as i32 - n as i32;
                }
            }
        }
        Ct { table, dnb, dfs, log }
    }

    fn init(&self, s: usize) -> u32 {
        let nb = self.dnb[s].wrapping_add(1 << 15) >> 16;
        let v = (nb << 16).wrapping_sub(self.dnb[s]);
        self.table[((v >> nb) as i32 + self.dfs[s]) as usize] as u32
    }

    fn enc(&self, st: &mut u32, s: usize, w: &mut Bw) {
        let nb = st.wrapping_add(self.dnb[s]) >> 16;
        w.put(*st as u64, nb);
        *st = self.table[((*st >> nb) as i32 + self.dfs[s]) as usize] as u32;
    }

    fn flush(&self, st: u32, w: &mut Bw) {
        w.put(st as u64, self.log);
    }
}

fn code(base: &[u32], bits: &[u8], v: u32) -> usize {
    (0..base.len()).rev().find(|&i| base[i] <= v && v - base[i] < 1 << bits[i]).unwrap()
}

fn lit_header(kind: u8, n: usize) -> Vec<u8> {
    match n {
        0..=31 => vec![kind | (n as u8) << 3],
        32..=4095 => (kind as u32 | 1 << 2 | (n as u32) << 4).to_le_bytes()[..2].to_vec(),
        _ => (kind as u32 | 3 << 2 | (n as u32) << 4).to_le_bytes()[..3].to_vec(),
    }
}

fn huf_lits(l: &[u8]) -> Option<Vec<u8>> {
    let mut f = [0u32; 256];
    l.iter().for_each(|&b| f[b as usize] += 1);
    let last = (0..256).rev().find(|&i| f[i] > 0)?;
    if last > 128 || f.iter().filter(|&&x| x > 0).count() < 2 || l.len() < 64 {
        return None;
    }
    let len = lengths(&f[..=last], 11);
    let max = *len.iter().max().unwrap() as u32;
    let w: Vec<u8> = len[..last].iter().map(|&b| if b > 0 { (max + 1 - b as u32) as u8 } else { 0 }).collect();
    let mut tree = vec![127 + last as u8];
    tree.extend(w.chunks(2).map(|c| c[0] << 4 | c.get(1).copied().unwrap_or(0)));
    let t = Huf::from_bits(&len).ok()?;
    let mut code = vec![0u32; last + 1];
    for (i, &s) in t.sym.iter().enumerate().rev() {
        code[s as usize] = (i >> (max - t.nb[i] as u32)) as u32;
    }
    let stream = |seg: &[u8]| {
        let mut w = Bw::new();
        for &s in seg.iter().rev() {
            w.put(code[s as usize] as u64, len[s as usize] as u32);
        }
        w.close()
    };
    let four = l.len() > 1023;
    let mut body = tree;
    if four {
        let q = l.len().div_ceil(4);
        let s: Vec<Vec<u8>> = l.chunks(q).map(stream).collect();
        if s.len() != 4 || s[..3].iter().any(|x| x.len() > 0xFFFF) {
            return None;
        }
        s[..3].iter().for_each(|x| body.extend((x.len() as u16).to_le_bytes()));
        s.iter().for_each(|x| body.extend(x));
    } else {
        body.extend(stream(l));
    }
    let (sf, bits, h) = match (four, l.len().max(body.len())) {
        (false, n) if n < 1024 => (0u64, 10, 3),
        (false, _) => return None,
        (_, n) if n < 1024 => (1, 10, 3),
        (_, n) if n < 16384 => (2, 14, 4),
        (_, n) if n < 262_144 => (3, 18, 5),
        _ => return None,
    };
    let v = 2 | sf << 2 | (l.len() as u64) << 4 | (body.len() as u64) << (4 + bits);
    let mut o = v.to_le_bytes()[..h].to_vec();
    o.extend(body);
    Some(o)
}

fn lits(l: &[u8]) -> Vec<u8> {
    if !l.is_empty() && l.iter().all(|&b| b == l[0]) {
        let mut o = lit_header(1, l.len());
        o.push(l[0]);
        return o;
    }
    let mut raw = lit_header(0, l.len());
    raw.extend_from_slice(l);
    match huf_lits(l) {
        Some(h) if h.len() < raw.len() => h,
        _ => raw,
    }
}

struct Seq {
    ll: u32,
    ml: u32,
    ov: u32,
}

fn seqs(s: &[Seq], cts: &[Ct; 3]) -> Vec<u8> {
    let n = s.len();
    let mut o = match n {
        0..=127 => vec![n as u8],
        128..=0x7EFF => vec![(n >> 8) as u8 + 128, n as u8],
        _ => {
            let m = (n - 0x7F00) as u16;
            vec![255, m as u8, (m >> 8) as u8]
        }
    };
    if n == 0 {
        return o;
    }
    o.push(0);
    let codes: Vec<(usize, usize, usize)> =
        s.iter().map(|q| (code(&LL_BASE, &LL_BITS, q.ll), hb(q.ov as u64) as usize, code(&ML_BASE, &ML_BITS, q.ml))).collect();
    let [ll, of, ml] = cts;
    let mut w = Bw::new();
    let extra = |w: &mut Bw, q: &Seq, c: (usize, usize, usize)| {
        w.put((q.ll - LL_BASE[c.0]) as u64, LL_BITS[c.0] as u32);
        w.put((q.ml - ML_BASE[c.2]) as u64, ML_BITS[c.2] as u32);
        w.put((q.ov - (1 << c.1)) as u64, c.1 as u32);
    };
    let c = codes[n - 1];
    let (mut sm, mut so, mut sl) = (ml.init(c.2), of.init(c.1), ll.init(c.0));
    extra(&mut w, &s[n - 1], c);
    for i in (0..n - 1).rev() {
        let c = codes[i];
        of.enc(&mut so, c.1, &mut w);
        ml.enc(&mut sm, c.2, &mut w);
        ll.enc(&mut sl, c.0, &mut w);
        extra(&mut w, &s[i], c);
    }
    ml.flush(sm, &mut w);
    of.flush(so, &mut w);
    ll.flush(sl, &mut w);
    o.extend(w.close());
    o
}

fn hash(d: &[u8], i: usize) -> usize {
    (u32::from_le_bytes(d[i..i + 4].try_into().unwrap()).wrapping_mul(0x9E37_79B1) >> 15) as usize
}

const WIN_BITS: u32 = 20;

struct Enc {
    head: Vec<i32>,
    prev: Vec<i32>,
    depth: usize,
    rep: [usize; 3],
    cts: [Ct; 3],
}

impl Enc {
    fn insert(&mut self, d: &[u8], i: usize) {
        if i + 4 <= d.len() {
            let h = hash(d, i);
            self.prev[i & ((1 << WIN_BITS) - 1)] = self.head[h];
            self.head[h] = i as i32;
        }
    }

    fn find(&self, d: &[u8], i: usize, end: usize, ll: usize) -> (usize, usize) {
        let max = end - i;
        let run = |o: usize| (0..max).take_while(|&k| d[i - o + k] == d[i + k]).count();
        let mut best = (0, 0);
        let reps = if ll == 0 { [self.rep[1], self.rep[2], self.rep[0].wrapping_sub(1)] } else { self.rep };
        for o in reps {
            if o >= 1 && o <= i {
                let l = run(o);
                if l > best.0 {
                    best = (l, o);
                }
            }
        }
        if max >= 4 {
            let (mut p, mut chain) = (self.head[hash(d, i)], self.depth);
            while p >= 0 && chain > 0 && best.0 < max {
                let o = i - p as usize;
                if o >= 1 << WIN_BITS {
                    break;
                }
                if d[p as usize + best.0] == d[i + best.0] {
                    let l = run(o);
                    if l > best.0 + 1 {
                        best = (l, o);
                    }
                }
                p = self.prev[p as usize & ((1 << WIN_BITS) - 1)];
                chain -= 1;
            }
        }
        if best.0 >= 3 { best } else { (0, 0) }
    }

    fn value(&self, off: usize, ll: usize) -> u32 {
        let r = self.rep;
        let opts: [usize; 3] = if ll == 0 { [r[1], r[2], r[0].wrapping_sub(1)] } else { r };
        match opts.iter().position(|&x| x == off) {
            Some(k) => k as u32 + 1,
            None => off as u32 + 3,
        }
    }

    fn block(&mut self, d: &[u8], start: usize, end: usize) -> Vec<u8> {
        let (mut lit, mut sq) = (Vec::new(), Vec::new());
        let (mut i, mut anchor) = (start, start);
        while i < end {
            let m = self.find(d, i, end, i - anchor);
            if m.0 == 0 {
                self.insert(d, i);
                i += 1;
                continue;
            }
            if m.0 < 64 && i + 1 < end {
                self.insert(d, i);
                let n = self.find(d, i + 1, end, i + 1 - anchor);
                if n.0 > m.0 + 1 {
                    i += 1;
                    continue;
                }
                (i + 1..i + m.0).for_each(|k| self.insert(d, k));
            } else {
                (i..i + m.0).for_each(|k| self.insert(d, k));
            }
            let ll = i - anchor;
            let ov = self.value(m.1, ll);
            rep(ov as usize, ll, &mut self.rep);
            lit.extend_from_slice(&d[anchor..i]);
            sq.push(Seq { ll: ll as u32, ml: m.0 as u32, ov });
            i += m.0;
            anchor = i;
        }
        lit.extend_from_slice(&d[anchor..end]);
        let mut o = lits(&lit);
        o.extend(seqs(&sq, &self.cts));
        o
    }
}

pub fn encode(d: &[u8], level: u32) -> Vec<u8> {
    let n = d.len();
    let mut o = MAGIC.to_le_bytes().to_vec();
    let fcs: (u8, Vec<u8>) = match n {
        0..=255 => (0, vec![n as u8]),
        256..=65791 => (1, ((n - 256) as u16).to_le_bytes().to_vec()),
        _ if n <= u32::MAX as usize => (2, (n as u32).to_le_bytes().to_vec()),
        _ => (3, (n as u64).to_le_bytes().to_vec()),
    };
    o.push(fcs.0 << 6 | 1 << 5 | 1 << 2);
    o.extend(fcs.1);
    let mut e = Enc {
        head: vec![-1; 1 << 17],
        prev: vec![-1; 1 << WIN_BITS],
        depth: [1, 4, 8, 16, 32, 64, 128, 256, 512, 1024][level.min(9) as usize],
        rep: [1, 4, 8],
        cts: [Ct::new(&LL_DIST, 6), Ct::new(&OF_DIST, 5), Ct::new(&ML_DIST, 6)],
    };
    let mut s = 0;
    loop {
        let end = (s + BLOCK).min(n);
        let last = (end == n) as u32;
        let raw = &d[s..end];
        let saved = e.rep;
        let (kind, body) = if !raw.is_empty() && raw.iter().all(|&b| b == raw[0]) {
            (1, vec![raw[0]])
        } else {
            let c = e.block(d, s, end);
            if c.len() < raw.len() {
                (2, c)
            } else {
                e.rep = saved;
                (0, raw.to_vec())
            }
        };
        if kind == 1 {
            (s..end).for_each(|k| e.insert(d, k));
        }
        let size = if kind == 2 { body.len() } else { end - s };
        o.extend(&(last | kind << 1 | (size as u32) << 3).to_le_bytes()[..3]);
        o.extend(body);
        s = end;
        if last == 1 {
            break;
        }
    }
    o.extend(&(xxh64(d, 0) as u32).to_le_bytes());
    o
}

#[cfg(test)]
mod t {
    use super::*;

    fn gen(n: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        let mut o: Vec<u8> = Vec::with_capacity(n);
        while o.len() < n {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            match s >> 29 {
                0 | 1 if o.len() > 100 => {
                    let b = o.len() - 1 - (s >> 6) as usize % 5000.min(o.len() - 1);
                    for k in 0..3 + (s >> 3) as usize % 60 {
                        o.push(o[b + k % (o.len() - b)]);
                    }
                }
                2 => o.extend(b"Event.Wait(self, \"OnEnter\", callback)\n"),
                3 => o.push((s >> 20) as u8),
                _ => o.push(b'a' + (s >> 24) as u8 % 26),
            }
        }
        o.truncate(n);
        o
    }

    #[test]
    fn tables() {
        let f = Fse::new(&LL_DIST, 6).unwrap();
        assert_eq!(f.sym.len(), 64);
        assert_eq!(code(&LL_BASE, &LL_BITS, 17), 16);
        assert_eq!(code(&LL_BASE, &LL_BITS, 70000), 35);
        assert_eq!(code(&ML_BASE, &ML_BITS, 3), 0);
        assert_eq!(code(&ML_BASE, &ML_BITS, 100), 42);
        let mut r = [1, 4, 8];
        assert_eq!(rep(1, 5, &mut r), 1);
        assert_eq!(rep(1, 0, &mut r), 4);
        assert_eq!(r, [4, 1, 8]);
        assert_eq!(rep(3, 0, &mut r), 3);
        assert_eq!(r, [3, 4, 1]);
        assert_eq!(rep(10, 1, &mut r), 7);
        assert_eq!(r, [7, 3, 4]);
    }

    #[test]
    fn frames() {
        let empty = [0x28, 0xB5, 0x2F, 0xFD, 0x20, 0x00, 0x01, 0x00, 0x00];
        assert_eq!(decode(&empty).unwrap(), b"");
        let rle = [0x28, 0xB5, 0x2F, 0xFD, 0x20, 0x05, 0x2B, 0x00, 0x00, b'x'];
        assert_eq!(decode(&rle).unwrap(), b"xxxxx");
        let mut skip = vec![0x50, 0x2A, 0x4D, 0x18, 2, 0, 0, 0, 9, 9];
        skip.extend(rle);
        assert_eq!(decode(&skip).unwrap(), b"xxxxx");
        assert!(decode(&rle[..9]).is_err());
        assert!(decode_max(&rle, 3).is_err());
    }

    #[test]
    fn roundtrip() {
        for (n, s) in [(0, 1), (1, 2), (5, 3), (300, 4), (5000, 5), (BLOCK, 6), (BLOCK * 2 + 7, 7), (700_000, 8)] {
            let d = gen(n, s);
            for lv in [1, 6] {
                let z = encode(&d, lv);
                assert_eq!(decode(&z).unwrap(), d, "n={n} lv={lv}");
            }
        }
        let txt = b"local t = {} for i = 1, 10 do t[i] = i end\n".repeat(3000);
        assert!(encode(&txt, 6).len() < txt.len() / 30);
        let z = encode(&vec![5u8; 300_000], 6);
        assert!(z.len() < 40);
    }

    #[test]
    fn garbage_is_error() {
        let d = gen(50_000, 9);
        let z = encode(&d, 6);
        for k in [6usize, 20, 100, 3000, z.len() - 2] {
            let mut b = z.clone();
            b[k] ^= 0x41;
            assert!(decode(&b).map_or(true, |v| v != d));
        }
    }
}

#[cfg(test)]
mod real {
    use super::*;
    use crate::lzx::pairs;

    #[test]
    #[ignore]
    fn reference() {
        let v = pairs("zref", "zst");
        for (p, z, raw) in &v {
            assert_eq!(&decode(z).unwrap(), raw, "{}", p.display());
        }
        let Ok(g) = std::env::var("AC_GOLDEN") else { return };
        let raws: Vec<_> = std::fs::read_dir(std::path::Path::new(&g).join("zref")).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "raw")).collect();
        for p in &raws {
            let d = std::fs::read(p).unwrap();
            let m = encode(&d, 6);
            assert_eq!(decode(&m).unwrap(), d);
            std::fs::write(p.with_extension("mine"), m).unwrap();
        }
        println!("zstd frames {} encoded {}", v.len(), raws.len());
    }
}
