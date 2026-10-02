use super::bits::{ilog, Br};
use super::book::Book;
use super::mdct::{window, Mdct};
use ac_core::{bad, Error, Res};

pub struct Floor1 {
    pub parts: Vec<usize>,
    pub cdim: Vec<usize>,
    pub csub: Vec<u32>,
    pub cmaster: Vec<usize>,
    pub sbooks: Vec<Vec<i32>>,
    pub mult: u32,
    pub xs: Vec<u32>,
}

pub struct Floor0 {
    order: usize,
    rate: u32,
    bark: u32,
    abits: u32,
    aoff: u32,
    books: Vec<usize>,
}

pub enum Floor {
    F0(Floor0),
    F1(Floor1),
}

pub struct Residue {
    pub kind: u32,
    pub begin: usize,
    pub end: usize,
    pub psize: usize,
    pub classes: usize,
    pub classbook: usize,
    pub books: Vec<[i32; 8]>,
}

pub struct Mapping {
    pub mux: Vec<usize>,
    pub submaps: Vec<(usize, usize)>,
    pub coupling: Vec<(usize, usize)>,
}

pub struct Mode {
    pub long: bool,
    pub mapping: usize,
}

pub struct Setup {
    pub channels: usize,
    pub rate: u32,
    pub bs: [usize; 2],
    pub vendor: String,
    pub comments: Vec<String>,
    pub books: Vec<Book>,
    pub floors: Vec<Floor>,
    pub residues: Vec<Residue>,
    pub maps: Vec<Mapping>,
    pub modes: Vec<Mode>,
}

fn e<T>(v: Option<T>) -> Res<T> {
    v.ok_or(Error::Bad("vorbis: truncated header"))
}

fn magic(r: &mut Br, t: u32) -> Res<()> {
    if e(r.get(8))? != t || (0..6).map(|_| r.get(8).unwrap_or(0) as u8).collect::<Vec<_>>() != b"vorbis" {
        return bad("vorbis: bad header packet");
    }
    Ok(())
}

pub fn ident(p: &[u8]) -> Res<(usize, u32, [usize; 2])> {
    let mut r = Br::new(p);
    magic(&mut r, 1)?;
    if e(r.get(32))? != 0 {
        return bad("vorbis: unsupported version");
    }
    let ch = e(r.get(8))? as usize;
    let rate = e(r.get(32))?;
    r.get(32);
    r.get(32);
    r.get(32);
    let (b0, b1) = (e(r.get(4))?, e(r.get(4))?);
    if ch == 0 || rate == 0 || !(6..=13).contains(&b0) || !(6..=13).contains(&b1) || b0 > b1 {
        return bad("vorbis: bad identification header");
    }
    Ok((ch, rate, [1 << b0, 1 << b1]))
}

fn text(r: &mut Br, max: usize) -> Res<String> {
    let n = e(r.get(32))? as usize;
    if n > max {
        return bad("vorbis: bad comment length");
    }
    let b: Vec<u8> = (0..n).map(|_| r.get(8).map(|x| x as u8)).collect::<Option<_>>().ok_or(Error::Bad("vorbis: truncated comment"))?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

pub fn comments(p: &[u8]) -> Res<(String, Vec<String>)> {
    let mut r = Br::new(p);
    magic(&mut r, 3)?;
    let vendor = text(&mut r, p.len())?;
    let n = e(r.get(32))? as usize;
    let out = (0..n.min(p.len())).map(|_| text(&mut r, p.len())).collect::<Res<_>>()?;
    Ok((vendor, out))
}

pub fn setup(p: &[u8], ch: usize) -> Res<(Vec<Book>, Vec<Floor>, Vec<Residue>, Vec<Mapping>, Vec<Mode>)> {
    let mut r = Br::new(p);
    magic(&mut r, 5)?;
    let g = |r: &mut Br, n| e(r.get(n));
    let books: Vec<Book> = (0..g(&mut r, 8)? + 1).map(|_| Book::parse(&mut r)).collect::<Res<_>>()?;
    let nb = books.len();
    let bk = |v: u32| if (v as usize) < nb { Ok(v as usize) } else { bad("vorbis: bad book index") };
    for _ in 0..g(&mut r, 6)? + 1 {
        if g(&mut r, 16)? != 0 {
            return bad("vorbis: bad time domain");
        }
    }
    let mut floors = Vec::new();
    for _ in 0..g(&mut r, 6)? + 1 {
        match g(&mut r, 16)? {
            0 => {
                let (order, rate, bark, abits, aoff) = (g(&mut r, 8)? as usize, g(&mut r, 16)?, g(&mut r, 16)?, g(&mut r, 6)?, g(&mut r, 8)?);
                let books = (0..g(&mut r, 4)? + 1).map(|_| bk(g(&mut r, 8)?)).collect::<Res<_>>()?;
                floors.push(Floor::F0(Floor0 { order, rate, bark, abits, aoff, books }));
            }
            1 => {
                let parts: Vec<usize> = (0..g(&mut r, 5)?).map(|_| g(&mut r, 4).map(|x| x as usize)).collect::<Res<_>>()?;
                let nc = parts.iter().max().map_or(0, |m| m + 1);
                let (mut cdim, mut csub, mut cmaster, mut sbooks) = (vec![], vec![], vec![], vec![]);
                for _ in 0..nc {
                    cdim.push(g(&mut r, 3)? as usize + 1);
                    let s = g(&mut r, 2)?;
                    csub.push(s);
                    cmaster.push(if s > 0 { bk(g(&mut r, 8)?)? } else { 0 });
                    sbooks.push((0..1 << s).map(|_| g(&mut r, 8).and_then(|b| if b == 0 { Ok(-1) } else { bk(b - 1).map(|x| x as i32) })).collect::<Res<Vec<i32>>>()?);
                }
                let mult = g(&mut r, 2)? + 1;
                let rb = g(&mut r, 4)?;
                let mut xs = vec![0, 1 << rb];
                for &c in &parts {
                    for _ in 0..cdim[c] {
                        xs.push(g(&mut r, rb)?);
                    }
                }
                if xs.len() > 65 {
                    return bad("vorbis: too many floor points");
                }
                floors.push(Floor::F1(Floor1 { parts, cdim, csub, cmaster, sbooks, mult, xs }));
            }
            _ => return bad("vorbis: bad floor type"),
        }
    }
    let mut residues = Vec::new();
    for _ in 0..g(&mut r, 6)? + 1 {
        let kind = g(&mut r, 16)?;
        if kind > 2 {
            return bad("vorbis: bad residue type");
        }
        let (begin, end, psize, classes, classbook) = (g(&mut r, 24)? as usize, g(&mut r, 24)? as usize, g(&mut r, 24)? as usize + 1, g(&mut r, 6)? as usize + 1, bk(g(&mut r, 8)?)?);
        let casc: Vec<u32> = (0..classes).map(|_| -> Res<u32> {
            let lo = g(&mut r, 3)?;
            Ok(if g(&mut r, 1)? == 1 { g(&mut r, 5)? << 3 | lo } else { lo })
        }).collect::<Res<_>>()?;
        let mut books = Vec::new();
        for c in casc {
            let mut b = [-1i32; 8];
            for (j, x) in b.iter_mut().enumerate() {
                if c >> j & 1 == 1 {
                    *x = bk(g(&mut r, 8)?)? as i32;
                }
            }
            books.push(b);
        }
        residues.push(Residue { kind, begin, end, psize, classes, classbook, books });
    }
    let mut maps = Vec::new();
    for _ in 0..g(&mut r, 6)? + 1 {
        if g(&mut r, 16)? != 0 {
            return bad("vorbis: bad mapping type");
        }
        let ns = if g(&mut r, 1)? == 1 { g(&mut r, 4)? as usize + 1 } else { 1 };
        let mut coupling = Vec::new();
        if g(&mut r, 1)? == 1 {
            let bits = ilog(ch as u32 - 1);
            for _ in 0..g(&mut r, 8)? + 1 {
                let (m, a) = (g(&mut r, bits)? as usize, g(&mut r, bits)? as usize);
                if m == a || m >= ch || a >= ch {
                    return bad("vorbis: bad coupling");
                }
                coupling.push((m, a));
            }
        }
        if g(&mut r, 2)? != 0 {
            return bad("vorbis: bad mapping reserved");
        }
        let mux = if ns > 1 { (0..ch).map(|_| g(&mut r, 4).map(|x| x as usize)).collect::<Res<Vec<_>>>()? } else { vec![0; ch] };
        if mux.iter().any(|&m| m >= ns) {
            return bad("vorbis: bad mux");
        }
        let mut submaps = Vec::new();
        for _ in 0..ns {
            g(&mut r, 8)?;
            let (f, res) = (g(&mut r, 8)? as usize, g(&mut r, 8)? as usize);
            if f >= floors.len() || res >= residues.len() {
                return bad("vorbis: bad submap");
            }
            submaps.push((f, res));
        }
        maps.push(Mapping { mux, submaps, coupling });
    }
    let mut modes = Vec::new();
    for _ in 0..g(&mut r, 6)? + 1 {
        let long = g(&mut r, 1)? == 1;
        g(&mut r, 16)?;
        g(&mut r, 16)?;
        let mapping = g(&mut r, 8)? as usize;
        if mapping >= maps.len() {
            return bad("vorbis: bad mode");
        }
        modes.push(Mode { long, mapping });
    }
    if g(&mut r, 1)? != 1 {
        return bad("vorbis: missing framing bit");
    }
    Ok((books, floors, residues, maps, modes))
}

pub fn neighbors(xs: &[u32], i: usize) -> (usize, usize) {
    let (mut lo, mut hi) = (0, 1);
    for j in 0..i {
        if xs[j] < xs[i] && xs[j] > xs[lo] {
            lo = j;
        }
        if xs[j] > xs[i] && xs[j] < xs[hi] {
            hi = j;
        }
    }
    (lo, hi)
}

pub fn point(x0: i32, y0: i32, x1: i32, y1: i32, x: i32) -> i32 {
    let (dy, adx) = (y1 - y0, x1 - x0);
    let off = dy.abs() * (x - x0) / adx;
    if dy < 0 { y0 - off } else { y0 + off }
}

pub fn line(x0: i32, y0: i32, x1: i32, y1: i32, v: &mut [i32]) {
    let (dy, adx) = (y1 - y0, x1 - x0);
    let base = dy / adx;
    let sy = if dy < 0 { base - 1 } else { base + 1 };
    let ady = dy.abs() - base.abs() * adx;
    let (mut y, mut err) = (y0, 0);
    if let Some(s) = v.get_mut(x0 as usize) {
        *s = y;
    }
    for x in x0 + 1..x1 {
        err += ady;
        if err >= adx {
            err -= adx;
            y += sy;
        } else {
            y += base;
        }
        if let Some(s) = v.get_mut(x as usize) {
            *s = y;
        }
    }
}

pub const RANGE: [i32; 4] = [256, 128, 86, 64];

pub fn db(i: i32) -> f32 {
    static T: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| std::array::from_fn(|k| (1.0649863e-07f64.ln() * (255 - k) as f64 / 255.0).exp() as f32))[i.clamp(0, 255) as usize]
}

impl Floor1 {
    pub fn unpack(&self, ys: &[i32]) -> (Vec<i32>, Vec<bool>) {
        let range = RANGE[self.mult as usize - 1];
        let n = self.xs.len();
        let (mut fy, mut ok) = (vec![0i32; n], vec![false; n]);
        fy[0] = ys[0];
        fy[1] = ys[1];
        ok[0] = true;
        ok[1] = true;
        for i in 2..n {
            let (lo, hi) = neighbors(&self.xs, i);
            let pred = point(self.xs[lo] as i32, fy[lo], self.xs[hi] as i32, fy[hi], self.xs[i] as i32);
            let (val, hroom, lroom) = (ys[i], range - pred, pred);
            let room = if hroom < lroom { hroom } else { lroom } * 2;
            if val != 0 {
                ok[lo] = true;
                ok[hi] = true;
                ok[i] = true;
                fy[i] = if val >= room {
                    if hroom > lroom { val - lroom + pred } else { pred - val + hroom - 1 }
                } else if val & 1 == 1 {
                    pred - (val + 1) / 2
                } else {
                    pred + val / 2
                };
            } else {
                fy[i] = pred;
            }
        }
        (fy, ok)
    }

    pub fn render(&self, fy: &[i32], ok: &[bool], n2: usize) -> Vec<f32> {
        let mut ord: Vec<usize> = (0..self.xs.len()).collect();
        ord.sort_by_key(|&i| self.xs[i]);
        let m = self.mult as i32;
        let mut v = vec![0i32; n2];
        let (mut lx, mut ly, mut hx, mut hy) = (0i32, fy[ord[0]] * m, 0i32, 0i32);
        for &i in &ord[1..] {
            if ok[i] {
                hy = fy[i] * m;
                hx = self.xs[i] as i32;
                line(lx, ly, hx, hy, &mut v);
                lx = hx;
                ly = hy;
            }
        }
        if (hx as usize) < n2 {
            line(hx, hy, n2 as i32, hy, &mut v);
        }
        v.iter().map(|&y| db(y)).collect()
    }

    fn read(&self, r: &mut Br, books: &[Book]) -> Option<Option<Vec<i32>>> {
        if r.get(1)? == 0 {
            return Some(None);
        }
        let bits = ilog(RANGE[self.mult as usize - 1] as u32 - 1);
        let mut ys = vec![r.get(bits)? as i32, r.get(bits)? as i32];
        for &c in &self.parts {
            let (cdim, cbits) = (self.cdim[c], self.csub[c]);
            let csub = (1 << cbits) - 1;
            let mut cval = if cbits > 0 { books[self.cmaster[c]].read(r)? } else { 0 };
            for _ in 0..cdim {
                let b = self.sbooks[c][(cval & csub) as usize];
                cval >>= cbits;
                ys.push(if b >= 0 { books[b as usize].read(r)? as i32 } else { 0 });
            }
        }
        Some(Some(ys))
    }
}

fn bark(x: f64) -> f64 {
    13.1 * (0.00074 * x).atan() + 2.24 * (0.0000000185 * x * x).atan() + 0.0001 * x
}

impl Floor0 {
    fn read(&self, r: &mut Br, books: &[Book], n2: usize) -> Option<Option<Vec<f32>>> {
        let amp = r.get(self.abits)?;
        if amp == 0 {
            return Some(None);
        }
        let bn = r.get(ilog(self.books.len() as u32))? as usize;
        let book = &books[*self.books.get(bn)?];
        let mut co: Vec<f32> = Vec::new();
        let mut last = 0f32;
        while co.len() < self.order {
            let v = book.vector(r)?;
            for &x in v {
                co.push(x + last);
            }
            last = *co.last()?;
        }
        co.truncate(self.order);
        let mut out = vec![0f32; n2];
        let bm = self.bark as f64;
        let nyq = bark(0.5 * self.rate as f64);
        let cosc: Vec<f64> = co.iter().map(|&c| (c as f64).cos()).collect();
        for (i, o) in out.iter_mut().enumerate() {
            let map = ((bark(self.rate as f64 * i as f64 / (2.0 * n2 as f64)) * bm / nyq).floor()).min(bm - 1.0);
            let w = std::f64::consts::PI * map / bm;
            let cw = w.cos();
            let (mut p, mut q) = (1.0f64, 1.0f64);
            for (j, &c) in cosc.iter().enumerate() {
                let t = 4.0 * (c - cw) * (c - cw);
                if j % 2 == 1 { p *= t } else { q *= t }
            }
            if self.order % 2 == 1 {
                p *= 1.0 - cw * cw;
                q *= 0.25;
            } else {
                p *= (1.0 - cw) / 2.0;
                q *= (1.0 + cw) / 2.0;
            }
            let maxa = ((1u64 << self.abits) - 1) as f64;
            *o = (0.11512925 * (amp as f64 * self.aoff as f64 / (maxa * (p + q).sqrt()) - self.aoff as f64)).exp() as f32;
        }
        Some(Some(out))
    }
}

fn residue(r: &mut Br, res: &Residue, books: &[Book], v: &mut [Vec<f32>], dnd: &[bool], n2: usize) -> Option<()> {
    let ch = v.len();
    if res.kind == 2 {
        if dnd.iter().all(|&d| d) {
            return Some(());
        }
        let mut one = vec![vec![0f32; n2 * ch]];
        let r1 = Residue { kind: 1, begin: res.begin, end: res.end, psize: res.psize, classes: res.classes, classbook: res.classbook, books: res.books.clone() };
        let out = residue(r, &r1, books, &mut one, &[false], n2 * ch);
        for (i, &x) in one[0].iter().enumerate() {
            v[i % ch][i / ch] = x;
        }
        return out;
    }
    let (begin, end) = (res.begin.min(n2), res.end.min(n2));
    if end <= begin {
        return Some(());
    }
    let parts = (end - begin) / res.psize;
    let cb = &books[res.classbook];
    let cw = cb.dim;
    let mut cls = vec![vec![0usize; parts + cw]; ch];
    for pass in 0..8 {
        let mut pc = 0;
        while pc < parts {
            if pass == 0 {
                for j in 0..ch {
                    if dnd[j] {
                        continue;
                    }
                    let mut t = cb.read(r)? as usize;
                    for i in (0..cw).rev() {
                        cls[j][pc + i] = t % res.classes;
                        t /= res.classes;
                    }
                }
            }
            for _ in 0..cw {
                if pc >= parts {
                    break;
                }
                for j in 0..ch {
                    if dnd[j] {
                        continue;
                    }
                    let b = res.books.get(cls[j][pc]).map_or(-1, |b| b[pass]);
                    if b < 0 {
                        continue;
                    }
                    let book = &books[b as usize];
                    let off = begin + pc * res.psize;
                    if res.kind == 0 {
                        let step = res.psize / book.dim;
                        for s in 0..step {
                            let vq = book.vector(r)?;
                            for (i, &x) in vq.iter().enumerate() {
                                v[j][off + s + i * step] += x;
                            }
                        }
                    } else {
                        let mut i = 0;
                        while i < res.psize {
                            for &x in book.vector(r)? {
                                if i < res.psize {
                                    v[j][off + i] += x;
                                }
                                i += 1;
                            }
                        }
                    }
                }
                pc += 1;
            }
        }
    }
    Some(())
}

pub struct Decoder {
    pub s: Setup,
    mdct: [Mdct; 2],
    prev: Option<(Vec<Vec<f32>>, usize)>,
}

impl Decoder {
    pub fn new(h: [&[u8]; 3]) -> Res<Decoder> {
        let (channels, rate, bs) = ident(h[0])?;
        let (vendor, comments) = comments(h[1])?;
        let (books, floors, residues, maps, modes) = setup(h[2], channels)?;
        let s = Setup { channels, rate, bs, vendor, comments, books, floors, residues, maps, modes };
        Ok(Decoder { mdct: [Mdct::new(bs[0]), Mdct::new(bs[1])], s, prev: None })
    }

    pub fn packet(&mut self, p: &[u8], out: &mut Vec<f32>) -> usize {
        let Some(frame) = self.frame(p) else { return 0 };
        let n = frame[0].len();
        let ch = self.s.channels;
        let mut produced = 0;
        if let Some((prev, pn)) = &self.prev {
            let total = pn / 4 + n / 4;
            let off = (n / 4) as isize - (pn / 4) as isize;
            for k in 0..total {
                let (p, c) = (pn / 2 + k, k as isize + off);
                for ch in 0..ch {
                    let a = prev[ch].get(p).copied().unwrap_or(0.0);
                    let b = if c >= 0 && (c as usize) < n { frame[ch][c as usize] } else { 0.0 };
                    out.push(a + b);
                }
            }
            produced = total;
        }
        self.prev = Some((frame, n));
        produced
    }

    fn frame(&mut self, p: &[u8]) -> Option<Vec<Vec<f32>>> {
        let mut r = Br::new(p);
        if r.get(1)? != 0 {
            return None;
        }
        let s = &self.s;
        let mode = &s.modes[r.get(ilog(s.modes.len() as u32 - 1))? as usize];
        let n = s.bs[mode.long as usize];
        let (pw, nw) = if mode.long { (r.flag()?, r.flag()?) } else { (false, false) };
        let map = &s.maps[mode.mapping];
        let n2 = n / 2;
        let ch = s.channels;
        let mut curves: Vec<Option<Vec<f32>>> = Vec::with_capacity(ch);
        let mut eof = false;
        for c in 0..ch {
            let (fi, _) = map.submaps[map.mux[c]];
            let res = match &s.floors[fi] {
                Floor::F1(f) => f.read(&mut r, &s.books).map(|o| o.map(|ys| {
                    let (fy, ok) = f.unpack(&ys);
                    f.render(&fy, &ok, n2)
                })),
                Floor::F0(f) => f.read(&mut r, &s.books, n2),
            };
            match res {
                Some(x) => curves.push(x),
                None => {
                    eof = true;
                    curves.push(None);
                }
            }
        }
        let mut nz: Vec<bool> = curves.iter().map(|c| c.is_some()).collect();
        for &(m, a) in &map.coupling {
            if nz[m] || nz[a] {
                nz[m] = true;
                nz[a] = true;
            }
        }
        let mut spec = vec![vec![0f32; n2]; ch];
        if !eof {
            for (si, &(_, ri)) in map.submaps.iter().enumerate() {
                let idx: Vec<usize> = (0..ch).filter(|&c| map.mux[c] == si).collect();
                let mut vs: Vec<Vec<f32>> = idx.iter().map(|_| vec![0f32; n2]).collect();
                let dnd: Vec<bool> = idx.iter().map(|&c| !nz[c]).collect();
                let _ = residue(&mut r, &s.residues[ri], &s.books, &mut vs, &dnd, n2);
                for (k, &c) in idx.iter().enumerate() {
                    spec[c] = std::mem::take(&mut vs[k]);
                }
            }
        }
        for &(mi, ai) in map.coupling.iter().rev() {
            for j in 0..n2 {
                let (m, a) = (spec[mi][j], spec[ai][j]);
                let (nm, na) = if m > 0.0 {
                    if a > 0.0 { (m, m - a) } else { (m + a, m) }
                } else if a > 0.0 {
                    (m, m + a)
                } else {
                    (m - a, m)
                };
                spec[mi][j] = nm;
                spec[ai][j] = na;
            }
        }
        let bs0 = s.bs[0];
        let (left, right) = if mode.long { (if pw { n / 2 } else { bs0 / 2 }, if nw { n / 2 } else { bs0 / 2 }) } else { (n / 2, n / 2) };
        let mut w = vec![0f32; n];
        window(n, left, right, &mut w);
        let mdct = &mut self.mdct[mode.long as usize];
        let mut outs = Vec::with_capacity(ch);
        for c in 0..ch {
            let mut y = vec![0f32; n];
            if let Some(curve) = &curves[c] {
                let x: Vec<f32> = spec[c].iter().zip(curve).map(|(a, b)| a * b).collect();
                mdct.inverse(&x, &mut y);
                for (v, k) in y.iter_mut().zip(&w) {
                    *v *= k;
                }
            }
            outs.push(y);
        }
        Some(outs)
    }
}

pub fn decode(packets: &[crate::ogg::Packet]) -> Res<(Setup, Vec<f32>)> {
    if packets.len() < 3 {
        return bad("vorbis: missing headers");
    }
    let mut d = Decoder::new([&packets[0].data, &packets[1].data, &packets[2].data])?;
    let ch = d.s.channels;
    let mut out = Vec::new();
    let mut total = 0i64;
    let mut end = None;
    for p in &packets[3..] {
        total += d.packet(&p.data, &mut out) as i64;
        if p.granule >= 0 {
            end = Some(p.granule);
        }
    }
    if let Some(g) = end {
        if g < total {
            out.truncate(g.max(0) as usize * ch);
        }
    }
    Ok((d.s, out))
}
