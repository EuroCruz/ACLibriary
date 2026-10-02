use crate::huff::{canon, full, Dec};
use ac_core::{bad, Error, Res};

pub const CHUNK: usize = 32768;
pub const BITS: u32 = 17;
const MAIN: usize = 256;
const LENS: usize = 249;
const MAX_LEN: usize = 257;

fn slots(bits: u32) -> usize {
    match bits {
        0..=19 => 2 * bits as usize,
        20 => 42,
        _ => 50,
    }
}

fn extra(s: usize) -> u32 {
    if s < 4 { 0 } else { ((s as u32 - 2) / 2).min(17) }
}

fn bases(n: usize) -> Vec<u32> {
    let mut b = vec![0u32; n + 1];
    for i in 0..n {
        b[i + 1] = b[i] + (1 << extra(i));
    }
    b
}

struct Br<'a> {
    d: &'a [u8],
    p: usize,
    acc: u32,
    n: u32,
}

impl Br<'_> {
    fn bits(&mut self, k: u32) -> u32 {
        if k > 16 {
            let hi = self.bits(16);
            return hi << (k - 16) | self.bits(k - 16);
        }
        if k == 0 {
            return 0;
        }
        if self.n < k {
            let w = self.d.get(self.p..self.p + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as u32);
            self.p += 2;
            self.acc = self.acc << 16 | w;
            self.n += 16;
        }
        self.n -= k;
        self.acc >> self.n & ((1u32 << k) - 1)
    }

    fn bit(&mut self) -> Res<u32> {
        Ok(self.bits(1))
    }

    fn align(&mut self) {
        if self.n == 0 {
            self.bits(16);
        }
        self.n = 0;
    }

    fn raw(&mut self, k: usize) -> Res<&[u8]> {
        let s = self.d.get(self.p..self.p + k).ok_or(Error::Eof { at: self.p, need: k })?;
        self.p += k;
        Ok(s)
    }
}

fn tree(b: &mut Br, l: &mut [u8], from: usize, to: usize) -> Res<()> {
    let pl: Vec<u8> = (0..20).map(|_| b.bits(4) as u8).collect();
    let pd = Dec::new(&pl)?;
    let mut i = from;
    while i < to {
        let c = pd.decode(|| b.bit())? as u8;
        let (k, v) = match c {
            0..=16 => (1, (l[i] + 17 - c) % 17),
            17 => (4 + b.bits(4) as usize, 0),
            18 => (20 + b.bits(5) as usize, 0),
            _ => {
                let k = 4 + b.bits(1) as usize;
                let c = pd.decode(|| b.bit())? as u8;
                if c > 16 {
                    return bad("bad pretree run");
                }
                (k, (l[i] + 17 - c) % 17)
            }
        };
        if i + k > to {
            return bad("tree run overflow");
        }
        l[i..i + k].fill(v);
        i += k;
    }
    Ok(())
}

pub struct Lzx {
    win: usize,
    base: Vec<u32>,
    h: Vec<u8>,
    at: usize,
    out: usize,
    r: [u32; 3],
    main: Vec<u8>,
    lens: Vec<u8>,
    kind: u8,
    size: usize,
    left: usize,
    md: Option<Dec>,
    ld: Option<Dec>,
    ad: Option<Dec>,
    started: bool,
    e8: Option<i32>,
}

impl Lzx {
    pub fn new(bits: u32) -> Lzx {
        let n = slots(bits);
        Lzx {
            win: 1 << bits,
            base: bases(n),
            h: Vec::new(),
            at: 0,
            out: 0,
            r: [1; 3],
            main: vec![0; MAIN + 8 * n],
            lens: vec![0; LENS],
            kind: 0,
            size: 0,
            left: 0,
            md: None,
            ld: None,
            ad: None,
            started: false,
            e8: None,
        }
    }

    fn header(&mut self, b: &mut Br) -> Res<()> {
        self.kind = b.bits(3) as u8;
        self.size = b.bits(24) as usize;
        self.left = self.size;
        if self.size == 0 {
            return bad("empty lzx block");
        }
        match self.kind {
            1 | 2 => {
                self.ad = match self.kind {
                    2 => Some(Dec::new(&(0..8).map(|_| b.bits(3) as u8).collect::<Vec<_>>())?),
                    _ => None,
                };
                let n = self.main.len();
                tree(b, &mut self.main, 0, MAIN)?;
                tree(b, &mut self.main, MAIN, n)?;
                tree(b, &mut self.lens, 0, LENS)?;
                self.md = Some(Dec::new(&self.main)?);
                self.ld = if self.lens.iter().all(|&x| x == 0) { None } else { Some(Dec::new(&self.lens)?) };
            }
            3 => {
                b.align();
                for i in 0..3 {
                    self.r[i] = u32::from_le_bytes(b.raw(4)?.try_into().unwrap());
                }
            }
            _ => return bad("bad lzx block type"),
        }
        Ok(())
    }

    fn end(&self) -> usize {
        self.at + self.h.len()
    }

    fn step(&mut self, b: &mut Br, want: usize) -> Res<()> {
        if self.kind == 3 {
            let k = self.left.min(want).min(b.d.len().saturating_sub(b.p));
            if k == 0 {
                return bad("truncated lzx raw block");
            }
            let s = b.raw(k)?;
            self.h.extend_from_slice(s);
            self.left -= k;
            return Ok(());
        }
        let m = self.md.as_ref().ok_or(Error::Bad("missing main tree"))?.decode(|| b.bit())? as usize;
        if m < MAIN {
            self.h.push(m as u8);
            self.left -= 1;
            return Ok(());
        }
        let (hdr, slot) = ((m - MAIN) & 7, (m - MAIN) >> 3);
        let len = match hdr {
            7 => self.ld.as_ref().ok_or(Error::Bad("empty length tree"))?.decode(|| b.bit())? as usize + 9,
            _ => hdr + 2,
        };
        let off = match slot {
            0 => self.r[0],
            1 | 2 => {
                self.r.swap(0, slot);
                self.r[0]
            }
            _ => {
                let e = extra(slot);
                let f = self.base[slot]
                    + match &self.ad {
                        Some(a) if e >= 3 => (b.bits(e - 3) << 3) + a.decode(|| b.bit())? as u32,
                        _ => b.bits(e),
                    };
                self.r = [f - 2, self.r[0], self.r[1]];
                f - 2
            }
        };
        let off = off as usize;
        if off == 0 || off > self.end() || off > self.h.len() || len > self.left {
            return bad("bad lzx match");
        }
        let st = self.h.len() - off;
        for k in 0..len {
            self.h.push(self.h[st + k]);
        }
        self.left -= len;
        Ok(())
    }

    pub fn chunk(&mut self, src: &[u8], n: usize) -> Res<Vec<u8>> {
        let mut b = Br { d: src, p: 0, acc: 0, n: 0 };
        if !self.started {
            self.started = true;
            if b.bits(1) == 1 {
                self.e8 = Some(b.bits(32) as i32);
            }
        }
        while self.end() < self.out + n {
            if self.left == 0 {
                if self.kind == 3 && self.size % 2 == 1 {
                    b.p += 1;
                }
                self.header(&mut b)?;
            }
            self.step(&mut b, self.out + n - self.end())?;
            if b.p > src.len() + 2 {
                return bad("lzx input overrun");
            }
        }
        let s = self.out - self.at;
        let mut o = self.h[s..s + n].to_vec();
        if let Some(size) = self.e8 {
            e8(&mut o, self.out, size);
        }
        self.out += n;
        if self.h.len() > 2 * self.win {
            let k = self.h.len() - self.win;
            self.h.drain(..k);
            self.at += k;
        }
        Ok(o)
    }
}

fn e8(o: &mut [u8], at: usize, size: i32) {
    if at >= 0x4000_0000 || o.len() <= 10 {
        return;
    }
    let mut i = 0;
    while i < o.len() - 10 {
        if o[i] != 0xE8 {
            i += 1;
            continue;
        }
        let abs = i32::from_le_bytes(o[i + 1..i + 5].try_into().unwrap());
        let cur = (at + i) as i32;
        if abs >= -cur && abs < size {
            let rel = if abs >= 0 { abs - cur } else { abs + size };
            o[i + 1..i + 5].copy_from_slice(&rel.to_le_bytes());
        }
        i += 5;
    }
}

pub fn decode(src: &[u8], n: usize) -> Res<Vec<u8>> {
    let mut z = Lzx::new(BITS);
    let mut o = Vec::with_capacity(n);
    let mut p = 0;
    let eof = |p: usize, k: usize| Error::Eof { at: p, need: k };
    while o.len() < n {
        let h = src.get(p..p + 2).ok_or(eof(p, 2))?;
        let (hl, ul, cl) = if h[0] == 0xFF {
            let x = src.get(p + 1..p + 5).ok_or(eof(p, 5))?;
            (5, u16::from_be_bytes([x[0], x[1]]) as usize, u16::from_be_bytes([x[2], x[3]]) as usize)
        } else {
            (2, CHUNK.min(n - o.len()), u16::from_be_bytes([h[0], h[1]]) as usize)
        };
        if ul == 0 || ul > n - o.len() {
            return bad("bad lzx chunk size");
        }
        let c = src.get(p + hl..p + hl + cl).ok_or(eof(p + hl, cl))?;
        o.extend(z.chunk(c, ul)?);
        p += hl + cl;
    }
    Ok(o)
}

struct Bw {
    o: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bw {
    fn put(&mut self, v: u32, k: u32) {
        if k > 16 {
            self.put(v >> 16, k - 16);
            return self.put(v & 0xffff, 16);
        }
        if k == 0 {
            return;
        }
        self.acc = self.acc << k | v & ((1u32 << k) - 1);
        self.n += k;
        if self.n >= 16 {
            self.n -= 16;
            self.o.extend(((self.acc >> self.n) as u16).to_le_bytes());
        }
    }

    fn flush(&mut self) {
        if self.n > 0 {
            self.put(0, 16 - self.n);
        }
    }
}

#[derive(Clone, Copy)]
struct Tok {
    main: u16,
    len: u16,
    foot: u32,
    fb: u8,
}

fn put_tree(w: &mut Bw, old: &[u8], new: &[u8]) {
    let mut syms: Vec<(u8, u32, u32)> = Vec::new();
    let mut i = 0;
    while i < new.len() {
        let z = new[i..].iter().take_while(|&&x| x == 0).count();
        if z >= 20 {
            let k = z.min(51);
            syms.push((18, (k - 20) as u32, 5));
            i += k;
        } else if z >= 4 {
            let k = z.min(19);
            syms.push((17, (k - 4) as u32, 4));
            i += k;
        } else {
            syms.push(((old[i] + 17 - new[i]) % 17, 0, 0));
            i += 1;
        }
    }
    let mut f = [0u32; 20];
    syms.iter().for_each(|s| f[s.0 as usize] += 1);
    let pl = full(&f, 15);
    let pc = canon(&pl);
    pl.iter().for_each(|&l| w.put(l as u32, 4));
    for (s, x, k) in syms {
        w.put(pc[s as usize] as u32, pl[s as usize] as u32);
        w.put(x, k);
    }
}

fn hash(d: &[u8], i: usize) -> usize {
    let v = d[i] as u32 | (d[i + 1] as u32) << 8 | (d[i + 2] as u32) << 16;
    (v.wrapping_mul(0x9E37_79B1) >> 16) as usize
}

pub struct Enc {
    bits: u32,
    base: Vec<u32>,
    r: [u32; 3],
    main: Vec<u8>,
    lens: Vec<u8>,
    head: Vec<i32>,
    prev: Vec<i32>,
    depth: usize,
}

impl Enc {
    pub fn new(bits: u32, level: u32) -> Enc {
        let n = slots(bits);
        Enc {
            bits,
            base: bases(n),
            r: [1; 3],
            main: vec![0; MAIN + 8 * n],
            lens: vec![0; LENS],
            head: vec![-1; 1 << 16],
            prev: vec![-1; 1 << bits],
            depth: [1, 4, 8, 16, 32, 64, 128, 256, 512, 1024][level.min(9) as usize],
        }
    }

    fn insert(&mut self, d: &[u8], i: usize) {
        if i + 2 < d.len() {
            let h = hash(d, i);
            self.prev[i & ((1 << self.bits) - 1)] = self.head[h];
            self.head[h] = i as i32;
        }
    }

    fn find(&self, d: &[u8], i: usize, end: usize) -> (usize, usize) {
        let max = (end - i).min(MAX_LEN);
        let lim = ((1usize << self.bits) - 3).min(i);
        let run = |o: usize| (0..max).take_while(|&k| d[i - o + k] == d[i + k]).count();
        let mut best = (0, 0);
        for &o in &self.r {
            let o = o as usize;
            if o >= 1 && o <= lim {
                let l = run(o);
                if l > best.0 {
                    best = (l, o);
                }
            }
        }
        if max >= 3 {
            let (mut p, mut chain) = (self.head[hash(d, i)], self.depth);
            while p >= 0 && chain > 0 && best.0 < max {
                let o = i - p as usize;
                if o > lim {
                    break;
                }
                if d[p as usize + best.0] == d[i + best.0] {
                    let l = run(o);
                    if l > best.0 + (self.r.contains(&(best.1 as u32)) as usize) {
                        best = (l, o);
                    }
                }
                p = self.prev[p as usize & ((1 << self.bits) - 1)];
                chain -= 1;
            }
        }
        let ok = best.0 >= 3 || (best.0 == 2 && self.r.contains(&(best.1 as u32)));
        if ok { best } else { (0, 0) }
    }

    fn token(&mut self, len: usize, off: usize) -> Tok {
        let o = off as u32;
        let (slot, foot, fb) = match self.r.iter().position(|&x| x == o) {
            Some(s) => {
                self.r.swap(0, s);
                (s, 0, 0)
            }
            None => {
                let f = o + 2;
                let s = (3..self.base.len() - 1).find(|&s| self.base[s + 1] > f).unwrap();
                self.r = [o, self.r[0], self.r[1]];
                (s, f - self.base[s], extra(s) as u8)
            }
        };
        let hdr = (len - 2).min(7);
        Tok { main: (MAIN + (slot << 3 | hdr)) as u16, len: if hdr == 7 { (len - 9) as u16 } else { u16::MAX }, foot, fb }
    }

    pub fn chunk(&mut self, d: &[u8], start: usize) -> Vec<u8> {
        let end = (start + CHUNK).min(d.len());
        let mut toks = Vec::new();
        let mut i = start;
        while i < end {
            let m = self.find(d, i, end);
            if m.0 == 0 {
                toks.push(Tok { main: d[i] as u16, len: u16::MAX, foot: 0, fb: 0 });
                self.insert(d, i);
                i += 1;
                continue;
            }
            if m.0 < 32 && i + 1 < end {
                self.insert(d, i);
                let n = self.find(d, i + 1, end);
                if n.0 > m.0 + 1 {
                    toks.push(Tok { main: d[i] as u16, len: u16::MAX, foot: 0, fb: 0 });
                    i += 1;
                    continue;
                }
                toks.push(self.token(m.0, m.1));
                (i + 1..i + m.0).for_each(|k| self.insert(d, k));
            } else {
                toks.push(self.token(m.0, m.1));
                (i..i + m.0).for_each(|k| self.insert(d, k));
            }
            i += m.0;
        }
        let (mut mf, mut lf) = (vec![0u32; self.main.len()], vec![0u32; LENS]);
        for t in &toks {
            mf[t.main as usize] += 1;
            if t.len != u16::MAX {
                lf[t.len as usize] += 1;
            }
        }
        let ml = full(&mf, 16);
        let ll = if lf.iter().any(|&x| x > 0) { full(&lf, 16) } else { vec![0; LENS] };
        let mut w = Bw { o: Vec::new(), acc: 0, n: 0 };
        if start == 0 {
            w.put(0, 1);
        }
        w.put(1, 3);
        w.put((end - start) as u32, 24);
        put_tree(&mut w, &self.main[..MAIN], &ml[..MAIN]);
        put_tree(&mut w, &self.main[MAIN..], &ml[MAIN..]);
        put_tree(&mut w, &self.lens, &ll);
        let (mc, lc) = (canon(&ml), canon(&ll));
        for t in &toks {
            w.put(mc[t.main as usize] as u32, ml[t.main as usize] as u32);
            if t.len != u16::MAX {
                w.put(lc[t.len as usize] as u32, ll[t.len as usize] as u32);
            }
            w.put(t.foot, t.fb as u32);
        }
        w.flush();
        self.main = ml;
        self.lens = ll;
        w.o
    }
}

pub fn encode(d: &[u8], level: u32) -> Vec<u8> {
    let mut e = Enc::new(BITS, level);
    let mut o = Vec::with_capacity(d.len() / 2 + 16);
    for s in (0..d.len()).step_by(CHUNK) {
        let c = e.chunk(d, s);
        let n = (d.len() - s).min(CHUNK);
        if n != CHUNK {
            o.push(0xFF);
            o.extend((n as u16).to_be_bytes());
        }
        o.extend((c.len() as u16).to_be_bytes());
        o.extend(c);
    }
    o
}

#[cfg(test)]
mod t {
    use super::*;

    fn gen(n: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        let mut o = Vec::with_capacity(n);
        while o.len() < n {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            match s >> 30 {
                0 if o.len() > 300 => {
                    let (b, l) = (o.len() - 1 - (s >> 8) as usize % 300.min(o.len() - 1), 2 + (s >> 4) as usize % 300);
                    for k in 0..l {
                        o.push(o[b + k % (o.len() - b)]);
                    }
                }
                1 => o.extend(b"function Object:Update(dt) end\n"),
                _ => o.push((s >> 16) as u8),
            }
        }
        o.truncate(n);
        o
    }

    #[test]
    fn tables() {
        assert_eq!(slots(17), 34);
        let b = bases(34);
        assert_eq!(&b[..8], [0, 1, 2, 3, 4, 6, 8, 12]);
        assert_eq!(b[34], 1 << 17);
    }

    #[test]
    fn raw_block() {
        let mut w = Bw { o: Vec::new(), acc: 0, n: 0 };
        w.put(0, 1);
        w.put(3, 3);
        w.put(5, 24);
        w.flush();
        let mut v = w.o;
        [1u32, 1, 1].iter().for_each(|r| v.extend(r.to_le_bytes()));
        v.extend(b"hello\0");
        assert_eq!(Lzx::new(17).chunk(&v, 5).unwrap(), b"hello");
    }

    #[test]
    fn roundtrip() {
        for (n, s) in [(0, 1), (1, 2), (2, 3), (100, 4), (CHUNK, 5), (CHUNK + 1, 6), (300_000, 7)] {
            let d = gen(n, s);
            for lv in [1, 6, 9] {
                let z = encode(&d, lv);
                assert_eq!(decode(&z, n).unwrap(), d, "n={n} lv={lv}");
            }
        }
        let rep = vec![7u8; 200_000];
        let z = encode(&rep, 6);
        assert!(z.len() < 2000, "{}", z.len());
        assert_eq!(decode(&z, rep.len()).unwrap(), rep);
        let txt: Vec<u8> = b"local t = {} for i = 1, 10 do t[i] = i end\n".repeat(3000);
        assert!(encode(&txt, 9).len() < txt.len() / 20);
    }

    #[test]
    fn garbage_is_error() {
        let d = gen(70_000, 9);
        let z = encode(&d, 6);
        for k in [3usize, 50, 400, 9000] {
            let mut b = z.clone();
            b[k % z.len()] ^= 0x5a;
            let _ = decode(&b, d.len());
        }
        assert!(decode(&z[..z.len() / 2], d.len()).is_err());
        assert!(decode(&[0xff, 0, 0, 0, 0], 4).is_err());
    }
}

#[cfg(test)]
pub(crate) fn pairs(sub: &str, ext: &str) -> Vec<(std::path::PathBuf, Vec<u8>, Vec<u8>)> {
    let Ok(g) = std::env::var("AC_GOLDEN") else { return Vec::new() };
    let mut v: Vec<_> = std::fs::read_dir(std::path::Path::new(&g).join(sub)).into_iter().flatten().flatten().map(|e| e.path()).collect();
    v.sort();
    v.into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == ext))
        .filter_map(|p| Some((p.clone(), std::fs::read(&p).ok()?, std::fs::read(p.with_extension("")).ok()?)))
        .collect()
}

#[cfg(test)]
mod real {
    use super::*;

    #[test]
    #[ignore]
    fn reference() {
        let v = pairs("lzx", "lzx");
        let (mut theirs, mut mine) = (0, 0);
        for (p, z, raw) in &v {
            assert_eq!(&decode(z, raw.len()).unwrap(), raw, "{}", p.display());
            let m = encode(raw, 6);
            assert_eq!(&decode(&m, raw.len()).unwrap(), raw);
            std::fs::write(p.with_extension("mine"), &m).unwrap();
            (theirs, mine) = (theirs + z.len(), mine + m.len());
        }
        println!("lzx pairs {}: reference {theirs} mine {mine}", v.len());
    }
}
