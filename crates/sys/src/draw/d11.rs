use super::{Atlas, Batch};
use crate::d3d::{method, release, Obj};
use std::ffi::c_void;
use std::ptr::{null, null_mut};

#[path = "dxbc.rs"]
mod dxbc;

pub const HLSL: &str = "struct I{float2 p:POSITION;float2 t:TEXCOORD;float4 c:COLOR;};\
struct O{float4 p:SV_Position;float2 t:TEXCOORD;float4 c:COLOR;};\
O vs(I i){O o;o.p=float4(i.p,0,1);o.t=i.t;o.c=i.c;return o;}\
Texture2D T:register(t0);SamplerState S:register(s0);\
float4 ps(O i):SV_Target{return T.Sample(S,i.t)*i.c;}";

const IID_DEVICE: [u8; 16] = guid(0xdb6f6ddb, 0xac77, 0x4e88, [0x82, 0x53, 0x81, 0x9d, 0xf9, 0xbb, 0xf1, 0x40]);
const IID_TEX2D: [u8; 16] = guid(0x6f15aaf2, 0xd208, 0x4e89, [0x9a, 0xb4, 0x48, 0x95, 0x35, 0xd3, 0x4f, 0x9c]);

const fn guid(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
    let (a, b, c) = (a.to_le_bytes(), b.to_le_bytes(), c.to_le_bytes());
    [a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]
}

#[repr(C)]
#[derive(Clone, Copy)]
struct V {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    c: u32,
}

#[repr(C)]
struct Elem {
    name: *const u8,
    index: u32,
    fmt: u32,
    slot: u32,
    off: u32,
    class: u32,
    step: u32,
}

unsafe fn get<T: Copy>(o: Obj, i: usize) -> T {
    method::<T>(o, i)
}

struct Saved {
    rtv: [Obj; 8],
    dsv: Obj,
    vp: [[f32; 6]; 16],
    nvp: u32,
    layout: Obj,
    topo: u32,
    vb: Obj,
    stride: u32,
    off: u32,
    vs: Obj,
    ps: Obj,
    gs: Obj,
    hs: Obj,
    ds: Obj,
    srv: Obj,
    smp: Obj,
    blend: Obj,
    factor: [f32; 4],
    mask: u32,
    depth: Obj,
    sref: u32,
    rs: Obj,
}

pub struct D11 {
    dev: Obj,
    ctx: Obj,
    swap: Obj,
    rtv: Obj,
    size: (f32, f32),
    srv: Obj,
    vs: Obj,
    ps: Obj,
    layout: Obj,
    blend: Obj,
    depth: Obj,
    rs: Obj,
    smp: Obj,
    vb: Obj,
    cap: usize,
    buf: Vec<V>,
}

impl D11 {
    pub fn from_swap(swap: Obj, a: &Atlas) -> Option<D11> {
        let mut dev = null_mut();
        unsafe {
            if get::<unsafe extern "system" fn(Obj, *const [u8; 16], *mut Obj) -> i32>(swap, 7)(swap, &IID_DEVICE, &mut dev) < 0 || dev.is_null() {
                return None;
            }
            let mut ctx = null_mut();
            get::<unsafe extern "system" fn(Obj, *mut Obj)>(dev, 40)(dev, &mut ctx);
            let r = D11::new(dev, ctx, a);
            release(ctx);
            release(dev);
            r.map(|mut r| {
                r.swap = swap;
                r
            })
        }
    }

    pub fn new(dev: Obj, ctx: Obj, a: &Atlas) -> Option<D11> {
        unsafe {
            get::<unsafe extern "system" fn(Obj) -> u32>(dev, 1)(dev);
            get::<unsafe extern "system" fn(Obj) -> u32>(ctx, 1)(ctx);
            let mut r = D11 { dev, ctx, swap: null_mut(), rtv: null_mut(), size: (0.0, 0.0), srv: null_mut(), vs: null_mut(), ps: null_mut(), layout: null_mut(), blend: null_mut(), depth: null_mut(), rs: null_mut(), smp: null_mut(), vb: null_mut(), cap: 0, buf: Vec::new() };
            r.init(a).then_some(r)
        }
    }

    unsafe fn init(&mut self, a: &Atlas) -> bool {
        let d = self.dev;
        let px: Vec<u32> = a.px.iter().map(|&p| p & 0xff00ff00 | (p >> 16) & 255 | (p & 255) << 16).collect();
        let desc = [a.w, a.h, 1, 1, 28, 1, 0, 1, 8, 0, 0];
        let init: (*const c_void, u32, u32) = (px.as_ptr().cast(), a.w * 4, 0);
        let mut tex = null_mut();
        if get::<unsafe extern "system" fn(Obj, *const u32, *const (*const c_void, u32, u32), *mut Obj) -> i32>(d, 5)(d, desc.as_ptr(), &init, &mut tex) < 0 {
            return false;
        }
        let ok = get::<unsafe extern "system" fn(Obj, Obj, *const c_void, *mut Obj) -> i32>(d, 7)(d, tex, null(), &mut self.srv) >= 0;
        release(tex);
        let names = [b"POSITION\0".as_ptr(), b"TEXCOORD\0".as_ptr(), b"COLOR\0".as_ptr()];
        let el = [(0, 16, 0), (1, 16, 8), (2, 28, 16)].map(|(n, fmt, off)| Elem { name: names[n], index: 0, fmt, slot: 0, off, class: 0, step: 0 });
        let blend: [u32; 66] = std::array::from_fn(|i| match i {
            2 => 1,
            3 => 5,
            4 => 6,
            5 => 1,
            6 => 2,
            7 => 6,
            8 => 1,
            9 => 15,
            _ => 0,
        });
        let depth: [u32; 13] = [0, 0, 8, 0, 0xffff, 1, 1, 1, 8, 1, 1, 1, 8];
        let rs: [u32; 10] = [3, 1, 0, 0, 0, 0, 1, 0, 0, 0];
        let smp: [u32; 13] = [0, 3, 3, 3, 0, 1, 1, 0, 0, 0, 0, 0, f32::MAX.to_bits()];
        ok && get::<unsafe extern "system" fn(Obj, *const u8, usize, Obj, *mut Obj) -> i32>(d, 12)(d, dxbc::VS.as_ptr(), dxbc::VS.len(), null_mut(), &mut self.vs) >= 0
            && get::<unsafe extern "system" fn(Obj, *const u8, usize, Obj, *mut Obj) -> i32>(d, 15)(d, dxbc::PS.as_ptr(), dxbc::PS.len(), null_mut(), &mut self.ps) >= 0
            && get::<unsafe extern "system" fn(Obj, *const Elem, u32, *const u8, usize, *mut Obj) -> i32>(d, 11)(d, el.as_ptr(), 3, dxbc::VS.as_ptr(), dxbc::VS.len(), &mut self.layout) >= 0
            && get::<unsafe extern "system" fn(Obj, *const u32, *mut Obj) -> i32>(d, 20)(d, blend.as_ptr(), &mut self.blend) >= 0
            && get::<unsafe extern "system" fn(Obj, *const u32, *mut Obj) -> i32>(d, 21)(d, depth.as_ptr(), &mut self.depth) >= 0
            && get::<unsafe extern "system" fn(Obj, *const u32, *mut Obj) -> i32>(d, 22)(d, rs.as_ptr(), &mut self.rs) >= 0
            && get::<unsafe extern "system" fn(Obj, *const u32, *mut Obj) -> i32>(d, 23)(d, smp.as_ptr(), &mut self.smp) >= 0
    }

    pub fn reset(&mut self) {
        unsafe { release(std::mem::replace(&mut self.rtv, null_mut())) };
    }

    unsafe fn target(&mut self) -> bool {
        if !self.rtv.is_null() {
            return true;
        }
        if self.swap.is_null() {
            return false;
        }
        let mut tex = null_mut();
        if get::<unsafe extern "system" fn(Obj, u32, *const [u8; 16], *mut Obj) -> i32>(self.swap, 9)(self.swap, 0, &IID_TEX2D, &mut tex) < 0 {
            return false;
        }
        let mut desc = [0u32; 11];
        get::<unsafe extern "system" fn(Obj, *mut u32)>(tex, 10)(tex, desc.as_mut_ptr());
        let ok = get::<unsafe extern "system" fn(Obj, Obj, *const c_void, *mut Obj) -> i32>(self.dev, 9)(self.dev, tex, null(), &mut self.rtv) >= 0;
        release(tex);
        self.size = (desc[0] as f32, desc[1] as f32);
        ok
    }

    pub fn draw(&mut self, b: &Batch) {
        unsafe {
            if self.target() {
                let (rtv, (w, h)) = (self.rtv, self.size);
                self.draw_to(rtv, w, h, b);
            }
        }
    }

    pub unsafe fn draw_to(&mut self, rtv: Obj, w: f32, h: f32, b: &Batch) {
        if b.v.is_empty() || w <= 0.0 || h <= 0.0 {
            return;
        }
        let (d, c) = (self.dev, self.ctx);
        let n = b.v.len();
        if n > self.cap {
            release(std::mem::replace(&mut self.vb, null_mut()));
            self.cap = n.next_power_of_two().max(1024);
            let desc = [(self.cap * std::mem::size_of::<V>()) as u32, 2, 1, 0x10000, 0, 0];
            if get::<unsafe extern "system" fn(Obj, *const u32, *const c_void, *mut Obj) -> i32>(d, 3)(d, desc.as_ptr(), null(), &mut self.vb) < 0 {
                self.cap = 0;
                return;
            }
        }
        self.buf.clear();
        self.buf.extend(b.v.iter().map(|p| V { x: p.x / w * 2.0 - 1.0, y: 1.0 - p.y / h * 2.0, u: p.u, v: p.v, c: p.c & 0xff00ff00 | (p.c >> 16) & 255 | (p.c & 255) << 16 }));
        let mut m: (*mut c_void, u32, u32) = (null_mut(), 0, 0);
        if get::<unsafe extern "system" fn(Obj, Obj, u32, u32, u32, *mut (*mut c_void, u32, u32)) -> i32>(c, 14)(c, self.vb, 0, 4, 0, &mut m) < 0 {
            return;
        }
        std::ptr::copy_nonoverlapping(self.buf.as_ptr(), m.0 as *mut V, n);
        get::<unsafe extern "system" fn(Obj, Obj, u32)>(c, 15)(c, self.vb, 0);
        let s = self.save();
        let vp = [0.0, 0.0, w, h, 0.0, 1.0f32];
        get::<unsafe extern "system" fn(Obj, u32, *const Obj, Obj)>(c, 33)(c, 1, &rtv, null_mut());
        get::<unsafe extern "system" fn(Obj, u32, *const f32)>(c, 44)(c, 1, vp.as_ptr());
        get::<unsafe extern "system" fn(Obj, Obj)>(c, 17)(c, self.layout);
        get::<unsafe extern "system" fn(Obj, u32)>(c, 24)(c, 4);
        let (stride, off) = (std::mem::size_of::<V>() as u32, 0u32);
        get::<unsafe extern "system" fn(Obj, u32, u32, *const Obj, *const u32, *const u32)>(c, 18)(c, 0, 1, &self.vb, &stride, &off);
        let shader = |i: usize, o: Obj| get::<unsafe extern "system" fn(Obj, Obj, *const Obj, u32)>(c, i)(c, o, null(), 0);
        shader(11, self.vs);
        shader(9, self.ps);
        shader(23, null_mut());
        shader(60, null_mut());
        shader(64, null_mut());
        get::<unsafe extern "system" fn(Obj, u32, u32, *const Obj)>(c, 8)(c, 0, 1, &self.srv);
        get::<unsafe extern "system" fn(Obj, u32, u32, *const Obj)>(c, 10)(c, 0, 1, &self.smp);
        get::<unsafe extern "system" fn(Obj, Obj, *const f32, u32)>(c, 35)(c, self.blend, [0.0f32; 4].as_ptr(), 0xffffffff);
        get::<unsafe extern "system" fn(Obj, Obj, u32)>(c, 36)(c, self.depth, 0);
        get::<unsafe extern "system" fn(Obj, Obj)>(c, 43)(c, self.rs);
        get::<unsafe extern "system" fn(Obj, u32, u32)>(c, 13)(c, n as u32, 0);
        self.restore(s);
    }

    unsafe fn save(&self) -> Saved {
        let c = self.ctx;
        let mut s = Saved { rtv: [null_mut(); 8], dsv: null_mut(), vp: [[0.0; 6]; 16], nvp: 16, layout: null_mut(), topo: 0, vb: null_mut(), stride: 0, off: 0, vs: null_mut(), ps: null_mut(), gs: null_mut(), hs: null_mut(), ds: null_mut(), srv: null_mut(), smp: null_mut(), blend: null_mut(), factor: [0.0; 4], mask: 0, depth: null_mut(), sref: 0, rs: null_mut() };
        get::<unsafe extern "system" fn(Obj, u32, *mut Obj, *mut Obj)>(c, 89)(c, 8, s.rtv.as_mut_ptr(), &mut s.dsv);
        get::<unsafe extern "system" fn(Obj, *mut u32, *mut [f32; 6])>(c, 95)(c, &mut s.nvp, s.vp.as_mut_ptr());
        get::<unsafe extern "system" fn(Obj, *mut Obj)>(c, 78)(c, &mut s.layout);
        get::<unsafe extern "system" fn(Obj, *mut u32)>(c, 83)(c, &mut s.topo);
        get::<unsafe extern "system" fn(Obj, u32, u32, *mut Obj, *mut u32, *mut u32)>(c, 79)(c, 0, 1, &mut s.vb, &mut s.stride, &mut s.off);
        let shader = |i: usize, o: &mut Obj| get::<unsafe extern "system" fn(Obj, *mut Obj, *mut Obj, *mut u32)>(c, i)(c, o, null_mut(), null_mut());
        shader(76, &mut s.vs);
        shader(74, &mut s.ps);
        shader(82, &mut s.gs);
        shader(98, &mut s.hs);
        shader(102, &mut s.ds);
        get::<unsafe extern "system" fn(Obj, u32, u32, *mut Obj)>(c, 73)(c, 0, 1, &mut s.srv);
        get::<unsafe extern "system" fn(Obj, u32, u32, *mut Obj)>(c, 75)(c, 0, 1, &mut s.smp);
        get::<unsafe extern "system" fn(Obj, *mut Obj, *mut f32, *mut u32)>(c, 91)(c, &mut s.blend, s.factor.as_mut_ptr(), &mut s.mask);
        get::<unsafe extern "system" fn(Obj, *mut Obj, *mut u32)>(c, 92)(c, &mut s.depth, &mut s.sref);
        get::<unsafe extern "system" fn(Obj, *mut Obj)>(c, 94)(c, &mut s.rs);
        s
    }

    unsafe fn restore(&self, s: Saved) {
        let c = self.ctx;
        get::<unsafe extern "system" fn(Obj, u32, *const Obj, Obj)>(c, 33)(c, 8, s.rtv.as_ptr(), s.dsv);
        get::<unsafe extern "system" fn(Obj, u32, *const [f32; 6])>(c, 44)(c, s.nvp, s.vp.as_ptr());
        get::<unsafe extern "system" fn(Obj, Obj)>(c, 17)(c, s.layout);
        get::<unsafe extern "system" fn(Obj, u32)>(c, 24)(c, s.topo);
        get::<unsafe extern "system" fn(Obj, u32, u32, *const Obj, *const u32, *const u32)>(c, 18)(c, 0, 1, &s.vb, &s.stride, &s.off);
        for (i, o) in [(11, s.vs), (9, s.ps), (23, s.gs), (60, s.hs), (64, s.ds)] {
            get::<unsafe extern "system" fn(Obj, Obj, *const Obj, u32)>(c, i)(c, o, null(), 0);
        }
        get::<unsafe extern "system" fn(Obj, u32, u32, *const Obj)>(c, 8)(c, 0, 1, &s.srv);
        get::<unsafe extern "system" fn(Obj, u32, u32, *const Obj)>(c, 10)(c, 0, 1, &s.smp);
        get::<unsafe extern "system" fn(Obj, Obj, *const f32, u32)>(c, 35)(c, s.blend, s.factor.as_ptr(), s.mask);
        get::<unsafe extern "system" fn(Obj, Obj, u32)>(c, 36)(c, s.depth, s.sref);
        get::<unsafe extern "system" fn(Obj, Obj)>(c, 43)(c, s.rs);
        for o in s.rtv.into_iter().chain([s.dsv, s.layout, s.vb, s.vs, s.ps, s.gs, s.hs, s.ds, s.srv, s.smp, s.blend, s.depth, s.rs]) {
            release(o);
        }
    }
}

impl Drop for D11 {
    fn drop(&mut self) {
        unsafe {
            for o in [self.rtv, self.vb, self.smp, self.rs, self.depth, self.blend, self.layout, self.ps, self.vs, self.srv, self.ctx, self.dev] {
                release(o);
            }
        }
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::d3d::{Api, Dev};
    use crate::draw::rgba;

    #[test]
    fn renders() {
        let dev = Dev::new(Api::D11, 64, 64).expect("d3d11");
        let a = Atlas::new("Arial", 14, true).unwrap();
        let mut r = D11::from_swap(dev.swap, &a).expect("renderer");
        let mut b = Batch::new(&a);
        b.rect(8.0, 8.0, 16.0, 16.0, rgba(255, 0, 0, 255));
        b.rect(30.0, 8.0, 16.0, 16.0, rgba(0, 0, 255, 128));
        b.text(&a, 4.0, 36.0, "WWW", rgba(0, 255, 0, 255));
        let (d, c) = (dev.device, dev.context);
        unsafe {
            assert!(r.target());
            get::<unsafe extern "system" fn(Obj, Obj, *const f32)>(c, 50)(c, r.rtv, [0.0, 0.0, 0.0, 1.0f32].as_ptr());
            let marker = null_mut::<c_void>();
            get::<unsafe extern "system" fn(Obj, u32, *const Obj, Obj)>(c, 33)(c, 1, &marker, null_mut());
            r.draw(&b);
            let mut cur = [null_mut(); 1];
            get::<unsafe extern "system" fn(Obj, u32, *mut Obj, *mut Obj)>(c, 89)(c, 1, cur.as_mut_ptr(), null_mut());
            assert!(cur[0].is_null());
            let mut bb = null_mut();
            get::<unsafe extern "system" fn(Obj, u32, *const [u8; 16], *mut Obj) -> i32>(dev.swap, 9)(dev.swap, 0, &IID_TEX2D, &mut bb);
            let desc = [64u32, 64, 1, 1, 28, 1, 0, 3, 0, 0x20000, 0];
            let mut st = null_mut();
            assert!(get::<unsafe extern "system" fn(Obj, *const u32, *const c_void, *mut Obj) -> i32>(d, 5)(d, desc.as_ptr(), null(), &mut st) >= 0);
            get::<unsafe extern "system" fn(Obj, Obj, Obj)>(c, 47)(c, st, bb);
            let mut m: (*mut u8, u32, u32) = (null_mut(), 0, 0);
            assert!(get::<unsafe extern "system" fn(Obj, Obj, u32, u32, u32, *mut (*mut u8, u32, u32)) -> i32>(c, 14)(c, st, 0, 1, 0, &mut m) >= 0);
            let px = |x: usize, y: usize| *(m.0.add(y * m.1 as usize + x * 4) as *const u32) & 0xffffff;
            assert_eq!(px(16, 16), 0x0000ff);
            assert_eq!(px(4, 4), 0);
            let half = px(38, 16);
            assert!((0x70..=0x90).contains(&(half >> 16)) && half & 0xffff == 0, "{half:x}");
            assert!((36..36 + a.line as usize).any(|y| (4..60).any(|x| px(x, y) == 0x00ff00)));
            get::<unsafe extern "system" fn(Obj, Obj, u32)>(c, 15)(c, st, 0);
            release(st);
            release(bb);
        }
        r.reset();
        assert!(r.rtv.is_null());
        drop(r);
    }
}

#[cfg(test)]
mod gen {
    use super::HLSL;
    use crate::d3d::{method, release, Obj};
    use crate::win;
    use std::ptr::null_mut;

    #[test]
    #[ignore]
    fn shaders() {
        let Ok(dll) = std::env::var("AC_D3DC") else { return };
        let f = win::proc_addr(win::load(&dll), "D3DCompile");
        assert_ne!(f, 0);
        type C = unsafe extern "system" fn(*const u8, usize, *const u8, *const u8, *const u8, *const u8, *const u8, u32, u32, *mut Obj, *mut Obj) -> i32;
        let c: C = unsafe { std::mem::transmute(f) };
        let mut out = String::new();
        for (name, entry, target) in [("VS", "vs\0", "vs_4_0_level_9_1\0"), ("PS", "ps\0", "ps_4_0_level_9_1\0")] {
            let (mut code, mut err) = (null_mut(), null_mut());
            let r = unsafe { c(HLSL.as_ptr(), HLSL.len(), std::ptr::null(), std::ptr::null(), std::ptr::null(), entry.as_ptr(), target.as_ptr(), 1 << 15, 0, &mut code, &mut err) };
            assert!(r >= 0 && !code.is_null());
            let b = unsafe {
                let p = method::<unsafe extern "system" fn(Obj) -> *const u8>(code, 3)(code);
                let n = method::<unsafe extern "system" fn(Obj) -> usize>(code, 4)(code);
                std::slice::from_raw_parts(p, n).to_vec()
            };
            unsafe {
                release(code);
                release(err);
            }
            let bytes: Vec<String> = b.iter().map(|x| x.to_string()).collect();
            out += &format!("pub const {name}: [u8; {}] = [\n", b.len());
            for ch in bytes.chunks(32) {
                out += &format!("    {},\n", ch.join(", "));
            }
            out += "];\n";
        }
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/src/draw/dxbc.rs"), out).unwrap();
    }
}
