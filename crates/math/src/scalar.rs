pub const PI: f32 = std::f32::consts::PI;
pub const TAU: f32 = std::f32::consts::TAU;
pub const EPS: f32 = 1e-6;

pub fn rad(d: f32) -> f32 {
    d.to_radians()
}

pub fn deg(r: f32) -> f32 {
    r.to_degrees()
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn unlerp(a: f32, b: f32, v: f32) -> f32 {
    if (b - a).abs() < EPS { 0.0 } else { (v - a) / (b - a) }
}

pub fn remap(v: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    lerp(c, d, unlerp(a, b, v))
}

pub fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

pub fn sat(v: f32) -> f32 {
    clamp(v, 0.0, 1.0)
}

pub fn smooth(a: f32, b: f32, v: f32) -> f32 {
    let t = sat(unlerp(a, b, v));
    t * t * (3.0 - 2.0 * t)
}

pub fn wrap(v: f32, lo: f32, hi: f32) -> f32 {
    lo + (v - lo).rem_euclid(hi - lo)
}

pub fn wrap_angle(a: f32) -> f32 {
    wrap(a, -PI, PI)
}

pub fn angle_diff(a: f32, b: f32) -> f32 {
    wrap_angle(b - a)
}

pub fn approach(v: f32, target: f32, step: f32) -> f32 {
    if (target - v).abs() <= step { target } else { v + step.copysign(target - v) }
}

pub fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= EPS * a.abs().max(b.abs()).max(1.0)
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn basics() {
        assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
        assert_eq!(remap(5.0, 0.0, 10.0, 100.0, 200.0), 150.0);
        assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
        assert_eq!(smooth(0.0, 1.0, 0.5), 0.5);
        assert!(near(wrap_angle(3.0 * PI), PI.copysign(-1.0)) || near(wrap_angle(3.0 * PI), PI));
        assert!(near(angle_diff(rad(350.0), rad(10.0)), rad(20.0)));
        assert_eq!(approach(0.0, 1.0, 0.25), 0.25);
        assert_eq!(approach(0.9, 1.0, 0.25), 1.0);
    }
}
