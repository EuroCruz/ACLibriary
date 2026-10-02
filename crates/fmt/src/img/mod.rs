mod bc;
mod dds;
mod tile;

pub use bc::{bc1, bc2, bc3, bc4, bc5, bc7, block_bytes, decode, encode, Bc};
pub use dds::{Dds, Fmt};
pub use tile::{morton, tile360, tiled360_len, unmorton, untile360};

pub type Px = [u8; 4];

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub px: Vec<Px>,
}

impl Image {
    pub fn new(w: u32, h: u32) -> Image {
        Image { w, h, px: vec![[0; 4]; (w * h) as usize] }
    }

    pub fn from_rgba(w: u32, h: u32, b: &[u8]) -> Image {
        Image { w, h, px: b.chunks_exact(4).take((w * h) as usize).map(|c| [c[0], c[1], c[2], c[3]]).collect() }
    }

    pub fn rgba(&self) -> Vec<u8> {
        self.px.iter().flatten().copied().collect()
    }

    pub fn get(&self, x: u32, y: u32) -> Px {
        self.px[(y.min(self.h - 1) * self.w + x.min(self.w - 1)) as usize]
    }

    pub fn set(&mut self, x: u32, y: u32, p: Px) {
        if x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = p;
        }
    }

    pub fn opaque(&self) -> bool {
        self.px.iter().all(|p| p[3] == 255)
    }

    pub fn flip(&mut self) {
        let w = self.w as usize;
        for y in 0..self.h as usize / 2 {
            let z = self.h as usize - 1 - y;
            for x in 0..w {
                self.px.swap(y * w + x, z * w + x);
            }
        }
    }

    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Image {
        let mut o = Image::new(w, h);
        for j in 0..h {
            for i in 0..w {
                o.set(i, j, self.get(x + i, y + j));
            }
        }
        o
    }

    pub fn swap_rb(&mut self) {
        self.px.iter_mut().for_each(|p| p.swap(0, 2));
    }

    pub fn resize(&self, w: u32, h: u32) -> Image {
        let mut o = Image::new(w, h);
        let (sx, sy) = (self.w as f32 / w as f32, self.h as f32 / h as f32);
        for y in 0..h {
            let fy = ((y as f32 + 0.5) * sy - 0.5).max(0.0);
            let (y0, ty) = (fy as u32, fy.fract());
            for x in 0..w {
                let fx = ((x as f32 + 0.5) * sx - 0.5).max(0.0);
                let (x0, tx) = (fx as u32, fx.fract());
                let (a, b, c, d) = (self.get(x0, y0), self.get(x0 + 1, y0), self.get(x0, y0 + 1), self.get(x0 + 1, y0 + 1));
                let mut p = [0u8; 4];
                for k in 0..4 {
                    let top = a[k] as f32 + (b[k] as f32 - a[k] as f32) * tx;
                    let bot = c[k] as f32 + (d[k] as f32 - c[k] as f32) * tx;
                    p[k] = (top + (bot - top) * ty).round() as u8;
                }
                o.set(x, y, p);
            }
        }
        o
    }

    pub fn half(&self, normal: bool) -> Image {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let mut o = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let q = [self.get(2 * x, 2 * y), self.get(2 * x + 1, 2 * y), self.get(2 * x, 2 * y + 1), self.get(2 * x + 1, 2 * y + 1)];
                let mut p = [0u8; 4];
                for k in 0..4 {
                    p[k] = ((q.iter().map(|c| c[k] as u32).sum::<u32>() + 2) / 4) as u8;
                }
                if normal {
                    let v: Vec<f32> = (0..3).map(|k| p[k] as f32 / 127.5 - 1.0).collect();
                    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
                    for k in 0..3 {
                        p[k] = ((v[k] / l + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
                    }
                }
                o.set(x, y, p);
            }
        }
        o
    }

    pub fn mips(self, n: u32, normal: bool) -> Vec<Image> {
        let mut v = vec![self];
        while (v.len() as u32) < n.min(levels(v[0].w, v[0].h)) {
            let l = v.last().unwrap().half(normal);
            v.push(l);
        }
        v
    }
}

pub fn levels(w: u32, h: u32) -> u32 {
    32 - w.max(h).max(1).leading_zeros()
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn ops() {
        let mut i = Image::new(4, 2);
        i.set(0, 0, [1, 2, 3, 4]);
        i.set(3, 1, [9, 9, 9, 255]);
        assert_eq!(Image::from_rgba(4, 2, &i.rgba()), i);
        let mut f = i.clone();
        f.flip();
        assert_eq!(f.get(0, 1), [1, 2, 3, 4]);
        assert_eq!(i.crop(3, 1, 1, 1).px, [[9, 9, 9, 255]]);
        let mut s = i.clone();
        s.swap_rb();
        assert_eq!(s.get(0, 0), [3, 2, 1, 4]);
        assert_eq!(levels(256, 64), 9);
        assert_eq!(levels(1, 1), 1);
        let m = Image::new(16, 4).mips(99, false);
        assert_eq!(m.iter().map(|x| (x.w, x.h)).collect::<Vec<_>>(), [(16, 4), (8, 2), (4, 1), (2, 1), (1, 1)]);
        let r = Image { w: 2, h: 1, px: vec![[0; 4], [255; 4]] }.resize(4, 1);
        assert_eq!(r.px[0][0], 0);
        assert_eq!(r.px[3][0], 255);
        let n = Image { w: 2, h: 2, px: vec![[255, 128, 128, 255], [128, 255, 128, 255], [128, 128, 255, 255], [128, 128, 255, 255]] }.half(true);
        let v: f32 = (0..3).map(|k| (n.px[0][k] as f32 / 127.5 - 1.0).powi(2)).sum();
        assert!((v.sqrt() - 1.0).abs() < 0.02);
    }
}
