mod bits;
mod book;
mod dec;
mod enc;
mod mdct;

use crate::wav::Wav;
use ac_core::Res;

#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub channels: u16,
    pub rate: u32,
    pub vendor: String,
    pub comments: Vec<String>,
    pub samples: Vec<f32>,
}

pub fn decode(d: &[u8]) -> Res<Audio> {
    let (s, samples) = dec::decode(&crate::ogg::packets(d)?)?;
    Ok(Audio { channels: s.channels as u16, rate: s.rate, vendor: s.vendor, comments: s.comments, samples })
}

pub fn to_wav(d: &[u8]) -> Res<Wav> {
    let a = decode(d)?;
    Ok(Wav::new(a.channels, a.rate, a.samples.iter().map(|&x| (x.clamp(-1.0, 1.0) * 32767.0).round() as i16).collect()))
}

pub fn encode(a: &Audio, quality: f32) -> Vec<u8> {
    enc::encode(a.channels as usize, a.rate, &a.samples, quality, &a.comments)
}

pub fn from_wav(w: &Wav, quality: f32) -> Vec<u8> {
    let s: Vec<f32> = w.samples.iter().map(|&x| x as f32 / 32768.0).collect();
    enc::encode(w.channels as usize, w.rate, &s, quality, &[])
}

#[cfg(test)]
mod t {
    use super::*;

    fn synth(rate: u32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        let mut s = 0x1234_5678u32;
        (0..n).flat_map(|i| {
            let t = i as f32 / rate as f32;
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let noise = (s % 2001) as f32 / 1000.0 - 1.0;
            let click = if i % 11025 < 40 { 0.6 } else { 0.0 };
            let a = 0.3 * (t * 440.0 * std::f32::consts::TAU).sin() + 0.2 * (t * 1250.0 * std::f32::consts::TAU).sin() + 0.05 * noise + click;
            let b = 0.4 * (t * 220.0 * std::f32::consts::TAU).sin() + 0.1 * (t * 5000.0 * std::f32::consts::TAU).sin();
            [a, b]
        }).collect()
    }

    fn snr(a: &[f32], b: &[f32]) -> f64 {
        let (mut s, mut e) = (0f64, 0f64);
        for (&x, &y) in a.iter().zip(b) {
            s += (x as f64).powi(2);
            e += (x as f64 - y as f64).powi(2);
        }
        10.0 * (s / e.max(1e-20)).log10()
    }

    #[test]
    fn encode_roundtrip() {
        let src = synth(22050, 1.3);
        let a = Audio { channels: 2, rate: 22050, vendor: String::new(), comments: vec!["TITLE=test".into()], samples: src.clone() };
        let mut prev = 0;
        for q in [0.0f32, 0.5, 1.0] {
            let ogg = encode(&a, q);
            let b = decode(&ogg).unwrap();
            assert_eq!((b.channels, b.rate, b.samples.len(), b.vendor.as_str()), (2, 22050, src.len(), "ACLibriary"));
            assert_eq!(b.comments, ["TITLE=test"]);
            let db = snr(&src, &b.samples);
            println!("q {q}: {} bytes, snr {db:.1} dB", ogg.len());
            assert!(db > 10.0 + 16.0 * q as f64, "q {q} snr {db}");
            assert!(ogg.len() > prev);
            prev = ogg.len();
        }
        let w = Wav::new(1, 8000, (0..8000).map(|i| ((i as f32 * 0.07).sin() * 12000.0) as i16).collect());
        let back = to_wav(&from_wav(&w, 0.6)).unwrap();
        assert_eq!((back.channels, back.rate, back.samples.len()), (1, 8000, 8000));
        let sil = Audio { channels: 1, rate: 8000, vendor: String::new(), comments: vec![], samples: vec![0.0; 5000] };
        assert!(decode(&encode(&sil, 0.5)).unwrap().samples.iter().all(|&x| x == 0.0));
    }

    #[test]
    #[ignore]
    fn real_files() {
        let Ok(dir) = std::env::var("AC_OGG") else { return };
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap().flatten().take(std::env::var("AC_OGG_N").ok().and_then(|s| s.parse().ok()).unwrap_or(3)) {
            let d = std::fs::read(e.path()).unwrap();
            let t = std::time::Instant::now();
            let a = decode(&d).unwrap();
            let secs = a.samples.len() as f64 / a.channels as f64 / a.rate as f64;
            let peak = a.samples.iter().fold(0f32, |m, &x| m.max(x.abs()));
            let rms = (a.samples.iter().map(|&x| (x * x) as f64).sum::<f64>() / a.samples.len() as f64).sqrt();
            println!("{}: {} ch {} Hz {:.1}s peak {peak:.3} rms {rms:.3} vendor {:?} in {:?}", e.file_name().to_string_lossy(), a.channels, a.rate, secs, a.vendor, t.elapsed());
            assert!(secs > 10.0 && peak > 0.1 && peak < 1.5 && rms > 0.01);
            n += 1;
        }
        assert!(n > 0);
    }
}

