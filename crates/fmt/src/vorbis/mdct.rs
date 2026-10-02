use std::f64::consts::PI;

#[derive(Clone, Copy, Default)]
struct C {
    r: f64,
    i: f64,
}

impl C {
    fn mul(self, o: C) -> C {
        C { r: self.r * o.r - self.i * o.i, i: self.r * o.i + self.i * o.r }
    }
}

pub struct Mdct {
    pub n: usize,
    tw: Vec<C>,
    roots: Vec<C>,
    rev: Vec<usize>,
    buf: Vec<C>,
}

impl Mdct {
    pub fn new(n: usize) -> Mdct {
        let m = n / 2;
        let q = m / 2;
        let tw = (0..q).map(|k| {
            let a = -PI * (k as f64 + 0.125) / m as f64;
            C { r: a.cos(), i: a.sin() }
        });
        let roots = (0..q / 2).map(|k| {
            let a = -2.0 * PI * k as f64 / q as f64;
            C { r: a.cos(), i: a.sin() }
        });
        let bits = q.trailing_zeros();
        let rev = (0..q).map(|i| if bits == 0 { 0 } else { i.reverse_bits() >> (usize::BITS - bits) });
        Mdct { n, tw: tw.collect(), roots: roots.collect(), rev: rev.collect(), buf: vec![C::default(); q] }
    }

    fn fft(&mut self) {
        let q = self.buf.len();
        for i in 0..q {
            let j = self.rev[i];
            if i < j {
                self.buf.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= q {
            let step = q / len;
            for s in (0..q).step_by(len) {
                for k in 0..len / 2 {
                    let w = self.roots[k * step];
                    let a = self.buf[s + k];
                    let b = self.buf[s + k + len / 2].mul(w);
                    self.buf[s + k] = C { r: a.r + b.r, i: a.i + b.i };
                    self.buf[s + k + len / 2] = C { r: a.r - b.r, i: a.i - b.i };
                }
            }
            len *= 2;
        }
    }

    pub fn dct4(&mut self, x: &[f32], u: &mut [f32]) {
        let m = self.n / 2;
        let q = m / 2;
        for k in 0..q {
            self.buf[k] = C { r: x[2 * k] as f64, i: x[m - 1 - 2 * k] as f64 }.mul(self.tw[k]);
        }
        self.fft();
        for p in 0..q {
            let c = self.buf[p].mul(self.tw[p]);
            u[2 * p] = c.r as f32;
            u[m - 1 - 2 * p] = -c.i as f32;
        }
    }

    pub fn inverse(&mut self, x: &[f32], y: &mut [f32]) {
        let m = self.n / 2;
        let mut u = vec![0f32; m];
        self.dct4(x, &mut u);
        for (n, v) in y.iter_mut().enumerate().take(self.n) {
            let k = n + m / 2;
            *v = if k < m {
                u[k]
            } else if k < 2 * m {
                -u[2 * m - 1 - k]
            } else {
                -u[k - 2 * m]
            };
        }
    }

    pub fn forward(&mut self, x: &[f32], out: &mut [f32]) {
        let m = self.n / 2;
        let h = m / 2;
        let mut u = vec![0f32; m];
        for (n, v) in u.iter_mut().enumerate() {
            *v = if n < h { -x[3 * h - 1 - n] - x[3 * h + n] } else { x[n - h] - x[3 * h - 1 - n] };
        }
        self.dct4(&u, out);
    }
}

pub fn slope(i: usize, len: usize) -> f32 {
    let s = ((i as f64 + 0.5) / len as f64 * PI / 2.0).sin();
    (PI / 2.0 * s * s).sin() as f32
}

pub fn window(n: usize, left: usize, right: usize, w: &mut [f32]) {
    let (ls, rs) = (n / 4 - left / 2, n * 3 / 4 - right / 2);
    for (i, v) in w.iter_mut().enumerate().take(n) {
        *v = if i < ls {
            0.0
        } else if i < ls + left {
            slope(i - ls, left)
        } else if i < rs {
            1.0
        } else if i < rs + right {
            slope(right - 1 - (i - rs), right)
        } else {
            0.0
        };
    }
}

#[cfg(test)]
mod t {
    use super::*;

    fn rnd(n: usize, seed: u64) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s % 2001) as f32 / 1000.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn matches_direct() {
        for n in [64usize, 256, 2048] {
            let m = n / 2;
            let x = rnd(m, n as u64);
            let mut y = vec![0f32; n];
            let mut t = Mdct::new(n);
            t.inverse(&x, &mut y);
            for (i, &v) in y.iter().enumerate().step_by(7) {
                let d: f64 = (0..m).map(|k| x[k] as f64 * (2.0 * PI / n as f64 * (i as f64 + 0.5 + n as f64 / 4.0) * (k as f64 + 0.5)).cos()).sum();
                assert!((v as f64 - d).abs() < 1e-3 * (1.0 + d.abs()), "imdct n={n} i={i} {v} {d}");
            }
            let s = rnd(n, 7 + n as u64);
            let mut f = vec![0f32; m];
            t.forward(&s, &mut f);
            for (k, &v) in f.iter().enumerate().step_by(5) {
                let d: f64 = (0..n).map(|i| s[i] as f64 * (2.0 * PI / n as f64 * (i as f64 + 0.5 + n as f64 / 4.0) * (k as f64 + 0.5)).cos()).sum();
                assert!((v as f64 - d).abs() < 1e-3 * (1.0 + d.abs()), "mdct n={n} k={k} {v} {d}");
            }
        }
    }

    #[test]
    fn perfect_reconstruction() {
        let n = 256;
        let sig = rnd(n * 4, 99);
        let mut t = Mdct::new(n);
        let mut w = vec![0f32; n];
        window(n, n / 2, n / 2, &mut w);
        let mut out = vec![0f32; n * 4 + n];
        let (mut c, mut y) = (vec![0f32; n / 2], vec![0f32; n]);
        let mut pos = 0;
        while pos + n <= sig.len() {
            let blk: Vec<f32> = (0..n).map(|i| sig[pos + i] * w[i]).collect();
            t.forward(&blk, &mut c);
            t.inverse(&c, &mut y);
            for i in 0..n {
                out[pos + i] += y[i] * w[i] * 4.0 / n as f32;
            }
            pos += n / 2;
        }
        for i in n / 2..sig.len() - n / 2 {
            assert!((out[i] - sig[i]).abs() < 1e-4, "{i} {} {}", out[i], sig[i]);
        }
        let mut sw = vec![0f32; 2048];
        window(2048, 128, 1024, &mut sw);
        assert_eq!((sw[0], sw[447], sw[1023], sw[2047]), (0.0, 0.0, 1.0, slope(0, 1024)));
        assert!(sw[448] > 0.0 && sw[575] > 0.99);
    }
}
