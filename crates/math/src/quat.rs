use crate::{Mat4, V3};
use std::ops::Mul;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Default for Quat {
    fn default() -> Quat {
        Quat::ID
    }
}

impl Quat {
    pub const ID: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Quat {
        Quat { x, y, z, w }
    }

    pub fn axis(a: V3, ang: f32) -> Quat {
        let (s, c) = (ang * 0.5).sin_cos();
        let n = a.norm() * s;
        Quat::new(n.x, n.y, n.z, c)
    }

    pub fn euler(pitch: f32, yaw: f32, roll: f32) -> Quat {
        Quat::axis(V3::Y, yaw) * Quat::axis(V3::X, pitch) * Quat::axis(V3::Z, roll)
    }

    pub fn xyz(self) -> V3 {
        V3::new(self.x, self.y, self.z)
    }

    pub fn dot(self, o: Quat) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    pub fn conj(self) -> Quat {
        Quat::new(-self.x, -self.y, -self.z, self.w)
    }

    pub fn norm(self) -> Quat {
        let l = self.dot(self).sqrt();
        if l > 1e-12 { Quat::new(self.x / l, self.y / l, self.z / l, self.w / l) } else { Quat::ID }
    }

    pub fn inv(self) -> Quat {
        let d = self.dot(self);
        let c = self.conj();
        Quat::new(c.x / d, c.y / d, c.z / d, c.w / d)
    }

    pub fn rot(self, v: V3) -> V3 {
        let t = self.xyz().cross(v) * 2.0;
        v + t * self.w + self.xyz().cross(t)
    }

    pub fn nlerp(self, o: Quat, t: f32) -> Quat {
        let s = if self.dot(o) < 0.0 { -1.0 } else { 1.0 };
        Quat::new(
            self.x + (o.x * s - self.x) * t,
            self.y + (o.y * s - self.y) * t,
            self.z + (o.z * s - self.z) * t,
            self.w + (o.w * s - self.w) * t,
        )
        .norm()
    }

    pub fn slerp(self, o: Quat, t: f32) -> Quat {
        let mut d = self.dot(o);
        let o = if d < 0.0 {
            d = -d;
            Quat::new(-o.x, -o.y, -o.z, -o.w)
        } else {
            o
        };
        if d > 0.9995 {
            return self.nlerp(o, t);
        }
        let th = d.acos();
        let (a, b) = (((1.0 - t) * th).sin() / th.sin(), (t * th).sin() / th.sin());
        Quat::new(self.x * a + o.x * b, self.y * a + o.y * b, self.z * a + o.z * b, self.w * a + o.w * b)
    }

    pub fn to_mat(self) -> Mat4 {
        let Quat { x, y, z, w } = self;
        Mat4::from_rows([
            1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w), 0.0,
            2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w), 0.0,
            2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y), 0.0,
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    pub fn from_mat(m: &Mat4) -> Quat {
        let g = |r, c| m.at(r, c);
        let tr = g(0, 0) + g(1, 1) + g(2, 2);
        let q = if tr > 0.0 {
            let s = (tr + 1.0).sqrt() * 2.0;
            Quat::new((g(2, 1) - g(1, 2)) / s, (g(0, 2) - g(2, 0)) / s, (g(1, 0) - g(0, 1)) / s, s * 0.25)
        } else if g(0, 0) > g(1, 1) && g(0, 0) > g(2, 2) {
            let s = (1.0 + g(0, 0) - g(1, 1) - g(2, 2)).sqrt() * 2.0;
            Quat::new(s * 0.25, (g(0, 1) + g(1, 0)) / s, (g(0, 2) + g(2, 0)) / s, (g(2, 1) - g(1, 2)) / s)
        } else if g(1, 1) > g(2, 2) {
            let s = (1.0 + g(1, 1) - g(0, 0) - g(2, 2)).sqrt() * 2.0;
            Quat::new((g(0, 1) + g(1, 0)) / s, s * 0.25, (g(1, 2) + g(2, 1)) / s, (g(0, 2) - g(2, 0)) / s)
        } else {
            let s = (1.0 + g(2, 2) - g(0, 0) - g(1, 1)).sqrt() * 2.0;
            Quat::new((g(0, 2) + g(2, 0)) / s, (g(1, 2) + g(2, 1)) / s, s * 0.25, (g(1, 0) - g(0, 1)) / s)
        };
        q.norm()
    }

    pub fn to_euler(self) -> (f32, f32, f32) {
        let m = self.to_mat();
        let s = -m.at(1, 2);
        if s.abs() > 0.99999 {
            (s.signum() * std::f32::consts::FRAC_PI_2, (-m.at(2, 0)).atan2(m.at(0, 0)), 0.0)
        } else {
            (s.asin(), m.at(0, 2).atan2(m.at(2, 2)), m.at(1, 0).atan2(m.at(1, 1)))
        }
    }

    pub fn between(a: V3, b: V3) -> Quat {
        let (a, b) = (a.norm(), b.norm());
        let d = a.dot(b);
        if d < -0.999999 {
            let ax = if a.x.abs() < 0.9 { V3::X } else { V3::Y }.cross(a).norm();
            return Quat::axis(ax, std::f32::consts::PI);
        }
        let c = a.cross(b);
        Quat::new(c.x, c.y, c.z, 1.0 + d).norm()
    }

    pub fn look(fwd: V3, up: V3) -> Quat {
        let z = fwd.norm();
        let x = up.cross(z).norm();
        let y = z.cross(x);
        Quat::from_mat(&Mat4::from_rows([x.x, y.x, z.x, 0.0, x.y, y.y, z.y, 0.0, x.z, y.z, z.z, 0.0, 0.0, 0.0, 0.0, 1.0]))
    }

    pub fn angle(self) -> f32 {
        2.0 * self.w.clamp(-1.0, 1.0).acos()
    }
}

impl Mul for Quat {
    type Output = Quat;
    fn mul(self, o: Quat) -> Quat {
        Quat::new(
            self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
            self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
        )
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::near;

    fn close(a: V3, b: V3) -> bool {
        near(a.x, b.x) && near(a.y, b.y) && near(a.z, b.z)
    }

    #[test]
    fn rotation() {
        let q = Quat::axis(V3::Z, std::f32::consts::FRAC_PI_2);
        assert!(close(q.rot(V3::X), V3::Y));
        assert!(close(q.to_mat().point(V3::X), V3::Y));
        assert!(close((q * q.inv()).rot(V3::X), V3::X));
        assert!(near(q.angle(), std::f32::consts::FRAC_PI_2));
    }

    #[test]
    fn conversions() {
        let q = Quat::euler(0.3, -0.8, 0.5);
        let r = Quat::from_mat(&q.to_mat());
        assert!(close(q.rot(V3::new(1.0, 2.0, 3.0)), r.rot(V3::new(1.0, 2.0, 3.0))));
        let (p, y, ro) = q.to_euler();
        assert!(near(p, 0.3) && near(y, -0.8) && near(ro, 0.5));
        assert!(close(Quat::between(V3::X, V3::Y).rot(V3::X), V3::Y));
        assert!(close(Quat::between(V3::X, -V3::X).rot(V3::X), -V3::X));
        assert!(close(Quat::look(V3::X, V3::Y).rot(V3::Z), V3::X));
    }

    #[test]
    fn interpolation() {
        let a = Quat::ID;
        let b = Quat::axis(V3::Y, std::f32::consts::FRAC_PI_2);
        let h = a.slerp(b, 0.5);
        assert!(near(h.angle(), std::f32::consts::FRAC_PI_4));
        assert!(close(a.slerp(b, 1.0).rot(V3::Z), b.rot(V3::Z)));
    }
}
