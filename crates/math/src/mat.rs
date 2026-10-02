use crate::{V3, V4};
use std::ops::Mul;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Mat4 {
    pub m: [f32; 16],
}

impl Default for Mat4 {
    fn default() -> Mat4 {
        Mat4::ID
    }
}

impl Mat4 {
    pub const ID: Mat4 = Mat4 { m: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] };

    pub fn from_rows(r: [f32; 16]) -> Mat4 {
        Mat4 { m: r }.transpose()
    }

    pub fn rows(&self) -> [f32; 16] {
        self.transpose().m
    }

    pub fn at(&self, r: usize, c: usize) -> f32 {
        self.m[c * 4 + r]
    }

    pub fn set(&mut self, r: usize, c: usize, v: f32) {
        self.m[c * 4 + r] = v;
    }

    pub fn transpose(&self) -> Mat4 {
        let mut o = [0.0; 16];
        for r in 0..4 {
            for c in 0..4 {
                o[r * 4 + c] = self.m[c * 4 + r];
            }
        }
        Mat4 { m: o }
    }

    pub fn pos(&self) -> V3 {
        V3::new(self.at(0, 3), self.at(1, 3), self.at(2, 3))
    }

    pub fn translate(t: V3) -> Mat4 {
        Mat4::from_rows([1.0, 0.0, 0.0, t.x, 0.0, 1.0, 0.0, t.y, 0.0, 0.0, 1.0, t.z, 0.0, 0.0, 0.0, 1.0])
    }

    pub fn scale(s: V3) -> Mat4 {
        Mat4::from_rows([s.x, 0.0, 0.0, 0.0, 0.0, s.y, 0.0, 0.0, 0.0, 0.0, s.z, 0.0, 0.0, 0.0, 0.0, 1.0])
    }

    pub fn rot_x(a: f32) -> Mat4 {
        let (s, c) = a.sin_cos();
        Mat4::from_rows([1.0, 0.0, 0.0, 0.0, 0.0, c, -s, 0.0, 0.0, s, c, 0.0, 0.0, 0.0, 0.0, 1.0])
    }

    pub fn rot_y(a: f32) -> Mat4 {
        let (s, c) = a.sin_cos();
        Mat4::from_rows([c, 0.0, s, 0.0, 0.0, 1.0, 0.0, 0.0, -s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0])
    }

    pub fn rot_z(a: f32) -> Mat4 {
        let (s, c) = a.sin_cos();
        Mat4::from_rows([c, -s, 0.0, 0.0, s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0])
    }

    fn view(x: V3, y: V3, z: V3, e: V3) -> Mat4 {
        Mat4::from_rows([x.x, x.y, x.z, -x.dot(e), y.x, y.y, y.z, -y.dot(e), z.x, z.y, z.z, -z.dot(e), 0.0, 0.0, 0.0, 1.0])
    }

    pub fn look_lh(eye: V3, target: V3, up: V3) -> Mat4 {
        let z = (target - eye).norm();
        let x = up.cross(z).norm();
        Mat4::view(x, z.cross(x), z, eye)
    }

    pub fn look_rh(eye: V3, target: V3, up: V3) -> Mat4 {
        let z = (eye - target).norm();
        let x = up.cross(z).norm();
        Mat4::view(x, z.cross(x), z, eye)
    }

    pub fn persp_lh(fov: f32, aspect: f32, zn: f32, zf: f32) -> Mat4 {
        let ys = 1.0 / (fov * 0.5).tan();
        let q = zf / (zf - zn);
        Mat4::from_rows([ys / aspect, 0.0, 0.0, 0.0, 0.0, ys, 0.0, 0.0, 0.0, 0.0, q, -zn * q, 0.0, 0.0, 1.0, 0.0])
    }

    pub fn persp_rh(fov: f32, aspect: f32, zn: f32, zf: f32) -> Mat4 {
        let ys = 1.0 / (fov * 0.5).tan();
        let q = zf / (zn - zf);
        Mat4::from_rows([ys / aspect, 0.0, 0.0, 0.0, 0.0, ys, 0.0, 0.0, 0.0, 0.0, q, zn * q, 0.0, 0.0, -1.0, 0.0])
    }

    pub fn ortho(l: f32, r: f32, b: f32, t: f32, zn: f32, zf: f32) -> Mat4 {
        let q = 1.0 / (zf - zn);
        Mat4::from_rows([
            2.0 / (r - l), 0.0, 0.0, -(r + l) / (r - l),
            0.0, 2.0 / (t - b), 0.0, -(t + b) / (t - b),
            0.0, 0.0, q, -zn * q,
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    pub fn point(&self, v: V3) -> V3 {
        (*self * v.ext(1.0)).xyz()
    }

    pub fn dir(&self, v: V3) -> V3 {
        (*self * v.ext(0.0)).xyz()
    }

    pub fn proj(&self, v: V3) -> V3 {
        let r = *self * v.ext(1.0);
        if r.w.abs() > 1e-12 { r.xyz() / r.w } else { r.xyz() }
    }

    pub fn det(&self) -> f32 {
        self.elim().map_or(0.0, |(_, d)| d as f32)
    }

    pub fn inv(&self) -> Option<Mat4> {
        self.elim().map(|(m, _)| m)
    }

    fn elim(&self) -> Option<(Mat4, f64)> {
        let mut a = [[0f64; 8]; 4];
        for r in 0..4 {
            for c in 0..4 {
                a[r][c] = self.at(r, c) as f64;
            }
            a[r][4 + r] = 1.0;
        }
        let mut det = 1f64;
        for i in 0..4 {
            let p = (i..4).max_by(|&x, &y| a[x][i].abs().total_cmp(&a[y][i].abs()))?;
            if a[p][i].abs() < 1e-12 {
                return None;
            }
            if p != i {
                a.swap(p, i);
                det = -det;
            }
            let d = a[i][i];
            det *= d;
            a[i].iter_mut().for_each(|v| *v /= d);
            for r in 0..4 {
                if r != i {
                    let f = a[r][i];
                    let row = a[i];
                    a[r].iter_mut().zip(row).for_each(|(v, s)| *v -= f * s);
                }
            }
        }
        let mut o = Mat4::ID;
        for r in 0..4 {
            for c in 0..4 {
                o.set(r, c, a[r][4 + c] as f32);
            }
        }
        Some((o, det))
    }
}

impl Mul for Mat4 {
    type Output = Mat4;
    fn mul(self, o: Mat4) -> Mat4 {
        let mut m = [0.0; 16];
        for c in 0..4 {
            for r in 0..4 {
                m[c * 4 + r] = (0..4).map(|k| self.at(r, k) * o.at(k, c)).sum();
            }
        }
        Mat4 { m }
    }
}

impl Mul<V4> for Mat4 {
    type Output = V4;
    fn mul(self, v: V4) -> V4 {
        let a = v.arr();
        let r = |i: usize| (0..4).map(|k| self.at(i, k) * a[k]).sum::<f32>();
        V4::new(r(0), r(1), r(2), r(3))
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
    fn transforms() {
        let m = Mat4::translate(V3::new(1.0, 2.0, 3.0)) * Mat4::scale(V3::splat(2.0));
        assert!(close(m.point(V3::ONE), V3::new(3.0, 4.0, 5.0)));
        assert!(close(m.dir(V3::ONE), V3::splat(2.0)));
        assert!(close(Mat4::rot_z(std::f32::consts::FRAC_PI_2).point(V3::X), V3::Y));
        assert!(close(Mat4::rot_y(std::f32::consts::FRAC_PI_2).point(V3::Z), V3::X));
    }

    #[test]
    fn inverse() {
        let m = Mat4::translate(V3::new(1.0, -2.0, 3.0)) * Mat4::rot_x(0.7) * Mat4::scale(V3::new(1.0, 2.0, 3.0));
        let i = m.inv().unwrap();
        let p = V3::new(0.3, 0.4, 0.5);
        assert!(close(i.point(m.point(p)), p));
        assert!(Mat4::scale(V3::new(1.0, 0.0, 1.0)).inv().is_none());
        assert!(near(Mat4::scale(V3::new(2.0, 3.0, 4.0)).det(), 24.0));
        assert_eq!(Mat4::from_rows(m.rows()), m);
    }

    #[test]
    fn projection() {
        let p = Mat4::persp_lh(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 10.0);
        assert!(near(p.proj(V3::new(0.0, 0.0, 1.0)).z, 0.0));
        assert!(near(p.proj(V3::new(0.0, 0.0, 10.0)).z, 1.0));
        let v = Mat4::look_lh(V3::new(0.0, 0.0, -5.0), V3::ZERO, V3::Y);
        assert!(close(v.point(V3::ZERO), V3::new(0.0, 0.0, 5.0)));
        let r = Mat4::look_rh(V3::new(0.0, 0.0, 5.0), V3::ZERO, V3::Y);
        assert!(close(r.point(V3::ZERO), V3::new(0.0, 0.0, -5.0)));
    }
}
