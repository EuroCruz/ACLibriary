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

const RS: [(u32, u32); 14] = [(7, 0), (8, 3), (14, 0), (15, 0), (19, 5), (20, 6), (22, 1), (27, 1), (28, 0), (52, 0), (136, 1), (137, 0), (168, 15), (171, 1)];
const TS: [(u32, u32, u32); 15] = [(0, 1, 4), (0, 2, 2), (0, 3, 0), (0, 4, 4), (0, 5, 2), (0, 6, 0), (0, 11, 0), (0, 24, 0), (0, 13, 3), (0, 14, 3), (0, 16, 1), (0, 17, 1), (0, 18, 0), (1, 1, 1), (1, 4, 1)];

pub struct D8 {
    dev: Obj,
    tex: Obj,
    sb: u32,
    buf: Vec<V>,
}

impl D8 {
    pub fn new(dev: Obj, a: &Atlas) -> Option<D8> {
        unsafe {
            let mut tex = null_mut();
            let ct: unsafe extern "system" fn(Obj, u32, u32, u32, u32, u32, u32, *mut Obj) -> i32 = method(dev, 20);
            if ct(dev, a.w, a.h, 1, 0, 21, 1, &mut tex) < 0 || tex.is_null() {
                return None;
            }
            let mut lr: (i32, *mut u8) = (0, null_mut());
            if method::<unsafe extern "system" fn(Obj, u32, *mut (i32, *mut u8), *const c_void, u32) -> i32>(tex, 16)(tex, 0, &mut lr, std::ptr::null(), 0) < 0 {
                release(tex);
                return None;
            }
            for y in 0..a.h as usize {
                std::ptr::copy_nonoverlapping(a.px.as_ptr().add(y * a.w as usize), lr.1.add(y * lr.0 as usize) as *mut u32, a.w as usize);
            }
            method::<unsafe extern "system" fn(Obj, u32) -> i32>(tex, 17)(tex, 0);
            Some(D8 { dev, tex, sb: 0, buf: Vec::new() })
        }
    }

    pub fn reset(&mut self) {
        if self.sb != 0 {
            unsafe { method::<unsafe extern "system" fn(Obj, u32) -> i32>(self.dev, 56)(self.dev, self.sb) };
            self.sb = 0;
        }
    }

    pub fn draw(&mut self, b: &Batch) {
        if b.v.is_empty() {
            return;
        }
        let d = self.dev;
        unsafe {
            if self.sb == 0 && method::<unsafe extern "system" fn(Obj, u32, *mut u32) -> i32>(d, 57)(d, 1, &mut self.sb) < 0 {
                self.sb = 0;
            }
            if self.sb != 0 {
                method::<unsafe extern "system" fn(Obj, u32) -> i32>(d, 55)(d, self.sb);
            }
            let rs: unsafe extern "system" fn(Obj, u32, u32) -> i32 = method(d, 50);
            let ts: unsafe extern "system" fn(Obj, u32, u32, u32) -> i32 = method(d, 63);
            RS.iter().for_each(|&(k, v)| {
                rs(d, k, v);
            });
            TS.iter().for_each(|&(s, k, v)| {
                ts(d, s, k, v);
            });
            method::<unsafe extern "system" fn(Obj, u32) -> i32>(d, 88)(d, 0);
            method::<unsafe extern "system" fn(Obj, u32) -> i32>(d, 76)(d, 0x144);
            method::<unsafe extern "system" fn(Obj, u32, Obj) -> i32>(d, 61)(d, 0, self.tex);
            self.buf.clear();
            self.buf.extend(b.v.iter().map(|p| V { x: p.x - 0.5, y: p.y - 0.5, z: 0.0, w: 1.0, c: p.c, u: p.u, v: p.v }));
            let up: unsafe extern "system" fn(Obj, u32, u32, *const c_void, u32) -> i32 = method(d, 72);
            for ch in self.buf.chunks(3 * 0x4000) {
                up(d, 4, (ch.len() / 3) as u32, ch.as_ptr().cast(), std::mem::size_of::<V>() as u32);
            }
            if self.sb != 0 {
                method::<unsafe extern "system" fn(Obj, u32) -> i32>(d, 54)(d, self.sb);
            }
        }
    }
}

impl Drop for D8 {
    fn drop(&mut self) {
        self.reset();
        unsafe { release(self.tex) };
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::d3d::{Api, Dev};
    use crate::draw::rgba;

    #[test]
    fn renders() {
        let Some(dev) = Dev::new(Api::D8, 64, 64) else {
            assert!(cfg!(target_pointer_width = "64"));
            return;
        };
        let a = Atlas::new("Arial", 14, true).unwrap();
        let mut r = D8::new(dev.device, &a).expect("renderer");
        let mut b = Batch::new(&a);
        b.rect(8.0, 8.0, 16.0, 16.0, rgba(255, 0, 0, 255));
        b.rect(30.0, 8.0, 16.0, 16.0, rgba(0, 0, 255, 128));
        b.text(&a, 4.0, 36.0, "WWW", rgba(0, 255, 0, 255));
        let d = dev.device;
        unsafe {
            method::<unsafe extern "system" fn(Obj, u32, *const c_void, u32, u32, f32, u32) -> i32>(d, 36)(d, 0, std::ptr::null(), 1, 0xff000000, 1.0, 0);
            method::<unsafe extern "system" fn(Obj) -> i32>(d, 34)(d);
            r.draw(&b);
            method::<unsafe extern "system" fn(Obj) -> i32>(d, 35)(d);
            if !dev.hw {
                eprintln!("no hardware device, pixels not checked");
                return;
            }
            let mut bb = null_mut();
            method::<unsafe extern "system" fn(Obj, u32, u32, *mut Obj) -> i32>(d, 16)(d, 0, 0, &mut bb);
            let mut lr: (i32, *mut u8) = (0, null_mut());
            assert!(method::<unsafe extern "system" fn(Obj, *mut (i32, *mut u8), *const c_void, u32) -> i32>(bb, 9)(bb, &mut lr, std::ptr::null(), 0x10) >= 0);
            let px = |x: usize, y: usize| *(lr.1.add(y * lr.0 as usize + x * 4) as *const u32) & 0xffffff;
            assert_eq!(px(16, 16), 0xff0000);
            assert_eq!(px(4, 4), 0);
            let half = px(38, 16);
            assert!((0x70..=0x90).contains(&(half & 255)) && half >> 8 == 0, "{half:x}");
            assert!((36..36 + a.line as usize).any(|y| (4..60).any(|x| px(x, y) == 0x00ff00)));
            method::<unsafe extern "system" fn(Obj) -> i32>(bb, 10)(bb);
            release(bb);
        }
        r.reset();
    }
}
