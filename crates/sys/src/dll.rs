use crate::win::{self, AllocConsole, GetLocalTime, GetModuleFileNameW, GetModuleHandleExW, OutputDebugStringA, SetConsoleTitleW, HMODULE};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub fn module_path(m: HMODULE) -> Option<PathBuf> {
    let mut b = vec![0u16; 1024];
    loop {
        let n = unsafe { GetModuleFileNameW(m, b.as_mut_ptr(), b.len() as u32) } as usize;
        if n == 0 {
            return None;
        }
        if n < b.len() {
            return Some(PathBuf::from(String::from_utf16_lossy(&b[..n])));
        }
        b.resize(b.len() * 2, 0);
    }
}

pub fn module_at(addr: usize) -> HMODULE {
    let mut m = std::ptr::null_mut();
    unsafe { GetModuleHandleExW(6, addr as *const u16, &mut m) };
    m
}

pub fn exe_path() -> PathBuf {
    module_path(std::ptr::null_mut()).unwrap_or_default()
}

pub fn exe_dir() -> PathBuf {
    exe_path().parent().map(Path::to_path_buf).unwrap_or_default()
}

pub fn self_path() -> PathBuf {
    module_path(module_at(self_path as fn() -> PathBuf as usize)).unwrap_or_default()
}

pub fn self_dir() -> PathBuf {
    self_path().parent().map(Path::to_path_buf).unwrap_or_default()
}

pub fn stamp() -> String {
    let mut t = [0u16; 8];
    unsafe { GetLocalTime(&mut t) };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}", t[0], t[1], t[3], t[4], t[5], t[6], t[7])
}

static LOG: Mutex<Option<File>> = Mutex::new(None);
static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn log_path() -> Option<PathBuf> {
    LOG_PATH.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn log_to(path: &Path, append: bool) -> std::io::Result<()> {
    let f = OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(path)?;
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    *LOG_PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.to_path_buf());
    Ok(())
}

pub fn log(s: &str) {
    let line = format!("[{}] {s}\r\n", stamp());
    if let Some(f) = LOG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
    unsafe { OutputDebugStringA(win::cstr(&line).as_ptr().cast()) };
}

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { $crate::dll::log(&format!($($t)*)) };
}

pub fn console(title: &str) -> bool {
    let ok = unsafe { AllocConsole() } != 0;
    unsafe { SetConsoleTitleW(win::wide(title).as_ptr()) };
    ok
}

pub fn call(name: &str, f: fn()) {
    if let Err(e) = std::panic::catch_unwind(f) {
        let m = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_default();
        log(&format!("{name} panicked: {m}"));
    }
}

pub fn run(name: &'static str, f: fn()) {
    let _ = std::thread::Builder::new().name(name.into()).spawn(move || call(name, f));
}

static PREV_FILTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

static FAULTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn place(at: usize) -> String {
    let m = module_at(at);
    match module_path(m) {
        Some(p) if !m.is_null() => format!("{}+0x{:x}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), at - m as usize),
        _ => String::new(),
    }
}

unsafe fn describe(info: *const [usize; 2]) -> (u32, String) {
    let ps = std::mem::size_of::<usize>();
    let rec = (*info)[0] as *const u8;
    let code = *(rec as *const u32);
    let at = *(rec.add(8 + ps) as *const usize);
    let params = *(rec.add(8 + 2 * ps) as *const u32) as usize;
    let args = rec.add((8 + 2 * ps + 4 + ps - 1) & !(ps - 1)) as *const usize;
    let target = if code == 0xc000_0005 && params >= 2 { format!(" ({} 0x{:x})", if *args == 0 { "reading" } else { "writing" }, *args.add(1)) } else { String::new() };
    (code, format!("exception 0x{code:08x} at 0x{at:08x} {}{target}", place(at)))
}

pub fn error_box(title: &str, text: &str, ask: bool) -> bool {
    unsafe { win::ClipCursor(std::ptr::null()) };
    let kind = 0x10 | 0x1000 | 0x10000 | 0x40000 | if ask { 1 } else { 0 };
    unsafe { win::MessageBoxW(std::ptr::null_mut(), win::wide(text).as_ptr(), win::wide(title).as_ptr(), kind) == 1 }
}

static CRASH_BOX: Mutex<Option<String>> = Mutex::new(None);

pub fn show_crashes(title: &str) {
    *CRASH_BOX.lock().unwrap_or_else(|e| e.into_inner()) = Some(title.to_string());
}

unsafe extern "system" fn set_filter(f: usize) -> usize {
    PREV_FILTER.swap(f, std::sync::atomic::Ordering::Relaxed)
}

pub fn keep_crash_filter() -> bool {
    crate::hook::hook_iat(crate::mem::Module::main(), "kernel32.dll", "SetUnhandledExceptionFilter", set_filter as *const () as usize).is_some()
}

unsafe extern "system" fn crash(info: *const [usize; 2]) -> i32 {
    let text = describe(info).1;
    log(&format!("crash: {text}"));
    let title = CRASH_BOX.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(title) = title {
        let at = log_path().map_or(String::new(), |p| format!("\n\nDetails: {}", p.display()));
        error_box(&title, &format!("The game crashed.\n\n{text}{at}"), false);
    }
    match PREV_FILTER.load(std::sync::atomic::Ordering::Relaxed) {
        0 => 0,
        f => std::mem::transmute::<usize, unsafe extern "system" fn(*const [usize; 2]) -> i32>(f)(info),
    }
}

unsafe extern "system" fn fault(info: *const [usize; 2]) -> i32 {
    let (code, text) = describe(info);
    if matches!(code, 0xc000_0005 | 0xc000_001d | 0xc000_0094 | 0xc000_0096 | 0xc000_00fd | 0xc000_0409) && FAULTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 8 {
        log(&format!("fault: {text}"));
    }
    0
}

pub fn log_crashes() {
    let prev = unsafe { win::SetUnhandledExceptionFilter(crash as *const () as usize) };
    PREV_FILTER.store(prev, std::sync::atomic::Ordering::Relaxed);
    unsafe { win::AddVectoredExceptionHandler(1, fault as *const () as usize) };
}

#[cfg(target_pointer_width = "32")]
const CTX: (usize, usize, u32, usize, usize) = (0x2cc, 0, 0x1_0001, 0xb8, 0xc4);
#[cfg(target_pointer_width = "64")]
const CTX: (usize, usize, u32, usize, usize) = (0x4d0, 0x30, 0x10_0001, 0xf8, 0x98);

#[repr(C, align(16))]
struct Ctx([u8; CTX.0]);

unsafe extern "system" fn hung_window(w: win::HWND, out: isize) -> i32 {
    if win::IsWindowVisible(w) != 0 && win::IsHungAppWindow(w) != 0 {
        *(out as *mut bool) = true;
    }
    1
}

fn hung(thread: u32) -> bool {
    let mut h = false;
    unsafe { win::EnumThreadWindows(thread, hung_window as *const () as usize, &mut h as *mut bool as isize) };
    h
}

fn code(at: usize) -> bool {
    let mut m = std::mem::MaybeUninit::<win::MemInfo>::zeroed();
    let ok = unsafe { win::VirtualQuery(at as *const _, m.as_mut_ptr(), std::mem::size_of::<win::MemInfo>()) } != 0;
    ok && !module_at(at).is_null() && unsafe { m.assume_init() }.prot & 0xf0 != 0
}

fn stack_end(sp: usize) -> usize {
    let mut m = std::mem::MaybeUninit::<win::MemInfo>::zeroed();
    if unsafe { win::VirtualQuery(sp as *const _, m.as_mut_ptr(), std::mem::size_of::<win::MemInfo>()) } == 0 {
        return sp;
    }
    let m = unsafe { m.assume_init() };
    (m.base + m.size).min(sp + 0x4000)
}

pub fn sample(thread: u32) -> Option<String> {
    let h = unsafe { win::OpenThread(0x0a, 0, thread) };
    if h.is_null() {
        return None;
    }
    let mut c = Ctx([0; CTX.0]);
    unsafe { *(c.0.as_mut_ptr().add(CTX.1) as *mut u32) = CTX.2 };
    let mut words = Vec::new();
    let mut ip = 0;
    unsafe {
        if win::SuspendThread(h) != u32::MAX {
            if win::GetThreadContext(h, c.0.as_mut_ptr() as *mut _) != 0 {
                ip = *(c.0.as_ptr().add(CTX.3) as *const usize);
                let sp = *(c.0.as_ptr().add(CTX.4) as *const usize);
                let end = stack_end(sp);
                let ps = std::mem::size_of::<usize>();
                words = (sp..end).step_by(ps).map(|a| *(a as *const usize)).collect();
            }
            win::ResumeThread(h);
        }
        win::CloseHandle(h);
    }
    if ip == 0 {
        return None;
    }
    let calls: Vec<String> = words.into_iter().filter(|&a| code(a)).take(24).map(|a| format!("0x{a:08x} {}", place(a))).collect();
    Some(format!("at 0x{ip:08x} {}; stack: {}", place(ip), calls.join(", ")))
}

pub fn threads() -> Vec<u32> {
    let pid = unsafe { win::GetCurrentProcessId() };
    let snap = unsafe { win::CreateToolhelp32Snapshot(4, 0) };
    if snap as isize == -1 {
        return Vec::new();
    }
    let mut e = [28u32, 0, 0, 0, 0, 0, 0];
    let mut out = Vec::new();
    let mut ok = unsafe { win::Thread32First(snap, &mut e) };
    while ok != 0 {
        if e[3] == pid {
            out.push(e[2]);
        }
        e[0] = 28;
        ok = unsafe { win::Thread32Next(snap, &mut e) };
    }
    unsafe { win::CloseHandle(snap) };
    out
}

pub fn watch_hangs() {
    let main = unsafe { win::GetCurrentThreadId() };
    let _ = std::thread::Builder::new().name("hang watch".into()).spawn(move || {
        let mut shots = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if !hung(main) {
                shots = 0;
                continue;
            }
            if shots < 3 {
                shots += 1;
                if let Some(s) = sample(main) {
                    log(&format!("hang: main thread does not respond, {s}"));
                }
                if shots != 2 {
                    let me = unsafe { win::GetCurrentThreadId() };
                    for t in threads().into_iter().filter(|&t| t != main && t != me) {
                        if let Some(s) = sample(t) {
                            log(&format!("hang: thread {t} {s}"));
                        }
                    }
                }
            }
        }
    });
}

#[macro_export]
macro_rules! dll_main {
    (now $f:path) => {
        #[no_mangle]
        pub extern "system" fn DllMain(h: $crate::win::HMODULE, reason: u32, _: *mut ::std::ffi::c_void) -> i32 {
            if reason == 1 {
                unsafe { $crate::win::DisableThreadLibraryCalls(h) };
                $crate::dll::call(module_path!(), $f);
            }
            1
        }
    };
    ($f:path) => {
        #[no_mangle]
        pub extern "system" fn DllMain(h: $crate::win::HMODULE, reason: u32, _: *mut ::std::ffi::c_void) -> i32 {
            static ONCE: ::std::sync::Once = ::std::sync::Once::new();
            if reason == 1 {
                unsafe { $crate::win::DisableThreadLibraryCalls(h) };
                ONCE.call_once(|| $crate::dll::run(module_path!(), $f));
            }
            1
        }
    };
}

#[cfg(test)]
mod t {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static HITS: AtomicU32 = AtomicU32::new(0);

    fn body() {
        HITS.fetch_add(1, Ordering::SeqCst);
        if HITS.load(Ordering::SeqCst) == 1 {
            panic!("boom");
        }
    }

    mod entry {
        crate::dll_main!(super::body);
    }

    #[test]
    fn crash_filter_stays_ours() {
        log_crashes();
        assert!(keep_crash_filter());
        let before = PREV_FILTER.load(Ordering::Relaxed);
        unsafe { win::SetUnhandledExceptionFilter(0x1234) };
        assert_eq!(PREV_FILTER.load(Ordering::Relaxed), 0x1234);
        PREV_FILTER.store(before, Ordering::Relaxed);
    }

    #[test]
    fn samples_a_busy_thread() {
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s2 = stop.clone();
        let t = std::thread::spawn(move || {
            tx.send(unsafe { win::GetCurrentThreadId() }).unwrap();
            while !s2.load(Ordering::Relaxed) {
                std::hint::spin_loop();
            }
        });
        let id = rx.recv().unwrap();
        let s = sample(id).unwrap();
        assert!(threads().contains(&id));
        stop.store(true, Ordering::Relaxed);
        t.join().unwrap();
        assert!(s.starts_with("at 0x") && s.contains("stack: "), "{s}");
        assert!(!hung(id));
    }

    #[test]
    fn paths_log_entry() {
        assert_eq!(exe_path().extension().and_then(|e| e.to_str()), Some("exe"));
        assert!(exe_dir().is_dir());
        assert_eq!(self_path(), exe_path());
        assert!(module_at(0x10).is_null());
        assert_eq!(stamp().len(), 23);
        let p = std::env::temp_dir().join(format!("ac_log_{}.txt", std::process::id()));
        log_to(&p, false).unwrap();
        crate::log!("hello {}", 42);
        let k = win::module("kernel32.dll");
        assert_eq!(module_path(k).and_then(|p| p.file_name().map(|n| n.to_ascii_lowercase())), Some("kernel32.dll".into()));
        assert_eq!(entry::DllMain(std::ptr::null_mut(), 1, std::ptr::null_mut()), 1);
        assert_eq!(entry::DllMain(std::ptr::null_mut(), 1, std::ptr::null_mut()), 1);
        assert_eq!(entry::DllMain(std::ptr::null_mut(), 0, std::ptr::null_mut()), 1);
        for _ in 0..200 {
            if std::fs::read_to_string(&p).unwrap_or_default().contains("panicked: boom") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(HITS.load(Ordering::SeqCst), 1);
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("] hello 42\r\n") && s.contains("panicked: boom"), "{s}");
        *LOG.lock().unwrap() = None;
        std::fs::remove_file(&p).unwrap();
    }
}
