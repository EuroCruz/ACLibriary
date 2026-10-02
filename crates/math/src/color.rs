#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Rgba {
    pub const WHITE: Rgba = Rgba::new(255, 255, 255, 255);
    pub const BLACK: Rgba = Rgba::new(0, 0, 0, 255);
    pub const CLEAR: Rgba = Rgba::new(0, 0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba { r, g, b, a }
    }

    pub const fn from_argb(c: u32) -> Rgba {
        Rgba::new((c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8)
    }

    pub const fn argb(self) -> u32 {
        (self.a as u32) << 24 | (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }

    pub const fn from_abgr(c: u32) -> Rgba {
        Rgba::new(c as u8, (c >> 8) as u8, (c >> 16) as u8, (c >> 24) as u8)
    }

    pub const fn abgr(self) -> u32 {
        (self.a as u32) << 24 | (self.b as u32) << 16 | (self.g as u32) << 8 | self.r as u32
    }

    pub fn parse(s: &str) -> Option<Rgba> {
        let s = s.trim().trim_start_matches('#');
        let n = u32::from_str_radix(s, 16).ok()?;
        match s.len() {
            6 => Some(Rgba::new((n >> 16) as u8, (n >> 8) as u8, n as u8, 255)),
            8 => Some(Rgba::new((n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8, n as u8)),
            _ => None,
        }
    }

    pub fn f32(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a].map(|c| c as f32 / 255.0)
    }

    pub fn from_f32(c: [f32; 4]) -> Rgba {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Rgba::new(q(c[0]), q(c[1]), q(c[2]), q(c[3]))
    }

    pub fn alpha(self, a: u8) -> Rgba {
        Rgba { a, ..self }
    }

    pub fn fade(self, k: f32) -> Rgba {
        self.alpha((self.a as f32 * k.clamp(0.0, 1.0) + 0.5) as u8)
    }

    pub fn lerp(self, o: Rgba, t: f32) -> Rgba {
        let (a, b) = (self.f32(), o.f32());
        Rgba::from_f32([0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t))
    }

    pub fn hsv(self) -> Hsv {
        let [r, g, b, _] = self.f32();
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        let d = mx - mn;
        let h = if d == 0.0 {
            0.0
        } else if mx == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if mx == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        Hsv { h: h * 60.0, s: if mx == 0.0 { 0.0 } else { d / mx }, v: mx }
    }
}

impl Hsv {
    pub fn rgba(self, a: u8) -> Rgba {
        let c = self.v * self.s;
        let h = self.h.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = self.v - c;
        Rgba::from_f32([r + m, g + m, b + m, a as f32 / 255.0])
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn packing() {
        let c = Rgba::new(1, 2, 3, 4);
        assert_eq!(c.argb(), 0x0401_0203);
        assert_eq!(Rgba::from_argb(c.argb()), c);
        assert_eq!(Rgba::from_abgr(c.abgr()), c);
        assert_eq!(Rgba::parse("#ff8000"), Some(Rgba::new(255, 128, 0, 255)));
        assert_eq!(Rgba::parse("ff800080"), Some(Rgba::new(255, 128, 0, 128)));
        assert_eq!(Rgba::parse("xyz"), None);
        assert_eq!(Rgba::WHITE.lerp(Rgba::BLACK, 1.0), Rgba::BLACK);
        assert_eq!(Rgba::WHITE.fade(0.0).a, 0);
    }

    #[test]
    fn hsv_roundtrip() {
        for c in [Rgba::new(255, 0, 0, 255), Rgba::new(10, 200, 90, 255), Rgba::new(120, 120, 120, 255)] {
            assert_eq!(c.hsv().rgba(255), c);
        }
        assert_eq!(Rgba::new(255, 0, 0, 255).hsv().h, 0.0);
    }
}
