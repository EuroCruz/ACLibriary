use ac_core::{bad, Endian, Reader, Res, Writer};

#[derive(Clone, Debug, PartialEq)]
pub struct Wav {
    pub channels: u16,
    pub rate: u32,
    pub samples: Vec<i16>,
}

fn sample(b: &[u8], bits: u16, float: bool) -> i16 {
    match (bits, float) {
        (8, false) => ((b[0] as i16) - 128) << 8,
        (16, false) => i16::from_le_bytes([b[0], b[1]]),
        (24, false) => i16::from_le_bytes([b[1], b[2]]),
        (32, false) => i16::from_le_bytes([b[2], b[3]]),
        (32, true) => (f32::from_le_bytes([b[0], b[1], b[2], b[3]]).clamp(-1.0, 1.0) * 32767.0).round() as i16,
        (64, true) => (f64::from_le_bytes(b[..8].try_into().unwrap()).clamp(-1.0, 1.0) * 32767.0).round() as i16,
        _ => 0,
    }
}

impl Wav {
    pub fn new(channels: u16, rate: u32, samples: Vec<i16>) -> Wav {
        Wav { channels, rate, samples }
    }

    pub fn parse(d: &[u8]) -> Res<Wav> {
        let mut r = Reader::new(d, Endian::Le);
        r.magic(b"RIFF")?;
        r.skip(4)?;
        r.magic(b"WAVE")?;
        let mut fmt = None;
        while r.left() >= 8 {
            let id = r.arr::<4>()?;
            let n = r.get::<u32>()? as usize;
            let body = r.take(n.min(r.left()))?;
            if n % 2 == 1 && r.left() > 0 {
                r.skip(1)?;
            }
            match &id {
                b"fmt " => {
                    let mut f = Reader::new(body, Endian::Le);
                    let mut tag = f.get::<u16>()?;
                    let ch = f.get::<u16>()?;
                    let rate = f.get::<u32>()?;
                    f.skip(6)?;
                    let bits = f.get::<u16>()?;
                    if tag == 0xfffe {
                        f.skip(8)?;
                        tag = f.get::<u16>()?;
                    }
                    fmt = Some((tag, ch, rate, bits));
                }
                b"data" => {
                    let Some((tag, ch, rate, bits)) = fmt else { return bad("wav: data before fmt") };
                    let float = match tag {
                        1 => false,
                        3 => true,
                        _ => return bad("wav: unsupported encoding"),
                    };
                    if ch == 0 || !matches!((bits, float), (8 | 16 | 24 | 32, false) | (32 | 64, true)) {
                        return bad("wav: unsupported sample format");
                    }
                    let w = (bits / 8) as usize;
                    let frame = w * ch as usize;
                    let samples = body[..body.len() / frame * frame].chunks(w).map(|b| sample(b, bits, float)).collect();
                    return Ok(Wav { channels: ch, rate, samples });
                }
                _ => {}
            }
        }
        bad("wav: no data chunk")
    }

    pub fn write(&self) -> Vec<u8> {
        let n = self.samples.len() as u32 * 2;
        let mut w = Writer::new(Endian::Le);
        w.bytes(b"RIFF").put(36 + n).bytes(b"WAVEfmt ").put(16u32).put(1u16).put(self.channels).put(self.rate);
        w.put(self.rate * self.channels as u32 * 2).put(self.channels * 2).put(16u16).bytes(b"data").put(n);
        for s in &self.samples {
            w.put(*s);
        }
        w.finish()
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn secs(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }

    pub fn tone(hz: f32, secs: f32, rate: u32, amp: f32) -> Wav {
        let n = (secs * rate as f32) as usize;
        Wav::new(1, rate, (0..n).map(|i| ((i as f32 * hz * std::f32::consts::TAU / rate as f32).sin() * amp * 32767.0) as i16).collect())
    }
}

#[cfg(test)]
mod t {
    use super::*;

    fn riff(tag: u16, ch: u16, bits: u16, data: &[u8], ext: bool) -> Vec<u8> {
        let mut w = Writer::new(Endian::Le);
        let fl = if ext { 40u32 } else { 16 };
        w.bytes(b"RIFF").put(0u32).bytes(b"WAVE").bytes(b"LIST").put(3u32).bytes(b"abc\0").bytes(b"fmt ").put(fl);
        w.put(if ext { 0xfffe } else { tag }).put(ch).put(8000u32).put(8000 * ch as u32 * bits as u32 / 8).put(ch * bits / 8).put(bits);
        if ext {
            w.put(22u16).put(bits).put(3u32).put(tag).bytes(&[0; 14]);
        }
        w.bytes(b"data").put(data.len() as u32).bytes(data);
        w.finish()
    }

    #[test]
    fn formats() {
        let w = Wav::tone(440.0, 0.01, 8000, 0.5);
        assert_eq!((w.frames(), w.samples.iter().map(|s| s.abs()).max().unwrap() > 16000), (80, true));
        assert_eq!(Wav::parse(&w.write()).unwrap(), w);
        assert_eq!(Wav::parse(&riff(1, 1, 8, &[0, 128, 255], false)).unwrap().samples, [-32768, 0, 32512]);
        assert_eq!(Wav::parse(&riff(1, 2, 24, &[0, 0, 0x40, 0, 0, 0xc0], true)).unwrap(), Wav::new(2, 8000, vec![0x4000, -0x4000]));
        let f: Vec<u8> = [0.5f32, -1.5].iter().flat_map(|x| x.to_le_bytes()).collect();
        assert_eq!(Wav::parse(&riff(3, 1, 32, &f, false)).unwrap().samples, [16384, -32767]);
        let i: Vec<u8> = [0x12345678i32, -0x10000].iter().flat_map(|x| x.to_le_bytes()).collect();
        assert_eq!(Wav::parse(&riff(1, 1, 32, &i, true)).unwrap().samples, [0x1234, -1]);
        assert!(Wav::parse(&riff(2, 1, 4, &[0], false)).is_err());
        assert!(Wav::parse(&riff(1, 1, 12, &[0, 0], false)).is_err());
        assert!(Wav::parse(b"RIFF\0\0\0\0WAVE").is_err());
        assert!(Wav::parse(b"junk").is_err());
        assert!((Wav::new(2, 100, vec![0; 400]).secs() - 2.0).abs() < 1e-9);
    }
}
