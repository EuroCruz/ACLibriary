use super::{Image, Px};
use ac_core::{bad, Res};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bc {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc7,
}

pub fn block_bytes(b: Bc) -> usize {
    match b {
        Bc::Bc1 | Bc::Bc4 => 8,
        _ => 16,
    }
}

fn c565(c: u16) -> [i32; 3] {
    let (r, g, b) = ((c >> 11) as i32, (c >> 5 & 63) as i32, (c & 31) as i32);
    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]
}

fn to565(c: [f32; 3]) -> u16 {
    let q = |v: f32, m: f32| (v.clamp(0.0, 255.0) * m / 255.0).round() as u16;
    q(c[0], 31.0) << 11 | q(c[1], 63.0) << 5 | q(c[2], 31.0)
}

fn palette(c0: u16, c1: u16, four: bool) -> [[i32; 4]; 4] {
    let (a, b) = (c565(c0), c565(c1));
    let raw = |c: u16| [(c >> 11) as i32, (c >> 5 & 63) as i32, (c & 31) as i32];
    let (ra, rb) = (raw(c0), raw(c1));
    let mix = |p: i32, q: i32, d: i32| {
        let s = [0, 1, 2].map(|k| ra[k] * p + rb[k] * q);
        if d == 3 {
            [(s[0] * 351 + 61) >> 7, (s[1] * 2763 + 1039) >> 11, (s[2] * 351 + 61) >> 7]
        } else {
            [(s[0] * 1053 + 125) >> 8, (s[1] * 4145 + 1019) >> 11, (s[2] * 1053 + 125) >> 8]
        }
    };
    let rgb = |c: [i32; 3], al: i32| [c[0], c[1], c[2], al];
    if four || c0 > c1 {
        [rgb(a, 255), rgb(b, 255), rgb(mix(2, 1, 3), 255), rgb(mix(1, 2, 3), 255)]
    } else {
        [rgb(a, 255), rgb(b, 255), rgb(mix(1, 1, 2), 255), [0, 0, 0, 0]]
    }
}

fn color(b: &[u8], four: bool, o: &mut [Px; 16]) {
    let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
    let p = palette(c0, c1, four);
    let idx = u32::from_le_bytes(b[4..8].try_into().unwrap());
    for (i, px) in o.iter_mut().enumerate() {
        let c = p[(idx >> (2 * i) & 3) as usize];
        *px = [c[0] as u8, c[1] as u8, c[2] as u8, c[3] as u8];
    }
}

fn alphas(a0: u8, a1: u8) -> [u8; 8] {
    let (a, b) = (a0 as u32, a1 as u32);
    let mut v = [a0, a1, 0, 0, 0, 0, 0, 255];
    if a0 > a1 {
        for i in 1..7 {
            v[i + 1] = (((7 - i) as u32 * a + i as u32 * b) / 7) as u8;
        }
    } else {
        for i in 1..5 {
            v[i + 1] = (((5 - i) as u32 * a + i as u32 * b) / 5) as u8;
        }
        v[6] = 0;
    }
    v
}

fn alpha(b: &[u8]) -> [u8; 16] {
    let v = alphas(b[0], b[1]);
    let idx = b[2..8].iter().rev().fold(0u64, |a, &x| a << 8 | x as u64);
    std::array::from_fn(|i| v[(idx >> (3 * i) & 7) as usize])
}

pub fn bc1(b: &[u8]) -> [Px; 16] {
    let mut o = [[0; 4]; 16];
    color(b, false, &mut o);
    o
}

pub fn bc2(b: &[u8]) -> [Px; 16] {
    let mut o = [[0; 4]; 16];
    color(&b[8..], true, &mut o);
    let a = u64::from_le_bytes(b[..8].try_into().unwrap());
    for (i, p) in o.iter_mut().enumerate() {
        p[3] = (a >> (4 * i) & 15) as u8 * 17;
    }
    o
}

pub fn bc3(b: &[u8]) -> [Px; 16] {
    let mut o = [[0; 4]; 16];
    color(&b[8..], true, &mut o);
    let a = alpha(b);
    o.iter_mut().zip(a).for_each(|(p, a)| p[3] = a);
    o
}

pub fn bc4(b: &[u8]) -> [Px; 16] {
    alpha(b).map(|v| [v, v, v, 255])
}

pub fn bc5(b: &[u8]) -> [Px; 16] {
    let (r, g) = (alpha(b), alpha(&b[8..]));
    std::array::from_fn(|i| [r[i], g[i], 0, 255])
}

struct Bits(u128, u32);

impl Bits {
    fn get(&mut self, n: u32) -> u32 {
        let v = (self.0 >> self.1) as u32 & ((1u32 << n) - 1);
        self.1 += n;
        v
    }
}

const W2: [u32; 4] = [0, 21, 43, 64];
const W3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const W4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

fn weights(n: u32) -> &'static [u32] {
    match n {
        2 => &W2,
        3 => &W3,
        _ => &W4,
    }
}

fn lerp(a: u32, b: u32, w: u32) -> u8 {
    (((64 - w) * a + w * b + 32) >> 6) as u8
}

pub fn bc7(b: &[u8]) -> [Px; 16] {
    let mut r = Bits(u128::from_le_bytes(b[..16].try_into().unwrap()), 0);
    let Some(mode) = (0..8).find(|_| r.get(1) == 1) else { return [[0; 4]; 16] };
    let subs = [3, 2, 3, 2, 1, 1, 1, 2][mode];
    let part = match mode {
        0 => r.get(4),
        1 | 2 | 3 | 7 => r.get(6),
        _ => 0,
    } as usize;
    let rot = if mode == 4 || mode == 5 { r.get(2) } else { 0 };
    let isb = if mode == 4 { r.get(1) } else { 0 };
    let (cb, ab) = ([4, 6, 5, 7, 5, 7, 7, 5][mode], [0, 0, 0, 0, 6, 8, 7, 5][mode]);
    let n = subs * 2;
    let mut e = [[0u32; 4]; 6];
    for k in 0..3 {
        for x in e.iter_mut().take(n) {
            x[k] = r.get(cb);
        }
    }
    for x in e.iter_mut().take(n) {
        x[3] = if ab > 0 { r.get(ab) } else { 255 };
    }
    let pb = matches!(mode, 0 | 1 | 3 | 6 | 7) as u32;
    if pb == 1 {
        let bits: Vec<u32> = if mode == 1 {
            let (p, q) = (r.get(1), r.get(1));
            vec![p, p, q, q]
        } else {
            (0..n).map(|_| r.get(1)).collect()
        };
        for (x, p) in e.iter_mut().zip(bits) {
            for k in 0..4 {
                x[k] = x[k] << 1 | p;
            }
        }
    }
    for x in e.iter_mut().take(n) {
        let c = cb + pb;
        for v in x.iter_mut().take(3) {
            *v = (*v << (8 - c)) | (*v << (8 - c)) >> c;
        }
        if ab > 0 {
            let a = ab + pb;
            x[3] = (x[3] << (8 - a)) | (x[3] << (8 - a)) >> a;
        } else {
            x[3] = 255;
        }
    }
    let sub = |i: usize| -> usize {
        match subs {
            2 => (P2[part] >> i & 1) as usize,
            3 => (P3[part] >> (2 * i) & 3) as usize,
            _ => 0,
        }
    };
    let anchor = |i: usize| -> bool {
        i == 0
            || match subs {
                2 => i == A2[part] as usize,
                3 => i == A3[part].0 as usize || i == A3[part].1 as usize,
                _ => false,
            }
    };
    let ib = [3, 3, 2, 2, 2, 2, 4, 2][mode];
    let ib2 = [0, 0, 0, 0, 3, 2, 0, 0][mode];
    let i1: [u32; 16] = std::array::from_fn(|i| r.get(ib - anchor(i) as u32));
    let i2: [u32; 16] = std::array::from_fn(|i| if ib2 > 0 { r.get(ib2 - (i == 0) as u32) } else { 0 });
    std::array::from_fn(|i| {
        let s = sub(i);
        let (a, c) = (e[2 * s], e[2 * s + 1]);
        let mut p = [0u8; 4];
        if ib2 == 0 {
            let w = weights(ib);
            for k in 0..4 {
                p[k] = lerp(a[k], c[k], w[i1[i] as usize]);
            }
        } else {
            let (ci, cw, ai, aw) = if isb == 1 { (i2[i], ib2, i1[i], ib) } else { (i1[i], ib, i2[i], ib2) };
            for k in 0..3 {
                p[k] = lerp(a[k], c[k], weights(cw)[ci as usize]);
            }
            p[3] = lerp(a[3], c[3], weights(aw)[ai as usize]);
        }
        match rot {
            1 => p.swap(0, 3),
            2 => p.swap(1, 3),
            3 => p.swap(2, 3),
            _ => {}
        }
        p
    })
}

pub fn decode(b: Bc, d: &[u8], w: u32, h: u32) -> Res<Image> {
    let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
    let n = block_bytes(b);
    if d.len() < bw * bh * n {
        return bad("bc data too short");
    }
    let f: fn(&[u8]) -> [Px; 16] = match b {
        Bc::Bc1 => bc1,
        Bc::Bc2 => bc2,
        Bc::Bc3 => bc3,
        Bc::Bc4 => bc4,
        Bc::Bc5 => bc5,
        Bc::Bc7 => bc7,
    };
    let mut o = Image::new(w, h);
    for by in 0..bh {
        for bx in 0..bw {
            let px = f(&d[(by * bw + bx) * n..]);
            for (i, p) in px.iter().enumerate() {
                o.set((bx * 4 + i % 4) as u32, (by * 4 + i / 4) as u32, *p);
            }
        }
    }
    Ok(o)
}

fn err(a: [i32; 4], p: Px) -> i32 {
    (0..3).map(|k| (a[k] - p[k] as i32).pow(2)).sum()
}

fn fit(px: &[Px; 16], use_: &[bool; 16], c0: u16, c1: u16, four: bool) -> (u32, i32) {
    let pal = palette(c0, c1, four);
    let k = if four || c0 > c1 { 4 } else { 3 };
    let (mut idx, mut tot) = (0u32, 0);
    for i in 0..16 {
        if !use_[i] {
            idx |= 3 << (2 * i);
            continue;
        }
        let (j, e) = (0..k).map(|j| (j, err(pal[j], px[i]))).min_by_key(|x| x.1).unwrap();
        idx |= (j as u32) << (2 * i);
        tot += e;
    }
    (idx, tot)
}

fn axis(pts: &[[f32; 3]], m: [f32; 3]) -> [f32; 3] {
    let mut c = [[0f32; 3]; 3];
    for p in pts {
        let d = [p[0] - m[0], p[1] - m[1], p[2] - m[2]];
        for i in 0..3 {
            for j in 0..3 {
                c[i][j] += d[i] * d[j];
            }
        }
    }
    let mut v = [1.0f32, 1.0, 1.0];
    for _ in 0..8 {
        let n = [0, 1, 2].map(|i| c[i][0] * v[0] + c[i][1] * v[1] + c[i][2] * v[2]);
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l < 1e-6 {
            return [0.0; 3];
        }
        v = n.map(|x| x / l);
    }
    v
}

fn refine(pts: &[[f32; 3]], t: &[f32]) -> Option<([f32; 3], [f32; 3])> {
    let (mut aa, mut bb, mut ab) = (0f32, 0f32, 0f32);
    let (mut ax, mut bx) = ([0f32; 3], [0f32; 3]);
    for (p, &w) in pts.iter().zip(t) {
        let (a, b) = (1.0 - w, w);
        aa += a * a;
        bb += b * b;
        ab += a * b;
        for k in 0..3 {
            ax[k] += a * p[k];
            bx[k] += b * p[k];
        }
    }
    let det = aa * bb - ab * ab;
    if det.abs() < 1e-6 {
        return None;
    }
    let s = [0, 1, 2].map(|k| (ax[k] * bb - bx[k] * ab) / det);
    let e = [0, 1, 2].map(|k| (bx[k] * aa - ax[k] * ab) / det);
    Some((s, e))
}

fn color_block(px: &[Px; 16], punch: bool) -> [u8; 8] {
    let trans = punch && px.iter().any(|p| p[3] < 128);
    let use_: [bool; 16] = std::array::from_fn(|i| !trans || px[i][3] >= 128);
    let pts: Vec<[f32; 3]> = (0..16).filter(|&i| use_[i]).map(|i| [0, 1, 2].map(|k| px[i][k] as f32)).collect();
    let pack = |c0: u16, c1: u16, idx: u32| {
        let mut o = [0u8; 8];
        o[..2].copy_from_slice(&c0.to_le_bytes());
        o[2..4].copy_from_slice(&c1.to_le_bytes());
        o[4..].copy_from_slice(&idx.to_le_bytes());
        o
    };
    if pts.is_empty() {
        return pack(0, 0, u32::MAX);
    }
    let m = [0, 1, 2].map(|k| pts.iter().map(|p| p[k]).sum::<f32>() / pts.len() as f32);
    let ax = axis(&pts, m);
    let proj: Vec<f32> = pts.iter().map(|p| (0..3).map(|k| (p[k] - m[k]) * ax[k]).sum()).collect();
    let (lo, hi) = proj.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let mut cands = vec![(to565([0, 1, 2].map(|k| m[k] + ax[k] * hi)), to565([0, 1, 2].map(|k| m[k] + ax[k] * lo)))];
    let bb = |f: fn(f32, f32) -> f32, init: f32| to565([0, 1, 2].map(|k| pts.iter().map(|p| p[k]).fold(init, f)));
    cands.push((bb(f32::max, 0.0), bb(f32::min, 255.0)));
    let order = |c0: u16, c1: u16| if trans == (c0 > c1) { (c1, c0) } else { (c0, c1) };
    let mut best = (u16::MAX, 0, 0, i32::MAX);
    let mut k = 0;
    while k < cands.len() {
        let (c0, c1) = order(cands[k].0, cands[k].1);
        let (idx, e) = fit(px, &use_, c0, c1, false);
        if e < best.3 {
            best = (c0, c1, idx, e);
        }
        if k < 6 && !trans {
            let ws = [0.0, 1.0, 1.0 / 3.0, 2.0 / 3.0];
            let t: Vec<f32> = (0..16).filter(|&i| use_[i]).map(|i| ws[(idx >> (2 * i) & 3) as usize]).collect();
            if let Some((s, e2)) = refine(&pts, &t) {
                cands.push((to565(s), to565(e2)));
            }
        }
        k += 1;
    }
    let (c0, c1, mut idx) = (best.0, best.1, best.2);
    if !trans && c0 == c1 {
        idx = 0;
    }
    pack(c0, c1, idx)
}

fn alpha_block(v: &[u8; 16]) -> [u8; 8] {
    let (lo, hi) = (*v.iter().min().unwrap(), *v.iter().max().unwrap());
    let inner: Vec<u8> = v.iter().copied().filter(|&x| x != 0 && x != 255).collect();
    let mut tries = vec![(hi, lo)];
    if let (Some(&a), Some(&b)) = (inner.iter().min(), inner.iter().max()) {
        tries.push((a, b));
    }
    let mut best = ([0u8; 8], i32::MAX);
    for (a0, a1) in tries {
        let pal = alphas(a0, a1);
        let (mut idx, mut tot) = (0u64, 0);
        for (i, &x) in v.iter().enumerate() {
            let (j, e) = (0..8).map(|j| (j, (pal[j] as i32 - x as i32).abs())).min_by_key(|t| t.1).unwrap();
            idx |= (j as u64) << (3 * i);
            tot += e * e;
        }
        if tot < best.1 {
            let mut o = [a0, a1, 0, 0, 0, 0, 0, 0];
            o[2..].copy_from_slice(&idx.to_le_bytes()[..6]);
            best = (o, tot);
        }
    }
    best.0
}

pub fn encode(b: Bc, img: &Image) -> Res<Vec<u8>> {
    let (bw, bh) = (img.w.div_ceil(4), img.h.div_ceil(4));
    let mut o = Vec::with_capacity((bw * bh) as usize * block_bytes(b));
    for by in 0..bh {
        for bx in 0..bw {
            let px: [Px; 16] = std::array::from_fn(|i| img.get(bx * 4 + i as u32 % 4, by * 4 + i as u32 / 4));
            let ch = |k: usize| -> [u8; 16] { std::array::from_fn(|i| px[i][k]) };
            match b {
                Bc::Bc1 => o.extend(color_block(&px, true)),
                Bc::Bc2 => {
                    let a = (0..16).fold(0u64, |a, i| a | ((px[i][3] as u64 * 15 + 127) / 255) << (4 * i));
                    o.extend(a.to_le_bytes());
                    o.extend(color_block(&px, false));
                }
                Bc::Bc3 => {
                    o.extend(alpha_block(&ch(3)));
                    o.extend(color_block(&px, false));
                }
                Bc::Bc4 => o.extend(alpha_block(&ch(0))),
                Bc::Bc5 => {
                    o.extend(alpha_block(&ch(0)));
                    o.extend(alpha_block(&ch(1)));
                }
                Bc::Bc7 => return bad("bc7 encoding unsupported"),
            }
        }
    }
    Ok(o)
}

#[cfg(test)]
mod t {
    use super::*;

    fn grad(w: u32, h: u32) -> Image {
        let mut i = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                i.set(x, y, [(x * 255 / w) as u8, (y * 255 / h) as u8, ((x + y) * 127 / (w + h)) as u8, 255]);
            }
        }
        i
    }

    fn mse(a: &Image, b: &Image, ch: usize) -> f64 {
        a.px.iter().zip(&b.px).map(|(p, q)| (0..ch).map(|k| (p[k] as f64 - q[k] as f64).powi(2)).sum::<f64>()).sum::<f64>() / (a.px.len() * ch) as f64
    }

    #[test]
    fn vectors() {
        let b = [0x00, 0xf8, 0x1f, 0x00, 0b11_10_01_00, 0, 0, 0];
        let p = bc1(&b);
        assert_eq!(p[0], [255, 0, 0, 255]);
        assert_eq!(p[1], [0, 0, 255, 255]);
        assert_eq!(p[2], [170, 0, 85, 255]);
        let t = bc1(&[0x1f, 0x00, 0x00, 0xf8, 0b11, 0, 0, 0]);
        assert_eq!(t[0], [0, 0, 0, 0]);
        assert_eq!(alphas(255, 0)[2], 218);
        assert_eq!(alphas(0, 255)[7], 255);
        let mut b7 = [0u8; 16];
        b7[0] = 1 << 6;
        b7[1] = 0xfe;
        b7[2] = 0xff;
        b7[3] = 0xff;
        b7[4] = 0xff;
        b7[5] = 0xff;
        b7[6] = 0xff;
        b7[7] = 0xff;
        b7[8] = 0xff;
        let p = bc7(&b7);
        assert!(p.iter().all(|x| x[3] > 200));
        assert_eq!(bc7(&[0u8; 16])[5], [0, 0, 0, 0]);
    }

    #[test]
    fn roundtrips() {
        let g = grad(64, 32);
        for (b, lim) in [(Bc::Bc1, 36.0), (Bc::Bc2, 36.0), (Bc::Bc3, 36.0)] {
            let d = decode(b, &encode(b, &g).unwrap(), 64, 32).unwrap();
            assert!(mse(&g, &d, 3) < lim, "{b:?} {}", mse(&g, &d, 3));
        }
        let mut s = grad(32, 32);
        s.px.iter_mut().for_each(|p| p[1] = 90);
        let d = decode(Bc::Bc1, &encode(Bc::Bc1, &s).unwrap(), 32, 32).unwrap();
        assert!(mse(&s, &d, 3) < 12.0, "{}", mse(&s, &d, 3));
        let mut a = grad(16, 16);
        a.px.iter_mut().enumerate().for_each(|(i, p)| p[3] = (i * 255 / 256) as u8);
        let d = decode(Bc::Bc3, &encode(Bc::Bc3, &a).unwrap(), 16, 16).unwrap();
        assert!(a.px.iter().zip(&d.px).all(|(p, q)| (p[3] as i32 - q[3] as i32).abs() <= 3));
        let d = decode(Bc::Bc5, &encode(Bc::Bc5, &a).unwrap(), 16, 16).unwrap();
        assert!(mse(&a, &d, 2) < 4.0);
        let mut cut = grad(8, 8);
        cut.px[3][3] = 0;
        let d = decode(Bc::Bc1, &encode(Bc::Bc1, &cut).unwrap(), 8, 8).unwrap();
        assert_eq!(d.px[3][3], 0);
        assert!(d.px.iter().filter(|p| p[3] == 0).count() == 1);
        let solid = Image { w: 5, h: 3, px: vec![[10, 200, 30, 255]; 15] };
        let d = decode(Bc::Bc1, &encode(Bc::Bc1, &solid).unwrap(), 5, 3).unwrap();
        assert!(mse(&solid, &d, 3) < 20.0);
        assert!(encode(Bc::Bc7, &solid).is_err());
        assert!(decode(Bc::Bc3, &[0; 10], 4, 4).is_err());
    }
}


const P2: [u16; 64] = [0xcccc, 0x8888, 0xeeee, 0xecc8, 0xc880, 0xfeec, 0xfec8, 0xec80, 0xc800, 0xffec, 0xfe80, 0xe800, 0xffe8, 0xff00, 0xfff0, 0xf000, 0xf710, 0x008e, 0x7100, 0x08ce, 0x008c, 0x7310, 0x3100, 0x8cce, 0x088c, 0x3110, 0x6666, 0x366c, 0x17e8, 0x0ff0, 0x718e, 0x399c, 0xaaaa, 0xf0f0, 0x5a5a, 0x33cc, 0x3c3c, 0x55aa, 0x9696, 0xa55a, 0x73ce, 0x13c8, 0x324c, 0x3bdc, 0x6996, 0xc33c, 0x9966, 0x0660, 0x0272, 0x04e4, 0x4e40, 0x2720, 0xc936, 0x936c, 0x39c6, 0x639c, 0x9336, 0x9cc6, 0x817e, 0xe718, 0xccf0, 0x0fcc, 0x7744, 0xee22, ];
const A2: [u8; 64] = [15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 2, 8, 2, 2, 8, 8, 15, 2, 8, 2, 2, 8, 8, 2, 2, 15, 15, 6, 8, 2, 8, 15, 15, 2, 8, 2, 2, 2, 15, 15, 6, 6, 2, 6, 8, 15, 15, 2, 2, 15, 15, 15, 15, 15, 2, 2, 15, ];
const P3: [u32; 64] = [0xaa685050, 0x6a5a5040, 0x5a5a4200, 0x5450a0a8, 0xa5a50000, 0xa0a05050, 0x5555a0a0, 0x5a5a5050, 0xaa550000, 0xaa555500, 0xaaaa5500, 0x90909090, 0x94949494, 0xa4a4a4a4, 0xa9a59450, 0x2a0a4250, 0xa5945040, 0x0a425054, 0xa5a5a500, 0x55a0a0a0, 0xa8a85454, 0x6a6a4040, 0xa4a45000, 0x1a1a0500, 0x0050a4a4, 0xaaa59090, 0x14696914, 0x69691400, 0xa08585a0, 0xaa821414, 0x50a4a450, 0x6a5a0200, 0xa9a58000, 0x5090a0a8, 0xa8a09050, 0x24242424, 0x00aa5500, 0x24924924, 0x24499224, 0x50a50a50, 0x500aa550, 0xaaaa4444, 0x66660000, 0xa5a0a5a0, 0x50a050a0, 0x69286928, 0x44aaaa44, 0x66666600, 0xaa444444, 0x54a854a8, 0x95809580, 0x96969600, 0xa85454a8, 0x80959580, 0xaa141414, 0x96960000, 0xaaaa1414, 0xa05050a0, 0xa0a5a5a0, 0x96000000, 0x40804080, 0xa9a8a9a8, 0xaaaaaa44, 0x2a4a5254, ];
const A3: [(u8, u8); 64] = [(3, 15), (3, 8), (15, 8), (15, 3), (8, 15), (3, 15), (15, 3), (15, 8), (8, 15), (8, 15), (6, 15), (6, 15), (6, 15), (5, 15), (3, 15), (3, 8), (3, 15), (3, 8), (8, 15), (15, 3), (3, 15), (3, 8), (6, 15), (10, 8), (5, 3), (8, 15), (8, 6), (6, 10), (8, 15), (5, 15), (15, 10), (15, 8), (8, 15), (15, 3), (3, 15), (5, 10), (6, 10), (10, 8), (8, 9), (15, 10), (15, 6), (3, 15), (15, 8), (5, 15), (15, 3), (15, 6), (15, 6), (15, 8), (3, 15), (15, 3), (5, 15), (5, 15), (5, 15), (8, 15), (5, 15), (10, 15), (5, 15), (10, 15), (8, 15), (13, 15), (15, 3), (12, 15), (3, 15), (3, 8), ];

#[cfg(test)]
mod real {
    use super::*;

    #[test]
    #[ignore]
    fn reference_bcdec() {
        let Ok(g) = std::env::var("AC_GOLDEN") else { return };
        let dir = std::path::Path::new(&g).join("bc");
        let blocks = std::fs::read(dir.join("blocks.bin")).unwrap();
        let fs: [(&str, fn(&[u8]) -> [Px; 16], usize); 6] = [("bc1", bc1, 8), ("bc2", bc2, 16), ("bc3", bc3, 16), ("bc4", bc4, 8), ("bc5", bc5, 16), ("bc7", bc7, 16)];
        for (n, f, _) in fs {
            let r = std::fs::read(dir.join(format!("{n}.out"))).unwrap();
            let (mut worst, mut diff) = (0i32, 0usize);
            for (i, b) in blocks.chunks_exact(16).enumerate() {
                let px = f(b);
                for (j, p) in px.iter().enumerate() {
                    for k in 0..4 {
                        let d = (p[k] as i32 - r[i * 64 + j * 4 + k] as i32).abs();
                        worst = worst.max(d);
                        diff += (d > 0) as usize;
                    }
                }
            }
            println!("{n}: worst {worst} differing {diff}");
            assert!(worst <= 1, "{n}");
        }
    }
}
