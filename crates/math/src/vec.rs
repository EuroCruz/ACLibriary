use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

macro_rules! vecn {
    ($n:ident, $c:expr, $($f:ident),+) => {
        #[derive(Clone, Copy, PartialEq, Debug, Default)]
        pub struct $n {
            $(pub $f: f32),+
        }

        impl $n {
            pub const ZERO: $n = $n { $($f: 0.0),+ };
            pub const ONE: $n = $n { $($f: 1.0),+ };

            pub const fn new($($f: f32),+) -> $n {
                $n { $($f),+ }
            }

            pub const fn splat(v: f32) -> $n {
                $n { $($f: v),+ }
            }

            pub fn arr(self) -> [f32; $c] {
                [$(self.$f),+]
            }

            pub fn from_arr(a: [f32; $c]) -> $n {
                let [$($f),+] = a;
                $n { $($f),+ }
            }

            pub fn dot(self, o: $n) -> f32 {
                0.0 $(+ self.$f * o.$f)+
            }

            pub fn len2(self) -> f32 {
                self.dot(self)
            }

            pub fn len(self) -> f32 {
                self.len2().sqrt()
            }

            pub fn dist(self, o: $n) -> f32 {
                (self - o).len()
            }

            pub fn norm(self) -> $n {
                let l = self.len();
                if l > 1e-12 { self / l } else { $n::ZERO }
            }

            pub fn lerp(self, o: $n, t: f32) -> $n {
                self + (o - self) * t
            }

            pub fn min(self, o: $n) -> $n {
                $n { $($f: self.$f.min(o.$f)),+ }
            }

            pub fn max(self, o: $n) -> $n {
                $n { $($f: self.$f.max(o.$f)),+ }
            }

            pub fn abs(self) -> $n {
                $n { $($f: self.$f.abs()),+ }
            }

            pub fn clamp(self, lo: $n, hi: $n) -> $n {
                self.max(lo).min(hi)
            }

            pub fn map(self, f: impl Fn(f32) -> f32) -> $n {
                $n { $($f: f(self.$f)),+ }
            }

            pub fn cmul(self, o: $n) -> $n {
                $n { $($f: self.$f * o.$f),+ }
            }
        }

        impl Add for $n {
            type Output = $n;
            fn add(self, o: $n) -> $n {
                $n { $($f: self.$f + o.$f),+ }
            }
        }

        impl Sub for $n {
            type Output = $n;
            fn sub(self, o: $n) -> $n {
                $n { $($f: self.$f - o.$f),+ }
            }
        }

        impl Mul<f32> for $n {
            type Output = $n;
            fn mul(self, o: f32) -> $n {
                $n { $($f: self.$f * o),+ }
            }
        }

        impl Mul<$n> for f32 {
            type Output = $n;
            fn mul(self, o: $n) -> $n {
                o * self
            }
        }

        impl Div<f32> for $n {
            type Output = $n;
            fn div(self, o: f32) -> $n {
                $n { $($f: self.$f / o),+ }
            }
        }

        impl Neg for $n {
            type Output = $n;
            fn neg(self) -> $n {
                $n { $($f: -self.$f),+ }
            }
        }

        impl AddAssign for $n {
            fn add_assign(&mut self, o: $n) {
                *self = *self + o;
            }
        }

        impl SubAssign for $n {
            fn sub_assign(&mut self, o: $n) {
                *self = *self - o;
            }
        }

        impl MulAssign<f32> for $n {
            fn mul_assign(&mut self, o: f32) {
                *self = *self * o;
            }
        }
    };
}

vecn!(V2, 2, x, y);
vecn!(V3, 3, x, y, z);
vecn!(V4, 4, x, y, z, w);

impl V2 {
    pub fn perp(self) -> V2 {
        V2::new(-self.y, self.x)
    }

    pub fn cross(self, o: V2) -> f32 {
        self.x * o.y - self.y * o.x
    }

    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    pub fn from_angle(a: f32) -> V2 {
        V2::new(a.cos(), a.sin())
    }

    pub fn rot(self, a: f32) -> V2 {
        let (s, c) = a.sin_cos();
        V2::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }
}

impl V3 {
    pub const X: V3 = V3::new(1.0, 0.0, 0.0);
    pub const Y: V3 = V3::new(0.0, 1.0, 0.0);
    pub const Z: V3 = V3::new(0.0, 0.0, 1.0);

    pub fn cross(self, o: V3) -> V3 {
        V3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn xy(self) -> V2 {
        V2::new(self.x, self.y)
    }

    pub fn ext(self, w: f32) -> V4 {
        V4::new(self.x, self.y, self.z, w)
    }

    pub fn reflect(self, n: V3) -> V3 {
        self - n * (2.0 * self.dot(n))
    }

    pub fn project(self, onto: V3) -> V3 {
        let d = onto.len2();
        if d > 1e-12 { onto * (self.dot(onto) / d) } else { V3::ZERO }
    }
}

impl V4 {
    pub fn xyz(self) -> V3 {
        V3::new(self.x, self.y, self.z)
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn ops() {
        let a = V3::new(1.0, 2.0, 3.0);
        assert_eq!(a + a, a * 2.0);
        assert_eq!(a.dot(V3::X), 1.0);
        assert_eq!(V3::X.cross(V3::Y), V3::Z);
        assert_eq!(V3::new(3.0, 4.0, 0.0).len(), 5.0);
        assert_eq!(V3::ZERO.norm(), V3::ZERO);
        assert_eq!(V3::from_arr(a.arr()), a);
        assert_eq!(a.lerp(V3::ZERO, 1.0), V3::ZERO);
        assert_eq!(V3::new(1.0, -1.0, 0.0).reflect(V3::Y), V3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn two_d() {
        let v = V2::new(1.0, 0.0).rot(std::f32::consts::FRAC_PI_2);
        assert!((v.x).abs() < 1e-6 && (v.y - 1.0).abs() < 1e-6);
        assert_eq!(V2::new(1.0, 0.0).cross(V2::new(0.0, 1.0)), 1.0);
    }
}
