pub mod d11;
pub mod d8;
pub mod d9;

use crate::win::{self, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GdiFlush, GetTextExtentPoint32W, Point, SelectObject, SetBkColor, SetBkMode, SetTextColor, TextOutW};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;

pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Atlas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
    pub line: f32,
    map: HashMap<char, Glyph>,
}

pub fn charset() -> impl Iterator<Item = char> {
    (0x20u32..0x7f).chain(0xa0..0x180).chain(0x400..0x460).chain([0x2013, 0x2014, 0x2018, 0x2019, 0x201c, 0x201d, 0x2026, 0x2116, 0x20ac, 0x2122]).filter_map(char::from_u32)
}

#[repr(C)]
struct BitmapInfo {
    size: u32,
    w: i32,
    h: i32,
    planes: u16,
    bits: u16,
    comp: u32,
    image: u32,
    xppm: i32,
    yppm: i32,
    used: u32,
    important: u32,
    colors: u32,
}

impl Atlas {
    pub fn new(face: &str, size: i32, bold: bool) -> Option<Atlas> {
        Atlas::with(face, size, bold, charset())
    }

    pub fn with(face: &str, size: i32, bold: bool, chars: impl Iterator<Item = char>) -> Option<Atlas> {
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return None;
            }
            let font = CreateFontW(-size, 0, 0, 0, if bold { 700 } else { 400 }, 0, 0, 0, 1, 0, 0, 4, 0, win::wide(face).as_ptr());
            let old = SelectObject(dc, font);
            let mut cells = Vec::new();
            for c in chars {
                let mut b = [0u16; 2];
                let s = c.encode_utf16(&mut b);
                let mut p = Point::default();
                GetTextExtentPoint32W(dc, s.as_ptr(), s.len() as i32, &mut p);
                if p.x > 0 {
                    cells.push((c, p.x, p.y));
                }
            }
            let line = cells.iter().map(|c| c.2).max().unwrap_or(size);
            let w = 512i32;
            let (mut x, mut y) = (4, 0);
            let mut pos = Vec::with_capacity(cells.len());
            for &(_, cw, _) in &cells {
                if x + cw + 1 > w {
                    x = 0;
                    y += line + 1;
                }
                pos.push((x, y));
                x += cw + 1;
            }
            let h = ((y + line + 1) as u32).next_power_of_two().max(4) as i32;
            let info = BitmapInfo { size: 40, w, h: -h, planes: 1, bits: 32, comp: 0, image: 0, xppm: 0, yppm: 0, used: 0, important: 0, colors: 0 };
            let mut bits: *mut c_void = null_mut();
            let bmp = CreateDIBSection(dc, (&info as *const BitmapInfo).cast(), 0, &mut bits, null_mut(), 0);
            let res = if bmp.is_null() || bits.is_null() {
                None
            } else {
                let ob = SelectObject(dc, bmp);
                SetBkMode(dc, 2);
                SetBkColor(dc, 0);
                SetTextColor(dc, 0xffffff);
                for (&(c, _, _), &(x, y)) in cells.iter().zip(&pos) {
                    let mut b = [0u16; 2];
                    let s = c.encode_utf16(&mut b);
                    TextOutW(dc, x, y, s.as_ptr(), s.len() as i32);
                }
                GdiFlush();
                let raw = std::slice::from_raw_parts(bits as *const u32, (w * h) as usize);
                let mut px: Vec<u32> = raw.iter().map(|&p| (p >> 16 & 255).max(p >> 8 & 255).max(p & 255) << 24 | 0xffffff).collect();
                for i in [0, 1, w as usize, w as usize + 1] {
                    px[i] = 0xffffffff;
                }
                SelectObject(dc, ob);
                DeleteObject(bmp);
                let (fw, fh) = (w as f32, h as f32);
                let map = cells.iter().zip(&pos).map(|(&(c, cw, ch), &(x, y))| (c, Glyph { u0: x as f32 / fw, v0: y as f32 / fh, u1: (x + cw) as f32 / fw, v1: (y + ch) as f32 / fh, w: cw as f32, h: ch as f32 })).collect();
                Some(Atlas { w: w as u32, h: h as u32, px, line: line as f32, map })
            };
            SelectObject(dc, old);
            DeleteObject(font);
            DeleteDC(dc);
            res
        }
    }

    pub fn glyph(&self, c: char) -> Option<&Glyph> {
        self.map.get(&c).or_else(|| self.map.get(&'?'))
    }

    pub fn white(&self) -> (f32, f32) {
        (1.0 / self.w as f32, 1.0 / self.h as f32)
    }

    pub fn measure(&self, s: &str) -> (f32, f32) {
        let mut w: f32 = 0.0;
        let (mut x, mut n) = (0.0, 1.0);
        for c in s.chars() {
            if c == '\n' {
                w = w.max(x);
                x = 0.0;
                n += 1.0;
            } else {
                x += self.glyph(c).map_or(0.0, |g| g.w);
            }
        }
        (w.max(x), n * self.line)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vtx {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub v: f32,
    pub c: u32,
}

#[derive(Default)]
pub struct Batch {
    pub v: Vec<Vtx>,
    white: (f32, f32),
}

impl Batch {
    pub fn new(a: &Atlas) -> Batch {
        Batch { v: Vec::new(), white: a.white() }
    }

    pub fn clear(&mut self) {
        self.v.clear();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn quad(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, u0: f32, v0: f32, u1: f32, v1: f32, c: u32) {
        let p = |x, y, u, v| Vtx { x, y, u, v, c };
        let (a, b, d, e) = (p(x0, y0, u0, v0), p(x1, y0, u1, v0), p(x1, y1, u1, v1), p(x0, y1, u0, v1));
        self.v.extend([a, b, d, a, d, e]);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32) {
        let (u, v) = self.white;
        self.quad(x, y, x + w, y + h, u, v, u, v, c);
    }

    pub fn frame(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, c: u32) {
        self.rect(x, y, w, t, c);
        self.rect(x, y + h - t, w, t, c);
        self.rect(x, y + t, t, h - 2.0 * t, c);
        self.rect(x + w - t, y + t, t, h - 2.0 * t, c);
    }

    pub fn text(&mut self, a: &Atlas, x: f32, y: f32, s: &str, c: u32) -> f32 {
        let (mut cx, mut cy, mut w) = (x, y, 0.0f32);
        for ch in s.chars() {
            if ch == '\n' {
                w = w.max(cx - x);
                cx = x;
                cy += a.line;
                continue;
            }
            if let Some(g) = a.glyph(ch) {
                if ch != ' ' {
                    self.quad(cx, cy, cx + g.w, cy + g.h, g.u0, g.v0, g.u1, g.v1, c);
                }
                cx += g.w;
            }
        }
        w.max(cx - x)
    }

    pub fn shadow(&mut self, a: &Atlas, x: f32, y: f32, s: &str, c: u32, sh: u32) -> f32 {
        self.text(a, x + 1.0, y + 1.0, s, sh);
        self.text(a, x, y, s, c)
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn atlas_and_batch() {
        let a = Atlas::new("Arial", 16, false).expect("atlas");
        assert_eq!(a.w, 512);
        assert!(a.h.is_power_of_two() && a.line >= 14.0);
        assert_eq!(a.px[0], 0xffffffff);
        let g = *a.glyph('W').unwrap();
        assert!(g.w > 5.0 && g.u1 > g.u0);
        assert!(a.glyph('Ж').is_some() && a.glyph('é').is_some());
        assert_eq!(a.glyph('\u{4e2d}'), a.glyph('?'));
        let x0 = (g.u0 * a.w as f32) as u32;
        let y0 = (g.v0 * a.h as f32) as u32;
        let ink = (y0..y0 + g.h as u32).flat_map(|y| (x0..x0 + g.w as u32).map(move |x| (x, y))).filter(|&(x, y)| a.px[(y * a.w + x) as usize] >> 24 > 128).count();
        assert!(ink > 10, "{ink}");
        let (w, h) = a.measure("WW\nW");
        assert_eq!((w, h), (2.0 * g.w, 2.0 * a.line));
        let mut b = Batch::new(&a);
        b.rect(1.0, 2.0, 3.0, 4.0, rgba(255, 0, 0, 255));
        assert_eq!(b.v.len(), 6);
        assert_eq!((b.v[2].x, b.v[2].y, b.v[2].c), (4.0, 6.0, 0xffff0000));
        b.frame(0.0, 0.0, 10.0, 10.0, 1.0, 0);
        assert_eq!(b.v.len(), 30);
        let n = b.v.len();
        assert_eq!(b.text(&a, 0.0, 0.0, "W W", 0), 2.0 * g.w + a.glyph(' ').unwrap().w);
        assert_eq!(b.v.len(), n + 12);
        b.clear();
        assert!(b.v.is_empty());
    }
}
