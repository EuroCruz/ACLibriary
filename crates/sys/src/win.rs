#![allow(non_snake_case, non_camel_case_types, clippy::too_many_arguments)]

use std::ffi::{c_char, c_void};

pub type HANDLE = *mut c_void;
pub type HWND = *mut c_void;
pub type HMODULE = *mut c_void;
pub type HDC = *mut c_void;
pub type BOOL = i32;
pub type DWORD = u32;

pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
pub const MEM_COMMIT: u32 = 0x1000;
pub const MEM_RESERVE: u32 = 0x2000;
pub const MEM_FREE: u32 = 0x10000;
pub const MEM_RELEASE: u32 = 0x8000;
pub const GWLP_WNDPROC: i32 = -4;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemInfo {
    pub base: usize,
    pub alloc_base: usize,
    pub alloc_prot: u32,
    #[cfg(target_pointer_width = "64")]
    pub pad1: u32,
    pub size: usize,
    pub state: u32,
    pub prot: u32,
    pub kind: u32,
    #[cfg(target_pointer_width = "64")]
    pub pad2: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SysInfo {
    pub arch: u32,
    pub page: u32,
    pub min_addr: usize,
    pub max_addr: usize,
    pub mask: usize,
    pub cpus: u32,
    pub kind: u32,
    pub granularity: u32,
    pub level: u16,
    pub rev: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MonitorInfo {
    pub size: u32,
    pub monitor: RectI,
    pub work: RectI,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RectI {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleA(name: *const c_char) -> HMODULE;
    pub fn GetModuleHandleW(name: *const u16) -> HMODULE;
    pub fn GetModuleFileNameW(m: HMODULE, buf: *mut u16, n: u32) -> u32;
    pub fn ReadFile(f: HANDLE, buf: *mut c_void, n: u32, read: *mut u32, ov: *mut c_void) -> BOOL;
    pub fn SetFilePointerEx(f: HANDLE, dist: i64, new: *mut i64, how: u32) -> BOOL;
    pub fn GetFinalPathNameByHandleW(f: HANDLE, buf: *mut u16, n: u32, flags: u32) -> u32;
    pub fn GetProcAddress(m: HMODULE, name: *const c_char) -> *mut c_void;
    pub fn LoadLibraryA(name: *const c_char) -> HMODULE;
    pub fn LoadLibraryW(name: *const u16) -> HMODULE;
    pub fn GetSystemDirectoryW(buf: *mut u16, n: u32) -> u32;
    pub fn ExitProcess(code: u32) -> !;
    pub fn SetUnhandledExceptionFilter(f: usize) -> usize;
    pub fn AddVectoredExceptionHandler(first: u32, f: usize) -> *mut c_void;
    pub fn FreeLibrary(m: HMODULE) -> BOOL;
    pub fn DisableThreadLibraryCalls(m: HMODULE) -> BOOL;
    pub fn VirtualProtect(a: *mut c_void, n: usize, new: u32, old: *mut u32) -> BOOL;
    pub fn VirtualAlloc(a: *mut c_void, n: usize, kind: u32, prot: u32) -> *mut c_void;
    pub fn VirtualFree(a: *mut c_void, n: usize, kind: u32) -> BOOL;
    pub fn VirtualQuery(a: *const c_void, out: *mut MemInfo, n: usize) -> usize;
    pub fn FlushInstructionCache(p: HANDLE, a: *const c_void, n: usize) -> BOOL;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn GetCurrentProcessId() -> u32;
    pub fn GetCurrentThreadId() -> u32;
    pub fn GetSystemInfo(i: *mut SysInfo);
    pub fn GetLastError() -> u32;
    pub fn Sleep(ms: u32);
    pub fn QueryPerformanceCounter(c: *mut i64) -> BOOL;
    pub fn QueryPerformanceFrequency(f: *mut i64) -> BOOL;
    pub fn OutputDebugStringA(s: *const c_char);
    pub fn AllocConsole() -> BOOL;
    pub fn CloseHandle(h: HANDLE) -> BOOL;
    pub fn GetModuleHandleExW(flags: u32, name: *const u16, m: *mut HMODULE) -> BOOL;
    pub fn GetLocalTime(t: *mut [u16; 8]);
    pub fn SetConsoleTitleW(s: *const u16) -> BOOL;
    pub fn CreateNamedPipeW(name: *const u16, open: u32, mode: u32, max: u32, out: u32, inb: u32, timeout: u32, sa: *mut c_void) -> HANDLE;
    pub fn ConnectNamedPipe(h: HANDLE, ov: *mut c_void) -> BOOL;
    pub fn OpenThread(access: u32, inherit: BOOL, id: u32) -> HANDLE;
    pub fn SuspendThread(h: HANDLE) -> u32;
    pub fn ResumeThread(h: HANDLE) -> u32;
    pub fn GetThreadContext(h: HANDLE, ctx: *mut c_void) -> BOOL;
    pub fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> HANDLE;
    pub fn Thread32First(snap: HANDLE, e: *mut [u32; 7]) -> BOOL;
    pub fn Thread32Next(snap: HANDLE, e: *mut [u32; 7]) -> BOOL;
    pub fn EnterCriticalSection(cs: *mut c_void);
    pub fn LeaveCriticalSection(cs: *mut c_void);
}

#[link(name = "user32")]
extern "system" {
    pub fn GetAsyncKeyState(k: i32) -> i16;
    pub fn GetRawInputDeviceList(list: *mut [usize; 2], n: *mut u32, size: u32) -> u32;
    pub fn GetRawInputDeviceInfoW(dev: HANDLE, cmd: u32, data: *mut c_void, size: *mut u32) -> u32;
    pub fn GetKeyState(k: i32) -> i16;
    pub fn GetKeyNameTextW(lp: i32, s: *mut u16, n: i32) -> i32;
    pub fn ActivateKeyboardLayout(hkl: HANDLE, flags: u32) -> HANDLE;
    pub fn GetForegroundWindow() -> HWND;
    pub fn GetWindowThreadProcessId(w: HWND, pid: *mut u32) -> u32;
    pub fn GetCursorPos(p: *mut Point) -> BOOL;
    pub fn SetCursorPos(x: i32, y: i32) -> BOOL;
    pub fn ScreenToClient(w: HWND, p: *mut Point) -> BOOL;
    pub fn GetClientRect(w: HWND, r: *mut RectI) -> BOOL;
    pub fn SetWindowPos(w: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> BOOL;
    pub fn MonitorFromWindow(w: HWND, flags: u32) -> HANDLE;
    pub fn GetMonitorInfoW(m: HANDLE, info: *mut MonitorInfo) -> BOOL;
    pub fn CreateWindowExA(ex: u32, class: *const c_char, title: *const c_char, style: u32, x: i32, y: i32, w: i32, h: i32, parent: HWND, menu: *mut c_void, inst: HMODULE, param: *mut c_void) -> HWND;
    pub fn DestroyWindow(w: HWND) -> BOOL;
    pub fn CallWindowProcA(prev: usize, w: HWND, msg: u32, wp: usize, lp: isize) -> isize;
    pub fn DefWindowProcA(w: HWND, msg: u32, wp: usize, lp: isize) -> isize;
    pub fn SendMessageA(w: HWND, msg: u32, wp: usize, lp: isize) -> isize;
    pub fn GetSystemMetrics(i: i32) -> i32;
    pub fn MessageBoxA(w: HWND, text: *const c_char, cap: *const c_char, kind: u32) -> i32;
    pub fn MessageBoxW(w: HWND, text: *const u16, cap: *const u16, kind: u32) -> i32;
    pub fn ClipCursor(r: *const RectI) -> BOOL;
    pub fn IsWindow(w: HWND) -> BOOL;
    pub fn GetDC(w: HWND) -> HDC;
    pub fn ReleaseDC(w: HWND, dc: HDC) -> i32;
    pub fn EnumThreadWindows(id: u32, f: usize, lp: isize) -> BOOL;
    pub fn IsHungAppWindow(w: HWND) -> BOOL;
    pub fn IsWindowVisible(w: HWND) -> BOOL;
}

#[cfg(target_pointer_width = "64")]
#[link(name = "user32")]
extern "system" {
    #[link_name = "SetWindowLongPtrA"]
    pub fn SetWindowLongPtr(w: HWND, i: i32, v: isize) -> isize;
    #[link_name = "GetWindowLongPtrA"]
    pub fn GetWindowLongPtr(w: HWND, i: i32) -> isize;
}

#[cfg(target_pointer_width = "32")]
#[link(name = "user32")]
extern "system" {
    #[link_name = "SetWindowLongA"]
    pub fn SetWindowLongPtr(w: HWND, i: i32, v: isize) -> isize;
    #[link_name = "GetWindowLongA"]
    pub fn GetWindowLongPtr(w: HWND, i: i32) -> isize;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateFontW(h: i32, w: i32, esc: i32, orient: i32, weight: i32, italic: u32, underline: u32, strike: u32, charset: u32, out: u32, clip: u32, quality: u32, pitch: u32, face: *const u16) -> HANDLE;
    pub fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub fn CreateDIBSection(dc: HDC, info: *const c_void, usage: u32, bits: *mut *mut c_void, section: HANDLE, off: u32) -> HANDLE;
    pub fn SelectObject(dc: HDC, o: HANDLE) -> HANDLE;
    pub fn DeleteObject(o: HANDLE) -> BOOL;
    pub fn DeleteDC(dc: HDC) -> BOOL;
    pub fn SetBkMode(dc: HDC, mode: i32) -> i32;
    pub fn SetTextColor(dc: HDC, c: u32) -> u32;
    pub fn SetBkColor(dc: HDC, c: u32) -> u32;
    pub fn TextOutW(dc: HDC, x: i32, y: i32, s: *const u16, n: i32) -> BOOL;
    pub fn GetTextExtentPoint32W(dc: HDC, s: *const u16, n: i32, size: *mut Point) -> BOOL;
    pub fn GdiFlush() -> BOOL;
}

#[link(name = "winmm")]
extern "system" {
    pub fn waveOutOpen(h: *mut HANDLE, dev: u32, fmt: *const WaveFormat, cb: usize, inst: usize, flags: u32) -> u32;
    pub fn waveOutPrepareHeader(h: HANDLE, hdr: *mut WaveHdr, n: u32) -> u32;
    pub fn waveOutUnprepareHeader(h: HANDLE, hdr: *mut WaveHdr, n: u32) -> u32;
    pub fn waveOutWrite(h: HANDLE, hdr: *mut WaveHdr, n: u32) -> u32;
    pub fn waveOutReset(h: HANDLE) -> u32;
    pub fn waveOutClose(h: HANDLE) -> u32;
    pub fn waveOutGetNumDevs() -> u32;
    pub fn waveOutSetVolume(h: HANDLE, v: u32) -> u32;
    pub fn waveOutPause(h: HANDLE) -> u32;
    pub fn waveOutRestart(h: HANDLE) -> u32;
    pub fn timeBeginPeriod(ms: u32) -> u32;
    pub fn timeEndPeriod(ms: u32) -> u32;
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct WaveFormat {
    pub tag: u16,
    pub channels: u16,
    pub rate: u32,
    pub bytes_per_sec: u32,
    pub align: u16,
    pub bits: u16,
    pub extra: u16,
}

#[repr(C)]
pub struct WaveHdr {
    pub data: *mut u8,
    pub len: u32,
    pub recorded: u32,
    pub user: usize,
    pub flags: u32,
    pub loops: u32,
    pub next: *mut WaveHdr,
    pub reserved: usize,
}

#[link(name = "shell32")]
extern "system" {
    pub fn ShellExecuteA(w: HWND, op: *const c_char, file: *const c_char, params: *const c_char, dir: *const c_char, show: i32) -> HMODULE;
}

#[link(name = "winhttp")]
extern "system" {
    pub fn WinHttpOpen(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> HANDLE;
    pub fn WinHttpConnect(s: HANDLE, server: *const u16, port: u16, reserved: u32) -> HANDLE;
    pub fn WinHttpOpenRequest(c: HANDLE, verb: *const u16, path: *const u16, ver: *const u16, referrer: *const u16, accept: *const *const u16, flags: u32) -> HANDLE;
    pub fn WinHttpSendRequest(r: HANDLE, headers: *const u16, hlen: u32, data: *const c_void, dlen: u32, total: u32, ctx: usize) -> BOOL;
    pub fn WinHttpQueryHeaders(r: HANDLE, level: u32, name: *const u16, buf: *mut c_void, len: *mut u32, index: *mut u32) -> BOOL;
    pub fn WinHttpReceiveResponse(r: HANDLE, reserved: *mut c_void) -> BOOL;
    pub fn WinHttpReadData(r: HANDLE, buf: *mut c_void, n: u32, read: *mut u32) -> BOOL;
    pub fn WinHttpCloseHandle(h: HANDLE) -> BOOL;
    pub fn WinHttpSetTimeouts(h: HANDLE, resolve: i32, connect: i32, send: i32, receive: i32) -> BOOL;
}

pub const HKCU: usize = 0x8000_0001;
pub const HKLM: usize = 0x8000_0002;

#[link(name = "advapi32")]
extern "system" {
    pub fn RegGetValueW(key: HANDLE, sub: *const u16, name: *const u16, flags: u32, kind: *mut u32, data: *mut c_void, size: *mut u32) -> i32;
}

pub fn reg_str(root: usize, path: &str, name: &str) -> Option<String> {
    let mut b = vec![0u16; 1024];
    let mut n = (b.len() * 2) as u32;
    let r = unsafe { RegGetValueW(root as HANDLE, wide(path).as_ptr(), wide(name).as_ptr(), 2, std::ptr::null_mut(), b.as_mut_ptr().cast(), &mut n) };
    (r == 0).then(|| String::from_utf16_lossy(&b[..(n as usize / 2).saturating_sub(1)]))
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn cstr(s: &str) -> Vec<u8> {
    s.bytes().chain(Some(0)).collect()
}

pub fn module(name: &str) -> HMODULE {
    unsafe { GetModuleHandleA(cstr(name).as_ptr().cast()) }
}

pub fn load(name: &str) -> HMODULE {
    unsafe { LoadLibraryA(cstr(name).as_ptr().cast()) }
}

pub fn proc_addr(m: HMODULE, name: &str) -> usize {
    unsafe { GetProcAddress(m, cstr(name).as_ptr().cast()) as usize }
}

pub fn monitor_rect(w: HWND) -> Option<RectI> {
    let mut i = MonitorInfo { size: std::mem::size_of::<MonitorInfo>() as u32, ..Default::default() };
    (unsafe { GetMonitorInfoW(MonitorFromWindow(w, 2), &mut i) } != 0).then_some(i.monitor)
}

pub fn borderless(w: HWND) -> Option<RectI> {
    if unsafe { IsWindow(w) } == 0 {
        return None;
    }
    let r = monitor_rect(w)?;
    unsafe {
        let style = GetWindowLongPtr(w, -16) as u32 & !0x00cf_0000 | 0x9000_0000;
        let ex = GetWindowLongPtr(w, -20) as u32 & !0x0002_0309;
        SetWindowLongPtr(w, -16, style as i32 as isize);
        SetWindowLongPtr(w, -20, ex as i32 as isize);
        SetWindowPos(w, -2isize as HWND, r.l, r.t, r.r - r.l, r.b - r.t, 0x0020 | 0x0040 | 0x0200);
    }
    Some(r)
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn links_and_resolves() {
        let k = module("kernel32.dll");
        assert!(!k.is_null());
        assert_ne!(proc_addr(k, "VirtualProtect"), 0);
        assert_eq!(proc_addr(k, "NoSuchExport"), 0);
        assert_eq!(wide("a"), [97, 0]);
        let mut q = 0i64;
        assert_eq!(unsafe { QueryPerformanceFrequency(&mut q) }, 1);
        assert!(q > 0);
        assert_eq!(std::mem::size_of::<MemInfo>(), if cfg!(target_pointer_width = "64") { 48 } else { 28 });
        assert_eq!(std::mem::size_of::<SysInfo>(), if cfg!(target_pointer_width = "64") { 48 } else { 36 });
        let w = unsafe { CreateWindowExA(0, c"STATIC".as_ptr(), c"".as_ptr(), 0x00cf_0000, 10, 10, 200, 100, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        let m = borderless(w).unwrap();
        let mut c = RectI::default();
        unsafe { GetClientRect(w, &mut c) };
        assert_eq!((c.r, c.b), (m.r - m.l, m.b - m.t));
        assert_eq!(unsafe { GetWindowLongPtr(w, -16) } as u32 & 0x00cf_0000, 0);
        assert_eq!(unsafe { GetWindowLongPtr(w, -20) } as u32 & 0x8, 0);
        unsafe { DestroyWindow(w) };
        assert!(borderless(std::ptr::null_mut()).is_none());
        assert!(reg_str(HKLM, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "ProductName").is_some_and(|s| s.starts_with("Windows")));
        assert!(reg_str(HKLM, r"SOFTWARE\NoSuchKey", "x").is_none());
    }
}
