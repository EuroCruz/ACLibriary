use super::bc::{block_bytes, decode, encode, Bc};
use super::Image;
use ac_core::{bad, tag, Endian, Reader, Res, Writer};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    Bc(Bc),
    Rgba8,
    Bgra8,
    Bgrx8,
    R8,
    A8,
    Mask { bits: u8, m: [u32; 4], lum: bool },
    Float { ch: u8, half: bool },
}

impl Fmt {
    pub fn size(self, w: u32, h: u32) -> usize {
        match self {
            Fmt::Bc(b) => (w.div_ceil(4) * h.div_ceil(4)) as usize * block_bytes(b),
            _ => (w * h) as usize * self.bpp(),
        }
    }

    fn bpp(self) -> usize {
        match self {
            Fmt::Rgba8 | Fmt::Bgra8 | Fmt::Bgrx8 => 4,
            Fmt::R8 | Fmt::A8 => 1,
            Fmt::Mask { bits, .. } => bits as usize / 8,
            Fmt::Float { ch, half } => ch as usize * if half { 2 } else { 4 },
            Fmt::Bc(_) => 0,
        }
    }

    fn masks(self) -> ([u32; 4], bool) {
        match self {
            Fmt::Rgba8 => ([0xff, 0xff00, 0xff_0000, 0xff00_0000], false),
            Fmt::Bgra8 => ([0xff_0000, 0xff00, 0xff, 0xff00_0000], false),
            Fmt::Bgrx8 => ([0xff_0000, 0xff00, 0xff, 0], false),
            Fmt::R8 => ([0xff, 0, 0, 0], false),
            Fmt::A8 => ([0, 0, 0, 0xff], false),
            Fmt::Mask { m, lum, .. } => (m, lum),
            Fmt::Float { .. } => ([0; 4], false),
            Fmt::Bc(_) => ([0; 4], false),
        }
    }

    pub fn decode(self, d: &[u8], w: u32, h: u32) -> Res<Image> {
        if let Fmt::Bc(b) = self {
            return decode(b, d, w, h);
        }
        if let Fmt::Float { ch, half } = self {
            if d.len() < self.size(w, h) {
                return bad("dds surface too short");
            }
            let n = if half { 2 } else { 4 };
            let f = |c: &[u8]| if half { f16(u16::from_le_bytes([c[0], c[1]])) } else { f32::from_le_bytes(c.try_into().unwrap()) };
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let px = d
                .chunks_exact(n * ch as usize)
                .take((w * h) as usize)
                .map(|c| {
                    let v: Vec<f32> = c.chunks_exact(n).map(f).collect();
                    match ch {
                        1 => [q(v[0]); 3].into_iter().chain([255]).collect::<Vec<_>>(),
                        2 => vec![q(v[0]), q(v[1]), 0, 255],
                        _ => vec![q(v[0]), q(v[1]), q(v[2]), q(v[3])],
                    }
                    .try_into()
                    .unwrap()
                })
                .collect();
            return Ok(Image { w, h, px });
        }
        let n = self.bpp();
        if n == 0 || d.len() < self.size(w, h) {
            return bad("dds surface too short");
        }
        let (m, lum) = self.masks();
        let get = |v: u32, k: usize, def: u8| -> u8 {
            if m[k] == 0 {
                return def;
            }
            let s = m[k].trailing_zeros();
            let max = (m[k] >> s) as u64;
            (((v & m[k]) >> s) as u64 * 255 / max) as u8
        };
        let px = d
            .chunks_exact(n)
            .take((w * h) as usize)
            .map(|c| {
                let v = c.iter().rev().fold(0u32, |a, &b| a << 8 | b as u32);
                if lum {
                    let l = get(v, 0, 0);
                    [l, l, l, get(v, 3, 255)]
                } else {
                    [get(v, 0, 0), get(v, 1, 0), get(v, 2, 0), get(v, 3, 255)]
                }
            })
            .collect();
        Ok(Image { w, h, px })
    }

    pub fn encode(self, img: &Image) -> Res<Vec<u8>> {
        if let Fmt::Bc(b) = self {
            return encode(b, img);
        }
        if let Fmt::Float { ch, half } = self {
            let mut o = Vec::with_capacity(img.px.len() * self.bpp());
            for p in &img.px {
                for &c in &p[..ch.min(4) as usize] {
                    let v = c as f32 / 255.0;
                    if half {
                        o.extend(to_f16(v).to_le_bytes());
                    } else {
                        o.extend(v.to_le_bytes());
                    }
                }
            }
            return Ok(o);
        }
        let (m, lum) = self.masks();
        let n = self.bpp();
        let put = |c: u8, k: usize| -> u32 {
            if m[k] == 0 {
                return 0;
            }
            let s = m[k].trailing_zeros();
            let max = m[k] >> s;
            ((c as u32 * max + 127) / 255) << s
        };
        let mut o = Vec::with_capacity(img.px.len() * n);
        for p in &img.px {
            let v = if lum {
                let l = ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8;
                put(l, 0) | put(p[3], 3)
            } else {
                put(p[0], 0) | put(p[1], 1) | put(p[2], 2) | put(p[3], 3)
            };
            o.extend(&v.to_le_bytes()[..n]);
        }
        Ok(o)
    }
}

pub fn f16(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) as u32, (h >> 10 & 31) as u32, (h & 1023) as u32);
    let v = match e {
        0 => m as f32 / 16_777_216.0,
        31 => if m == 0 { f32::INFINITY } else { f32::NAN },
        _ => f32::from_bits((e + 112) << 23 | m << 13),
    };
    if s == 1 { -v } else { v }
}

pub fn to_f16(v: f32) -> u16 {
    let b = v.to_bits();
    let s = (b >> 16 & 0x8000) as u16;
    let e = (b >> 23 & 255) as i32 - 112;
    let m = b & 0x7f_ffff;
    if v.is_nan() {
        return s | 0x7e00;
    }
    if e >= 31 {
        return s | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return s;
        }
        let m = (m | 0x80_0000) >> (1 - e);
        return s | ((m + 0x1000) >> 13) as u16;
    }
    let r = ((e as u32) << 10 | m >> 13) + ((m >> 12) & 1);
    s | r.min(0x7c00) as u16
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dds {
    pub w: u32,
    pub h: u32,
    pub depth: u32,
    pub mips: u32,
    pub layers: u32,
    pub cube: bool,
    pub fmt: Fmt,
    pub data: Vec<u8>,
}

fn dxgi(f: u32) -> Option<Fmt> {
    Some(match f {
        71 | 72 => Fmt::Bc(Bc::Bc1),
        74 | 75 => Fmt::Bc(Bc::Bc2),
        77 | 78 => Fmt::Bc(Bc::Bc3),
        80 | 81 => Fmt::Bc(Bc::Bc4),
        83 | 84 => Fmt::Bc(Bc::Bc5),
        98 | 99 => Fmt::Bc(Bc::Bc7),
        28 | 29 => Fmt::Rgba8,
        87 | 91 => Fmt::Bgra8,
        88 | 93 => Fmt::Bgrx8,
        61 => Fmt::R8,
        65 => Fmt::A8,
        2 => Fmt::Float { ch: 4, half: false },
        10 => Fmt::Float { ch: 4, half: true },
        16 => Fmt::Float { ch: 2, half: false },
        34 => Fmt::Float { ch: 2, half: true },
        41 => Fmt::Float { ch: 1, half: false },
        54 => Fmt::Float { ch: 1, half: true },
        _ => return None,
    })
}

fn to_dxgi(f: Fmt) -> u32 {
    match f {
        Fmt::Bc(Bc::Bc1) => 71,
        Fmt::Bc(Bc::Bc2) => 74,
        Fmt::Bc(Bc::Bc3) => 77,
        Fmt::Bc(Bc::Bc4) => 80,
        Fmt::Bc(Bc::Bc5) => 83,
        Fmt::Bc(Bc::Bc7) => 98,
        Fmt::Rgba8 => 28,
        Fmt::Bgra8 => 87,
        Fmt::Bgrx8 => 88,
        Fmt::R8 => 61,
        Fmt::A8 => 65,
        Fmt::Mask { .. } => 0,
        Fmt::Float { ch, half } => [[41, 16, 0, 2], [54, 34, 0, 10]][half as usize][ch as usize - 1],
    }
}

fn fourcc(c: u32) -> Option<Fmt> {
    let b = |s: &[u8; 4]| tag(s);
    Some(match c {
        x if x == b(b"DXT1") => Fmt::Bc(Bc::Bc1),
        x if x == b(b"DXT2") || x == b(b"DXT3") => Fmt::Bc(Bc::Bc2),
        x if x == b(b"DXT4") || x == b(b"DXT5") => Fmt::Bc(Bc::Bc3),
        x if x == b(b"ATI1") || x == b(b"BC4U") || x == b(b"BC4S") => Fmt::Bc(Bc::Bc4),
        x if x == b(b"ATI2") || x == b(b"BC5U") || x == b(b"BC5S") => Fmt::Bc(Bc::Bc5),
        116 => Fmt::Float { ch: 4, half: false },
        113 => Fmt::Float { ch: 4, half: true },
        115 => Fmt::Float { ch: 2, half: false },
        112 => Fmt::Float { ch: 2, half: true },
        114 => Fmt::Float { ch: 1, half: false },
        111 => Fmt::Float { ch: 1, half: true },
        _ => return None,
    })
}

impl Dds {
    pub fn parse(d: &[u8]) -> Res<Dds> {
        let mut r = Reader::new(d, Endian::Le);
        r.magic(b"DDS ")?;
        if r.u32()? != 124 {
            return bad("bad dds header size");
        }
        let (_flags, h, w, _pitch, depth, mips) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        r.skip(44)?;
        let (_pfs, pff, cc, bits) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        let m = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        let (_caps, caps2) = (r.u32()?, r.u32()?);
        r.skip(12)?;
        let (mut layers, mut cube) = (1, caps2 & 0x200 != 0);
        let fmt = if pff & 4 != 0 && cc == tag(b"DX10") {
            let (f, _dim, misc, arr) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
            r.skip(4)?;
            cube = misc & 4 != 0;
            layers = arr.max(1);
            dxgi(f).ok_or(ac_core::Error::Msg(format!("unsupported dxgi format {f}")))?
        } else if pff & 4 != 0 {
            fourcc(cc).ok_or(ac_core::Error::Msg(format!("unsupported fourcc {:?}", String::from_utf8_lossy(&cc.to_le_bytes()))))?
        } else {
            let lum = pff & 0x20000 != 0;
            let m = [m[0], m[1], m[2], if pff & 3 != 0 { m[3] } else { 0 }];
            match (bits, m, lum) {
                (32, [0xff, 0xff00, 0xff_0000, 0xff00_0000], false) => Fmt::Rgba8,
                (32, [0xff_0000, 0xff00, 0xff, 0xff00_0000], false) => Fmt::Bgra8,
                (32, [0xff_0000, 0xff00, 0xff, 0], false) => Fmt::Bgrx8,
                (8 | 16 | 24 | 32, _, _) => Fmt::Mask { bits: bits as u8, m, lum },
                _ => return bad("unsupported dds pixel format"),
            }
        };
        if cube {
            layers *= 6;
        }
        let dds = Dds { w, h, depth: if caps2 & 0x20_0000 != 0 { depth.max(1) } else { 1 }, mips: mips.max(1), layers, cube, fmt, data: r.rest().to_vec() };
        if dds.data.len() < dds.total() {
            return bad("dds data too short");
        }
        Ok(dds)
    }

    fn dims(&self, mip: u32) -> (u32, u32, u32) {
        ((self.w >> mip).max(1), (self.h >> mip).max(1), (self.depth >> mip).max(1))
    }

    fn mip_size(&self, mip: u32) -> usize {
        let (w, h, d) = self.dims(mip);
        self.fmt.size(w, h) * d as usize
    }

    fn layer_size(&self) -> usize {
        (0..self.mips).map(|m| self.mip_size(m)).sum()
    }

    fn total(&self) -> usize {
        self.layer_size() * self.layers as usize
    }

    pub fn surface(&self, layer: u32, mip: u32) -> Option<&[u8]> {
        if layer >= self.layers || mip >= self.mips {
            return None;
        }
        let at = layer as usize * self.layer_size() + (0..mip).map(|m| self.mip_size(m)).sum::<usize>();
        self.data.get(at..at + self.mip_size(mip))
    }

    pub fn image(&self, layer: u32, mip: u32) -> Res<Image> {
        let (w, h, _) = self.dims(mip);
        self.fmt.decode(self.surface(layer, mip).ok_or(ac_core::Error::Bad("no such dds surface"))?, w, h)
    }

    pub fn from_images(fmt: Fmt, layers: &[Vec<Image>], cube: bool) -> Res<Dds> {
        let first = layers.first().and_then(|l| l.first()).ok_or(ac_core::Error::Bad("no images"))?;
        let (w, h, mips) = (first.w, first.h, layers[0].len() as u32);
        let mut data = Vec::new();
        for l in layers {
            if l.len() as u32 != mips {
                return bad("mip count mismatch");
            }
            for (m, i) in l.iter().enumerate() {
                if (i.w, i.h) != ((w >> m).max(1), (h >> m).max(1)) {
                    return bad("mip size mismatch");
                }
                data.extend(fmt.encode(i)?);
            }
        }
        Ok(Dds { w, h, depth: 1, mips, layers: layers.len() as u32, cube, fmt, data })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut o = Writer::new(Endian::Le);
        let dx10 = matches!(self.fmt, Fmt::Bc(Bc::Bc7) | Fmt::R8 | Fmt::Float { .. }) || (self.layers > 1 && !(self.cube && self.layers == 6));
        let bc = matches!(self.fmt, Fmt::Bc(_));
        let mut flags = 0x1007 | if bc { 0x8_0000 } else { 8 };
        if self.mips > 1 {
            flags |= 0x2_0000;
        }
        let pitch = if bc { self.fmt.size(self.w, self.h) } else { self.w as usize * self.fmt.bpp() };
        o.bytes(b"DDS ").u32(124).u32(flags).u32(self.h).u32(self.w).u32(pitch as u32).u32(0).u32(self.mips).pad(44);
        let (m, lum) = self.fmt.masks();
        let cc = match (dx10, self.fmt) {
            (true, _) => Some(*b"DX10"),
            (_, Fmt::Bc(Bc::Bc1)) => Some(*b"DXT1"),
            (_, Fmt::Bc(Bc::Bc2)) => Some(*b"DXT3"),
            (_, Fmt::Bc(Bc::Bc3)) => Some(*b"DXT5"),
            (_, Fmt::Bc(Bc::Bc4)) => Some(*b"ATI1"),
            (_, Fmt::Bc(_)) => Some(*b"ATI2"),
            _ => None,
        };
        o.u32(32);
        match cc {
            Some(c) => {
                o.u32(4).bytes(&c).pad(20);
            }
            None => {
                let pf = if lum { 0x2_0000 } else if m[0] | m[1] | m[2] == 0 { 2 } else { 0x40 } | if m[3] != 0 && m[0] | m[1] | m[2] != 0 { 1 } else { 0 };
                o.u32(pf).u32(0).u32(self.fmt.bpp() as u32 * 8).u32(m[0]).u32(m[1]).u32(m[2]).u32(m[3]);
            }
        }
        let complex = self.mips > 1 || self.cube || self.layers > 1;
        o.u32(0x1000 | if complex { 8 } else { 0 } | if self.mips > 1 { 0x40_0000 } else { 0 });
        o.u32(if self.cube { 0xFE00 } else { 0 }).pad(12);
        if dx10 {
            let arr = if self.cube { self.layers / 6 } else { self.layers };
            o.u32(to_dxgi(self.fmt)).u32(3).u32(if self.cube { 4 } else { 0 }).u32(arr).u32(0);
        }
        o.bytes(&self.data);
        o.finish()
    }
}

#[cfg(test)]
mod t {
    use super::*;

    fn img(w: u32, h: u32, s: u8) -> Image {
        let mut i = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                i.set(x, y, [(x * 30) as u8 ^ s, (y * 50) as u8, s, 200]);
            }
        }
        i
    }

    #[test]
    fn roundtrip_formats() {
        for f in [Fmt::Float { ch: 4, half: false }, Fmt::Float { ch: 4, half: true }, Fmt::Rgba8, Fmt::Bgra8, Fmt::Bc(Bc::Bc1), Fmt::Bc(Bc::Bc3), Fmt::Bc(Bc::Bc5), Fmt::Mask { bits: 16, m: [0xf800, 0x7e0, 0x1f, 0], lum: false }] {
            let base = img(8, 4, 3);
            let mips = base.clone().mips(9, false);
            let d = Dds::from_images(f, &[mips.clone()], false).unwrap();
            let p = Dds::parse(&d.write()).unwrap();
            assert_eq!(p, d, "{f:?}");
            assert_eq!(p.mips, 4);
            let back = p.image(0, 0).unwrap();
            assert_eq!((back.w, back.h), (8, 4));
            if matches!(f, Fmt::Rgba8 | Fmt::Bgra8 | Fmt::Float { ch: 4, .. }) {
                assert_eq!(back, base);
                assert_eq!(p.image(0, 3).unwrap(), mips[3]);
            }
        }
    }

    #[test]
    fn halves() {
        for (h, v) in [(0x3c00u16, 1.0f32), (0xc000, -2.0), (0x3555, 0.333_251_95), (0x0001, 5.960_464_5e-8), (0x7bff, 65504.0), (0, 0.0)] {
            assert_eq!(f16(h), v);
            assert_eq!(to_f16(v), h);
        }
        assert_eq!(to_f16(1e9), 0x7c00);
        assert!(f16(0x7e00).is_nan());
    }

    #[test]
    fn cube_and_array() {
        let faces: Vec<Vec<Image>> = (0..6).map(|s| vec![img(4, 4, s as u8 * 40)]).collect();
        let d = Dds::from_images(Fmt::Rgba8, &faces, true).unwrap();
        let p = Dds::parse(&d.write()).unwrap();
        assert!(p.cube && p.layers == 6);
        assert_eq!(p.image(5, 0).unwrap(), faces[5][0]);
        let arr: Vec<Vec<Image>> = (0..3).map(|s| vec![img(4, 4, s as u8)]).collect();
        let a = Dds::from_images(Fmt::Bc(Bc::Bc7), &arr, false);
        assert!(a.is_err());
        let a = Dds::parse(&Dds::from_images(Fmt::Bgrx8, &arr, false).unwrap().write()).unwrap();
        assert_eq!(a.layers, 3);
        assert!(Dds::parse(b"DDS ").is_err());
        assert!(a.surface(3, 0).is_none());
    }

    #[test]
    #[ignore]
    fn real_files() {
        let Ok(d) = std::env::var("AC_DDS_DIR") else { return };
        let (mut ok, mut skip) = (0, Vec::new());
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dds")) {
                continue;
            }
            match Dds::parse(&std::fs::read(&p).unwrap()) {
                Ok(x) => {
                    for m in 0..x.mips {
                        x.image(0, m).unwrap();
                    }
                    let i = x.image(0, 0).unwrap();
                    assert_eq!(Dds::parse(&x.write()).unwrap().image(0, 0).unwrap(), i);
                    ok += 1;
                }
                Err(e) => skip.push(format!("{}: {e}", p.file_name().unwrap().to_string_lossy())),
            }
        }
        println!("dds ok {ok}, unsupported {}: {:?}", skip.len(), &skip[..skip.len().min(10)]);
    }
}
