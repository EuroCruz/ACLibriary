use super::{Atlas, Batch};
use crate::d3d::{method, release, Obj};
use std::ffi::c_void;
use std::ptr::null_mut;

#[repr(C)]
#[derive(Clone, Copy)]
struct V {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
    c: u32,
    u: f32,
    v: f32,
}

const RS: [(u32, u32); 17] = [(7, 0), (8, 3), (14, 0), (15, 0), (19, 5), (20, 6), (22, 1), (27, 1), (28, 0), (52, 0), (136, 1), (137, 0), (168, 15), (171, 1), (174, 0), (194, 0), (206, 0)];
const TS: [(u32, u32, u32); 10] = [(0, 1, 4), (0, 2, 2), (0, 3, 0), (0, 4, 4), (0, 5, 2), (0, 6, 0), (0, 11, 0), (0, 24, 0), (1, 1, 1), (1, 4, 1)];
const SS: [(u32, u32); 5] = [(1, 3), (2, 3), (5, 1), (6, 1), (7, 0)];

pub struct D9 {
    dev: Obj,
    tex: Obj,
    sb: Obj,
    buf: Vec<V>,
}

impl D9 {
    pub fn new(dev: Obj, a: &Atlas) -> Option<D9> {
        unsafe {
            let mut tex = null_mut();
            let ct: unsafe extern "system" fn(Obj, u32, u32, u32, u32, u32, u32, *mut Obj, *mut c_void) -> i32 = method(dev, 23);
            if ct(dev, a.w, a.h, 1, 0, 21, 1, &mut tex, null_mut()) < 0 || tex.is_null() {
                return None;
            }
            let mut lr: (i32, *mut u8) = (0, null_mut());
            if method::<unsafe extern "system" fn(Obj, u32, *mut (i32, *mut u8), *const c_void, u32) -> i32>(tex, 19)(tex, 0, &mut lr, std::ptr::null(), 0) < 0 {
                release(tex);
                return None;
            }
            for y in 0..a.h as usize {
                std::ptr::copy_nonoverlapping(a.px.as_ptr().add(y * a.w as usize), lr.1.add(y * lr.0 as usize) as *mut u32, a.w as usize);
            }
            method::<unsafe extern "system" fn(Obj, u32) -> i32>(tex, 20)(tex, 0);
            Some(D9 { dev, tex, sb: null_mut(), buf: Vec::new() })
        }
    }

    pub fn reset(&mut self) {
        unsafe { release(std::mem::replace(&mut self.sb, null_mut())) };
    }

    pub fn draw(&mut self, b: &Batch) {
        if b.v.is_empty() {
            return;
        }
        let d = self.dev;
        unsafe {
            if self.sb.is_null() && method::<unsafe extern "system" fn(Obj, u32, *mut Obj) -> i32>(d, 59)(d, 1, &mut self.sb) < 0 {
                self.sb = null_mut();
            }
            if !self.sb.is_null() {
                method::<unsafe extern "system" fn(Obj) -> i32>(self.sb, 4)(self.sb);
            }
            let rs: unsafe extern "system" fn(Obj, u32, u32) -> i32 = method(d, 57);
            let ts: unsafe extern "system" fn(Obj, u32, u32, u32) -> i32 = method(d, 67);
            let ss: unsafe extern "system" fn(Obj, u32, u32, u32) -> i32 = method(d, 69);
            let ptr: unsafe extern "system" fn(Obj, Obj) -> i32 = method(d, 92);
            RS.iter().for_each(|&(k, v)| {
                rs(d, k, v);
            });
            TS.iter().for_each(|&(s, k, v)| {
                ts(d, s, k, v);
            });
            SS.iter().for_each(|&(k, v)| {
                ss(d, 0, k, v);
            });
            ptr(d, null_mut());
            method::<unsafe extern "system" fn(Obj, Obj) -> i32>(d, 107)(d, null_mut());
            method::<unsafe extern "system" fn(Obj, u32) -> i32>(d, 89)(d, 0x144);
            method::<unsafe extern "system" fn(Obj, u32, Obj) -> i32>(d, 65)(d, 0, self.tex);
            self.buf.clear();
            self.buf.extend(b.v.iter().map(|p| V { x: p.x - 0.5, y: p.y - 0.5, z: 0.0, w: 1.0, c: p.c, u: p.u, v: p.v }));
            let up: unsafe extern "system" fn(Obj, u32, u32, *const c_void, u32) -> i32 = method(d, 83);
            for ch in self.buf.chunks(3 * 0x4000) {
                up(d, 4, (ch.len() / 3) as u32, ch.as_ptr().cast(), std::mem::size_of::<V>() as u32);
            }
            if !self.sb.is_null() {
                method::<unsafe extern "system" fn(Obj) -> i32>(self.sb, 5)(self.sb);
            }
        }
    }
}

impl Drop for D9 {
    fn drop(&mut self) {
        unsafe {
            release(self.sb);
            release(self.tex);
        }
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::d3d::{Api, Dev};
    use crate::draw::rgba;

    unsafe fn pixels(d: Obj, w: u32, h: u32) -> Vec<u32> {
        let mut bb = null_mut();
        method::<unsafe extern "system" fn(Obj, u32, u32, u32, *mut Obj) -> i32>(d, 18)(d, 0, 0, 0, &mut bb);
        let mut desc = [0u32; 8];
        method::<unsafe extern "system" fn(Obj, *mut u32) -> i32>(bb, 12)(bb, desc.as_mut_ptr());
        let mut off = null_mut();
        method::<unsafe extern "system" fn(Obj, u32, u32, u32, u32, *mut Obj, *mut c_void) -> i32>(d, 36)(d, w, h, desc[0], 2, &mut off, null_mut());
        assert!(method::<unsafe extern "system" fn(Obj, Obj, Obj) -> i32>(d, 32)(d, bb, off) >= 0);
        let mut lr: (i32, *mut u8) = (0, null_mut());
        method::<unsafe extern "system" fn(Obj, *mut (i32, *mut u8), *const c_void, u32) -> i32>(off, 13)(off, &mut lr, std::ptr::null(), 0);
        let v = (0..h as usize).flat_map(|y| (0..w as usize).map(move |x| (x, y))).map(|(x, y)| *(lr.1.add(y * lr.0 as usize + x * 4) as *const u32) & 0xffffff).collect();
        method::<unsafe extern "system" fn(Obj) -> i32>(off, 14)(off);
        release(off);
        release(bb);
        v
    }

    #[test]
    fn renders() {
        let dev = Dev::new(Api::D9, 64, 64).expect("d3d9");
        let a = Atlas::new("Arial", 14, true).unwrap();
        let mut r = D9::new(dev.device, &a).expect("renderer");
        let mut b = Batch::new(&a);
        b.rect(8.0, 8.0, 16.0, 16.0, rgba(255, 0, 0, 255));
        b.rect(30.0, 8.0, 16.0, 16.0, rgba(0, 0, 255, 128));
        b.text(&a, 4.0, 36.0, "WWW", rgba(0, 255, 0, 255));
        let d = dev.device;
        unsafe {
            method::<unsafe extern "system" fn(Obj, u32, *const c_void, u32, u32, f32, u32) -> i32>(d, 43)(d, 0, std::ptr::null(), 1, 0xff000000, 1.0, 0);
            method::<unsafe extern "system" fn(Obj) -> i32>(d, 41)(d);
            r.draw(&b);
            method::<unsafe extern "system" fn(Obj) -> i32>(d, 42)(d);
            if !dev.hw {
                eprintln!("no hardware device, pixels not checked");
                return;
            }
            let p = pixels(d, 64, 64);
            assert_eq!(p[16 * 64 + 16], 0xff0000);
            assert_eq!(p[4 * 64 + 4], 0);
            let half = p[16 * 64 + 38];
            assert!((0x70..=0x90).contains(&(half & 255)) && half >> 8 == 0, "{half:x}");
            assert!((36..36 + a.line as usize).any(|y| (4..60).any(|x| p[y * 64 + x] == 0x00ff00)));
        }
        r.reset();
        r.draw(&b);
    }
}
