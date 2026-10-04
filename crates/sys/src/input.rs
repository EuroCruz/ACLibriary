use crate::win::{self, CallWindowProcA, GetAsyncKeyState, GetCurrentProcessId, GetRawInputDeviceInfoW, GetRawInputDeviceList, GetForegroundWindow, GetWindowThreadProcessId, SetWindowLongPtr, GWLP_WNDPROC, HWND};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

pub const WM_KEYDOWN: u32 = 0x100;
pub const WM_KEYUP: u32 = 0x101;
pub const WM_CHAR: u32 = 0x102;
pub const WM_SYSKEYDOWN: u32 = 0x104;
pub const WM_SYSKEYUP: u32 = 0x105;
pub const WM_MOUSEMOVE: u32 = 0x200;
pub const WM_MOUSEWHEEL: u32 = 0x20a;

const NAMES: &[(i32, &str)] = &[
    (0x01, "LMB"), (0x02, "RMB"), (0x04, "MMB"), (0x05, "Mouse4"), (0x06, "Mouse5"),
    (0x08, "Backspace"), (0x09, "Tab"), (0x0d, "Enter"), (0x10, "Shift"), (0x11, "Ctrl"), (0x12, "Alt"),
    (0x13, "Pause"), (0x14, "CapsLock"), (0x1b, "Esc"), (0x20, "Space"), (0x21, "PageUp"), (0x22, "PageDown"),
    (0x23, "End"), (0x24, "Home"), (0x25, "Left"), (0x26, "Up"), (0x27, "Right"), (0x28, "Down"),
    (0x2c, "PrintScreen"), (0x2d, "Insert"), (0x2e, "Delete"), (0x5b, "LWin"), (0x5c, "RWin"), (0x5d, "Apps"),
    (0x6a, "NumMul"), (0x6b, "NumAdd"), (0x6d, "NumSub"), (0x6e, "NumDec"), (0x6f, "NumDiv"),
    (0x90, "NumLock"), (0x91, "ScrollLock"), (0xa0, "LShift"), (0xa1, "RShift"), (0xa2, "LCtrl"), (0xa3, "RCtrl"),
    (0xa4, "LAlt"), (0xa5, "RAlt"), (0xba, ";"), (0xbb, "="), (0xbc, ","), (0xbd, "-"), (0xbe, "."), (0xbf, "/"),
    (0xc0, "`"), (0xdb, "["), (0xdc, "\\"), (0xdd, "]"), (0xde, "'"),
];

const ALIASES: &[(&str, i32)] = &[
    ("ESCAPE", 0x1b), ("RETURN", 0x0d), ("CONTROL", 0x11), ("MENU", 0x12), ("BACK", 0x08), ("DEL", 0x2e),
    ("INS", 0x2d), ("PGUP", 0x21), ("PGDN", 0x22), ("TILDE", 0xc0), ("GRAVE", 0xc0), ("MOUSE1", 0x01),
    ("MOUSE2", 0x02), ("MOUSE3", 0x04), ("MIDDLEMOUSE", 0x04), ("PRINT", 0x2c), ("WIN", 0x5b),
];

pub fn code(name: &str) -> Option<i32> {
    let n = name.trim();
    let u = n.to_ascii_uppercase();
    if let Some(h) = u.strip_prefix("0X") {
        return i32::from_str_radix(h, 16).ok().filter(|k| (1..256).contains(k));
    }
    if let Some(f) = u.strip_prefix('F').and_then(|d| d.parse::<i32>().ok()).filter(|d| (1..=24).contains(d)) {
        return Some(0x6f + f);
    }
    for p in ["NUMPAD", "NUM"] {
        if let Some(d) = u.strip_prefix(p).and_then(|d| d.parse::<i32>().ok()).filter(|d| (0..=9).contains(d)) {
            return Some(0x60 + d);
        }
    }
    let mut c = u.chars();
    if let (Some(ch), None) = (c.next(), c.next()) {
        if ch.is_ascii_alphanumeric() {
            return Some(ch as i32);
        }
    }
    NAMES.iter().find(|(_, s)| s.eq_ignore_ascii_case(n)).map(|x| x.0).or_else(|| ALIASES.iter().find(|(s, _)| *s == u).map(|x| x.1))
}

pub fn name(k: i32) -> String {
    match k {
        0x30..=0x39 | 0x41..=0x5a => (k as u8 as char).to_string(),
        0x60..=0x69 => format!("Num{}", k - 0x60),
        0x70..=0x87 => format!("F{}", k - 0x6f),
        _ => NAMES.iter().find(|x| x.0 == k).map_or_else(|| format!("0x{k:02X}"), |x| x.1.to_string()),
    }
}

pub fn scan_name(dik: u8) -> Option<&'static str> {
    const ROW: [&str; 0x59] = [
        "", "Esc", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "=", "Backspace", "Tab", "Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P", "[", "]", "Enter", "Ctrl",
        "A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'", "`", "Shift", "\\", "Z", "X", "C", "V", "B", "N", "M", ",", ".", "/", "Right Shift", "Num *", "Alt", "Space",
        "Caps Lock", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "Num Lock", "Scroll Lock", "Num 7", "Num 8", "Num 9", "Num -", "Num 4", "Num 5", "Num 6",
        "Num +", "Num 1", "Num 2", "Num 3", "Num 0", "Num Del", "Sys Req", "", "\\", "F11", "F12",
    ];
    const EXT: &[(u8, &str)] = &[
        (0x7c, "F13"), (0x7d, "F14"), (0x7e, "F15"), (0x7f, "F16"), (0x9c, "Num Enter"), (0x9d, "Right Ctrl"), (0xb5, "Num /"), (0xb7, "Prnt Scrn"), (0xb8, "Right Alt"),
        (0xc5, "Pause"), (0xc7, "Home"), (0xc8, "Up"), (0xc9, "Page Up"), (0xcb, "Left"), (0xcd, "Right"), (0xcf, "End"), (0xd0, "Down"), (0xd1, "Page Down"),
        (0xd2, "Insert"), (0xd3, "Delete"), (0xdb, "Left Windows"), (0xdc, "Right Windows"), (0xdd, "Application"),
    ];
    ROW.get(dik as usize).copied().filter(|n| !n.is_empty()).or_else(|| EXT.iter().find(|e| e.0 == dik).map(|e| e.1))
}

pub fn down(k: i32) -> bool {
    unsafe { GetAsyncKeyState(k) < 0 }
}

pub fn focused() -> bool {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) != 0 && pid == GetCurrentProcessId() }
}

#[derive(Clone)]
pub struct Keys {
    cur: [bool; 256],
    prev: [bool; 256],
}

impl Default for Keys {
    fn default() -> Keys {
        Keys { cur: [false; 256], prev: [false; 256] }
    }
}

impl Keys {
    pub fn new() -> Keys {
        Keys::default()
    }

    pub fn poll(&mut self) {
        let mut s = [false; 256];
        for (k, v) in s.iter_mut().enumerate().skip(1) {
            *v = down(k as i32);
        }
        self.feed(s);
    }

    pub fn feed(&mut self, s: [bool; 256]) {
        self.prev = std::mem::replace(&mut self.cur, s);
    }

    fn at(v: &[bool; 256], k: i32) -> bool {
        usize::try_from(k).ok().and_then(|k| v.get(k)).copied().unwrap_or(false)
    }

    pub fn down(&self, k: i32) -> bool {
        Self::at(&self.cur, k)
    }

    pub fn hit(&self, k: i32) -> bool {
        Self::at(&self.cur, k) && !Self::at(&self.prev, k)
    }

    pub fn up(&self, k: i32) -> bool {
        !Self::at(&self.cur, k) && Self::at(&self.prev, k)
    }

    pub fn any_hit(&self) -> Option<i32> {
        (1..256).filter(|&k| !(0x10..=0x12).contains(&k) && !(0xa0..=0xa5).contains(&k)).find(|&k| self.hit(k))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Combo {
    pub key: i32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Combo {
    pub fn parse(s: &str) -> Option<Combo> {
        let mut c = Combo::default();
        let parts: Vec<&str> = if s.trim().ends_with("++") { s.trim().trim_end_matches('+').split('+').chain(["="]).collect() } else { s.split('+').collect() };
        let (last, mods) = parts.split_last()?;
        for m in mods {
            match m.trim().to_ascii_uppercase().as_str() {
                "CTRL" | "CONTROL" => c.ctrl = true,
                "SHIFT" => c.shift = true,
                "ALT" => c.alt = true,
                _ => return None,
            }
        }
        c.key = code(last)?;
        Some(c)
    }

    pub fn hit(&self, k: &Keys) -> bool {
        k.hit(self.key) && k.down(0x11) == self.ctrl && k.down(0x10) == self.shift && k.down(0x12) == self.alt
    }
}

impl std::fmt::Display for Combo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (on, s) in [(self.ctrl, "Ctrl+"), (self.shift, "Shift+"), (self.alt, "Alt+")] {
            if on {
                f.write_str(s)?;
            }
        }
        f.write_str(&name(self.key))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Up = 0x1,
    Down = 0x2,
    Left = 0x4,
    Right = 0x8,
    Start = 0x10,
    Back = 0x20,
    L3 = 0x40,
    R3 = 0x80,
    LB = 0x100,
    RB = 0x200,
    A = 0x1000,
    B = 0x2000,
    X = 0x4000,
    Y = 0x8000,
    LT = 0x10000,
    RT = 0x20000,
}

const BTNS: [(Btn, &str); 16] = [
    (Btn::Up, "Up"), (Btn::Down, "Down"), (Btn::Left, "Left"), (Btn::Right, "Right"), (Btn::Start, "Start"),
    (Btn::Back, "Back"), (Btn::L3, "L3"), (Btn::R3, "R3"), (Btn::LB, "LB"), (Btn::RB, "RB"), (Btn::A, "A"),
    (Btn::B, "B"), (Btn::X, "X"), (Btn::Y, "Y"), (Btn::LT, "LT"), (Btn::RT, "RT"),
];

impl Btn {
    pub fn parse(s: &str) -> Option<Btn> {
        let s = s.trim();
        BTNS.iter().find(|x| x.1.eq_ignore_ascii_case(s)).map(|x| x.0).or(match s.to_ascii_uppercase().as_str() {
            "SELECT" => Some(Btn::Back),
            "DPADUP" => Some(Btn::Up),
            "DPADDOWN" => Some(Btn::Down),
            "DPADLEFT" => Some(Btn::Left),
            "DPADRIGHT" => Some(Btn::Right),
            _ => None,
        })
    }

    pub fn name(self) -> &'static str {
        BTNS.iter().find(|x| x.0 == self).map_or("", |x| x.1)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Raw {
    pub packet: u32,
    pub buttons: u16,
    pub lt: u8,
    pub rt: u8,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
}

type GetState = unsafe extern "system" fn(u32, *mut Raw) -> u32;
type SetState = unsafe extern "system" fn(u32, *const [u16; 2]) -> u32;

fn xinput() -> Option<(GetState, SetState)> {
    static F: OnceLock<Option<(usize, usize)>> = OnceLock::new();
    let &(g, s) = F
        .get_or_init(|| {
            ["xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"].iter().find_map(|d| {
                let m = win::load(d);
                let (g, s) = (win::proc_addr(m, "XInputGetState"), win::proc_addr(m, "XInputSetState"));
                (!m.is_null() && g != 0 && s != 0).then_some((g, s))
            })
        })
        .as_ref()?;
    Some(unsafe { (std::mem::transmute::<usize, GetState>(g), std::mem::transmute::<usize, SetState>(s)) })
}

pub fn xinput_ids() -> Vec<(u16, u16)> {
    let mut n = 0u32;
    let sz = std::mem::size_of::<[usize; 2]>() as u32;
    unsafe { GetRawInputDeviceList(std::ptr::null_mut(), &mut n, sz) };
    let mut list = vec![[0usize; 2]; n as usize];
    let got = unsafe { GetRawInputDeviceList(list.as_mut_ptr(), &mut n, sz) };
    if got == u32::MAX {
        return Vec::new();
    }
    let mut ids = Vec::new();
    for d in &list[..got as usize] {
        let mut len = 0u32;
        unsafe { GetRawInputDeviceInfoW(d[0] as _, 0x2000_0007, std::ptr::null_mut(), &mut len) };
        let mut b = vec![0u16; len as usize + 1];
        if unsafe { GetRawInputDeviceInfoW(d[0] as _, 0x2000_0007, b.as_mut_ptr().cast(), &mut len) } == u32::MAX {
            continue;
        }
        if let Some(id) = xinput_id(&String::from_utf16_lossy(&b)).filter(|id| !ids.contains(id)) {
            ids.push(id);
        }
    }
    ids
}

pub fn xinput_id(path: &str) -> Option<(u16, u16)> {
    let p = path.to_ascii_uppercase();
    let hex = |k: &str| p.find(k).and_then(|i| p.get(i + k.len()..i + k.len() + 4)).and_then(|h| u16::from_str_radix(h, 16).ok());
    p.contains("IG_").then_some(())?;
    Some((hex("VID_")?, hex("PID_")?))
}

pub fn stick(x: i16, y: i16, dz: f32) -> (f32, f32) {
    let (x, y) = (x as f32, y as f32);
    let m = (x * x + y * y).sqrt();
    if m <= dz || m == 0.0 {
        return (0.0, 0.0);
    }
    let k = ((m - dz) / (32767.0 - dz)).min(1.0) / m;
    ((x * k).clamp(-1.0, 1.0), (y * k).clamp(-1.0, 1.0))
}

pub fn trigger(v: u8, dz: u8) -> f32 {
    if v <= dz { 0.0 } else { (v - dz) as f32 / (255 - dz) as f32 }
}

#[derive(Clone, Debug)]
pub struct Pad {
    pub id: u32,
    pub ok: bool,
    pub cur: Raw,
    pub prev: Raw,
    pub ldz: f32,
    pub rdz: f32,
    pub tdz: u8,
}

impl Pad {
    pub fn new(id: u32) -> Pad {
        Pad { id, ok: false, cur: Raw::default(), prev: Raw::default(), ldz: 7849.0, rdz: 8689.0, tdz: 30 }
    }

    pub fn available() -> bool {
        xinput().is_some()
    }

    pub fn connected() -> Vec<u32> {
        (0..4).filter(|&id| Pad::new(id).poll()).collect()
    }

    pub fn poll(&mut self) -> bool {
        let mut r = Raw::default();
        self.ok = xinput().is_some_and(|(g, _)| unsafe { g(self.id, &mut r) } == 0);
        self.feed(if self.ok { r } else { Raw::default() });
        self.ok
    }

    pub fn feed(&mut self, r: Raw) {
        self.prev = std::mem::replace(&mut self.cur, r);
    }

    fn mask(&self, r: &Raw) -> u32 {
        r.buttons as u32 | if r.lt > self.tdz { 0x10000 } else { 0 } | if r.rt > self.tdz { 0x20000 } else { 0 }
    }

    pub fn down(&self, b: Btn) -> bool {
        self.mask(&self.cur) & b as u32 != 0
    }

    pub fn hit(&self, b: Btn) -> bool {
        self.down(b) && self.mask(&self.prev) & b as u32 == 0
    }

    pub fn up(&self, b: Btn) -> bool {
        !self.down(b) && self.mask(&self.prev) & b as u32 != 0
    }

    pub fn left(&self) -> (f32, f32) {
        stick(self.cur.lx, self.cur.ly, self.ldz)
    }

    pub fn right(&self) -> (f32, f32) {
        stick(self.cur.rx, self.cur.ry, self.rdz)
    }

    pub fn lt(&self) -> f32 {
        trigger(self.cur.lt, self.tdz)
    }

    pub fn rt(&self) -> f32 {
        trigger(self.cur.rt, self.tdz)
    }

    pub fn vibrate(&self, l: f32, r: f32) -> bool {
        let v = [(l.clamp(0.0, 1.0) * 65535.0) as u16, (r.clamp(0.0, 1.0) * 65535.0) as u16];
        xinput().is_some_and(|(_, s)| unsafe { s(self.id, &v) } == 0)
    }
}

pub type Filter = fn(HWND, u32, usize, isize) -> bool;

static PREV: AtomicUsize = AtomicUsize::new(0);
static FILTER: AtomicUsize = AtomicUsize::new(0);
static WND: AtomicUsize = AtomicUsize::new(0);

extern "system" fn wndproc(w: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    let f = FILTER.load(Ordering::Acquire);
    if f != 0 && unsafe { std::mem::transmute::<usize, Filter>(f) }(w, msg, wp, lp) {
        return 0;
    }
    unsafe { CallWindowProcA(PREV.load(Ordering::Acquire), w, msg, wp, lp) }
}

pub fn hook_wnd(w: HWND, f: Filter) -> bool {
    if w.is_null() || WND.load(Ordering::Acquire) != 0 {
        return false;
    }
    FILTER.store(f as usize, Ordering::Release);
    let old = unsafe { SetWindowLongPtr(w, GWLP_WNDPROC, wndproc as extern "system" fn(HWND, u32, usize, isize) -> isize as isize) };
    if old == 0 {
        FILTER.store(0, Ordering::Release);
        return false;
    }
    PREV.store(old as usize, Ordering::Release);
    WND.store(w as usize, Ordering::Release);
    true
}

pub fn unhook_wnd() -> bool {
    let w = WND.swap(0, Ordering::AcqRel);
    if w == 0 {
        return false;
    }
    unsafe { SetWindowLongPtr(w as HWND, GWLP_WNDPROC, PREV.load(Ordering::Acquire) as isize) };
    FILTER.store(0, Ordering::Release);
    true
}

pub fn chars(wp: usize, hi: &mut u16) -> Option<char> {
    let u = wp as u16;
    if (0xd800..0xdc00).contains(&u) {
        *hi = u;
        return None;
    }
    if (0xdc00..0xe000).contains(&u) {
        let h = std::mem::take(hi);
        return char::decode_utf16([h, u]).next()?.ok();
    }
    char::from_u32(u as u32)
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::win::{cstr, CreateWindowExA, DestroyWindow, SendMessageA};
    use std::sync::atomic::AtomicU32;

    #[test]
    fn names() {
        for (s, k) in [("F1", 0x70), (" f12 ", 0x7b), ("F24", 0x87), ("w", 0x57), ("7", 0x37), ("Shift", 0x10), ("numpad5", 0x65), ("Num0", 0x60), ("Space", 0x20), ("`", 0xc0), ("tilde", 0xc0), ("Esc", 0x1b), ("escape", 0x1b), ("0x41", 0x41), ("[", 0xdb), ("RCtrl", 0xa3)] {
            assert_eq!(code(s), Some(k), "{s}");
        }
        for s in ["nonsense", "F25", "F0", "Num10", "0x100", ""] {
            assert_eq!(code(s), None, "{s}");
        }
        for k in 1..256 {
            let n = name(k);
            assert_eq!(code(&n), Some(k), "{k:#x} {n}");
        }
        assert_eq!(name(0x70), "F1");
        assert_eq!(name(0x65), "Num5");
    }

    #[test]
    fn scan_names_are_us_layout() {
        assert_eq!((scan_name(0x10), scan_name(0x39), scan_name(0x2b), scan_name(0x45), scan_name(0xc5), scan_name(0x9d)), (Some("Q"), Some("Space"), Some("\\"), Some("Num Lock"), Some("Pause"), Some("Right Ctrl")));
        assert_eq!((scan_name(0), scan_name(0x55), scan_name(0x80), scan_name(0xff)), (None, None, None, None));
        if unsafe { win::ActivateKeyboardLayout(0x0409_0409 as _, 0) }.is_null() {
            return;
        }
        for k in (1..=0xffu8).filter(|k| !matches!(k, 0x45 | 0xc5)) {
            if let Some(n) = scan_name(k) {
                let mut b = [0u16; 64];
                let l = unsafe { win::GetKeyNameTextW(((k as i32 & 0x7f) << 16) | ((k as i32 & 0x80) << 17), b.as_mut_ptr(), 64) };
                assert_eq!(String::from_utf16_lossy(&b[..l as usize]), n, "{k:#x}");
            }
        }
    }

    #[test]
    fn keys_and_combos() {
        let mut k = Keys::new();
        let mut s = [false; 256];
        s[0x41] = true;
        k.feed(s);
        assert!(k.down(0x41) && k.hit(0x41) && !k.up(0x41));
        assert_eq!(k.any_hit(), Some(0x41));
        k.feed(s);
        assert!(k.down(0x41) && !k.hit(0x41));
        assert_eq!(k.any_hit(), None);
        s[0x41] = false;
        k.feed(s);
        assert!(k.up(0x41) && !k.down(0x41));
        assert!(!k.down(-1) && !k.down(999));
        let c = Combo::parse("Ctrl+Shift+F5").unwrap();
        assert_eq!(c, Combo { key: 0x74, ctrl: true, shift: true, alt: false });
        assert_eq!(c.to_string(), "Ctrl+Shift+F5");
        assert_eq!(Combo::parse("ctrl++"), Some(Combo { key: 0xbb, ctrl: true, ..Combo::default() }));
        assert_eq!(Combo::parse("Meta+A"), None);
        assert_eq!(Combo::parse("Ctrl+"), None);
        s[0x11] = true;
        s[0x10] = true;
        k.feed(s);
        s[0x74] = true;
        k.feed(s);
        assert!(c.hit(&k));
        s[0x12] = true;
        k.feed(s);
        assert!(!c.hit(&k));
        assert_eq!(k.any_hit(), None);
    }

    #[test]
    fn xinput_paths() {
        assert_eq!(xinput_id(r"\\?\HID#VID_045E&PID_02FF&IG_00#7&1d&0&0000#{4d1e55b2}"), Some((0x045e, 0x02ff)));
        assert_eq!(xinput_id(r"\\?\hid#vid_054c&pid_09cc&mi_03#8&2a&0&0000#{4d1e55b2}"), None);
        assert!(xinput_ids().iter().all(|&(v, _)| v != 0));
        assert!(Pad::connected().iter().all(|&id| id < 4));
    }

    #[test]
    fn pad() {
        assert_eq!(stick(0, 0, 7849.0), (0.0, 0.0));
        assert_eq!(stick(5000, 5000, 7849.0), (0.0, 0.0));
        let (x, y) = stick(32767, 0, 7849.0);
        assert!((x - 1.0).abs() < 1e-6 && y == 0.0);
        let (x, y) = stick(-32768, -32768, 7849.0);
        assert!((x * x + y * y).sqrt() <= 1.0 + 1e-6 && x < 0.0 && y < 0.0);
        let (x, _) = stick(20308, 0, 7849.0);
        assert!((x - 0.5).abs() < 0.01);
        assert_eq!(trigger(30, 30), 0.0);
        assert_eq!(trigger(255, 30), 1.0);
        let mut p = Pad::new(0);
        p.feed(Raw { buttons: Btn::A as u16 | Btn::Up as u16, rt: 200, ..Raw::default() });
        assert!(p.hit(Btn::A) && p.hit(Btn::Up) && p.hit(Btn::RT) && !p.down(Btn::LT) && !p.down(Btn::B));
        p.feed(Raw { buttons: Btn::A as u16, ..Raw::default() });
        assert!(p.down(Btn::A) && !p.hit(Btn::A) && p.up(Btn::Up) && p.up(Btn::RT));
        for (b, n) in BTNS {
            assert_eq!(Btn::parse(n), Some(b));
            assert_eq!(b.name(), n);
        }
        assert_eq!(Btn::parse("select"), Some(Btn::Back));
        assert_eq!(Btn::parse("Z"), None);
        let mut q = Pad::new(3);
        if Pad::available() {
            q.poll();
        }
        assert_eq!(std::mem::size_of::<Raw>(), 16);
    }

    static SEEN: AtomicU32 = AtomicU32::new(0);

    fn filt(_: HWND, msg: u32, wp: usize, _: isize) -> bool {
        if msg == 0x401 {
            SEEN.store(wp as u32, Ordering::SeqCst);
            return true;
        }
        false
    }

    #[test]
    fn wnd_and_chars() {
        let w = unsafe { CreateWindowExA(0, cstr("STATIC").as_ptr().cast(), cstr("t").as_ptr().cast(), 0, 0, 0, 10, 10, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        assert!(!w.is_null());
        assert!(hook_wnd(w, filt));
        assert!(!hook_wnd(w, filt));
        assert_eq!(unsafe { SendMessageA(w, 0x401, 77, 0) }, 0);
        assert_eq!(SEEN.load(Ordering::SeqCst), 77);
        assert!(unhook_wnd());
        assert!(!unhook_wnd());
        unsafe { SendMessageA(w, 0x401, 5, 0) };
        assert_eq!(SEEN.load(Ordering::SeqCst), 77);
        unsafe { DestroyWindow(w) };
        let mut hi = 0;
        assert_eq!(chars('ж' as usize, &mut hi), Some('ж'));
        let u: Vec<u16> = "😀".encode_utf16().collect();
        assert_eq!(chars(u[0] as usize, &mut hi), None);
        assert_eq!(chars(u[1] as usize, &mut hi), Some('😀'));
        let _ = focused();
    }
}
