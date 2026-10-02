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
    fn morton_order() {
        assert_eq!((0..4).map(|i| mort(i % 2, i / 2, 2, 2)).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert_eq!(mort(3, 0, 4, 4), 5);
        assert_eq!(mort(0, 3, 4, 1), 0);
        assert_eq!(mort(3, 0, 8, 2), 5);
        let d: Vec<u8> = (0..8 * 4 * 2).map(|i| i as u8).collect();
        assert_eq!(unmorton(&morton(&d, 8, 4, 2), 8, 4, 2), d);
    }
}
