use crate::win::{self, ExitProcess, GetSystemDirectoryW, LoadLibraryW, HMODULE};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub fn system_dir() -> PathBuf {
    let mut b = [0u16; 512];
    let n = unsafe { GetSystemDirectoryW(b.as_mut_ptr(), b.len() as u32) } as usize;
    PathBuf::from(String::from_utf16_lossy(&b[..n.min(b.len())]))
}

pub fn load_system(name: &str) -> HMODULE {
    let p = system_dir().join(name);
    unsafe { LoadLibraryW(win::wide(&p.to_string_lossy()).as_ptr()) }
}

pub struct Lib {
    name: &'static str,
    h: AtomicUsize,
}

impl Lib {
    pub const fn new(name: &'static str) -> Lib {
        Lib { name, h: AtomicUsize::new(0) }
    }

    pub fn handle(&self) -> HMODULE {
        let h = self.h.load(Ordering::Acquire);
        if h != 0 {
            return h as HMODULE;
        }
        let h = load_system(self.name);
        self.h.store(h as usize, Ordering::Release);
        h
    }
}

#[repr(C)]
pub struct Stub {
    slot: AtomicUsize,
    name: &'static str,
}

impl Stub {
    pub const fn new(name: &'static str) -> Stub {
        Stub { slot: AtomicUsize::new(0), name }
    }
}

#[doc(hidden)]
pub extern "C" fn resolve(lib: &Lib, s: &Stub) -> usize {
    let a = win::proc_addr(lib.handle(), s.name);
    if a == 0 {
        crate::dll::log(&format!("proxy: {} has no {}", lib.name, s.name));
        unsafe { ExitProcess(1) }
    }
    s.slot.store(a, Ordering::Release);
    a
}

#[macro_export]
macro_rules! proxy {
    ($dll:literal: $($f:ident),+ $(,)?) => {
        #[allow(non_upper_case_globals)]
        mod proxy_exports {
            pub static LIB: $crate::proxy::Lib = $crate::proxy::Lib::new($dll);
            $(pub static $f: $crate::proxy::Stub = $crate::proxy::Stub::new(stringify!($f));)+
            $($crate::proxy_stub!($f);)+
        }
    };
}

#[cfg(target_arch = "x86")]
#[macro_export]
macro_rules! export {
    ($name:ident => $f:path) => {
        ::std::arch::global_asm!(
            concat!(".globl _", stringify!($name)),
            concat!("_", stringify!($name), ":"),
            "jmp {f}",
            ".section .drectve",
            concat!(".ascii \" -export:", stringify!($name), "\""),
            ".text",
            f = sym $f,
        );
    };
}

#[cfg(target_arch = "x86_64")]
#[macro_export]
macro_rules! export {
    ($name:ident => $f:path) => {
        ::std::arch::global_asm!(
            concat!(".globl ", stringify!($name)),
            concat!(stringify!($name), ":"),
            "jmp {f}",
            ".section .drectve",
            concat!(".ascii \" -export:", stringify!($name), "\""),
            ".text",
            f = sym $f,
        );
    };
}

#[cfg(target_arch = "x86")]
#[doc(hidden)]
#[macro_export]
macro_rules! proxy_stub {
    ($f:ident) => {
        ::std::arch::global_asm!(
            concat!(".globl _", stringify!($f)),
            concat!("_", stringify!($f), ":"),
            "mov eax, dword ptr [{s}]",
            "test eax, eax",
            "jz 1f",
            "jmp eax",
            "1:",
            "push ecx",
            "push edx",
            "lea eax, [{s}]",
            "push eax",
            "lea eax, [{l}]",
            "push eax",
            "call {r}",
            "add esp, 8",
            "pop edx",
            "pop ecx",
            "jmp eax",
            ".section .drectve",
            concat!(".ascii \" -export:", stringify!($f), "\""),
            ".text",
            s = sym $f,
            l = sym LIB,
            r = sym $crate::proxy::resolve,
        );
    };
}

#[cfg(target_arch = "x86_64")]
#[doc(hidden)]
#[macro_export]
macro_rules! proxy_stub {
    ($f:ident) => {
        ::std::arch::global_asm!(
            concat!(".globl ", stringify!($f)),
            concat!(stringify!($f), ":"),
            "mov rax, qword ptr [rip + {s}]",
            "test rax, rax",
            "jz 1f",
            "jmp rax",
            "1:",
            "push rcx",
            "push rdx",
            "push r8",
            "push r9",
            "sub rsp, 0x68",
            "movdqu [rsp + 0x20], xmm0",
            "movdqu [rsp + 0x30], xmm1",
            "movdqu [rsp + 0x40], xmm2",
            "movdqu [rsp + 0x50], xmm3",
            "lea rcx, [rip + {l}]",
            "lea rdx, [rip + {s}]",
            "call {r}",
            "movdqu xmm0, [rsp + 0x20]",
            "movdqu xmm1, [rsp + 0x30]",
            "movdqu xmm2, [rsp + 0x40]",
            "movdqu xmm3, [rsp + 0x50]",
            "add rsp, 0x68",
            "pop r9",
            "pop r8",
            "pop rdx",
            "pop rcx",
            "jmp rax",
            ".section .drectve",
            concat!(".ascii \" -export:", stringify!($f), "\""),
            ".text",
            s = sym $f,
            l = sym LIB,
            r = sym $crate::proxy::resolve,
        );
    };
}

#[cfg(test)]
mod t {
    use super::*;

    crate::proxy!("version.dll": GetFileVersionInfoSizeW);

    extern "system" fn twice(x: u32) -> u32 {
        x * 2
    }

    crate::export!(WspTwice => twice);

    extern "C" {
        #[link_name = "GetFileVersionInfoSizeW"]
        fn stub();
        #[link_name = "WspTwice"]
        fn exported();
    }

    #[test]
    fn forwards_to_system_copy() {
        assert!(system_dir().join("kernel32.dll").is_file());
        let f: extern "system" fn(*const u16, *mut u32) -> u32 = unsafe { std::mem::transmute(stub as *const ()) };
        let k = win::wide(&system_dir().join("kernel32.dll").to_string_lossy());
        let mut h = 7u32;
        assert!(f(k.as_ptr(), &mut h) > 0);
        assert_eq!(h, 0);
        let p = proxy_exports::GetFileVersionInfoSizeW.slot.load(Ordering::Acquire);
        assert_eq!(p, win::proc_addr(proxy_exports::LIB.handle(), "GetFileVersionInfoSizeW"));
        assert!(f(k.as_ptr(), &mut h) > 0);
        let g: extern "system" fn(u32) -> u32 = unsafe { std::mem::transmute(exported as *const ()) };
        assert_eq!(g(21), 42);
    }
}
