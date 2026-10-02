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

pub fn log_to(path: &Path, append: bool) -> std::io::Result<()> {
    let f = OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(path)?;
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
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

pub fn run(name: &'static str, f: fn()) {
    let _ = std::thread::Builder::new().name(name.into()).spawn(move || {
        if let Err(e) = std::panic::catch_unwind(f) {
            let m = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_default();
            log(&format!("{name} panicked: {m}"));
        }
    });
}

#[macro_export]
macro_rules! dll_main {
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
