use crate::win::{self, cstr, CreateWindowExA, DestroyWindow, HWND};
use std::ffi::c_void;
use std::ptr::null_mut;

pub type Obj = *mut c_void;

pub mod d8 {
    pub const RESET: usize = 14;
    pub const PRESENT: usize = 15;
    pub const BEGIN_SCENE: usize = 34;
    pub const END_SCENE: usize = 35;
    pub const DEVICE: usize = 97;
}

pub mod d9 {
    pub const RESET: usize = 16;
    pub const PRESENT: usize = 17;
    pub const BEGIN_SCENE: usize = 41;
    pub const END_SCENE: usize = 42;
    pub const DEVICE: usize = 119;
}

pub mod dxgi {
    pub const PRESENT: usize = 8;
    pub const RESIZE_BUFFERS: usize = 13;
    pub const RESIZE_TARGET: usize = 14;
    pub const SWAP: usize = 18;
}

pub mod d11 {
    pub const DRAW_INDEXED: usize = 12;
    pub const DRAW: usize = 13;
    pub const OM_SET_RENDER_TARGETS: usize = 33;
    pub const CLEAR_RENDER_TARGET_VIEW: usize = 50;
    pub const DEVICE: usize = 43;
    pub const CONTEXT: usize = 115;
}

pub unsafe fn method<F: Copy>(o: Obj, i: usize) -> F {
    let vt = *(o as *const *const usize);
    std::mem::transmute_copy(&*vt.add(i))
}

pub unsafe fn release(o: Obj) {
    if !o.is_null() {
        method::<unsafe extern "system" fn(Obj) -> u32>(o, 2)(o);
    }
}

pub fn methods(o: Obj, n: usize) -> Vec<usize> {
    let vt = crate::mem::read::<usize>(o as usize).unwrap_or(0);
    (0..n).map(|i| crate::mem::read::<usize>(vt + i * std::mem::size_of::<usize>()).unwrap_or(0)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    D8,
    D9,
    D11,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub device: Vec<usize>,
    pub swap: Vec<usize>,
    pub context: Vec<usize>,
}

impl Table {
    pub fn present(&self, api: Api) -> usize {
        match api {
            Api::D8 => self.device[d8::PRESENT],
            Api::D9 => self.device[d9::PRESENT],
            Api::D11 => self.swap[dxgi::PRESENT],
        }
    }

    pub fn reset(&self, api: Api) -> usize {
        match api {
            Api::D8 => self.device[d8::RESET],
            Api::D9 => self.device[d9::RESET],
            Api::D11 => self.swap[dxgi::RESIZE_BUFFERS],
        }
    }
}

pub struct Dev {
    pub api: Api,
    pub hw: bool,
    pub wnd: HWND,
    pub device: Obj,
    pub swap: Obj,
    pub context: Obj,
    root: Obj,
}

impl Drop for Dev {
    fn drop(&mut self) {
        unsafe {
            for o in [self.context, self.swap, self.device, self.root] {
                release(o);
            }
            DestroyWindow(self.wnd);
        }
    }
}

fn export(dll: &str, name: &str) -> usize {
    let m = win::load(dll);
    if m.is_null() { 0 } else { win::proc_addr(m, name) }
}

#[repr(C)]
#[derive(Default)]
struct Pp8 {
    w: u32,
    h: u32,
    fmt: u32,
    count: u32,
    ms: u32,
    swap: u32,
    wnd: usize,
    windowed: i32,
    depth: i32,
    depth_fmt: u32,
    flags: u32,
    hz: u32,
    interval: u32,
}

#[repr(C)]
#[derive(Default)]
struct Pp9 {
    w: u32,
    h: u32,
    fmt: u32,
    count: u32,
    ms: u32,
    msq: u32,
    swap: u32,
    wnd: usize,
    windowed: i32,
    depth: i32,
    depth_fmt: u32,
    flags: u32,
    hz: u32,
    interval: u32,
}

#[repr(C)]
struct SwapDesc {
    w: u32,
    h: u32,
    num: u32,
    den: u32,
    fmt: u32,
    order: u32,
    scale: u32,
    count: u32,
    quality: u32,
    usage: u32,
    buffers: u32,
    wnd: usize,
    windowed: i32,
    effect: u32,
    flags: u32,
}

type Create = unsafe extern "system" fn(Obj, u32, u32, HWND, u32, *mut c_void, *mut Obj) -> i32;
type Create11 = unsafe extern "system" fn(Obj, u32, Obj, u32, *const u32, u32, u32, *const SwapDesc, *mut Obj, *mut Obj, *mut u32, *mut Obj) -> i32;

impl Dev {
    pub fn new(api: Api, w: u32, h: u32) -> Option<Dev> {
        let wnd = unsafe { CreateWindowExA(0, cstr("STATIC").as_ptr().cast(), cstr("ac_d3d").as_ptr().cast(), 0, 0, 0, w as i32, h as i32, null_mut(), null_mut(), null_mut(), null_mut()) };
        if wnd.is_null() {
            return None;
        }
        let mut d = Dev { api, hw: false, wnd, device: null_mut(), swap: null_mut(), context: null_mut(), root: null_mut() };
        let ok = unsafe {
            match api {
                Api::D8 => d.d8(w, h),
                Api::D9 => d.d9(w, h),
                Api::D11 => d.d11(w, h),
            }
        };
        ok.then_some(d)
    }

    unsafe fn d8(&mut self, w: u32, h: u32) -> bool {
        let f = export("d3d8.dll", "Direct3DCreate8");
        if f == 0 {
            return false;
        }
        self.root = std::mem::transmute::<usize, unsafe extern "system" fn(u32) -> Obj>(f)(220);
        if self.root.is_null() {
            return false;
        }
        let mut mode = [0u32; 4];
        if method::<unsafe extern "system" fn(Obj, u32, *mut u32) -> i32>(self.root, 8)(self.root, 0, mode.as_mut_ptr()) < 0 {
            return false;
        }
        let cd: Create = method(self.root, 15);
        for kind in [1, 2] {
            let mut pp = Pp8 { w, h, fmt: mode[3], count: 1, swap: 1, wnd: self.wnd as usize, windowed: 1, flags: 1, ..Pp8::default() };
            if cd(self.root, 0, kind, self.wnd, 0x20, (&mut pp as *mut Pp8).cast(), &mut self.device) >= 0 && !self.device.is_null() {
                self.hw = kind == 1;
                return true;
            }
        }
        false
    }

    unsafe fn d9(&mut self, w: u32, h: u32) -> bool {
        let f = export("d3d9.dll", "Direct3DCreate9");
        if f == 0 {
            return false;
        }
        self.root = std::mem::transmute::<usize, unsafe extern "system" fn(u32) -> Obj>(f)(32);
        if self.root.is_null() {
            return false;
        }
        let cd: Create = method(self.root, 16);
        for kind in [1, 4, 2] {
            let mut pp = Pp9 { w, h, count: 1, swap: 1, wnd: self.wnd as usize, windowed: 1, ..Pp9::default() };
            if cd(self.root, 0, kind, self.wnd, 0x20, (&mut pp as *mut Pp9).cast(), &mut self.device) >= 0 && !self.device.is_null() {
                self.hw = kind == 1;
                return true;
            }
        }
        false
    }

    unsafe fn d11(&mut self, w: u32, h: u32) -> bool {
        let f = export("d3d11.dll", "D3D11CreateDeviceAndSwapChain");
        if f == 0 {
            return false;
        }
        let c: Create11 = std::mem::transmute(f);
        let desc = SwapDesc { w, h, num: 60, den: 1, fmt: 28, order: 0, scale: 0, count: 1, quality: 0, usage: 0x20, buffers: 1, wnd: self.wnd as usize, windowed: 1, effect: 0, flags: 0 };
        for kind in [1, 5] {
            let mut lvl = 0;
            if c(null_mut(), kind, null_mut(), 0, std::ptr::null(), 0, 7, &desc, &mut self.swap, &mut self.device, &mut lvl, &mut self.context) >= 0 && !self.swap.is_null() {
                self.hw = true;
                return true;
            }
        }
        false
    }

    pub fn table(&self) -> Table {
        match self.api {
            Api::D8 => Table { device: methods(self.device, d8::DEVICE), ..Table::default() },
            Api::D9 => Table { device: methods(self.device, d9::DEVICE), ..Table::default() },
            Api::D11 => Table { device: methods(self.device, d11::DEVICE), swap: methods(self.swap, dxgi::SWAP), context: methods(self.context, d11::CONTEXT) },
        }
    }
}

pub fn table(api: Api) -> Option<Table> {
    let t = Dev::new(api, 64, 64)?.table();
    t.device.iter().chain(&t.swap).chain(&t.context).all(|&a| a != 0).then_some(t)
}

pub fn loaded() -> Vec<Api> {
    [("d3d8.dll", Api::D8), ("d3d9.dll", Api::D9), ("d3d11.dll", Api::D11)].into_iter().filter(|(d, _)| !win::module(d).is_null()).map(|x| x.1).collect()
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::mem::Module;

    fn inside(m: &str, a: usize) -> bool {
        Module::named(m).is_some_and(|m| a >= m.base && a < m.base + m.size())
    }

    #[test]
    fn tables() {
        let t9 = table(Api::D9).expect("d3d9");
        assert_eq!(t9.device.len(), d9::DEVICE);
        assert!(inside("d3d9.dll", t9.present(Api::D9)) && inside("d3d9.dll", t9.device[d9::END_SCENE]));
        assert_ne!(t9.present(Api::D9), t9.reset(Api::D9));
        let t11 = table(Api::D11).expect("d3d11");
        assert_eq!((t11.device.len(), t11.swap.len(), t11.context.len()), (d11::DEVICE, dxgi::SWAP, d11::CONTEXT));
        assert!(inside("dxgi.dll", t11.present(Api::D11)) && inside("dxgi.dll", t11.reset(Api::D11)));
        assert!(inside("d3d11.dll", t11.context[d11::DRAW_INDEXED]));
        if cfg!(target_pointer_width = "32") {
            let t8 = table(Api::D8).expect("d3d8");
            assert_eq!(t8.device.len(), d8::DEVICE);
            assert!(inside("d3d8.dll", t8.present(Api::D8)) && inside("d3d8.dll", t8.device[d8::END_SCENE]));
            assert_ne!(t8.present(Api::D8), t8.reset(Api::D8));
        }
        assert!(loaded().contains(&Api::D9) && loaded().contains(&Api::D11));
        assert_eq!(table(Api::D9).unwrap().present(Api::D9), t9.present(Api::D9));
    }
}
