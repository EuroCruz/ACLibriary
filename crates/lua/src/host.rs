use std::ffi::c_void;

pub type Load = unsafe extern "C" fn(*mut c_void, *const u8, usize, *const u8) -> i32;
pub type Pcall = unsafe extern "C" fn(*mut c_void, i32, i32, i32) -> i32;
pub type Tostr = unsafe extern "C" fn(*mut c_void, i32, *mut usize) -> *const u8;
pub type Settop = unsafe extern "C" fn(*mut c_void, i32);

#[derive(Clone, Copy)]
pub struct Api {
    pub load: Load,
    pub pcall: Pcall,
    pub tostr: Tostr,
    pub settop: Settop,
}

impl Api {
    pub unsafe fn from_addrs(load: usize, pcall: usize, tostr: usize, settop: usize) -> Api {
        Api {
            load: std::mem::transmute::<usize, Load>(load),
            pcall: std::mem::transmute::<usize, Pcall>(pcall),
            tostr: std::mem::transmute::<usize, Tostr>(tostr),
            settop: std::mem::transmute::<usize, Settop>(settop),
        }
    }
    pub unsafe fn run(&self, l: *mut c_void, src: &str, name: &str) -> Result<(), String> {
        let n = format!("{name}\0");
        let mut st = (self.load)(l, src.as_ptr(), src.len(), n.as_ptr());
        if st == 0 {
            st = (self.pcall)(l, 0, 0, 0);
        }
        if st == 0 {
            return Ok(());
        }
        let mut len = 0usize;
        let p = (self.tostr)(l, -1, &mut len);
        let msg = if p.is_null() { format!("lua error {st}") } else { String::from_utf8_lossy(std::slice::from_raw_parts(p, len)).into_owned() };
        (self.settop)(l, -2);
        Err(msg)
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};

    static LOADED: AtomicI32 = AtomicI32::new(0);
    static POPPED: AtomicI32 = AtomicI32::new(0);
    static FAIL: AtomicI32 = AtomicI32::new(0);

    unsafe extern "C" fn load(_: *mut c_void, b: *const u8, n: usize, _: *const u8) -> i32 {
        let s = std::slice::from_raw_parts(b, n);
        LOADED.fetch_add(1, Ordering::SeqCst);
        if s == b"syntax" { 3 } else { 0 }
    }

    unsafe extern "C" fn pcall(_: *mut c_void, _: i32, _: i32, _: i32) -> i32 {
        FAIL.load(Ordering::SeqCst)
    }

    unsafe extern "C" fn tostr(_: *mut c_void, _: i32, n: *mut usize) -> *const u8 {
        *n = 4;
        b"boom".as_ptr()
    }

    unsafe extern "C" fn settop(_: *mut c_void, i: i32) {
        POPPED.store(i, Ordering::SeqCst);
    }

    #[test]
    fn runs_and_reports() {
        let api = unsafe { Api::from_addrs(load as *const () as usize, pcall as *const () as usize, tostr as *const () as usize, settop as *const () as usize) };
        let l = std::ptr::null_mut();
        unsafe {
            assert!(api.run(l, "print(1)", "t").is_ok());
            assert_eq!(api.run(l, "syntax", "t").unwrap_err(), "boom");
            assert_eq!(POPPED.load(Ordering::SeqCst), -2);
            FAIL.store(2, Ordering::SeqCst);
            assert_eq!(api.run(l, "x", "t").unwrap_err(), "boom");
        }
        assert_eq!(LOADED.load(Ordering::SeqCst), 3);
    }
}
