use crate::huff::{codes, lengths};
use crate::flate::tables::*;

struct Out {
    v: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Out {
    fn put(&mut self, v: u32, k: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += k;
        while self.n >= 8 {
            self.v.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    fn align(&mut self) {
        if self.n > 0 {
            self.v.push(self.acc as u8);
            self.acc = 0;
            self.n = 0;
        }
    }
}

const WIN: usize = 32768;
const MASK: usize = WIN - 1;
const MATCH: u32 = 0x8000_0000;
const BLOCK: usize = 1 << 15;

fn hash(d: &[u8], i: usize) -> usize {
    let v = d[i] as u32 | (d[i + 1] as u32) << 8 | (d[i + 2] as u32) << 16;
    (v.wrapping_mul(0x9E37_79B1) >> 17) as usize
}

fn tokens(d: &[u8], level: u32) -> Vec<u32> {
    let depth = [0, 4, 8, 16, 32, 64, 128, 256, 512, 1024][level.min(9) as usize];
    let lazy = level >= 4;
    let mut head = vec![-1i32; 1 << 15];
    let mut prev = vec![-1i32; WIN];
    let n = d.len();
    let mut out = Vec::with_capacity(n / 2 + 16);
    let insert = |head: &mut Vec<i32>, prev: &mut Vec<i32>, i: usize| {
        if i + 2 < n {
            let h = hash(d, i);
            prev[i & MASK] = head[h];
            head[h] = i as i32;
        }
    };
    let find = |head: &Vec<i32>, prev: &Vec<i32>, i: usize| -> (usize, usize) {
        let max = (n - i).min(258);
        if max < 3 {
            return (0, 0);
        }
        let mut best = (0, 0);
        let mut p = head[hash(d, i)];
        let mut chain = depth;
        while p >= 0 && chain > 0 {
            let pu = p as usize;
            if i - pu > WIN {
                break;
            }
            if d[pu + best.0] == d[i + best.0.min(max - 1)] || best.0 == 0 {
                let l = (0..max).take_while(|&k| d[pu + k] == d[i + k]).count();
                if l > best.0 {
                    best = (l, i - pu);
                    if l >= max {
                        break;
                    }
                }
            }
            p = prev[pu & MASK];
            chain -= 1;
        }
        if best.0 >= 3 { best } else { (0, 0) }
    };
    let mut i = 0;
    while i < n {
        let m = if depth > 0 { find(&head, &prev, i) } else { (0, 0) };
        if m.0 == 0 {
            out.push(d[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
            continue;
        }
        insert(&mut head, &mut prev, i);
        if lazy && m.0 < 32 && i + 1 < n {
            let nx = find(&head, &prev, i + 1);
            if nx.0 > m.0 {
                out.push(d[i] as u32);
                i += 1;
                continue;
            }
        }
        out.push(MATCH | ((m.0 - 3) as u32) << 16 | (m.1 - 1) as u32);
        for k in 1..m.0 {
            insert(&mut head, &mut prev, i + k);
        }
        i += m.0;
    }
    out
}

fn sym_len(l: usize) -> usize {
    LEN_BASE.partition_point(|&b| b as usize <= l) - 1
}

fn sym_dist(d: usize) -> usize {
    DIST_BASE.partition_point(|&b| b as usize <= d) - 1
}

fn span(t: &[u32]) -> usize {
    t.iter().map(|&x| if x & MATCH != 0 { ((x >> 16) & 0xff) as usize + 3 } else { 1 }).sum()
}

fn freqs(t: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let (mut l, mut d) = (vec![0u32; 286], vec![0u32; 30]);
    for &x in t {
        if x & MATCH != 0 {
            l[257 + sym_len(((x >> 16) & 0xff) as usize + 3)] += 1;
            d[sym_dist((x & 0xffff) as usize + 1)] += 1;
        } else {
            l[x as usize] += 1;
        }
    }
    l[256] = 1;
    (l, d)
}

fn data_bits(f: &(Vec<u32>, Vec<u32>), ll: &[u8], dl: &[u8]) -> usize {
    let mut b = 0usize;
    for (s, &c) in f.0.iter().enumerate() {
        b += c as usize * ll.get(s).copied().unwrap_or(0) as usize;
        if s >= 257 {
            b += c as usize * LEN_EXTRA[s - 257] as usize;
        }
    }
    for (s, &c) in f.1.iter().enumerate() {
        b += c as usize * (dl.get(s).copied().unwrap_or(0) as usize + DIST_EXTRA[s] as usize);
    }
    b
}

fn rle(seq: &[u8]) -> Vec<(u8, u8)> {
    let mut o = Vec::new();
    let mut i = 0;
    while i < seq.len() {
        let v = seq[i];
        let run = seq[i..].iter().take_while(|&&x| x == v).count();
        if v == 0 && run >= 3 {
            let r = run.min(138);
            o.push(if r >= 11 { (18, (r - 11) as u8) } else { (17, (r - 3) as u8) });
            i += r;
        } else if run >= 4 {
            o.push((v, 0));
            let r = (run - 1).min(6);
            o.push((16, (r - 3) as u8));
            i += 1 + r;
        } else {
            o.push((v, 0));
            i += 1;
        }
    }
    o
}

struct Dyn {
    ll: Vec<u8>,
    dl: Vec<u8>,
    nl: usize,
    nd: usize,
    items: Vec<(u8, u8)>,
    cl: Vec<u8>,
    nc: usize,
    bits: usize,
}

fn plan(f: &(Vec<u32>, Vec<u32>)) -> Dyn {
    let mut lf = f.0.clone();
    if lf.iter().filter(|&&x| x > 0).count() < 2 {
        lf[0] = 1;
    }
    let mut df = f.1.clone();
    if df.iter().all(|&x| x == 0) {
        df[0] = 1;
    }
    let (ll, dl) = (lengths(&lf, 15), lengths(&df, 15));
    let nl = (ll.iter().rposition(|&x| x > 0).unwrap() + 1).max(257);
    let nd = dl.iter().rposition(|&x| x > 0).unwrap() + 1;
    let seq: Vec<u8> = ll[..nl].iter().chain(&dl[..nd]).copied().collect();
    let items = rle(&seq);
    let mut cf = vec![0u32; 19];
    items.iter().for_each(|&(s, _)| cf[s as usize] += 1);
    let cl = lengths(&cf, 7);
    let nc = CL_ORDER.iter().rposition(|&i| cl[i] > 0).map_or(4, |p| p + 1).max(4);
    let hb: usize = 14 + nc * 3
        + items.iter().map(|&(s, _)| cl[s as usize] as usize + match s { 16 => 2, 17 => 3, 18 => 7, _ => 0 }).sum::<usize>();
    let bits = hb + data_bits(f, &ll, &dl);
    Dyn { ll, dl, nl, nd, items, cl, nc, bits }
}

fn emit(o: &mut Out, t: &[u32], ll: &[u8], dl: &[u8]) {
    let (lc, dc) = (codes(ll), codes(dl));
    for &x in t {
        if x & MATCH != 0 {
            let l = ((x >> 16) & 0xff) as usize + 3;
            let d = (x & 0xffff) as usize + 1;
            let s = sym_len(l);
            o.put(lc[257 + s] as u32, ll[257 + s] as u32);
            o.put((l - LEN_BASE[s] as usize) as u32, LEN_EXTRA[s] as u32);
            let s = sym_dist(d);
            o.put(dc[s] as u32, dl[s] as u32);
            o.put((d - DIST_BASE[s] as usize) as u32, DIST_EXTRA[s] as u32);
        } else {
            o.put(lc[x as usize] as u32, ll[x as usize] as u32);
        }
    }
    o.put(lc[256] as u32, ll[256] as u32);
}

fn block(o: &mut Out, t: &[u32], raw: &[u8], last: bool) {
    let f = freqs(t);
    let dy = plan(&f);
    let (fl, fd) = (fixed_lit(), fixed_dist());
    let fixed = 3 + data_bits(&f, &fl, &fd);
    let stored = 8 * (raw.len() + 5 * raw.len().div_ceil(65535).max(1)) + 7;
    if stored < dy.bits.min(fixed) {
        let chunks: Vec<&[u8]> = if raw.is_empty() { vec![raw] } else { raw.chunks(65535).collect() };
        for (i, c) in chunks.iter().enumerate() {
            o.put((last && i + 1 == chunks.len()) as u32, 1);
            o.put(0, 2);
            o.align();
            o.v.extend_from_slice(&(c.len() as u16).to_le_bytes());
            o.v.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
            o.v.extend_from_slice(c);
        }
    } else if fixed <= dy.bits + 3 {
        o.put(last as u32, 1);
        o.put(1, 2);
        emit(o, t, &fl, &fd);
    } else {
        o.put(last as u32, 1);
        o.put(2, 2);
        o.put((dy.nl - 257) as u32, 5);
        o.put((dy.nd - 1) as u32, 5);
        o.put((dy.nc - 4) as u32, 4);
        for &i in CL_ORDER.iter().take(dy.nc) {
            o.put(dy.cl[i] as u32, 3);
        }
        let cc = codes(&dy.cl);
        for &(s, x) in &dy.items {
            o.put(cc[s as usize] as u32, dy.cl[s as usize] as u32);
            match s {
                16 => o.put(x as u32, 2),
                17 => o.put(x as u32, 3),
                18 => o.put(x as u32, 7),
                _ => {}
            }
        }
        emit(o, t, &dy.ll, &dy.dl);
    }
}

pub fn deflate(d: &[u8], level: u32) -> Vec<u8> {
    let t = tokens(d, level);
    let mut o = Out { v: Vec::with_capacity(d.len() / 2 + 16), acc: 0, n: 0 };
    let mut pos = 0;
    let mut chunks: Vec<&[u32]> = t.chunks(BLOCK).collect();
    if chunks.is_empty() {
        chunks.push(&[]);
    }
    let count = chunks.len();
    for (i, c) in chunks.into_iter().enumerate() {
        let n = span(c);
        block(&mut o, c, &d[pos..pos + n], i + 1 == count);
        pos += n;
    }
    o.align();
    o.v
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::flate::inflate;

    fn sample(n: usize) -> Vec<u8> {
        let mut s = 12345u32;
        (0..n)
            .map(|i| {
                s = s.wrapping_mul(1103515245).wrapping_add(12345);
                if i % 7 < 4 { b"the quick brown fox "[i % 20] } else { (s >> 16) as u8 % 8 }
            })
            .collect()
    }

    #[test]
    fn roundtrip_levels() {
        for n in [0usize, 1, 2, 3, 100, 5000, 200_000] {
            let d = sample(n);
            for lv in [0, 1, 4, 6, 9] {
                let c = deflate(&d, lv);
                assert_eq!(inflate(&c, n).unwrap(), d, "n={n} lv={lv}");
            }
        }
    }

    #[test]
    fn compresses() {
        let d = vec![b'a'; 100_000];
        assert!(deflate(&d, 6).len() < 400);
        let t = sample(50_000);
        assert!(deflate(&t, 6).len() < t.len());
        let rnd: Vec<u8> = (0..4000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        assert!(deflate(&rnd, 6).len() <= rnd.len() + 16);
    }

    fn p(n: &str) -> String {
        format!("{}/../../target/{n}", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    #[ignore]
    fn read_external() {
        let raw = std::fs::read(p("flate_raw.bin")).unwrap();
        for n in ["py_z.bin", "py_z1.bin"] {
            assert_eq!(crate::flate::unzlib(&std::fs::read(p(n)).unwrap()).unwrap(), raw, "{n}");
        }
        assert_eq!(crate::flate::gunzip(&std::fs::read(p("py_gz.bin")).unwrap()).unwrap(), raw);
    }

    #[test]
    #[ignore]
    fn dump_for_external_check() {
        let d = sample(300_000);
        std::fs::write(p("flate_raw.bin"), &d).unwrap();
        std::fs::write(p("flate_z.bin"), crate::flate::zlib(&d, 6)).unwrap();
        std::fs::write(p("flate_gz.bin"), crate::flate::gzip(&d, 9)).unwrap();
    }
}
