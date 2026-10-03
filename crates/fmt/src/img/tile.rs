fn offset360(x: u32, y: u32, w: u32, lb: u32) -> u32 {
    let aw = (w + 31) & !31;
    let mac = ((x >> 5) + (y >> 5) * (aw >> 5)) << (lb + 7);
    let mic = ((x & 7) + ((y & 6) << 2)) << lb;
    let o = mac + ((mic & !15) << 1) + (mic & 15) + ((y & 8) << (3 + lb)) + ((y & 1) << 4);
    (((o & !511) << 3) + ((o & 448) << 2) + (o & 63) + ((y & 16) << 7) + (((((y & 8) >> 2) + (x >> 3)) & 3) << 6)) >> lb
}

fn log_bpp(n: u32) -> u32 {
    (n >> 2) + ((n >> 1) >> (n >> 2))
}

fn walk360(w: u32, h: u32, n: u32, mut f: impl FnMut(usize, usize)) {
    let lb = log_bpp(n);
    for y in 0..h {
        for x in 0..w {
            f(offset360(x, y, w, lb) as usize * n as usize, ((y * w + x) * n) as usize);
        }
    }
}

pub fn tiled360_len(w: u32, h: u32, n: u32) -> usize {
    (((w + 31) & !31) * ((h + 31) & !31) * n) as usize
}

pub fn untile360(d: &[u8], w: u32, h: u32, n: u32) -> Vec<u8> {
    let mut o = vec![0u8; (w * h * n) as usize];
    let k = n as usize;
    walk360(w, h, n, |t, l| {
        if let Some(s) = d.get(t..t + k) {
            o[l..l + k].copy_from_slice(s);
        }
    });
    o
}

pub fn tile360(d: &[u8], w: u32, h: u32, n: u32) -> Vec<u8> {
    let mut o = vec![0u8; tiled360_len(w, h, n)];
    let k = n as usize;
    walk360(w, h, n, |t, l| {
        if let (Some(s), true) = (d.get(l..l + k), t + k <= o.len()) {
            o[t..t + k].copy_from_slice(s);
        }
    });
    o
}

fn clog2(v: u32) -> u32 {
    32 - v.max(1).saturating_sub(1).leading_zeros()
}

fn a32(v: u32) -> u32 {
    (v + 31) & !31
}

enum Spot {
    Own { at: usize, grid: (u32, u32), size: (u32, u32) },
    Tail { x: u32, y: u32, size: (u32, u32) },
}

struct Chain {
    spots: Vec<Spot>,
    tail: (usize, u32, u32),
    len: usize,
}

fn chain(w: u32, h: u32, mips: u32, b: u32, n: u32) -> Chain {
    let (lw, lh) = (clog2(w), clog2(h));
    let first = lw.min(lh).saturating_sub(4);
    let wide = lw > lh;
    let (mut spots, mut at, mut ext) = (Vec::new(), 0usize, (0u32, 0u32));
    for m in 0..mips {
        let size = (((w >> m).max(1)).div_ceil(b), ((h >> m).max(1)).div_ceil(b));
        let pad = if m == 0 { size } else { ((w.next_power_of_two() >> m).max(1).div_ceil(b), (h.next_power_of_two() >> m).max(1).div_ceil(b)) };
        if m < first {
            let grid = (a32(pad.0), a32(pad.1));
            spots.push(Spot::Own { at, grid, size });
            at += (grid.0 * grid.1 * n) as usize;
            continue;
        }
        let p = m - first;
        let (x, y) = match (p < 3, wide) {
            (true, true) => (0, 16 >> p),
            (true, false) => (16 >> p, 0),
            (false, true) => ((1 << (lw - first)) >> (p - 2), 0),
            (false, false) => (0, (1 << (lh - first)) >> (p - 2)),
        };
        let (x, y) = (x / b, y / b);
        ext = (ext.0.max(x + pad.0), ext.1.max(y + pad.1));
        spots.push(Spot::Tail { x, y, size });
    }
    let (tw, th) = if mips > first { (a32(ext.0), a32(ext.1)) } else { (0, 0) };
    Chain { spots, tail: (at, tw, th), len: at + (tw * th * n) as usize }
}

pub fn tiled360_mips_len(w: u32, h: u32, mips: u32, block: u32, n: u32) -> usize {
    chain(w, h, mips, block, n).len
}

fn rect(src: &[u8], sw: u32, x: u32, y: u32, size: (u32, u32), n: u32, o: &mut Vec<u8>) {
    for r in 0..size.1 {
        let s = (((y + r) * sw + x) * n) as usize;
        o.extend_from_slice(&src[s..s + (size.0 * n) as usize]);
    }
}

fn place(dst: &mut [u8], dw: u32, x: u32, y: u32, size: (u32, u32), n: u32, src: &[u8]) {
    let k = (size.0 * n) as usize;
    for r in 0..size.1 {
        let d = (((y + r) * dw + x) * n) as usize;
        let s = r as usize * k;
        dst[d..d + k].copy_from_slice(&src[s..s + k]);
    }
}

pub fn untile360_mips(d: &[u8], w: u32, h: u32, mips: u32, block: u32, n: u32) -> Option<Vec<u8>> {
    let c = chain(w, h, mips, block, n);
    if d.len() < c.len {
        return None;
    }
    let (ta, tw, th) = c.tail;
    let tail = (tw > 0).then(|| untile360(&d[ta..ta + (tw * th * n) as usize], tw, th, n));
    let mut o = Vec::new();
    for s in &c.spots {
        match *s {
            Spot::Own { at, grid, size } => rect(&untile360(&d[at..at + (grid.0 * grid.1 * n) as usize], grid.0, grid.1, n), grid.0, 0, 0, size, n, &mut o),
            Spot::Tail { x, y, size } => rect(tail.as_deref()?, tw, x, y, size, n, &mut o),
        }
    }
    Some(o)
}

pub fn tile360_mips(lin: &[u8], w: u32, h: u32, mips: u32, block: u32, n: u32) -> Option<Vec<u8>> {
    let c = chain(w, h, mips, block, n);
    let (ta, tw, th) = c.tail;
    let mut o = vec![0u8; c.len];
    let mut tail = vec![0u8; (tw * th * n) as usize];
    let mut p = 0usize;
    for s in &c.spots {
        let (Spot::Own { size, .. } | Spot::Tail { size, .. }) = *s;
        let k = (size.0 * size.1 * n) as usize;
        let src = lin.get(p..p + k)?;
        p += k;
        match *s {
            Spot::Own { at, grid, .. } => {
                let mut g = vec![0u8; (grid.0 * grid.1 * n) as usize];
                place(&mut g, grid.0, 0, 0, size, n, src);
                let t = tile360(&g, grid.0, grid.1, n);
                o[at..at + t.len()].copy_from_slice(&t);
            }
            Spot::Tail { x, y, .. } => place(&mut tail, tw, x, y, size, n, src),
        }
    }
    if tw > 0 {
        let t = tile360(&tail, tw, th, n);
        o[ta..ta + t.len()].copy_from_slice(&t);
    }
    Some(o)
}

fn mort(x: u32, y: u32, w: u32, h: u32) -> usize {
    let (mut o, mut bit, mut i) = (0usize, 0, 0);
    while (1 << i) < w || (1 << i) < h {
        if (1 << i) < w {
            o |= ((x >> i & 1) as usize) << bit;
            bit += 1;
        }
        if (1 << i) < h {
            o |= ((y >> i & 1) as usize) << bit;
            bit += 1;
        }
        i += 1;
    }
    o
}

pub fn unmorton(d: &[u8], w: u32, h: u32, n: u32) -> Vec<u8> {
    let k = n as usize;
    let mut o = vec![0u8; (w * h * n) as usize];
    for y in 0..h {
        for x in 0..w {
            let s = mort(x, y, w, h) * k;
            let t = ((y * w + x) * n) as usize;
            if let Some(v) = d.get(s..s + k) {
                o[t..t + k].copy_from_slice(v);
            }
        }
    }
    o
}

pub fn morton(d: &[u8], w: u32, h: u32, n: u32) -> Vec<u8> {
    let k = n as usize;
    let mut o = vec![0u8; (w * h * n) as usize];
    for y in 0..h {
        for x in 0..w {
            let s = ((y * w + x) * n) as usize;
            let t = mort(x, y, w, h) * k;
            if let (Some(v), true) = (d.get(s..s + k), t + k <= o.len()) {
                o[t..t + k].copy_from_slice(v);
            }
        }
    }
    o
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn x360_is_bijective_and_reversible() {
        for (w, h, n) in [(32, 32, 8), (64, 32, 16), (128, 64, 4), (16, 8, 8), (40, 24, 16)] {
            let tw = (w + 31) & !31;
            let th = (h + 31) & !31;
            let mut seen = vec![false; (tw * th) as usize];
            for y in 0..th {
                for x in 0..tw {
                    let o = offset360(x, y, tw, log_bpp(n)) as usize;
                    assert!(!seen[o], "dup {w}x{h} {x},{y}");
                    seen[o] = true;
                }
            }
            let d: Vec<u8> = (0..w * h * n).map(|i| (i * 7 % 251) as u8).collect();
            assert_eq!(untile360(&tile360(&d, w, h, n), w, h, n), d);
        }
        assert_eq!(offset360(0, 0, 32, 3), 0);
    }

    #[test]
    fn x360_mip_chains() {
        assert_eq!(tiled360_mips_len(128, 128, 8, 4, 8), 32768);
        assert_eq!(tiled360_mips_len(256, 256, 9, 4, 8), 65536);
        assert_eq!(tiled360_mips_len(384, 32, 9, 4, 16), 81920);
        assert_eq!(tiled360_mips_len(128, 128, 8, 1, 4), 90112);
        assert_eq!(tiled360_mips_len(512, 512, 1, 4, 8), 131072);
        for (w, h, m, b, n) in [(128, 128, 8, 4, 8), (256, 64, 9, 4, 16), (64, 256, 9, 4, 8), (128, 128, 8, 1, 4), (16, 16, 5, 4, 8), (512, 32, 10, 4, 8)] {
            let len: usize = (0..m).map(|i| (((w >> i).max(1) as u32).div_ceil(b) * ((h >> i).max(1) as u32).div_ceil(b) * n) as usize).sum();
            let d: Vec<u8> = (0..len).map(|i| (i * 13 % 251) as u8).collect();
            let t = tile360_mips(&d, w, h, m, b, n).unwrap();
            assert_eq!(t.len(), tiled360_mips_len(w, h, m, b, n));
            assert_eq!(untile360_mips(&t, w, h, m, b, n).unwrap(), d, "{w}x{h}");
        }
    }

    #[test]
    fn morton_order() {
        assert_eq!((0..4).map(|i| mort(i % 2, i / 2, 2, 2)).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert_eq!(mort(3, 0, 4, 4), 5);
        assert_eq!(mort(0, 3, 4, 1), 0);
        assert_eq!(mort(3, 0, 8, 2), 5);
        let d: Vec<u8> = (0..8 * 4 * 2).map(|i| i as u8).collect();
        assert_eq!(unmorton(&morton(&d, 8, 4, 2), 8, 4, 2), d);
    }
}
