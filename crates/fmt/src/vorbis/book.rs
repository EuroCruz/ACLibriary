use super::bits::{float32, ilog, pack32, Br, Bw};
use ac_core::{bad, Res};

#[derive(Clone, Debug, Default)]
pub struct Book {
    pub dim: usize,
    pub lens: Vec<u8>,
    codes: Vec<u32>,
    fast: Vec<i32>,
    slow: Vec<(u32, u8, u32)>,
    single: Option<u32>,
    pub vq: Vec<f32>,
}

const FAST: u32 = 10;

pub fn lookup1(entries: u32, dim: u32) -> u32 {
    let mut r = (entries as f64).powf(1.0 / dim as f64).floor() as u32;
    while (r + 1).checked_pow(dim).is_some_and(|p| p <= entries) {
        r += 1;
    }
    while r > 0 && r.checked_pow(dim).is_none_or(|p| p > entries) {
        r -= 1;
    }
    r
}

fn assign(lens: &[u8]) -> Res<Vec<u32>> {
    let mut avail = [0u32; 33];
    let mut codes = vec![0u32; lens.len()];
    let used: Vec<usize> = (0..lens.len()).filter(|&i| lens[i] > 0).collect();
    if used.len() == 1 {
        return Ok(codes);
    }
    let mut first = true;
    for &i in &used {
        let l = lens[i] as usize;
        if first {
            for (j, a) in avail.iter_mut().enumerate().take(l + 1).skip(1) {
                *a = 1u32.wrapping_shl(32 - j as u32);
            }
            codes[i] = 0;
            first = false;
            continue;
        }
        let mut z = l;
        while z > 0 && avail[z] == 0 {
            z -= 1;
        }
        if z == 0 {
            return bad("vorbis: overspecified codebook");
        }
        let res = avail[z];
        avail[z] = 0;
        codes[i] = res.reverse_bits();
        for (j, a) in avail.iter_mut().enumerate().take(l + 1).skip(z + 1) {
            *a = res.wrapping_add(1u32.wrapping_shl(32 - j as u32));
        }
    }
    Ok(codes)
}

impl Book {
    pub fn new(dim: usize, lens: Vec<u8>, vq: Vec<f32>) -> Res<Book> {
        let codes = assign(&lens)?;
        let mut fast = vec![-1i32; 1 << FAST];
        let mut slow = Vec::new();
        for (i, (&l, &c)) in lens.iter().zip(&codes).enumerate() {
            if l == 0 {
                continue;
            }
            if l as u32 <= FAST {
                let step = 1usize << l;
                let mut k = c as usize;
                while k < fast.len() {
                    fast[k] = i as i32;
                    k += step;
                }
            } else {
                slow.push((c, l, i as u32));
            }
        }
        let single = if lens.iter().filter(|&&l| l > 0).count() == 1 { lens.iter().position(|&l| l > 0).map(|e| e as u32) } else { None };
        Ok(Book { dim, lens, codes, fast, slow, single, vq })
    }

    pub fn read(&self, r: &mut Br) -> Option<u32> {
        if let Some(e) = self.single {
            r.get(self.lens[e as usize] as u32)?;
            return Some(e);
        }
        let start = r.pos;
        let mut peek = 0u32;
        let mut n = 0;
        while n < FAST {
            match r.bit() {
                Some(b) => peek |= b << n,
                None => break,
            }
            n += 1;
        }
        r.pos = start;
        let e = self.fast[peek as usize];
        if e >= 0 && self.lens[e as usize] as u32 <= n {
            r.pos += self.lens[e as usize] as usize;
            return Some(e as u32);
        }
        let mut code = 0u32;
        for l in 1..=32u8 {
            code |= r.bit()? << (l - 1);
            if l as u32 > FAST {
                if let Some(x) = self.slow.iter().find(|x| x.1 == l && x.0 == code) {
                    return Some(x.2);
                }
            }
        }
        None
    }

    pub fn vector(&self, r: &mut Br) -> Option<&[f32]> {
        let e = self.read(r)? as usize;
        self.vq.get(e * self.dim..(e + 1) * self.dim)
    }

    pub fn write(&self, w: &mut Bw, e: usize) {
        w.put(self.codes[e], self.lens[e] as u32);
    }

    pub fn parse(r: &mut Br) -> Res<Book> {
        let g = |r: &mut Br, n| r.get(n).ok_or(ac_core::Error::Bad("vorbis: eof in codebook"));
        if g(r, 24)? != 0x564342 {
            return bad("vorbis: bad codebook sync");
        }
        let dim = g(r, 16)? as usize;
        let entries = g(r, 24)? as usize;
        if dim == 0 || entries == 0 {
            return bad("vorbis: empty codebook");
        }
        let mut lens = vec![0u8; entries];
        if g(r, 1)? == 0 {
            let sparse = g(r, 1)? == 1;
            for l in lens.iter_mut() {
                if !sparse || g(r, 1)? == 1 {
                    *l = g(r, 5)? as u8 + 1;
                }
            }
        } else {
            let mut cur = g(r, 5)? + 1;
            let mut i = 0;
            while i < entries {
                let n = g(r, ilog((entries - i) as u32))? as usize;
                if i + n > entries || cur > 32 {
                    return bad("vorbis: bad ordered codebook");
                }
                lens[i..i + n].fill(cur as u8);
                i += n;
                cur += 1;
            }
        }
        let kind = g(r, 4)?;
        let vq = match kind {
            0 => Vec::new(),
            1 | 2 => {
                let min = float32(g(r, 32)?);
                let delta = float32(g(r, 32)?);
                let bits = g(r, 4)? + 1;
                let seq = g(r, 1)? == 1;
                let n = if kind == 1 { lookup1(entries as u32, dim as u32) as usize } else { entries * dim };
                let mul: Vec<u32> = (0..n).map(|_| g(r, bits)).collect::<Res<_>>()?;
                let mut v = vec![0f32; entries * dim];
                for e in 0..entries {
                    let mut last = 0f32;
                    let mut div = 1usize;
                    for j in 0..dim {
                        let off = if kind == 1 { (e / div) % n } else { e * dim + j };
                        let x = mul[off] as f32 * delta + min + last;
                        v[e * dim + j] = x;
                        if seq {
                            last = x;
                        }
                        div *= n;
                    }
                }
                v
            }
            _ => return bad("vorbis: bad lookup type"),
        };
        Book::new(dim, lens, vq)
    }
}

pub fn write_header(w: &mut Bw, dim: usize, lens: &[u8], lattice: Option<(f32, f32, u32, u32)>) {
    w.put(0x564342, 24);
    w.put(dim as u32, 16);
    w.put(lens.len() as u32, 24);
    w.flag(false);
    let sparse = lens.contains(&0);
    w.flag(sparse);
    for &l in lens {
        if sparse {
            w.flag(l > 0);
        }
        if l > 0 {
            w.put(l as u32 - 1, 5);
        }
    }
    match lattice {
        None => w.put(0, 4),
        Some((min, delta, bits, n)) => {
            w.put(1, 4);
            w.put(pack32(min), 32);
            w.put(pack32(delta), 32);
            w.put(bits - 1, 4);
            w.flag(false);
            for i in 0..n {
                w.put(i, bits);
            }
        }
    }
}

pub fn huffman(counts: &[u64], max: u8) -> Vec<u8> {
    let n = counts.len();
    if n == 1 {
        return vec![1];
    }
    let mut f: Vec<u64> = counts.iter().map(|&c| c + 1).collect();
    loop {
        let mut nodes: Vec<(u64, Vec<usize>)> = f.iter().enumerate().map(|(i, &c)| (c, vec![i])).collect();
        let mut lens = vec![0u8; n];
        while nodes.len() > 1 {
            nodes.sort_by(|a, b| b.0.cmp(&a.0));
            let a = nodes.pop().unwrap();
            let b = nodes.pop().unwrap();
            for &i in a.1.iter().chain(&b.1) {
                lens[i] += 1;
            }
            let mut m = a.1;
            m.extend(b.1);
            nodes.push((a.0 + b.0, m));
        }
        if lens.iter().all(|&l| l <= max) {
            return lens;
        }
        f.iter_mut().for_each(|c| *c = *c / 2 + 1);
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn codes_and_vq() {
        let lens = vec![2u8, 4, 4, 4, 4, 2, 3, 3];
        let b = Book::new(1, lens.clone(), Vec::new()).unwrap();
        let mut w = Bw::new();
        let seq = [0usize, 7, 3, 5, 1, 6, 2, 4, 0, 0];
        for &e in &seq {
            b.write(&mut w, e);
        }
        let mut r = Br::new(&w.d);
        for &e in &seq {
            assert_eq!(b.read(&mut r), Some(e as u32));
        }
        assert!(Book::new(1, vec![1, 1, 1], Vec::new()).is_err());
        assert_eq!(lookup1(961, 2), 31);
        assert_eq!(lookup1(1000, 3), 10);
        assert_eq!(lookup1(999, 3), 9);
        let counts: Vec<u64> = (0..300).map(|i| if i < 3 { 1_000_000 } else { (i % 5) as u64 }).collect();
        let hl = huffman(&counts, 20);
        assert!(hl.iter().all(|&l| l > 0 && l <= 20));
        let kraft: f64 = hl.iter().map(|&l| 0.5f64.powi(l as i32)).sum();
        assert!((kraft - 1.0).abs() < 1e-9);
        let mut w = Bw::new();
        write_header(&mut w, 2, &[2, 2, 2, 2, 0, 0, 0, 0, 0], Some((-1.0, 1.0, 2, 3)));
        let p = Book::parse(&mut Br::new(&w.d)).unwrap();
        assert_eq!(p.vq[..8], [-1.0, -1.0, 0.0, -1.0, 1.0, -1.0, -1.0, 0.0]);
        let long: Vec<u8> = (1..=14).chain([14]).collect();
        let lb = Book::new(1, long, Vec::new()).unwrap();
        let mut w = Bw::new();
        for e in [14usize, 13, 0, 12, 11] {
            lb.write(&mut w, e);
        }
        let mut r = Br::new(&w.d);
        for e in [14u32, 13, 0, 12, 11] {
            assert_eq!(lb.read(&mut r), Some(e));
        }
    }
}
