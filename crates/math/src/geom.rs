use crate::{Mat4, V2, V3};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sphere {
    pub c: V3,
    pub r: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Plane {
    pub n: V3,
    pub d: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Ray {
    pub o: V3,
    pub d: V3,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Aabb {
    pub fn new(a: V3, b: V3) -> Aabb {
        Aabb { min: a.min(b), max: a.max(b) }
    }

    pub fn of(pts: &[V3]) -> Option<Aabb> {
        let (&f, r) = pts.split_first()?;
        Some(r.iter().fold(Aabb { min: f, max: f }, |b, &p| b.grow(p)))
    }

    pub fn grow(self, p: V3) -> Aabb {
        Aabb { min: self.min.min(p), max: self.max.max(p) }
    }

    pub fn union(self, o: Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }

    pub fn center(&self) -> V3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> V3 {
        self.max - self.min
    }

    pub fn has(&self, p: V3) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y && p.z >= self.min.z && p.z <= self.max.z
    }

    pub fn hits(&self, o: &Aabb) -> bool {
        self.min.x <= o.max.x && self.max.x >= o.min.x && self.min.y <= o.max.y && self.max.y >= o.min.y && self.min.z <= o.max.z && self.max.z >= o.min.z
    }

    pub fn xform(&self, m: &Mat4) -> Aabb {
        let (a, b) = (self.min, self.max);
        let c = [
            V3::new(a.x, a.y, a.z), V3::new(b.x, a.y, a.z), V3::new(a.x, b.y, a.z), V3::new(b.x, b.y, a.z),
            V3::new(a.x, a.y, b.z), V3::new(b.x, a.y, b.z), V3::new(a.x, b.y, b.z), V3::new(b.x, b.y, b.z),
        ];
        Aabb::of(&c.map(|p| m.point(p))).unwrap()
    }
}

impl Plane {
    pub fn new(n: V3, d: f32) -> Plane {
        Plane { n: n.norm(), d }
    }

    pub fn tri(a: V3, b: V3, c: V3) -> Plane {
        let n = (b - a).cross(c - a).norm();
        Plane { n, d: -n.dot(a) }
    }

    pub fn dist(&self, p: V3) -> f32 {
        self.n.dot(p) + self.d
    }

    pub fn nearest(&self, p: V3) -> V3 {
        p - self.n * self.dist(p)
    }
}

impl Ray {
    pub fn new(o: V3, d: V3) -> Ray {
        Ray { o, d: d.norm() }
    }

    pub fn at(&self, t: f32) -> V3 {
        self.o + self.d * t
    }

    pub fn plane(&self, p: &Plane) -> Option<f32> {
        let den = p.n.dot(self.d);
        if den.abs() < 1e-8 {
            return None;
        }
        let t = -p.dist(self.o) / den;
        (t >= 0.0).then_some(t)
    }

    pub fn sphere(&self, s: &Sphere) -> Option<f32> {
        let oc = self.o - s.c;
        let b = oc.dot(self.d);
        let c = oc.len2() - s.r * s.r;
        let h = b * b - c;
        if h < 0.0 {
            return None;
        }
        let h = h.sqrt();
        let t = if -b - h >= 0.0 { -b - h } else { -b + h };
        (t >= 0.0).then_some(t)
    }

    pub fn aabb(&self, b: &Aabb) -> Option<f32> {
        let (mut t0, mut t1) = (0.0f32, f32::INFINITY);
        for (o, d, lo, hi) in [(self.o.x, self.d.x, b.min.x, b.max.x), (self.o.y, self.d.y, b.min.y, b.max.y), (self.o.z, self.d.z, b.min.z, b.max.z)] {
            if d.abs() < 1e-12 {
                if o < lo || o > hi {
                    return None;
                }
            } else {
                let (a, c) = ((lo - o) / d, (hi - o) / d);
                t0 = t0.max(a.min(c));
                t1 = t1.min(a.max(c));
                if t0 > t1 {
                    return None;
                }
            }
        }
        Some(t0)
    }

    pub fn tri(&self, a: V3, b: V3, c: V3) -> Option<f32> {
        let (e1, e2) = (b - a, c - a);
        let p = self.d.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-10 {
            return None;
        }
        let inv = 1.0 / det;
        let s = self.o - a;
        let u = s.dot(p) * inv;
        let q = s.cross(e1);
        let v = self.d.dot(q) * inv;
        let t = e2.dot(q) * inv;
        (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t >= 0.0).then_some(t)
    }
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn from_pts(a: V2, b: V2) -> Rect {
        let (lo, hi) = (a.min(b), a.max(b));
        Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
    }

    pub fn r(&self) -> f32 {
        self.x + self.w
    }

    pub fn b(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> V2 {
        V2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn has(&self, p: V2) -> bool {
        p.x >= self.x && p.x < self.r() && p.y >= self.y && p.y < self.b()
    }

    pub fn hits(&self, o: &Rect) -> bool {
        self.x < o.r() && self.r() > o.x && self.y < o.b() && self.b() > o.y
    }

    pub fn clip(&self, o: &Rect) -> Option<Rect> {
        let (x, y) = (self.x.max(o.x), self.y.max(o.y));
        let (r, b) = (self.r().min(o.r()), self.b().min(o.b()));
        (r > x && b > y).then(|| Rect::new(x, y, r - x, b - y))
    }

    pub fn union(&self, o: &Rect) -> Rect {
        Rect::from_pts(V2::new(self.x.min(o.x), self.y.min(o.y)), V2::new(self.r().max(o.r()), self.b().max(o.b())))
    }

    pub fn pad(&self, d: f32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + d * 2.0, self.h + d * 2.0)
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn boxes() {
        let b = Aabb::of(&[V3::new(-1.0, -1.0, -1.0), V3::ONE]).unwrap();
        assert!(b.has(V3::ZERO) && !b.has(V3::splat(2.0)));
        assert_eq!(b.center(), V3::ZERO);
        assert!(b.hits(&Aabb::new(V3::ONE, V3::splat(3.0))));
        assert_eq!(b.xform(&Mat4::translate(V3::X)).min, V3::new(0.0, -1.0, -1.0));
        assert!(Aabb::of(&[]).is_none());
    }

    #[test]
    fn rays() {
        let r = Ray::new(V3::new(0.0, 0.0, -5.0), V3::Z);
        let b = Aabb::new(V3::splat(-1.0), V3::ONE);
        assert_eq!(r.aabb(&b), Some(4.0));
        assert_eq!(r.sphere(&Sphere { c: V3::ZERO, r: 1.0 }), Some(4.0));
        assert_eq!(r.plane(&Plane::new(V3::Z, 0.0)), Some(5.0));
        assert_eq!(r.tri(V3::new(-1.0, -1.0, 0.0), V3::new(1.0, -1.0, 0.0), V3::new(0.0, 1.0, 0.0)), Some(5.0));
        assert!(Ray::new(V3::new(5.0, 0.0, -5.0), V3::Z).aabb(&b).is_none());
    }

    #[test]
    fn rects() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert!(a.hits(&b) && a.has(V2::new(1.0, 1.0)));
        assert_eq!(a.clip(&b), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 15.0, 15.0));
        assert!(a.clip(&Rect::new(20.0, 20.0, 1.0, 1.0)).is_none());
    }
}
