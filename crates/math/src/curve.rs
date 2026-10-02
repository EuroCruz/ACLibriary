use crate::{V2, V3, V4};
use std::ops::{Add, Mul, Sub};

pub trait Lin: Copy + Add<Output = Self> + Sub<Output = Self> + Mul<f32, Output = Self> {
    fn gap(self, o: Self) -> f32;
}

impl Lin for f32 {
    fn gap(self, o: f32) -> f32 {
        (self - o).abs()
    }
}

macro_rules! lin {
    ($($t:ty),*) => {$(
        impl Lin for $t {
            fn gap(self, o: $t) -> f32 {
                self.dist(o)
            }
        }
    )*};
}

lin!(V2, V3, V4);

pub fn bezier<T: Lin>(a: T, b: T, c: T, d: T, t: f32) -> T {
    let u = 1.0 - t;
    a * (u * u * u) + b * (3.0 * u * u * t) + c * (3.0 * u * t * t) + d * (t * t * t)
}

pub fn hermite<T: Lin>(p0: T, m0: T, p1: T, m1: T, t: f32) -> T {
    let (t2, t3) = (t * t, t * t * t);
    p0 * (2.0 * t3 - 3.0 * t2 + 1.0) + m0 * (t3 - 2.0 * t2 + t) + p1 * (-2.0 * t3 + 3.0 * t2) + m1 * (t3 - t2)
}

pub fn catmull<T: Lin>(p0: T, p1: T, p2: T, p3: T, t: f32) -> T {
    hermite(p1, (p2 - p0) * 0.5, p2, (p3 - p1) * 0.5, t)
}

#[derive(Clone, Debug)]
pub struct Spline<T: Lin> {
    pts: Vec<T>,
    acc: Vec<f32>,
}

const STEPS: usize = 16;

impl<T: Lin> Spline<T> {
    pub fn new(pts: Vec<T>) -> Option<Spline<T>> {
        if pts.len() < 2 {
            return None;
        }
        let mut s = Spline { pts, acc: Vec::new() };
        let n = (s.pts.len() - 1) * STEPS;
        let mut acc = vec![0.0];
        let mut prev = s.at(0.0);
        for i in 1..=n {
            let p = s.at(i as f32 / n as f32);
            acc.push(acc[i - 1] + prev.gap(p));
            prev = p;
        }
        s.acc = acc;
        Some(s)
    }

    fn pt(&self, i: isize) -> T {
        self.pts[i.clamp(0, self.pts.len() as isize - 1) as usize]
    }

    pub fn at(&self, u: f32) -> T {
        let segs = (self.pts.len() - 1) as f32;
        let f = u.clamp(0.0, 1.0) * segs;
        let i = (f.floor() as isize).min(self.pts.len() as isize - 2);
        catmull(self.pt(i - 1), self.pt(i), self.pt(i + 1), self.pt(i + 2), f - i as f32)
    }

    pub fn len(&self) -> f32 {
        *self.acc.last().unwrap()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn at_dist(&self, d: f32) -> T {
        let d = d.clamp(0.0, self.len());
        let i = self.acc.partition_point(|&a| a < d).clamp(1, self.acc.len() - 1);
        let (a, b) = (self.acc[i - 1], self.acc[i]);
        let k = if b > a { (d - a) / (b - a) } else { 0.0 };
        self.at((i - 1) as f32 / (self.acc.len() - 1) as f32 + k / (self.acc.len() - 1) as f32)
    }

    pub fn tangent(&self, u: f32) -> T {
        let e = 1e-3;
        self.at((u + e).min(1.0)) - self.at((u - e).max(0.0))
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::near;

    #[test]
    fn segments() {
        assert!(near(bezier(0.0, 0.0, 1.0, 1.0, 0.5), 0.5));
        assert!(near(catmull(0.0, 1.0, 2.0, 3.0, 0.5), 1.5));
        assert_eq!(hermite(V3::ZERO, V3::X, V3::X, V3::X, 1.0), V3::X);
    }

    #[test]
    fn spline() {
        let s = Spline::new(vec![V3::ZERO, V3::new(10.0, 0.0, 0.0), V3::new(20.0, 0.0, 0.0)]).unwrap();
        assert!(near(s.len(), 20.0));
        assert!((s.at(0.5).x - 10.0).abs() < 1e-4);
        assert!((s.at_dist(5.0).x - 5.0).abs() < 0.05);
        assert!(s.tangent(0.3).x > 0.0);
        assert!(Spline::new(vec![V3::ZERO]).is_none());
        assert_eq!(s.at(0.0), V3::ZERO);
    }
}
